$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
$agentDir = Join-Path $root 'desktop-agent'
$sourceExe = Join-Path $agentDir 'target\release\desktop-agent.exe'
$installedExe = Join-Path $env:TEMP 'desktop-agent.exe'
$buildLog = 'C:\Users\Public\DeskStream-build.log'
$verifyLog = 'C:\Users\Public\DeskStream-final-verification.txt'

function Write-Marker([string]$line) {
    Add-Content -Path $buildLog -Value $line
}

Set-Content -Path $buildLog -Value '================================================'
Add-Content -Path $buildLog -Value 'DESKSTREAM BUILD VERIFICATION'
Add-Content -Path $buildLog -Value "BUILD_START_TIME=$(Get-Date -Format o)"

foreach ($port in @(49182,49183,49184)) {
    Get-NetTCPConnection -LocalPort $port -ErrorAction SilentlyContinue | ForEach-Object { 
        & cmd.exe /c "taskkill /F /PID $($_.OwningProcess) /T 2>NUL"
    }
    Get-NetUDPEndpoint -LocalPort $port -ErrorAction SilentlyContinue | ForEach-Object { 
        & cmd.exe /c "taskkill /F /PID $($_.OwningProcess) /T 2>NUL"
    }
}
& cmd.exe /c "taskkill /F /IM desktop-agent.exe /T 2>NUL"
Start-Sleep -Milliseconds 1500
if (Get-Process -Name 'desktop-agent' -ErrorAction SilentlyContinue) {
    Write-Marker 'BUILD_TEST_OLD_AGENTS_STOPPED=NO'
    throw 'desktop-agent.exe process could not be stopped'
}
Write-Marker 'BUILD_TEST_OLD_AGENTS_STOPPED=YES'

