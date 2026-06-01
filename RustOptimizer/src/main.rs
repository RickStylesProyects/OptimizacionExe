use std::env;
use std::fs;
use std::io::{self, Write};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;

const CREATE_NO_WINDOW: u32 = 0x08000000;

const REG_RESOURCE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/embedded.reg"));
const EXE_RESOURCE: &[u8] = include_bytes!("../../RamOptimizer/RS RAM Optimizer.exe");

// ═══════════════════════════════════════════════════════════════════════════
//  UTILIDADES: PowerShell
// ═══════════════════════════════════════════════════════════════════════════

fn run_powershell_command(command: &str) -> Result<String, String> {
    let output = Command::new("powershell")
        .args(&[
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            command,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("PowerShell error: {:?}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if !output.status.success() {
        let msg = if !stderr.is_empty() { stderr } else { stdout };
        return Err(format!("PowerShell exit code non-zero: {}", msg));
    }

    Ok(stdout)
}

// ═══════════════════════════════════════════════════════════════════════════
//  UTILIDADES: Colores ANSI
// ═══════════════════════════════════════════════════════════════════════════

fn set_color_green() {
    print!("\x1b[32m");
}
fn set_color_yellow() {
    print!("\x1b[33m");
}
fn set_color_cyan() {
    print!("\x1b[36m");
}
fn set_color_dark_gray() {
    print!("\x1b[90m");
}
fn set_color_red() {
    print!("\x1b[31m");
}
fn reset_color() {
    print!("\x1b[0m");
}

#[cfg(windows)]
fn enable_virtual_terminal_processing() {
    type Handle = *mut std::ffi::c_void;
    type Dword = u32;
    const STD_OUTPUT_HANDLE: u32 = 0xFFFFFFF5;
    const ENABLE_VIRTUAL_TERMINAL_PROCESSING: Dword = 0x0004;

    extern "system" {
        fn GetStdHandle(nStdHandle: u32) -> Handle;
        fn GetConsoleMode(hConsoleHandle: Handle, lpMode: *mut Dword) -> i32;
        fn SetConsoleMode(hConsoleHandle: Handle, dwMode: Dword) -> i32;
    }

    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        let mut mode: Dword = 0;
        if GetConsoleMode(handle, &mut mode) != 0 {
            let _ = SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
        }
    }
}

fn print_status_line(desc: &str, ok: bool, warn: bool) {
    print!("  {} ", desc);
    for _ in 0..72_usize.saturating_sub(2 + desc.len()) {
        print!(" ");
    }
    io::stdout().flush().unwrap();

    if ok {
        set_color_green();
        println!("[ OK ]");
    } else if warn {
        set_color_yellow();
        println!("[ ADVERTENCIA ]");
    } else {
        set_color_dark_gray();
        println!("[ NO APLICADO ]");
    }
    reset_color();
}

/// Intenta obtener el valor máximo válido de una propiedad avanzada del NIC.
/// Devuelve Some(max_value) si la propiedad existe y tiene valores válidos.
fn get_nic_property_max(adapter: &str, keyword: &str) -> Option<u32> {
    let cmd = format!(
        "try {{ $p = Get-NetAdapterAdvancedProperty -Name '{}' -RegistryKeyword '{}' -ErrorAction Stop; \
         $vals = $p.ValidDisplayValues | ForEach-Object {{ [uint32]$_ }} | Where-Object {{ $_ -gt 0 }}; \
         if ($vals) {{ ($vals | Measure-Object -Maximum).Maximum }} else {{ 0 }} \
         }} catch {{ 0 }}",
        adapter, keyword
    );
    run_powershell_command(&cmd)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .filter(|&v| v > 0)
}

/// Intenta establecer una propiedad avanzada del NIC. Devuelve true si tuvo éxito.
fn set_nic_property(adapter: &str, keyword: &str, value: u32) -> bool {
    let cmd = format!(
        "Set-NetAdapterAdvancedProperty -Name '{}' -RegistryKeyword '{}' -RegistryValue {} -ErrorAction Stop",
        adapter, keyword, value
    );
    run_powershell_command(&cmd).is_ok()
}

/// Intenta establecer una propiedad usando múltiples posibles nombres de keyword.
/// Devuelve true si alguno tuvo éxito.
fn set_nic_property_multi(adapter: &str, keywords: &[&str], value: u32) -> bool {
    for kw in keywords {
        if set_nic_property(adapter, kw, value) {
            return true;
        }
    }
    false
}

/// Obtiene la lista de adaptadores de red físicos a optimizar (filtrando virtuales como VirtualBox, VMware, VPNs, etc.).
fn get_active_adapters() -> Vec<String> {
    run_powershell_command(
        "Get-NetAdapter -Physical | Where-Object { $_.InterfaceDescription -notmatch 'VirtualBox|VMware|Virtual|Hyper-V|vEthernet|Loopback|TAP' -and $_.Name -notmatch 'VirtualBox|VMware|Virtual|Hyper-V|vEthernet|Loopback|TAP' } | Select-Object -ExpandProperty Name",
    )
    .unwrap_or_default()
    .lines()
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty())
    .collect()
}

fn is_admin() -> bool {
    #[cfg(windows)]
    {
        #[link(name = "shell32")]
        extern "system" {
            fn IsUserAnAdmin() -> i32;
        }
        unsafe { IsUserAnAdmin() != 0 }
    }
    #[cfg(not(windows))]
    true
}

fn run_as_admin() -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use std::ptr;

        #[link(name = "shell32")]
        extern "system" {
            fn ShellExecuteW(
                hwnd: *mut std::ffi::c_void,
                lpOperation: *const u16,
                lpFile: *const u16,
                lpParameters: *const u16,
                lpDirectory: *const u16,
                nShowCmd: i32,
            ) -> *mut std::ffi::c_void;
        }

        let self_exe = match std::env::current_exe() {
            Ok(path) => path,
            Err(_) => return false,
        };

        let operation: Vec<u16> = std::ffi::OsStr::new("runas").encode_wide().chain(Some(0)).collect();
        let file: Vec<u16> = self_exe.as_os_str().encode_wide().chain(Some(0)).collect();

        // Pass original arguments
        let args: Vec<String> = std::env::args().skip(1).collect();
        let parameters_str = args.join(" ");
        let parameters: Vec<u16> = std::ffi::OsStr::new(&parameters_str).encode_wide().chain(Some(0)).collect();

        let result = unsafe {
            ShellExecuteW(
                ptr::null_mut(),
                operation.as_ptr(),
                file.as_ptr(),
                parameters.as_ptr(),
                ptr::null(),
                1, // SW_SHOWNORMAL
            )
        };

        (result as usize) > 32
    }
    #[cfg(not(windows))]
    true
}

// ═══════════════════════════════════════════════════════════════════════════
//  MAIN
// ═══════════════════════════════════════════════════════════════════════════

