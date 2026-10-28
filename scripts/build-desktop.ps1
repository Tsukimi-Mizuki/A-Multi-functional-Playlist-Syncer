#requires -Version 5.1
<#
.SYNOPSIS
  Build the Tauri desktop shell (Vue frontend + .NET sidecar).
.DESCRIPTION
  1. Ensures the Rust toolchain exists (ASCII path D:\lpp-rust, rustup mirror rsproxy).
  2. Publishes LunaticPrincess.Server as a self-contained single file sidecar.
  3. Copies it into desktop/src-tauri/binaries with the Rust target triple suffix.
  4. Builds the Vue bundle and runs cargo build for the Tauri shell.
  5. Copies the produced exe back into publish\desktop-<profile>.
.NOTES
  The workspace path contains CJK characters which the stock mingw ld cannot open.
  RUSTFLAGS therefore points rustc at rust-lld (wide-char safe). A short ASCII
  junction D:\lpp -> workspace root keeps the cargo command lines readable.
#>
param(
    [switch]$SkipInstall,
    [switch]$SkipServer,
    [switch]$SkipFrontend,
    [switch]$SkipRust,
    [switch]$Bundle,
    [string]$Configuration = 'Release'
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$dotnet = Join-Path $root '.dotnet\dotnet.exe'
$nodeExe = 'C:\Users\TsukimiMizuki\.dsh\dsh-runtimes\dsh-primary-runtime\dependencies\node\bin\node.exe'
$pnpmJs = 'C:\Users\TsukimiMizuki\.dsh\dsh-runtimes\dsh-primary-runtime\dependencies\pnpm\bin\pnpm.mjs'
if (-not (Test-Path $nodeExe)) { $nodeExe = 'node' }
if (-not (Test-Path $pnpmJs)) { $pnpmJs = 'pnpm' }

function Write-Step([string]$message) { Write-Host "==> $message" -ForegroundColor Cyan }

# Rust environment: homes live at ASCII paths, target dir stays inside the workspace.
$rustHome = 'D:\lpp-rust\rustup'
$asciiRoot = 'D:\lpp'
if (-not (Test-Path $asciiRoot)) {
    Write-Step "creating ASCII junction $asciiRoot -> $root"
    New-Item -ItemType Junction -Path $asciiRoot -Target $root | Out-Null
}
$cargoHome = Join-Path $asciiRoot '.cargo-home'
$targetDir = Join-Path $asciiRoot '.cargo-target'
$env:RUSTUP_HOME = $rustHome
$env:CARGO_HOME = $cargoHome
$env:CARGO_TARGET_DIR = $targetDir
$env:PATH = "$cargoHome\bin;$env:PATH"

$env:DOTNET_CLI_HOME = Join-Path $root '.dotnet-home'
$env:TMP = Join-Path $asciiRoot '.tmp'
$env:TEMP = $env:TMP
$env:TMPDIR = $env:TMP
$env:NUGET_HTTP_CACHE_PATH = Join-Path $root '.nuget-http-cache'
$env:NUGET_PLUGINS_CACHE_PATH = Join-Path $root '.nuget-plugins-cache'
$env:NUGET_PACKAGES = Join-Path $root '.nuget-packages'

$cargo = Join-Path $cargoHome 'bin\cargo.exe'
if (-not (Test-Path $cargo)) {
    Write-Step 'installing rustup + stable toolchain (rsproxy mirror)'
    $rustupInit = Join-Path $asciiRoot '.tmp\rustup-init.exe'
    if (-not (Test-Path $rustupInit)) {
        Invoke-WebRequest -Uri 'https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-gnu/rustup-init.exe' -OutFile $rustupInit -UseBasicParsing
    }
    $env:RUSTUP_INIT_SKIP_PATH_CHECK = 'yes'
    $env:RUSTUP_DIST_SERVER = 'https://rsproxy.cn'
    $env:RUSTUP_UPDATE_ROOT = 'https://rsproxy.cn/rustup'
    & $rustupInit -y --default-host x86_64-pc-windows-gnu --default-toolchain stable --profile minimal --no-modify-path
    if (-not (Test-Path $cargo)) { throw 'rustup install failed' }
}

$targetTriple = 'x86_64-pc-windows-gnu'
$rustc = Join-Path $cargoHome 'bin\rustc.exe'
if (Test-Path $rustc) {
    $hostLine = (& $rustc -vV 2>$null | Select-String '^host:')
    if ($hostLine) { $targetTriple = $hostLine.ToString().Split(' ')[1].Trim() }
}
$rustLld = Join-Path $rustHome "toolchains\stable-$targetTriple\lib\rustlib\$targetTriple\bin\rust-lld.exe"
if (Test-Path $rustLld) {
    $env:RUSTFLAGS = "$env:RUSTFLAGS -C linker=$rustLld".Trim()
}
Write-Step "rust target: $targetTriple (lld: $(Test-Path $rustLld))"

# windres (mingw) cannot open paths containing CJK characters, and this checkout's path is CJK.
# The D:\lpp junction does not help: tauri-winres resolves it back to the real CJK path.
# So stage the icons in a plain ASCII directory and override bundle.icon through TAURI_CONFIG.
$iconSource = Join-Path $root 'desktop\src-tauri\icons'
if (Test-Path $iconSource) {
    $iconTarget = $null
    foreach ($candidate in @('D:\lpp-icons', 'C:\lpp-icons')) {
        if ($candidate -notmatch '^[\x20-\x7E]+$') { continue }
        try {
            New-Item -ItemType Directory -Force -Path $candidate | Out-Null
            $iconTarget = $candidate
            break
        } catch { }
    }
    if ($iconTarget) {
        Copy-Item (Join-Path $iconSource '*.ico') $iconTarget -Force -ErrorAction SilentlyContinue
        Copy-Item (Join-Path $iconSource '*.png') $iconTarget -Force -ErrorAction SilentlyContinue
        $icons = @(Get-ChildItem $iconTarget -File | Sort-Object Name | ForEach-Object { $_.FullName -replace '\\', '/' })
        if ($icons.Count -gt 0) {
            $env:TAURI_CONFIG = (@{ bundle = @{ icon = $icons } } | ConvertTo-Json -Compress -Depth 5)
            Write-Step "icons staged at $iconTarget (windres cannot open non-ASCII paths)"
        }
    } else {
        Write-Warning 'no ASCII staging directory available; windres may fail on the icon path'
    }
}

if (-not $SkipServer) {
    Write-Step 'publishing sidecar (LunaticPrincess.Server)'
    $serverOut = Join-Path $asciiRoot '.tmp\server-publish'
    if (Test-Path $serverOut) { Remove-Item $serverOut -Recurse -Force }
    & $dotnet publish (Join-Path $root 'src\LunaticPrincess.Server\LunaticPrincess.Server.csproj') -c $Configuration -r win-x64 --self-contained true -p:PublishSingleFile=true -p:DebugType=none -o $serverOut
    if ($LASTEXITCODE -ne 0) { throw 'sidecar publish failed' }

    $binDir = Join-Path $root 'desktop\src-tauri\binaries'
    New-Item -ItemType Directory -Force -Path $binDir | Out-Null
    Get-ChildItem $binDir -Filter 'lunatic-princess-server*' -ErrorAction SilentlyContinue | Remove-Item -Force
    $exe = Join-Path $serverOut 'LunaticPrincess.Server.exe'
    if (-not (Test-Path $exe)) { throw "sidecar exe not found at $exe" }
    Copy-Item $exe (Join-Path $binDir "lunatic-princess-server-$targetTriple.exe") -Force
    Write-Step "sidecar copied: lunatic-princess-server-$targetTriple.exe"
}

$desktop = Join-Path $root 'desktop'
if (-not $SkipInstall) {
    Write-Step 'installing npm dependencies'
    Push-Location $desktop
    try {
        & $nodeExe $pnpmJs install
        if ($LASTEXITCODE -ne 0) { throw 'pnpm install failed' }
    } finally { Pop-Location }
}

if (-not $SkipFrontend) {
    Write-Step 'building Vue frontend'
    Push-Location $desktop
    try {
        & $nodeExe $pnpmJs run build
        if ($LASTEXITCODE -ne 0) { throw 'frontend build failed' }
    } finally { Pop-Location }
}

if (-not $SkipRust) {
    Write-Step "cargo build ($Configuration)"
    Push-Location (Join-Path $asciiRoot 'desktop\src-tauri')
    try {
        $cargoArgs = @('build')
        if ($Configuration -eq 'Release') { $cargoArgs += '--release' }
        & $cargo @cargoArgs
        if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }

        if ($Bundle) {
            Write-Step 'tauri build (installer)'
            & $nodeExe $pnpmJs tauri build
            if ($LASTEXITCODE -ne 0) { throw 'tauri build failed' }
        }
    } finally { Pop-Location }

    $profileDir = if ($Configuration -eq 'Release') { 'release' } else { 'debug' }
    $built = Join-Path $targetDir "$profileDir\lunatic-princess.exe"
    if (-not (Test-Path $built)) { $built = Join-Path $targetDir "$profileDir\Lunatic Princess.exe" }
    $deliverDir = Join-Path $root "publish\desktop-$profileDir"
    New-Item -ItemType Directory -Force -Path $deliverDir | Out-Null
    Get-ChildItem $deliverDir -Filter '*.exe' -ErrorAction SilentlyContinue | Remove-Item -Force
    Get-ChildItem $deliverDir -Filter '*.dll' -ErrorAction SilentlyContinue | Remove-Item -Force
    if (Test-Path $built) {
        Copy-Item $built $deliverDir -Force

        # WebView2Loader.dll sits next to the exe in the cargo output dir; without it
        # the portable build dies instantly with 0xC0000135 STATUS_DLL_NOT_FOUND.
        $loader = Join-Path $targetDir "$profileDir\WebView2Loader.dll"
        if (Test-Path $loader) {
            Copy-Item $loader $deliverDir -Force
        } else {
            Write-Warning "WebView2Loader.dll not found under $targetDir\$profileDir"
        }

        # Keep the sidecar next to the shell so the portable folder runs as-is.
        $sidecar = Join-Path $targetDir "$profileDir\lunatic-princess-server.exe"
        if (Test-Path $sidecar) { Copy-Item $sidecar $deliverDir -Force }

        Write-Step "built: $deliverDir"
    } else {
        Write-Warning "expected exe not found under $targetDir\$profileDir"
    }
}