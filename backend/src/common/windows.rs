use std::path::{Path, PathBuf};
use tracing::info;

#[cfg(target_os = "windows")]
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
#[cfg(target_os = "windows")]
use winreg::RegKey;

pub const SINGLE_INSTANCE_MUTEX: &str = r"Local\code-Manager-rust-single-instance";
const STARTUP_RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const STARTUP_VALUE_NAME: &str = "code-Manager-rust";

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

/// 剥离 Windows 扩展长度路径前缀（如 \\?\C:\... 或 \??\C:\... 或 \\?\UNC\server\share）
pub fn strip_windows_verbatim_prefix<P: AsRef<Path>>(path: P) -> PathBuf {
    let p = path.as_ref();
    let s = p.to_string_lossy();

    if s.starts_with(r"\\?\UNC\") || s.starts_with(r"\??\UNC\") {
        return PathBuf::from(format!(r"\\{}", &s[8..]));
    }
    if s.starts_with("//?/UNC/") || s.starts_with("/??/UNC/") {
        return PathBuf::from(format!(r"\\{}", &s[8..].replace('/', "\\")));
    }
    if s.starts_with(r"\\?\") || s.starts_with(r"\??\") || s.starts_with("//?/") || s.starts_with("/??/") {
        let stripped = &s[4..];
        return PathBuf::from(stripped.replace('/', "\\"));
    }

    p.to_path_buf()
}

/// 清洗路径字符串（去除引号、首尾空白、Windows 逐字前缀，统一反斜杠，保留盘符根斜杠并去除普通路径末尾斜杠）
pub fn clean_path_str(path_str: &str) -> String {
    let trimmed = path_str.trim().trim_matches(['"', '\'']).trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let p = Path::new(trimmed);
    let stripped = strip_windows_verbatim_prefix(p);
    let mut s = stripped.to_string_lossy().replace('/', "\\");

    // 若形如 "C:\" 或 "D:\"，保留根斜杠；若是 "C:\abc\"，去除末尾斜杠
    if s.len() > 3 && s.ends_with('\\') {
        s.truncate(s.trim_end_matches('\\').len());
    }
    s
}

/// 跨平台安全的路径规范化函数：
/// 执行 canonicalize 并剥除 Windows \\?\ 扩展前缀
pub fn canonicalize_clean<P: AsRef<Path>>(path: P) -> std::io::Result<PathBuf> {
    let canon = path.as_ref().canonicalize()?;
    Ok(strip_windows_verbatim_prefix(&canon))
}

/// 尝试规范化并清理路径，若路径不存在则退回剥除前缀后的路径
pub fn canonicalize_or_clean<P: AsRef<Path>>(path: P) -> PathBuf {
    let p = path.as_ref();
    if let Ok(canon) = canonicalize_clean(p) {
        canon
    } else {
        strip_windows_verbatim_prefix(p)
    }
}

pub fn is_in_user_path(target_dir: &Path) -> bool {
    let target_clean = canonicalize_or_clean(target_dir);

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
                    let p_clean = canonicalize_or_clean(p);
                    if p_clean == target_clean
                        || clean_path_str(&p.to_string_lossy())
                            .eq_ignore_ascii_case(&clean_path_str(&target_dir.to_string_lossy()))
                    {
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
        let target_str = clean_path_str(&target_dir.to_string_lossy());

        let new_path = if current_path.is_empty() {
            target_str.clone()
        } else if current_path.ends_with(';') {
            format!("{}{}", current_path, target_str)
        } else {
            format!("{};{}", current_path, target_str)
        };

        env_key
            .set_value("Path", &new_path)
            .map_err(|e| format!("更新用户 PATH 失败: {}", e))?;

        info!("Added {:?} to user PATH", target_str);
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = target_dir;
        Ok(())
    }
}

