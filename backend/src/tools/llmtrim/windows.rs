use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::info;

#[cfg(target_os = "windows")]
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE};
#[cfg(target_os = "windows")]
use winreg::RegKey;

use crate::common::windows::get_user_profile_dir;

pub const LLMTRIM_PROXY_URL: &str = "http://127.0.0.1:43117";
pub const LLMTRIM_NO_PROXY: &str = "localhost,127.0.0.1,::1,10.0.0.0/8,172.16.0.0/12,192.168.0.0/16,169.254.0.0/16,fd00::/8,*.local";
pub const LLMTRIM_RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
pub const LLMTRIM_STARTUP_APPROVED_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

pub fn get_llmtrim_ca_path() -> PathBuf {
    get_user_profile_dir().join(".llmtrim").join("ca.pem")
}

pub fn is_llmtrim_proxy_value(val: &str) -> bool {
    let clean = val.trim().trim_end_matches('/');
    clean.eq_ignore_ascii_case(LLMTRIM_PROXY_URL)
}

pub fn verify_llmtrim_windows_setup() -> (bool, String) {
    #[cfg(target_os = "windows")]
    {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let env_key = match hkcu.open_subkey_with_flags(r"Environment", KEY_READ) {
            Ok(k) => k,
            Err(e) => return (false, format!("无法读取 HKCU\\Environment: {}", e)),
        };

        let mut missing = Vec::new();

        for name in &["HTTPS_PROXY", "HTTP_PROXY"] {
            match env_key.get_value::<String, _>(*name) {
                Ok(val) if is_llmtrim_proxy_value(&val) => {}
                _ => missing.push(format!("{}={}", name, LLMTRIM_PROXY_URL)),
            }
        }

        let ca_path = get_llmtrim_ca_path();
        if !ca_path.exists() {
            missing.push(format!("CA 文件 {:?}", ca_path));
        }

        let ca_path_str = ca_path.to_string_lossy().to_string();
        match env_key.get_value::<String, _>("NODE_EXTRA_CA_CERTS") {
            Ok(val) if val.trim().eq_ignore_ascii_case(&ca_path_str) || val.trim().eq_ignore_ascii_case(r"%USERPROFILE%\.llmtrim\ca.pem") => {}
            _ => missing.push(format!("NODE_EXTRA_CA_CERTS={}", ca_path_str)),
        }

        match env_key.get_value::<String, _>("NODE_USE_ENV_PROXY") {
            Ok(val) if val.trim() == "1" => {}
            _ => missing.push("NODE_USE_ENV_PROXY=1".to_string()),
        }

        if !missing.is_empty() {
            return (false, format!("缺少或不匹配：{}", missing.join("；")));
        }

        (true, "Windows 用户代理环境和本地 CA 已配置。".to_string())
    }

    #[cfg(not(target_os = "windows"))]
    {
        (true, "非 Windows 平台免除注册表环境校验".to_string())
    }
}

pub fn clear_llmtrim_user_environment() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let env_key = match hkcu.open_subkey_with_flags(r"Environment", KEY_READ | KEY_WRITE) {
            Ok(k) => k,
            Err(e) => return Err(format!("打开 HKCU\\Environment 失败: {}", e)),
        };

        let mut managed_proxy = false;
        for name in &["HTTPS_PROXY", "HTTP_PROXY"] {
            if let Ok(val) = env_key.get_value::<String, _>(*name) {
                if is_llmtrim_proxy_value(&val) {
                    let _ = env_key.delete_value(*name);
                    managed_proxy = true;
                }
            }
        }

        if let Ok(val) = env_key.get_value::<String, _>("NO_PROXY") {
            if val.trim().eq_ignore_ascii_case(LLMTRIM_NO_PROXY) {
                let _ = env_key.delete_value("NO_PROXY");
            }
        }

        let ca_path_str = get_llmtrim_ca_path().to_string_lossy().to_string();
        if let Ok(val) = env_key.get_value::<String, _>("NODE_EXTRA_CA_CERTS") {
            if val.trim().eq_ignore_ascii_case(&ca_path_str) || val.trim().eq_ignore_ascii_case(r"%USERPROFILE%\.llmtrim\ca.pem") {
                let _ = env_key.delete_value("NODE_EXTRA_CA_CERTS");
            }
        }

        if managed_proxy {
            if let Ok(val) = env_key.get_value::<String, _>("NODE_USE_ENV_PROXY") {
                if val.trim() == "1" {
                    let _ = env_key.delete_value("NODE_USE_ENV_PROXY");
                }
            }
        }

        broadcast_windows_environment_change();
        info!("已成功清理 Windows 用户环境中的 llmtrim 变量");
    }
    Ok(())
}

