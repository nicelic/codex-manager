use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub blocked_by: String,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub trust_notice: String,
    #[serde(default)]
    pub modified_agents: Vec<String>,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct RtkInstallRequest {
    pub tag_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RtkInstallResponse {
    pub path: String,
    pub version: String,
    pub user_path: bool,
    pub system_path: bool,
    pub codex_available: bool,
    pub codex_configured: bool,
    pub claude_available: bool,
    pub claude_hook_configured: bool,
    pub claude_prompt_configured: bool,
    pub claude_configured: bool,
    pub running: bool,
    pub desired_running: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RtkUninstallResponse {
    pub path: String,
    pub installed: bool,
    pub directory_exists: bool,
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
    pub running: bool,
    pub desired_running: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RtkReleaseOption {
    pub tag_name: String,
    pub name: String,
    pub published_at: String,
    pub prerelease: bool,
    pub available: bool,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub asset_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RtkReleaseListResponse {
    pub releases: Vec<RtkReleaseOption>,
    pub page: usize,
    pub per_page: usize,
    pub has_more: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ManagedToolState {
    #[serde(default)]
    pub desired_running: bool,
    #[serde(default)]
    pub running: bool,
    #[serde(default)]
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub user_path: bool,
    #[serde(default)]
    pub system_path: bool,
    #[serde(default)]
    pub owned_agents: Vec<String>,
    #[serde(default)]
    pub owned_files: Vec<String>,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}
