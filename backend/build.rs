fn main() {
    println!("cargo:rerun-if-changed=assets/tray.ico");
    println!("cargo:rerun-if-changed=build.rs");

    #[cfg(target_os = "windows")]
    {
        use std::path::Path;
        use std::process::Command;

        let out_dir = std::env::var("OUT_DIR").unwrap();
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let icon_path = Path::new(&manifest_dir).join("assets").join("tray.ico");

        if icon_path.exists() {
            let rc_content = format!("1 ICON \"{}\"\n", icon_path.to_str().unwrap().replace('\\', "/"));
            let rc_path = Path::new(&out_dir).join("tray.rc");
            let obj_path = Path::new(&out_dir).join("tray.o");

            if std::fs::write(&rc_path, rc_content).is_ok() {
                let status = Command::new("windres")
                    .arg("-i")
                    .arg(&rc_path)
                    .arg("-o")
                    .arg(&obj_path)
                    .status();

                if let Ok(s) = status {
                    if s.success() {
                        println!("cargo:rustc-link-arg={}", obj_path.to_str().unwrap().replace('\\', "/"));
                    }
                }
            }
        }
    }
}
