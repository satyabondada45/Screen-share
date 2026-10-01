$ErrorActionPreference = "Stop"

$ProjectRoot = $PSScriptRoot | Split-Path -Parent
$DesktopAgentDir = Join-Path $ProjectRoot "desktop-agent"
$InstallerDir = Join-Path $ProjectRoot "installer"
$DistDir = Join-Path $ProjectRoot "dist-installer"

function Ensure-Nsis {
    $existing = Get-Command makensis -ErrorAction SilentlyContinue
    if ($existing) {
        return $existing.Source
    }

    $winget = Get-Command winget -ErrorAction SilentlyContinue
    if ($winget) {
        Write-Host "NSIS not found; installing via winget..."
        & $winget install --id NSIS.NSIS -e --accept-source-agreements --accept-package-agreements | Out-Host
        $candidate = @(
            'C:\Program Files\NSIS\makensis.exe',
            'C:\Program Files (x86)\NSIS\makensis.exe'
        )
        foreach ($path in $candidate) {
            if (Test-Path $path) {
                $env:Path += ";$(Split-Path -Parent $path)"
                return $path
            }
        }
    }

    throw "makensis is required to build the real Windows installer. Install NSIS and try again."
}

Write-Host "Verifying source tree..."

Set-Location $DesktopAgentDir

Write-Host "Building Cargo release..."
cargo build --release

$ExePath = Join-Path $DesktopAgentDir "target\release\DeskStream.exe"
if (-Not (Test-Path $ExePath)) {
    throw "Release executable not found at $ExePath"
}

if ($env:DESKSTREAM_CERT_THUMBPRINT) {
    & (Join-Path $ProjectRoot 'scripts\sign-release.ps1') -ExePath $ExePath
    if ($LASTEXITCODE -ne 0) {
        throw "Release signing failed."
    }
} else {
    Write-Host "Skipping code signing because DESKSTREAM_CERT_THUMBPRINT is not configured."
}

$ExeHash = (Get-FileHash $ExePath -Algorithm SHA256).Hash
Write-Host "Release Executable SHA256: $ExeHash"

if (-Not (Test-Path $DistDir)) {
    New-Item -ItemType Directory -Force -Path $DistDir | Out-Null
}

$staleInstaller = Join-Path $DistDir "DeskStream-Setup-x64.exe"
if (Test-Path $staleInstaller) {
    Remove-Item $staleInstaller -Force
}

Write-Host "Building NSIS installer..."
$makensisPath = Ensure-Nsis
Set-Location $InstallerDir
& $makensisPath "DeskStream.nsi"
if ($LASTEXITCODE -ne 0) {
    throw "NSIS installer build failed."
}

$InstallerPath = Join-Path $DistDir "DeskStream-Setup-x64.exe"
if (-Not (Test-Path $InstallerPath)) {
    throw "Installer not found at $InstallerPath"
}

if ((Get-Item $InstallerPath).Length -lt 1024) {
    throw "The generated setup file is too small to be a real installer: $InstallerPath"
}

$InstallerHash = (Get-FileHash $InstallerPath -Algorithm SHA256).Hash
Write-Host "Installer SHA256: $InstallerHash"

Write-Host "Build pipeline complete."
