@echo off
"%~dp0runtime\node.exe" "%~dp0launcher.mjs" %*
if errorlevel 1 pause
