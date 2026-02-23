@echo off
title Construyendo RS Optimizer
cls
echo ==========================================
echo    CONSTRUYENDO EJECUTABLE INDEPENDIENTE
echo ==========================================
echo.

REM 1. Compilar RS RAM Optimizer (C++)
echo [1/2] Compilando RS RAM Optimizer (Microprograma)...
cd RamOptimizer

REM Añadir rutas comunes de compiladores al PATH temporalmente para facilitar la vida del usuario
set "PATH=%PATH%;C:\msys64\ucrt64\bin;C:\msys64\mingw64\bin;C:\MinGW\bin"

where cl >nul 2>nul
if %errorlevel%==0 (
    echo    [-] Usando MSVC...
    rc resource.rc
    cl main.cpp resource.res /Fe:"RS RAM Optimizer.exe" User32.lib Psapi.lib Shell32.lib /link /SUBSYSTEM:WINDOWS
    del *.obj *.res >nul 2>nul
    goto :compile_csharp
)

where g++ >nul 2>nul
if %errorlevel%==0 (
    echo    [-] Usando MinGW/GCC...
    windres resource.rc -O coff -o resource.res
    g++ main.cpp resource.res -o "RS RAM Optimizer.exe" -O3 -mwindows -lpsapi -luser32 -lshell32 -lpdh -static-libgcc -static-libstdc++
    del resource.res >nul 2>nul
    goto :compile_csharp
)

color 4
echo [ADVERTENCIA] No se encontro compilador C++ (cl.exe o g++) en el PATH actual.
echo Usando el ejecutable preexistente si esta disponible (sin los ultimos cambios de codigo).
if exist "RS RAM Optimizer.exe" (
    echo    [+] "RS RAM Optimizer.exe" precompilado encontrado.
) else (
    echo [ERROR] No se encontro "RS RAM Optimizer.exe".
    pause
    exit /b 1
)

:compile_csharp
cd ..
echo.

REM 2. Compilar RS Optimizer Principal (C#)
echo [2/2] Compilando RS Optimizer Principal (.NET)...
set OUTPUT_DIR=Build_Final

REM Limpiar carpeta anterior si existe
if exist "%OUTPUT_DIR%" (
    echo    [-] Limpiando build anterior...
    rd /s /q "%OUTPUT_DIR%"
)

mkdir "%OUTPUT_DIR%"
echo.
echo Compilando... (esto puede tardar unos segundos)
echo.

REM Ejecutar dotnet publish
REM -c Release: optimizado para produccion
REM -r win-x64: para Windows 64 bits
REM --self-contained true: incluye .NET dentro del exe (no necesita instalar nada)
REM -p:PublishSingleFile=true: un solo archivo .exe (sin dlls sueltas)
REM -o "%OUTPUT_DIR%": guardarlo en nuestra carpeta
dotnet publish -c Release -r win-x64 --self-contained true -p:PublishSingleFile=true -o "%OUTPUT_DIR%"

if %errorlevel% neq 0 (
    color 4
    echo.
    echo [ERROR] La compilacion fallo!
    pause
    exit /b %errorlevel%
)

color 2
echo.
echo ==========================================
echo          COMPILACION EXITOSA
echo ==========================================
echo.
echo El ejecutable esta listo en la carpeta: %OUTPUT_DIR%
echo Nombre del archivo: RS Optimizer.exe
echo.
pause
