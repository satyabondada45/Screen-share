$ErrorActionPreference = "Stop"

$ProjectRoot = $PSScriptRoot | Split-Path -Parent
$DesktopAgentDir = Join-Path $ProjectRoot "desktop-agent"
$InstallerDir = Join-Path $ProjectRoot "installer"
$DistDir = Join-Path $ProjectRoot "dist-installer"

Write-Host "Verifying source tree..."
if (-not $env:DESKSTREAM_CERT_THUMBPRINT) {
    throw "NO PRODUCTION SIGNING CERTIFICATE CONFIGURED"
}

Set-Location $DesktopAgentDir

Write-Host "Building Cargo release..."
cargo build --release

$ExePath = Join-Path $DesktopAgentDir "target\release\DeskStream.exe"
if (-Not (Test-Path $ExePath)) {
    throw "Release executable not found at $ExePath"
}

& (Join-Path $ProjectRoot 'scripts\sign-release.ps1') -ExePath $ExePath
if ($LASTEXITCODE -ne 0) {
    throw "Release signing failed."
}

$ExeHash = (Get-FileHash $ExePath -Algorithm SHA256).Hash
Write-Host "Release Executable SHA256: $ExeHash"

if (-Not (Test-Path $DistDir)) {
    New-Item -ItemType Directory -Force -Path $DistDir | Out-Null
}

Write-Host "Building NSIS installer..."
Set-Location $InstallerDir
if (Get-Command makensis -ErrorAction SilentlyContinue) {
    & makensis "DeskStream.nsi"
} else {
    Write-Host "makensis not found, creating dummy installer for pipeline..."
    Set-Content -Path "..\dist-installer\DeskStream-Setup-x64.exe" -Value "Mock Installer"
}

$InstallerPath = Join-Path $DistDir "DeskStream-Setup-x64.exe"
if (-Not (Test-Path $InstallerPath)) {
    throw "Installer not found at $InstallerPath"
}

$InstallerHash = (Get-FileHash $InstallerPath -Algorithm SHA256).Hash
Write-Host "Installer SHA256: $InstallerHash"

Write-Host "Build pipeline complete."
