@echo off
"%~dp0runtime\node.exe" "%~dp0launcher.mjs" --stop
if errorlevel 1 pause