fn main() {
    #[cfg(windows)]
    enable_virtual_terminal_processing();

    if !is_admin() {
        set_color_red();
        println!("======================================================");
        println!("  [ ADVERTENCIA ] Este programa requiere permisos");
        println!("  de Administrador para optimizar el sistema.");
        println!("  Solicitando permisos de Administrador...");
        println!("======================================================");
        reset_color();

        if run_as_admin() {
            // Relaunch was successful, exit current instance
            return;
        } else {
            set_color_red();
            println!("\n  [ ERROR ] No se concedieron permisos de Administrador.");
            println!("  Por favor, haz clic derecho sobre el programa y elige");
            println!("  'Ejecutar como Administrador'.");
            println!("\n  Presiona Enter para salir...");
            reset_color();
            let mut input = String::new();
            let _ = std::io::stdin().read_line(&mut input);
            return;
        }
    }

    set_color_cyan();
    println!("======================================================");
    println!("       RS Optimizer v1.D                               ");
    println!("       by RickStyles                                   ");
    println!("======================================================");
    reset_color();

    // ── Paso 1: Registro base (.reg) ────────────────────────────────────
    println!("\n[1/9] Aplicando registro base stable (RegEnhancer)...");
    apply_embedded_registry();

    // ── Paso 2: Loopback MTU ────────────────────────────────────────────
    println!("\n[2/9] Optimizando parametros globales de Loopback...");
    optimize_loopback();

    // ── Paso 3: Algoritmo de congestion TCP (CTCP) ─────────────────────
    println!("\n[3/9] Configurando algoritmo de congestion TCP...");
    apply_congestion_providers();

    // ── Paso 4: Parametros TCP globales ─────────────────────────────────
    println!("\n[4/9] Ajustando parametros TCP globales...");
    apply_tcp_global_optimizations();

    // ── Paso 5: Offloads de adaptadores (RSC, LSO, RSS) ────────────────
    println!("\n[5/9] Optimizando offloads de adaptadores de red...");
    apply_adapter_offload_tweaks();

    // ── Paso 6: Propiedades avanzadas de NIC (buffers, IRQ, EEE, etc.) ─
    println!("\n[6/9] Ajustando propiedades avanzadas de NIC...");
    apply_nic_advanced_tweaks();

    // ── Paso 7: Firewall (bloqueo de telemetria) ────────────────────────
    println!("\n[7/9] Configurando Firewall (bloqueo de telemetria)...");
    apply_firewall_rules();

    // ── Paso 8: Hardware (MSI, BCD, CPU, DWM, MMCSS, IRQ affinity) ────
    println!("\n[8/9] Detectando y optimizando hardware...");
    apply_dynamic_hardware_tweaks();

    // ── Paso 9: RAM Optimizer ───────────────────────────────────────────
    println!("\n[9/9] Instalando y ejecutando RAM Optimizer...");
    install_and_run_ram_optimizer();

    // ── Estado final ────────────────────────────────────────────────────
    println!("\n======================================================");
    println!("Estado Final de la Configuracion:");
    println!("------------------------------------------------------");

    let final_tcp = run_powershell_command(
        "Get-NetTCPSetting -SettingName Internet | Select-Object SettingName, CongestionProvider, AutoTuningLevelGroup | Format-Table -AutoSize | Out-String",
    )
    .unwrap_or_default();
    println!("{}", final_tcp.trim());

    let final_nic = run_powershell_command(
        "Get-NetAdapter -Physical | Where-Object { $_.InterfaceDescription -notmatch 'VirtualBox|VMware|Virtual|Hyper-V|vEthernet|Loopback|TAP' -and $_.Name -notmatch 'VirtualBox|VMware|Virtual|Hyper-V|vEthernet|Loopback|TAP' } | Select-Object Name, Status, LinkSpeed, MacAddress | Format-Table -AutoSize | Out-String",
    )
    .unwrap_or_default();
    println!("Adaptadores de red detectados:");
    println!("{}", final_nic.trim());

    let final_tcp_global = run_powershell_command(
        "netsh int tcp show global",
    )
    .unwrap_or_default();
    println!("Parametros TCP globales:");
    println!("{}", final_tcp_global.trim());

    println!("======================================================");
    reset_color();

    println!("\nPresiona [Enter] para cerrar...");
    let mut input = String::new();
    let _ = io::stdin().read_line(&mut input);
}

// ═══════════════════════════════════════════════════════════════════════════
//  PASO 1: Loopback
// ═══════════════════════════════════════════════════════════════════════════

fn optimize_loopback() {
    let ok = run_powershell_command(
        "netsh int ipv4 set global loopbacklargemtu=disable; netsh int ipv6 set global loopbacklargemtu=disable",
    )
    .is_ok();
    print_status_line("Loopback Large MTU deshabilitado (IPv4 + IPv6)", ok, !ok);
}

// ═══════════════════════════════════════════════════════════════════════════
//  PASO 2: Congestion Providers (CTCP)
// ═══════════════════════════════════════════════════════════════════════════

fn apply_congestion_providers() {
    let profiles_output = run_powershell_command(
        "Get-NetTCPSetting | Select-Object -ExpandProperty SettingName -Unique",
    )
    .unwrap_or_default();

    let mut profiles: Vec<String> = profiles_output
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if profiles.is_empty() {
        profiles = vec![
            "Internet".to_string(),
            "Datacenter".to_string(),
            "Compat".to_string(),
            "InternetCustom".to_string(),
            "DatacenterCustom".to_string(),
        ];
        print_status_line(
            "Lectura dinamica de perfiles (usando lista predefinida)",
            false,
            true,
        );
    } else {
        print_status_line(
            &format!("{} perfiles TCP encontrados", profiles.len()),
            true,
            false,
        );
    }

    let mut success_count = 0;
    for profile in &profiles {
        let mut success = false;
        let mut provider_used = "Default";

        // Intentar CUBIC primero
        let cubic_cmds = vec![
            format!(
                "Set-NetTCPSetting -SettingName '{}' -CongestionProvider CUBIC -ErrorAction Stop",
                profile
            ),
            format!(
                "netsh int tcp set supplemental template={} congestionprovider=cubic",
                profile
            ),
        ];
        for cmd in &cubic_cmds {
            if run_powershell_command(cmd).is_ok() {
                success = true;
                provider_used = "CUBIC";
                break;
            }
        }

        // Si falla CUBIC, intentar CTCP como fallback
        if !success {
            let ctcp_cmds = vec![
                format!(
                    "Set-NetTCPSetting -SettingName '{}' -CongestionProvider CTCP -ErrorAction Stop",
                    profile
                ),
                format!(
                    "netsh int tcp set supplemental template={} congestionprovider=ctcp",
                    profile
                ),
            ];
            for cmd in &ctcp_cmds {
                if run_powershell_command(cmd).is_ok() {
                    success = true;
                    provider_used = "CTCP";
                    break;
                }
            }
        }

        let desc = format!("CongestionProvider -> {} [{}]", provider_used, profile);
        print_status_line(&desc, success, false);
        if success {
            success_count += 1;
        }
    }

    set_color_green();
    println!(
        "  >> Resumen: {}/{} perfiles actualizados (CUBIC/CTCP)",
        success_count,
        profiles.len()
    );
    reset_color();
}

// ═══════════════════════════════════════════════════════════════════════════
//  PASO 3: TCP Global (ampliado)
// ═══════════════════════════════════════════════════════════════════════════

