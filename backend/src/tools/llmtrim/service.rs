use std::fs;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tracing::{info, warn};

use super::config::{
    is_config_managed, remove_default_tracking_database,
    remove_managed_config_dir, remove_tracking_database, sync_managed_llmtrim_config, tracking_db_path,
};
use super::ownership::{
    mark_managed_tool_attention, read_managed_tool_state, set_managed_tool_desired_running,
    write_managed_tool_state, ManagedToolState,
};
use super::release::{download_and_install_llmtrim, fetch_llmtrim_releases};
use super::types::{
    LlmtrimActivationSnapshot, LlmtrimReleaseListResponse, LlmtrimStatusResponse,
};
use super::windows::{
    clear_llmtrim_user_environment, query_llmtrim_residual, remove_llmtrim_autostart,
    remove_llmtrim_user_trust, verify_llmtrim_windows_setup,
};
use crate::common::process::{
    find_all_process_ids, invalidate_process_cache, is_pid_alive, kill_process_by_pid,
};
use crate::common::windows::get_user_profile_dir;

pub const LLMTRIM_PORT: &str = "43117";
pub const LLMTRIM_EXECUTABLE_NAME: &str = "llmtrim.exe";
pub const LLMTRIM_TRAY_EXECUTABLE_NAME: &str = "llmtrim-tray.exe";

#[cfg(target_os = "windows")]
fn process_matches_executable(pid: u32, executable: &Path) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut buffer = vec![0u16; 32768];
        let mut len = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut len) != 0;
        CloseHandle(handle);
        if !ok {
            return false;
        }
        let actual = PathBuf::from(String::from_utf16_lossy(&buffer[..len as usize]));
        let actual = crate::common::windows::canonicalize_or_clean(actual);
        let expected = crate::common::windows::canonicalize_or_clean(executable);
        actual.to_string_lossy().eq_ignore_ascii_case(&expected.to_string_lossy())
    }
}

#[cfg(not(target_os = "windows"))]
fn process_matches_executable(_pid: u32, _executable: &Path) -> bool {
    false
}

pub(super) fn recorded_daemon_pid(state_dir: &Path, executable: &Path) -> Option<u32> {
    let content = fs::read(state_dir.join("serve.pid")).ok()?;
    let value = serde_json::from_slice::<serde_json::Value>(&content).ok()?;
    let pid = u32::try_from(value.get("pid")?.as_u64()?).ok()?;
    if is_pid_alive(pid) && process_matches_executable(pid, executable) {
        Some(pid)
    } else {
        None
    }
}

#[cfg(target_os = "windows")]
pub(super) fn listener_pid_for_executable(executable: &Path, port: u16) -> Option<u32> {
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCPROW_OWNER_PID, MIB_TCPTABLE_OWNER_PID,
        TCP_TABLE_OWNER_PID_LISTENER,
    };
    use windows_sys::Win32::Networking::WinSock::AF_INET;

    unsafe {
        let mut size = 0u32;
        GetExtendedTcpTable(std::ptr::null_mut(), &mut size, 0, AF_INET as u32, TCP_TABLE_OWNER_PID_LISTENER, 0);
        if size == 0 {
            return None;
        }
        // u32 storage keeps the variable-length Windows table properly aligned.
        let capacity = (size as usize).max(std::mem::size_of::<MIB_TCPTABLE_OWNER_PID>());
        let mut buffer = vec![0u32; (capacity + 3) / 4];
        size = (buffer.len() * 4) as u32;
        if GetExtendedTcpTable(buffer.as_mut_ptr().cast(), &mut size, 0, AF_INET as u32, TCP_TABLE_OWNER_PID_LISTENER, 0) != 0 {
            return None;
        }
        let table = &*(buffer.as_ptr().cast::<MIB_TCPTABLE_OWNER_PID>());
        let count = table.dwNumEntries as usize;
        let header_size = std::mem::offset_of!(MIB_TCPTABLE_OWNER_PID, table);
        if count > (size as usize).saturating_sub(header_size) / std::mem::size_of::<MIB_TCPROW_OWNER_PID>() {
            return None;
        }
        let rows = std::slice::from_raw_parts(table.table.as_ptr(), count);
        rows.iter()
            .filter(|row| u16::from_be(row.dwLocalPort as u16) == port)
            .filter(|row| row.dwLocalAddr == 0 || row.dwLocalAddr == u32::from_ne_bytes([127, 0, 0, 1]))
            .find(|row| process_matches_executable(row.dwOwningPid, executable))
            .map(|row| row.dwOwningPid)
    }
}

