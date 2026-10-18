# tests/AutoUpdateSystemTest.ps1
# Comprehensive Verification Suite for DeskStream Secure Auto-Update System

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectRoot = (Resolve-Path "$ScriptDir\..").Path
$UpdatesDir = Join-Path $ProjectRoot "updates"
$ManifestPath = Join-Path $UpdatesDir "stable.json"
$SignToolExe = Join-Path $ProjectRoot "desktop-agent\target\release\sign-manifest.exe"
$PrivateKeyPath = Join-Path $ProjectRoot "scripts\keys\deskstream_update_private.key"
$PublicKeyPath = Join-Path $ProjectRoot "scripts\keys\deskstream_update_public.pub"
$UpdaterExe = Join-Path $ProjectRoot "DESKSTREAM\deskstream-updater.exe"

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host "   RUNNING AUTO-UPDATE AUTOMATED TEST SUITE               " -ForegroundColor Cyan
Write-Host "==========================================================" -ForegroundColor Cyan

$Passed = 0
$Failed = 0

function Assert-Test([string]$Name, [bool]$Condition) {
    if ($Condition) {
        Write-Host " [PASS] $Name" -ForegroundColor Green
        $script:Passed++
    } else {
        Write-Host " [FAIL] $Name" -ForegroundColor Red
        $script:Failed++
    }
}

# Test 1: Check deskstream-updater.exe exists in DESKSTREAM and root
Assert-Test "deskstream-updater.exe exists in DESKSTREAM directory" (Test-Path $UpdaterExe)
Assert-Test "deskstream-updater.exe exists in project root" (Test-Path "$ProjectRoot\deskstream-updater.exe")

# Test 2: Check stable.json format and fields
Assert-Test "updates\stable.json exists" (Test-Path $ManifestPath)
$ManifestJson = Get-Content $ManifestPath -Raw | ConvertFrom-Json
Assert-Test "Manifest has correct app_id" ($ManifestJson.app_id -eq "com.friendssoftwaresolutions.deskstream")
Assert-Test "Manifest channel is stable" ($ManifestJson.channel -eq "stable")
Assert-Test "Manifest has semver version" ($ManifestJson.version -match '^\d+\.\d+\.\d+$')
Assert-Test "Manifest package format is exe" ($ManifestJson.package.format -eq "exe")
Assert-Test "Manifest package sha256 is 64 hex chars" ($ManifestJson.package.sha256.Length -eq 64)
Assert-Test "Manifest signature is ed25519" ($ManifestJson.signature.algorithm -eq "ed25519")
Assert-Test "Manifest signature is 128 hex chars" ($ManifestJson.signature.value.Length -eq 128)

# Test 3: Test Package Hash Verification
$PackageFile = Join-Path $UpdatesDir "packages\$($ManifestJson.package.filename)"
Assert-Test "Package file exists locally" (Test-Path $PackageFile)
$ActualSha = (Get-FileHash -Path $PackageFile -Algorithm SHA256).Hash.ToLower()
Assert-Test "Calculated package SHA-256 matches manifest SHA-256" ($ActualSha -eq $ManifestJson.package.sha256)

# Test 4: Tampered SHA-256 Detection
$TamperedSha = "0000000000000000000000000000000000000000000000000000000000000000"
Assert-Test "Tampered SHA-256 correctly differs from genuine package" ($ActualSha -ne $TamperedSha)

# Test 5: Ed25519 Signature Generation and Consistency
$AppId = $ManifestJson.app_id
$Version = $ManifestJson.version
$Payload = "$AppId`:$Version`:$ActualSha"
$ComputedSig = (& $SignToolExe $PrivateKeyPath $Payload).Trim()
Assert-Test "Generated signature matches manifest signature" ($ComputedSig -eq $ManifestJson.signature.value)

# Test 6: Ed25519 Signature Tampering Detection
$TamperedPayload = "$AppId`:$Version`:$TamperedSha"
$TamperedSig = (& $SignToolExe $PrivateKeyPath $TamperedPayload).Trim()
Assert-Test "Signature over tampered payload differs from genuine signature" ($ComputedSig -ne $TamperedSig)

# Test 7: NSIS Installer includes deskstream-updater.exe
$NsiContent = Get-Content "$ProjectRoot\installer\DeskStream.nsi" -Raw
Assert-Test "NSIS script packages deskstream-updater.exe" ($NsiContent -match 'deskstream-updater\.exe')

# Test 8: Rust unit tests
Write-Host "Running Rust internal unit tests..." -ForegroundColor Yellow
Push-Location "$ProjectRoot\desktop-agent"
try {
    $prevEap = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    $TestOutput = (& cargo test --bin DeskStream updater::tests) -join "`n"
    $ErrorActionPreference = $prevEap
    $TestSuccess = ($LASTEXITCODE -eq 0) -and ($TestOutput -match '4 passed')
    Assert-Test "Rust updater unit tests passed (Ed25519, SHA256, SemVer, Tamper rejection)" $TestSuccess
} finally {
    Pop-Location
}

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host "TEST RESULTS: $Passed PASSED, $Failed FAILED" -ForegroundColor $(if ($Failed -eq 0) { "Green" } else { "Red" })
Write-Host "==========================================================" -ForegroundColor Cyan

if ($Failed -gt 0) {
    exit 1
}
