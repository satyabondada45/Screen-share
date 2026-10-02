param (
    [ValidateSet("Production", "Development")]
    [string]$SigningMode = "Production"
)

$ErrorActionPreference = "Stop"

$ProjectRoot = $PSScriptRoot | Split-Path -Parent
$DesktopAgentDir = Join-Path $ProjectRoot "desktop-agent"
$InstallerDir = Join-Path $ProjectRoot "installer"
$DistDir = Join-Path $ProjectRoot "dist-installer"
$ProductionSigner = Join-Path $ProjectRoot "scripts\sign-release.ps1"
$DevelopmentSigner = Join-Path $ProjectRoot "scripts\sign-windows.ps1"
$SignatureVerifier = Join-Path $ProjectRoot "scripts\verify-release-signature.ps1"

function Ensure-Nsis {
    $existing = Get-Command makensis -ErrorAction SilentlyContinue
    if ($existing) {
        return $existing.Source
    }

    function Get-CodeSigningCertificate([string]$Thumbprint) {
        foreach ($store in @('Cert:\CurrentUser\My', 'Cert:\LocalMachine\My')) {
            $certificate = Get-ChildItem $store -ErrorAction SilentlyContinue |
                Where-Object { $_.Thumbprint -eq $Thumbprint -and $_.HasPrivateKey } |
                Select-Object -First 1
            if ($certificate) {
                return $certificate
            }
        }
        return $null
    }

    Write-Host "Signing mode: $SigningMode"
    if ($SigningMode -eq "Production") {
        if (-not $env:DESKSTREAM_CERT_THUMBPRINT) {
            throw "Production signing requires DESKSTREAM_CERT_THUMBPRINT for a trusted code-signing certificate. Use -SigningMode Development only for local testing."
        }
        if (-not (Test-Path $ProductionSigner) -or -not (Test-Path $SignatureVerifier)) {
            throw "Production signing or verification script is missing."
        }

        $productionCert = Get-CodeSigningCertificate $env:DESKSTREAM_CERT_THUMBPRINT
        if (-not $productionCert -or $productionCert.NotBefore -gt (Get-Date) -or $productionCert.NotAfter -lt (Get-Date)) {
            throw "The configured production signing certificate is missing, has no private key, or is expired."
        }
        if ($productionCert.Subject -eq $productionCert.Issuer) {
            throw "A self-signed certificate cannot be used for Production signing."
        }

        $codeSigningEku = @($productionCert.EnhancedKeyUsageList | ForEach-Object { $_.ObjectId.Value })
        if ($codeSigningEku -notcontains '1.3.6.1.5.5.7.3.3') {
            throw "The configured production certificate does not have the Code Signing EKU."
        }

        $signTool = $env:DESKSTREAM_SIGNTOOL
        if (-not $signTool) {
            $signTool = Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\bin\*\x64\signtool.exe' -ErrorAction SilentlyContinue |
                Sort-Object FullName -Descending |
                Select-Object -First 1 -ExpandProperty FullName
        }
        if (-not $signTool -or -not (Test-Path $signTool)) {
            throw "signtool.exe was not found. Set DESKSTREAM_SIGNTOOL to its full path."
        }
        $env:DESKSTREAM_SIGNTOOL = $signTool
        $signingScript = $ProductionSigner
    } else {
        if (-not (Test-Path $DevelopmentSigner)) {
            throw "Development signing script is missing."
        }
        $signingScript = $DevelopmentSigner
        Write-Warning "Development signatures are self-signed and will NOT provide public Smart App Control trust."
    }

    function Sign-Binary([string]$Path) {
        Write-Host "Signing $Path..."
        & $signingScript -ExePath $Path
        if ($LASTEXITCODE -ne 0) {
            throw "Signing failed for $Path"
        }
    }

    function Verify-ProductionSignature([string]$Path) {
        & $SignatureVerifier -ExePath $Path
        if ($LASTEXITCODE -ne 0) {
            throw "Production signature verification failed for $Path"
        }
    }

    function Report-Signature([string]$Path) {
        $signature = Get-AuthenticodeSignature -FilePath $Path
        $certificate = $signature.SignerCertificate
        [pscustomobject]@{
            File = [IO.Path]::GetFileName($Path)
            Status = $signature.Status
            Publisher = if ($certificate) { $certificate.GetNameInfo([Security.Cryptography.X509Certificates.X509NameType]::SimpleName, $false) } else { "(none)" }
            Subject = if ($certificate) { $certificate.Subject } else { "(none)" }
            Issuer = if ($certificate) { $certificate.Issuer } else { "(none)" }
            Expires = if ($certificate) { $certificate.NotAfter.ToString("yyyy-MM-dd") } else { "(none)" }
        } | Format-List

        if (-not $certificate -or $signature.Status -notin @('Valid', 'UnknownError')) {
            throw "Binary has no verifiable Authenticode signature: $Path ($($signature.Status))"
        }
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

# Stop any running DeskStream process
Write-Host "Stopping any running DeskStream instances..."
Get-Process -Name "DeskStream" -ErrorAction SilentlyContinue | Stop-Process -Force
Get-Process -Name "DeskStream-Agent" -ErrorAction SilentlyContinue | Stop-Process -Force

$env:CARGO_TARGET_DIR = "C:\cargo-target\deskstream"

Write-Host "Building Cargo release..."
cargo build --release

$CargoExePath = "C:\cargo-target\deskstream\release\DeskStream.exe"
if (-Not (Test-Path $CargoExePath)) {
    throw "Release executable not found at $CargoExePath"
}

$FinalExeDir = Join-Path $ProjectRoot "DESKSTREAM"
if (-Not (Test-Path $FinalExeDir)) {
    New-Item -ItemType Directory -Force -Path $FinalExeDir | Out-Null
}

$ExePath = Join-Path $FinalExeDir "DeskStream.exe"
Copy-Item -Path $CargoExePath -Destination $ExePath -Force

$CargoExeHash = (Get-FileHash $CargoExePath -Algorithm SHA256).Hash
$ExeHash = (Get-FileHash $ExePath -Algorithm SHA256).Hash

Write-Host "Cargo Target Executable SHA256: $CargoExeHash"
Write-Host "Copied Final Executable SHA256: $ExeHash"

if ($CargoExeHash -ne $ExeHash) {
    throw "SHA256 mismatch: Copied EXE does not match the Cargo target EXE!"
}

Sign-Binary $ExePath
$ExeHash = (Get-FileHash $ExePath -Algorithm SHA256).Hash

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
& $makensisPath "/DDESKSTREAM_SIGNING_SCRIPT=$signingScript" "DeskStream.nsi"
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

Sign-Binary $InstallerPath

if ($SigningMode -eq "Production") {
    Verify-ProductionSignature $ExePath
    Verify-ProductionSignature $InstallerPath
} else {
    Report-Signature $ExePath
    Report-Signature $InstallerPath
}

$InstallerHash = (Get-FileHash $InstallerPath -Algorithm SHA256).Hash
Write-Host "Final Executable SHA256: $ExeHash"
Write-Host "Installer SHA256: $InstallerHash"

Write-Host "Build pipeline complete."
