# build_release.ps1
# Builds the desktop agent, the Screen Share GUI, and the self-contained installer.
# Outputs:
#   desktop-agent/target/release/desktop-agent.exe
#   screenshare-gui/target/release/ScreenShare.exe
#   screenshare-setup/target/release/screenshare-setup.exe  ->  installer/ScreenShare-Setup.exe
#                                                 and  ->  frontend/downloads/ScreenShare-Setup.exe

$ErrorActionPreference = "Stop"
$Root = $PSScriptRoot

function Build($name, $dir) {
    Write-Host "========================================"
    Write-Host " Building $name"
    Write-Host "========================================"
    Push-Location (Join-Path $Root $dir)
    cargo build --release
    if ($LASTEXITCODE -ne 0) { Write-Error "$name build FAILED."; exit 1 }
    Pop-Location
}

# 1. Agent (existing video / control pipeline — unchanged)
Build "desktop-agent" "desktop-agent"

# 2. GUI (native Win32 wrapper around the agent)
Build "screenshare-gui" "screenshare-gui"

# 3. Copy built binaries to staging directory
$stagingDir = Join-Path $Root "installer_staging"
if (Test-Path $stagingDir) { Remove-Item -Recurse -Force $stagingDir }
New-Item -ItemType Directory -Path $stagingDir -Force | Out-Null

$agentSrc = Join-Path $Root "desktop-agent\target\release\desktop-agent.exe"
$guiSrc = Join-Path $Root "screenshare-gui\target\release\ScreenShare.exe"
$relaySrc = Join-Path $Root "relay-server\target\release\relay-server.exe"

if (-not (Test-Path $agentSrc)) { Write-Error "ERROR: Fresh desktop-agent.exe was not found. Installer build aborted."; exit 1 }
if (-not (Test-Path $guiSrc)) { Write-Error "ERROR: Fresh ScreenShare.exe was not found. Installer build aborted."; exit 1 }
if (-not (Test-Path $relaySrc)) { Write-Error "ERROR: Fresh relay-server.exe was not found. Installer build aborted."; exit 1 }

Copy-Item $agentSrc -Destination "$stagingDir\desktop-agent.exe" -Force
Copy-Item $guiSrc -Destination "$stagingDir\ScreenShare.exe" -Force
Copy-Item $relaySrc -Destination "$stagingDir\relay-server.exe" -Force

# 4. Verify copies using SHA256
$agentHashSrc = (Get-FileHash $agentSrc -Algorithm SHA256).Hash
$agentHashDst = (Get-FileHash "$stagingDir\desktop-agent.exe" -Algorithm SHA256).Hash
if ($agentHashSrc -ne $agentHashDst) { Write-Error "ERROR: Installer contains a different Desktop Agent binary. Build aborted."; exit 1 }

# 5. Installer (embeds the binaries from staging)
Build "screenshare-setup" "screenshare-setup"

# 4. Publish the installer
$setup = Join-Path $Root "screenshare-setup\target\release\screenshare-setup.exe"
$dst1  = Join-Path $Root "installer\ScreenShare-Setup.exe"
$dst2  = Join-Path $Root "frontend\downloads\ScreenShare-Setup.exe"

New-Item -ItemType Directory -Force -Path (Split-Path $dst1) | Out-Null
New-Item -ItemType Directory -Force -Path (Split-Path $dst2) | Out-Null
Copy-Item $setup $dst1 -Force
Copy-Item $setup $dst2 -Force

Write-Host ""
Write-Host "========================================"
Write-Host " DONE"
Write-Host "   Agent : desktop-agent\target\release\desktop-agent.exe"
Write-Host "   GUI   : screenshare-gui\target\release\ScreenShare.exe"
Write-Host "   Setup : installer\ScreenShare-Setup.exe"
Write-Host "          frontend\downloads\ScreenShare-Setup.exe"
Write-Host "========================================"
