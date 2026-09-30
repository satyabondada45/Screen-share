# DeskStream Agent Installer
$ErrorActionPreference = "Stop"

$DeployDir = "$env:LOCALAPPDATA\DeskStream\bin"
$DeployedExe = "$DeployDir\desktop-agent.exe"

Write-Host "Installing DeskStream Desktop Agent..."

# 1. Stop if running
Write-Host "- Stopping existing agent..."
Get-Process -Name desktop-agent -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1

# 2. Copy binary
Write-Host "- Copying files..."
if (-not (Test-Path $DeployDir)) { New-Item -ItemType Directory -Path $DeployDir | Out-Null }
Copy-Item "$PSScriptRoot\desktop-agent.exe" $DeployedExe -Force

# 3. Register Custom Protocol (deskstream://)
Write-Host "- Registering custom protocol..."
New-Item -Path "HKCU:\Software\Classes\deskstream" -Force | Out-Null
New-ItemProperty -Path "HKCU:\Software\Classes\deskstream" -Name "(Default)" -Value "URL:DeskStream Protocol" -PropertyType String -Force | Out-Null
New-ItemProperty -Path "HKCU:\Software\Classes\deskstream" -Name "URL Protocol" -Value "" -PropertyType String -Force | Out-Null
New-Item -Path "HKCU:\Software\Classes\deskstream\shell\open\command" -Force | Out-Null
New-ItemProperty -Path "HKCU:\Software\Classes\deskstream\shell\open\command" -Name "(Default)" -Value ""$DeployedExe" "%1"" -PropertyType String -Force | Out-Null

# 4. Firewall rule - allow TCP 9001 inbound (so this laptop can connect to relay)
Write-Host "- Adding firewall rule for TCP 9001..."
$fwRule = Get-NetFirewallRule -DisplayName "DeskStream Relay 9001" -ErrorAction SilentlyContinue
if (-not $fwRule) {
    New-NetFirewallRule -DisplayName "DeskStream Relay 9001" -Direction Inbound -Protocol TCP -LocalPort 9001 -Action Allow -Profile Any | Out-Null
    Write-Host "  Firewall rule created."
}

# 5. Register Auto-Start
Write-Host "- Configuring auto-start on login..."
New-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run" -Name "DeskStreamAgent" -Value ""$DeployedExe" "%1"" -PropertyType String -Force | Out-Null

# 5. Start Agent
Write-Host "- Starting agent..."
Start-Process -FilePath $DeployedExe -WindowStyle Hidden

Write-Host "======================================"
Write-Host "Installation Complete! You can now use the DeskStream Dashboard."
Write-Host "======================================"
Start-Sleep -Seconds 3
