@echo off
setlocal
rem Baut MQTT Probe fuer Windows. WICHTIG: Das Target-Verzeichnis muss auf einem
rem lokalen NTFS-Laufwerk liegen - auf dem VirtualBox-Share (Y:) scheitert der
rem Linker mit LNK1104, weil rustc dort keine Hardlinks anlegen kann.
cd /d "%~dp0"
set "CARGO_TARGET_DIR=C:\cargo-target\mqtt-probe"
if not exist "target\release" mkdir "target\release"
cargo build --release
if errorlevel 1 exit /b 1
copy /Y "%CARGO_TARGET_DIR%\release\mqtt-probe.exe" "target\release\mqtt-probe.exe" >nul
echo Fertig: %~dp0target\release\mqtt-probe.exe