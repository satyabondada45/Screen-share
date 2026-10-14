@echo off
set "LOCALAPPDATA=C:\cargo-target\appdata"
set "PATH=C:\cargo-target\zig;%PATH%"
cd /d "%~dp0"
cargo zigbuild --target x86_64-unknown-linux-gnu --release --bin relay-server
