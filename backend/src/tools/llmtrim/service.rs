use super::types::{LlmtrimReleaseListResponse, LlmtrimReleaseOption, LlmtrimStatusResponse};
use crate::common::downloader::{download_and_extract_zip, fetch_github_releases};
use crate::common::process::{find_process_id, is_process_running};
use crate::common::windows::{get_user_profile_dir, open_log_viewer};
use std::fs;
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Duration;
use tracing::info;

const LLMTRIM_REPO: &str = "fkiene/llmtrim";
const LLMTRIM_PORT: &str = "43117";

pub struct LlmtrimService;

impl LlmtrimService {
    pub fn get_install_dir() -> PathBuf {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                return parent.join("llmtrim");
            }
        }
        get_user_profile_dir().join(".llmtrim")
    }

    pub fn get_state_dir() -> PathBuf {
        get_user_profile_dir().join(".llmtrim")
    }

    pub fn get_executable_path() -> PathBuf {
        Self::get_install_dir().join("llmtrim.exe")
    }

    pub fn is_port_open() -> bool {
        if let Ok(addr) = "127.0.0.1:43117".parse() {
            TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok()
        } else {
            false
        }
    }

    pub fn get_status(configured_path: &str) -> LlmtrimStatusResponse {
        let exe_path = if !configured_path.is_empty() {
            PathBuf::from(configured_path)
        } else {
            Self::get_executable_path()
        };

        let install_dir = Self::get_install_dir();
        let state_dir = Self::get_state_dir();
        let installed = exe_path.exists() && exe_path.is_file();
        let directory_exists = install_dir.is_dir();
        let state_dir_exists = state_dir.is_dir();

        let proc_running = is_process_running("llmtrim.exe");
        let port_listening = Self::is_port_open();
        let running = proc_running || port_listening;

        let pid = find_process_id("llmtrim.exe").unwrap_or(0);
        let tray_running = is_process_running("llmtrim-tray.exe");
        let tray_pid = find_process_id("llmtrim-tray.exe").unwrap_or(0);

        let version = if installed {
            let version_file = install_dir.join(".llmtrim-version");
            fs::read_to_string(version_file).unwrap_or_else(|_| "v0.8.2".to_string()).trim().to_string()
        } else {
            String::new()
        };

        let activation_state = if running {
            "running".to_string()
        } else if installed {
            "installed_stopped".to_string()
        } else {
            "not_installed".to_string()
        };

        let message = if running {
            "llmtrim 守护进程运行正常，已在 43117 端口提供本地优化网关。".to_string()
        } else if installed {
            "llmtrim 已安装，守护进程当前处于停止状态。".to_string()
        } else {
            "llmtrim 尚未安装。".to_string()
        };

        LlmtrimStatusResponse {
            path: exe_path.to_string_lossy().to_string(),
            version,
            running,
            desired_running: running,
            activation_state,
            installed,
            configured: installed,
            directory_exists,
            state_dir_exists,
            tray_running,
            process_id: pid,
            tray_process_id: tray_pid,
            residual: false,
            port: LLMTRIM_PORT.to_string(),
            message,
        }
    }

    pub async fn fetch_releases(page: usize, proxy_url: Option<&str>) -> Result<LlmtrimReleaseListResponse, String> {
        let releases = fetch_github_releases(LLMTRIM_REPO, page, 5, proxy_url).await?;
        let options = releases
            .into_iter()
            .map(|rel| {
                let asset = rel.assets.iter().find(|a| a.name.ends_with(".zip") && (a.name.contains("windows") || a.name.contains("amd64")));
                let (available, asset_name) = match asset {
                    Some(a) => (true, a.name.clone()),
                    None => (false, String::new()),
                };

                LlmtrimReleaseOption {
                    tag_name: rel.tag_name,
                    name: rel.name.unwrap_or_default(),
                    published_at: rel.published_at.unwrap_or_default(),
                    prerelease: rel.prerelease.unwrap_or(false),
                    available,
                    asset_name,
                }
            })
            .collect();

        Ok(LlmtrimReleaseListResponse {
            releases: options,
            page,
            has_more: true,
        })
    }

    pub async fn install(tag_name: Option<String>, proxy_url: Option<&str>) -> Result<(), String> {
        let install_dir = Self::get_install_dir();
        fs::create_dir_all(&install_dir).map_err(|e| format!("创建 llmtrim 目录失败: {}", e))?;

        let releases = fetch_github_releases(LLMTRIM_REPO, 1, 10, proxy_url).await?;
        let target_release = if let Some(tag) = &tag_name {
            releases.into_iter().find(|r| r.tag_name == *tag)
        } else {
            releases.into_iter().next()
        }.ok_or_else(|| "未找到匹配的 llmtrim 发布版本".to_string())?;

        let asset = target_release
            .assets
            .iter()
            .find(|a| a.name.ends_with(".zip") && (a.name.contains("windows") || a.name.contains("amd64")))
            .ok_or_else(|| "未找到适用于 Windows 的 llmtrim 资产包".to_string())?;

        info!("Downloading llmtrim from {}", asset.browser_download_url);
        download_and_extract_zip(&asset.browser_download_url, &install_dir, proxy_url).await?;

        let _ = fs::write(install_dir.join(".llmtrim-version"), &target_release.tag_name);
        Ok(())
    }

    pub fn start() -> Result<(), String> {
        let exe = Self::get_executable_path();
        if !exe.exists() {
            return Err("llmtrim.exe 不存在，请先安装 llmtrim".to_string());
        }

        let mut cmd = std::process::Command::new(&exe);
        cmd.arg("setup");

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }

        cmd.spawn().map_err(|e| format!("启动 llmtrim setup 失败: {}", e))?;
        Ok(())
    }

    pub fn stop() -> Result<(), String> {
        let exe = Self::get_executable_path();
        if exe.exists() {
            let _ = std::process::Command::new(&exe).arg("stop").output();
        }

        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/IM", "llmtrim.exe"])
            .output();

        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/IM", "llmtrim-tray.exe"])
            .output();

        Ok(())
    }

    pub fn uninstall() -> Result<(), String> {
        let _ = Self::stop();
        let install_dir = Self::get_install_dir();
        if install_dir.exists() {
            fs::remove_dir_all(&install_dir).map_err(|e| format!("删除 llmtrim 目录失败: {}", e))?;
        }
        Ok(())
    }

    pub fn show_logs() -> Result<(), String> {
        let log_path = Self::get_state_dir().join("llmtrim.log");
        if !log_path.exists() {
            if let Some(parent) = log_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(&log_path, "llmtrim 日志监视器已就绪...\r\n");
        }
        open_log_viewer("LLMTrim Console Logs", &log_path)
    }
}
