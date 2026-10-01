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
    $iconScript = @"
from PIL import Image
from pathlib import Path
import struct
icon_path = Path(r'$IconPath')
png_path = Path(r'$IconPng')

def parse_ico_sizes(path: Path):
    try:
        with Image.open(path) as img:
            sizes = set(img.info.get('sizes', set()))
            if not sizes:
                sizes = {(img.size[0], img.size[1])}
            return sizes
    except Exception:
        return set()

sizes = parse_ico_sizes(icon_path)
if len(sizes) < 2:
    src = Image.open(png_path).convert('RGBA')
    sizes_list = [16, 24, 32, 48, 64, 128, 256]
    image_data = []
    entries = []
    for size in sizes_list:
        image = src.resize((size, size), Image.Resampling.LANCZOS).convert('RGBA')
        tmp_path = Path(r'C:\Temp\deskstream_ico_tmp.bmp')
        image.save(tmp_path, format='BMP')
        raw = tmp_path.read_bytes()
        dib = raw[14:]
        image_data.append(dib)
        entries.append((0 if size == 256 else size, 0 if size == 256 else size, 0, 0, 1, 32, len(dib), 0))
        tmp_path.unlink(missing_ok=True)

    header = struct.pack('<HHH', 0, 1, len(entries))
    payload = bytearray(header)
    offset = 6 + len(entries) * 16
    for w, h, color_count, reserved, planes, bit_count, image_size, _ in entries:
        payload += struct.pack('<BBBBHHII', w, h, color_count, reserved, planes, bit_count, image_size, offset)
        offset += image_size
    for dib in image_data:
        payload += dib
    icon_path.write_bytes(payload)
    print('regenerated')
else:
    print('valid')
    print(sorted(sizes))
"@

    $iconStatus = python -c $iconScript
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to validate or regenerate the DeskStream ICO icon."
    }

    $icoSizes = python -c @"
from PIL import Image
from pathlib import Path
import struct
p = Path(r'$IconPath')
try:
    with p.open('rb') as fh:
        data = fh.read()
    reserved, image_type, count = struct.unpack_from('<HHH', data, 0)
    sizes = []
    offset = 6
    for _ in range(count):
        width, height, color_count, reserved_value, planes, bitcount, image_size, image_offset = struct.unpack_from('<BBBBHHII', data, offset)
        if width == 0:
            width = 256
        if height == 0:
            height = 256
        sizes.append((width, height))
        offset += 16
    print(sorted(set(sizes)))
except Exception as exc:
    print(f'ERROR:{exc}')
"@
    if ($LASTEXITCODE -ne 0 -or $icoSizes -match 'ERROR' -or ($icoSizes -split ' ' | Where-Object { $_ -match '\d' }).Count -lt 2) {
        throw "DeskStream icon is not a valid multi-resolution Windows ICO."
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
