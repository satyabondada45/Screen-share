param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$ExePath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Fail([string]$Message) {
    throw $Message
}

if (-not $ExePath) {
    Fail 'Usage: ./scripts/sign-release.ps1 <path-to-exe>'
}

if (-not (Test-Path $ExePath)) {
    Fail "Executable not found: $ExePath"
}

$ResolvedExe = (Resolve-Path $ExePath).Path
$SignedTool = $env:DESKSTREAM_SIGNTOOL

if (-not $SignedTool) {
    $Candidate = Get-ChildItem 'C:\Program Files (x86)\Windows Kits\*\bin\*\x64\signtool.exe' -ErrorAction SilentlyContinue |
        Sort-Object FullName -Descending |
        Select-Object -First 1

    if (-not $Candidate) {
        Fail 'No signtool.exe found. Set DESKSTREAM_SIGNTOOL to the signing tool path.'
    }

    $SignedTool = $Candidate.FullName
}

if (-not (Test-Path $SignedTool)) {
    Fail "signtool.exe not found at: $SignedTool"
}

$CertThumbprint = $env:DESKSTREAM_CERT_THUMBPRINT
$TimestampUrl = $env:DESKSTREAM_TIMESTAMP_URL

if (-not $TimestampUrl) {
    $TimestampUrl = 'https://timestamp.digicert.com'
}

if (-not $CertThumbprint) {
    Fail 'NO PRODUCTION SIGNING CERTIFICATE CONFIGURED'
}

$CertMatches = @(
    Get-ChildItem Cert:\CurrentUser\My, Cert:\LocalMachine\My -ErrorAction SilentlyContinue |
        Where-Object {
            $_.Thumbprint -eq $CertThumbprint -and
            $_.HasPrivateKey -and
            ($_.EnhancedKeyUsageList.ObjectId.Value -contains '1.3.6.1.5.5.7.3.3') -and
            $_.NotBefore -le (Get-Date) -and
            $_.NotAfter -ge (Get-Date) -and
            $_.Issuer -ne $_.Subject
        }
)

if (-not $CertMatches -or $CertMatches.Count -eq 0) {
    Fail 'NO PRODUCTION SIGNING CERTIFICATE CONFIGURED'
}

$ProductionCert = $CertMatches[0]
$chain = New-Object System.Security.Cryptography.X509Certificates.X509Chain
try {
    $chain.ChainPolicy.RevocationMode = [System.Security.Cryptography.X509Certificates.X509RevocationMode]::Online
    $chain.ChainPolicy.RevocationFlag = [System.Security.Cryptography.X509Certificates.X509RevocationFlag]::ExcludeRoot
    $chain.ChainPolicy.VerificationFlags = [System.Security.Cryptography.X509Certificates.X509VerificationFlags]::NoFlag
    $chain.ChainPolicy.UrlRetrievalTimeout = [TimeSpan]::FromSeconds(15)
    if (-not $chain.Build($ProductionCert)) {
        $chainErrors = ($chain.ChainStatus | ForEach-Object { $_.StatusInformation.Trim() }) -join '; '
        Fail "Configured signing certificate does not chain to a trusted root: $chainErrors"
    }
} finally {
    $chain.Dispose()
}

$SignArgs = @(
    'sign',
    '/fd', 'SHA256',
    '/tr', $TimestampUrl,
    '/td', 'SHA256',
    '/sha1', $CertThumbprint,
    '/v',
    $ResolvedExe
)

& $SignedTool @SignArgs
if ($LASTEXITCODE -ne 0) {
    Fail "Signing failed for: $ResolvedExe"
}

& $SignedTool verify /pa /v $ResolvedExe
if ($LASTEXITCODE -ne 0) {
    Fail "Signature verification failed for: $ResolvedExe"
}

$Signature = Get-AuthenticodeSignature $ResolvedExe
if (-not $Signature -or $Signature.Status -ne 'Valid') {
    Fail 'The resulting Authenticode signature is not valid.'
}

if (-not $Signature.SignerCertificate) {
    Fail 'SignerCertificate is missing after signing.'
}
if ($Signature.SignerCertificate.Thumbprint -ne $ProductionCert.Thumbprint) {
    Fail 'The resulting Authenticode signature does not match DESKSTREAM_CERT_THUMBPRINT.'
}

if (-not $Signature.SignerCertificate.Extensions) {
    Fail 'Signing certificate EKU information is missing.'
}

$Hash = (Get-FileHash $ResolvedExe -Algorithm SHA256).Hash
Write-Host "SIGNATURE_STATUS=$($Signature.Status)"
Write-Host "SIGNER_SUBJECT=$($Signature.SignerCertificate.Subject)"
Write-Host "SIGNER_ISSUER=$($Signature.SignerCertificate.Issuer)"
Write-Host "SIGNER_THUMBPRINT=$($Signature.SignerCertificate.Thumbprint)"
Write-Host "CERT_VALIDITY=$($Signature.SignerCertificate.NotBefore.ToString('yyyy-MM-dd')) to $($Signature.SignerCertificate.NotAfter.ToString('yyyy-MM-dd'))"
Write-Host "TIMESTAMP_STATUS=Valid"
Write-Host "SHA256=$Hash"
