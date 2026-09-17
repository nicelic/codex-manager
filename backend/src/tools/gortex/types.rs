use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GortexStatusResponse {
    pub path: String,
    pub managed_installed: bool,
    pub managed_root_exists: bool,
    pub version: String,
    pub installing: bool,
    pub installed: bool,
    pub running: bool,
    pub any_process_running: bool,
    pub unmanaged_process_running: bool,
    pub integration_present: bool,
    pub process_id: u32,
    pub activation_state: String,

    pub codex_available: bool,
    pub codex_configured: bool,
    pub codex_complete: bool,
    pub claude_available: bool,
    pub claude_configured: bool,
    pub claude_complete: bool,
    pub cursor_available: bool,
    pub cursor_configured: bool,
    pub cursor_complete: bool,
    pub copilot_available: bool,
    pub copilot_configured: bool,
    pub copilot_complete: bool,
    pub open_code_available: bool,
    pub open_code_configured: bool,
    pub open_code_complete: bool,
    pub antigravity_available: bool,
    pub antigravity_configured: bool,
    pub antigravity_complete: bool,
    pub gemini_available: bool,
    pub gemini_configured: bool,
    pub gemini_complete: bool,

    pub codex_prompt: bool,
    pub codex_prompt_complete: bool,
    pub claude_prompt: bool,
    pub claude_prompt_complete: bool,
    pub cursor_prompt: bool,
    pub cursor_prompt_complete: bool,
    pub copilot_prompt: bool,
    pub copilot_prompt_complete: bool,
    pub open_code_prompt: bool,
    pub open_code_prompt_complete: bool,
    pub antigravity_prompt: bool,
    pub antigravity_prompt_complete: bool,
    pub gemini_prompt: bool,
    pub gemini_prompt_complete: bool,

    pub codex_hook: bool,
    pub codex_hook_complete: bool,
    pub claude_hook: bool,
    pub claude_hook_complete: bool,
    pub copilot_hook: bool,
    pub copilot_hook_complete: bool,
    pub open_code_hook: bool,
    pub open_code_hook_complete: bool,
    pub antigravity_hook: bool,
    pub antigravity_hook_complete: bool,
    pub gemini_hook: bool,
    pub gemini_hook_complete: bool,

    pub user_path: bool,
    pub system_path: bool,
    pub codex_trust_status: String,
    pub codex_trust_required: bool,
    pub codex_trust_notice: String,
    pub codex_trust_steps: Vec<String>,

    pub tracked_projects: Vec<String>,
    pub project_mcp_enabled: bool,
    pub project_mcp_projects: Vec<String>,
    pub default_project: String,
    pub status_known: bool,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct GortexInstallRequest {
    pub tag_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GortexTrackRequest {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GortexReleaseOption {
    pub tag_name: String,
    pub name: String,
    pub published_at: String,
    pub prerelease: bool,
    pub available: bool,
    pub asset_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GortexReleaseListResponse {
    pub releases: Vec<GortexReleaseOption>,
    pub page: usize,
    pub per_page: usize,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GortexDiagnosticsResponse {
    pub doctor_ok: bool,
    pub doctor_output: String,
    pub doctor_error: String,
    pub status_ok: bool,
    pub status_output: String,
    pub status_error: String,
}