#[cfg(not(target_os = "windows"))]
pub(super) fn listener_pid_for_executable(_executable: &Path, _port: u16) -> Option<u32> {
    None
}

pub(super) fn run_llmtrim_setup(executable: &Path) -> Result<(), String> {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let log_path = std::env::temp_dir().join(format!("code-manager-llmtrim-{}-{}.log", std::process::id(), nonce));
    let result = (|| {
        let log = fs::OpenOptions::new().write(true).create_new(true).open(&log_path)
            .map_err(|e| format!("创建 llmtrim setup 输出文件失败: {}", e))?;
        let stderr = log.try_clone().map_err(|e| format!("准备 llmtrim setup 输出失败: {}", e))?;
        let mut cmd = Command::new(executable);
        cmd.args(["setup", "--port", LLMTRIM_PORT])
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(stderr));
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000);
        }
        let mut child = cmd.spawn().map_err(|e| format!("启动 llmtrim setup 失败: {}", e))?;
        let deadline = Instant::now() + Duration::from_secs(40);
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("llmtrim setup 执行超时（40 秒）".to_string());
                }
                Err(e) => return Err(format!("等待 llmtrim setup 失败: {}", e)),
            }
        };
        if !status.success() {
            let output = fs::read(&log_path).unwrap_or_default();
            let message = String::from_utf8_lossy(&output);
            let detail: String = message.trim().chars().take(2000).collect();
            return Err(if detail.is_empty() {
                format!("llmtrim setup 失败: {}", status)
            } else {
                format!("llmtrim setup 失败: {}", detail)
            });
        }
        Ok(())
    })();
    let _ = fs::remove_file(log_path);
    result
}

#[cfg(target_os = "windows")]
fn launch_status_console(script: &str, working_dir: &Path) -> Result<u32, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, PROCESS_INFORMATION, STARTUPINFOW, CREATE_NEW_CONSOLE,
    };

    let encoded = crate::common::windows::encode_powershell_base64(script);
    let command = format!("powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -NoExit -EncodedCommand {}", encoded);
    let mut command: Vec<u16> = std::ffi::OsStr::new(&command).encode_wide().chain(Some(0)).collect();
    let directory: Vec<u16> = working_dir.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        let mut startup: STARTUPINFOW = std::mem::zeroed();
        startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        let mut process: PROCESS_INFORMATION = std::mem::zeroed();
        let created = CreateProcessW(
            std::ptr::null(), command.as_mut_ptr(), std::ptr::null(), std::ptr::null(), 0,
            CREATE_NEW_CONSOLE, std::ptr::null(), directory.as_ptr(), &startup, &mut process,
        );
        if created == 0 {
            return Err(format!("启动 llmtrim 状态窗口失败: {}", std::io::Error::last_os_error()));
        }
        CloseHandle(process.hThread);
        CloseHandle(process.hProcess);
        Ok(process.dwProcessId)
    }
}

#[cfg(not(target_os = "windows"))]
fn launch_status_console(script: &str, working_dir: &Path) -> Result<u32, String> {
    crate::common::windows::launch_visible_powershell(script, Some(working_dir))
}

