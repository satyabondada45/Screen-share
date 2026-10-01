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

$IconPath = Join-Path $DesktopAgentDir "assets\icon.ico"
$IconPng = Join-Path $DesktopAgentDir "assets\icon.png"
if (Test-Path $IconPng) {
    $iconValidate = @'
from PIL import Image
import sys
for path in [r"$IconPath", r"$IconPng"]:
    if path.lower().endswith(".ico"):
        try:
            with Image.open(path) as img:
                sizes = set(getattr(img, "info", {}).get("sizes", []))
                if not sizes:
                    sizes = {(img.size[0], img.size[1])}
                print(f"{path}: {sorted(sizes)}")
                if len(sizes) < 2:
                    raise SystemExit(2)
        except Exception:
            raise SystemExit(2)
'@
    $iconValidate = $iconValidate.Replace('$IconPath', $IconPath).Replace('$IconPng', $IconPng)
    $iconValid = python -c $iconValidate
    if ($LASTEXITCODE -ne 0 -or $iconValid -match 'ERROR') {
        Write-Host "DeskStream icon is invalid or single-size; regenerating a proper multi-resolution ICO from assets/icon.png..."
        python -c "from PIL import Image; src=Image.open(r'$IconPng').convert('RGBA'); sizes=[16,24,32,48,64,128,256]; frames=[src.resize((s,s), Image.LANCZOS) for s in sizes]; frames[0].save(r'$IconPath', format='ICO', sizes=[(s,s) for s in sizes], append_images=frames[1:])"
        if ($LASTEXITCODE -ne 0) { throw "Failed to regenerate the DeskStream ICO icon." }
    }
}

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
