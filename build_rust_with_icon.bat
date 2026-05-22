@echo off
setlocal EnableExtensions EnableDelayedExpansion
title Compilador de RS Optimizer + RS RAM Optimizer
color 0b

echo ==============================================
echo     COMPILANDO RS OPTIMIZER + RAM OPTIMIZER
echo ==============================================
echo.

set "ROOT_DIR=%~dp0"
if "%ROOT_DIR:~-1%"=="\" set "ROOT_DIR=%ROOT_DIR:~0,-1%"

set "RUST_DIR=%ROOT_DIR%\RustOptimizer"
set "RAM_DIR=%ROOT_DIR%\RamOptimizer"
set "FINAL_DIR=%ROOT_DIR%\Build_Final"
set "RUST_EXE=%FINAL_DIR%\RS Optimizer.exe"
set "RAM_EXE=%RAM_DIR%\RS RAM Optimizer.exe"

set "CPP_SRC=%RAM_DIR%\main.cpp"
set "RC_SRC=%RAM_DIR%\resource.rc"
set "RUST_SRC=%RUST_DIR%\src\main.rs"

echo [INFO] Directorio raiz: %ROOT_DIR%
echo [INFO] Rust dir: %RUST_DIR%
echo [INFO] RAM dir:  %RAM_DIR%
echo [INFO] C++ src:  %CPP_SRC%
echo.

REM 1. Verificar Rust/Cargo
where cargo >nul 2>nul
if errorlevel 1 goto INSTALL_RUST

echo [OK] Rust y Cargo detectados en el sistema.

REM 2. Verificar compilador C++
call :CHECK_CPP_COMPILER
if errorlevel 1 (
    color 4
    echo [ERROR] No se encontro compilador C++ compatible.
    pause
    exit /b 1
)

echo [OK] Compilador C++ detectado: %CPP_COMPILER%
echo.

REM 3. Preparar directorios
if not exist "%FINAL_DIR%" mkdir "%FINAL_DIR%"

REM 4. Eliminar EXEs anteriores
call :DELETE_OLD_EXE "%RAM_EXE%"
call :DELETE_OLD_EXE "%RUST_EXE%"

REM 5. Compilar RAM Optimizer primero
echo [1/2] Compilando RS RAM Optimizer (con icono)...
if not exist "%RAM_DIR%" (
    color 4
    echo [ERROR] No existe la carpeta RamOptimizer.
    pause
    exit /b 1
)

pushd "%RAM_DIR%"

REM Compilar recurso RC a RES u OBJ dependiendo del compilador
set "RES_FILE="
if exist "%RC_SRC%" (
    echo [INFO] Archivo de recursos detectado: %RC_SRC%
    call :COMPILE_RC "%RC_SRC%"
    if errorlevel 1 (
        echo [WARN] Fallo la compilacion del icono. Se compilara sin icono.
        set "RES_FILE="
    )
) else (
    echo [WARN] No se encontro resource.rc. Se compilara sin icono.
)

call :COMPILE_CPP "%CPP_SRC%" "%RAM_EXE%" "!RES_FILE!"
if errorlevel 1 (
    popd
    color 4
    echo [ERROR] La compilacion del RAM Optimizer fallo.
    pause
    exit /b 1
)

REM Limpiar archivos intermedios de recursos
if exist "resource.res" del "resource.res"
if exist "resource.o" del "resource.o"

popd

if not exist "%RAM_EXE%" (
    color 4
    echo [ERROR] No se genero el archivo %RAM_EXE%
    pause
    exit /b 1
)

echo [OK] RS RAM Optimizer compilado exitosamente.
echo.

REM 6. Compilar Rust Optimizer
echo [2/2] Compilando RS Optimizer en Rust...
if not exist "%RUST_DIR%" (
    color 4
    echo [ERROR] No existe la carpeta RustOptimizer.
    pause
    exit /b 1
)

pushd "%RUST_DIR%"
echo Ejecutando Cargo Build Release...
cargo build --release
if errorlevel 1 (
    color 0e
    echo.
    echo [!] Primera compilacion Rust fallo. Reintentando en 3 segundos...
    ping 127.0.0.1 -n 4 >nul
    cargo build --release
    if errorlevel 1 (
        popd
        color 4
        echo [ERROR] La compilacion de Rust fallo definitivamente.
        pause
        exit /b 1
    )
)

if not exist "target\release\rs_optimizer.exe" (
    popd
    color 4
    echo [ERROR] Cargo termino pero no se encontro target\release\rs_optimizer.exe
    pause
    exit /b 1
)

move /Y "target\release\rs_optimizer.exe" "%RUST_EXE%" >nul
popd

echo.
echo ==============================================
color 2
echo              COMPILACION EXITOSA
echo ==============================================
color 0f
echo [OK] RS RAM Optimizer  -^> %RAM_EXE%
echo [OK] RS Optimizer      -^> %RUST_EXE%
echo.
pause
exit /b 0


