@echo off
title Compilador de RS Optimizer (Rust Native)
color 0b
echo ==============================================
echo     COMPILANDO RS OPTIMIZER EN RUST NATIVO
echo ==============================================
echo.

REM 1. Verificar si cargo existe (Rust instalado)
where cargo >nul 2>nul
if errorlevel 1 goto INSTALL_RUST

:CHECK_DONE
echo [OK] Rust y Cargo detectados en el sistema.
goto COMPILE

:INSTALL_RUST
color 0e
echo [!] Rust/Cargo no esta instalado.
echo.
echo Descargando rustup-init...
curl.exe -sSfL "https://win.rustup.rs/x86_64" -o rustup-init.exe
if errorlevel 1 (
    color 4
    echo [ERROR] No se pudo descargar rustup-init.exe.
    pause
    exit /b 1
)

echo Instalando Rust (esto tomara un par de minutos, por favor espera)...
rustup-init.exe -y -q

REM Refrescar el PATH sumando la carpeta de Cargo
set "PATH=%PATH%;%USERPROFILE%\.cargo\bin"

where cargo >nul 2>nul
if errorlevel 1 (
    color 4
    echo [ERROR] Fallo la instalacion de Rust.
    pause
    exit /b 1
)

color 0a
echo [OK] Rust instalado correctamente!
color 0b
echo.
goto COMPILE

:COMPILE
REM 2. Compilar
echo.
echo [1/1] Descargando y compilando RS Optimizer en Rust...
cd RustOptimizer

echo Ejecutando Cargo Build Release...
cargo build --release

if errorlevel 1 (
    color 0e
    echo.
    echo [!] Advertencia: Windows Defender o tu editor de codigo (EJ: rust-analyzer) 
    echo esta bloqueando los archivos temporales (error 32).
    echo Reintentando la compilacion en 3 segundos...
    ping 127.0.0.1 -n 4 >nul
    
    cargo build --release
    if errorlevel 1 (
        color 4
        echo.
        echo [ERROR] La compilacion de Rust fallo definitivamente.
        pause
        exit /b 1
    )
)

echo.
echo ==============================================
color 2
echo             COMPILACION EXITOSA
echo ==============================================
echo.
color 0f
echo Tu ejecutable ultra-ligero se compilo con exito.
echo Moviendo el archivo al directorio Build_Final...
if not exist "..\Build_Final" mkdir "..\Build_Final"
move /Y "target\release\rs_optimizer.exe" "..\Build_Final\RS Optimizer.exe" >nul
echo.
pause