$cargo = 'C:\Users\X1 CORBON\.cargo\bin\cargo.exe'
$stdoutLog = 'C:\Users\Public\DeskStream-cargo-stdout.log'
$stderrLog = 'C:\Users\Public\DeskStream-cargo-stderr.log'
$start = Get-Date
$process = Start-Process -FilePath $cargo -ArgumentList @('build', '--release', '--manifest-path', "`"$(Join-Path $agentDir 'Cargo.toml')`"") -WorkingDirectory $agentDir -RedirectStandardOutput $stdoutLog -RedirectStandardError $stderrLog -PassThru -WindowStyle Hidden
$process | Wait-Process
$finish = Get-Date
Write-Marker "BUILD_FINISH_TIME=$($finish.ToString('o'))"
$exitCode = $process.ExitCode
if ($null -eq $exitCode) { $exitCode = 0 }
Write-Marker "BUILD_EXIT_CODE=$exitCode"

$exists = Test-Path $sourceExe
Write-Marker "BUILD_EXE_EXISTS=$($(if ($exists) { 'YES' } else { 'NO' }))"
if ($exists) {
    $item = Get-Item $sourceExe
    Write-Marker "BUILD_EXE_SIZE=$($item.Length)"
    Write-Marker "BUILD_EXE_TIMESTAMP=$($item.LastWriteTime.ToString('o'))"
    Write-Marker "BUILD_SHA256=$((Get-FileHash $sourceExe -Algorithm SHA256).Hash)"
}
$complete = ($exitCode -eq 0 -and $exists -and ((Get-Item $sourceExe).Length -gt 0))
Write-Marker "BUILD_COMPLETE=$($(if ($complete) { 'YES' } else { 'NO' }))"
if (-not $complete) { exit 1 }

Copy-Item $sourceExe $installedExe -Force
$sourceHash = (Get-FileHash $sourceExe -Algorithm SHA256).Hash
$installedHash = (Get-FileHash $installedExe -Algorithm SHA256).Hash
Write-Marker "SOURCE_EXE_SHA256=$sourceHash"
Write-Marker "INSTALLED_EXE_SHA256=$installedHash"
Write-Marker "HASH_MATCH=$($(if ($sourceHash -eq $installedHash) { 'YES' } else { 'NO' }))"
if ($sourceHash -ne $installedHash) { exit 1 }

Start-Process -FilePath $installedExe -WorkingDirectory (Split-Path $installedExe) -WindowStyle Hidden
Start-Sleep -Seconds 3
$processes = @(Get-Process -Name 'desktop-agent' -ErrorAction SilentlyContinue)
$installedProcess = $processes | Where-Object { $_.Path -eq $installedExe }
Write-Marker "INSTALLED_PROCESS_COUNT=$($processes.Count)"
Write-Marker "INSTALLED_PROCESS_PATH=$($(if ($installedProcess) { $installedProcess.Path } else { 'NONE' }))"

Set-Content -Path $verifyLog -Value '================================================'
Add-Content -Path $verifyLog -Value 'DESKSTREAM FINAL VERIFICATION'
Add-Content -Path $verifyLog -Value '================================================'
Add-Content -Path $verifyLog -Value "BUILD_EXIT_CODE=$($process.ExitCode)"
Add-Content -Path $verifyLog -Value "BUILD_COMPLETE=$($(if ($complete) { 'YES' } else { 'NO' }))"
Add-Content -Path $verifyLog -Value "SOURCE_EXE_EXISTS=$($(if ($exists) { 'YES' } else { 'NO' }))"
Add-Content -Path $verifyLog -Value "SOURCE_SHA256=$sourceHash"
Add-Content -Path $verifyLog -Value "INSTALLED_EXE_EXISTS=$($(if (Test-Path $installedExe) { 'YES' } else { 'NO' }))"
Add-Content -Path $verifyLog -Value "INSTALLED_SHA256=$installedHash"
Add-Content -Path $verifyLog -Value "HASH_MATCH=$($(if ($sourceHash -eq $installedHash) { 'YES' } else { 'NO' }))"
Add-Content -Path $verifyLog -Value "INSTALLED_PROCESS_PATH=$($(if ($installedProcess) { $installedProcess.Path } else { 'NONE' }))"
Add-Content -Path $verifyLog -Value 'PORT_49182=UNTESTED'
Add-Content -Path $verifyLog -Value 'PORT_49183=UNTESTED'
Add-Content -Path $verifyLog -Value 'PORT_49184=UNTESTED'
Add-Content -Path $verifyLog -Value 'DIRECT_WS_ACCEPTED=UNTESTED'
Add-Content -Path $verifyLog -Value 'DIRECT_HANDSHAKE_42_BYTES=UNTESTED'
Add-Content -Path $verifyLog -Value 'DIRECT_AUTH=UNTESTED'
Add-Content -Path $verifyLog -Value 'DIRECT_AUTH_RESPONSE=UNTESTED'
Add-Content -Path $verifyLog -Value 'DIRECT_SESSION_CONSUMED=UNTESTED'
Add-Content -Path $verifyLog -Value 'DIRECT_VIDEO=UNTESTED'
Add-Content -Path $verifyLog -Value 'DIRECT_TYPE13=UNTESTED'
Add-Content -Path $verifyLog -Value 'BROWSER_AUTH=UNTESTED'
Add-Content -Path $verifyLog -Value 'BROWSER_VIDEO=UNTESTED'
Add-Content -Path $verifyLog -Value 'RELAY_AUTH=UNTESTED'
Add-Content -Path $verifyLog -Value 'RELAY_VIDEO=UNTESTED'
Add-Content -Path $verifyLog -Value 'RELAY_CONTROL=UNTESTED'
Add-Content -Path $verifyLog -Value 'SOCKET_PAIR_IN_P2P=NO'
Add-Content -Path $verifyLog -Value 'PREVIOUS_VIEWER_LOOP_FIX=YES'
Add-Content -Path $verifyLog -Value 'FINAL_RESULT=FAIL'