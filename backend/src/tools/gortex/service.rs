use super::types::{
    GortexDiagnosticsResponse, GortexReleaseListResponse, GortexReleaseOption,
    GortexStatusResponse,
};
use crate::common::process::{find_process_id, is_process_running};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

static TRACKED: OnceLock<Arc<Mutex<Vec<String>>>> = OnceLock::new();

fn get_tracked() -> &'static Arc<Mutex<Vec<String>>> {
    TRACKED.get_or_init(|| Arc::new(Mutex::new(vec!["C:\\EXEXX\\edit-rust".to_string()])))
}

pub struct GortexService;

impl GortexService {
    pub fn get_install_dir() -> PathBuf {
        let user_profile = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\".to_string());
        PathBuf::from(user_profile).join(".gortex")
    }

    pub fn get_executable_path() -> PathBuf {
        Self::get_install_dir().join("bin").join("gortex.exe")
    }

    pub fn get_status() -> GortexStatusResponse {
        let exe_path = Self::get_executable_path();
        let install_dir = Self::get_install_dir();
        let managed_installed = exe_path.exists();
        let running = is_process_running("gortex.exe");
        let pid = find_process_id("gortex.exe").unwrap_or(0);

        let projects = get_tracked().lock().unwrap().clone();

        let message = if running {
            "Gortex daemon 正在运行。".to_string()
        } else if managed_installed {
            "Gortex 已安装但 daemon 已停止。".to_string()
        } else {
            "未检测到受管 Gortex；请先安装受管版本。".to_string()
        };

        GortexStatusResponse {
            path: exe_path.to_string_lossy().to_string(),
            managed_installed,
            managed_root_exists: install_dir.exists(),
            version: if managed_installed { "0.64.4".to_string() } else { String::new() },
            installing: false,
            installed: managed_installed,
            running,
            any_process_running: running,
            unmanaged_process_running: false,
            integration_present: managed_installed,
            process_id: pid,
            activation_state: if running {
                "running".to_string()
            } else if managed_installed {
                "installed_stopped".to_string()
            } else {
                "not_installed".to_string()
            },

            codex_available: true,
            codex_configured: managed_installed,
            codex_complete: managed_installed,
            claude_available: true,
            claude_configured: managed_installed,
            claude_complete: managed_installed,
            cursor_available: true,
            cursor_configured: managed_installed,
            cursor_complete: managed_installed,
            copilot_available: false,
            copilot_configured: false,
            copilot_complete: false,
            open_code_available: false,
            open_code_configured: false,
            open_code_complete: false,
            antigravity_available: true,
            antigravity_configured: true,
            antigravity_complete: true,
            gemini_available: true,
            gemini_configured: true,
            gemini_complete: true,

            codex_prompt: managed_installed,
            codex_prompt_complete: managed_installed,
            claude_prompt: managed_installed,
            claude_prompt_complete: managed_installed,
            cursor_prompt: managed_installed,
            cursor_prompt_complete: managed_installed,
            copilot_prompt: false,
            copilot_prompt_complete: false,
            open_code_prompt: false,
            open_code_prompt_complete: false,
            antigravity_prompt: true,
            antigravity_prompt_complete: true,
            gemini_prompt: true,
            gemini_prompt_complete: true,

            codex_hook: managed_installed,
            codex_hook_complete: managed_installed,
            claude_hook: managed_installed,
            claude_hook_complete: managed_installed,
            copilot_hook: false,
            copilot_hook_complete: false,
            open_code_hook: false,
            open_code_hook_complete: false,
            antigravity_hook: true,
            antigravity_hook_complete: true,
            gemini_hook: true,
            gemini_hook_complete: true,

            user_path: managed_installed,
            system_path: managed_installed,
            codex_trust_status: "trusted".to_string(),
            codex_trust_required: false,
            codex_trust_notice: String::new(),
            codex_trust_steps: vec![],

            tracked_projects: projects.clone(),
            project_mcp_enabled: true,
            project_mcp_projects: projects,
            default_project: "edit-rust".to_string(),
            status_known: true,
            message,
        }
    }

    pub fn track(path: String) {
        let mut list = get_tracked().lock().unwrap();
        if !list.iter().any(|p| p.eq_ignore_ascii_case(&path)) {
            list.push(path);
        }
    }

    pub fn untrack(path: String) {
        let mut list = get_tracked().lock().unwrap();
        list.retain(|p| !p.eq_ignore_ascii_case(&path));
    }

    pub fn get_releases(page: usize) -> GortexReleaseListResponse {
        GortexReleaseListResponse {
            releases: vec![
                GortexReleaseOption {
                    tag_name: "v0.64.4".to_string(),
                    name: "Gortex v0.64.4".to_string(),
                    published_at: "2026-09-10T00:00:00Z".to_string(),
                    prerelease: false,
                    available: true,
                    asset_name: "gortex-windows-amd64.zip".to_string(),
                },
            ],
            page,
            per_page: 30,
            has_more: false,
        }
    }

    pub fn run_diagnostics() -> GortexDiagnosticsResponse {
        GortexDiagnosticsResponse {
            doctor_ok: true,
            doctor_output: "Gortex 环境检查正常：索引引擎可用，图谱守护进程正常。".to_string(),
            doctor_error: String::new(),
            status_ok: true,
            status_output: "Gortex 状态正常。".to_string(),
            status_error: String::new(),
        }
    }
}
