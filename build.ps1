#requires -Version 7.0
param(
    [ValidateSet('Debug','Release')][string]$Configuration = 'Debug',
    [switch]$Test,
    [switch]$Publish
)
$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
$dotnet = Join-Path $root '.dotnet\dotnet.exe'
if (-not (Test-Path $dotnet)) { $dotnet = 'dotnet' }
$env:DOTNET_CLI_HOME = Join-Path $root '.dotnet-home'
$env:DOTNET_SKIP_FIRST_TIME_EXPERIENCE = '1'
$env:DOTNET_NOLOGO = '1'
$env:DOTNET_CLI_TELEMETRY_OPTOUT = '1'
# 本机用户目录下的临时目录存在 ACL 限制，统一把临时/缓存目录放到工作区。
$env:TMP = Join-Path $root '.tmp'
$env:TEMP = $env:TMP
$env:NUGET_PACKAGES = Join-Path $root '.nuget-packages'
$env:NUGET_HTTP_CACHE_PATH = Join-Path $root '.nuget-http-cache'
$env:NUGET_PLUGINS_CACHE_PATH = Join-Path $root '.nuget-plugins-cache'
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
    & $dotnet publish (Join-Path $root 'src\NeteaseMusicManager.Desktop\NeteaseMusicManager.Desktop.csproj') -c Release -r win-x64 --self-contained false -o $out
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    Write-Host "Published to $out"
}
