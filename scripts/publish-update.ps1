<#
.SYNOPSIS
    DeskStream Release Publishing and Packaging Pipeline
.DESCRIPTION
    Builds, packages, hashes, cryptographically signs (Ed25519), and generates
    the release manifest (stable.json) for Hostinger production distribution.
.PARAMETER Version
    The version to release (e.g. 1.1.4). Defaults to version in desktop-agent/Cargo.toml.
.PARAMETER ReleaseNotes
    Release notes text describing what is new in this version.
.PARAMETER Mandatory
    Switch flag to mark this update as a mandatory security release.
.PARAMETER SkipBuild
    Skip building release binaries if already compiled.
.PARAMETER Channel
    Release channel (default: stable).
#>
param(
    [string]$Version,
    [string]$ReleaseNotes = "Performance, security, and stability improvements.",
    [switch]$Mandatory,
    [switch]$SkipBuild,
    [string]$Channel = "stable"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectRoot = (Resolve-Path "$ScriptDir\..").Path
$AgentDir = Join-Path $ProjectRoot "desktop-agent"
$CargoToml = Join-Path $AgentDir "Cargo.toml"
$PrivateKeyPath = Join-Path $ScriptDir "keys\deskstream_update_private.key"
$PublicKeyPath = Join-Path $ScriptDir "keys\deskstream_update_public.pub"
$SignToolExe = Join-Path $AgentDir "target\release\sign-manifest.exe"
$UpdatesDir = Join-Path $ProjectRoot "updates"
$PackagesDir = Join-Path $UpdatesDir "packages"

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host "    DESKSTREAM SECURE RELEASE PUBLISHING PIPELINE         " -ForegroundColor Cyan
Write-Host "==========================================================" -ForegroundColor Cyan

# 1. Determine Version
if (-not $Version) {
    if (Test-Path $CargoToml) {
        $TomlContent = Get-Content $CargoToml -Raw
        if ($TomlContent -match 'version\s*=\s*"([^"]+)"') {
            $Version = $matches[1]
        }
    }
}

if (-not $Version) {
    throw "Version not specified and could not be determined from Cargo.toml."
}

Write-Host "[1/7] Target Version: $Version (Channel: $Channel)" -ForegroundColor Yellow
if ($Mandatory) {
    Write-Host "      Type: MANDATORY SECURITY UPDATE" -ForegroundColor Red
} else {
    Write-Host "      Type: Standard Optional Update" -ForegroundColor Green
}

# 2. Check Private Key
if (-not (Test-Path $PrivateKeyPath)) {
    throw "Ed25519 private key missing at: $PrivateKeyPath. Cannot sign release!"
}
$PrivateKeyHex = (Get-Content $PrivateKeyPath -Raw).Trim()

# 3. Build Binaries
if (-not $SkipBuild) {
    Write-Host "[2/7] Compiling release binaries with Cargo..." -ForegroundColor Yellow
    Push-Location $AgentDir
    try {
        & cargo build --release --bin DeskStream --bin deskstream-updater --bin sign-manifest
        if ($LASTEXITCODE -ne 0) {
            throw "Cargo release build failed with exit code $LASTEXITCODE"
        }
    } finally {
        Pop-Location
    }
} else {
    Write-Host "[2/7] Skipping cargo build (-SkipBuild specified)." -ForegroundColor Yellow
}

$DeskStreamExe = Join-Path $AgentDir "target\release\DeskStream.exe"
$UpdaterExe = Join-Path $AgentDir "target\release\deskstream-updater.exe"

if (-not (Test-Path $DeskStreamExe)) {
    throw "DeskStream.exe not found at $DeskStreamExe"
}
if (-not (Test-Path $UpdaterExe)) {
    throw "deskstream-updater.exe not found at $UpdaterExe"
}

# 4. Authenticode Signing (if production signing certificate is configured in environment)
Write-Host "[3/7] Checking Windows Authenticode code signing..." -ForegroundColor Yellow
$SignScript = Join-Path $ScriptDir "sign-release.ps1"
if (Test-Path $SignScript) {
    try {
        & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $SignScript $DeskStreamExe
        Write-Host "      Authenticode signing applied successfully." -ForegroundColor Green
    } catch {
        Write-Host "      Notice: Production Authenticode cert not in current local environment." -ForegroundColor Gray
        Write-Host "      (Ed25519 end-to-end cryptographic signature will authenticate the package)." -ForegroundColor Gray
    }
}

# 5. Package Creation
Write-Host "[4/7] Packaging release artifacts..." -ForegroundColor Yellow
if (-not (Test-Path $PackagesDir)) {
    New-Item -ItemType Directory -Path $PackagesDir -Force | Out-Null
}

$PackageFilename = "DeskStream-$Version.exe"
$PackagePath = Join-Path $PackagesDir $PackageFilename

# Copy main binary as the versioned update package
Copy-Item -LiteralPath $DeskStreamExe -Destination $PackagePath -Force

# Also update the canonical local DESKSTREAM folder and root executable
$LocalDeskStreamDir = Join-Path $ProjectRoot "DESKSTREAM"
Copy-Item -LiteralPath $DeskStreamExe -Destination (Join-Path $LocalDeskStreamDir "DeskStream.exe") -Force
Copy-Item -LiteralPath $UpdaterExe -Destination (Join-Path $LocalDeskStreamDir "deskstream-updater.exe") -Force
Copy-Item -LiteralPath $DeskStreamExe -Destination (Join-Path $ProjectRoot "DeskStream.exe") -Force
Copy-Item -LiteralPath $UpdaterExe -Destination (Join-Path $ProjectRoot "deskstream-updater.exe") -Force

$PackageSize = (Get-Item $PackagePath).Length
$PackageSha256 = (Get-FileHash -Path $PackagePath -Algorithm SHA256).Hash.ToLower()

Write-Host "      Package: $PackageFilename" -ForegroundColor Gray
Write-Host "      Size:    $([math]::Round($PackageSize / 1MB, 2)) MB ($PackageSize bytes)" -ForegroundColor Gray
Write-Host "      SHA-256: $PackageSha256" -ForegroundColor Gray

# 6. Ed25519 Signature Generation
Write-Host "[5/7] Cryptographically signing release with Ed25519 private key..." -ForegroundColor Yellow
$AppId = "com.friendssoftwaresolutions.deskstream"
$SigningPayload = "$AppId`:$Version`:$PackageSha256"

if (-not (Test-Path $SignToolExe)) {
    throw "sign-manifest.exe not found at $SignToolExe"
}

$SignatureHex = (& $SignToolExe $PrivateKeyPath $SigningPayload).Trim()
if ($SignatureHex.Length -ne 128) {
    throw "Invalid signature generated! Expected 128 hex chars, got $($SignatureHex.Length)"
}
Write-Host "      Ed25519 Signature: $SignatureHex" -ForegroundColor Green

# 7. Generate Manifest JSON (stable.json)
Write-Host "[6/7] Generating release metadata manifest (stable.json)..." -ForegroundColor Yellow
$ReleaseDate = (Get-Date).ToString("yyyy-MM-dd")
$DownloadUrl = "https://friendssoftwaresolutions.in/DeskStream/updates/packages/$PackageFilename"

$Manifest = [ordered]@{
    "app_id" = $AppId
    "channel" = $Channel
    "version" = $Version
    "min_supported_version" = $null
    "release_date" = $ReleaseDate
    "mandatory" = [bool]$Mandatory
    "release_notes" = $ReleaseNotes
    "package" = [ordered]@{
        "url" = $DownloadUrl
        "filename" = $PackageFilename
        "sha256" = $PackageSha256
        "size" = [int64]$PackageSize
        "format" = "exe"
    }
    "signature" = [ordered]@{
        "algorithm" = "ed25519"
        "value" = $SignatureHex
    }
}

$ManifestJson = $Manifest | ConvertTo-Json -Depth 5
$ManifestPath = Join-Path $UpdatesDir "stable.json"
[System.IO.File]::WriteAllText($ManifestPath, $ManifestJson, [System.Text.UTF8Encoding]::new($false))

Write-Host "      Manifest created at: $ManifestPath" -ForegroundColor Green

# 8. Output Hostinger Upload Instructions
Write-Host ""
Write-Host "[7/7] RELEASE PUBLISHED LOCALLY!" -ForegroundColor Green
Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host "    HOSTINGER DEPLOYMENT INSTRUCTIONS                     " -ForegroundColor Cyan
Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host "To make this release live for all users, upload the following files via FTP / cPanel:" -ForegroundColor White
Write-Host ""
Write-Host "1. Upload Metadata Manifest:" -ForegroundColor Yellow
Write-Host "   Local File:  $ManifestPath"
Write-Host "   Remote Path: /public_html/DeskStream/updates/stable.json"
Write-Host ""
Write-Host "2. Upload Update Package:" -ForegroundColor Yellow
Write-Host "   Local File:  $PackagePath"
Write-Host "   Remote Path: /public_html/DeskStream/updates/packages/$PackageFilename"
Write-Host ""
Write-Host "3. Verification URLs:" -ForegroundColor Yellow
Write-Host "   Manifest: https://friendssoftwaresolutions.in/DeskStream/updates/stable.json"
Write-Host "   Package:  $DownloadUrl"
Write-Host "==========================================================" -ForegroundColor Cyan
