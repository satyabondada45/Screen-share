# Builds and signs the DeskStream MSIX using the production code-signing certificate.

$ErrorActionPreference = "Stop"
$projectRoot = Split-Path -Parent $PSScriptRoot
$msixDir = Join-Path $projectRoot "packaging\msix"
$outputDir = Join-Path $projectRoot "packaging\output"
$cargoTargetDir = "C:\cargo-target\deskstream"
$cargoExe = Join-Path $cargoTargetDir "release\DeskStream.exe"
$msixFile = Join-Path $outputDir "DeskStream.msix"
$reusableStagedExe = Join-Path $msixDir "DeskStream.exe"
$signer = Join-Path $projectRoot "scripts\sign-release.ps1"
$signatureVerifier = Join-Path $projectRoot "scripts\verify-release-signature.ps1"

if (Test-Path -LiteralPath $msixFile) {
    Remove-Item -LiteralPath $msixFile -Force
}
if (Test-Path -LiteralPath $reusableStagedExe) {
    Remove-Item -LiteralPath $reusableStagedExe -Force
}

if (-not $env:DESKSTREAM_CERT_THUMBPRINT) {
    throw "Production MSIX signing unavailable: DESKSTREAM_CERT_THUMBPRINT is not configured. No MSIX will be created."
}
if (-not (Test-Path -LiteralPath $msixDir -PathType Container) -or
    -not (Test-Path -LiteralPath (Join-Path $msixDir "AppxManifest.xml") -PathType Leaf)) {
    throw "MSIX package definition is missing: $msixDir\AppxManifest.xml. No MSIX was built or published."
}
if (-not (Test-Path -LiteralPath $signer -PathType Leaf) -or
    -not (Test-Path -LiteralPath $signatureVerifier -PathType Leaf)) {
    throw "Production signing and verification scripts are required for MSIX packaging."
}

$certificate = Get-ChildItem Cert:\CurrentUser\My,Cert:\LocalMachine\My -ErrorAction SilentlyContinue |
    Where-Object {
        $_.Thumbprint -eq $env:DESKSTREAM_CERT_THUMBPRINT -and
        $_.HasPrivateKey -and
        $_.Subject -ne $_.Issuer -and
        $_.NotBefore -le (Get-Date) -and
        $_.NotAfter -ge (Get-Date) -and
        ($_.EnhancedKeyUsageList.ObjectId.Value -contains "1.3.6.1.5.5.7.3.3")
    } |
    Select-Object -First 1
if (-not $certificate) {
    throw "Configured production certificate is unavailable, expired, self-signed, or lacks the Code Signing EKU."
}

$chain = New-Object System.Security.Cryptography.X509Certificates.X509Chain
try {
    $chain.ChainPolicy.RevocationMode = [System.Security.Cryptography.X509Certificates.X509RevocationMode]::Online
    $chain.ChainPolicy.RevocationFlag = [System.Security.Cryptography.X509Certificates.X509RevocationFlag]::ExcludeRoot
    $chain.ChainPolicy.VerificationFlags = [System.Security.Cryptography.X509Certificates.X509VerificationFlags]::NoFlag
    $chain.ChainPolicy.UrlRetrievalTimeout = [TimeSpan]::FromSeconds(15)
    if (-not $chain.Build($certificate)) {
        $chainErrors = ($chain.ChainStatus | ForEach-Object { $_.StatusInformation.Trim() }) -join '; '
        throw "The configured MSIX signing certificate does not chain to a trusted root: $chainErrors"
    }
} finally {
    $chain.Dispose()
}

[xml]$manifest = Get-Content -LiteralPath (Join-Path $msixDir "AppxManifest.xml") -Raw
$identity = $manifest.SelectSingleNode("//*[local-name()='Identity']")
if (-not $identity -or [string]::IsNullOrWhiteSpace($identity.GetAttribute("Name")) -or
    $identity.GetAttribute("Publisher") -ne $certificate.Subject) {
    throw "MSIX Identity Name must be set and Publisher must exactly match the production signing certificate subject ($($certificate.Subject))."
}
$application = $manifest.SelectSingleNode("//*[local-name()='Application']")
if (-not $application -or $application.GetAttribute("Executable") -ne "DeskStream.exe") {
    throw "MSIX manifest must declare DeskStream.exe as its application executable."
}

$makeAppx = (Get-Command MakeAppx.exe -ErrorAction SilentlyContinue).Source
if (-not $makeAppx) {
    $makeAppx = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\makeappx.exe" -ErrorAction SilentlyContinue |
        Sort-Object FullName -Descending |
        Select-Object -First 1 -ExpandProperty FullName
}
if (-not $makeAppx -or -not (Test-Path -LiteralPath $makeAppx)) {
    throw "MakeAppx.exe was not found. Install the Windows SDK to build MSIX packages."
}

$signTool = $env:DESKSTREAM_SIGNTOOL
if (-not $signTool) {
    $signTool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" -ErrorAction SilentlyContinue |
        Sort-Object FullName -Descending |
        Select-Object -First 1 -ExpandProperty FullName
}
if (-not $signTool -or -not (Test-Path -LiteralPath $signTool)) {
    throw "signtool.exe was not found. Install the Windows SDK or set DESKSTREAM_SIGNTOOL."
}

$previousTargetDir = $env:CARGO_TARGET_DIR
try {
    $env:CARGO_TARGET_DIR = $cargoTargetDir
    if (Test-Path -LiteralPath $cargoExe) {
        Remove-Item -LiteralPath $cargoExe -Force
    }
    Push-Location $PSScriptRoot
    try {
        cargo build --release --bin DeskStream
        if ($LASTEXITCODE -ne 0) {
            throw "Cargo build failed with exit code $LASTEXITCODE."
        }
    } finally {
        Pop-Location
        $env:CARGO_TARGET_DIR = $previousTargetDir
    }
} finally {
    $env:CARGO_TARGET_DIR = $previousTargetDir
}
if (-not (Test-Path -LiteralPath $cargoExe -PathType Leaf)) {
    throw "Fresh Cargo executable was not produced at $cargoExe."
}

