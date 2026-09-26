use std::path::Path;
use std::process::Command;

#[cfg(target_os = "windows")]
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE, REG_EXPAND_SZ, REG_SZ};
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

pub fn query_gortex_path_status(bin_dir: &Path) -> (bool, bool, Option<String>) {
    let entry = bin_dir.to_string_lossy().to_string();
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
fn update_registry_path_value(hkey: winreg::HKEY, subkey: &str, entry: &str, add: bool) -> Result<bool, std::io::Error> {
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
            bytes: updated.encode_utf16().chain(std::iter::once(0)).flat_map(|u| u.to_le_bytes()).collect(),
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

fn run_elevated_gortex_path_update(entry: &str, add: bool) -> Result<(), String> {
    let action = if add { "add" } else { "remove" };
    let quoted_entry = format!("'{}'", entry.replace('\'', "''"));
    let script = format!(
        r#"$entry={};$current=[Environment]::GetEnvironmentVariable('Path','Machine');$parts=@($current -split ';' | ForEach-Object {{ $_.Trim() }} | Where-Object {{ $_ }});if('{}' -eq 'add'){{if(-not ($parts | Where-Object {{ [String]::Equals($_,$entry,[StringComparison]::OrdinalIgnoreCase) }})){{$parts += $entry}}}}else{{$parts=@($parts | Where-Object {{ -not [String]::Equals($_,$entry,[StringComparison]::OrdinalIgnoreCase) }})}};[Environment]::SetEnvironmentVariable('Path',($parts -join ';'),'Machine')"#,
        quoted_entry, action
    );

    let encoded = crate::common::windows::encode_powershell_base64(&script);
    let outer = format!(
        r#"$p=Start-Process -FilePath 'powershell.exe' -Verb RunAs -WindowStyle Hidden -ArgumentList @('-NoLogo','-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-EncodedCommand','{}') -Wait -PassThru;exit $p.ExitCode"#,
        encoded
    );

    let output = Command::new("powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &outer])
        .output()
        .map_err(|e| format!("调用 UAC 提权 PowerShell 失败: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let msg = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("提权命令退出码 {}", output.status.code().unwrap_or(-1))
        };
        return Err(msg);
    }

    Ok(())
}

pub fn configure_gortex_path(bin_dir: &Path) -> Result<(bool, bool), String> {
    let entry = bin_dir.to_string_lossy().to_string();
    let (user_path, system_path, msg) = query_gortex_path_status(bin_dir);
    if let Some(err) = msg {
        return Err(err);
    }

    #[cfg(target_os = "windows")]
    {
        let mut user_added = false;
        if !user_path {
            match update_registry_path_value(HKEY_CURRENT_USER, USER_ENV_KEY, &entry, true) {
                Ok(changed) => {
                    user_added = changed;
                }
                Err(e) => return Err(format!("写入用户 PATH 失败: {}", e)),
            }
        }

        if !system_path {
            match update_registry_path_value(HKEY_LOCAL_MACHINE, SYSTEM_ENV_KEY, &entry, true) {
                Ok(_) => {}
                Err(e) => {
                    let is_denied = e.kind() == std::io::ErrorKind::PermissionDenied;
                    let elevated_res = if is_denied {
                        run_elevated_gortex_path_update(&entry, true)
                    } else {
                        Err(e.to_string())
                    };

                    if let Err(elevated_err) = elevated_res {
                        let mut rollback_err = None;
                        if user_added {
                            if let Err(r_err) = update_registry_path_value(HKEY_CURRENT_USER, USER_ENV_KEY, &entry, false) {
                                rollback_err = Some(r_err.to_string());
                            }
                        }
                        broadcast_environment_change();
                        if let Some(r_msg) = rollback_err {
                            return Err(format!("写入系统 PATH 需要管理员权限，UAC 操作未完成: {}；回滚用户 PATH 失败: {}", elevated_err, r_msg));
                        }
                        return Err(format!("写入系统 PATH 需要管理员权限，UAC 操作未完成: {}", elevated_err));
                    }
                }
            }
        }

        broadcast_environment_change();
        let (u, s, _) = query_gortex_path_status(bin_dir);
        if !u || !s {
            return Err("用户或系统 PATH 写入后仍未检测到 Gortex 目录".to_string());
        }
        return Ok((u, s));
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = entry;
        Ok((true, true))
    }
}

pub fn remove_owned_gortex_path(bin_dir: &Path, user_owned: bool, system_owned: bool) -> Result<(), String> {
    let entry = bin_dir.to_string_lossy().to_string();
    let mut failures = Vec::new();

    #[cfg(target_os = "windows")]
    {
        if user_owned {
            if let Err(e) = update_registry_path_value(HKEY_CURRENT_USER, USER_ENV_KEY, &entry, false) {
                failures.push(format!("用户 PATH: {}", e));
            }
        }

        if system_owned {
            if let Err(e) = update_registry_path_value(HKEY_LOCAL_MACHINE, SYSTEM_ENV_KEY, &entry, false) {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    if let Err(elevated_err) = run_elevated_gortex_path_update(&entry, false) {
                        failures.push(format!("系统 PATH: {}", elevated_err));
                    }
                } else {
                    failures.push(format!("系统 PATH: {}", e));
                }
            }
        }

        broadcast_environment_change();
    }

    if !failures.is_empty() {
        return Err(failures.join("；"));
    }

    Ok(())
}
