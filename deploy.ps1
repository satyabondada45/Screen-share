Write-Host "Waiting for cargo to finish..."
while (Get-Process cargo -ErrorAction SilentlyContinue) { Start-Sleep -Seconds 2 }

Write-Host "
--- REPLACEMENT PROCESS ---"

$targetExe = "C:\xampp\htdocs\Screen Share\desktop-agent\target\release\DeskStream.exe"
$deployExe = "C:\xampp\htdocs\Screen Share\DESKSTREAM\DeskStream.exe"

$targetHash = (Get-FileHash $targetExe -Algorithm SHA256).Hash
$targetSize = (Get-Item $targetExe).Length
$targetTime = (Get-Item $targetExe).LastWriteTime

Write-Host "BUILD EXE SHA-256: $targetHash"
Write-Host "BUILD EXE SIZE: $targetSize"
Write-Host "BUILD EXE TIME: $targetTime"

Copy-Item $targetExe -Destination $deployExe -Force
Start-Sleep -Seconds 1

$deployHash = (Get-FileHash $deployExe -Algorithm SHA256).Hash
$deploySize = (Get-Item $deployExe).Length
$deployTime = (Get-Item $deployExe).LastWriteTime

Write-Host "DEPLOYMENT EXE SHA-256: $deployHash"
Write-Host "DEPLOYMENT EXE SIZE: $deploySize"
Write-Host "DEPLOYMENT EXE TIME: $deployTime"

if ($targetHash -eq $deployHash) {
    Write-Host "HASH MATCH: TRUE"
} else {
    Write-Host "HASH MATCH: FALSE"
}

Write-Host "--- SHORTCUTS ---"
Get-ChildItem -Path "$env:USERPROFILE\Desktop" -Filter "*DeskStream*.lnk" -ErrorAction SilentlyContinue | ForEach-Object {
    $shell = New-Object -ComObject WScript.Shell
    $shortcut = $shell.CreateShortcut($_.FullName)
    Write-Host "Shortcut: $($_.Name) -> $($shortcut.TargetPath)"
}

