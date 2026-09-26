use std::path::Path;
use std::process::Command;

#[cfg(target_os = "windows")]
use winreg::enums::{
    HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE, REG_EXPAND_SZ, REG_SZ,
};
#[cfg(target_os = "windows")]
use winreg::{RegKey, RegValue};

use crate::common::windows::clean_path_str;

const USER_ENV_KEY: &str = r"Environment";
const SYSTEM_ENV_KEY: &str = r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment";

pub fn normalize_path_entry(val: &str) -> String {
    clean_path_str(val)
}

pub fn path_contains_entry(path_value: &str, entry: &str) -> bool {
    let normalized_entry = normalize_path_entry(entry);
    for part in path_value.split(';') {
        if normalize_path_entry(part).eq_ignore_ascii_case(&normalized_entry) {
            return true;
        }
    }
    false
}

pub fn update_path_entries(current: &str, entry: &str, add: bool) -> String {
    let normalized_entry = normalize_path_entry(entry);
    let mut parts: Vec<String> = current
        .split(';')
        .filter_map(|part| {
            let p = part.trim();
            if p.is_empty() {
                return None;
            }
            if !add && normalize_path_entry(p).eq_ignore_ascii_case(&normalized_entry) {
                return None;
            }
            Some(p.to_string())
        })
        .collect();

    if add && !path_contains_entry(current, entry) {
        parts.push(normalized_entry);
    }

    parts.join(";")
}

pub fn query_snip_path_status(install_dir: &Path) -> (bool, bool, Option<String>) {
    let entry = install_dir.to_string_lossy().to_string();
    let mut user_path = false;
    let mut system_path = false;
    let mut messages = Vec::new();

    #[cfg(target_os = "windows")]
    {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        match hkcu.open_subkey_with_flags(USER_ENV_KEY, KEY_READ) {
            Ok(key) => {
                if let Ok(val) = key.get_value::<String, _>("Path") {
                    user_path = path_contains_entry(&val, &entry);
                }
            }
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    messages.push(format!("无法读取用户 PATH: {}", e));
                }
            }
        }

        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        match hklm.open_subkey_with_flags(SYSTEM_ENV_KEY, KEY_READ) {
            Ok(key) => {
                if let Ok(val) = key.get_value::<String, _>("Path") {
                    system_path = path_contains_entry(&val, &entry);
                }
            }
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    messages.push(format!("无法读取系统 PATH: {}", e));
                }
            }
        }
    }

    let err_msg = if messages.is_empty() {
        None
    } else {
        Some(messages.join("；"))
    };

    (user_path, system_path, err_msg)
}

#[cfg(target_os = "windows")]
fn update_registry_path_value(
    hkey: winreg::HKEY,
    subkey: &str,
    entry: &str,
    add: bool,
) -> Result<bool, std::io::Error> {
    let root = RegKey::predef(hkey);
    let key = if add && hkey == HKEY_CURRENT_USER {
        match root.create_subkey_with_flags(subkey, KEY_READ | KEY_WRITE) {
            Ok((k, _)) => k,
            Err(e) => return Err(e),
        }
    } else {
        match root.open_subkey_with_flags(subkey, KEY_READ | KEY_WRITE) {
            Ok(k) => k,
            Err(e) => {
                if !add && e.kind() == std::io::ErrorKind::NotFound {
                    return Ok(false);
                }
                return Err(e);
            }
        }
    };

    let (current, val_type) = match key.get_raw_value("Path") {
        Ok(raw) => {
            let s = key.get_value::<String, _>("Path").unwrap_or_default();
            (s, raw.vtype)
        }
        Err(_e) => {
            if !add {
                return Ok(false);
            }
            (String::new(), REG_SZ)
        }
    };

    let updated = update_path_entries(&current, entry, add);
    if updated == current {
        return Ok(false);
    }

    if val_type == REG_EXPAND_SZ {
        let val = RegValue {
            bytes: updated
                .encode_utf16()
                .chain(std::iter::once(0))
                .flat_map(|u| u.to_le_bytes())
                .collect(),
            vtype: REG_EXPAND_SZ,
        };
        key.set_raw_value("Path", &val)?;
    } else {
        key.set_value("Path", &updated)?;
    }

    Ok(true)
}

pub fn broadcast_environment_change() {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows_sys::Win32::Foundation::{LPARAM, WPARAM};
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SendMessageTimeoutW, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
        };

        const HWND_BROADCAST: windows_sys::Win32::Foundation::HWND = 0xffff as _;
        let env_str: Vec<u16> = "Environment\0".encode_utf16().collect();
        let mut result: usize = 0;
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0 as WPARAM,
            env_str.as_ptr() as LPARAM,
            SMTO_ABORTIFHUNG,
            5000,
            &mut result as *mut usize as *mut _,
        );
    }
}