pub fn remove_from_user_path(target_dir: &Path) -> Result<(), String> {
    let target_clean = canonicalize_or_clean(target_dir);

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
                        let p_clean = canonicalize_or_clean(p);
                        !(p_clean == target_clean
                            || clean_path_str(&p.to_string_lossy())
                                .eq_ignore_ascii_case(&clean_path_str(&target_dir.to_string_lossy())))
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

pub fn encode_powershell_base64(command: &str) -> String {
    let utf16_bytes: Vec<u8> = command
        .encode_utf16()
        .flat_map(|u| u.to_le_bytes())
        .collect();

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
    encoded
}

pub fn launch_visible_powershell(command: &str, workdir: Option<&Path>) -> Result<u32, String> {
    use std::process::Command;
    #[cfg(target_os = "windows")]
    use std::os::windows::process::CommandExt;

    let encoded = encode_powershell_base64(command);
    let mut cmd = Command::new("powershell.exe");

    #[cfg(target_os = "windows")]
    {
        // CREATE_NEW_CONSOLE: 为控制台程序创建独立的控制台窗口，直接捕获其真实 PID
        cmd.creation_flags(0x00000010);
    }

    cmd.args([
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

    let child = cmd.spawn().map_err(|e| format!("启动 PowerShell 失败: {}", e))?;
    Ok(child.id())
}

pub fn open_log_viewer(title: &str, log_path: &Path) -> Result<u32, String> {
    let script = format!(
        "$host.UI.RawUI.WindowTitle = '{}'; [Console]::InputEncoding = [System.Text.Encoding]::UTF8; [Console]::OutputEncoding = [System.Text.Encoding]::UTF8; $OutputEncoding = [System.Text.Encoding]::UTF8; Get-Content -LiteralPath '{}' -Encoding UTF8 -Wait -Tail 100",
        title.replace('\'', "''"),
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
            return Err("code-Manager-rust 实例已在运行中，请勿重复启动".to_string());
        }

        Ok(Some(SingleInstanceGuard { handle }))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = name;
        Ok(None)
    }
}

pub fn refresh_shortcut_icons() {
    #[cfg(target_os = "windows")]
    unsafe {
        let exe_path = match std::env::current_exe() {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("刷新快捷方式图标跳过: 无法获取当前可执行文件路径: {}", e);
                return;
            }
        };

        type HRESULT = i32;
        use windows_sys::core::GUID;

        const CLSID_SHELL_LINK: GUID = GUID {
            data1: 0x00021401,
            data2: 0x0000,
            data3: 0x0000,
            data4: [0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
        };
        const IID_ISHELL_LINK_W: GUID = GUID {
            data1: 0x000214f9,
            data2: 0x0000,
            data3: 0x0000,
            data4: [0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
        };
        const IID_IPERSIST_FILE: GUID = GUID {
            data1: 0x0000010b,
            data2: 0x0000,
            data3: 0x0000,
            data4: [0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
        };

        const COINIT_APARTMENTTHREADED: u32 = 0x2;
        const SHCNE_ASSOCCHANGED: i32 = 0x08000000;
        const SHCNF_IDLIST: u32 = 0x0000;

        #[link(name = "ole32")]
        extern "system" {
            fn CoInitializeEx(pvReserved: *const std::ffi::c_void, dwCoInit: u32) -> HRESULT;
            fn CoUninitialize();
        }

        use windows_sys::Win32::UI::Shell::SHChangeNotify;

        let co_init_res = CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED);
        let need_uninit = co_init_res == 0;

        let roots = [
            std::env::var("USERPROFILE").map(|p| PathBuf::from(p).join("Desktop")).ok(),
            std::env::var("PUBLIC").map(|p| PathBuf::from(p).join("Desktop")).ok(),
            std::env::var("APPDATA").map(|p| PathBuf::from(p).join("Microsoft").join("Windows").join("Start Menu")).ok(),
            std::env::var("PROGRAMDATA").map(|p| PathBuf::from(p).join("Microsoft").join("Windows").join("Start Menu")).ok(),
            std::env::var("APPDATA").map(|p| PathBuf::from(p).join("Microsoft").join("Windows").join("Start Menu").join("Programs").join("Startup")).ok(),
        ];

        let exe_str = exe_path.to_string_lossy();
        let exe_wide: Vec<u16> = exe_str.encode_utf16().chain(std::iter::once(0)).collect();

        for root_opt in roots.into_iter().flatten() {
            if !root_opt.exists() {
                continue;
            }
            walk_and_refresh(&root_opt, &exe_wide, &CLSID_SHELL_LINK, &IID_ISHELL_LINK_W, &IID_IPERSIST_FILE);
        }

        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, std::ptr::null(), std::ptr::null());

        if need_uninit {
            CoUninitialize();
        }
    }
}

#[cfg(target_os = "windows")]
unsafe fn walk_and_refresh(
    dir: &Path,
    exe_wide: &[u16],
    clsid_shell_link: &windows_sys::core::GUID,
    iid_shell_link: &windows_sys::core::GUID,
    iid_persist_file: &windows_sys::core::GUID,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            walk_and_refresh(&p, exe_wide, clsid_shell_link, iid_shell_link, iid_persist_file);
        } else if p.extension().map(|e| e.eq_ignore_ascii_case("lnk")).unwrap_or(false) {
            let _ = refresh_single_shortcut(&p, exe_wide, clsid_shell_link, iid_shell_link, iid_persist_file);
        }
    }
}

