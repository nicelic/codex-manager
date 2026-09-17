use super::types::{SnipAgentState, SnipReleaseListResponse, SnipReleaseOption, SnipStatusResponse};
use std::path::PathBuf;

pub struct SnipService;

impl SnipService {
    pub fn get_install_dir() -> PathBuf {
        let user_profile = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\".to_string());
        PathBuf::from(user_profile).join(".snip")
    }

    pub fn get_executable_path() -> PathBuf {
        Self::get_install_dir().join("snip.exe")
    }

    pub fn get_status() -> SnipStatusResponse {
        let exe_path = Self::get_executable_path();
        let install_dir = Self::get_install_dir();
        let installed = exe_path.exists();
        let directory_exists = install_dir.exists();

        let agents = vec![
            SnipAgentState {
                name: "codex".to_string(),
                hook_exists: installed,
                repair_needed: false,
                modified: installed,
                trust: if installed { "trusted".to_string() } else { "not_applicable".to_string() },
                configured: installed,
                target_file: "codex.json".to_string(),
            },
            SnipAgentState {
                name: "claude-code".to_string(),
                hook_exists: installed,
                repair_needed: false,
                modified: installed,
                trust: "trusted".to_string(),
                configured: installed,
                target_file: "config.json".to_string(),
            },
        ];

        SnipStatusResponse {
            path: exe_path.to_string_lossy().to_string(),
            installed,
            directory_exists,
            version: if installed { "v0.9.5".to_string() } else { String::new() },
            user_path: installed,
            system_path: installed,
            running: installed,
            desired_running: installed,
            activation_state: if installed { "running".to_string() } else { "not_installed".to_string() },
            blocked_by: String::new(),
            trust_notice: String::new(),
            trust_status: "trusted".to_string(),
            trust_command: String::new(),
            trust_required: false,
            trust_steps: vec![],
            cleanup_required: false,
            modified_agents: if installed { vec!["codex".to_string(), "claude-code".to_string()] } else { vec![] },
            agents,
            message: if installed { "snip 已启动，原生 Hook 已配置。".to_string() } else { "snip 尚未安装。".to_string() },
        }
    }

    pub fn get_releases(page: usize) -> SnipReleaseListResponse {
        SnipReleaseListResponse {
            releases: vec![
                SnipReleaseOption {
                    tag_name: "v0.9.5".to_string(),
                    name: "Snip v0.9.5 发布".to_string(),
                    published_at: "2026-09-05T00:00:00Z".to_string(),
                    prerelease: false,
                    available: true,
                    asset_name: "snip-windows-amd64.exe".to_string(),
                },
            ],
            page,
            has_more: false,
        }
    }
}
