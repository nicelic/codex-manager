use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RtkStatusResponse {
    pub path: String,
    pub installed: bool,
    pub directory_exists: bool,
    pub version: String,
    pub user_path: bool,
    pub system_path: bool,
    pub codex_available: bool,
    pub codex_configured: bool,
    pub codex_residual: bool,
    pub claude_available: bool,
    pub claude_hook_configured: bool,
    pub claude_prompt_configured: bool,
    pub claude_configured: bool,
    pub claude_residual: bool,
    pub copilot_available: bool,
    pub copilot_configured: bool,
    pub cursor_available: bool,
    pub cursor_configured: bool,
    pub running: bool,
    pub desired_running: bool,
    pub activation_state: String,
    pub blocked_by: String,
    pub modified_agents: Vec<String>,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct RtkInstallRequest {
    pub tag_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RtkReleaseOption {
    pub tag_name: String,
    pub name: String,
    pub published_at: String,
    pub prerelease: bool,
    pub available: bool,
    pub asset_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RtkReleaseListResponse {
    pub releases: Vec<RtkReleaseOption>,
    pub page: usize,
    pub has_more: bool,
}
