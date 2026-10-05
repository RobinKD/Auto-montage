@echo off
rem Windows : installe les raccourcis Auto-montage (bureau et menu Demarrer).
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0windows\install.ps1"
pause
