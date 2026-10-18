param (
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$ExePath,

    [string]$Thumbprint = $null
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path $ExePath)) {
    throw "File to sign not found: $ExePath"
}

$ResolvedExe = (Resolve-Path $ExePath).Path

# Locate signtool.exe
$signtool = $env:DESKSTREAM_SIGNTOOL
if (-not $signtool -or -not (Test-Path $signtool)) {
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
            $signtool = $found.FullName
            break
        }
    }
}

if (-not $signtool -or -not (Test-Path $signtool)) {
    throw "signtool.exe not found. Install the Windows 10/11 SDK or set DESKSTREAM_SIGNTOOL."
}

# Determine certificate thumbprint
$targetThumbprint = if ($Thumbprint) {
    $Thumbprint
} elseif ($env:DESKSTREAM_DEV_CERT_THUMBPRINT) {
    $env:DESKSTREAM_DEV_CERT_THUMBPRINT
} elseif ($env:DESKSTREAM_CERT_THUMBPRINT) {
    $env:DESKSTREAM_CERT_THUMBPRINT
} else {
    $null
}

$cert = $null
if ($targetThumbprint) {
    foreach ($store in @('Cert:\CurrentUser\My', 'Cert:\LocalMachine\My')) {
        $cert = Get-Item "$store\$targetThumbprint" -ErrorAction SilentlyContinue
        if ($cert) { break }
    }
}

if (-not $cert) {
    $allCerts = Get-ChildItem Cert:\CurrentUser\My, Cert:\LocalMachine\My -ErrorAction SilentlyContinue |
        Where-Object {
            $_.HasPrivateKey -and
            ($_.NotBefore -le (Get-Date)) -and
            ($_.NotAfter -ge (Get-Date)) -and
            (
                ($_.EnhancedKeyUsageList | Where-Object { $_.FriendlyName -eq 'Code Signing' -or $_.Value -eq '1.3.6.1.5.5.7.3.3' -or $_.ObjectId.Value -eq '1.3.6.1.5.5.7.3.3' }) -or
                ($_.Extensions | Where-Object { $_ -is [System.Security.Cryptography.X509Certificates.X509EnhancedKeyUsageExtension] -and ($_.EnhancedKeyUsages | Where-Object { $_.Value -eq '1.3.6.1.5.5.7.3.3' -or $_.FriendlyName -eq 'Code Signing' }) })
            )
        }

    $cert = $allCerts | Where-Object { $_.Subject -match 'Friends Software Solutions' } | Select-Object -First 1
    if (-not $cert) {
        $cert = $allCerts | Where-Object { $_.Subject -match 'DeskStream Dev Signing' } | Select-Object -First 1
    }
    if (-not $cert) {
        $cert = $allCerts | Select-Object -First 1
    }
}

if (-not $cert) {
    Write-Host "Creating new self-signed certificate for development..."
    $cert = New-SelfSignedCertificate -Subject "CN=DeskStream Dev Signing" -Type CodeSigningCert -CertStoreLocation "Cert:\CurrentUser\My"
}

Write-Host "Signing $ResolvedExe with $($cert.Subject) ($($cert.Thumbprint))..."
$timestampUrl = if ($env:DESKSTREAM_TIMESTAMP_URL) { $env:DESKSTREAM_TIMESTAMP_URL } else { "http://timestamp.digicert.com" }

& $signtool sign /fd SHA256 /sha1 $cert.Thumbprint /s My /tr $timestampUrl /td SHA256 $ResolvedExe
if ($LASTEXITCODE -ne 0) {
    # Retry once without timestamp if timestamp server timed out
    Write-Warning "Timestamped signing failed; retrying..."
    & $signtool sign /fd SHA256 /sha1 $cert.Thumbprint /s My $ResolvedExe
    if ($LASTEXITCODE -ne 0) {
        throw "signtool.exe failed to sign $ResolvedExe (exit code $LASTEXITCODE)"
    }
}

$sig = Get-AuthenticodeSignature -FilePath $ResolvedExe
if (-not $sig.SignerCertificate) {
    throw "Signing failed for ${ResolvedExe}: missing signer certificate."
}
Write-Host "Successfully signed $ResolvedExe (Status: $($sig.Status), Signer: $($sig.SignerCertificate.Subject))"
