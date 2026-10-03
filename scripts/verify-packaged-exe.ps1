param (
    [Parameter(Mandatory = $true)]
    [string]$SourcePath,
    [Parameter(Mandatory = $true)]
    [string]$StagedPath,
    [Parameter(Mandatory = $true)]
    [string]$InstallerPath,
    [ValidateSet("Embedded", "NSIS")]
    [string]$InstallerFormat = "Embedded"
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path -LiteralPath $SourcePath -PathType Leaf)) {
    throw "Fresh Cargo executable is missing: $SourcePath"
}
if (-not (Test-Path -LiteralPath $StagedPath -PathType Leaf)) {
    throw "Installer staging executable is missing: $StagedPath"
}
if (-not (Test-Path -LiteralPath $InstallerPath -PathType Leaf)) {
    throw "Final installer is missing: $InstallerPath"
}

$sourceHash = (Get-FileHash -LiteralPath $SourcePath -Algorithm SHA256).Hash
$stagedHash = (Get-FileHash -LiteralPath $StagedPath -Algorithm SHA256).Hash
if ($sourceHash -ne $stagedHash) {
    throw "Fresh Cargo executable and installer staging SHA256 values do not match."
}

$sourceBytes = [System.IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $SourcePath).Path)
$extractionDir = $null
$embeddedPath = $null
$previousVerifyDir = $env:DESKSTREAM_VERIFY_DIR
try {
    if ($InstallerFormat -eq "NSIS") {
        $extractionDir = Join-Path $env:TEMP "DeskStream-packaged-payload-$PID"
        $embeddedPath = Join-Path $extractionDir "DeskStream.exe"
        New-Item -ItemType Directory -Path $extractionDir -Force | Out-Null
        $env:DESKSTREAM_VERIFY_DIR = $extractionDir
        $process = Start-Process -FilePath $InstallerPath `
            -ArgumentList "/VERIFY-PAYLOAD=1" `
            -Wait -PassThru -WindowStyle Hidden
        if ($process.ExitCode -ne 0) {
            throw "Setup payload extraction exited with code $($process.ExitCode)."
        }
        if (-not (Test-Path -LiteralPath $embeddedPath -PathType Leaf)) {
            throw "Final installer did not extract its DeskStream.exe payload."
        }
    } else {
        Add-Type -TypeDefinition @"
using System;

public static class DeskStreamPayloadSearch
{
    public static int Find(byte[] data, byte[] pattern)
    {
        if (pattern == null || pattern.Length == 0) return 0;
        if (data == null || pattern.Length > data.Length) return -1;

        var prefix = new int[pattern.Length];
        for (int i = 1, matched = 0; i < pattern.Length; i++)
        {
            while (matched > 0 && pattern[i] != pattern[matched])
                matched = prefix[matched - 1];
            if (pattern[i] == pattern[matched]) matched++;
            prefix[i] = matched;
        }

        for (int i = 0, matched = 0; i < data.Length; i++)
        {
            while (matched > 0 && data[i] != pattern[matched])
                matched = prefix[matched - 1];
            if (data[i] == pattern[matched]) matched++;
            if (matched == pattern.Length) return i - pattern.Length + 1;
        }
        return -1;
    }
}
"@
        $installerBytes = [System.IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $InstallerPath).Path)
        $payloadOffset = [DeskStreamPayloadSearch]::Find($installerBytes, $sourceBytes)
        if ($payloadOffset -lt 0) {
            throw "Final installer does not contain the exact current Cargo executable payload."
        }
        $embeddedPath = Join-Path $env:TEMP "DeskStream-packaged-payload-$PID.exe"
        $embeddedBytes = New-Object byte[] $sourceBytes.Length
        [Array]::Copy($installerBytes, $payloadOffset, $embeddedBytes, 0, $sourceBytes.Length)
        [System.IO.File]::WriteAllBytes($embeddedPath, $embeddedBytes)
    }

    $embeddedHash = (Get-FileHash -LiteralPath $embeddedPath -Algorithm SHA256).Hash

    if ($embeddedHash -ne $sourceHash) {
        throw "Extracted Setup payload SHA256 differs from the current Cargo executable."
    }

    $sourceTimestamp = (Get-Item -LiteralPath $SourcePath).LastWriteTimeUtc.ToString("o")
    $stagedTimestamp = (Get-Item -LiteralPath $StagedPath).LastWriteTimeUtc.ToString("o")
    $embeddedTimestamp = (Get-Item -LiteralPath $embeddedPath).LastWriteTimeUtc.ToString("o")
    $installerTimestamp = (Get-Item -LiteralPath $InstallerPath).LastWriteTimeUtc.ToString("o")
    $peTimestamp = "unavailable"
    if ($sourceBytes.Length -ge 64) {
        $peOffset = [BitConverter]::ToInt32($sourceBytes, 0x3c)
        if ($peOffset -ge 0 -and $peOffset + 12 -le $sourceBytes.Length) {
            $seconds = [BitConverter]::ToUInt32($sourceBytes, $peOffset + 8)
            try {
                $peTimestamp = [DateTimeOffset]::FromUnixTimeSeconds($seconds).UtcDateTime.ToString("o")
            } catch {
                $peTimestamp = "invalid"
            }
        }
    }

    Write-Host "[PAYLOAD VERIFIED] Cargo EXE: $SourcePath"
    Write-Host "[PAYLOAD VERIFIED] Cargo EXE timestamp (UTC): $sourceTimestamp"
    Write-Host "[PAYLOAD VERIFIED] Cargo EXE SHA256: $sourceHash"
    Write-Host "[PAYLOAD VERIFIED] Staging EXE: $StagedPath"
    Write-Host "[PAYLOAD VERIFIED] Staging timestamp (UTC): $stagedTimestamp"
    Write-Host "[PAYLOAD VERIFIED] Staging SHA256: $stagedHash"
    Write-Host "[PAYLOAD VERIFIED] Extracted Setup EXE timestamp (UTC): $embeddedTimestamp"
    Write-Host "[PAYLOAD VERIFIED] Embedded EXE SHA256: $embeddedHash"
    Write-Host "[PAYLOAD VERIFIED] Embedded PE timestamp (UTC): $peTimestamp"
    Write-Host "[PAYLOAD VERIFIED] Final Setup: $InstallerPath"
    Write-Host "[PAYLOAD VERIFIED] Setup timestamp (UTC): $installerTimestamp"
    Write-Host "[PAYLOAD VERIFIED] Setup SHA256: $((Get-FileHash -LiteralPath $InstallerPath -Algorithm SHA256).Hash)"
} finally {
    $env:DESKSTREAM_VERIFY_DIR = $previousVerifyDir
    if ($extractionDir -and (Test-Path -LiteralPath $extractionDir)) {
        Remove-Item -LiteralPath $extractionDir -Recurse -Force
    }
    if ($embeddedPath -and (Test-Path -LiteralPath $embeddedPath)) {
        Remove-Item -LiteralPath $embeddedPath -Force
    }
}
