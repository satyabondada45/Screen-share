@echo off
setlocal enabledelayedexpansion

echo ========================================================
echo        DeskStream Pro — Release Packaging Tool
echo ========================================================
echo.

echo [1/3] Compiling optimized release binary...
cd /d "%~dp0\desktop-agent"
cargo build --release

if %ERRORLEVEL% neq 0 (
    echo [ERROR] Build failed! Check compiler output.
    pause
    exit /b %ERRORLEVEL%
)

echo.
echo [2/3] Preparing distribution folder...
cd /d "%~dp0"
if not exist "dist" mkdir "dist"
copy /y "desktop-agent\target\release\desktop-agent.exe" "dist\DeskStream.exe" >nul

echo.
echo [3/3] Creating desktop shortcut...
powershell -ExecutionPolicy Bypass -File "%~dp0Create_Desktop_Shortcut.ps1"

echo.
echo ========================================================
echo  SUCCESS: DeskStream.exe is ready!
echo  Location: %~dp0dist\DeskStream.exe
echo  Desktop shortcut created.
echo.
echo  You can now simply DOUBLE-CLICK "DeskStream.exe" or the
echo  Desktop Shortcut to launch the application.
echo ========================================================
echo.
pause