fn apply_tcp_global_optimizations() {
    let tcp_cmds: Vec<(&str, &str)> = vec![
        (
            "Desactivando ECN (Explicit Congestion Notification)...",
            "netsh int tcp set global ecncapability=disabled",
        ),
        (
            "Desactivando Heuristica TCP...",
            "netsh int tcp set heuristics disabled",
        ),
        (
            "Ajustando AutoTuningLevel (Normal)...",
            "netsh int tcp set global autotuninglevel=normal",
        ),
        (
            "Activando TCP Timestamps (mejor estimacion RTT)...",
            "netsh int tcp set global timestamps=enabled",
        ),
        (
            "Desactivando Chimney Offload (obsoleto)...",
            "netsh int tcp set global chimney=disabled",
        ),
        (
            "Desactivando Direct Cache Access...",
            "netsh int tcp set global dca=disabled",
        ),
        (
            "Ajustando Initial RTO a 2000ms...",
            "netsh int tcp set global initialRto=2000",
        ),
        (
            "Activando RSS global...",
            "netsh int tcp set global rss=enabled",
        ),
        (
            "Ajustando Max SYN Retransmissions a 8...",
            "netsh int tcp set global maxsynretransmissions=8",
        ),
        (
            "Desactivando resiliencia Non-SACK RST...",
            "netsh int tcp set global nonsackrttresiliency=disabled",
        ),
        (
            "Desactivando Receive Segment Coalescing (RSC) global...",
            "netsh int tcp set global rsc=disabled",
        ),
        (
            "Activando Caching de conexiones (TCPCache)...",
            "netsh int tcp set global tcpcachingmode=enabled",
        ),
    ];

    for (desc, cmd) in &tcp_cmds {
        let ok = run_powershell_command(cmd).is_ok();
        print_status_line(desc, ok, !ok);
    }

    // minrwnd no disponible en todas las versiones de Windows
    let minrwnd_ok = run_powershell_command("netsh int tcp set global minrwnd=65535").is_ok();
    print_status_line(
        "Ajustando Min RWIN a 65535...",
        minrwnd_ok,
        !minrwnd_ok,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
//  PASO 4: Offloads de adaptadores
// ═══════════════════════════════════════════════════════════════════════════

fn apply_adapter_offload_tweaks() {
    let adapters = get_active_adapters();

    if adapters.is_empty() {
        print_status_line("No se encontraron adaptadores activos", false, true);
        return;
    }

    print_status_line(
        &format!("{} adaptadores activos detectados", adapters.len()),
        true,
        false,
    );

    for adapter in &adapters {
        // RSC (Receive Segment Coalescing) → OFF
        let rsc_ok = run_powershell_command(&format!(
            "Disable-NetAdapterRsc -Name '{}'",
            adapter
        ))
        .is_ok();
        print_status_line(
            &format!("RSC deshabilitado [{}]", adapter),
            rsc_ok,
            !rsc_ok,
        );

        // LSO (Large Send Offload) → OFF
        let lso_ok = run_powershell_command(&format!(
            "Disable-NetAdapterLso -Name '{}'",
            adapter
        ))
        .is_ok();
        print_status_line(
            &format!("LSO deshabilitado [{}]", adapter),
            lso_ok,
            !lso_ok,
        );

        // LROv2 IPv4/IPv6 → OFF
        let lro_cmd = format!(
            "Set-NetAdapterAdvancedProperty -Name '{}' -RegistryKeyword '*LsoV2IPv4' -RegistryValue 0 -ErrorAction SilentlyContinue; \
             Set-NetAdapterAdvancedProperty -Name '{}' -RegistryKeyword '*LsoV2IPv6' -RegistryValue 0 -ErrorAction SilentlyContinue",
            adapter, adapter
        );
        let _ = run_powershell_command(&lro_cmd);
        print_status_line(
            &format!("LROv2 deshabilitado [{}]", adapter),
            true,
            false,
        );

        // RSS (Receive Side Scaling) → ON
        let rss_ok = run_powershell_command(&format!(
            "Enable-NetAdapterRss -Name '{}'",
            adapter
        ))
        .is_ok();
        print_status_line(
            &format!("RSS habilitado [{}]", adapter),
            rss_ok,
            !rss_ok,
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  PASO 5: Propiedades avanzadas de NIC (NUEVO)
// ═══════════════════════════════════════════════════════════════════════════

fn apply_nic_advanced_tweaks() {
    let adapters = get_active_adapters();

    if adapters.is_empty() {
        print_status_line("No se encontraron adaptadores para ajustar", false, true);
        return;
    }

    for adapter in &adapters {
        println!("  ── Adaptador: {} ──", adapter);

        // ── Receive Buffers → MAX ───────────────────────────────────────
        if let Some(max_val) = get_nic_property_max(adapter, "*ReceiveBuffers") {
            let ok = set_nic_property(adapter, "*ReceiveBuffers", max_val);
            print_status_line(
                &format!("Receive Buffers = {} [{}]", max_val, adapter),
                ok,
                !ok,
            );
        } else {
            print_status_line(
                &format!("Receive Buffers [{}] (no expuesto)", adapter),
                false,
                true,
            );
        }

        // ── Transmit Buffers → MAX ──────────────────────────────────────
        if let Some(max_val) = get_nic_property_max(adapter, "*TransmitBuffers") {
            let ok = set_nic_property(adapter, "*TransmitBuffers", max_val);
            print_status_line(
                &format!("Transmit Buffers = {} [{}]", max_val, adapter),
                ok,
                !ok,
            );
        } else {
            print_status_line(
                &format!("Transmit Buffers [{}] (no expuesto)", adapter),
                false,
                true,
            );
        }

        // ── Interrupt Moderation → OFF ──────────────────────────────────
        //    Desactivar la moderacion de interrupciones reduce latencia
        //    al precio de un ligero aumento de uso de CPU.
        let irq_ok = set_nic_property(adapter, "*InterruptModeration", 0);
        print_status_line(
            &format!("Interrupt Moderation OFF [{}]", adapter),
            irq_ok,
            !irq_ok,
        );

        // ── Flow Control → Disabled ─────────────────────────────────────
        //    Flow control (802.3x) puede causar pausas que se acumulan
        //    y generan micro-perdidas. Desactivar en redes locales sanas.
        let flow_ok = set_nic_property(adapter, "*FlowControl", 0);
        print_status_line(
            &format!("Flow Control deshabilitado [{}]", adapter),
            flow_ok,
            !flow_ok,
        );

        // ── Energy Efficient Ethernet (EEE) → OFF ──────────────────────
        //    EEE introduce micro-suspensiones del enlace que pueden
        //    causar perdida puntual de paquetes.
        let eee_ok = set_nic_property_multi(
            adapter,
            &["*EEE", "*EnableGreenEthernet", "*AdvancedEEE"],
            0,
        );
        print_status_line(
            &format!("EEE / Green Ethernet OFF [{}]", adapter),
            eee_ok,
            !eee_ok,
        );

        // ── RSS Queues → DYNAMIC TAILORED TO CPU CORES ────────────────────────────────────────────
        //    Mas colas RSS distribuyen mejor el procesamiento entre nucleos, pero demasiadas colas
        //    pueden causar cache bouncing. El número óptimo es min(Cores / 2, MaxNICQueues), clampado entre 2 y 8.
        if let Some(max_queues) = get_nic_property_max(adapter, "*NumRssQueues") {
            let cores = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4);
            let mut optimal_queues = (cores / 2) as u32;
            if optimal_queues < 2 { optimal_queues = 2; }
            if optimal_queues > 8 { optimal_queues = 8; }
            if optimal_queues > max_queues { optimal_queues = max_queues; }

            let ok = set_nic_property(adapter, "*NumRssQueues", optimal_queues);
            print_status_line(
                &format!("Colas RSS = {} (Clase {} cores) [{}]", optimal_queues, cores, adapter),
                ok,
                !ok,
            );
        } else {
            print_status_line(
                &format!("RSS Queues [{}] (no expuesto)", adapter),
                false,
                true,
            );
        }

        // ── Wake on Magic Packet → OFF ──────────────────────────────────
        let wake_mp_ok = run_powershell_command(&format!(
            "Set-NetAdapterPowerManagement -Name '{}' -WakeOnMagicPacket Disabled -ErrorAction Stop",
            adapter
        ))
        .is_ok();
        print_status_line(
            &format!("Wake on Magic Packet OFF [{}]", adapter),
            wake_mp_ok,
            !wake_mp_ok,
        );

        // ── Wake on Pattern → OFF ───────────────────────────────────────
        let wake_pat_ok = run_powershell_command(&format!(
            "Set-NetAdapterPowerManagement -Name '{}' -WakeOnPattern Disabled -ErrorAction Stop",
            adapter
        ))
        .is_ok();
        print_status_line(
            &format!("Wake on Pattern OFF [{}]", adapter),
            wake_pat_ok,
            !wake_pat_ok,
        );

        // ── ARP Offload → OFF ───────────────────────────────────────────
        let arp_ok = set_nic_property(adapter, "*PMARPOffload", 0);
        print_status_line(
            &format!("ARP Offload OFF [{}]", adapter),
            arp_ok,
            !arp_ok,
        );

        // ── NS Offload → OFF ────────────────────────────────────────────
        let ns_ok = set_nic_property(adapter, "*PMNSOffload", 0);
        print_status_line(
            &format!("NS Offload OFF [{}]", adapter),
            ns_ok,
            !ns_ok,
        );

        // ── Prevenir suspensión de energía de red ───────────────────────
        let pwr_ok = run_powershell_command(&format!(
            "Set-NetAdapterPowerManagement -Name '{}' -AllowComputerToTurnOffDevice Disabled -ErrorAction SilentlyContinue",
            adapter
        )).is_ok();
        print_status_line(
            &format!("Ahorro de energía deshabilitado [{}]", adapter),
            pwr_ok,
            !pwr_ok,
        );

        // ── Disable Nagle per-interface (TcpAckFrequency=1, TCPNoDelay=1)
        //    Nagle agrupa paquetes pequenos; desactivarlo reduce latencia
        //    y evita retransmisiones innecesarias en trafico interactivo.
        let nagle_cmd = format!(
            r#"try {{
                $guid = (Get-NetAdapter -Name '{}' | Select-Object -ExpandProperty InterfaceGuid)
                $path = "HKLM:\SYSTEM\CurrentControlSet\Services\Tcpip\Parameters\Interfaces\$guid"
                if (Test-Path $path) {{
                    New-ItemProperty -Path $path -Name 'TcpAckFrequency' -Value 1 -PropertyType DWord -Force | Out-Null
                    New-ItemProperty -Path $path -Name 'TCPNoDelay' -Value 1 -PropertyType DWord -Force | Out-Null
                    New-ItemProperty -Path $path -Name 'TcpDelAckTicks' -Value 0 -PropertyType DWord -Force | Out-Null
                    Write-Output 'OK'
                }} else {{ Write-Output 'SKIP' }}
            }} catch {{ Write-Output 'ERR' }}"#,
            adapter
        );
        let nagle_result = run_powershell_command(&nagle_cmd).unwrap_or_default();
        let nagle_ok = nagle_result.contains("OK");
        print_status_line(
            &format!("Nagle OFF (TcpAckFrequency=1) [{}]", adapter),
            nagle_ok,
            !nagle_ok,
        );

        println!();
    }

    // ── Wi-Fi Background Scanning Latency Spike Optimization ────────────
    let wifi_tweak = run_powershell_command(
        r#"try {
            $wlanPath = "HKLM:\SYSTEM\CurrentControlSet\Services\WlanSvc\Parameters\Interfaces"
            if (Test-Path $wlanPath) {
                Get-ChildItem $wlanPath | ForEach-Object {
                    New-ItemProperty -Path $_.PSPath -Name "ScanOnlyWhenAssociated" -Value 1 -PropertyType DWord -Force | Out-Null
                }
                Write-Output "OK"
            } else { Write-Output "SKIP" }
        } catch { Write-Output "ERR" }"#
    ).unwrap_or_default();
    print_status_line(
        "Latencia Wi-Fi optimizada (ScanOnlyWhenAssociated = 1)",
        wifi_tweak.contains("OK"),
        wifi_tweak.contains("ERR")
    );

    // ── Nagle global (parametros TCP del registro) ──────────────────────
    let global_nagle = run_powershell_command(
        r#"New-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Services\Tcpip\Parameters' -Name 'TcpAckFrequency' -Value 1 -PropertyType DWord -Force -ErrorAction SilentlyContinue | Out-Null;
           New-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Services\Tcpip\Parameters' -Name 'TCPNoDelay' -Value 1 -PropertyType DWord -Force -ErrorAction SilentlyContinue | Out-Null;
           New-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Services\Tcpip\Parameters' -Name 'TcpDelAckTicks' -Value 0 -PropertyType DWord -Force -ErrorAction SilentlyContinue | Out-Null;
           Write-Output 'OK'"#,
    )
    .unwrap_or_default();
    print_status_line(
        "Nagle global deshabilitado (registro TCP)",
        global_nagle.contains("OK"),
        false,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
//  PASO 6: Firewall (bloqueo de telemetria)
// ═══════════════════════════════════════════════════════════════════════════

fn apply_firewall_rules() {
    let fw_cmds: Vec<(&str, &str, &str)> = vec![
        (
            "Block DiagTrack",
            "Bloqueando DiagTrack (Outbound)...",
            "netsh advfirewall firewall add rule name=\"Block DiagTrack\" dir=out action=block service=DiagTrack",
        ),
        (
            "Block WerSvc",
            "Bloqueando WerSvc (Outbound)...",
            "netsh advfirewall firewall add rule name=\"Block WerSvc\" dir=out action=block service=WerSvc",
        ),
        (
            "Block dmwappushservice",
            "Bloqueando dmwappushservice (Outbound)...",
            "netsh advfirewall firewall add rule name=\"Block dmwappushservice\" dir=out action=block service=dmwappushservice",
        ),
    ];

    for (name, desc, add_cmd) in &fw_cmds {
        // Primero eliminamos cualquier regla previa con este nombre para evitar duplicados.
        // Ignoramos el error en caso de que la regla no exista previamente en el sistema.
        let del_cmd = format!("netsh advfirewall firewall delete rule name=\"{}\"", name);
        let _ = run_powershell_command(&del_cmd);

        // Ahora agregamos la nueva regla.
        let ok = run_powershell_command(add_cmd).is_ok();
        print_status_line(desc, ok, false);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  PASO 7: Hardware (MSI, BCD, CPU, DWM, MMCSS, IRQ)
// ═══════════════════════════════════════════════════════════════════════════

fn apply_dynamic_hardware_tweaks() {
    let temp_dir = env::temp_dir();
    let script_path = temp_dir.join(format!("rsopt_hw_{}.ps1", std::process::id()));
    let log_path = temp_dir.join(format!("rsopt_hw_{}.log", std::process::id()));
    let log_path_ps = log_path.to_string_lossy().replace('\\', "\\\\");

    let ps_script = format!(
        r#"
$ErrorActionPreference = 'SilentlyContinue'
$ProgressPreference = 'SilentlyContinue'
$LogPath = "{log_path}"

function Add-Log {{
    param([string]$Kind, [string]$Category, [string]$Message, [string]$Status = 'OK')
    $line = "$Kind|$Category|$Status|$Message"
    Add-Content -LiteralPath $LogPath -Value $line -Encoding UTF8
}}

function Safe-Name {{
    param($Value)
    if ($null -eq $Value) {{ return '' }}
    return ([string]$Value).Replace("`r", " ").Replace("`n", " ").Trim()
}}

Remove-Item -LiteralPath $LogPath -Force -ErrorAction SilentlyContinue
New-Item -ItemType File -Path $LogPath -Force | Out-Null

# ── BCD Optimizations ──────────────────────────────────────────────────────
try {{
    bcdedit /set disabledynamictick yes | Out-Null
    Add-Log 'INFO' 'BCD' 'disabledynamictick = yes'
}} catch {{
    Add-Log 'WARN' 'BCD' 'No se pudo aplicar disabledynamictick' 'WARN'
}}

try {{
    bcdedit /set useplatformtick yes | Out-Null
    Add-Log 'INFO' 'BCD' 'useplatformtick = yes'
}} catch {{
    Add-Log 'WARN' 'BCD' 'No se pudo aplicar useplatformtick' 'WARN'
}}

try {{
    bcdedit /set useplatformclock no | Out-Null
    Add-Log 'INFO' 'BCD' 'useplatformclock = no (Reloj TSC forzado)'
}} catch {{
    Add-Log 'WARN' 'BCD' 'No se pudo aplicar useplatformclock' 'WARN'
}}

try {{
    # ── Desactivar Paginación de 5 Niveles (Address57 translation overhead bypass)
    bcdedit /set linearaddress57 OptOut | Out-Null
    Add-Log 'INFO' 'BCD' 'linearaddress57 = OptOut (Paginacion de 5 niveles desactivada)'
}} catch {{
    Add-Log 'WARN' 'BCD' 'No se pudo aplicar linearaddress57' 'WARN'
}}

try {{
    # ── Desactivar inicio de Virtual Secure Mode (VBS/Core Isolation boot bypass)
    bcdedit /set vsmlaunchtype Off | Out-Null
    Add-Log 'INFO' 'BCD' 'vsmlaunchtype = Off (VBS/Core Isolation desactivado al inicio)'
}} catch {{
    Add-Log 'WARN' 'BCD' 'No se pudo desactivar vsmlaunchtype' 'WARN'
}}

try {{
    bcdedit /set nx OptIn | Out-Null
    Add-Log 'INFO' 'BCD' 'DEP = OptIn'
}} catch {{
    Add-Log 'WARN' 'BCD' 'No se pudo ajustar DEP' 'WARN'
}}

try {{
    bcdedit /set x2apicpolicy Enable | Out-Null
    Add-Log 'INFO' 'BCD' 'x2apicpolicy = Enable (Extended APIC habilitado)'
}} catch {{
    Add-Log 'WARN' 'BCD' 'No se pudo aplicar x2apicpolicy' 'WARN'
}}

try {{
    bcdedit /set tscsyncpolicy Enhanced | Out-Null
    Add-Log 'INFO' 'BCD' 'tscsyncpolicy = Enhanced (Sincronizacion TSC forzada)'
}} catch {{
    Add-Log 'WARN' 'BCD' 'No se pudo aplicar tscsyncpolicy' 'WARN'
}}

try {{
    bcdedit /set usephysicaldestination yes | Out-Null
    Add-Log 'INFO' 'BCD' 'usephysicaldestination = yes (APIC direccionamiento fisico)'
}} catch {{
    Add-Log 'WARN' 'BCD' 'No se pudo aplicar usephysicaldestination' 'WARN'
}}

try {{
    bcdedit /set firstmegabytepolicy UseAll | Out-Null
    Add-Log 'INFO' 'BCD' 'firstmegabytepolicy = UseAll (Primer megabyte de RAM activo)'
}} catch {{
    Add-Log 'WARN' 'BCD' 'No se pudo aplicar firstmegabytepolicy' 'WARN'
}}

# ── Windows Memory Management & Compression Tuning ─────────────────────────
try {{
    # ── Desactivar compresión de memoria (MMAgent Memory Compression Off)
    Disable-MMAgent -MemoryCompression | Out-Null
    Add-Log 'INFO' 'KERNEL' 'MemoryCompression = OFF (Compresion de RAM desactivada)'
}} catch {{
    Add-Log 'WARN' 'KERNEL' 'No se pudo desactivar la compresion de memoria' 'WARN'
}}

try {{
    # ── Desactivar VBS e Integridad de Memoria (HVCI) en el Registro
    $vbsPath = "HKLM:\System\CurrentControlSet\Control\DeviceGuard"
    if (!(Test-Path $vbsPath)) {{ New-Item -Path $vbsPath -Force | Out-Null }}
    New-ItemProperty -Path $vbsPath -Name "EnableVirtualizationBasedSecurity" -Value 0 -PropertyType DWord -Force | Out-Null
    
    $hvciPath = "HKLM:\System\CurrentControlSet\Control\DeviceGuard\Scenarios\HypervisorEnforcedCodeIntegrity"
    if (!(Test-Path $hvciPath)) {{ New-Item -Path $hvciPath -Force | Out-Null }}
    New-ItemProperty -Path $hvciPath -Name "Enabled" -Value 0 -PropertyType DWord -Force | Out-Null
    Add-Log 'INFO' 'KERNEL' 'HVCI / VBS deshabilitado en Registro'
}} catch {{
    Add-Log 'WARN' 'KERNEL' 'No se pudo deshabilitar VBS/HVCI en el Registro' 'WARN'
}}

try {{
    # ── Forzar Kernel y Controladores (Drivers) a RAM física (DisablePagingExecutive)
    $mmPath = "HKLM:\System\CurrentControlSet\Control\Session Manager\Memory Management"
    New-ItemProperty -Path $mmPath -Name "DisablePagingExecutive" -Value 1 -PropertyType DWord -Force | Out-Null
    Add-Log 'INFO' 'KERNEL' 'DisablePagingExecutive = 1 (Kernel y Controladores en RAM)'
}} catch {{
    Add-Log 'WARN' 'KERNEL' 'No se pudo aplicar DisablePagingExecutive' 'WARN'
}}

# ── Pagefile Optimization ──────────────────────────────────────────────────
try {{
    $ramBytes = (Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory
    $ramGB = [Math]::Round($ramBytes / 1GB)
    
    if ($ramGB -ge 32) {{ $size = 4096 }}
    elseif ($ramGB -ge 16) {{ $size = 8192 }}
    else {{ $size = 12288 }}
    
    $cs = Get-CimInstance Win32_ComputerSystem
    if ($cs.AutomaticManagedPagefile) {{
        $cs.AutomaticManagedPagefile = $false
        Set-CimInstance -CimInstance $cs -ErrorAction Stop | Out-Null
    }}
    
    $pf = Get-CimInstance Win32_PageFileSetting
    if ($pf) {{
        $pf.InitialSize = $size
        $pf.MaximumSize = $size
        Set-CimInstance -CimInstance $pf -ErrorAction Stop | Out-Null
    }} else {{
        New-CimInstance -ClassName Win32_PageFileSetting -Property @{{ Name = 'C:\pagefile.sys'; InitialSize = $size; MaximumSize = $size }} -ErrorAction Stop | Out-Null
    }}
    Add-Log 'INFO' 'PAGING' ("Pagefile estatico optimizado a " + $size + " MB (RAM: " + $ramGB + " GB)")
}} catch {{
    Add-Log 'WARN' 'PAGING' 'No se pudo configurar el archivo de paginacion estatico' 'WARN'
}}

# ── NTFS File System Optimization ──────────────────────────────────────────
try {{
    fsutil behavior set disablelastaccess 1 | Out-Null
    fsutil behavior set disable8dot3 1 | Out-Null
    fsutil behavior set memoryusage 2 | Out-Null
    
    $ramBytes = (Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory
    $ramGB = [Math]::Round($ramBytes / 1GB)
    if ($ramGB -ge 32) {{ $mftZoneVal = 4 }}
    elseif ($ramGB -ge 16) {{ $mftZoneVal = 3 }}
    else {{ $mftZoneVal = 2 }}
    
    fsutil behavior set mftzone $mftZoneVal | Out-Null
    Add-Log 'INFO' 'NTFS' ("NTFS optimizado: LastAccess=OFF, 8dot3=OFF, MemoryUsage=2, MftZone=" + $mftZoneVal)
}} catch {{
    Add-Log 'WARN' 'NTFS' 'No se pudieron aplicar todas las optimizaciones NTFS' 'WARN'
}}

# ── CPU Kernel Scheduling (Win32PrioritySeparation = 38) ───────────────────
try {{
    Set-ItemProperty -Path "HKLM:\System\CurrentControlSet\Control\PriorityControl" -Name "Win32PrioritySeparation" -Value 38 | Out-Null
    Add-Log 'INFO' 'KERNEL' 'Win32PrioritySeparation optimizado a 38 (0x26 - Foreground boost)'
}} catch {{
    Add-Log 'WARN' 'KERNEL' 'No se pudo aplicar Win32PrioritySeparation' 'WARN'
}}

# ── Speculative Execution Control (Spectre/Meltdown mitigations bypass) ────
try {{
    $path = "HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management"
    New-ItemProperty -Path $path -Name "FeatureSettingsOverride" -Value 3 -PropertyType DWord -Force | Out-Null
    New-ItemProperty -Path $path -Name "FeatureSettingsOverrideMask" -Value 3 -PropertyType DWord -Force | Out-Null
    Add-Log 'INFO' 'KERNEL' 'Mitigaciones de CPU (Spectre/Meltdown) desactivadas para rendimiento'
}} catch {{
    Add-Log 'WARN' 'KERNEL' 'No se pudieron desactivar las mitigaciones de CPU' 'WARN'
}}

# ── GPU Hardware-Accelerated Scheduling (HAGS) & TDR Delay ──────────────────
try {{
    $path = "HKLM:\System\CurrentControlSet\Control\GraphicsDrivers"
    New-ItemProperty -Path $path -Name "HwSchMode" -Value 2 -PropertyType DWord -Force | Out-Null
    New-ItemProperty -Path $path -Name "TdrDelay" -Value 10 -PropertyType DWord -Force | Out-Null
    Add-Log 'INFO' 'KERNEL' 'GPU Tuning: HAGS forzado y TdrDelay fijados'
}} catch {{
    Add-Log 'WARN' 'KERNEL' 'No se pudieron aplicar ajustes avanzados de GPU' 'WARN'
}}


# ── MSI Mode for Devices ───────────────────────────────────────────────────
function Enable-MsiForDevice {{
    param(
        [string]$PnpId,
        [string]$Name,
        [string]$Type,
        [bool]$IsGpu
    )

    $PnpId = Safe-Name $PnpId
    $Name = Safe-Name $Name
    if ([string]::IsNullOrWhiteSpace($PnpId) -or [string]::IsNullOrWhiteSpace($Name)) {{ return }}

    $base = 'HKLM:\SYSTEM\CurrentControlSet\Enum\' + $PnpId + '\Device Parameters\Interrupt Management'
    $msiPath = $base + '\MessageSignaledInterruptProperties'

    try {{
        if (!(Test-Path $msiPath)) {{ New-Item -Path $msiPath -Force | Out-Null }}
        New-ItemProperty -Path $msiPath -Name 'MSISupported' -Value 1 -PropertyType DWord -Force | Out-Null
        Add-Log 'HW' $Type ('MSI habilitado -> ' + $Name)
    }} catch {{
        Add-Log 'WARN' $Type ('No se pudo habilitar MSI -> ' + $Name) 'WARN'
    }}

    if ($IsGpu) {{
        $affPath = $base + '\Affinity Policy'
        try {{
            if (!(Test-Path $affPath)) {{ New-Item -Path $affPath -Force | Out-Null }}
            New-ItemProperty -Path $affPath -Name 'DevicePriority' -Value 3 -PropertyType DWord -Force | Out-Null
            New-ItemProperty -Path $affPath -Name 'DevicePolicy' -Value 3 -PropertyType DWord -Force | Out-Null

            $cores = @(Get-CimInstance Win32_Processor | Select-Object -ExpandProperty NumberOfLogicalProcessors | Where-Object {{ $_ -gt 0 }} | Measure-Object -Sum).Sum
            if ($cores -gt 1) {{
                $mask = [uint64](([math]::Pow(2, $cores)) - 2)
                $bytes = [BitConverter]::GetBytes($mask)
                New-ItemProperty -Path $affPath -Name 'AssignmentSetOverride' -Value $bytes -PropertyType Binary -Force | Out-Null
                Add-Log 'HW' $Type ('Afinidad IRQ aplicada (mask=0x' + $mask.ToString('X') + ') -> ' + $Name)
            }} else {{
                Add-Log 'WARN' $Type ('No se aplico afinidad IRQ por nucleos insuficientes -> ' + $Name) 'WARN'
            }}
        }} catch {{
            Add-Log 'WARN' $Type ('No se pudo aplicar prioridad/afinidad GPU -> ' + $Name) 'WARN'
        }}
    }}
}}

# ── GPUs ────────────────────────────────────────────────────────────────────
try {{
    $gpus = Get-CimInstance Win32_VideoController | Select-Object PNPDeviceID, Name
    foreach ($gpu in $gpus) {{
        Enable-MsiForDevice -PnpId $gpu.PNPDeviceID -Name $gpu.Name -Type 'GPU' -IsGpu $true
    }}
}} catch {{
    Add-Log 'WARN' 'GPU' 'No se pudieron enumerar GPUs' 'WARN'
}}

# ── Storage Controllers ────────────────────────────────────────────────────
try {{
    $storageControllers = Get-PnpDevice -PresentOnly | Where-Object {{
        $_.Class -in @('SCSIAdapter','HDC','IDE','Storage')
    }} | Select-Object InstanceId, FriendlyName, Class

    foreach ($ctrl in $storageControllers) {{
        $nm = if ([string]::IsNullOrWhiteSpace($ctrl.FriendlyName)) {{ $ctrl.InstanceId }} else {{ $ctrl.FriendlyName }}
        Enable-MsiForDevice -PnpId $ctrl.InstanceId -Name $nm -Type 'Storage' -IsGpu $false
    }}
}} catch {{
    Add-Log 'WARN' 'Storage' 'No se pudieron enumerar controladores de almacenamiento' 'WARN'
}}

# ── NICs Fisicas ───────────────────────────────────────────────────────────
try {{
    $nics = Get-NetAdapter -Physical -ErrorAction SilentlyContinue | Where-Object {{ $_.InterfaceDescription -notmatch 'VirtualBox|VMware|Virtual|Hyper-V|vEthernet|Loopback|TAP' -and $_.Name -notmatch 'VirtualBox|VMware|Virtual|Hyper-V|vEthernet|Loopback|TAP' }} | Select-Object Name, InterfaceDescription, PnPDeviceID
    foreach ($nic in $nics) {{
        $nm = if ([string]::IsNullOrWhiteSpace($nic.InterfaceDescription)) {{ $nic.Name }} else {{ $nic.InterfaceDescription }}
        Enable-MsiForDevice -PnpId $nic.PnPDeviceID -Name $nm -Type 'NIC' -IsGpu $false
    }}
}} catch {{
    Add-Log 'WARN' 'NIC' 'No se pudieron enumerar NICs fisicas' 'WARN'
}}

# ── USB Controllers ────────────────────────────────────────────────────────
try {{
    $usbControllers = Get-PnpDevice -PresentOnly | Where-Object {{
        $_.Class -eq 'USB'
    }} | Select-Object InstanceId, FriendlyName

    foreach ($usb in $usbControllers) {{
        $nm = if ([string]::IsNullOrWhiteSpace($usb.FriendlyName)) {{ $usb.InstanceId }} else {{ $usb.FriendlyName }}
        Enable-MsiForDevice -PnpId $usb.InstanceId -Name $nm -Type 'USB' -IsGpu $false
    }}
}} catch {{
    Add-Log 'WARN' 'USB' 'No se pudieron enumerar controladores USB' 'WARN'
}}

# ── DWM MMCSS ──────────────────────────────────────────────────────────────
try {{
    Add-Type -TypeDefinition @'
using System.Runtime.InteropServices;
public static class DwmBridge {{
    [DllImport("dwmapi.dll", PreserveSig=false)]
    public static extern void DwmEnableMMCSS(bool enable);
}}
'@
    [DwmBridge]::DwmEnableMMCSS($true)
    Add-Log 'INFO' 'DWM' 'DwmEnableMMCSS(true) ejecutado'
}} catch {{
    Add-Log 'WARN' 'DWM' 'No se pudo invocar DwmEnableMMCSS' 'WARN'
}}

# ── MMCSS Network Throttling ───────────────────────────────────────────────
try {{
    $mmcssPath = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile'
    if (Test-Path $mmcssPath) {{
        New-ItemProperty -Path $mmcssPath -Name 'NetworkThrottlingIndex' -Value 0xFFFFFFFF -PropertyType DWord -Force | Out-Null
        New-ItemProperty -Path $mmcssPath -Name 'SystemResponsiveness' -Value 0 -PropertyType DWord -Force | Out-Null
        Add-Log 'INFO' 'MMCSS' 'NetworkThrottlingIndex=FFFFFFFF, SystemResponsiveness=0'
    }}
}} catch {{
    Add-Log 'WARN' 'MMCSS' 'No se pudo ajustar MMCSS' 'WARN'
}}

# ── GPU Priority via MMCSS Tasks ───────────────────────────────────────────
try {{
    $gpuPath = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile\Tasks\Games'
    if (!(Test-Path $gpuPath)) {{ New-Item -Path $gpuPath -Force | Out-Null }}
    New-ItemProperty -Path $gpuPath -Name 'GPU Priority' -Value 8 -PropertyType DWord -Force | Out-Null
    New-ItemProperty -Path $gpuPath -Name 'Priority' -Value 6 -PropertyType DWord -Force | Out-Null
    New-ItemProperty -Path $gpuPath -Name 'Scheduling Category' -Value 'High' -PropertyType String -Force | Out-Null
    New-ItemProperty -Path $gpuPath -Name 'SFIO Priority' -Value 'High' -PropertyType String -Force | Out-Null
    Add-Log 'INFO' 'MMCSS' 'GPU Priority y Games Task optimizados'
}} catch {{
    Add-Log 'WARN' 'MMCSS' 'No se pudo ajustar GPU Priority' 'WARN'
}}

# ── Global Timer Resolution & Power Throttling Windows 10/11 ──────────────
try {{
    $kernelPath = 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\kernel'
    if (!(Test-Path $kernelPath)) {{ New-Item -Path $kernelPath -Force | Out-Null }}
    New-ItemProperty -Path $kernelPath -Name 'GlobalTimerResolutionRequests' -Value 1 -PropertyType DWord -Force | Out-Null
    Add-Log 'INFO' 'BCD' 'GlobalTimerResolutionRequests = 1'
}} catch {{
    Add-Log 'WARN' 'BCD' 'No se pudo aplicar GlobalTimerResolutionRequests' 'WARN'
}}

try {{
    $powerPath = 'HKLM:\SYSTEM\CurrentControlSet\Control\Power\PowerThrottling'
    if (!(Test-Path $powerPath)) {{ New-Item -Path $powerPath -Force | Out-Null }}
    New-ItemProperty -Path $powerPath -Name 'PowerThrottlingOff' -Value 1 -PropertyType DWord -Force | Out-Null
    Add-Log 'INFO' 'POWER' 'PowerThrottlingOff = 1'
}} catch {{
    Add-Log 'WARN' 'POWER' 'No se pudo desactivar Power Throttling' 'WARN'
}}

try {{
    $powerPath = 'HKLM:\SYSTEM\CurrentControlSet\Control\Power'
    if (!(Test-Path $powerPath)) {{ New-Item -Path $powerPath -Force | Out-Null }}
    New-ItemProperty -Path $powerPath -Name 'DisableInterruptSteering' -Value 1 -PropertyType DWord -Force | Out-Null
    Add-Log 'INFO' 'POWER' 'DisableInterruptSteering = 1 (Interrupt steering desactivado)'
}} catch {{
    Add-Log 'WARN' 'POWER' 'No se pudo aplicar DisableInterruptSteering' 'WARN'
}}
"#,
        log_path = log_path_ps
    );

    if let Err(e) = fs::write(&script_path, ps_script) {
        set_color_red();
        println!("  [ ERROR ] No se pudo crear el script temporal: {}", e);
        reset_color();
        return;
    }

    let output_result = Command::new("powershell")
        .args(&[
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            script_path.to_str().unwrap(),
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    let _ = fs::remove_file(&script_path);

    if output_result.is_err() {
        set_color_red();
        println!("  [ ERROR ] Fallo total al invocar PowerShell.");
        reset_color();
        return;
    }

    let log_content = fs::read_to_string(&log_path).unwrap_or_default();
    let _ = fs::remove_file(&log_path);

    let mut emitted = false;
    for raw in log_content.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(4, '|').collect();
        if parts.len() < 4 {
            continue;
        }
        emitted = true;
        let kind = parts[0];
        let category = parts[1];
        let status = parts[2];
        let message = parts[3];

        let mut msg = message.to_string();
        if msg.len() > 56 {
            msg.truncate(53);
            msg.push_str("...");
        }

        let desc = match kind {
            "HW" => format!("MSI/IRQ -> [{}] {}", category, msg),
            "WARN" => format!("Aviso [{}] -> {}", category, msg),
            _ => format!("Aplicando [{}] -> {}", category, msg),
        };

        let ok = status.eq_ignore_ascii_case("OK");
        let warn = status.eq_ignore_ascii_case("WARN");
        print_status_line(&desc, ok, warn);
    }

    if !emitted {
        set_color_yellow();
        println!("  [ INFO ] Sin salida en el motor de hardware.");
        reset_color();
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  PASO 8a: Registro embebido
// ═══════════════════════════════════════════════════════════════════════════

fn apply_embedded_registry() {
    print!("  Aplicando archivo .reg de base estable... ");
    io::stdout().flush().unwrap();

    let temp_dir = env::temp_dir();
    let reg_path = temp_dir.join(format!("RSOpt_{}.reg", std::process::id()));

    if fs::write(&reg_path, REG_RESOURCE).is_ok() {
        let status = Command::new("regedit.exe")
            .args(&["/s", reg_path.to_str().unwrap()])
            .creation_flags(CREATE_NO_WINDOW)
            .status();

        thread::sleep(Duration::from_millis(500));
        let _ = fs::remove_file(&reg_path);

        if status.is_ok() && status.unwrap().success() {
            set_color_green();
            println!("[ OK ]");
            reset_color();
            return;
        }
    }

    set_color_yellow();
    println!("[ ERROR ] No se pudo aplicar registro base");
    reset_color();
}

// ═══════════════════════════════════════════════════════════════════════════
//  PASO 8b: RAM Optimizer
// ═══════════════════════════════════════════════════════════════════════════

fn install_and_run_ram_optimizer() {
    print!("  Cerrando instancias anteriores... ");
    io::stdout().flush().unwrap();
    
    // Matar procesos con ambos nombres posibles de forma exhaustiva
    let _ = Command::new("taskkill")
        .args(&["/F", "/IM", "RS RAM Optimizer.exe"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    let _ = Command::new("taskkill")
        .args(&["/F", "/IM", "RS_RAM_Optimizer.exe"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    thread::sleep(Duration::from_millis(1500));
    set_color_green();
    println!("[ OK ]");
    reset_color();

    print!("  Limpiando rastros y configuraciones antiguas... ");
    io::stdout().flush().unwrap();

    if let Ok(appdata) = env::var("APPDATA") {
        // 1. Eliminar la carpeta física antigua de RickStyles/RS_Optimizer (con guiones bajos)
        let old_folder = PathBuf::from(&appdata).join("RickStyles").join("RS_Optimizer");
        if old_folder.exists() {
            let _ = fs::remove_dir_all(&old_folder);
        }

        // 2. Limpieza exhaustiva de cualquier clave Run residual en el registro (HKCU y HKLM)
        let registry_names = vec![
            "RS_RAM_Optimizer",
            "RSRAMOptimizer",
            "RSRamOptimizer",
            "RS RAM Optimizer",
            "RickStyles RAM Optimizer"
        ];
        
        for name in &registry_names {
            let _ = run_powershell_command(&format!(
                "Remove-ItemProperty -Path HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run -Name '{}' -ErrorAction SilentlyContinue",
                name
            ));
            let _ = run_powershell_command(&format!(
                "Remove-ItemProperty -Path HKLM:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run -Name '{}' -ErrorAction SilentlyContinue",
                name
            ));
        }

        // 3. Limpieza de tareas programadas residuales que puedan entrar en conflicto
        let task_names = vec![
            "RS_RAM_Optimizer",
            "RSRAMOptimizer",
            "RSRamOptimizer",
            "RS RAM Optimizer",
            "RickStyles RAM Optimizer"
        ];

        for tname in &task_names {
            let _ = run_powershell_command(&format!(
                "Unregister-ScheduledTask -TaskName '{}' -Confirm:$false -ErrorAction SilentlyContinue",
                tname
            ));
            let _ = run_powershell_command(&format!(
                "schtasks /delete /tn \"{}\" /f",
                tname
            ));
        }

        // 4. Limpieza de accesos directos en las carpetas de Inicio (Startup) de Windows
        let startup_clean_cmds = vec![
            "Get-ChildItem \"$env:APPDATA\\Microsoft\\Windows\\Start Menu\\Programs\\Startup\" -Filter \"*optimizer*.lnk\" -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue",
            "Get-ChildItem \"$env:ProgramData\\Microsoft\\Windows\\Start Menu\\Programs\\Startup\" -Filter \"*optimizer*.lnk\" -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue",
            "Get-ChildItem \"$env:APPDATA\\Microsoft\\Windows\\Start Menu\\Programs\\Startup\" -Filter \"*ram*.lnk\" -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue",
            "Get-ChildItem \"$env:ProgramData\\Microsoft\\Windows\\Start Menu\\Programs\\Startup\" -Filter \"*ram*.lnk\" -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue"
        ];
        for scmd in startup_clean_cmds {
            let _ = run_powershell_command(scmd);
        }

        set_color_green();
        println!("[ OK ]");
        reset_color();

        print!("  Extrayendo RAM Optimizer... ");
        io::stdout().flush().unwrap();

        let rs_folder = PathBuf::from(&appdata).join("RickStyles").join("RSOptimizer");
        let _ = fs::create_dir_all(&rs_folder);
        let target_exe = rs_folder.join("RS RAM Optimizer.exe");

        if fs::write(&target_exe, EXE_RESOURCE).is_ok() {
            set_color_green();
            println!("[ OK ]");
            reset_color();

            print!("  Configurando inicio con Windows (Tarea Programada)... ");
            io::stdout().flush().unwrap();

            // Crear un script temporal .ps1 para registrar la tarea programada.
            // Se usa archivo en vez de inline para evitar que el runner elimine
            // los $ de las variables PowerShell.
            // Incluye: -WorkingDirectory y Delay PT10S en el trigger AtLogon
            // para esperar a que Explorer/DWM estén listos antes de iniciar.
            let task_script = rs_folder.join("_register_task.ps1");
            let script_content = format!(
                "$ErrorActionPreference = 'SilentlyContinue'\r\n\
                 Unregister-ScheduledTask -TaskName 'RSRAMOptimizer' -Confirm:$false -ErrorAction SilentlyContinue\r\n\
                 $act = New-ScheduledTaskAction -Execute '{}' -WorkingDirectory '{}'\r\n\
                 $trig = New-ScheduledTaskTrigger -AtLogon\r\n\
                 $trig.Delay = 'PT10S'\r\n\
                 $sett = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -DontStopOnIdleEnd -ExecutionTimeLimit ([System.TimeSpan]::Zero)\r\n\
                 $sett.Priority = 4\r\n\
                 $sett.StartWhenAvailable = $true\r\n\
                 $sett.RestartCount = 5\r\n\
                 $sett.RestartInterval = 'PT1M'\r\n\
                 $prin = New-ScheduledTaskPrincipal -GroupId 'S-1-5-32-544' -RunLevel Highest\r\n\
                 Register-ScheduledTask -TaskName 'RSRAMOptimizer' -Action $act -Trigger $trig -Settings $sett -Principal $prin -Force\r\n\
                 if (-not (Get-ScheduledTask -TaskName 'RSRAMOptimizer' -ErrorAction SilentlyContinue)) {{ exit 1 }}",
                target_exe.display(),
                rs_folder.display()
            );

            let task_ok = if fs::write(&task_script, &script_content).is_ok() {
                let result = Command::new("powershell")
                    .args(&[
                        "-NoProfile",
                        "-NonInteractive",
                        "-ExecutionPolicy",
                        "Bypass",
                        "-File",
                        &task_script.to_string_lossy(),
                    ])
                    .creation_flags(CREATE_NO_WINDOW)
                    .output();
                let _ = fs::remove_file(&task_script); // Limpiar script temporal
                match result {
                    Ok(out) => out.status.success(),
                    Err(_) => false,
                }
            } else {
                false
            };

            if task_ok {
                set_color_green();
                println!("[ OK ]");
                reset_color();
            } else {
                set_color_yellow();
                println!("[ ADVERTENCIA: Se usara schtasks como respaldo ]");
                reset_color();

                let schtasks_cmd = format!(
                    "schtasks /create /tn \"RSRAMOptimizer\" /tr \"'{}'\" /sc onlogon /rl highest /f",
                    target_exe.display()
                );
                let _ = run_powershell_command(&schtasks_cmd);
            }

            print!("  Iniciando optimizador en 2do plano... ");
            io::stdout().flush().unwrap();

            // Iniciar la tarea programada directamente para asegurar privilegios elevados de inmediato
            let start_cmd = "Start-ScheduledTask -TaskName 'RSRAMOptimizer'";
            if run_powershell_command(start_cmd).is_ok() {
                set_color_green();
                println!("[ OK ]");
                reset_color();
            } else {
                // Caída si falla la tarea programada
                if Command::new(&target_exe)
                    .creation_flags(CREATE_NO_WINDOW)
                    .spawn()
                    .is_ok()
                {
                    set_color_green();
                    println!("[ OK ]");
                    reset_color();
                } else {
                    set_color_red();
                    println!("[ ERROR ]");
                    reset_color();
                }
            }
        } else {
            set_color_red();
            println!("[ ERROR ] No se pudo escribir el archivo exe.");
            reset_color();
        }
    }
}

