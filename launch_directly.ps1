$ErrorActionPreference = 'SilentlyContinue'

$isAdmin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Output "Requesting Admin elevation to stop elevated task process..."
    Start-Process powershell -ArgumentList "-ExecutionPolicy Bypass -File `"$($MyInvocation.MyCommand.Path)`"" -Verb RunAs -Wait
    exit
}

Write-Output "Stopping elevated RS RAM Optimizer process..."
Stop-Process -Name "RS RAM Optimizer" -Force
taskkill /F /IM "RS RAM Optimizer.exe"

# Obtener ruta
$exePath = 'C:\Users\WinterOS\AppData\Roaming\RickStyles\RSOptimizer\RS RAM Optimizer.exe'

Write-Output "Launching RS RAM Optimizer directly..."
Start-Process -FilePath $exePath

Write-Output "Successfully launched directly!"
