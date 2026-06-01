use std::env;
use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=../Regedit");

    if std::env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows" {
        let mut res = winres::WindowsResource::new();
        res.set_icon("../Icono.ico");
        res.set_manifest_file("../app.manifest");
        res.set("CompanyName", "MapleProjects");
        res.set("ProductName", "RS Optimizer");
        res.set("FileDescription", "RS Optimizer natively compiled to Rust");
        res.set("LegalCopyright", "Copyright (c) 2026 MapleProjects");
        res.compile().unwrap();
    }

    // Automatically find any .reg file in ../Regedit and copy it to OUT_DIR
    let reg_dir = Path::new("../Regedit");
    let mut reg_file_path = None;
    if let Ok(entries) = fs::read_dir(reg_dir) {
        for entry in entries.flatten() {
            if entry.path().extension().and_then(|e| e.to_str()) == Some("reg") {
                reg_file_path = Some(entry.path());
                break;
            }
        }
    }

    if let Some(path) = reg_file_path {
        let out_dir = env::var_os("OUT_DIR").unwrap();
        let dest_path = Path::new(&out_dir).join("embedded.reg");
        fs::copy(&path, &dest_path).expect("Failed to copy .reg file to OUT_DIR");
    } else {
        panic!("No se encontró ningún archivo .reg en la carpeta Regedit");
    }
}
