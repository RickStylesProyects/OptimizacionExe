$ErrorActionPreference = 'SilentlyContinue'
$ProgressPreference = 'SilentlyContinue'
$LogPath = "C:\Users\WinterOS\Desktop\Programacion\OptimizacionExe\test_hw.log"

function Add-Log {
    param([string]$Kind, [string]$Category, [string]$Message, [string]$Status = 'OK')
    $line = "$Kind|$Category|$Status|$Message"
    Add-Content -LiteralPath $LogPath -Value $line -Encoding UTF8
}

function Safe-Name {
    param($Value)
    if ($null -eq $Value) { return '' }
    return ([string]$Value).Replace("`r", " ").Replace("`n", " ").Trim()
}

Remove-Item -LiteralPath $LogPath -Force -ErrorAction SilentlyContinue
New-Item -ItemType File -Path $LogPath -Force | Out-Null

# ── BCD Optimizations ──────────────────────────────────────────────────────
try {
    bcdedit /set disabledynamictick yes | Out-Null
    Add-Log 'INFO' 'BCD' 'disabledynamictick = yes'
} catch {
    Add-Log 'WARN' 'BCD' 'No se pudo aplicar disabledynamictick' 'WARN'
}

try {
    bcdedit /set useplatformtick yes | Out-Null
    Add-Log 'INFO' 'BCD' 'useplatformtick = yes'
} catch {
    Add-Log 'WARN' 'BCD' 'No se pudo aplicar useplatformtick' 'WARN'
}

try {
    bcdedit /set useplatformclock no | Out-Null
    Add-Log 'INFO' 'BCD' 'useplatformclock = no (Reloj TSC forzado)'
} catch {
    Add-Log 'WARN' 'BCD' 'No se pudo aplicar useplatformclock' 'WARN'
}

try {
    # ── Desactivar inicio de Virtual Secure Mode (VBS/Core Isolation boot bypass)
    bcdedit /set vsmlaunchtype Off | Out-Null
    Add-Log 'INFO' 'BCD' 'vsmlaunchtype = Off (VBS/Core Isolation desactivado al inicio)'
} catch {
    Add-Log 'WARN' 'BCD' 'No se pudo desactivar vsmlaunchtype' 'WARN'
}

try {
    bcdedit /set nx OptIn | Out-Null
    Add-Log 'INFO' 'BCD' 'DEP = OptIn'
} catch {
    Add-Log 'WARN' 'BCD' 'No se pudo ajustar DEP' 'WARN'
}

try {
    bcdedit /set tscsyncpolicy Enhanced | Out-Null
    Add-Log 'INFO' 'BCD' 'tscsyncpolicy = Enhanced (Sincronizacion TSC forzada)'
} catch {
    Add-Log 'WARN' 'BCD' 'No se pudo aplicar tscsyncpolicy' 'WARN'
}

# ── Windows Memory Management & Compression Tuning ─────────────────────────
try {
    # ── Desactivar compresión de memoria (MMAgent Memory Compression Off)
    Disable-MMAgent -MemoryCompression | Out-Null
    Add-Log 'INFO' 'KERNEL' 'MemoryCompression = OFF (Compresion de RAM desactivada)'
} catch {
    Add-Log 'WARN' 'KERNEL' 'No se pudo desactivar la compresion de memoria' 'WARN'
}

try {
    # ── Desactivar VBS e Integridad de Memoria (HVCI) en el Registro
    $vbsPath = "HKLM:\System\CurrentControlSet\Control\DeviceGuard"
    if (!(Test-Path $vbsPath)) { New-Item -Path $vbsPath -Force | Out-Null }
    New-ItemProperty -Path $vbsPath -Name "EnableVirtualizationBasedSecurity" -Value 0 -PropertyType DWord -Force | Out-Null
    
    $hvciPath = "HKLM:\System\CurrentControlSet\Control\DeviceGuard\Scenarios\HypervisorEnforcedCodeIntegrity"
    if (!(Test-Path $hvciPath)) { New-Item -Path $hvciPath -Force | Out-Null }
    New-ItemProperty -Path $hvciPath -Name "Enabled" -Value 0 -PropertyType DWord -Force | Out-Null
    Add-Log 'INFO' 'KERNEL' 'HVCI / VBS deshabilitado en Registro'
} catch {
    Add-Log 'WARN' 'KERNEL' 'No se pudo deshabilitar VBS/HVCI en el Registro' 'WARN'
}

try {
    # ── Forzar Kernel y Controladores (Drivers) a RAM física (DisablePagingExecutive)
    $mmPath = "HKLM:\System\CurrentControlSet\Control\Session Manager\Memory Management"
    New-ItemProperty -Path $mmPath -Name "DisablePagingExecutive" -Value 1 -PropertyType DWord -Force | Out-Null
    Add-Log 'INFO' 'KERNEL' 'DisablePagingExecutive = 1 (Kernel y Controladores en RAM)'
} catch {
    Add-Log 'WARN' 'KERNEL' 'No se pudo aplicar DisablePagingExecutive' 'WARN'
}