& $signer -ExePath $cargoExe
if ($LASTEXITCODE -ne 0) {
    throw "Production signing of DeskStream.exe failed."
}
& $signatureVerifier -ExePath $cargoExe
if ($LASTEXITCODE -ne 0) {
    throw "DeskStream.exe production signature verification failed."
}

New-Item -ItemType Directory -Path $outputDir -Force | Out-Null
$cargoHash = (Get-FileHash -LiteralPath $cargoExe -Algorithm SHA256).Hash
$packageStageDir = Join-Path ([System.IO.Path]::GetTempPath()) ("DeskStream-msix-stage-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $packageStageDir -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $msixDir "AppxManifest.xml") -Destination $packageStageDir
Copy-Item -LiteralPath (Join-Path $msixDir "Assets") -Destination $packageStageDir -Recurse
$packagedExe = Join-Path $packageStageDir "DeskStream.exe"
Copy-Item -LiteralPath $cargoExe -Destination $packagedExe -Force
$stagedHash = (Get-FileHash -LiteralPath $packagedExe -Algorithm SHA256).Hash
if ($cargoHash -ne $stagedHash) {
    throw "MSIX staging executable differs from the freshly built, signed Cargo executable."
}
if (@(Get-ChildItem -LiteralPath $packageStageDir -Filter "*.exe" -File -Recurse).Count -ne 1) {
    throw "MSIX staging must contain exactly one executable payload: the fresh DeskStream.exe."
}
if (Test-Path -LiteralPath $msixFile) {
    Remove-Item -LiteralPath $msixFile -Force
}

& $makeAppx pack /d $packageStageDir /p $msixFile /o
if ($LASTEXITCODE -ne 0) {
    throw "MakeAppx failed to create the MSIX package."
}

$timestampUrl = $env:DESKSTREAM_TIMESTAMP_URL
if (-not $timestampUrl) {
    $timestampUrl = "https://timestamp.digicert.com"
}
& $signTool sign /fd SHA256 /sha1 $certificate.Thumbprint /tr $timestampUrl /td SHA256 /v $msixFile
if ($LASTEXITCODE -ne 0) {
    throw "signtool failed to sign the MSIX package."
}
$verifyOutput = & $signTool verify /pa /v $msixFile 2>&1
$verifyOutput | ForEach-Object { Write-Host $_ }
if ($LASTEXITCODE -ne 0) {
    throw "MSIX signature or certificate chain verification failed."
}

$verifyDir = Join-Path ([System.IO.Path]::GetTempPath()) ("DeskStream-msix-" + [Guid]::NewGuid().ToString("N"))
try {
    & $makeAppx unpack /p $msixFile /d $verifyDir /o
    if ($LASTEXITCODE -ne 0) {
        throw "MakeAppx could not unpack the signed MSIX for payload verification."
    }
    $embeddedExe = Join-Path $verifyDir "DeskStream.exe"
    if (-not (Test-Path -LiteralPath $embeddedExe -PathType Leaf)) {
        throw "Signed MSIX does not contain DeskStream.exe."
    }
    $embeddedExecutables = @(Get-ChildItem -LiteralPath $verifyDir -Filter "*.exe" -File -Recurse)
    if ($embeddedExecutables.Count -ne 1 -or $embeddedExecutables[0].FullName -ne $embeddedExe) {
        throw "Final MSIX must contain exactly one executable payload: DeskStream.exe."
    }
    $embeddedHash = (Get-FileHash -LiteralPath $embeddedExe -Algorithm SHA256).Hash
    if ($embeddedHash -ne $cargoHash) {
        throw "Signed MSIX contains a DeskStream.exe different from the fresh Cargo output."
    }
    & $signatureVerifier -ExePath $embeddedExe
    if ($LASTEXITCODE -ne 0) {
        throw "Embedded MSIX executable signature verification failed."
    }

    $packageHash = (Get-FileHash -LiteralPath $msixFile -Algorithm SHA256).Hash
    $packageSignature = Get-AuthenticodeSignature -FilePath $msixFile
    Write-Host "Cargo release DeskStream.exe SHA256: $cargoHash"
    Write-Host "Staged DeskStream.exe SHA256: $stagedHash"
    Write-Host "MSIX embedded DeskStream.exe SHA256: $embeddedHash"
    Write-Host "FINAL_EXE_SHA256=$embeddedHash"
    Write-Host "FINAL_PACKAGE_SHA256=$packageHash"
    Write-Host "FINAL_PACKAGE_PATH=$msixFile"
    Write-Host "SIGNATURE_STATUS=Verified by signtool /pa /v; Authenticode status: $($packageSignature.Status)"
    Write-Host "CERTIFICATE_SUBJECT=$($certificate.Subject)"
    Write-Host "CERTIFICATE_PUBLISHER=$($certificate.GetNameInfo([System.Security.Cryptography.X509Certificates.X509NameType]::SimpleName, $false))"
    Write-Host "PACKAGE_TRUSTED=TRUE (signtool verification and trusted certificate chain passed)"
    Write-Host "MSIX timestamp (UTC): $((Get-Item -LiteralPath $msixFile).LastWriteTimeUtc.ToString('o'))"
} finally {
    foreach ($temporaryDir in @($verifyDir, $packageStageDir)) {
        if ($temporaryDir -and (Test-Path -LiteralPath $temporaryDir)) {
            Remove-Item -LiteralPath $temporaryDir -Recurse -Force
        }
    }
}
