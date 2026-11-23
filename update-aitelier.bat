@echo off
rem AItelier updater: downloads the latest version from GitHub into C:\dev\aitelier,
rem builds it, and refreshes the Desktop / Start-menu shortcuts.

rem Run from a temporary copy, because the update may replace this file.
if /i not "%~1"=="--run" (
  copy /y "%~f0" "%TEMP%\aitelier-update.bat" >nul
  "%TEMP%\aitelier-update.bat" --run
)

set "DEST=C:\dev\aitelier"
set "URL=https://github.com/donquoique/aitelier/archive/refs/heads/main.zip"

echo Downloading the latest AItelier...
powershell -NoProfile -ExecutionPolicy Bypass -Command ^
  "$ErrorActionPreference = 'Stop';" ^
  "$zip = Join-Path $env:TEMP 'aitelier-main.zip';" ^
  "$tmp = Join-Path $env:TEMP 'aitelier-main';" ^
  "Invoke-WebRequest -Uri '%URL%' -OutFile $zip -UseBasicParsing;" ^
  "if (Test-Path $tmp) { Remove-Item $tmp -Recurse -Force };" ^
  "Expand-Archive $zip -DestinationPath $tmp;" ^
  "New-Item -ItemType Directory -Force '%DEST%' | Out-Null;" ^
  "Copy-Item -Path (Join-Path $tmp 'aitelier-main\*') -Destination '%DEST%' -Recurse -Force"
if errorlevel 1 (
  echo.
  echo Download failed. Check the internet connection and try again.
  pause
  exit /b 1
)

echo.
echo Building (the first build takes several minutes, later ones less than a minute)...
cd /d "%DEST%"
cargo build --release
if errorlevel 1 (
  echo.
  echo The build failed. Copy the red error messages above and send them to Claude.
  pause
  exit /b 1
)

call "%DEST%\create-shortcuts.bat" --quiet
echo.
echo AItelier is up to date. Starting it...
start "" "%DEST%\target\release\aitelier-editor.exe"
timeout /t 5 >nul
