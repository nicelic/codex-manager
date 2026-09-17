use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnipAgentState {
    pub name: String,
    pub hook_exists: bool,
    pub repair_needed: bool,
    pub modified: bool,
    pub trust: String,
    pub configured: bool,
    pub target_file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnipStatusResponse {
    pub path: String,
    pub installed: bool,
    pub directory_exists: bool,
    pub version: String,
    pub user_path: bool,
    pub system_path: bool,
    pub running: bool,
    pub desired_running: bool,
    pub activation_state: String,
    pub blocked_by: String,
    pub trust_notice: String,
    pub trust_status: String,
    pub trust_command: String,
    pub trust_required: bool,
    pub trust_steps: Vec<String>,
    pub cleanup_required: bool,
    pub modified_agents: Vec<String>,
    pub agents: Vec<SnipAgentState>,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct SnipInstallRequest {
    pub tag_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnipReleaseOption {
    pub tag_name: String,
    pub name: String,
    pub published_at: String,
    pub prerelease: bool,
    pub available: bool,
    pub asset_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnipReleaseListResponse {
    pub releases: Vec<SnipReleaseOption>,
    pub page: usize,
    pub has_more: bool,
}
