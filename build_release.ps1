# build_release.ps1
# Builds the desktop agent, the Screen Share GUI, and the self-contained installer.
# Outputs:
#   desktop-agent/target/release/desktop-agent.exe
#   screenshare-gui/target/release/ScreenShare.exe
#   screenshare-setup/target/release/screenshare-setup.exe  ->  installer/ScreenShare-Setup.exe
#                                                 and  ->  frontend/downloads/ScreenShare-Setup.exe

$ErrorActionPreference = "Stop"
$Root = $PSScriptRoot
$CargoTargetDir = "C:\cargo-target\deskstream"
$CargoExePath = Join-Path $CargoTargetDir "release\DeskStream.exe"
$PayloadVerifier = Join-Path $Root "scripts\verify-packaged-exe.ps1"
if (-not $env:DESKSTREAM_CERT_THUMBPRINT) {
    throw "DESKSTREAM_CERT_THUMBPRINT is required for production signing."
}

function Build($name, $dir) {
    Write-Host "========================================"
    Write-Host " Building $name"
    Write-Host "========================================"
    Push-Location (Join-Path $Root $dir)
    cargo build --release
    if ($LASTEXITCODE -ne 0) { Write-Error "$name build FAILED."; exit 1 }
    Pop-Location
}

# 1. Build the current DeskStream binary at the required Cargo output path.
$previousCargoTargetDir = $env:CARGO_TARGET_DIR
$env:CARGO_TARGET_DIR = $CargoTargetDir
try {
    if (Test-Path -LiteralPath $CargoExePath) {
        Remove-Item -LiteralPath $CargoExePath -Force
        if (Test-Path -LiteralPath $CargoExePath) {
            throw "Could not remove the previous release executable: $CargoExePath"
        }
    }
    Build "desktop-agent" "desktop-agent"
} finally {
    $env:CARGO_TARGET_DIR = $previousCargoTargetDir
}

if (-not (Test-Path -LiteralPath $CargoExePath)) {
    throw "Fresh Cargo release executable not found at $CargoExePath"
}

# Refuse to create a public release from an unsigned or untrusted executable.
& (Join-Path $Root "scripts\sign-release.ps1") -ExePath $CargoExePath
if ($LASTEXITCODE -ne 0) { throw "DeskStream production signing failed." }
& (Join-Path $Root "scripts\verify-release-signature.ps1") -ExePath $CargoExePath
if ($LASTEXITCODE -ne 0) { throw "DeskStream production signature verification failed." }

# 2. GUI (native Win32 wrapper around the agent)
Build "screenshare-gui" "screenshare-gui"
Build "relay-server" "relay-server"

# 3. Copy built binaries to staging directory
$stagingDir = Join-Path $Root "installer_staging"
New-Item -ItemType Directory -Path $stagingDir -Force | Out-Null

$agentSrc = $CargoExePath
$guiSrc = Join-Path $Root "screenshare-gui\target\release\ScreenShare.exe"
$relaySrc = Join-Path $Root "relay-server\target\release\relay-server.exe"

if (-not (Test-Path $agentSrc)) { Write-Error "ERROR: Fresh DeskStream.exe was not found at $agentSrc. Installer build aborted."; exit 1 }
if (-not (Test-Path $guiSrc)) { Write-Error "ERROR: Fresh ScreenShare.exe was not found. Installer build aborted."; exit 1 }
if (-not (Test-Path $relaySrc)) { Write-Error "ERROR: Fresh relay-server.exe was not found. Installer build aborted."; exit 1 }

foreach ($binary in @($guiSrc, $relaySrc)) {
    & (Join-Path $Root "scripts\sign-release.ps1") -ExePath $binary
    if ($LASTEXITCODE -ne 0) { throw "Production signing failed for $binary." }
    & (Join-Path $Root "scripts\verify-release-signature.ps1") -ExePath $binary
    if ($LASTEXITCODE -ne 0) { throw "Production signature verification failed for $binary." }
}

Copy-Item $agentSrc -Destination "$stagingDir\DeskStream.exe" -Force
Copy-Item $agentSrc -Destination "$stagingDir\desktop-agent.exe" -Force
Copy-Item $guiSrc -Destination "$stagingDir\ScreenShare.exe" -Force
Copy-Item $relaySrc -Destination "$stagingDir\relay-server.exe" -Force

