@echo off
setlocal EnableExtensions
chcp 65001 >nul

set "ROOT=%~dp0"
pushd "%ROOT%"
if errorlevel 1 (
    echo Cannot enter project directory: %ROOT%
    pause
    exit /b 1
)

for %%I in ("%CD%") do set "ROOT=%%~fI"
set "RELEASE_DIR=%ROOT%\releases\code-Manager"
set "OUTPUT_EXE=%RELEASE_DIR%\code-Manager.exe"
set "UNINSTALL_TEMPLATE=%ROOT%\uninstall.bat.template"
set "ICON_SOURCE=%ROOT%\297763_sort-by-icon.svg"
set "ICON_RENDERER=%ROOT%\tools\render-tray-icon.html"
set "ICON_PNG=%ROOT%\assets\tray.png"
set "ICON_ICO=%ROOT%\assets\tray.ico"
set "EDGE="

if exist "C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe" set "EDGE=C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"
if not defined EDGE if exist "C:\Program Files\Microsoft\Edge\Application\msedge.exe" set "EDGE=C:\Program Files\Microsoft\Edge\Application\msedge.exe"

echo ========================================
echo code-Manager full build started
echo ========================================
echo.
echo This script will:
echo   1. Generate the tray icon
echo   2. Build the Vue frontend with npm.cmd
echo   3. Embed the frontend into code-Manager.exe with Go
echo   4. Create releases\code-Manager\code-Manager.exe
echo.

if not exist "%ICON_SOURCE%" (
    echo Build failed: tray icon source was not found: %ICON_SOURCE%
    goto :fail
)
if not exist "%ICON_RENDERER%" (
    echo Build failed: tray icon renderer was not found: %ICON_RENDERER%
    goto :fail
)
if not defined EDGE (
    echo Build failed: Microsoft Edge was not found. It is required to generate the tray icon.
    goto :fail
)
if not exist "%ROOT%\frontend\package.json" (
    echo Build failed: frontend\package.json was not found.
    goto :fail
)
if not exist "%UNINSTALL_TEMPLATE%" (
    echo Build failed: uninstall template was not found: %UNINSTALL_TEMPLATE%
    goto :fail
)

echo [1/5] Preparing a clean release directory...
if exist "%RELEASE_DIR%" rmdir /s /q "%RELEASE_DIR%"
if exist "%RELEASE_DIR%" (
    echo Build failed: cannot remove previous release directory: %RELEASE_DIR%
    goto :fail
)
mkdir "%RELEASE_DIR%"
if errorlevel 1 (
    echo Build failed: cannot create release directory: %RELEASE_DIR%
    goto :fail
)

echo [2/5] Generating the embedded tray icon...
set "ICON_RENDER_URI=file:///%ICON_RENDERER:\=/%"
"%EDGE%" --headless --disable-gpu --hide-scrollbars --force-device-scale-factor=1 --default-background-color=00000000 "--screenshot=%ICON_PNG%" --window-size=256,256 "%ICON_RENDER_URI%"
if errorlevel 1 (
    echo Build failed: tray icon PNG generation failed.
    goto :fail
)
go run .\tools\icon-to-ico.go "%ICON_PNG%" "%ICON_ICO%"
if errorlevel 1 (
    echo Build failed: tray icon ICO generation failed.
    goto :fail
)

echo [3/5] Installing frontend dependencies with npm.cmd...
pushd "%ROOT%\frontend"
call npm.cmd install
if errorlevel 1 (
    popd
    echo Build failed: npm.cmd install failed.
    goto :fail
)

echo [4/5] Building the Vue frontend with npm.cmd...
call npm.cmd run build
if errorlevel 1 (
    popd
    echo Build failed: npm.cmd run build failed.
    goto :fail
)
popd

echo [5/5] Building the single Windows GUI executable...
rem Go 1.27 的默认 HTTP/2 包装层会拒绝扩展 CONNECT 的 :protocol；
rem http2legacy 使用 x/net 的原生实现，供 H2 WebSocket 承载使用。
go build -tags http2legacy -ldflags "-H=windowsgui" -o "%OUTPUT_EXE%" .
if errorlevel 1 (
    echo Build failed: go build failed.
    goto :fail
)

if not exist "%OUTPUT_EXE%" (
    echo Build failed: output executable was not created.
    goto :fail
)

echo.
echo ========================================
echo Build succeeded: %OUTPUT_EXE%
echo ========================================
pause
exit /b 0

:fail
set "BUILD_EXIT=%ERRORLEVEL%"
if "%BUILD_EXIT%"=="0" set "BUILD_EXIT=1"
echo.
echo ========================================
echo Build failed, exit code: %BUILD_EXIT%
echo ========================================
popd
pause
exit /b %BUILD_EXIT%
