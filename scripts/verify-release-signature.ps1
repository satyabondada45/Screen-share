param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$ExePath,

    [string]$DestinationPath = $null
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Fail([string]$Message) {
    Write-Error $Message
    exit 1
}

if (-not (Test-Path $ExePath)) {
    Fail "Executable not found: $ExePath"
}

$ResolvedExe = (Resolve-Path $ExePath).Path
$Sig = Get-AuthenticodeSignature $ResolvedExe
if (-not $Sig -or $Sig.Status -ne 'Valid') {
    Fail "AUTHENTICODE_STATUS=NotValid"
}

if (-not $Sig.SignerCertificate) {
    Fail 'AUTHENTICODE_STATUS=MissingSignerCertificate'
}

$Cert = $Sig.SignerCertificate
$EkuList = @()
if ($Cert.EnhancedKeyUsageList) {
    $EkuList = @($Cert.EnhancedKeyUsageList | ForEach-Object { $_.FriendlyName; $_.ObjectId.Value; $_.ObjectId.FriendlyName })
}
if (-not ($EkuList -match 'Code Signing' -or $EkuList -match 'codesigning')) {
    Fail 'AUTHENTICODE_STATUS=MissingCodeSigningEKU'
}

$TrustedTimestamp = $null
if ($Sig.TimeStamperCertificate) {
    $TrustedTimestamp = $Sig.TimeStamperCertificate
}
if (-not $TrustedTimestamp) {
    Fail 'TIMESTAMP_STATUS=MissingTimestamp'
}

$Hash = (Get-FileHash $ResolvedExe -Algorithm SHA256).Hash
Write-Host "AUTHENTICODE_STATUS=$($Sig.Status)"
Write-Host "SIGNER_SUBJECT=$($Cert.Subject)"
Write-Host "SIGNER_ISSUER=$($Cert.Issuer)"
Write-Host "SIGNER_THUMBPRINT=$($Cert.Thumbprint)"
Write-Host "CERT_VALIDITY=$($Cert.NotBefore.ToString('yyyy-MM-dd')) to $($Cert.NotAfter.ToString('yyyy-MM-dd'))"
Write-Host "CODE_SIGNING_EKU=Present"
Write-Host "TIMESTAMP_STATUS=Valid"
Write-Host "SHA256=$Hash"
Write-Host "EXE_PATH=$ResolvedExe"

if ($DestinationPath) {
    $ResolvedDestination = (Resolve-Path $DestinationPath -ErrorAction SilentlyContinue)
    if (-not $ResolvedDestination) {
        Fail "Destination path not found: $DestinationPath"
    }
    $DestHash = (Get-FileHash $ResolvedDestination.Path -Algorithm SHA256).Hash
    Write-Host "DESTINATION_PATH=$($ResolvedDestination.Path)"
    Write-Host "DESTINATION_SHA256=$DestHash"
}
