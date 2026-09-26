use std::fs;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use tracing::{info, warn};

use super::config::{
    is_config_managed, remove_default_tracking_database,
    remove_managed_config_dir, remove_tracking_database, sync_managed_llmtrim_config, tracking_db_path,
};
use super::ownership::{
    mark_managed_tool_attention, read_managed_tool_state, write_managed_tool_state,
    ManagedToolState,
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
    find_process_id, is_process_running, kill_process_by_name,
};
use crate::common::windows::{get_user_profile_dir, open_log_viewer};

pub const LLMTRIM_PORT: &str = "43117";
pub const LLMTRIM_EXECUTABLE_NAME: &str = "llmtrim.exe";
pub const LLMTRIM_TRAY_EXECUTABLE_NAME: &str = "llmtrim-tray.exe";

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

        let proc_running = is_process_running(LLMTRIM_EXECUTABLE_NAME);
        let port_listening = Self::is_port_open();
        let pid = find_process_id(LLMTRIM_EXECUTABLE_NAME).unwrap_or(0);
        let running = (proc_running || pid != 0) && port_listening;

        let tray_running = is_process_running(LLMTRIM_TRAY_EXECUTABLE_NAME);
        let tray_pid = find_process_id(LLMTRIM_TRAY_EXECUTABLE_NAME).unwrap_or(0);

        let (state, state_read_err) = match read_managed_tool_state(&install_dir) {
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
            configured_path: configured_path.to_string(),
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
            } else if configured_path.is_empty() {
                message = "未记录可启动的 llmtrim.exe 路径，请重新安装。".to_string();
            } else if !configured_path_available {
                message = "已记录的 llmtrim.exe 路径不可用，请重新安装。".to_string();
            } else if !running && (state.desired_running || state.running) {
                message = "状态账本要求 llmtrim 运行，但 daemon 尚未就绪。".to_string();
            } else if let Some(last_err) = state.metadata.get("last_error") {
                message = format!("上次 llmtrim 操作需要处理: {}", last_err);
            }
        }

        LlmtrimStatusResponse {
            path: exe_path.to_string_lossy().to_string(),
            version,
            running,
            desired_running: state.desired_running,
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
        let _ = Self::stop("");

        // 2. 下载并校验解压
        let version = download_and_install_llmtrim(tag_name.as_deref(), &install_dir, proxy_url).await?;

        // 3. 同步 config.toml（包含 extra_hosts 和 db_path）
        if !upstream_url.trim().is_empty() {
            let _ = sync_managed_llmtrim_config(upstream_url, Some(&install_dir));
        }

        // 4. 执行 setup 初始化 CA 与自启动
        let exe = install_dir.join(LLMTRIM_EXECUTABLE_NAME);
        if exe.exists() {
            let mut setup_cmd = Command::new(&exe);
            setup_cmd.args(["setup", "--non-interactive"]);
            #[cfg(target_os = "windows")]
            {
                use std::os::windows::process::CommandExt;
                setup_cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
            }
            let _ = setup_cmd.output();
        }

        // 5. 等待 daemon 端口监听
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline {
            if Self::is_port_open() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }

        // 6. 记录受管状态
        let port_ok = Self::is_port_open();
        let state = ManagedToolState {
            desired_running: true,
            running: port_ok,
            metadata: Default::default(),
        };
        let _ = write_managed_tool_state(&install_dir, &state);

        info!("llmtrim {} 安装并启动流程已完成 (port_open={})", version, port_ok);
        Ok(())
    }

    pub fn start(configured_path: &str, upstream_url: &str) -> Result<(), String> {
        let exe = Self::get_executable_path(configured_path);
        if !exe.exists() {
            return Err("llmtrim.exe 不存在，请先安装 llmtrim".to_string());
        }

        let install_dir = exe.parent().unwrap_or_else(|| Path::new("")).to_path_buf();

        // 同步配置
        if !upstream_url.trim().is_empty() {
            let _ = sync_managed_llmtrim_config(upstream_url, Some(&install_dir));
        }

        // 执行 setup 恢复环境并启动 daemon
        let mut cmd = Command::new(&exe);
        cmd.args(["setup", "--non-interactive"]);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }

        let output = cmd.output().map_err(|e| format!("启动 llmtrim setup 失败: {}", e))?;
        if !output.status.success() {
            let err_msg = String::from_utf8_lossy(&output.stderr);
            warn!("llmtrim setup stderr: {}", err_msg);
        }

        // 等待端口监听
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline {
            if Self::is_port_open() {
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }

        let running = Self::is_port_open();
        let state = ManagedToolState {
            desired_running: true,
            running,
            metadata: Default::default(),
        };
        let _ = write_managed_tool_state(&install_dir, &state);

        if !running {
            let _ = mark_managed_tool_attention(&install_dir, "llmtrim 已执行 setup，但 43117 端口尚未确认监听");
            return Err("llmtrim 已启动，但 43117 端口尚未确认就绪".to_string());
        }

        info!("llmtrim 已成功启动并确认 43117 端口监听");
        Ok(())
    }

    pub fn stop(configured_path: &str) -> Result<(), String> {
        let exe = Self::get_executable_path(configured_path);
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

        // 终止进程树
        let _ = kill_process_by_name(LLMTRIM_EXECUTABLE_NAME);
        let _ = kill_process_by_name(LLMTRIM_TRAY_EXECUTABLE_NAME);

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
        let _ = Self::stop(configured_path);

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

    pub fn show_logs() -> Result<u32, String> {
        let state_dir = Self::get_state_dir();
        let serve_log = state_dir.join("serve.log");
        let fallback_log = state_dir.join("llmtrim.log");

        let log_path = if serve_log.exists() {
            serve_log
        } else {
            if !fallback_log.exists() {
                let _ = fs::create_dir_all(&state_dir);
                let _ = fs::write(&fallback_log, "llmtrim 日志监视器已就绪...\r\n");
            }
            fallback_log
        };

        open_log_viewer("LLMTrim Console Logs", &log_path)
    }
}