:CHECK_CPP_COMPILER
set "CPP_COMPILER="
where cl >nul 2>nul
if not errorlevel 1 (
    set "CPP_COMPILER=MSVC"
    exit /b 0
)
where clang++ >nul 2>nul
if not errorlevel 1 (
    set "CPP_COMPILER=CLANG"
    exit /b 0
)
where g++ >nul 2>nul
if not errorlevel 1 (
    set "CPP_COMPILER=GPP"
    exit /b 0
)
exit /b 1


:DELETE_OLD_EXE
set "TARGET_EXE=%~1"
if exist "%TARGET_EXE%" (
    echo [INFO] Eliminando ejecutable antiguo: "%TARGET_EXE%"
    attrib -r -h -s "%TARGET_EXE%" >nul 2>nul
    del /F /Q "%TARGET_EXE%" >nul 2>nul
    if exist "%TARGET_EXE%" (
        taskkill /F /IM "%~nx1" >nul 2>nul
        ping 127.0.0.1 -n 3 >nul
        del /F /Q "%TARGET_EXE%" >nul 2>nul
    )
)
exit /b 0

:COMPILE_RC
set "RC_IN=%~1"
if /I "%CPP_COMPILER%"=="MSVC" (
    call :ENSURE_MSVC_ENV
    rc.exe /nologo /fo "resource.res" "%RC_IN%"
    if not errorlevel 1 (
        set "RES_FILE=resource.res"
        exit /b 0
    )
)
if /I "%CPP_COMPILER%"=="CLANG" (
    REM llvm-rc es el compilador de recursos de LLVM
    where llvm-rc >nul 2>nul
    if not errorlevel 1 (
        llvm-rc "%RC_IN%" /FO "resource.res"
        if not errorlevel 1 (
            set "RES_FILE=resource.res"
            exit /b 0
        )
    )
    REM Si no hay llvm-rc pero hay rc de MSVC
    where rc >nul 2>nul
    if not errorlevel 1 (
        rc.exe /nologo /fo "resource.res" "%RC_IN%"
        if not errorlevel 1 (
            set "RES_FILE=resource.res"
            exit /b 0
        )
    )
)
if /I "%CPP_COMPILER%"=="GPP" (
    windres "%RC_IN%" -O coff -o "resource.o"
    if not errorlevel 1 (
        set "RES_FILE=resource.o"
        exit /b 0
    )
)
exit /b 1

:COMPILE_CPP
set "C_IN=%~1"
set "C_OUT=%~2"
set "C_RES=%~3"

if /I "%CPP_COMPILER%"=="MSVC" (
    call :ENSURE_MSVC_ENV
    if defined C_RES (
        cl /nologo /O2 /MT /EHsc /DNDEBUG "%C_IN%" "%C_RES%" /Fe:"%C_OUT%" /link user32.lib shell32.lib psapi.lib pdh.lib comctl32.lib /SUBSYSTEM:WINDOWS
    ) else (
        cl /nologo /O2 /MT /EHsc /DNDEBUG "%C_IN%" /Fe:"%C_OUT%" /link user32.lib shell32.lib psapi.lib pdh.lib comctl32.lib /SUBSYSTEM:WINDOWS
    )
    exit /b %errorlevel%
)

if /I "%CPP_COMPILER%"=="CLANG" (
    if defined C_RES (
        clang++ -O2 "%C_IN%" "%C_RES%" -o "%C_OUT%" -Wl,-subsystem:windows -lpsapi -lshell32 -luser32 -lpdh -lcomctl32
    ) else (
        clang++ -O2 "%C_IN%" -o "%C_OUT%" -Wl,-subsystem:windows -lpsapi -lshell32 -luser32 -lpdh -lcomctl32
    )
    exit /b %errorlevel%
)

if /I "%CPP_COMPILER%"=="GPP" (
    if defined C_RES (
        g++ -O2 -s -municode -mwindows "%C_IN%" "%C_RES%" -o "%C_OUT%" -lpsapi -lshell32 -luser32 -lpdh -lcomctl32
    ) else (
        g++ -O2 -s -municode -mwindows "%C_IN%" -o "%C_OUT%" -lpsapi -lshell32 -luser32 -lpdh -lcomctl32
    )
    exit /b %errorlevel%
)

exit /b 1

:ENSURE_MSVC_ENV
where cl >nul 2>nul
if not errorlevel 1 exit /b 0
if defined VSINSTALLDIR exit /b 0

for %%Y in (2022 2019) do (
    for %%E in (Enterprise Professional Community BuildTools) do (
        if exist "%ProgramFiles(x86)%\Microsoft Visual Studio\%%Y\%%E\VC\Auxiliary\Build\vcvars64.bat" (
            call "%ProgramFiles(x86)%\Microsoft Visual Studio\%%Y\%%E\VC\Auxiliary\Build\vcvars64.bat" >nul
            where cl >nul 2>nul
            if not errorlevel 1 exit /b 0
        )
    )
)
exit /b 1