pub fn query_llmtrim_environment_residual() -> Result<bool, String> {
    #[cfg(target_os = "windows")]
    {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let env_key = match hkcu.open_subkey_with_flags(r"Environment", KEY_READ) {
            Ok(k) => k,
            Err(_) => return Ok(false),
        };

        for name in &["HTTPS_PROXY", "HTTP_PROXY"] {
            if let Ok(val) = env_key.get_value::<String, _>(*name) {
                if is_llmtrim_proxy_value(&val) {
                    return Ok(true);
                }
            }
        }

        if let Ok(val) = env_key.get_value::<String, _>("NO_PROXY") {
            if val.trim().eq_ignore_ascii_case(LLMTRIM_NO_PROXY) {
                return Ok(true);
            }
        }

        let ca_path_str = get_llmtrim_ca_path().to_string_lossy().to_string();
        if let Ok(val) = env_key.get_value::<String, _>("NODE_EXTRA_CA_CERTS") {
            if val.trim().eq_ignore_ascii_case(&ca_path_str) || val.trim().eq_ignore_ascii_case(r"%USERPROFILE%\.llmtrim\ca.pem") {
                return Ok(true);
            }
        }

        if let Ok(val) = env_key.get_value::<String, _>("NODE_USE_ENV_PROXY") {
            if val.trim() == "1" {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub fn remove_llmtrim_autostart() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        for subkey in &[LLMTRIM_RUN_KEY, LLMTRIM_STARTUP_APPROVED_KEY] {
            if let Ok(key) = hkcu.open_subkey_with_flags(*subkey, KEY_READ | KEY_WRITE) {
                for name in &["llmtrim", "llmtrim-tray"] {
                    let _ = key.delete_value(*name);
                }
            }
        }

        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        let mut needs_elevation = false;
        for subkey in &[LLMTRIM_RUN_KEY, LLMTRIM_STARTUP_APPROVED_KEY] {
            if let Ok(key) = hklm.open_subkey_with_flags(*subkey, KEY_READ | KEY_WRITE) {
                for name in &["llmtrim", "llmtrim-tray"] {
                    let _ = key.delete_value(*name);
                }
            } else if let Ok(key) = hklm.open_subkey_with_flags(*subkey, KEY_READ) {
                for name in &["llmtrim", "llmtrim-tray"] {
                    if key.get_raw_value(*name).is_ok() {
                        needs_elevation = true;
                    }
                }
            }
        }

        if needs_elevation {
            let script = r#"$keys=@('Software\Microsoft\Windows\CurrentVersion\Run','Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run');$names=@('llmtrim','llmtrim-tray');foreach($key in $keys){foreach($name in $names){Remove-ItemProperty -LiteralPath ('HKLM:\'+$key) -Name $name -ErrorAction SilentlyContinue}}"#;
            let mut cmd = Command::new("powershell");
            cmd.args(["-NoProfile", "-NonInteractive", "-Command", script]);
            #[cfg(target_os = "windows")]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
            }
            let _ = cmd.output();
        }
    }
    Ok(())
}

pub fn query_llmtrim_autostart_residual() -> Result<bool, String> {
    #[cfg(target_os = "windows")]
    {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        for subkey in &[LLMTRIM_RUN_KEY, LLMTRIM_STARTUP_APPROVED_KEY] {
            if let Ok(key) = hkcu.open_subkey_with_flags(*subkey, KEY_READ) {
                for name in &["llmtrim", "llmtrim-tray"] {
                    if key.get_raw_value(*name).is_ok() {
                        return Ok(true);
                    }
                }
            }
        }

        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        for subkey in &[LLMTRIM_RUN_KEY, LLMTRIM_STARTUP_APPROVED_KEY] {
            if let Ok(key) = hklm.open_subkey_with_flags(*subkey, KEY_READ) {
                for name in &["llmtrim", "llmtrim-tray"] {
                    if key.get_raw_value(*name).is_ok() {
                        return Ok(true);
                    }
                }
            }
        }
    }
    Ok(false)
}

pub fn remove_llmtrim_user_trust() -> Result<bool, String> {
    #[cfg(target_os = "windows")]
    {
        let mut cmd = Command::new("certutil.exe");
        cmd.args(["-user", "-delstore", "Root", "llmtrim local CA"]);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        let _ = cmd.output();

        let mut verify_cmd = Command::new("certutil.exe");
        verify_cmd.args(["-user", "-store", "Root", "llmtrim local CA"]);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            verify_cmd.creation_flags(0x08000000);
        }
        if let Ok(output) = verify_cmd.output() {
            if output.status.success() {
                return Err("llmtrim local CA 仍存在于当前用户 Root 证书库".to_string());
            }
        }
    }
    Ok(true)
}

pub fn broadcast_windows_environment_change() {
    #[cfg(target_os = "windows")]
    {
        let script = r#"$sig='[DllImport("user32.dll", SetLastError=true, CharSet=CharSet.Auto)] public static extern IntPtr SendMessageTimeout(IntPtr hWnd,uint Msg,UIntPtr wParam,string lParam,uint fuFlags,uint uTimeout,out UIntPtr lpdwResult);';$t=Add-Type -MemberDefinition $sig -Name LLMTrimEnvBroadcast -Namespace CodeManager -PassThru;$r=[UIntPtr]::Zero;[void]$t::SendMessageTimeout([IntPtr]0xffff,0x1A,[UIntPtr]::Zero,'Environment',0x2,5000,[ref]$r)"#;
        let mut cmd = Command::new("powershell");
        cmd.args(["-NoProfile", "-NonInteractive", "-Command", script]);
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
        let _ = cmd.output();
    }
}

pub fn query_llmtrim_residual(
    configured_path: &str,
    install_dir: &Path,
    running: bool,
    tray_running: bool,
    directory_exists: bool,
    state_dir_exists: bool,
) -> Result<bool, String> {
    let mut residual = running || tray_running || directory_exists || !configured_path.trim().is_empty();
    if install_dir.exists() {
        residual = residual || state_dir_exists;
        let autostart = query_llmtrim_autostart_residual().unwrap_or(false);
        let env_res = query_llmtrim_environment_residual().unwrap_or(false);
        residual = residual || autostart || env_res;
    }
    Ok(residual)
}
