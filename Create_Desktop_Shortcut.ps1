$WshShell = New-Object -comObject WScript.Shell
$DesktopPath = [System.Environment]::GetFolderPath('Desktop')
$TargetExe = Join-Path $PSScriptRoot "dist\DeskStream.exe"
$ShortcutPath = Join-Path $DesktopPath "DeskStream Remote Desktop.lnk"

if (Test-Path $TargetExe) {
    $Shortcut = $WshShell.CreateShortcut($ShortcutPath)
    $Shortcut.TargetPath = $TargetExe
    $Shortcut.WorkingDirectory = (Split-Path -Parent $TargetExe)
    $Shortcut.Description = "DeskStream Pro — 120 FPS Ultra-Low Latency Remote Desktop"
    $Shortcut.Save()
    Write-Host "[OK] Desktop shortcut created at: $ShortcutPath" -ForegroundColor Green
} else {
    Write-Host "[WARN] $TargetExe not found yet. Run build_and_package.bat first." -ForegroundColor Yellow
}