# 4. Verify staging against the current Cargo output.
$agentHashSrc = (Get-FileHash $agentSrc -Algorithm SHA256).Hash
$agentHashDst = (Get-FileHash "$stagingDir\desktop-agent.exe" -Algorithm SHA256).Hash
Write-Host "Cargo DeskStream.exe SHA256: $agentHashSrc"
Write-Host "Staged DeskStream.exe SHA256: $agentHashDst"
Write-Host "Cargo DeskStream.exe timestamp (UTC): $((Get-Item $agentSrc).LastWriteTimeUtc.ToString('o'))"
if ($agentHashSrc -ne $agentHashDst) { throw "ERROR: Installer staging differs from the current Cargo DeskStream.exe." }
foreach ($stagedName in @("ScreenShare.exe", "relay-server.exe")) {
    $stagedPath = Join-Path $stagingDir $stagedName
    $sourcePath = if ($stagedName -eq "ScreenShare.exe") { $guiSrc } else { $relaySrc }
    if ((Get-FileHash -LiteralPath $sourcePath -Algorithm SHA256).Hash -ne
        (Get-FileHash -LiteralPath $stagedPath -Algorithm SHA256).Hash) {
        throw "Installer staging differs from the signed release binary: $stagedName"
    }
}

# 5. Installer (embeds the binaries from staging)
$staleSetup = Join-Path $Root "screenshare-setup\target\release\screenshare-setup.exe"
if (Test-Path -LiteralPath $staleSetup) {
    Remove-Item -LiteralPath $staleSetup -Force
    if (Test-Path -LiteralPath $staleSetup) {
        throw "Could not remove the previous setup artifact: $staleSetup"
    }
}
Build "screenshare-setup" "screenshare-setup"

# 4. Publish the installer
$setup = Join-Path $Root "screenshare-setup\target\release\screenshare-setup.exe"
& (Join-Path $Root "scripts\sign-release.ps1") -ExePath $setup
if ($LASTEXITCODE -ne 0) { throw "Setup production signing failed." }
& (Join-Path $Root "scripts\verify-release-signature.ps1") -ExePath $setup
if ($LASTEXITCODE -ne 0) { throw "Setup production signature verification failed." }

$dst1  = Join-Path $Root "installer\ScreenShare-Setup.exe"
$dst2  = Join-Path $Root "frontend\downloads\ScreenShare-Setup.exe"

New-Item -ItemType Directory -Force -Path (Split-Path $dst1) | Out-Null
New-Item -ItemType Directory -Force -Path (Split-Path $dst2) | Out-Null
Copy-Item $setup $dst1 -Force
Copy-Item $setup $dst2 -Force

& $PayloadVerifier -SourcePath $agentSrc -StagedPath "$stagingDir\desktop-agent.exe" -InstallerPath $dst1 -InstallerFormat Embedded
if ($LASTEXITCODE -ne 0) {
    throw "Final installer payload verification failed."
}

$publishedSetupHash = (Get-FileHash -LiteralPath $dst1 -Algorithm SHA256).Hash
$downloadSetupHash = (Get-FileHash -LiteralPath $dst2 -Algorithm SHA256).Hash
if ($publishedSetupHash -ne $downloadSetupHash) {
    throw "Published installer and downloads installer SHA256 values do not match."
}
& (Join-Path $Root "scripts\verify-release-signature.ps1") -ExePath $dst1
if ($LASTEXITCODE -ne 0) { throw "Published Setup signature verification failed." }
& (Join-Path $Root "scripts\verify-release-signature.ps1") -ExePath $dst2
if ($LASTEXITCODE -ne 0) { throw "Download Setup signature verification failed." }

Write-Host ""
Write-Host "========================================"
Write-Host " DONE"
Write-Host "   Agent : $CargoExePath"
Write-Host "   GUI   : screenshare-gui\target\release\ScreenShare.exe"
Write-Host "   Setup : installer\ScreenShare-Setup.exe"
Write-Host "          frontend\downloads\ScreenShare-Setup.exe"
Write-Host "========================================"
