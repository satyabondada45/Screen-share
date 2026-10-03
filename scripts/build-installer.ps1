param (
    [ValidateSet("Production", "Development")]
    [string]$SigningMode = "Production"
)

$ErrorActionPreference = "Stop"

if ($SigningMode -eq "Production" -and [string]::IsNullOrWhiteSpace($env:DESKSTREAM_CERT_THUMBPRINT)) {
    Write-Warning "DESKSTREAM_CERT_THUMBPRINT is not configured; falling back to an unsigned Development build."
    $SigningMode = "Development"
}

$ProjectRoot = $PSScriptRoot | Split-Path -Parent
$DesktopAgentDir = Join-Path $ProjectRoot "desktop-agent"
$InstallerDir = Join-Path $ProjectRoot "installer"
$DistDir = Join-Path $ProjectRoot "dist-installer"
$CargoTargetDir = Join-Path $DesktopAgentDir "target"
$CargoExePath = Join-Path $CargoTargetDir "release\DeskStream.exe"
$FinalExeDir = Join-Path $ProjectRoot "DESKSTREAM"
$ExePath = Join-Path $FinalExeDir "DeskStream.exe"
$PayloadVerifier = Join-Path $PSScriptRoot "verify-packaged-exe.ps1"
$ProductionSigner = Join-Path $ProjectRoot "scripts\sign-release.ps1"
$DevelopmentSigner = Join-Path $ProjectRoot "scripts\sign-windows.ps1"
$SignatureVerifier = Join-Path $ProjectRoot "scripts\verify-release-signature.ps1"

if ($SigningMode -eq "Production") {
    $staleProductionSetup = Join-Path $DistDir "DeskStream-Setup-x64.exe"
    if (Test-Path -LiteralPath $staleProductionSetup) {
        Remove-Item -LiteralPath $staleProductionSetup -Force
        if (Test-Path -LiteralPath $staleProductionSetup) {
            throw "Could not remove stale production Setup artifact: $staleProductionSetup"
        }
    }
}

