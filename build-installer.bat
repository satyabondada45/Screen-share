@echo off
REM build-installer.bat - Builds the Screen Share installer with SHA256 verification
REM 
REM Flow:
REM   Build Rust release binaries -> Locate fresh EXE -> Clean staging ->
REM   Copy fresh EXE -> Verify SHA256 -> Build installer -> Verify final installer
REM
REM Usage:
REM   build-installer.bat            (builds with default versioning)
REM   build-installer.bat --server-addr=host.com  (passes server address to installer)

setlocal enabledelayedexpansion

set PROJECT_ROOT=%~dp0
set AGENT_DIR=%PROJECT_ROOT%desktop-agent
set RELAY_DIR=%PROJECT_ROOT%relay-server
set GUI_DIR=%PROJECT_ROOT%screenshare-gui
set SETUP_DIR=%PROJECT_ROOT%screenshare-setup
set INSTALLER_DIR=%PROJECT_ROOT%installer
set DOWNLOADS_DIR=%PROJECT_ROOT%frontend\downloads
set "PREVIOUS_CARGO_TARGET_DIR=%CARGO_TARGET_DIR%"
set "CARGO_TARGET_DIR=C:\cargo-target\deskstream"
set AGENT_EXE=%CARGO_TARGET_DIR%\release\DeskStream.exe
set VERSION=1.0.2

if "%DESKSTREAM_CERT_THUMBPRINT%"=="" (
    echo ERROR: DESKSTREAM_CERT_THUMBPRINT is required for production signing.
    exit /b 1
)

echo ========================================
echo SCREEN SHARE RELEASE BUILD
echo ========================================
echo.

REM -- Parse arguments --
set SERVER_ADDR_ARG=
:parse_args
if "%~1"=="" goto :done_args
if "%~1"=="--server-addr=%~1" (
    set SERVER_ADDR_ARG=%~1
    shift
    goto :parse_args
)
if "%~1"=="--server-addr" (
    set SERVER_ADDR_ARG=--server-addr=%~2
    shift
    shift
    goto :parse_args
)
shift
goto :parse_args
:done_args

REM -- Step 1: Build all Rust components from source --
echo [1/7] Building Rust release binaries...
echo   Building desktop-agent...
if exist "%AGENT_EXE%" (
    del /f /q "%AGENT_EXE%"
    if exist "%AGENT_EXE%" (
        echo ERROR: Could not remove stale Cargo output: %AGENT_EXE%
        exit /b 1
    )
)
pushd "%AGENT_DIR%"
cargo build --release --bin DeskStream 2>&1
if errorlevel 1 (
    echo ERROR: desktop-agent build failed.
    popd
    exit /b 1
)
popd
set "CARGO_TARGET_DIR=%PREVIOUS_CARGO_TARGET_DIR%"
if not exist "%AGENT_EXE%" (
    echo ERROR: Fresh Cargo output not found: %AGENT_EXE%
    exit /b 1
)

echo   Building relay-server...
pushd "%RELAY_DIR%"
cargo build --release 2>&1
if errorlevel 1 (
    echo ERROR: relay-server build failed.
    popd
    exit /b 1
)
popd

echo   Building screenshare-gui...
pushd "%GUI_DIR%"
cargo build --release 2>&1
if errorlevel 1 (
    echo ERROR: screenshare-gui build failed.
    popd
    exit /b 1
)
popd
echo      Build OK
echo.

REM -- Step 2: Locate fresh EXE --
echo [2/7] Locating fresh binaries...
set RELAY_EXE=%RELAY_DIR%\target\release\relay-server.exe
set GUI_EXE=%GUI_DIR%\target\release\ScreenShare.exe

if not exist "%AGENT_EXE%" (
    echo ERROR: DeskStream.exe not found at %AGENT_EXE%
    exit /b 1
)
if not exist "%GUI_EXE%" (
    echo ERROR: ScreenShare.exe not found at %GUI_EXE%
    exit /b 1
)
if not exist "%RELAY_EXE%" (
    echo ERROR: relay-server.exe not found at %RELAY_EXE%
    exit /b 1
)
echo      desktop-agent.exe: %AGENT_EXE%
echo      ScreenShare.exe:   %GUI_EXE%
echo      relay-server.exe:  %RELAY_EXE%
echo.

