# ScreenShare-Setup.ps1
# Windows installer for Screen Share remote desktop application
# Builds release binaries and packages them into an installer

param(
    [string]$InstallDir = "$env:LOCALAPPDATA\DeskStream",
    [switch]$Uninstall
)

$ErrorActionPreference = "Stop"
$projectRoot = Split-Path -Parent $PSScriptRoot
$deployDir = "$InstallDir\bin"

if ($Uninstall) {
    Write-Host "======================================"
    Write-Host " DeskStream Uninstall"
    Write-Host "======================================"

    # Stop processes
    Get-Process -Name "desktop-agent" -ErrorAction SilentlyContinue | Stop-Process -Force
    Get-Process -Name "relay-server" -ErrorAction SilentlyContinue | Stop-Process -Force
    Get-Process -Name "ScreenShare-Tray" -ErrorAction SilentlyContinue | Stop-Process -Force

    # Remove auto-start
    Remove-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run" -Name "DeskStreamRelay" -ErrorAction SilentlyContinue
    Remove-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run" -Name "DeskStreamTray" -ErrorAction SilentlyContinue

    # Remove protocol handler
    Remove-Item -Path "HKCU:\Software\Classes\deskstream" -Recurse -Force -ErrorAction SilentlyContinue

    # Remove firewall rule
    Remove-NetFirewallRule -DisplayName "DeskStream Relay 9001" -ErrorAction SilentlyContinue

    # Remove installed files
    if (Test-Path $deployDir) {
        Remove-Item -Path $deployDir -Recurse -Force -ErrorAction SilentlyContinue
    }

    Write-Host "Uninstallation complete."
    Write-Host "======================================"
    return
}

# Self-Elevate
if (-not ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Host "Requesting administrator privileges..."
    Start-Process powershell -Verb RunAs -ArgumentList "-NoProfile -ExecutionPolicy Bypass -File `"$PSCommandPath`"" -Wait
    exit
}

Write-Host "======================================"
Write-Host " DeskStream Installer"
Write-Host "======================================"

# Step 1: Build release binaries
Write-Host "[1/5] Building release binaries..."

Push-Location "$projectRoot\desktop-agent"
cargo build --release --bin desktop-agent
if ($LASTEXITCODE -ne 0) { Write-Error "Agent build FAILED."; exit 1 }
cargo build --release --bin viewer
if ($LASTEXITCODE -ne 0) { Write-Error "Viewer build FAILED."; exit 1 }
Pop-Location

Push-Location "$projectRoot\relay-server"
cargo build --release
if ($LASTEXITCODE -ne 0) { Write-Error "Relay build FAILED."; exit 1 }
Pop-Location

Write-Host "      Build OK"

# Step 2: Stop running instances
Write-Host "[2/5] Stopping running instances..."
Get-Process -Name "desktop-agent" -ErrorAction SilentlyContinue | Stop-Process -Force
Get-Process -Name "relay-server" -ErrorAction SilentlyContinue | Stop-Process -Force
Get-Process -Name "ScreenShare-Tray" -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 500

# Step 3: Create directories and copy binaries
Write-Host "[3/5] Installing to $deployDir..."
if (-not (Test-Path $deployDir)) { New-Item -ItemType Directory -Path $deployDir -Force | Out-Null }

$agentExe = "$projectRoot\desktop-agent\target\release\desktop-agent.exe"
$viewerExe = "$projectRoot\desktop-agent\target\release\viewer.exe"
$relayExe = "$projectRoot\relay-server\target\release\relay-server.exe"
$trayScript = "$projectRoot\desktop-agent\ScreenShare-Tray.ps1"

Copy-Item $agentExe "$deployDir\desktop-agent.exe" -Force
Copy-Item $viewerExe "$deployDir\viewer.exe" -Force
Copy-Item $relayExe "$deployDir\relay-server.exe" -Force
Copy-Item $trayScript "$deployDir\ScreenShare-Tray.ps1" -Force

# Copy web dashboard to a local webserver-friendly location (optional)
# Users can point their browser to http://localhost:8080/dashboard.php

# Step 4: Register auto-start and protocol handler
Write-Host "[4/5] Registering auto-start and firewall..."

# Start with Windows (tray launcher)
$wshell = New-Object -ComObject WScript.Shell
$startupFolder = [Environment]::GetFolderPath("Startup")
$shortcutPath = "$startupFolder\ScreenShare.lnk"
$shell = New-Object -ComObject WScript.Shell
$shortcut = $shell.CreateShortcut($shortcutPath)
$shortcut.TargetPath = "powershell.exe"
$shortcut.Arguments = "-WindowStyle Hidden -File `"$deployDir\ScreenShare-Tray.ps1`""
$shortcut.WorkingDirectory = $deployDir
$shortcut.WindowStyle = 7
$shortcut.Save()

# Relay auto-start
New-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run" -Name "DeskStreamRelay" -Value "`"$deployDir\relay-server.exe`"" -PropertyType String -Force | Out-Null

# Firewall rule for relay port 9001
$existingRule = Get-NetFirewallRule -DisplayName "DeskStream Relay 9001" -ErrorAction SilentlyContinue
if ($existingRule) {
    Write-Host "      Firewall rule already present."
} else {
    New-NetFirewallRule -DisplayName "DeskStream Relay 9001" -Direction Inbound -Protocol TCP -LocalPort 9001 -Action Allow -Profile Any | Out-Null
    Write-Host "      Firewall rule created: TCP 9001 Inbound Allow"
}

# Step 5: Start services
Write-Host "[5/5] Starting services..."
Start-Process -FilePath $relayExe -WindowStyle Hidden
Start-Sleep -Seconds 1
Start-Process -FilePath $agentExe -WindowStyle Hidden
Start-Sleep -Seconds 3
Start-Process powershell -ArgumentList "-WindowStyle Hidden -File `"$deployDir\ScreenShare-Tray.ps1`"" -WindowStyle Hidden

$proc1 = Get-Process -Name "desktop-agent" -ErrorAction SilentlyContinue
$proc2 = Get-Process -Name "relay-server" -ErrorAction SilentlyContinue
if ($proc1 -and $proc2) {
    Write-Host "      Both running: Agent PID=$($proc1.Id), Relay PID=$($proc2.Id)"
} else {
    Write-Warning "One or both did not start!"
}

Write-Host "======================================"
Write-Host " Installation Complete!"
Write-Host " Dashboard: http://localhost:8080/dashboard.php"
Write-Host " Installed to: $deployDir"
Write-Host "======================================"
