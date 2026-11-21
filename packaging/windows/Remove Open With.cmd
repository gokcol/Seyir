@echo off
setlocal DisableDelayedExpansion
if not exist "%~dp0seyir.exe" (
    echo Extract all files from the Seyir ZIP into the same folder and try again.
    pause
    exit /b 1
)
start "" /wait "%~dp0seyir.exe" --unregister-open-with
if errorlevel 1 (
    echo Could not remove the Seyir Open with registration.
    pause
    exit /b 1
)
echo Removed the Seyir Open with registration.
pause
