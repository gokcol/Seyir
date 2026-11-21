@echo off
setlocal DisableDelayedExpansion
if not exist "%~dp0seyir.exe" (
    echo Extract all files from the Seyir ZIP into the same folder and try again.
    pause
    exit /b 1
)
start "" /wait "%~dp0seyir.exe" --register-open-with
if errorlevel 1 (
    echo Could not add Seyir to Open with.
    pause
    exit /b 1
)
echo Seyir is available under Open with for supported images.
pause
