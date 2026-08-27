@echo off
:: Batch script to add Windows Firewall Inbound Rule for Port 9001
echo ========================================================
echo   DeskStream — Adding Windows Firewall Rule for Port 9001
echo ========================================================
echo.

net session >nul 2>&1
if %errorLevel% neq 0 (
    echo [REQUEST] Requesting Administrator privileges...
    powershell -Command "Start-Process cmd -ArgumentList '/c netsh advfirewall firewall add rule name=\"DeskStream Relay Server (TCP 9001)\" dir=in action=allow protocol=TCP localport=9001 & pause' -Verb RunAs"
    exit /b
)

netsh advfirewall firewall add rule name="DeskStream Relay Server (TCP 9001)" dir=in action=allow protocol=TCP localport=9001
if %errorLevel% equ 0 (
    echo [OK] Windows Firewall rule added successfully! Inbound TCP port 9001 is now allowed.
) else (
    echo [WARN] Failed to add firewall rule. Please run this script as Administrator.
)

pause