REM -- Step 3: Stop old agent process --
echo [3/7] Stopping running agent (if any)...
powershell -NoProfile -Command "Get-Process -Name 'desktop-agent','DeskStream','ScreenShare' -ErrorAction SilentlyContinue | ForEach-Object { Stop-Process -Id $_.Id -Force }"
echo      Done
echo.

REM Production distribution must be Authenticode-signed with a trusted publisher certificate.
powershell -NoProfile -ExecutionPolicy Bypass -File "%PROJECT_ROOT%scripts\sign-release.ps1" -ExePath "%AGENT_EXE%"
if errorlevel 1 exit /b 1
powershell -NoProfile -ExecutionPolicy Bypass -File "%PROJECT_ROOT%scripts\verify-release-signature.ps1" -ExePath "%AGENT_EXE%"
if errorlevel 1 exit /b 1
powershell -NoProfile -ExecutionPolicy Bypass -File "%PROJECT_ROOT%scripts\sign-release.ps1" -ExePath "%GUI_EXE%"
if errorlevel 1 exit /b 1
powershell -NoProfile -ExecutionPolicy Bypass -File "%PROJECT_ROOT%scripts\verify-release-signature.ps1" -ExePath "%GUI_EXE%"
if errorlevel 1 exit /b 1
powershell -NoProfile -ExecutionPolicy Bypass -File "%PROJECT_ROOT%scripts\sign-release.ps1" -ExePath "%RELAY_EXE%"
if errorlevel 1 exit /b 1
powershell -NoProfile -ExecutionPolicy Bypass -File "%PROJECT_ROOT%scripts\verify-release-signature.ps1" -ExePath "%RELAY_EXE%"
if errorlevel 1 exit /b 1

REM -- Step 4: Clean staging and copy fresh binaries --
echo [4/7] Cleaning staging and copying fresh binaries...
set STAGING_DIR=%PROJECT_ROOT%installer_staging

if not exist "%STAGING_DIR%" mkdir "%STAGING_DIR%"

copy /Y "%AGENT_EXE%" "%STAGING_DIR%\DeskStream.exe" >nul
copy /Y "%AGENT_EXE%" "%STAGING_DIR%\desktop-agent.exe" >nul
copy /Y "%GUI_EXE%" "%STAGING_DIR%\ScreenShare.exe" >nul
copy /Y "%RELAY_EXE%" "%STAGING_DIR%\relay-server.exe" >nul
echo      Binaries copied to staging
echo.

REM -- Step 5: Verify SHA256 --
echo [5/7] Verifying SHA256 hashes...
echo   Fresh Rust EXE:
certutil -hashfile "%AGENT_EXE%" SHA256 | findstr /v "hash" | findstr /v "CertUtil" > "%TEMP%\hash_fresh.txt"
set /p FRESH_HASH=<"%TEMP%\hash_fresh.txt"
echo   SHA256: %FRESH_HASH%

echo   Staging EXE:
certutil -hashfile "%STAGING_DIR%\desktop-agent.exe" SHA256 | findstr /v "hash" | findstr /v "CertUtil" > "%TEMP%\hash_staging.txt"
set /p STAGING_HASH=<"%TEMP%\hash_staging.txt"
echo   SHA256: %STAGING_HASH%

if /I "%FRESH_HASH%" NEQ "%STAGING_HASH%" (
    echo.
    echo ERROR: SHA256 MISMATCH between fresh build and staging!
    echo   Fresh:  %FRESH_HASH%
    echo   Staging: %STAGING_HASH%
    echo.
    echo BUILD STOPPED. The installer is NOT being created with a mismatched binary.
    del "%TEMP%\hash_fresh.txt" 2>nul
    del "%TEMP%\hash_staging.txt" 2>nul
    exit /b 1
)
echo.
echo   Hashes match!
del "%TEMP%\hash_fresh.txt" 2>nul
del "%TEMP%\hash_staging.txt" 2>nul
echo.

REM -- Step 6: Build installer --
echo [6/7] Building installer...
set SETUP_EXE=%SETUP_DIR%\target\release\screenshare-setup.exe
if exist "%SETUP_EXE%" (
    del /f /q "%SETUP_EXE%"
    if exist "%SETUP_EXE%" (
        echo ERROR: Could not remove stale setup output: %SETUP_EXE%
        exit /b 1
    )
)
pushd "%SETUP_DIR%"
cargo build --release 2>&1
if errorlevel 1 (
    echo ERROR: Installer build failed.
    popd
    exit /b 1
)
popd
echo      Installer build OK
echo.

