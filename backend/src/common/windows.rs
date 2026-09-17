use std::path::{Path, PathBuf};
use tracing::info;

#[cfg(target_os = "windows")]
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
#[cfg(target_os = "windows")]
use winreg::RegKey;

const STARTUP_RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const STARTUP_VALUE_NAME: &str = "code-Manager";

pub fn sync_startup_registry(enabled: bool, background: bool) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if !enabled {
            if let Ok(key) = hkcu.open_subkey_with_flags(STARTUP_RUN_KEY, KEY_WRITE) {
                let _ = key.delete_value(STARTUP_VALUE_NAME);
            }
            return Ok(());
        }

        let exe_path = std::env::current_exe().map_err(|e| format!("获取当前可执行文件路径失败: {}", e))?;
        let exe_str = exe_path.to_string_lossy();

        let mut cmd = format!("\"{}\" --startup", exe_str);
        if background {
            cmd.push_str(" --background");
        }

        let (key, _) = hkcu
            .create_subkey(STARTUP_RUN_KEY)
            .map_err(|e| format!("打开 Windows 注册表启动项失败: {}", e))?;

        key.set_value(STARTUP_VALUE_NAME, &cmd)
            .map_err(|e| format!("写入 Windows 注册表启动项失败: {}", e))?;

        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (enabled, background);
        Ok(())
    }
}

pub fn read_startup_registry() -> (bool, bool) {
    #[cfg(target_os = "windows")]
    {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Ok(key) = hkcu.open_subkey_with_flags(STARTUP_RUN_KEY, KEY_READ) {
            if let Ok(val) = key.get_value::<String, _>(STARTUP_VALUE_NAME) {
                let is_bg = val.contains("--background");
                return (true, is_bg);
            }
        }
    }
    (false, false)
}

pub fn get_user_profile_dir() -> PathBuf {
    if let Ok(profile) = std::env::var("USERPROFILE") {
        PathBuf::from(profile)
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home)
    } else {
        PathBuf::from(r"C:\Users\Default")
    }
}

pub fn get_app_data_dir() -> PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        PathBuf::from(appdata)
    } else {
        get_user_profile_dir().join("AppData").join("Roaming")
    }
}

pub fn is_in_user_path(target_dir: &Path) -> bool {
    let target_clean = match target_dir.canonicalize() {
        Ok(c) => c,
        Err(_) => target_dir.to_path_buf(),
    };

    #[cfg(target_os = "windows")]
    {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Ok(env_key) = hkcu.open_subkey_with_flags("Environment", KEY_READ) {
            if let Ok(path_val) = env_key.get_value::<String, _>("Path") {
                for part in path_val.split(';') {
                    let trimmed = part.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let p = Path::new(trimmed);
                    let p_clean = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
                    if p_clean == target_clean || p.to_string_lossy().eq_ignore_ascii_case(&target_dir.to_string_lossy()) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

pub fn add_to_user_path(target_dir: &Path) -> Result<(), String> {
    if is_in_user_path(target_dir) {
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (env_key, _) = hkcu
            .create_subkey("Environment")
            .map_err(|e| format!("打开 HKCU\\Environment 失败: {}", e))?;

        let current_path: String = env_key.get_value("Path").unwrap_or_default();
        let target_str = target_dir.to_string_lossy();

        let new_path = if current_path.is_empty() {
            target_str.to_string()
        } else if current_path.ends_with(';') {
            format!("{}{}", current_path, target_str)
        } else {
            format!("{};{}", current_path, target_str)
        };

        env_key
            .set_value("Path", &new_path)
            .map_err(|e| format!("更新用户 PATH 失败: {}", e))?;

        info!("Added {:?} to user PATH", target_dir);
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = target_dir;
        Ok(())
    }
}

pub fn remove_from_user_path(target_dir: &Path) -> Result<(), String> {
    let target_clean = match target_dir.canonicalize() {
        Ok(c) => c,
        Err(_) => target_dir.to_path_buf(),
    };

    #[cfg(target_os = "windows")]
    {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Ok(env_key) = hkcu.open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE) {
            if let Ok(path_val) = env_key.get_value::<String, _>("Path") {
                let parts: Vec<&str> = path_val
                    .split(';')
                    .filter(|part| {
                        let trimmed = part.trim();
                        if trimmed.is_empty() {
                            return false;
                        }
                        let p = Path::new(trimmed);
                        let p_clean = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
                        !(p_clean == target_clean || p.to_string_lossy().eq_ignore_ascii_case(&target_dir.to_string_lossy()))
                    })
                    .collect();

                let new_path = parts.join(";");
                env_key
                    .set_value("Path", &new_path)
                    .map_err(|e| format!("移除用户 PATH 失败: {}", e))?;

                info!("Removed {:?} from user PATH", target_dir);
            }
        }
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = target_dir;
        Ok(())
    }
}

pub fn launch_visible_powershell(command: &str, workdir: Option<&Path>) -> Result<(), String> {
    use std::process::Command;

    // Encode powershell script as UTF-16LE Base64
    let utf16_bytes: Vec<u8> = command
        .encode_utf16()
        .flat_map(|u| u.to_le_bytes())
        .collect();

    // Standard base64 encoding
    const BASE64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::new();
    for chunk in utf16_bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };

        encoded.push(BASE64_ALPHABET[(b0 >> 2) as usize] as char);
        encoded.push(BASE64_ALPHABET[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            encoded.push(BASE64_ALPHABET[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            encoded.push('=');
        }
        if chunk.len() > 2 {
            encoded.push(BASE64_ALPHABET[(b2 & 0x3f) as usize] as char);
        } else {
            encoded.push('=');
        }
    }

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

    if let Some(dir) = workdir {
        cmd.current_dir(dir);
    }

    cmd.spawn().map_err(|e| format!("启动 PowerShell 失败: {}", e))?;
    Ok(())
}

pub fn open_log_viewer(title: &str, log_path: &Path) -> Result<(), String> {
    let script = format!(
        "$host.UI.RawUI.WindowTitle = '{}'; Get-Content -Path '{}' -Wait -Tail 50",
        title,
        log_path.to_string_lossy().replace('\'', "''")
    );
    launch_visible_powershell(&script, log_path.parent())
}

#[cfg(target_os = "windows")]
pub struct SingleInstanceGuard {
    handle: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(target_os = "windows")]
impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        unsafe {
            if !self.handle.is_null() {
                windows_sys::Win32::System::Threading::ReleaseMutex(self.handle);
                windows_sys::Win32::Foundation::CloseHandle(self.handle);
            }
        }
    }
}

pub fn acquire_single_instance(name: &str) -> Result<Option<SingleInstanceGuard>, String> {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
        use windows_sys::Win32::System::Threading::CreateMutexW;

        let wide_name: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        let handle = CreateMutexW(std::ptr::null(), 1, wide_name.as_ptr());

        if handle.is_null() {
            return Err("创建单实例互斥锁失败".to_string());
        }

        if GetLastError() == ERROR_ALREADY_EXISTS {
            windows_sys::Win32::Foundation::CloseHandle(handle);
            return Err("code-Manager 实例已在运行中".to_string());
        }

        Ok(Some(SingleInstanceGuard { handle }))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = name;
        Ok(None)
    }
}
