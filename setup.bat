@echo off
setlocal enabledelayedexpansion

title TOFU Setup and Installer

echo ============================================================
echo   TOFU - Lossless Container Setup ^& Global CLI Installer
echo ============================================================
echo.

:: 1. Check for Rust / Cargo
echo [*] Checking for Rust toolchain...
where cargo >nul 2>&1
if %ERRORLEVEL% EQU 0 (
    for /f "tokens=*" %%i in ('cargo --version') do set CARGO_VER=%%i
    echo [+] Rust is already installed: !CARGO_VER!
    goto :BUILD_TOFU
)

:: Also check default Cargo path if not yet in current cmd PATH
if exist "%USERPROFILE%\.cargo\bin\cargo.exe" (
    set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
    for /f "tokens=*" %%i in ('cargo --version') do set CARGO_VER=%%i
    echo [+] Found Rust in user profile: !CARGO_VER!
    goto :ENSURE_PATH
)

:: 2. Rust is missing - download and install rustup
echo [!] Rust/Cargo was not found on your system.
echo [*] Downloading rustup-init for Windows (x64)...
powershell -Command "[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; (New-Object Net.WebClient).DownloadFile('https://win.rustup.rs/x86_64', 'rustup-init.exe')"
if not exist "rustup-init.exe" (
    echo [x] Error: Failed to download rustup-init.exe.
    echo Please install Rust manually from https://rustup.rs
    pause
    exit /b 1
)

echo [*] Installing Rust toolchain (default profile)...
rustup-init.exe -y --default-toolchain stable
del /f /q rustup-init.exe >nul 2>&1

:: Refresh path in current session
if exist "%USERPROFILE%\.cargo\bin\cargo.exe" (
    set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
    echo [+] Rust successfully installed.
) else (
    echo [x] Error: Rust installation did not complete as expected.
    echo Please restart your terminal or install Rust from https://rustup.rs
    pause
    exit /b 1
)

:ENSURE_PATH
:: 3. Ensure %USERPROFILE%\.cargo\bin is permanently in User PATH
echo [*] Ensuring Cargo bin directory is in User PATH...
powershell -NoProfile -Command ^
    "$cargoBin = [System.IO.Path]::Combine($env:USERPROFILE, '.cargo', 'bin');" ^
    "$userPath = [Environment]::GetEnvironmentVariable('Path', 'User');" ^
    "if ($userPath -notlike ('*' + $cargoBin + '*')) {" ^
    "    [Environment]::SetEnvironmentVariable('Path', $userPath.TrimEnd(';') + ';' + $cargoBin, 'User');" ^
    "    Write-Host '  [+] Added ' $cargoBin ' to User PATH';" ^
    "} else {" ^
    "    Write-Host '  [+] Cargo bin is already in User PATH';" ^
    "}"

:BUILD_TOFU
:: 4. Build and install TOFU globally
echo.
echo [*] Building and installing TOFU CLI globally...
cd /d "%~dp0"
cargo install --path crates/tofu-cli --force
if %ERRORLEVEL% NEQ 0 (
    echo [x] Error: Failed to compile and install TOFU CLI.
    pause
    exit /b %ERRORLEVEL%
)

:: 5. Also build the standalone release binary
echo.
echo [*] Generating optimized standalone release binary...
cargo build --release
if %ERRORLEVEL% NEQ 0 (
    echo [!] Warning: Standalone release build had an issue, but global CLI was installed.
) else (
    echo [+] Standalone binary ready at: %~dp0target\release\tofu.exe
)

:: 6. Verification
echo.
echo [*] Verifying installation...
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
where tofu >nul 2>&1
if %ERRORLEVEL% EQU 0 (
    tofu.exe version
    echo.
    echo ============================================================
    echo   [+] SUCCESS: TOFU is installed and ready.
    echo ============================================================
    echo.
    echo You can now open ANY CMD or PowerShell terminal and run:
    echo.
    echo     tofu pack
    echo     tofu unpack
    echo     tofu list
    echo     tofu verify
    echo.
) else (
    echo [+] TOFU was installed to %USERPROFILE%\.cargo\bin\tofu.exe
    echo Please reopen your terminal window to refresh your PATH.
)

pause