# ── Pagefile Optimization ──────────────────────────────────────────────────
try {
    $ramBytes = (Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory
    $ramGB = [Math]::Round($ramBytes / 1GB)
    
    if ($ramGB -ge 32) { $size = 4096 }
    elseif ($ramGB -ge 16) { $size = 8192 }
    else { $size = 12288 }
    
    $cs = Get-CimInstance Win32_ComputerSystem
    if ($cs.AutomaticManagedPagefile) {
        $cs.AutomaticManagedPagefile = $false
        Set-CimInstance -CimInstance $cs -ErrorAction Stop | Out-Null
    }
    
    $pf = Get-CimInstance Win32_PageFileSetting
    if ($pf) {
        $pf.InitialSize = $size
        $pf.MaximumSize = $size
        Set-CimInstance -CimInstance $pf -ErrorAction Stop | Out-Null
    } else {
        New-CimInstance -ClassName Win32_PageFileSetting -Property @{ Name = 'C:\pagefile.sys'; InitialSize = $size; MaximumSize = $size } -ErrorAction Stop | Out-Null
    }
    Add-Log 'INFO' 'PAGING' ("Pagefile estatico optimizado a " + $size + " MB (RAM: " + $ramGB + " GB)")
} catch {
    Add-Log 'WARN' 'PAGING' 'No se pudo configurar el archivo de paginacion estatico' 'WARN'
}

# ── NTFS File System Optimization ──────────────────────────────────────────
try {
    fsutil behavior set disablelastaccess 1 | Out-Null
    fsutil behavior set disable8dot3 1 | Out-Null
    fsutil behavior set memoryusage 2 | Out-Null
    
    $ramBytes = (Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory
    $ramGB = [Math]::Round($ramBytes / 1GB)
    if ($ramGB -ge 32) { $mftZoneVal = 4 }
    elseif ($ramGB -ge 16) { $mftZoneVal = 3 }
    else { $mftZoneVal = 2 }
    
    fsutil behavior set mftzone $mftZoneVal | Out-Null
    Add-Log 'INFO' 'NTFS' ("NTFS optimizado: LastAccess=OFF, 8dot3=OFF, MemoryUsage=2, MftZone=" + $mftZoneVal)
} catch {
    Add-Log 'WARN' 'NTFS' 'No se pudieron aplicar todas las optimizaciones NTFS' 'WARN'
}

# ── CPU Kernel Scheduling (Win32PrioritySeparation = 38) ───────────────────
try {
    Set-ItemProperty -Path "HKLM:\System\CurrentControlSet\Control\PriorityControl" -Name "Win32PrioritySeparation" -Value 38 | Out-Null
    Add-Log 'INFO' 'KERNEL' 'Win32PrioritySeparation optimizado a 38 (0x26 - Foreground boost)'
} catch {
    Add-Log 'WARN' 'KERNEL' 'No se pudo aplicar Win32PrioritySeparation' 'WARN'
}

# ── Speculative Execution Control (Spectre/Meltdown mitigations bypass) ────
try {
    $path = "HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management"
    New-ItemProperty -Path $path -Name "FeatureSettingsOverride" -Value 3 -PropertyType DWord -Force | Out-Null
    New-ItemProperty -Path $path -Name "FeatureSettingsOverrideMask" -Value 3 -PropertyType DWord -Force | Out-Null
    Add-Log 'INFO' 'KERNEL' 'Mitigaciones de CPU (Spectre/Meltdown) desactivadas para rendimiento'
} catch {
    Add-Log 'WARN' 'KERNEL' 'No se pudieron desactivar las mitigaciones de CPU' 'WARN'
}

# ── GPU Hardware-Accelerated Scheduling (HAGS) & TDR Delay ──────────────────
try {
    $path = "HKLM:\System\CurrentControlSet\Control\GraphicsDrivers"
    New-ItemProperty -Path $path -Name "HwSchMode" -Value 2 -PropertyType DWord -Force | Out-Null
    New-ItemProperty -Path $path -Name "TdrDelay" -Value 10 -PropertyType DWord -Force | Out-Null
    Add-Log 'INFO' 'KERNEL' 'GPU Tuning: HAGS forzado y TdrDelay fijados'
} catch {
    Add-Log 'WARN' 'KERNEL' 'No se pudieron aplicar ajustes avanzados de GPU' 'WARN'
}

try {
    $powerPath = 'HKLM:\SYSTEM\CurrentControlSet\Control\Power'
    if (!(Test-Path $powerPath)) { New-Item -Path $powerPath -Force | Out-Null }
    New-ItemProperty -Path $powerPath -Name 'DisableInterruptSteering' -Value 1 -PropertyType DWord -Force | Out-Null
    Add-Log 'INFO' 'POWER' 'DisableInterruptSteering = 1 (Interrupt steering desactivado)'
} catch {
    Add-Log 'WARN' 'POWER' 'No se pudo aplicar DisableInterruptSteering' 'WARN'
}
