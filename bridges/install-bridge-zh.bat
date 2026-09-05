@echo off
chcp 65001 >nul
rem ============================================================
rem  ManiaMapAnalyser - Bridge Installer (Chinese UI)
rem  Double-click to run the installer with a Chinese interface.
rem  The interface text itself comes from install-bridge.ps1
rem  (the -Chinese switch); this file only launches it.
rem ============================================================
title ManiaMapAnalyser - Bridge Installer (Chinese UI)
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0install-bridge.ps1" -Chinese
set "EXITCODE=%ERRORLEVEL%"
echo.
if not "%EXITCODE%"=="0" (
    echo Installer exited with code %EXITCODE%. See messages above.
)
echo.
pause