@echo off
setlocal
cd /d "%~dp0"
title CEZECORE - Development Launcher

echo.
echo  ==========================================
echo       CEZECORE - DEVELOPER WORKSPACE
echo  ==========================================
echo.

where node >nul 2>&1
if errorlevel 1 (
  echo [ERROR] Node.js was not found. Install Node.js LTS first.
  goto :fail
)
where npm >nul 2>&1
if errorlevel 1 (
  echo [ERROR] npm was not found. Reinstall Node.js LTS and reopen this window.
  goto :fail
)
where rustc >nul 2>&1
if errorlevel 1 (
  echo [ERROR] Rust was not found. Install Rustup and the MSVC toolchain first.
  goto :fail
)

if not exist "node_modules" (
  echo [1/2] Installing frontend dependencies...
  call npm install
  if errorlevel 1 goto :fail
) else (
  echo [1/2] Frontend dependencies already installed.
)

echo.
echo [2/2] Starting CEZECORE desktop development mode...
echo Close this window to stop the development process.
echo.
call npm run tauri dev
if errorlevel 1 goto :fail
exit /b 0

:fail
echo.
echo CEZECORE could not start. Check the error above and the README.md setup guide.
pause
exit /b 1
