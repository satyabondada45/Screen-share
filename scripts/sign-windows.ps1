param (
    [Parameter(Mandatory=$true)]
    [string]$ExePath
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path $ExePath)) {
    throw "File to sign not found: $ExePath"
}

$CertSubject = "CN=DeskStream Dev Signing"
$StoreLocation = "Cert:\CurrentUser\My"

# Find existing cert
$cert = Get-ChildItem -Path $StoreLocation | Where-Object { $_.Subject -match $CertSubject } | Select-Object -First 1

if (-not $cert) {
    Write-Host "Creating new self-signed certificate for development..."
    $cert = New-SelfSignedCertificate -Subject $CertSubject -Type CodeSigningCert -CertStoreLocation $StoreLocation
} else {
    Write-Host "Reusing existing development certificate: $($cert.Thumbprint)"
}

Write-Host "Signing $ExePath..."

$signtool = Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\bin" -Filter "signtool.exe" -Recurse -ErrorAction SilentlyContinue | Where-Object { $_.FullName -match "x64" } | Select-Object -First 1 -ExpandProperty FullName

if ($signtool) {
    Write-Host "Using signtool.exe at $signtool"
    $thumbprint = $cert.Thumbprint
    & $signtool sign /sha1 $thumbprint /tr "http://timestamp.digicert.com" /td sha256 /fd sha256 $ExePath | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "signtool.exe failed to sign $ExePath"
    }
} else {
    Write-Host "signtool.exe not found, falling back to Set-AuthenticodeSignature"
    Set-AuthenticodeSignature -FilePath $ExePath -Certificate $cert -TimestampServer "http://timestamp.digicert.com" | Out-Null
}

$sig = Get-AuthenticodeSignature -FilePath $ExePath
if ($sig.Status -ne "Valid" -and $sig.Status -ne "UnknownError") {
    throw "Signing failed for $ExePath. Status: $($sig.Status) - $($sig.StatusMessage)"
}
Write-Host "Successfully signed $ExePath (Status: $($sig.Status))"
