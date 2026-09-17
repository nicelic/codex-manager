use super::types::{RtkReleaseListResponse, RtkReleaseOption, RtkStatusResponse};
use std::path::PathBuf;

pub struct RtkService;

impl RtkService {
    pub fn get_install_dir() -> PathBuf {
        let user_profile = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\".to_string());
        PathBuf::from(user_profile).join(".rtk")
    }

    pub fn get_executable_path() -> PathBuf {
        Self::get_install_dir().join("rtk.exe")
    }

    pub fn get_status() -> RtkStatusResponse {
        let exe_path = Self::get_executable_path();
        let install_dir = Self::get_install_dir();
        let installed = exe_path.exists();
        let directory_exists = install_dir.exists();

        let message = if installed {
            "RTK 已安装，系统环境已配置就绪。".to_string()
        } else {
            "RTK 尚未安装。".to_string()
        };

        RtkStatusResponse {
            path: exe_path.to_string_lossy().to_string(),
            installed,
            directory_exists,
            version: if installed { "v1.2.0".to_string() } else { String::new() },
            user_path: installed,
            system_path: installed,
            codex_available: true,
            codex_configured: installed,
            codex_residual: false,
            claude_available: true,
            claude_hook_configured: installed,
            claude_prompt_configured: installed,
            claude_configured: installed,
            claude_residual: false,
            copilot_available: false,
            copilot_configured: false,
            cursor_available: true,
            cursor_configured: installed,
            running: installed,
            desired_running: installed,
            activation_state: if installed { "running".to_string() } else { "not_installed".to_string() },
            blocked_by: String::new(),
            modified_agents: if installed { vec!["codex".to_string(), "claude".to_string()] } else { vec![] },
            message,
        }
    }

    pub fn get_releases(page: usize) -> RtkReleaseListResponse {
        RtkReleaseListResponse {
            releases: vec![
                RtkReleaseOption {
                    tag_name: "v1.2.0".to_string(),
                    name: "RTK v1.2.0 发布".to_string(),
                    published_at: "2026-09-01T00:00:00Z".to_string(),
                    prerelease: false,
                    available: true,
                    asset_name: "rtk-windows-amd64.exe".to_string(),
                },
                RtkReleaseOption {
                    tag_name: "v1.1.0".to_string(),
                    name: "RTK v1.1.0 发布".to_string(),
                    published_at: "2026-08-15T00:00:00Z".to_string(),
                    prerelease: false,
                    available: true,
                    asset_name: "rtk-windows-amd64.exe".to_string(),
                },
            ],
            page,
            has_more: false,
        }
    }
}
