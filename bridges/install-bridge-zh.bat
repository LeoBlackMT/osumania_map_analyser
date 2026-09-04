@echo off
chcp 65001 >nul
rem ============================================================
rem  ManiaMapAnalyser - Bridge Installer (Chinese UI)
rem  Double-click this file to install/remove the Etterna or
rem  Malody V bridge files for mma-shell, with Chinese UI text.
rem ============================================================
title ManiaMapAnalyser - 桥文件安装器 (中文)
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0install-bridge.ps1" -Chinese
set "EXITCODE=%ERRORLEVEL%"
echo.
if not "%EXITCODE%"=="0" (
    echo 安装器退出码 %EXITCODE%，请查看上方提示。
)
echo.
pause