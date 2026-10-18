param (
    [ValidateSet("Production", "Development", "Unsigned")]
    [string]$SigningMode = "Development",

    [string]$CertThumbprint = $null
)

$ErrorActionPreference = "Stop"

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

# Automatically upgrade to Production if a valid production certificate is configured
if ($SigningMode -eq "Development" -and -not [string]::IsNullOrWhiteSpace($env:DESKSTREAM_CERT_THUMBPRINT)) {
    $SigningMode = "Production"
}

function Ensure-Nsis {
    $existing = Get-Command makensis -ErrorAction SilentlyContinue
    if ($existing) {
        return $existing.Source
    }

    $candidate = @(
        'C:\Program Files (x86)\NSIS\makensis.exe',
        'C:\Program Files\NSIS\makensis.exe'
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

    throw "makensis is required to build the Windows installer. Install NSIS and try again."
}

function Locate-SignTool {
    $signTool = $env:DESKSTREAM_SIGNTOOL
    if ($signTool -and (Test-Path -LiteralPath $signTool)) {
        return $signTool
    }

    $candidates = @(
        'C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\signtool.exe',
        'C:\Program Files (x86)\Windows Kits\10\bin\*\x64\signtool.exe',
        'C:\Program Files (x86)\Windows Kits\*\bin\*\x64\signtool.exe'
    )
    foreach ($pattern in $candidates) {
        $found = Get-Item $pattern -ErrorAction SilentlyContinue |
            Sort-Object FullName -Descending |
            Select-Object -First 1
        if ($found -and (Test-Path $found.FullName)) {
            $env:DESKSTREAM_SIGNTOOL = $found.FullName
            return $found.FullName
        }
    }
    return $null
}

function Get-CodeSigningCertificate([string]$Thumbprint) {
    if ($Thumbprint) {
        foreach ($store in @('Cert:\CurrentUser\My', 'Cert:\LocalMachine\My')) {
            $certificate = Get-Item "$store\$Thumbprint" -ErrorAction SilentlyContinue
            if ($certificate -and $certificate.HasPrivateKey) {
                return $certificate
            }
        }
    }
    return $null
}

Write-Host "========================================"
Write-Host " DESKSTREAM RELEASE PACKAGING PIPELINE"
Write-Host "========================================"
Write-Host "Target Signing Mode: $SigningMode"

$signTool = Locate-SignTool
if ($SigningMode -ne "Unsigned" -and (-not $signTool -or -not (Test-Path -LiteralPath $signTool))) {
    throw "Signing tool signtool.exe not found. Install Windows SDK or set DESKSTREAM_SIGNTOOL."
}

$productionCert = $null
$developmentCert = $null
$signingScript = $null

if ($SigningMode -eq "Production") {
    $thumb = if ($CertThumbprint) { $CertThumbprint } else { $env:DESKSTREAM_CERT_THUMBPRINT }
    if (-not $thumb) {
        throw "Production signing requires DESKSTREAM_CERT_THUMBPRINT to be configured."
    }
    $productionCert = Get-CodeSigningCertificate $thumb
    if (-not $productionCert -or $productionCert.NotBefore -gt (Get-Date) -or $productionCert.NotAfter -lt (Get-Date)) {
        throw "Production signing failed: certificate $thumb missing, expired, or lacks a private key."
    }
    if ($productionCert.Subject -eq $productionCert.Issuer) {
        throw "A self-signed certificate ($($productionCert.Subject)) cannot be used for Production signing. For development/testing, run with -SigningMode Development."
    }

    $codeSigningEku = @($productionCert.EnhancedKeyUsageList | ForEach-Object { $_.ObjectId.Value })
    if ($codeSigningEku -notcontains '1.3.6.1.5.5.7.3.3') {
        throw "The configured certificate does not have the Code Signing EKU (1.3.6.1.5.5.7.3.3)."
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

    $signingScript = $ProductionSigner
    Write-Host "Configured Production certificate: $($productionCert.Subject)"
} elseif ($SigningMode -eq "Development") {
    $devThumb = if ($CertThumbprint) {
        $CertThumbprint
    } elseif ($env:DESKSTREAM_DEV_CERT_THUMBPRINT) {
        $env:DESKSTREAM_DEV_CERT_THUMBPRINT
    } else {
        $null
    }

    if ($devThumb) {
        $developmentCert = Get-CodeSigningCertificate $devThumb
    }

    if (-not $developmentCert) {
        # Discover available local signing certificates: prefer Friends Software Solutions, then DeskStream Dev Signing
        $candidates = Get-ChildItem Cert:\CurrentUser\My, Cert:\LocalMachine\My -ErrorAction SilentlyContinue |
            Where-Object {
                $_.HasPrivateKey -and
                ($_.NotBefore -le (Get-Date)) -and
                ($_.NotAfter -ge (Get-Date)) -and
                (
                    ($_.EnhancedKeyUsageList | Where-Object { $_.FriendlyName -eq 'Code Signing' -or $_.Value -eq '1.3.6.1.5.5.7.3.3' -or $_.ObjectId.Value -eq '1.3.6.1.5.5.7.3.3' }) -or
                    ($_.Extensions | Where-Object { $_ -is [System.Security.Cryptography.X509Certificates.X509EnhancedKeyUsageExtension] -and ($_.EnhancedKeyUsages | Where-Object { $_.Value -eq '1.3.6.1.5.5.7.3.3' -or $_.FriendlyName -eq 'Code Signing' }) })
                )
            }
        $developmentCert = $candidates | Where-Object { $_.Subject -match 'Friends Software Solutions' } | Select-Object -First 1
        if (-not $developmentCert) {
            $developmentCert = $candidates | Where-Object { $_.Subject -match 'DeskStream Dev Signing' } | Select-Object -First 1
        }
        if (-not $developmentCert) {
            $developmentCert = $candidates | Select-Object -First 1
        }
    }

    if (-not $developmentCert) {
        Write-Host "Creating new self-signed certificate for development..."
        $developmentCert = New-SelfSignedCertificate -Subject "CN=DeskStream Dev Signing" -Type CodeSigningCert -CertStoreLocation "Cert:\CurrentUser\My"
    }

    $env:DESKSTREAM_DEV_CERT_THUMBPRINT = $developmentCert.Thumbprint
    $signingScript = $DevelopmentSigner

    Write-Host "Using Development Signing Certificate: $($developmentCert.Subject)"
    Write-Host "Thumbprint: $($developmentCert.Thumbprint)"
    Write-Host "Valid until: $($developmentCert.NotAfter.ToString('yyyy-MM-dd'))"
    Write-Warning "DEVELOPMENT MODE: Signing with self-signed certificate '$($developmentCert.Subject)'."
    Write-Warning "NOTE: Self-signed certificates are untrusted outside this machine. Windows Smart App Control and SmartScreen on clean machines require a publicly trusted commercial CA certificate or established cloud reputation."
} else {
    $signingScript = $null
    Write-Warning "EXPLICIT UNSIGNED BUILD: Signing is disabled. Smart App Control will block unsigned binaries."
}

function global:Sign-Binary([string]$Path) {
    if ($SigningMode -eq "Unsigned") {
        Write-Host "Skipping signing for $Path (Unsigned mode requested)"
        return
    }
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "Sign-Binary cannot sign a missing file: $Path"
    }

    $timestampUrl = if ($env:DESKSTREAM_TIMESTAMP_URL) { $env:DESKSTREAM_TIMESTAMP_URL } else { "http://timestamp.digicert.com" }

    if ($SigningMode -eq "Production") {
        Write-Host "Signing $Path with Production certificate $($productionCert.Subject)..."
        & $ProductionSigner -ExePath $Path
        if ($LASTEXITCODE -ne 0) {
            throw "Production signing failed for $Path"
        }
        & $SignatureVerifier -ExePath $Path
        if ($LASTEXITCODE -ne 0) {
            throw "Production signature verification failed for $Path"
        }
    } elseif ($SigningMode -eq "Development") {
        Write-Host "Signing $Path with Development certificate $($developmentCert.Subject)..."
        & $DevelopmentSigner -ExePath $Path -Thumbprint $developmentCert.Thumbprint
        if ($LASTEXITCODE -ne 0) {
            throw "Development signing failed for $Path"
        }

        $sig = Get-AuthenticodeSignature -FilePath $Path
        if (-not $sig.SignerCertificate -or $sig.SignerCertificate.Thumbprint -ne $developmentCert.Thumbprint) {
            throw "Signature verification failed: file $Path does not have the expected signer certificate."
        }
        Write-Host "Verified development signature on $Path (Status: $($sig.Status))"
    }
}

function Report-Signature([string]$Path) {
    $signature = Get-AuthenticodeSignature -FilePath $Path
    $certificate = $signature.SignerCertificate
    $timestamp = $signature.TimeStamperCertificate
    [pscustomobject]@{
        File = [IO.Path]::GetFileName($Path)
        Status = $signature.Status
        StatusMessage = $signature.StatusMessage
        Signer = if ($certificate) { $certificate.Subject } else { "(none)" }
        Issuer = if ($certificate) { $certificate.Issuer } else { "(none)" }
        Expires = if ($certificate) { $certificate.NotAfter.ToString("yyyy-MM-dd") } else { "(none)" }
        Timestamp = if ($timestamp) { "$($timestamp.Subject) (Expires: $($timestamp.NotAfter.ToString('yyyy-MM-dd')))" } else { "(none)" }
    } | Format-List
}

# 1. Stop any running DeskStream processes
Write-Host "Ensuring DeskStream processes are stopped..."
Get-Process -Name "DeskStream", "desktop-agent" -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 500

# 2. Build fresh Cargo release executable
$previousCargoTargetDir = $env:CARGO_TARGET_DIR
$env:CARGO_TARGET_DIR = $CargoTargetDir
try {
    if (Test-Path -LiteralPath $CargoExePath) {
        Remove-Item -LiteralPath $CargoExePath -Force
        if (Test-Path -LiteralPath $CargoExePath) {
            throw "Could not remove stale executable: $CargoExePath"
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

# 3. Sign the fresh Cargo output binary FIRST
Sign-Binary $CargoExePath

# 4. Copy the verified signed executable to the distribution directories
if (-not (Test-Path $FinalExeDir)) {
    New-Item -ItemType Directory -Force -Path $FinalExeDir | Out-Null
}

Copy-Item -LiteralPath $CargoExePath -Destination $ExePath -Force
Copy-Item -LiteralPath $CargoExePath -Destination (Join-Path $ProjectRoot "DeskStream.exe") -Force
Copy-Item -LiteralPath $CargoExePath -Destination (Join-Path $ProjectRoot "desktop-agent.exe") -Force

# Verify byte integrity across outputs
$CargoExeHash = (Get-FileHash $CargoExePath -Algorithm SHA256).Hash
$ExeHash = (Get-FileHash $ExePath -Algorithm SHA256).Hash
Write-Host "Cargo release SHA256: $CargoExeHash"
Write-Host "Distribution release SHA256: $ExeHash"
if ($CargoExeHash -ne $ExeHash) {
    throw "SHA256 mismatch: Distributed executable differs from signed Cargo build."
}

# 5. Build NSIS installer
$makensisPath = Ensure-Nsis
Set-Location $InstallerDir

if (-not (Test-Path $DistDir)) {
    New-Item -ItemType Directory -Force -Path $DistDir | Out-Null
}

$InstallerPath = Join-Path $DistDir "DeskStream-Setup-x64.exe"
if ($SigningMode -eq "Unsigned") {
    $devDist = Join-Path $DistDir "development"
    if (-not (Test-Path $devDist)) { New-Item -ItemType Directory -Force -Path $devDist | Out-Null }
    $InstallerPath = Join-Path $devDist "DeskStream-Setup-x64-unsigned.exe"
}

if (Test-Path $InstallerPath) {
    Remove-Item $InstallerPath -Force
}

Write-Host "Building NSIS installer with payload from $FinalExeDir..."
if ($signingScript) {
    & $makensisPath "/DDESKSTREAM_SIGNING_SCRIPT=$signingScript" "/DDESKSTREAM_PAYLOAD_DIR=$FinalExeDir" "/DDESKSTREAM_OUTFILE=$InstallerPath" "DeskStream.nsi"
} else {
    & $makensisPath "/DDESKSTREAM_OUTFILE=$InstallerPath" "/DDESKSTREAM_PAYLOAD_DIR=$FinalExeDir" "/DDESKSTREAM_DEVELOPMENT_BUILD=1" "DeskStream.nsi"
}

if ($LASTEXITCODE -ne 0) {
    throw "NSIS installer build failed."
}

if (-not (Test-Path $InstallerPath)) {
    throw "Installer executable not found at $InstallerPath"
}

# 6. Sign the final installer
Sign-Binary $InstallerPath

# 7. Payload Verification: verify installer contains the exact signed binary
Write-Host "Verifying installer payload matches signed Cargo executable..."
& $PayloadVerifier -SourcePath $CargoExePath -StagedPath $ExePath -InstallerPath $InstallerPath -InstallerFormat NSIS
if ($LASTEXITCODE -ne 0) {
    throw "Installer payload verification failed: bundled executable does not match."
}

# 8. Report final signatures and hashes
Write-Host "========================================"
Write-Host " PACKAGING AND VERIFICATION REPORT"
Write-Host "========================================"
Write-Host "Release Executable: $ExePath"
Write-Host "SHA256: $ExeHash"
Report-Signature $ExePath

Write-Host "Installer: $InstallerPath"
$InstallerHash = (Get-FileHash $InstallerPath -Algorithm SHA256).Hash
Write-Host "SHA256: $InstallerHash"
Report-Signature $InstallerPath

Write-Host "FINAL_EXE_PATH=$ExePath"
Write-Host "FINAL_EXE_SHA256=$ExeHash"
Write-Host "FINAL_INSTALLER_PATH=$InstallerPath"
Write-Host "FINAL_INSTALLER_SHA256=$InstallerHash"

if ($SigningMode -eq "Production") {
    Write-Host "SIGNING_MODE=Production"
    Write-Host "SIGNATURE_STATUS=Valid (Chained to Publicly Trusted Root)"
    Write-Host "SMART_APP_CONTROL_READY=YES"
} elseif ($SigningMode -eq "Development") {
    Write-Host "SIGNING_MODE=Development"
    Write-Host "SIGNATURE_STATUS=Signed-Development ($($developmentCert.Subject))"
    Write-Host "SMART_APP_CONTROL_READY=NO (Self-signed certificate requires manual Root trust on local machine; clean machines without trust or reputation will trigger Smart App Control)"
} else {
    Write-Host "SIGNING_MODE=Unsigned"
    Write-Host "SIGNATURE_STATUS=NotSigned"
    Write-Host "SMART_APP_CONTROL_READY=NO"
}

Write-Host "========================================"
Write-Host " Build pipeline complete."
Write-Host "========================================"
