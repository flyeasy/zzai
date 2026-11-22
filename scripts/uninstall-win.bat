@echo off
rem Double-click to uninstall. Keeps %APPDATA%\Eryu (config/models/history)
rem unless you pass -PurgeData.
setlocal
cd /d "%~dp0"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0uninstall-win.ps1" %*
if errorlevel 1 (
  echo.
  echo Uninstall did not finish. Please copy the red text above and send it over.
  pause
  exit /b 1
)
echo.
echo Removed. Config and history are still in %APPDATA%\Eryu.
pause
