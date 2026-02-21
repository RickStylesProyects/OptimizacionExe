@echo off
title Construyendo RS Optimizer
cls
echo ==========================================
echo    CONSTRUYENDO EJECUTABLE INDEPENDIENTE
echo ==========================================
echo.

REM Definir carpeta de salida
set OUTPUT_DIR=Build_Final

REM Limpiar carpeta anterior si existe
if exist "%OUTPUT_DIR%" (
    echo Limpiando carpeta anterior...
    rd /s /q "%OUTPUT_DIR%"
)

REM Crear nueva carpeta
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
