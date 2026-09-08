@echo off
REM build-agent.bat - Builds the Screen Share desktop-agent, relay-server, and GUI from current source
REM Outputs release binaries with version, hash, and timestamp information
REM
REM Usage:
REM   build-agent.bat            (builds all components)
REM   build-agent.bat --clean    (runs cargo clean first)

setlocal enabledelayedexpansion

set PROJECT_ROOT=%~dp0
set AGENT_DIR=%PROJECT_ROOT%desktop-agent
set RELAY_DIR=%PROJECT_ROOT%relay-server
set GUI_DIR=%PROJECT_ROOT%screenshare-gui

REM Parse --clean flag
set CLEAN=0
:parse_args
if "%~1"=="" goto :done_args
if /i "%~1"=="--clean" set CLEAN=1
shift
goto :parse_args
:done_args

echo ========================================
echo SCREEN SHARE BUILD
echo ========================================
echo.

REM Optional clean
if "%CLEAN%"=="1" (
    echo Running cargo clean on all crates...
    pushd "%AGENT_DIR%" && cargo clean && popd
    pushd "%RELAY_DIR%" && cargo clean && popd
    pushd "%GUI_DIR%" && cargo clean && popd
    echo Clean done.
    echo.
)

REM Step 1: Build desktop-agent (host + viewer binaries)
echo [1/3] Building desktop-agent (host + viewer)...
pushd "%AGENT_DIR%"
cargo build --release 2>&1
if errorlevel 1 (
    echo ERROR: Agent build failed.
    popd
    exit /b 1
)
popd
echo      Build OK
echo.

REM Step 2: Build relay-server
echo [2/3] Building relay-server...
pushd "%RELAY_DIR%"
cargo build --release 2>&1
if errorlevel 1 (
    echo ERROR: Relay build failed.
    popd
    exit /b 1
)
popd
echo      Build OK
echo.

REM Step 3: Build screenshare-gui
echo [3/3] Building screenshare-gui...
pushd "%GUI_DIR%"
cargo build --release 2>&1
if errorlevel 1 (
    echo ERROR: GUI build failed.
    popd
    exit /b 1
)
popd
echo      Build OK
echo.

REM Verify binaries exist
set AGENT_EXE=%AGENT_DIR%\target\release\desktop-agent.exe
set RELAY_EXE=%RELAY_DIR%\target\release\relay-server.exe
set GUI_EXE=%GUI_DIR%\target\release\ScreenShare.exe

if not exist "%AGENT_EXE%" (
    echo ERROR: desktop-agent.exe was not produced!
    exit /b 1
)
if not exist "%RELAY_EXE%" (
    echo ERROR: relay-server.exe was not produced!
    exit /b 1
)
if not exist "%GUI_EXE%" (
    echo ERROR: ScreenShare.exe was not produced!
    exit /b 1
)

REM Print results
echo ========================================
echo AGENT BUILD SUCCESSFUL
echo ========================================
echo.

echo Agent (desktop-agent.exe):
echo   Path: %AGENT_EXE%
echo   Version: 1.0.2
for %%I in ("%AGENT_EXE%") do echo   Size: %%~zI bytes
echo   Timestamp: %%~tI
echo   SHA256:
certutil -hashfile "%AGENT_EXE%" SHA256 | findstr /v "hash" | findstr /v "CertUtil"
echo.

echo Relay (relay-server.exe):
echo   Path: %RELAY_EXE%
echo   Version: 1.0.2
for %%I in ("%RELAY_EXE%") do echo   Size: %%~zI bytes
echo   SHA256:
certutil -hashfile "%RELAY_EXE%" SHA256 | findstr /v "hash" | findstr /v "CertUtil"
echo.

echo GUI (ScreenShare.exe):
echo   Path: %GUI_EXE%
echo   Version: 1.0.2
for %%I in ("%GUI_EXE%") do echo   Size: %%~zI bytes
echo   SHA256:
certutil -hashfile "%GUI_EXE%" SHA256 | findstr /v "hash" | findstr /v "CertUtil"
echo.

echo ========================================
echo BUILD SUCCESSFUL
echo ========================================