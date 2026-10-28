@echo off
setlocal
"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\atelier.ps1" %*
if errorlevel 1 (
  echo.
  echo Atelier failed. Read the error above; no data was deleted.
  pause
  exit /b 1
)
