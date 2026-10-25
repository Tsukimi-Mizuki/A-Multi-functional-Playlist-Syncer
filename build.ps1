#requires -Version 5.1
param(
    [ValidateSet('Debug','Release')][string]$Configuration = 'Debug',
    [switch]$Test,
    [switch]$Publish,
    [switch]$SelfContained
)
$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
$dotnet = Join-Path $root '.dotnet\dotnet.exe'
if (-not (Test-Path $dotnet)) { $dotnet = 'dotnet' }
$env:DOTNET_CLI_HOME = Join-Path $root '.dotnet-home'
$env:DOTNET_SKIP_FIRST_TIME_EXPERIENCE = '1'
$env:DOTNET_NOLOGO = '1'
$env:DOTNET_CLI_TELEMETRY_OPTOUT = '1'
# Keep temp/cache directories inside the workspace (the user profile temp can be ACL-restricted).
$env:TMP = Join-Path $root '.tmp'
$env:TEMP = $env:TMP
$env:NUGET_PACKAGES = Join-Path $root '.nuget-packages'
$env:NUGET_HTTP_CACHE_PATH = Join-Path $root '.nuget-http-cache'
$env:NUGET_PLUGINS_CACHE_PATH = Join-Path $root '.nuget-plugins-cache'
$env:MSBUILDDISABLENODEREUSE = '1'
$env:DOTNET_CLI_USE_MSBUILD_SERVER = '0'
New-Item -ItemType Directory -Force -Path $env:TMP, $env:NUGET_PACKAGES, $env:NUGET_HTTP_CACHE_PATH, $env:NUGET_PLUGINS_CACHE_PATH | Out-Null
$solution = Join-Path $root 'NeteaseMusicManager.sln'
& $dotnet restore $solution --configfile (Join-Path $root 'NuGet.Config') --packages $env:NUGET_PACKAGES
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
& $dotnet build $solution -c $Configuration --no-restore
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
if ($Test) {
    & $dotnet test (Join-Path $root 'tests\NeteaseMusicManager.Core.Tests\NeteaseMusicManager.Core.Tests.csproj') -c $Configuration --no-build
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
if ($Publish) {
    $out = Join-Path $root 'publish\win-x64'
    New-Item -ItemType Directory -Force -Path $out | Out-Null
    # Keep the user's portable session/settings, remove stale assemblies from previous publishes.
    Get-ChildItem $out -Force | Where-Object { $_.Name -ne 'data' } | Remove-Item -Recurse -Force
    $selfContainedValue = if ($SelfContained) { 'true' } else { 'false' }
    & $dotnet publish (Join-Path $root 'src\NeteaseMusicManager.Desktop\NeteaseMusicManager.Desktop.csproj') -c Release -r win-x64 --self-contained $selfContainedValue -p:PublishReadyToRun=false -p:DebugType=None -p:DebugSymbols=false -o $out
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    # Remove unused non-Windows/designer assemblies from a Windows-only package.
    foreach ($name in @('Avalonia.FreeDesktop.dll','Avalonia.Native.dll','Avalonia.X11.dll','Tmds.DBus.Protocol.dll','Avalonia.DesignerSupport.dll')) {
        $pathToRemove = Join-Path $out $name
        if (Test-Path $pathToRemove) { Remove-Item $pathToRemove -Force }
    }
    $zip = Join-Path $root 'publish\NeteaseMusicManager-win-x64.zip'
    if (Test-Path $zip) { Remove-Item $zip -Force }
    # Do not package the portable data dir: it can contain the user's encrypted login cookie/settings.
    # Keep it on disk between publishes so the app keeps its login state.
    $archiveItems = Get-ChildItem $out -Force | Where-Object { $_.Name -ne 'data' } | Select-Object -ExpandProperty FullName
    Compress-Archive -Path $archiveItems -DestinationPath $zip
    Write-Host "Published to $out"
    Write-Host "Zip: $zip"
}
