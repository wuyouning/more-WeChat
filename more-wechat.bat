@echo off
setlocal
rem Open the desktop launcher without terminating WeChat.
if exist "%~dp0more-wechat.exe" (
    start "" "%~dp0more-wechat.exe"
    exit /b 0
)
if exist "%LOCALAPPDATA%\more-WeChat\more-wechat.exe" (
    start "" "%LOCALAPPDATA%\more-WeChat\more-wechat.exe"
    exit /b 0
)
if exist "%~dp0src-tauri\target\release\more-wechat.exe" (
    start "" "%~dp0src-tauri\target\release\more-wechat.exe"
    exit /b 0
)
echo Please install the more-WeChat desktop preview, or run pnpm tauri dev.
echo See readme.md for installation and compatibility details.
pause
exit /b 1