pub(super) fn build_llmtrim_log_script(executable: &Path, log_path: &Path) -> String {
    let executable = executable.to_string_lossy().replace('\'', "''");
    let log_path = log_path.to_string_lossy().replace('\'', "''");
    format!(
        "$host.UI.RawUI.WindowTitle = 'llmtrim logs'; [Console]::InputEncoding = [System.Text.Encoding]::UTF8; [Console]::OutputEncoding = [System.Text.Encoding]::UTF8; $OutputEncoding = [System.Text.Encoding]::UTF8; $logPath = '{}'; if (Test-Path -LiteralPath $logPath -PathType Leaf) {{ Write-Host \"llmtrim 日志: $logPath\"; Get-Content -LiteralPath $logPath -Encoding UTF8 -Tail 200 -Wait }} else {{ Write-Host \"llmtrim 尚未生成守护进程日志: $logPath\"; & '{}' status }}",
        log_path, executable,
    )
}

pub struct LlmtrimService;

impl LlmtrimService {
    pub fn get_install_dir() -> PathBuf {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                return parent.join("llmtrim");
            }
        }
        PathBuf::from("llmtrim")
    }

    pub fn get_state_dir() -> PathBuf {
        get_user_profile_dir().join(".llmtrim")
    }

    pub fn get_runtime_state_dir() -> PathBuf {
        if let Some(home) = std::env::var_os("LLMTRIM_HOME") {
            return PathBuf::from(home);
        }
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_else(get_user_profile_dir)
            .join(".llmtrim")
    }

    pub fn get_log_path() -> PathBuf {
        Self::get_runtime_state_dir().join("serve.log")
    }

    pub fn get_executable_path(configured_path: &str) -> PathBuf {
        let trimmed = configured_path.trim();
        if !trimmed.is_empty() {
            PathBuf::from(trimmed)
        } else {
            Self::get_install_dir().join(LLMTRIM_EXECUTABLE_NAME)
        }
    }

    pub fn is_port_open() -> bool {
        if let Ok(addr) = "127.0.0.1:43117".parse() {
            TcpStream::connect_timeout(&addr, Duration::from_millis(50)).is_ok()
        } else {
            false
        }
    }

    pub fn owns_listener(configured_path: &str) -> bool {
        let executable = Self::get_executable_path(configured_path);
        listener_pid_for_executable(&executable, 43117).is_some()
    }

    pub fn is_running(configured_path: &str) -> bool {
        Self::is_port_open() && Self::owns_listener(configured_path)
    }

    pub fn query_binary_version(exe_path: &Path) -> String {
        if !exe_path.exists() || !exe_path.is_file() {
            return String::new();
        }
        let mut cmd = Command::new(exe_path);
        cmd.arg("--version");
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        if let Ok(output) = cmd.output() {
            let text = String::from_utf8_lossy(&output.stdout);
            for line in text.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Some(v) = trimmed.strip_prefix("llmtrim ") {
                    return v.trim().to_string();
                }
                return trimmed.to_string();
            }
        }
        String::new()
    }

    pub fn get_version(install_dir: &Path, exe_path: &Path) -> String {
        let version_file = install_dir.join(".llmtrim-version");
        if let Ok(v) = fs::read_to_string(&version_file) {
            let trimmed = v.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
        let queried = Self::query_binary_version(exe_path);
        if !queried.is_empty() {
            let _ = fs::write(&version_file, format!("{}\n", queried));
            return queried;
        }
        String::new()
    }

    pub fn calculate_activation_state(snapshot: &LlmtrimActivationSnapshot) -> String {
        if snapshot.state_read_failed || snapshot.state_needs_attention {
            return "attention".to_string();
        }
        if snapshot.running {
            if snapshot.windows_configured {
                return "running".to_string();
            }
            return "attention".to_string();
        }
        if snapshot.tray_running
            || snapshot.process_id != 0
            || snapshot.port_open
            || snapshot.desired_running
            || snapshot.recorded_running
        {
            return "attention".to_string();
        }
        if snapshot.installed {
            if snapshot.configured_path.trim().is_empty() || !snapshot.configured_path_available {
                return "attention".to_string();
            }
            return "installed_stopped".to_string();
        }
        if snapshot.directory_exists || !snapshot.configured_path.trim().is_empty() {
            return "attention".to_string();
        }
        "not_installed".to_string()
    }

    pub fn sync_extra_hosts(upstream_base_url: &str) -> Result<(), String> {
        let install_dir = Self::get_install_dir();
        sync_managed_llmtrim_config(upstream_base_url, Some(&install_dir))
    }

    pub fn get_status(configured_path: &str) -> LlmtrimStatusResponse {
        let exe_path = Self::get_executable_path(configured_path);
        let install_dir = Self::get_install_dir();
        let state_dir = Self::get_state_dir();

        let configured_path_available = exe_path.exists() && exe_path.is_file();
        let default_exe = install_dir.join(LLMTRIM_EXECUTABLE_NAME);
        let installed = configured_path_available || (default_exe.exists() && default_exe.is_file());
        let directory_exists = install_dir.is_dir();
        let state_dir_exists = state_dir.is_dir();

        let port_listening = Self::is_port_open();
        let listener_pid = listener_pid_for_executable(&exe_path, 43117);
        let pid = listener_pid.or_else(|| recorded_daemon_pid(&state_dir, &exe_path)).unwrap_or(0);
        let running = port_listening && listener_pid.is_some();

        let tray_path = exe_path.with_file_name(LLMTRIM_TRAY_EXECUTABLE_NAME);
        let tray_pid = find_all_process_ids(LLMTRIM_TRAY_EXECUTABLE_NAME)
            .into_iter()
            .find(|pid| process_matches_executable(*pid, &tray_path))
            .unwrap_or(0);
        let tray_running = tray_pid != 0;

        let state_install_dir = exe_path.parent().unwrap_or(&install_dir);
        let (state, state_read_err) = match read_managed_tool_state(state_install_dir) {
            Ok(s) => (s, false),
            Err(_) => (ManagedToolState::default(), true),
        };

        let (windows_configured, config_msg) = verify_llmtrim_windows_setup();
        let version = if installed {
            Self::get_version(&install_dir, &exe_path)
        } else {
            String::new()
        };

        let residual = query_llmtrim_residual(
            configured_path,
            &install_dir,
            running,
            tray_running,
            directory_exists,
            state_dir_exists,
        )
        .unwrap_or(false);

        let snapshot = LlmtrimActivationSnapshot {
            installed,
            directory_exists,
            configured_path: exe_path.to_string_lossy().to_string(),
            configured_path_available,
            running,
            tray_running,
            process_id: pid,
            port_open: port_listening,
            desired_running: state.desired_running,
            recorded_running: state.running,
            windows_configured,
            state_read_failed: state_read_err,
            state_needs_attention: state.metadata.contains_key("attention_needed"),
        };

        let activation_state = Self::calculate_activation_state(&snapshot);

        let mut message = if running {
            "llmtrim 守护进程运行正常，已在 43117 端口提供本地优化网关。".to_string()
        } else if installed {
            "llmtrim 已安装，守护进程当前处于停止状态。".to_string()
        } else {
            "llmtrim 尚未安装。".to_string()
        };

        if activation_state == "attention" {
            if running && !windows_configured {
                message = format!("daemon 已运行，但 Windows 接管未通过校验: {}", config_msg);
            } else if !running && tray_running {
                message = "daemon 已停止但 tray 仍在运行，请点击停止清理。".to_string();
            } else if !running && port_listening {
                message = "43117 端口仍被占用，当前配置路径的 daemon 未确认就绪。".to_string();
            } else if let Some(last_err) = state.metadata.get("last_error") {
                message = format!("上次 llmtrim 操作需要处理: {}", last_err);
            } else if !configured_path_available {
                message = "已记录的 llmtrim.exe 路径不可用，请重新安装。".to_string();
            } else if !running && (state.desired_running || state.running) {
                message = "状态账本要求 llmtrim 运行，但 daemon 尚未就绪。".to_string();
            }
        }

        LlmtrimStatusResponse {
            path: exe_path.to_string_lossy().to_string(),
            version,
            running,
            desired_running: state.desired_running,
            route_selected: false,
            activation_state,
            installed,
            configured: windows_configured,
            directory_exists,
            state_dir_exists,
            tray_running,
            process_id: pid,
            tray_process_id: tray_pid,
            residual,
            port: format!("127.0.0.1:{}", LLMTRIM_PORT),
            message,
        }
    }

    pub async fn fetch_releases(
        page: usize,
        proxy_url: Option<&str>,
    ) -> Result<LlmtrimReleaseListResponse, String> {
        fetch_llmtrim_releases(page, proxy_url).await
    }

    pub async fn install(
        tag_name: Option<String>,
        upstream_url: &str,
        proxy_url: Option<&str>,
    ) -> Result<(), String> {
        let install_dir = Self::get_install_dir();
        let _ = fs::create_dir_all(&install_dir);

        // 1. 停止现有进程
        Self::stop("")?;
        set_managed_tool_desired_running(&install_dir, true)?;

        // 2. 下载并校验解压
        let version = download_and_install_llmtrim(tag_name.as_deref(), &install_dir, proxy_url).await?;

        let exe = install_dir.join(LLMTRIM_EXECUTABLE_NAME).to_string_lossy().to_string();
        let upstream_url = upstream_url.to_string();
        tokio::task::spawn_blocking(move || Self::start(&exe, &upstream_url))
            .await
            .map_err(|e| format!("llmtrim 安装后的启动任务失败: {}", e))?
            .map_err(|e| format!("llmtrim {} 已下载，但启动失败: {}", version, e))?;

        info!("llmtrim {} 安装并启动完成", version);
        Ok(())
    }

    pub fn start(configured_path: &str, upstream_url: &str) -> Result<(), String> {
        let exe = Self::get_executable_path(configured_path);
        if !exe.exists() {
            return Err("llmtrim.exe 不存在，请先安装 llmtrim".to_string());
        }

        let install_dir = exe.parent().unwrap_or_else(|| Path::new("")).to_path_buf();

        let start_result = (|| {
            if Self::is_port_open() && !Self::is_running(configured_path) {
                return Err("43117 端口已被其他程序或其他路径的 llmtrim 占用".to_string());
            }
            if !upstream_url.trim().is_empty() {
                sync_managed_llmtrim_config(upstream_url, Some(&install_dir))?;
            }
            run_llmtrim_setup(&exe)?;
            let deadline = Instant::now() + Duration::from_secs(8);
            while Instant::now() < deadline {
                if Self::is_running(configured_path) {
                    let (configured, message) = verify_llmtrim_windows_setup();
                    return if configured {
                        Ok(())
                    } else {
                        Err(format!("llmtrim daemon 已启动，但 Windows 配置未通过校验: {}", message))
                    };
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            Err("llmtrim setup 已完成，但当前可执行文件未在 43117 端口就绪".to_string())
        })();

        invalidate_process_cache();
        let running = Self::is_running(configured_path);
        let start_result = start_result.and_then(|()| {
            if running {
                Ok(())
            } else {
                Err("llmtrim 在完成启动检查后停止运行".to_string())
            }
        });
        let state = ManagedToolState {
            desired_running: true,
            running,
            metadata: Default::default(),
        };
        let _ = write_managed_tool_state(&install_dir, &state);

        if let Err(err) = start_result {
            let _ = mark_managed_tool_attention(&install_dir, &err);
            warn!("llmtrim 启动失败: {}", err);
            return Err(err);
        }

        info!("llmtrim 已成功启动并确认 43117 端口监听");
        Ok(())
    }

    pub fn stop(configured_path: &str) -> Result<(), String> {
        let exe = Self::get_executable_path(configured_path);
        if Self::is_port_open() && !Self::is_running(configured_path) {
            return Err("43117 端口由其他程序或其他路径的 llmtrim 占用，未停止该进程".to_string());
        }
        if let Ok(content) = fs::read(Self::get_state_dir().join("serve.pid")) {
            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&content) {
                if let Some(pid) = value.get("pid").and_then(|pid| pid.as_u64()).and_then(|pid| u32::try_from(pid).ok()) {
                    if is_pid_alive(pid) && !process_matches_executable(pid, &exe) {
                        return Err("llmtrim 状态文件指向其他路径的进程，未停止该进程".to_string());
                    }
                }
            }
        }
        if exe.exists() {
            // 先尝试关闭自启动
            let mut off_cmd = Command::new(&exe);
            off_cmd.args(["autostart", "--off"]);
            #[cfg(target_os = "windows")]
            {
                use std::os::windows::process::CommandExt;
                off_cmd.creation_flags(0x08000000);
            }
            let _ = off_cmd.output();

            // 执行 stop
            let mut stop_cmd = Command::new(&exe);
            stop_cmd.arg("stop");
            #[cfg(target_os = "windows")]
            {
                use std::os::windows::process::CommandExt;
                stop_cmd.creation_flags(0x08000000);
            }
            let _ = stop_cmd.output();
        }

        // Only terminate processes belonging to this installation.
        invalidate_process_cache();
        for (name, path) in [
            (LLMTRIM_EXECUTABLE_NAME, exe.clone()),
            (LLMTRIM_TRAY_EXECUTABLE_NAME, exe.with_file_name(LLMTRIM_TRAY_EXECUTABLE_NAME)),
        ] {
            for pid in find_all_process_ids(name) {
                if process_matches_executable(pid, &path) {
                    let _ = kill_process_by_pid(pid);
                }
            }
        }

        // 清理注册表自启动
        let _ = remove_llmtrim_autostart();

        // 清理用户环境变量并广播 WM_SETTINGCHANGE
        let _ = clear_llmtrim_user_environment();

        // 更新状态账本
        let install_dir = if let Some(parent) = exe.parent() {
            parent.to_path_buf()
        } else {
            Self::get_install_dir()
        };
        if install_dir.is_dir() {
            let state = ManagedToolState {
                desired_running: false,
                running: false,
                metadata: Default::default(),
            };
            let _ = write_managed_tool_state(&install_dir, &state);
        }

        info!("llmtrim 和 llmtrim-tray 已停止，相关环境变量与自启动已清理");
        Ok(())
    }

    pub fn uninstall(configured_path: &str) -> Result<Vec<String>, String> {
        let mut warnings = Vec::new();

        // 1. 停止运行中的进程并清理环境
        Self::stop(configured_path)?;

        // 2. 删除统计数据库（包括历史默认路径与受管目录中的 tracking.db）
        let install_dir = Self::get_install_dir();
        if let Ok(db) = tracking_db_path(&install_dir) {
            let _ = remove_tracking_database(&db);
        }
        let _ = remove_default_tracking_database();

        // 3. 删除受管安装目录
        if install_dir.exists() {
            if let Err(e) = fs::remove_dir_all(&install_dir) {
                warnings.push(format!("删除安装目录失败: {}", e));
            }
        }

        // 4. 移除本地 CA 信任
        if let Err(e) = remove_llmtrim_user_trust() {
            warnings.push(format!("清理 CA 信任状态: {}", e));
        }

        // 5. 删除用户状态目录 %USERPROFILE%\.llmtrim
        let state_dir = Self::get_state_dir();
        if state_dir.exists() {
            if let Err(e) = fs::remove_dir_all(&state_dir) {
                warnings.push(format!("删除状态目录失败: {}", e));
            }
        }

        // 6. 删除受管配置目录 %USERPROFILE%\.config\llmtrim
        if is_config_managed() {
            if let Err(e) = remove_managed_config_dir() {
                warnings.push(format!("删除受管配置目录失败: {}", e));
            }
        }

        info!("llmtrim 卸载与清理完成 (warnings={:?})", warnings);
        Ok(warnings)
    }

    pub fn show_logs(configured_path: &str) -> Result<u32, String> {
        let executable = Self::get_executable_path(configured_path);
        if !executable.is_file() {
            return Err(format!("llmtrim 可执行文件不存在: {}", executable.display()));
        }
        let script = build_llmtrim_log_script(&executable, &Self::get_log_path());
        launch_status_console(&script, executable.parent().unwrap_or_else(|| Path::new(".")))
    }
}
