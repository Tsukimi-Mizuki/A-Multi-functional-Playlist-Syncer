#requires -Version 5.1
<#
.SYNOPSIS
  Build, test and package Lunatic Princess (Rust + Tauri 2 + React, everything lives in app/).

.DESCRIPTION
  .\build.ps1                      pnpm install, then pnpm tauri build --no-bundle  -> app\target\release\LunaticPrincess.exe
  .\build.ps1 -Test                checks only: cargo fmt --check, clippy -D warnings, cargo test, pnpm build
  .\build.ps1 -Bundle              portable zip (exe + empty data\) + NSIS installer + SHA256SUMS.txt in dist-release\
  .\build.ps1 -Test -Bundle        checks first, then the bundle
  .\build.ps1 -Bundle -Version v2.1.0
                                   stamp this version (a tag such as v1.3 or v.1.0.0 is normalised to SemVer) into
                                   Cargo.toml / tauri.conf.json / package.json for the build and restore them afterwards

  -Tauri is the old switch from the rewrite branch. It is accepted and ignored: the Tauri build is now the default.

  Notes
  - Locally, TMP / TEMP point to .tmp\ inside the workspace. A repository inside a folder with a Low integrity label (for
    example Edge's download folder) makes test executables inherit that level, and they cannot write to the regular user
    temp. On GitHub Actions the runner's temp directory is used as is.
  - The script never touches publish\, .dotnet\ or any other local leftovers of the C# version (CSharp-GUI branch).
  - Messages are ASCII on purpose: Windows PowerShell 5.1 reads BOM-less script files as ANSI.
#>
[CmdletBinding()]
param(
    [switch]$Test,
    [switch]$Bundle,
    [string]$Version,
    [switch]$Tauri
)
$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
$appDir = Join-Path $root 'app'
$releaseDir = Join-Path $root 'dist-release'

if ($Tauri) { Write-Warning '-Tauri is obsolete and ignored: the Tauri build is the default now.' }

# Make cargo / pnpm findable in a fresh shell without dropping entries that CI added to the process PATH.
foreach ($tool in @('cargo', 'pnpm')) {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        $env:Path = $env:Path + ';' + [Environment]::GetEnvironmentVariable('Path', 'Machine') + ';' + [Environment]::GetEnvironmentVariable('Path', 'User')
        break
    }
}
foreach ($tool in @('cargo', 'pnpm')) {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) { throw "$tool was not found on PATH." }
}

# Not on GitHub Actions: there the runner's own temp directory is fine, and a workspace temp on another drive than
# %LOCALAPPDATA% would break tools that move files from temp to the Tauri tool cache.
if (-not $env:GITHUB_ACTIONS) {
    $env:TMP = Join-Path $root '.tmp'
    $env:TEMP = $env:TMP
    New-Item -ItemType Directory -Force -Path $env:TMP | Out-Null
}

function Invoke-Step {
    param([string]$Title, [scriptblock]$Command)
    if ($env:GITHUB_ACTIONS) { Write-Host "::group::$Title" } else { Write-Host "==> $Title" -ForegroundColor Cyan }
    # cargo / pnpm write progress to stderr; rely on the exit code instead of PowerShell error records.
    $ErrorActionPreference = 'Continue'
    $global:LASTEXITCODE = 0
    try {
        & $Command
        if ($LASTEXITCODE -ne 0) { throw "Step failed (exit code $LASTEXITCODE): $Title" }
    }
    finally {
        if ($env:GITHUB_ACTIONS) { Write-Host '::endgroup::' }
    }
}

