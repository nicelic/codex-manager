@echo off
setlocal

echo ==================================================
echo   Building code-Manager-rust (One-Click Release)
echo ==================================================
echo.

cd /d "%~dp0"

echo [1/3] Building frontend assets (Vite)...
cd frontend
if not exist "node_modules" (
    echo Installing frontend dependencies...
    call npm install
    if errorlevel 1 (
        echo [ERROR] npm install failed!
        cd ..
        pause
        exit /b 1
    )
)

call npm run build
if errorlevel 1 (
    echo [ERROR] Frontend build failed!
    cd ..
    pause
    exit /b 1
)
cd ..

if not exist "frontend\dist\index.html" (
    echo [ERROR] frontend\dist\index.html not found!
    pause
    exit /b 1
)
echo [1/3] Frontend build complete.
echo.

echo [2/3] Compiling Rust backend (Release mode)...
cd backend
cargo build --release
if errorlevel 1 (
    echo [ERROR] Cargo build --release failed!
    cd ..
    pause
    exit /b 1
)
cd ..

if not exist "backend\target\release\code-Manager-rust.exe" (
    echo [ERROR] backend\target\release\code-Manager-rust.exe not found!
    pause
    exit /b 1
)
echo [2/3] Backend compilation complete.
echo.

echo [3/3] Packaging executable to releases folder...
if not exist "releases" (
    mkdir "releases"
)

copy /y "backend\target\release\code-Manager-rust.exe" "releases\code-Manager-rust.exe" > nul
if errorlevel 1 (
    echo [ERROR] Copying to releases\code-Manager-rust.exe failed!
    pause
    exit /b 1
)

echo.
echo ==================================================
echo   Build Successful!
echo ==================================================
echo Output File: %~dp0releases\code-Manager-rust.exe
echo Frontend is fully embedded into the standalone executable.
echo ==================================================
echo.
pause
