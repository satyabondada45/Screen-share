# Copy cargo-zigbuild.exe to a path without spaces
Copy-Item 'C:\Users\X1 CORBON\.cargo\bin\cargo-zigbuild.exe' 'C:\cargo-target\cargo-zigbuild.exe' -Force

$batFiles = Get-ChildItem -Path "$env:LOCALAPPDATA\cargo-zigbuild" -Filter "*.bat" -Recurse
foreach ($file in $batFiles) {
    $content = Get-Content $file.FullName
    $newContent = $content | ForEach-Object {
        $_ -replace '(?i)"C:\\Users\\X1 CORBON\\\.cargo\\bin\\cargo-zigbuild\.exe"', 'C:\cargo-target\cargo-zigbuild.exe'
    }
    Set-Content -Path $file.FullName -Value $newContent
    Write-Host "Patched $($file.FullName)"
}
