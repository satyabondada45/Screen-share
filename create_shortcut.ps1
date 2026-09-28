$WshShell = New-Object -comObject WScript.Shell
$DesktopPath = [Environment]::GetFolderPath('Desktop')
$ShortcutPath = "$DesktopPath\DeskStream.lnk"

# Delete existing shortcut if present
if (Test-Path $ShortcutPath) {
    Remove-Item $ShortcutPath -Force
}

# Delete existing EXE on Desktop if present
if (Test-Path "$DesktopPath\DeskStream.exe") {
    Remove-Item "$DesktopPath\DeskStream.exe" -Force
}

$Shortcut = $WshShell.CreateShortcut($ShortcutPath)
$Shortcut.TargetPath = "C:\xampp\htdocs\Screen Share\DESKSTREAM\DeskStream.exe"
$Shortcut.WorkingDirectory = "C:\xampp\htdocs\Screen Share\DESKSTREAM"
# Embedded icon from EXE
$Shortcut.IconLocation = "C:\xampp\htdocs\Screen Share\DESKSTREAM\DeskStream.exe, 0"
$Shortcut.Save()
Write-Host "Shortcut created at $ShortcutPath"
