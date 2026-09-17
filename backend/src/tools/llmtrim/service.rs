use super::types::{LlmtrimReleaseListResponse, LlmtrimReleaseOption, LlmtrimStatusResponse};
use crate::common::process::{find_process_id, is_process_running};
use std::path::PathBuf;

pub struct LlmtrimService;

impl LlmtrimService {
    pub fn get_install_dir() -> PathBuf {
        let user_profile = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\".to_string());
        PathBuf::from(user_profile).join(".llmtrim")
    }

    pub fn get_executable_path() -> PathBuf {
        Self::get_install_dir().join("llmtrim.exe")
    }

    pub fn get_status(configured_path: &str) -> LlmtrimStatusResponse {
        let exe_path = if !configured_path.is_empty() {
            PathBuf::from(configured_path)
        } else {
            Self::get_executable_path()
        };

        let install_dir = Self::get_install_dir();
        let installed = exe_path.exists();
        let directory_exists = install_dir.exists();
        let running = is_process_running("llmtrim.exe");
        let pid = find_process_id("llmtrim.exe").unwrap_or(0);
        let tray_running = is_process_running("llmtrim-tray.exe");
        let tray_pid = find_process_id("llmtrim-tray.exe").unwrap_or(0);

        let message = if running {
            "llmtrim 守护进程运行正常。".to_string()
        } else if installed {
            "llmtrim 已安装，守护进程已停止。".to_string()
        } else {
            "llmtrim 尚未安装。".to_string()
        };

        LlmtrimStatusResponse {
            path: exe_path.to_string_lossy().to_string(),
            version: if installed { "v0.8.2".to_string() } else { String::new() },
            running,
            desired_running: running,
            activation_state: if running {
                "running".to_string()
            } else if installed {
                "installed_stopped".to_string()
            } else {
                "not_installed".to_string()
            },
            installed,
            configured: installed,
            directory_exists,
            state_dir_exists: directory_exists,
            tray_running,
            process_id: pid,
            tray_process_id: tray_pid,
            residual: false,
            port: "127.0.0.1:43117".to_string(),
            message,
        }
    }

    pub fn get_releases(page: usize) -> LlmtrimReleaseListResponse {
        LlmtrimReleaseListResponse {
            releases: vec![
                LlmtrimReleaseOption {
                    tag_name: "v0.8.2".to_string(),
                    name: "LLMTrim v0.8.2".to_string(),
                    published_at: "2026-08-20T00:00:00Z".to_string(),
                    prerelease: false,
                    available: true,
                    asset_name: "llmtrim-windows-amd64.exe".to_string(),
                },
            ],
            page,
            has_more: false,
        }
    }
}
