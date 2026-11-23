@echo off
rem Creates "AItelier" and "Update AItelier" shortcuts on the Desktop and in the Start menu.
rem Run it once after building (double-click it in Explorer).

set "ROOT=%~dp0"
set "EXE=%ROOT%target\release\aitelier-editor.exe"
set "ICO=%ROOT%crates\aitelier-gui\assets\aitelier.ico"

if not exist "%EXE%" (
  echo aitelier-editor.exe was not found.
  echo Build it first: open PowerShell in this folder and run   cargo build --release
  pause
  exit /b 1
)

powershell -NoProfile -ExecutionPolicy Bypass -Command ^
  "$shell = New-Object -ComObject WScript.Shell;" ^
  "foreach ($dir in @([Environment]::GetFolderPath('Desktop'), [Environment]::GetFolderPath('Programs'))) {" ^
  "  $lnk = $shell.CreateShortcut((Join-Path $dir 'AItelier.lnk'));" ^
  "  $lnk.TargetPath = '%EXE%';" ^
  "  $lnk.WorkingDirectory = '%ROOT%';" ^
  "  $lnk.IconLocation = '%ICO%';" ^
  "  $lnk.Description = 'AItelier procedural modeler';" ^
  "  $lnk.Save();" ^
  "  $up = $shell.CreateShortcut((Join-Path $dir 'Update AItelier.lnk'));" ^
  "  $up.TargetPath = '%ROOT%update-aitelier.bat';" ^
  "  $up.WorkingDirectory = '%ROOT%';" ^
  "  $up.IconLocation = '%ICO%';" ^
  "  $up.Description = 'Download, build and install the latest AItelier';" ^
  "  $up.Save();" ^
  "  $old = Join-Path $dir 'Atelier.lnk'; if (Test-Path $old) { Remove-Item $old } }"

if errorlevel 1 (
  echo Something went wrong while creating the shortcuts.
) else (
  echo Done: "AItelier" and "Update AItelier" are on your Desktop and in the Start menu.
)
if /i not "%~1"=="--quiet" pause
