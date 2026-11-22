@echo off
rem Double-click to install (no admin needed). Wraps install-win.ps1 with
rem -ExecutionPolicy Bypass so the default policy does not block it.
setlocal
cd /d "%~dp0"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0install-win.ps1" %*
if errorlevel 1 (
  echo.
  echo Install failed. Please copy the red text above and send it over.
  pause
  exit /b 1
)
echo.
echo Done. Search "Eryu" in the Start menu to launch it.
echo The tray icon opens settings. See README.txt for details.
pause
