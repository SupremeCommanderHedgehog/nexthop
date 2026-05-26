#Requires -Version 7
<#
.SYNOPSIS
    Bumps the nexthop version across all manifests.

.DESCRIPTION
    Updates package.json, src-tauri/Cargo.toml, and src-tauri/tauri.conf.json
    to the supplied version, then refreshes Cargo.lock and package-lock.json
    so the lockfiles stay in sync.

    Run from the repo root.

.PARAMETER Version
    The new SemVer version, e.g. "0.2.1" or "0.3.0-rc.1".

.EXAMPLE
    ./scripts/bump-version.ps1 0.2.1
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$Version
)

$ErrorActionPreference = 'Stop'

$SemverPattern = '^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$'
if ($Version -notmatch $SemverPattern) {
    throw "Not a valid SemVer string: '$Version' (expected MAJOR.MINOR.PATCH[-PRERELEASE][+BUILD])"
}

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$PackageJson    = Join-Path $RepoRoot 'package.json'
$CargoToml      = Join-Path $RepoRoot 'src-tauri/Cargo.toml'
$TauriConfJson  = Join-Path $RepoRoot 'src-tauri/tauri.conf.json'

foreach ($f in @($PackageJson, $CargoToml, $TauriConfJson)) {
    if (-not (Test-Path $f)) { throw "Missing manifest: $f" }
}

function Update-FirstMatch {
    param(
        [string]$Path,
        [string]$Pattern,
        [string]$Replacement,
        [string]$Label
    )
    $content = Get-Content -Raw -LiteralPath $Path
    if ($content -notmatch $Pattern) {
        throw "Could not find $Label in $Path (pattern: $Pattern)"
    }
    $new = [regex]::Replace($content, $Pattern, $Replacement, 1)
    if ($new -eq $content) {
        Write-Host "  $Label : already at target (no change)"
    } else {
        Set-Content -LiteralPath $Path -Value $new -NoNewline
        Write-Host "  $Label : updated"
    }
}

Write-Host "Bumping nexthop to $Version"
Write-Host ""

Write-Host "package.json"
Update-FirstMatch -Path $PackageJson `
    -Pattern '("version"\s*:\s*")[^"]+(")' `
    -Replacement "`${1}$Version`${2}" `
    -Label 'version'

Write-Host "src-tauri/Cargo.toml"
Update-FirstMatch -Path $CargoToml `
    -Pattern '(?m)^(version\s*=\s*")[^"]+(")' `
    -Replacement "`${1}$Version`${2}" `
    -Label 'version'

Write-Host "src-tauri/tauri.conf.json"
Update-FirstMatch -Path $TauriConfJson `
    -Pattern '("version"\s*:\s*")[^"]+(")' `
    -Replacement "`${1}$Version`${2}" `
    -Label 'version'

Write-Host ""
Write-Host "Refreshing Cargo.lock"
Push-Location $RepoRoot
try {
    # For workspace members, `cargo update -p <crate>` refreshes the lockfile
    # entry against the new manifest version (no registry lookup involved).
    & cargo update -p nexthop 2>&1 | ForEach-Object { "  $_" }
    if ($LASTEXITCODE -ne 0) { throw "cargo update failed with exit code $LASTEXITCODE" }
} finally {
    Pop-Location
}

Write-Host ""
Write-Host "Refreshing package-lock.json"
Push-Location $RepoRoot
try {
    & npm install --package-lock-only 2>&1 | ForEach-Object { "  $_" }
    if ($LASTEXITCODE -ne 0) { throw "npm install failed with exit code $LASTEXITCODE" }
} finally {
    Pop-Location
}

Write-Host ""
Write-Host "Done. Next steps:"
Write-Host "  1. Update CHANGELOG.md (move [Unreleased] entries to [$Version] with today's date)"
Write-Host "  2. git add -A && git commit -S -m `"Release $Version`""
Write-Host "  3. git tag -s v$Version -m `"Release $Version`""
Write-Host "  4. git push origin master --tags"
Write-Host "  5. gh release create v$Version --notes-file <(awk ...)"
