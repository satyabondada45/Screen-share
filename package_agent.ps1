# Packages the verified unified DeskStream release without rebuilding it.

$ErrorActionPreference = "Stop"

$ReleaseExe = Join-Path $PSScriptRoot "DESKSTREAM\DeskStream.exe"
$CargoExe = "C:\cargo-target\deskstream\release\DeskStream.exe"
$DownloadsDir = Join-Path $PSScriptRoot "frontend\downloads"
$PackagePath = Join-Path $DownloadsDir "DeskStream-verified-x64.zip"
$SignatureVerifier = Join-Path $PSScriptRoot "scripts\verify-release-signature.ps1"

if (-not (Test-Path -LiteralPath $ReleaseExe -PathType Leaf)) {
    throw "Verified release executable was not found: $ReleaseExe"
}
if (-not (Test-Path -LiteralPath $CargoExe -PathType Leaf)) {
    throw "Fresh Cargo release executable was not found: $CargoExe"
}
if (-not (Test-Path -LiteralPath $SignatureVerifier -PathType Leaf)) {
    throw "Production signature verifier is missing: $SignatureVerifier"
}

$cargoHash = (Get-FileHash -LiteralPath $CargoExe -Algorithm SHA256).Hash
$releaseHash = (Get-FileHash -LiteralPath $ReleaseExe -Algorithm SHA256).Hash
if ($releaseHash -ne $cargoHash) {
    throw "Refusing to package a stale DeskStream.exe. The release copy does not match the current Cargo output."
}
& $SignatureVerifier -ExePath $ReleaseExe
if ($LASTEXITCODE -ne 0) {
    throw "Refusing to package an unsigned or untrusted DeskStream executable."
}
New-Item -ItemType Directory -Path $DownloadsDir -Force | Out-Null
$workDir = Join-Path ([System.IO.Path]::GetTempPath()) (
    "DeskStream-package-" + [Guid]::NewGuid().ToString("N")
)
$stagedExe = Join-Path $workDir "DeskStream.exe"
$temporaryZip = Join-Path $workDir "DeskStream-verified-x64.zip"
$temporaryPackage = Join-Path $DownloadsDir (".DeskStream-verified-x64-" + [Guid]::NewGuid().ToString("N") + ".zip")

try {
    New-Item -ItemType Directory -Path $workDir | Out-Null
    Copy-Item -LiteralPath $ReleaseExe -Destination $stagedExe

    $stagedHash = (Get-FileHash -LiteralPath $stagedExe -Algorithm SHA256).Hash
    if ($stagedHash -ne $releaseHash) {
        throw "Staged executable hash differs from the verified release. Expected $releaseHash; found $stagedHash."
    }

    Compress-Archive -LiteralPath $stagedExe -DestinationPath $temporaryZip -CompressionLevel Optimal

    Add-Type -AssemblyName System.IO.Compression
    $archive = [System.IO.Compression.ZipFile]::OpenRead($temporaryZip)
    try {
        if ($archive.Entries.Count -ne 1 -or $archive.Entries[0].FullName -ne "DeskStream.exe") {
            throw "Package contents are not exactly the expected DeskStream.exe."
        }

        $entryStream = $archive.Entries[0].Open()
        try {
            $entryHash = (Get-FileHash -InputStream $entryStream -Algorithm SHA256).Hash
        }
        finally {
            $entryStream.Dispose()
        }
    }
    finally {
        $archive.Dispose()
    }

    if ($entryHash -ne $releaseHash) {
        throw "Packaged executable hash differs from the verified release. Expected $releaseHash; found $entryHash."
    }

    Move-Item -LiteralPath $temporaryZip -Destination $temporaryPackage
    Move-Item -LiteralPath $temporaryPackage -Destination $PackagePath -Force
    $packageHash = (Get-FileHash -LiteralPath $PackagePath -Algorithm SHA256).Hash

    Write-Host "Package created: $PackagePath"
    Write-Host "Package SHA256: $packageHash"
    Write-Host "Packaged DeskStream.exe SHA256: $entryHash"
    Write-Host "Contents: DeskStream.exe (verified unified release)"
}
finally {
    if (Test-Path -LiteralPath $temporaryPackage) {
        Remove-Item -LiteralPath $temporaryPackage -Force
    }
    if (Test-Path -LiteralPath $workDir) {
        Remove-Item -LiteralPath $workDir -Recurse -Force
    }
}