#[cfg(target_os = "windows")]
unsafe fn refresh_single_shortcut(
    path: &Path,
    exe_wide: &[u16],
    clsid_shell_link: &windows_sys::core::GUID,
    iid_shell_link: &windows_sys::core::GUID,
    iid_persist_file: &windows_sys::core::GUID,
) -> Result<bool, ()> {
    const CLSCTX_INPROC_SERVER: u32 = 0x1;
    const STGM_READ: u32 = 0x0;
    const SLGP_RAWPATH: u32 = 0x4;
    const SHCNE_UPDATEITEM: i32 = 0x00002000;
    const SHCNF_PATHW: u32 = 0x0005;

    #[link(name = "ole32")]
    extern "system" {
        fn CoCreateInstance(
            rclsid: *const windows_sys::core::GUID,
            pUnkOuter: *mut std::ffi::c_void,
            dwClsContext: u32,
            riid: *const windows_sys::core::GUID,
            ppv: *mut *mut std::ffi::c_void,
        ) -> i32;
    }

    let mut link: *mut std::ffi::c_void = std::ptr::null_mut();
    if CoCreateInstance(clsid_shell_link, std::ptr::null_mut(), CLSCTX_INPROC_SERVER, iid_shell_link, &mut link) < 0 || link.is_null() {
        return Err(());
    }

    let link_vtable = *(link as *mut *mut usize);

    type FnQueryInterface = unsafe extern "system" fn(*mut std::ffi::c_void, *const windows_sys::core::GUID, *mut *mut std::ffi::c_void) -> i32;
    let query_interface: FnQueryInterface = std::mem::transmute(*link_vtable.add(0));
    let mut persist: *mut std::ffi::c_void = std::ptr::null_mut();
    if query_interface(link, iid_persist_file, &mut persist) < 0 || persist.is_null() {
        type FnRelease = unsafe extern "system" fn(*mut std::ffi::c_void) -> u32;
        let release: FnRelease = std::mem::transmute(*link_vtable.add(2));
        release(link);
        return Err(());
    }

    let persist_vtable = *(persist as *mut *mut usize);

    type FnLoad = unsafe extern "system" fn(*mut std::ffi::c_void, *const u16, u32) -> i32;
    let load: FnLoad = std::mem::transmute(*persist_vtable.add(5));
    let path_str = path.to_string_lossy();
    let path_wide: Vec<u16> = path_str.encode_utf16().chain(std::iter::once(0)).collect();
    if load(persist, path_wide.as_ptr(), STGM_READ) < 0 {
        type FnRelease = unsafe extern "system" fn(*mut std::ffi::c_void) -> u32;
        let release_persist: FnRelease = std::mem::transmute(*persist_vtable.add(2));
        release_persist(persist);
        let release_link: FnRelease = std::mem::transmute(*link_vtable.add(2));
        release_link(link);
        return Err(());
    }

    type FnGetPath = unsafe extern "system" fn(*mut std::ffi::c_void, *mut u16, i32, *mut std::ffi::c_void, u32) -> i32;
    let get_path: FnGetPath = std::mem::transmute(*link_vtable.add(3));
    let mut target_buf = vec![0u16; 32768];
    if get_path(link, target_buf.as_mut_ptr(), target_buf.len() as i32, std::ptr::null_mut(), SLGP_RAWPATH) < 0 {
        type FnRelease = unsafe extern "system" fn(*mut std::ffi::c_void) -> u32;
        let release_persist: FnRelease = std::mem::transmute(*persist_vtable.add(2));
        release_persist(persist);
        let release_link: FnRelease = std::mem::transmute(*link_vtable.add(2));
        release_link(link);
        return Err(());
    }

    let len = target_buf.iter().position(|&c| c == 0).unwrap_or(0);
    let target_str = String::from_utf16_lossy(&target_buf[..len]);
    let target_path = Path::new(&target_str);
    let target_name = target_path.file_name().and_then(|n| n.to_str()).unwrap_or("");

    let is_match = target_name.eq_ignore_ascii_case("code-Manager-rust.exe");

    let mut updated = false;
    if is_match {
        type FnSetIconLocation = unsafe extern "system" fn(*mut std::ffi::c_void, *const u16, i32) -> i32;
        let set_icon_location: FnSetIconLocation = std::mem::transmute(*link_vtable.add(17));
        if set_icon_location(link, exe_wide.as_ptr(), 0) >= 0 {
            type FnSave = unsafe extern "system" fn(*mut std::ffi::c_void, *const u16, i32) -> i32;
            let save: FnSave = std::mem::transmute(*persist_vtable.add(6));
            if save(persist, std::ptr::null(), 1) >= 0 {
                updated = true;
                windows_sys::Win32::UI::Shell::SHChangeNotify(
                    SHCNE_UPDATEITEM,
                    SHCNF_PATHW,
                    path_wide.as_ptr() as _,
                    std::ptr::null(),
                );
            }
        }
    }

    type FnRelease = unsafe extern "system" fn(*mut std::ffi::c_void) -> u32;
    let release_persist: FnRelease = std::mem::transmute(*persist_vtable.add(2));
    release_persist(persist);
    let release_link: FnRelease = std::mem::transmute(*link_vtable.add(2));
    release_link(link);

    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_windows_verbatim_prefix() {
        assert_eq!(
            strip_windows_verbatim_prefix(Path::new(r"\\?\C:\EXEXX\edit")),
            PathBuf::from(r"C:\EXEXX\edit")
        );
        assert_eq!(
            strip_windows_verbatim_prefix(Path::new(r"\??\C:\EXEXX\edit")),
            PathBuf::from(r"C:\EXEXX\edit")
        );
        assert_eq!(
            strip_windows_verbatim_prefix(Path::new(r"\\?\UNC\server\share\dir")),
            PathBuf::from(r"\\server\share\dir")
        );
        assert_eq!(
            strip_windows_verbatim_prefix(Path::new(r"C:\EXEXX\edit")),
            PathBuf::from(r"C:\EXEXX\edit")
        );
    }

    #[test]
    fn test_clean_path_str() {
        assert_eq!(clean_path_str(r#"  "\\?\C:\EXEXX\edit\"  "#), r"C:\EXEXX\edit");
        assert_eq!(clean_path_str(r"\\?\C:\"), r"C:\");
        assert_eq!(clean_path_str(r"C:/EXEXX/edit/"), r"C:\EXEXX\edit");
        assert_eq!(clean_path_str(r#"\\?\UNC\server/share\dir\"#), r"\\server\share\dir");
    }
}