REM -- Step 7: Verify final installer --
echo [7/7] Verifying final installer...
 powershell -NoProfile -ExecutionPolicy Bypass -File "%PROJECT_ROOT%scripts\sign-release.ps1" -ExePath "%SETUP_EXE%"
if errorlevel 1 exit /b 1
powershell -NoProfile -ExecutionPolicy Bypass -File "%PROJECT_ROOT%scripts\verify-release-signature.ps1" -ExePath "%SETUP_EXE%"
if errorlevel 1 exit /b 1

set VERSIONED_INSTALLER=%INSTALLER_DIR%\ScreenShare-Setup-%VERSION%.exe
set RAW_INSTALLER=%SETUP_DIR%\target\release\screenshare-setup.exe

REM Create versioned installer
if not exist "%INSTALLER_DIR%" mkdir "%INSTALLER_DIR%"
copy /Y "%RAW_INSTALLER%" "%VERSIONED_INSTALLER%" >nul

REM Also copy to latest/
set LATEST_DIR=%INSTALLER_DIR%\latest
if not exist "%LATEST_DIR%" mkdir "%LATEST_DIR%"
copy /Y "%RAW_INSTALLER%" "%LATEST_DIR%\ScreenShare-Setup.exe" >nul

REM Also update downloads
if not exist "%DOWNLOADS_DIR%" mkdir "%DOWNLOADS_DIR%"
copy /Y "%RAW_INSTALLER%" "%DOWNLOADS_DIR%\ScreenShare-Setup.exe" >nul

echo   Versioned installer: %VERSIONED_INSTALLER%
for %%I in ("%VERSIONED_INSTALLER%") do echo   Size: %%~zI bytes

REM Verify architecture using a simple powershell script to check PE header
powershell -Command "$bytes = [System.IO.File]::ReadAllBytes('%VERSIONED_INSTALLER%'); $peHeaderOffset = [BitConverter]::ToInt32($bytes, 0x3C); $machine = [BitConverter]::ToUInt16($bytes, $peHeaderOffset + 4); if ($machine -eq 0x8664) { Write-Host '  Architecture: x64' } else { Write-Host '  ERROR: Not x64 architecture!'; exit 1 }"
if errorlevel 1 exit /b 1

echo   SHA256:
certutil -hashfile "%VERSIONED_INSTALLER%" SHA256 | findstr /v "hash" | findstr /v "CertUtil" > "%TEMP%\final_hash.txt"
set /p FINAL_HASH=<"%TEMP%\final_hash.txt"
echo   %FINAL_HASH%
del "%TEMP%\final_hash.txt" 2>nul

echo   Verifying exact embedded Cargo executable...
powershell -NoProfile -ExecutionPolicy Bypass -File "%PROJECT_ROOT%scripts\verify-packaged-exe.ps1" -SourcePath "%AGENT_EXE%" -StagedPath "%STAGING_DIR%\desktop-agent.exe" -InstallerPath "%VERSIONED_INSTALLER%" -InstallerFormat Embedded
if errorlevel 1 (
    echo ERROR: Setup payload does not match the freshly built Cargo executable.
    exit /b 1
)

powershell -NoProfile -ExecutionPolicy Bypass -File "%PROJECT_ROOT%scripts\verify-release-signature.ps1" -ExePath "%VERSIONED_INSTALLER%"
if errorlevel 1 exit /b 1
powershell -NoProfile -ExecutionPolicy Bypass -File "%PROJECT_ROOT%scripts\verify-release-signature.ps1" -ExePath "%LATEST_DIR%\ScreenShare-Setup.exe"
if errorlevel 1 exit /b 1
powershell -NoProfile -ExecutionPolicy Bypass -File "%PROJECT_ROOT%scripts\verify-release-signature.ps1" -ExePath "%DOWNLOADS_DIR%\ScreenShare-Setup.exe"
if errorlevel 1 exit /b 1

echo.
echo ========================================
echo BUILD COMPLETE
echo ========================================
echo.
echo Final installer:
echo   %VERSIONED_INSTALLER%
echo   Version: %VERSION%
echo   SHA256: %FINAL_HASH%
echo.
echo   latest\ copy: %LATEST_DIR%\ScreenShare-Setup.exe
echo   downloads\ copy: %DOWNLOADS_DIR%\ScreenShare-Setup.exe
echo.
echo NOTE: The installer embeds freshly compiled binaries via include_bytes!.
echo       SHA256 of embedded agent matches the Rust release build above.
echo ========================================