# Reads the version that the manifests currently carry (tauri.conf.json is the reference).
function Get-ConfiguredVersion {
    $conf = Get-Content -LiteralPath (Join-Path $appDir 'src-tauri\tauri.conf.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    return [string]$conf.version
}

$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
# Replaces the first match of a pattern in a text file without touching anything else (line endings included).
function Set-FirstMatch {
    param([string]$Path, [string]$Pattern, [string]$Replacement)
    $text = [System.IO.File]::ReadAllText($Path, $utf8NoBom)
    $regex = New-Object System.Text.RegularExpressions.Regex($Pattern)
    if (-not $regex.IsMatch($text)) { throw "Version line not found in $Path" }
    [System.IO.File]::WriteAllText($Path, $regex.Replace($text, $Replacement, 1), $utf8NoBom)
}

$stamped = @{}
# Puts the stamped manifests back byte for byte. A virus scanner or indexer can hold a file for a moment ("user-mapped
# section open"), so retry for a few seconds before giving up, and never skip the remaining files.
function Restore-StampedFiles {
    foreach ($file in @($stamped.Keys)) {
        $restored = $false
        for ($attempt = 1; $attempt -le 40 -and -not $restored; $attempt++) {
            try { [System.IO.File]::WriteAllBytes($file, $stamped[$file]); $restored = $true }
            catch { Start-Sleep -Milliseconds 250 }
        }
        if (-not $restored) { Write-Warning "Could not restore $file - reset its version line by hand (git checkout -- <file> if it has no other edits)." }
    }
}
$failed = $false
try {
    Push-Location $appDir

    Invoke-Step 'pnpm install' {
        if (Test-Path 'pnpm-lock.yaml') { pnpm install --frozen-lockfile } else { pnpm install }
    }

    if ($Test) {
        Invoke-Step 'cargo fmt --check' { cargo fmt --check }
        Invoke-Step 'cargo clippy' { cargo clippy --workspace --all-targets --locked -- -D warnings }
        Invoke-Step 'cargo test' { cargo test --workspace --locked }
        Invoke-Step 'pnpm build (tsc + vite)' { pnpm build }
        Write-Host 'All checks passed.' -ForegroundColor Green
        if (-not $Bundle) { return }
    }

    if ($Version) {
        $Version = & (Join-Path $root 'scripts\resolve-version.ps1') -Tag $Version
        Write-Host "Stamping version $Version"
        $files = @('Cargo.toml', 'Cargo.lock', 'package.json', 'src-tauri\tauri.conf.json') | ForEach-Object { Join-Path $appDir $_ }
        foreach ($file in $files) { $stamped[$file] = [System.IO.File]::ReadAllBytes($file) }
        Set-FirstMatch (Join-Path $appDir 'Cargo.toml') '(?m)^(version\s*=\s*")[^"]+(")' ('${1}' + $Version + '${2}')
        Set-FirstMatch (Join-Path $appDir 'package.json') '(?m)^(\s*"version"\s*:\s*")[^"]+(")' ('${1}' + $Version + '${2}')
        Set-FirstMatch (Join-Path $appDir 'src-tauri\tauri.conf.json') '(?m)^(\s*"version"\s*:\s*")[^"]+(")' ('${1}' + $Version + '${2}')
    }
    else {
        $Version = Get-ConfiguredVersion
    }

    Invoke-Step 'pnpm tauri build --no-bundle' { pnpm tauri build --no-bundle }
    $exe = Join-Path $appDir 'target\release\LunaticPrincess.exe'
    if (-not (Test-Path -LiteralPath $exe)) { throw "Missing $exe" }
    if (-not $Bundle) {
        Write-Host "Built: $exe"
        return
    }

    New-Item -ItemType Directory -Force -Path $releaseDir | Out-Null
    Get-ChildItem -LiteralPath $releaseDir -Force | Where-Object { $_.Name -like 'LunaticPrincess-*' -or $_.Name -eq 'SHA256SUMS.txt' } | Remove-Item -Force

    # Asset names carry no spaces (GitHub turns them into dots). The zip holds the executable and an empty data\ folder,
    # which makes the extracted app keep its settings, login and downloads next to itself. The folder is only an empty
    # entry, so a real data directory (and its login cookie) can never end up in a package. The zip is made before the
    # NSIS step because bundling patches the exe in place (a "bundle type" marker for the updater) and the portable build
    # should stay pristine.
    $zipOut = Join-Path $releaseDir "LunaticPrincess-$Version-win-x64-portable.zip"
    # Entries are written by hand: Compress-Archive cannot add an empty folder, and ZipFile.CreateFromDirectory on
    # Windows PowerShell 5.1 names entries with backslashes, which the zip format does not allow.
    Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
    $zip = [System.IO.Compression.ZipFile]::Open($zipOut, [System.IO.Compression.ZipArchiveMode]::Create)
    try {
        [void][System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $exe, 'LunaticPrincess.exe', [System.IO.Compression.CompressionLevel]::Optimal)
        [void]$zip.CreateEntry('data/')
    }
    finally { $zip.Dispose() }

    $started = Get-Date
    Invoke-Step 'pnpm tauri bundle --bundles nsis' { pnpm tauri bundle --bundles nsis }
    $nsisDir = Join-Path $appDir 'target\release\bundle\nsis'
    $setup = Get-ChildItem -LiteralPath $nsisDir -Filter '*-setup.exe' -ErrorAction SilentlyContinue |
        Where-Object { $_.LastWriteTime -ge $started.AddSeconds(-5) } |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if (-not $setup) { throw "The NSIS installer was not produced in $nsisDir" }
    $setupOut = Join-Path $releaseDir "LunaticPrincess-$Version-win-x64-setup.exe"
    Copy-Item -LiteralPath $setup.FullName -Destination $setupOut

    $sums = foreach ($item in @($setupOut, $zipOut)) {
        $hash = (Get-FileHash -LiteralPath $item -Algorithm SHA256).Hash.ToLowerInvariant()
        "$hash  $(Split-Path -Leaf $item)"
    }
    [System.IO.File]::WriteAllText((Join-Path $releaseDir 'SHA256SUMS.txt'), (($sums -join "`n") + "`n"), $utf8NoBom)

    Write-Host "Bundle ($Version) written to $releaseDir" -ForegroundColor Green
    Get-ChildItem -LiteralPath $releaseDir | ForEach-Object { Write-Host ('  {0}  ({1:N1} MB)' -f $_.Name, ($_.Length / 1MB)) }
}
catch {
    Write-Host $_.Exception.Message -ForegroundColor Red
    $failed = $true
}
finally {
    # Put the manifests back exactly as they were (only differs when -Version was used).
    Restore-StampedFiles
    Pop-Location
}
if ($failed) { exit 1 }
