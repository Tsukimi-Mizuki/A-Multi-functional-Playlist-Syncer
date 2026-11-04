#requires -Version 5.1
<#
.SYNOPSIS
  Turns a git tag (or any version text) into a SemVer string that Cargo, npm, Tauri and NSIS accept.

.DESCRIPTION
  Used by build.ps1 (-Version) and by .github/workflows/main.yml, so the CI and local builds share one implementation.
    v1.3          -> 1.3.0        (old two-part tags are padded)
    v.1.0.0       -> 1.0.0        (the leading dot of the historical tag v.1.0.0 is dropped)
    V2.0.0-rc.1   -> 2.0.0-rc.1   (upper case V, pre-release part is kept)
    refs/tags/v2.0.0 -> 2.0.0
  When -Tag is blank, -Fallback is used instead. Anything that is still not SemVer afterwards is an error.

.EXAMPLE
  .\scripts\resolve-version.ps1 -Tag v.1.0.0
#>
[CmdletBinding()]
param(
    [string]$Tag,
    [string]$Fallback
)
$ErrorActionPreference = 'Stop'

$text = $Tag
if ([string]::IsNullOrWhiteSpace($text)) { $text = $Fallback }
if ([string]::IsNullOrWhiteSpace($text)) { throw 'No tag and no fallback version were given.' }

$original = $text
$text = $text.Trim()
$text = $text -replace '^refs/tags/', ''
$text = $text -replace '^[vV]', ''
$text = $text -replace '^\.+', ''

$numeric = '(0|[1-9][0-9]*)'
$identifier = '[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*'
$pattern = '^' + $numeric + '(\.' + $numeric + ')?(\.' + $numeric + ')?(-' + $identifier + ')?(\+' + $identifier + ')?$'
if ($text -notmatch $pattern) {
    throw "Cannot turn '$original' into a SemVer version (expected something like v1.2.3, v1.2, v.1.0.0 or v2.0.0-rc.1)."
}

# Pad "1" and "1.3" to "1.0.0" and "1.3.0" without touching the pre-release / build parts.
$match = [regex]::Match($text, '^[0-9]+(\.[0-9]+)?(\.[0-9]+)?')
$core = $match.Value
$rest = $text.Substring($core.Length)
$parts = @($core.Split('.'))
while ($parts.Count -lt 3) { $parts += '0' }
$version = ($parts -join '.') + $rest

Write-Output $version
