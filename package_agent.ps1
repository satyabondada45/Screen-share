# Packages the verified unified DeskStream release without rebuilding it.

$ErrorActionPreference = "Stop"

$ReleaseExe = Join-Path $PSScriptRoot "DESKSTREAM\DeskStream.exe"
$DownloadsDir = Join-Path $PSScriptRoot "frontend\downloads"
$PackagePath = Join-Path $DownloadsDir "DeskStream-verified-x64.zip"
$ExpectedExeSha256 = "AD3CF3AF97A83FDF93CFCCCF82828C2113740BE427B42A579A4AF78EC660825F"

if (-not (Test-Path -LiteralPath $ReleaseExe -PathType Leaf)) {
    throw "Verified release executable was not found: $ReleaseExe"
}

$releaseHash = (Get-FileHash -LiteralPath $ReleaseExe -Algorithm SHA256).Hash
if ($releaseHash -ne $ExpectedExeSha256) {
    throw "Release executable hash mismatch. Expected $ExpectedExeSha256; found $releaseHash."
}

if (Test-Path -LiteralPath $PackagePath) {
    throw "Refusing to overwrite an existing package: $PackagePath"
}

New-Item -ItemType Directory -Path $DownloadsDir -Force | Out-Null
$workDir = Join-Path ([System.IO.Path]::GetTempPath()) (
    "DeskStream-package-" + [Guid]::NewGuid().ToString("N")
)
$stagedExe = Join-Path $workDir "DeskStream.exe"
$temporaryZip = Join-Path $workDir "DeskStream-verified-x64.zip"

try {
    New-Item -ItemType Directory -Path $workDir | Out-Null
    Copy-Item -LiteralPath $ReleaseExe -Destination $stagedExe

    $stagedHash = (Get-FileHash -LiteralPath $stagedExe -Algorithm SHA256).Hash
    if ($stagedHash -ne $ExpectedExeSha256) {
        throw "Staged executable hash mismatch. Expected $ExpectedExeSha256; found $stagedHash."
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

    if ($entryHash -ne $ExpectedExeSha256) {
        throw "Packaged executable hash mismatch. Expected $ExpectedExeSha256; found $entryHash."
    }

    Move-Item -LiteralPath $temporaryZip -Destination $PackagePath
    $packageHash = (Get-FileHash -LiteralPath $PackagePath -Algorithm SHA256).Hash

    Write-Host "Package created: $PackagePath"
    Write-Host "Package SHA256: $packageHash"
    Write-Host "Packaged DeskStream.exe SHA256: $entryHash"
    Write-Host "Contents: DeskStream.exe (verified unified release)"
}
finally {
    if (Test-Path -LiteralPath $workDir) {
        Remove-Item -LiteralPath $workDir -Recurse -Force
    }
}
