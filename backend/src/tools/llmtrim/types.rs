use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmtrimStatusResponse {
    pub path: String,
    pub version: String,
    pub running: bool,
    pub desired_running: bool,
    pub activation_state: String,
    pub installed: bool,
    pub configured: bool,
    pub directory_exists: bool,
    pub state_dir_exists: bool,
    pub tray_running: bool,
    pub process_id: u32,
    pub tray_process_id: u32,
    pub residual: bool,
    pub port: String,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct LlmtrimInstallRequest {
    pub tag_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmtrimReleaseOption {
    pub tag_name: String,
    pub name: String,
    pub published_at: String,
    pub prerelease: bool,
    pub available: bool,
    pub asset_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmtrimReleaseListResponse {
    pub releases: Vec<LlmtrimReleaseOption>,
    pub page: usize,
    pub has_more: bool,
}