function Ensure-Nsis {
    $existing = Get-Command makensis -ErrorAction SilentlyContinue
    if ($existing) {
        return $existing.Source
    }

    $candidate = @(
        'C:\Program Files\NSIS\makensis.exe',
        'C:\Program Files (x86)\NSIS\makensis.exe'
    )
    foreach ($path in $candidate) {
        if (Test-Path -LiteralPath $path) {
            $env:Path += ";$(Split-Path -Parent $path)"
            return $path
        }
    }

    $winget = Get-Command winget -ErrorAction SilentlyContinue
    if ($winget) {
        Write-Host "NSIS not found; installing via winget..."
        & $winget install --id NSIS.NSIS -e --accept-source-agreements --accept-package-agreements | Out-Host
        foreach ($path in $candidate) {
            if (Test-Path -LiteralPath $path) {
                $env:Path += ";$(Split-Path -Parent $path)"
                return $path
            }
        }
    }

    throw "makensis is required to build the real Windows installer. Install NSIS and try again."
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
            throw "Production signing unavailable: DESKSTREAM_CERT_THUMBPRINT is not configured with a trusted code-signing certificate. No production Setup will be created."
        }
        if (-not (Test-Path $ProductionSigner) -or -not (Test-Path $SignatureVerifier)) {
            throw "Production signing or verification script is missing."
        }

        $productionCert = Get-CodeSigningCertificate $env:DESKSTREAM_CERT_THUMBPRINT
        if (-not $productionCert -or $productionCert.NotBefore -gt (Get-Date) -or $productionCert.NotAfter -lt (Get-Date)) {
            throw "Production signing unavailable: the configured certificate is missing, has no private key, or is expired."
        }
        if ($productionCert.Subject -eq $productionCert.Issuer) {
            throw "A self-signed certificate cannot be used for Production signing."
        }

        $codeSigningEku = @($productionCert.EnhancedKeyUsageList | ForEach-Object { $_.ObjectId.Value })
        if ($codeSigningEku -notcontains '1.3.6.1.5.5.7.3.3') {
            throw "The configured production certificate does not have the Code Signing EKU."
        }

        $chain = New-Object System.Security.Cryptography.X509Certificates.X509Chain
        try {
            $chain.ChainPolicy.RevocationMode = [System.Security.Cryptography.X509Certificates.X509RevocationMode]::Online
            $chain.ChainPolicy.RevocationFlag = [System.Security.Cryptography.X509Certificates.X509RevocationFlag]::ExcludeRoot
            $chain.ChainPolicy.VerificationFlags = [System.Security.Cryptography.X509Certificates.X509VerificationFlags]::NoFlag
            $chain.ChainPolicy.UrlRetrievalTimeout = [TimeSpan]::FromSeconds(15)
            if (-not $chain.Build($productionCert)) {
                $chainErrors = ($chain.ChainStatus | ForEach-Object { $_.StatusInformation.Trim() }) -join '; '
                throw "The configured signing certificate does not chain to a trusted root: $chainErrors"
            }
        } finally {
            $chain.Dispose()
        }

        $signTool = $env:DESKSTREAM_SIGNTOOL
        if (-not $signTool) {
            $signTool = Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\bin\*\x64\signtool.exe' -ErrorAction SilentlyContinue |
                Sort-Object FullName -Descending |
                Select-Object -First 1 -ExpandProperty FullName
        }
        if (-not $signTool -or -not (Test-Path -LiteralPath $signTool)) {
            throw "Production signing unavailable: signtool.exe was not found. Install the Windows SDK or set DESKSTREAM_SIGNTOOL."
        }
        $env:DESKSTREAM_SIGNTOOL = $signTool
        $signingScript = $ProductionSigner
    } else {
        $signingScript = $null
        Write-Warning "UNSIGNED DEVELOPMENT BUILD"
        Write-Warning "Smart App Control/SmartScreen warnings are expected for this unsigned artifact."
    }

    function global:Sign-Binary([string]$Path) {
        if (-not $signingScript) {
            Write-Host "Skipping signing for $Path (Development mode)"
            return
        }
        if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
            throw "Sign-Binary cannot sign a missing file: $Path"
        }

        Write-Host "Signing $Path with certificate $($productionCert.Subject)..."
        try {
            & $signingScript -ExePath $Path
            if ($LASTEXITCODE -ne 0) {
                throw "sign-release.ps1 exited with code $LASTEXITCODE."
            }

            $sig = Get-AuthenticodeSignature -FilePath $Path
            if ($sig.Status -ne 'Valid' -or -not $sig.SignerCertificate) {
                throw "Windows reports signature status '$($sig.Status)' for $Path."
            }
            if ($sig.SignerCertificate.Thumbprint -ne $productionCert.Thumbprint) {
                throw "The resulting signature does not match the configured production certificate."
            }

            Verify-ProductionSignature $Path
        } catch {
            throw "Sign-Binary failed for '$Path': $($_.Exception.Message)"
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

        if ($SigningMode -eq "Production" -and (-not $certificate -or $signature.Status -notin @('Valid', 'UnknownError'))) {
            throw "Binary has no verifiable Authenticode signature: $Path ($($signature.Status))"
        }
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

$previousCargoTargetDir = $env:CARGO_TARGET_DIR
$env:CARGO_TARGET_DIR = $CargoTargetDir
try {
    if (Test-Path -LiteralPath $CargoExePath) {
        Remove-Item -LiteralPath $CargoExePath -Force
        if (Test-Path -LiteralPath $CargoExePath) {
            throw "Could not remove the previous release executable: $CargoExePath"
        }
    }

    Write-Host "Building Cargo release into $CargoTargetDir..."
    Set-Location -Path $DesktopAgentDir
    cargo build --release --bin DeskStream
    if ($LASTEXITCODE -ne 0) {
        throw "Cargo release build failed with exit code $LASTEXITCODE."
    }
} finally {
    $env:CARGO_TARGET_DIR = $previousCargoTargetDir
    Set-Location -Path $ProjectRoot
}

if (-not (Test-Path -LiteralPath $CargoExePath)) {
    throw "Fresh Cargo release executable not found at $CargoExePath"
}

if (-Not (Test-Path $FinalExeDir)) {
    New-Item -ItemType Directory -Force -Path $FinalExeDir | Out-Null
}

# Sign the Cargo output itself, when production signing is configured, then copy
# those exact bytes into the NSIS input. Development mode deliberately remains unsigned.
Sign-Binary $CargoExePath
Copy-Item -LiteralPath $CargoExePath -Destination $ExePath -Force
$CargoExeHash = (Get-FileHash $CargoExePath -Algorithm SHA256).Hash
$ExeHash = (Get-FileHash $ExePath -Algorithm SHA256).Hash
Write-Host "Cargo release executable: $CargoExePath"
Write-Host "Cargo release timestamp (UTC): $((Get-Item -LiteralPath $CargoExePath).LastWriteTimeUtc.ToString('o'))"
Write-Host "Cargo release SHA256: $CargoExeHash"
Write-Host "NSIS input executable: $ExePath"
Write-Host "NSIS input timestamp (UTC): $((Get-Item -LiteralPath $ExePath).LastWriteTimeUtc.ToString('o'))"
Write-Host "NSIS input SHA256: $ExeHash"
if ($CargoExeHash -ne $ExeHash) {
    throw "SHA256 mismatch: NSIS input differs from the current Cargo release executable."
}

$InstallerOutputDir = $DistDir
$InstallerOutputName = "DeskStream-Setup-x64.exe"
if ($SigningMode -eq "Development") {
    $InstallerOutputDir = Join-Path $DistDir "development"
    $InstallerOutputName = "DeskStream-Setup-x64-unsigned.exe"
}
if (-Not (Test-Path $InstallerOutputDir)) {
    New-Item -ItemType Directory -Force -Path $InstallerOutputDir | Out-Null
}

$staleInstaller = Join-Path $InstallerOutputDir $InstallerOutputName
if (Test-Path $staleInstaller) {
    Remove-Item $staleInstaller -Force
    if (Test-Path $staleInstaller) {
        throw "Could not remove the previous installer artifact: $staleInstaller"
    }
}

Write-Host "Building NSIS installer..."
$makensisPath = Ensure-Nsis
Set-Location $InstallerDir
if ($signingScript) {
    & $makensisPath "/DDESKSTREAM_SIGNING_SCRIPT=$signingScript" "DeskStream.nsi"
} else {
    & $makensisPath "/DDESKSTREAM_OUTFILE=..\dist-installer\development\DeskStream-Setup-x64-unsigned.exe" "/DDESKSTREAM_DEVELOPMENT_BUILD=1" "DeskStream.nsi"
}
if ($LASTEXITCODE -ne 0) {
    throw "NSIS installer build failed."
}

$InstallerPath = Join-Path $InstallerOutputDir $InstallerOutputName
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
Write-Host "Installer timestamp (UTC): $((Get-Item -LiteralPath $InstallerPath).LastWriteTimeUtc.ToString('o'))"
Write-Host "FINAL_EXE_SHA256=$ExeHash"
Write-Host "FINAL_PACKAGE_SHA256=$InstallerHash"
Write-Host "FINAL_PACKAGE_PATH=$InstallerPath"
if ($SigningMode -eq "Production") {
    Write-Host "SIGNATURE_STATUS=Valid"
    Write-Host "CERTIFICATE_SUBJECT=$($productionCert.Subject)"
    Write-Host "PACKAGE_TRUSTED=TRUE (signtool verification and trusted certificate chain passed)"
} else {
    Write-Host "SIGNATURE_STATUS=NotSigned"
    Write-Host "CERTIFICATE_SUBJECT=(none)"
    Write-Host "PACKAGE_TRUSTED=FALSE (unsigned development artifact)"
}

& $PayloadVerifier -SourcePath $CargoExePath -StagedPath $ExePath -InstallerPath $InstallerPath -InstallerFormat NSIS
if ($LASTEXITCODE -ne 0) {
    throw "Installer payload verification failed."
}

Write-Host "Build pipeline complete."
