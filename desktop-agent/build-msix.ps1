# build-msix.ps1
# Reproducible build script for DeskStream MSIX package

$ErrorActionPreference = "Stop"
$projectRoot = Split-Path -Parent $PSScriptRoot
$msixDir = "$projectRoot\packaging\msix"
$outputDir = "$projectRoot\packaging\output"

Write-Host "======================================"
Write-Host " DeskStream MSIX Packager"
Write-Host "======================================"

# 1. Clean previous build
Write-Host "[1/5] Cleaning previous build..."
if (Test-Path $outputDir) { Remove-Item -Path $outputDir -Recurse -Force }
New-Item -ItemType Directory -Path $outputDir -Force | Out-Null
if (Test-Path "$msixDir\DeskStream.exe") { Remove-Item -Path "$msixDir\DeskStream.exe" -Force }

# 2. Build the Rust application
Write-Host "[2/5] Building Rust application..."
Push-Location $projectRoot
cargo build --release
if ($LASTEXITCODE -ne 0) { Write-Error "Cargo build failed!"; exit 1 }
Pop-Location

# 3. Copy binaries to MSIX directory
Write-Host "[3/5] Copying binaries..."
Copy-Item "$projectRoot\target\release\DeskStream.exe" "$msixDir\DeskStream.exe" -Force

# Note: The assets (icons, html) are embedded inside DeskStream.exe via include_bytes! 
# so we do not need to copy them to the MSIX root for runtime, EXCEPT the Store logos.
# Note: For production, ensure you generate the correct StoreLogo.png, Square150x150Logo.png, etc.
# in the packaging\msix\Assets\ directory.

# 4. Generate the MSIX package
Write-Host "[4/5] Creating MSIX package..."
# We assume Windows SDK 'MakeAppx.exe' is in the PATH. If not, it needs to be located.
$makeAppxPath = (Get-Command MakeAppx.exe -ErrorAction SilentlyContinue).Source
if (-not $makeAppxPath) {
    # Try common Windows SDK paths
    $sdkPath = "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\makeappx.exe"
    $makeAppxPath = (Resolve-Path $sdkPath | Select-Object -Last 1).Path
}

if (-not $makeAppxPath) {
    Write-Error "MakeAppx.exe not found! Please install the Windows SDK or run in Developer Command Prompt."
    exit 1
}

$msixFile = "$outputDir\DeskStream.msix"
& $makeAppxPath pack /d $msixDir /p $msixFile /o
if ($LASTEXITCODE -ne 0) { Write-Error "MakeAppx failed!"; exit 1 }

# 5. Signing (Development only)
Write-Host "[5/5] Signing (DEVELOPMENT ONLY)..."
Write-Host "NOTE: For Microsoft Store distribution, the package will be signed by Microsoft."
Write-Host "For direct website distribution, you MUST use a valid OV/EV Code Signing Certificate."

Write-Host "======================================"
Write-Host " MSIX package created successfully!"
Write-Host " Output: $msixFile"
Write-Host "======================================"
