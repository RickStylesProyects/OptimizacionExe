use std::process::Command;
use std::os::windows::process::CommandExt;
use std::io::{self, Write};
use std::fs;
use std::path::PathBuf;
use std::env;
use std::thread;
use std::time::Duration;

const CREATE_NO_WINDOW: u32 = 0x08000000;

// Embed the regedit file and the executable
const REG_RESOURCE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/embedded.reg"));
const EXE_RESOURCE: &[u8] = include_bytes!("../../RamOptimizer/RS RAM Optimizer.exe");

// Helper to run powershell commands silently
fn run_powershell(command: &str) -> Result<String, String> {
    let output = Command::new("powershell")
        .args(&["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", command])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("PowerShell error: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    
    if !output.status.success() {
        return Err("PowerShell exit code non-zero".to_string());
    }

    Ok(stdout)
}

fn set_color_green() { print!("\x1b[32m"); }
fn set_color_yellow() { print!("\x1b[33m"); }
fn set_color_cyan() { print!("\x1b[36m"); }
fn set_color_dark_gray() { print!("\x1b[90m"); }
fn set_color_red() { print!("\x1b[31m"); }
fn reset_color() { print!("\x1b[0m"); }

#[cfg(windows)]
fn enable_virtual_terminal_processing() {
    type HANDLE = *mut std::ffi::c_void;
    type DWORD = u32;
    const STD_OUTPUT_HANDLE: u32 = 0xFFFFFFF5; // -11i32 as u32
    const ENABLE_VIRTUAL_TERMINAL_PROCESSING: DWORD = 0x0004;

    extern "system" {
        fn GetStdHandle(nStdHandle: u32) -> HANDLE;
        fn GetConsoleMode(hConsoleHandle: HANDLE, lpMode: *mut DWORD) -> i32;
        fn SetConsoleMode(hConsoleHandle: HANDLE, dwMode: DWORD) -> i32;
    }

    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        let mut mode: DWORD = 0;
        if GetConsoleMode(handle, &mut mode) != 0 {
            SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
        }
    }
}

fn main() {
    // Enable ANSI colors for Windows 10/11 properly
    #[cfg(windows)]
    enable_virtual_terminal_processing();


    set_color_cyan();
    println!("+--------------------------------------------------+");
    println!("|                  RS Optimizer                    |");
    println!("|                by RickStyles                     |");
    println!("+--------------------------------------------------+");
    reset_color();

    println!("Optimizando parametros globales de Loopback...");
    if run_powershell("netsh int ipv4 set global loopbacklargemtu=disable; netsh int ipv6 set global loopbacklargemtu=disable").is_ok() {
        set_color_green();
        println!("[ OK ] Parametros de compatibilidad aplicados.");
        reset_color();
    } else {
        set_color_yellow();
        println!("(!) No se pudieron aplicar ajustes de loopback.");
        reset_color();
    }

    println!("\nDetectando perfiles TCP disponibles...");
    let profiles_output = run_powershell("Get-NetTCPSetting | Select-Object -ExpandProperty SettingName -Unique").unwrap_or_default();
    let mut profiles: Vec<String> = profiles_output.lines().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();

    if profiles.is_empty() {
        profiles = vec!["Internet".to_string(), "Datacenter".to_string(), "Compat".to_string(), "InternetCustom".to_string(), "DatacenterCustom".to_string()];
        set_color_yellow();
        println!("(!) No se pudo leer la lista dinamica. Usando lista predefinida.");
        reset_color();
    }

    println!("Se encontraron {} perfiles. Iniciando optimizacion...\n", profiles.len());
    let mut success_count = 0;

    for profile in &profiles {
        print!(" Configurando '{}'...", profile);
        for _ in 0..(40_usize.saturating_sub(18 + profile.len())) { print!(" "); }
        io::stdout().flush().unwrap();

        let msgs = vec![
            format!("netsh int tcp set supplemental template=\"{}\" congestionprovider=bbr2", profile),
            format!("netsh int tcp set supplemental template=\"{}\" congestionprovider=bbr", profile),
            format!("Set-NetTCPSetting -SettingName \"{}\" -CongestionProvider BBR -ErrorAction Stop", profile),
            format!("Set-NetTCPSetting -SettingName \"{}\" -CongestionProvider BBR2 -ErrorAction Stop", profile),
        ];

        let mut success = false;
        for cmd in msgs {
            if run_powershell(&cmd).is_ok() {
                success = true;
                break;
            }
        }

        if success {
            set_color_green();
            println!("[ OK ]");
            reset_color();
            success_count += 1;
        } else {
            set_color_dark_gray();
            println!("[ OMITIDO / PROTEGIDO ]");
            reset_color();
        }
    }

    println!("\n----------------------------------------------------");
    set_color_green();
    println!("Resumen: {} de {} perfiles actualizados a BBR.", success_count, profiles.len());
    reset_color();

    apply_advanced_optimizations();

    println!("\n[Estado Final de la Configuracion]");
    println!("(Muestra solo el perfil 'Internet' como referencia)");

    let final_status = run_powershell("Get-NetTCPSetting -SettingName Internet | Select-Object SettingName, CongestionProvider | Format-Table -AutoSize | Out-String").unwrap_or_default();
    println!("{}", final_status.trim());
    println!("\n----------------------------------------------------");

    println!("Buscando y aplicando configuraciones de registro...");
    apply_embedded_registry();

    println!("\n----------------------------------------------------");
    println!("Instalando e Iniciando RS RAM Optimizer...");
    install_and_run_ram_optimizer();

    reset_color();
    println!("\nPresiona Enter para cerrar...");
    let mut input = String::new();
    let _ = io::stdin().read_line(&mut input);
}