fn run_elevated_snip_path_update(entry: &str, add: bool) -> Result<(), String> {
    let action = if add { "add" } else { "remove" };
    let quoted_entry = format!("'{}'", entry.replace('\'', "''"));
    let script = format!(
        r#"$entry={};$current=[Environment]::GetEnvironmentVariable('Path','Machine');$parts=@($current -split ';' | ForEach-Object {{ $_.Trim() }} | Where-Object {{ $_ }});if('{}' -eq 'add'){{if(-not ($parts | Where-Object {{ [String]::Equals($_,$entry,[StringComparison]::OrdinalIgnoreCase) }})){{$parts += $entry}}}}else{{$parts=@($parts | Where-Object {{ -not [String]::Equals($_,$entry,[StringComparison]::OrdinalIgnoreCase) }})}};[Environment]::SetEnvironmentVariable('Path',($parts -join ';'),'Machine')"#,
        quoted_entry, action
    );

    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

        let op: Vec<u16> = "runas\0".encode_utf16().collect();
        let file: Vec<u16> = "powershell.exe\0".encode_utf16().collect();
        let params_str = format!("-NoProfile -NonInteractive -WindowStyle Hidden -Command \"{}\"\0", script);
        let params: Vec<u16> = params_str.encode_utf16().collect();

        let ret = unsafe {
            ShellExecuteW(
                0 as _,
                op.as_ptr(),
                file.as_ptr(),
                params.as_ptr(),
                std::ptr::null(),
                SW_HIDE as i32,
            )
        };

        if (ret as usize) <= 32 {
            return Err(format!("UAC 提权修改系统 PATH 失败或用户已取消 (错误码 {})", ret as usize));
        }

        std::thread::sleep(std::time::Duration::from_millis(1500));
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = script;
        Ok(())
    }
}

pub fn configure_snip_path(install_dir: &Path) -> Result<(bool, bool), String> {
    let entry = install_dir.to_string_lossy().to_string();

    #[cfg(target_os = "windows")]
    {
        let user_changed = match update_registry_path_value(HKEY_CURRENT_USER, USER_ENV_KEY, &entry, true) {
            Ok(changed) => changed,
            Err(e) => return Err(format!("写入用户 PATH 失败: {}", e)),
        };

        let direct_system = update_registry_path_value(HKEY_LOCAL_MACHINE, SYSTEM_ENV_KEY, &entry, true);
        let system_changed = match direct_system {
            Ok(changed) => changed,
            Err(_) => {
                run_elevated_snip_path_update(&entry, true)?;
                true
            }
        };

        if user_changed || system_changed {
            broadcast_environment_change();
        }

        Ok((user_changed, system_changed))
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = entry;
        Ok((false, false))
    }
}

pub fn remove_snip_path(install_dir: &Path) -> Result<(), String> {
    let entry = install_dir.to_string_lossy().to_string();

    #[cfg(target_os = "windows")]
    {
        let user_changed = update_registry_path_value(HKEY_CURRENT_USER, USER_ENV_KEY, &entry, false)
            .unwrap_or(false);

        let direct_system = update_registry_path_value(HKEY_LOCAL_MACHINE, SYSTEM_ENV_KEY, &entry, false);
        let system_changed = match direct_system {
            Ok(changed) => changed,
            Err(_) => {
                let _ = run_elevated_snip_path_update(&entry, false);
                true
            }
        };

        if user_changed || system_changed {
            broadcast_environment_change();
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = entry;
    }

    Ok(())
}

pub fn remove_owned_independent_path(
    install_dir: &Path,
    user_path: bool,
    system_path: bool,
) -> Result<(), String> {
    let entry = install_dir.to_string_lossy().to_string();
    let mut changed = false;

    #[cfg(target_os = "windows")]
    {
        if user_path {
            if let Ok(c) = update_registry_path_value(HKEY_CURRENT_USER, USER_ENV_KEY, &entry, false) {
                if c {
                    changed = true;
                }
            }
        }
        if system_path {
            let direct = update_registry_path_value(HKEY_LOCAL_MACHINE, SYSTEM_ENV_KEY, &entry, false);
            match direct {
                Ok(c) => {
                    if c {
                        changed = true;
                    }
                }
                Err(_) => {
                    let _ = run_elevated_snip_path_update(&entry, false);
                    changed = true;
                }
            }
        }
        if changed {
            broadcast_environment_change();
        }
    }

    Ok(())
}

pub fn launch_visible_powershell(command: &str, workdir: &Path) -> Result<(), String> {
    let script = format!("$env:TERM = $null; {}", command);
    let encoded = crate::common::windows::encode_powershell_base64(&script);

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        let mut cmd = Command::new("cmd.exe");
        cmd.args([
            "/d",
            "/c",
            "start",
            "powershell.exe",
            "-NoLogo",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-NoExit",
            "-EncodedCommand",
            &encoded,
        ]);
        cmd.current_dir(workdir);
        cmd.creation_flags(CREATE_NO_WINDOW);

        for (k, _) in std::env::vars() {
            if k.eq_ignore_ascii_case("TERM") {
                cmd.env_remove(&k);
            }
        }

        cmd.spawn().map_err(|e| format!("启动 PowerShell 失败: {}", e))?;
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (command, workdir);
    }

    Ok(())
}
