@echo off
setlocal
cd /d "%~dp0"
title CEZECORE - Build Windows EXE

echo.
echo  ==========================================
echo       CEZECORE - BUILD WINDOWS EXE
echo  ==========================================
echo.

where node >nul 2>&1
if errorlevel 1 (
  echo [ERROR] Node.js was not found.
  goto :fail
)
where rustc >nul 2>&1
if errorlevel 1 (
  echo [ERROR] Rust was not found.
  goto :fail
)
if not exist "node_modules" (
  echo Installing frontend dependencies first...
  call npm install
  if errorlevel 1 goto :fail
)

echo Building CEZECORE without an installer bundle...
call npm run tauri build -- --no-bundle
if errorlevel 1 goto :fail

echo.
echo Build complete. Look for:
echo src-tauri\target\release\cezecore.exe
pause
exit /b 0

:fail
echo.
echo Build failed. Check prerequisites and README.md.
pause
exit /b 1