fn apply_advanced_optimizations() {
    println!("\n----------------------------------------------------");
    println!("Optimizando Adaptadores de Red (RSC / LSO) y TCP...");

    if let Ok(adapters_output) = run_powershell("Get-NetAdapter | Where-Object Status -eq 'Up' | Select-Object -ExpandProperty Name") {
        let adapters: Vec<String> = adapters_output.lines().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        for adapter in adapters {
            print!(" Configurando RSC en '{}'...", adapter);
            for _ in 0..(50_usize.saturating_sub(23 + adapter.len())) { print!(" "); }
            io::stdout().flush().unwrap();
            
            if run_powershell(&format!("Disable-NetAdapterRsc -Name \"{}\"", adapter)).is_ok() {
                set_color_green();
                println!("[ OK ]");
                reset_color();
            } else {
                set_color_dark_gray();
                println!("[ OMITIDO / NO SOPORTADO ]");
                reset_color();
            }

            print!(" Configurando LSO en '{}'...", adapter);
            for _ in 0..(50_usize.saturating_sub(23 + adapter.len())) { print!(" "); }
            io::stdout().flush().unwrap();
            
            if run_powershell(&format!("Disable-NetAdapterLso -Name \"{}\"", adapter)).is_ok() {
                set_color_green();
                println!("[ OK ]");
                reset_color();
            } else {
                set_color_dark_gray();
                println!("[ OMITIDO / NO SOPORTADO ]");
                reset_color();
            }
        }
    } else {
        set_color_yellow();
        println!("(!) No se pudo obtener la lista de adaptadores.");
        reset_color();
    }

    println!("\nAplicando Optimizaciones Globales TCP:");

    let tcp_cmds = [
        ("Desactivando ECN (Explicit Congestion)...", "netsh int tcp set global ecncapability=disabled"),
        ("Desactivando Heuristica TCP...", "netsh int tcp set heuristics disabled"),
        ("Ajustando AutoTuningLevel (Normal)...", "netsh int tcp set global autotuninglevel=normal"),
    ];

    for (desc, cmd) in tcp_cmds.iter() {
        print!(" {}", desc);
        for _ in 0..(50_usize.saturating_sub(1 + desc.len())) { print!(" "); }
        io::stdout().flush().unwrap();
        
        if run_powershell(cmd).is_ok() {
            set_color_green();
            println!("[ OK ]");
            reset_color();
        } else {
            set_color_dark_gray();
            println!("[ OMITIDO / PROTEGIDO ]");
            reset_color();
        }
    }
}

fn apply_embedded_registry() {
    print!(" Aplicando archivo incrustado...");
    for _ in 0..(40_usize.saturating_sub(32)) { print!(" "); }
    io::stdout().flush().unwrap();

    let temp_dir = env::temp_dir();
    let reg_path = temp_dir.join(format!("RS_Opt_{}.reg", std::process::id()));

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
    println!("(!) Error aplicando registro incrustado");
    reset_color();
}

fn install_and_run_ram_optimizer() {
    print!(" Cerrando instancias anteriores...");
    for _ in 0..(40_usize.saturating_sub(34)) { print!(" "); }
    io::stdout().flush().unwrap();

    let _ = Command::new("taskkill").args(&["/F", "/IM", "RS RAM Optimizer.exe"]).creation_flags(CREATE_NO_WINDOW).output();
    thread::sleep(Duration::from_millis(1500));
    set_color_green();
    println!("[ OK ]");
    reset_color();

    print!(" Extrayendo RAM Optimizer...");
    for _ in 0..(40_usize.saturating_sub(28)) { print!(" "); }
    io::stdout().flush().unwrap();

    if let Ok(appdata) = env::var("APPDATA") {
        let rs_folder = PathBuf::from(appdata).join("RickStyles").join("RS_Optimizer");
        let _ = fs::create_dir_all(&rs_folder);
        let target_exe = rs_folder.join("RS RAM Optimizer.exe");

        if fs::write(&target_exe, EXE_RESOURCE).is_ok() {
            set_color_green();
            println!("[ OK ]");
            reset_color();

            print!(" Configurando inicio con Windows...");
            for _ in 0..(40_usize.saturating_sub(35)) { print!(" "); }
            io::stdout().flush().unwrap();

            let reg_cmd = format!("Set-ItemProperty -Path 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run' -Name 'RS_RAM_Optimizer' -Value '\"{}\"' -Force", target_exe.display());
            if run_powershell(&reg_cmd).is_ok() {
                set_color_green();
                println!("[ OK ]");
                reset_color();
            } else {
                set_color_yellow();
                println!("[ ADVERTENCIA ]");
                reset_color();
            }

            print!(" Iniciando optimizador en 2do plano...");
            for _ in 0..(40_usize.saturating_sub(38)) { print!(" "); }
            io::stdout().flush().unwrap();

            if Command::new(&target_exe).creation_flags(CREATE_NO_WINDOW).spawn().is_ok() {
                set_color_green();
                println!("[ OK ]");
                reset_color();
            } else {
                set_color_yellow();
                println!("[ ERROR ]");
                reset_color();
            }
        } else {
            set_color_red();
            println!("[ ERROR ] No se pudo escribir el archivo exe.");
            reset_color();
        }
    }
}
