# deploy_agent.ps1
# Builds the latest release binary and deploys it to the auto-start location.
# Run this whenever you make changes to the desktop-agent source code.

# --- Self-Elevation (require admin for firewall rule) ---
if (-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Host "Restarting as Administrator..."
    Start-Process powershell -Verb RunAs -ArgumentList "-NoProfile -ExecutionPolicy Bypass -File `"$PSCommandPath`"" -Wait
    exit
}

$ErrorActionPreference = "Stop"

$SourceDirAgent = "$PSScriptRoot\desktop-agent"
$SourceDirRelay = "$PSScriptRoot\relay-server"
$ReleaseExeAgent = "$SourceDirAgent\target\release\desktop-agent.exe"
$ReleaseExeRelay = "$SourceDirRelay\target\release\relay-server.exe"

$DeployDir    = "$env:LOCALAPPDATA\DeskStream\bin"
$DeployedExeAgent  = "$DeployDir\desktop-agent.exe"
$DeployedExeRelay  = "$DeployDir\relay-server.exe"

Write-Host "======================================"
Write-Host " DeskStream Deploy Script"
Write-Host "======================================"

# Step 1: Build
Write-Host "[1/4] Building release binaries..."
Push-Location $SourceDirAgent
cargo build --release
if ($LASTEXITCODE -ne 0) { Write-Error "Agent Build FAILED."; exit 1 }
Pop-Location

Push-Location $SourceDirRelay
cargo build --release
if ($LASTEXITCODE -ne 0) { Write-Error "Relay Build FAILED."; exit 1 }
Pop-Location
Write-Host "      Build OK"

# Step 2: Stop any running instances
Write-Host "[2/4] Stopping running agent and relay (if any)..."
Get-Process -Name desktop-agent -ErrorAction SilentlyContinue | Stop-Process -Force
Get-Process -Name relay-server -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 500

# Step 3: Copy binary and register protocol
Write-Host "[3/4] Deploying to $DeployDir ..."
if (-not (Test-Path $DeployDir)) { New-Item -ItemType Directory -Path $DeployDir | Out-Null }
Copy-Item $ReleaseExeAgent $DeployedExeAgent -Force
Copy-Item $ReleaseExeRelay $DeployedExeRelay -Force

Write-Host "Registering deskstream:// protocol handler..."
New-Item -Path "HKCU:\Software\Classes\deskstream" -Force | Out-Null
New-ItemProperty -Path "HKCU:\Software\Classes\deskstream" -Name "(Default)" -Value "URL:DeskStream Protocol" -PropertyType String -Force | Out-Null
New-ItemProperty -Path "HKCU:\Software\Classes\deskstream" -Name "URL Protocol" -Value "" -PropertyType String -Force | Out-Null
New-Item -Path "HKCU:\Software\Classes\deskstream\shell\open\command" -Force | Out-Null
New-ItemProperty -Path "HKCU:\Software\Classes\deskstream\shell\open\command" -Name "(Default)" -Value "`"$DeployedExeAgent`" `"%1`"" -PropertyType String -Force | Out-Null

Write-Host "      Deploy & Registration OK"

# Step 3b: Firewall rule for relay port 9001
Write-Host "Ensuring Windows Firewall allows TCP 9001 (relay)..."
$existingRule = Get-NetFirewallRule -DisplayName "DeskStream Relay 9001" -ErrorAction SilentlyContinue
if ($existingRule) {
    Write-Host "      Firewall rule already present."
} else {
    New-NetFirewallRule -DisplayName "DeskStream Relay 9001" -Direction Inbound -Protocol TCP -LocalPort 9001 -Action Allow -Profile Any | Out-Null
    Write-Host "      Firewall rule created: TCP 9001 Inbound Allow"
}

# Step 3c: Register relay auto-start
Write-Host "Configuring relay auto-start on login..."
New-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run" -Name "DeskStreamRelay" -Value "`"$DeployedExeRelay`"" -PropertyType String -Force | Out-Null

# Step 4: Restart agent
Write-Host "[4/4] Starting relay and agent..."
Start-Process -FilePath $DeployedExeRelay -WindowStyle Hidden
Start-Sleep -Seconds 1
Start-Process -FilePath $DeployedExeAgent -WindowStyle Hidden
Start-Sleep -Seconds 3
$proc1 = Get-Process -Name desktop-agent -ErrorAction SilentlyContinue
$proc2 = Get-Process -Name relay-server -ErrorAction SilentlyContinue
if ($proc1 -and $proc2) {
    Write-Host "      Both running: Agent PID=$($proc1.Id), Relay PID=$($proc2.Id)"
} else {
    Write-Warning "One or both did not start!"
}

Write-Host "======================================"
Write-Host " Done."
Write-Host "======================================"
