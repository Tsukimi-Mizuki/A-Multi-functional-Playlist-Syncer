#Requires -Version 5.1
<#
Builds the Flutter desktop app and produces a portable zip in dist-release.

The Flutter runner keeps its own runtime in runtime\ next to the exe
(see app/flutter/windows/CMakeLists.txt), while data\ stays reserved for
lp-core: the zip ships a data\ folder with a readme, so the app starts in
portable mode and keeps settings, the login session and downloads under data\.

Keep this file ASCII only: PowerShell 5.1 reads BOM-less scripts with the
ANSI code page.
#>
[CmdletBinding()]
param(
    # Overrides the version used in the artifact name; defaults to pubspec.yaml.
    [string]$Version = '',
    # Where the zip is written; defaults to dist-release in the repository root.
    [string]$OutputDir = ''
)

$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSScriptRoot
$flutterDir = Join-Path $repo 'app\flutter'
$pubspec = Join-Path $flutterDir 'pubspec.yaml'

function Resolve-Version {
    if (-not [string]::IsNullOrWhiteSpace($Version)) {
        return $Version.Trim().TrimStart('v', 'V')
    }
    $line = Select-String -Path $pubspec -Pattern '^version:\s*(\S+)' | Select-Object -First 1
    if (-not $line) { throw 'Could not read the version from app\flutter\pubspec.yaml.' }
    # pubspec uses "3.0.0+1" (build number) while artifacts only need the semver part.
    return ($line.Matches[0].Groups[1].Value -split '\+')[0]
}

function Resolve-Flutter {
    $command = Get-Command flutter -ErrorAction SilentlyContinue
    if ($command) { return $command.Source }
    if ($env:FLUTTER_ROOT) {
        $candidate = Join-Path $env:FLUTTER_ROOT 'bin\flutter.bat'
        if (Test-Path $candidate) { return $candidate }
    }
    throw 'flutter was not found. Add the Flutter SDK bin folder to PATH or set FLUTTER_ROOT.'
}

$resolvedVersion = Resolve-Version
$flutter = Resolve-Flutter
Write-Host "Version: $resolvedVersion"
Write-Host "Flutter: $flutter"

Push-Location $flutterDir
try {
    # pub get is explicit so a stale .dart_tool fails loudly instead of quietly.
    & $flutter pub get
    if ($LASTEXITCODE -ne 0) { throw 'flutter pub get failed.' }

    & $flutter build windows --release --no-pub
    if ($LASTEXITCODE -ne 0) { throw 'flutter build windows failed.' }
}
finally {
    Pop-Location
}

$release = Join-Path $flutterDir 'build\windows\x64\runner\Release'
$exe = Join-Path $release 'lunatic_princess.exe'
if (-not (Test-Path $exe)) { throw "The build produced no executable: $exe" }

# Stage a clean copy: the zip should contain only what the app needs at runtime.
$stage = Join-Path $flutterDir 'build\portable'
if (Test-Path $stage) { Remove-Item -Path $stage -Recurse -Force }
New-Item -ItemType Directory -Path $stage | Out-Null
Copy-Item -Path (Join-Path $release '*') -Destination $stage -Recurse -Force

$dataDir = Join-Path $stage 'data'
New-Item -ItemType Directory -Path $dataDir -Force | Out-Null
# A non-empty folder survives zipping and explains what data\ is for.
$note = @(
    'Lunatic Princess keeps its data in this folder.',
    '',
    'settings.json, the login session and the Music folder live here, and the',
    'app runs in portable mode because this folder sits next to the exe.',
    'Delete the folder to start over (this also removes the saved login).'
) -join "`r`n"
Set-Content -Path (Join-Path $dataDir 'README.txt') -Value $note -Encoding ASCII

$dist = if ([string]::IsNullOrWhiteSpace($OutputDir)) { Join-Path $repo 'dist-release' } else { $OutputDir }
New-Item -ItemType Directory -Path $dist -Force | Out-Null

$zipName = "LunaticPrincess-$resolvedVersion-win-x64-flutter-portable.zip"
$zip = Join-Path $dist $zipName
if (Test-Path $zip) { Remove-Item -Path $zip -Force }
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip -CompressionLevel Optimal

$hash = (Get-FileHash -Algorithm SHA256 -Path $zip).Hash.ToLowerInvariant()
$sizeMb = [math]::Round((Get-Item $zip).Length / 1MB, 1)
$sumFile = Join-Path $dist 'SHA256SUMS-flutter.txt'
Set-Content -Path $sumFile -Value "$hash  $zipName" -Encoding ASCII

Write-Host "Portable package: $zip ($sizeMb MB)"
Write-Host "SHA256: $hash"
