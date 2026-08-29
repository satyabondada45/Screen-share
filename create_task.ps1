# Ensure script is running as Administrator
if (!([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Start-Process PowerShell -Verb RunAs "-NoProfile -ExecutionPolicy Bypass -Command `"cd '$PSScriptRoot'; & '$PSCommandPath'`"";
    exit;
}

# Remove old/broken task if it exists
Unregister-ScheduledTask -TaskName "SpaceAgentTask" -Confirm:$false -ErrorAction SilentlyContinue

# Create a new scheduled task that runs on user logon
$deployedPath = "C:\Users\$env:USERNAME\AppData\Local\DeskStream\bin\desktop-agent.exe"
$workDir = "C:\Users\$env:USERNAME\AppData\Local\DeskStream\bin"

$action = New-ScheduledTaskAction -Execute $deployedPath -WorkingDirectory $workDir
$trigger = New-ScheduledTaskTrigger -AtLogOn -User $env:USERNAME
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable

Register-ScheduledTask -TaskName "DeskStreamAgentLogon" -Action $action -Trigger $trigger -Settings $settings -Description "DeskStream Remote Desktop Agent - starts on user logon"

Write-Host "Scheduled task 'DeskStreamAgentLogon' created successfully."
Write-Host "The agent will start automatically on next user logon."
Write-Host ""
Write-Host "To start now, run: Start-ScheduledTask -TaskName 'DeskStreamAgentLogon'"
