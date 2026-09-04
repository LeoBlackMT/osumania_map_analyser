@echo off
rem ============================================================
rem  ManiaMapAnalyser - Bridge Installer launcher
rem  Double-click this file to install/remove the Etterna or
rem  Malody V bridge files for mma-shell.
rem ============================================================
title ManiaMapAnalyser - Bridge Installer
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0install-bridge.ps1"
set "EXITCODE=%ERRORLEVEL%"
echo.
if not "%EXITCODE%"=="0" (
    echo Installer exited with code %EXITCODE%.
    echo See the message above for details.
)
pause