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
pub struct LlmtrimInstallResponse {
    pub path: String,
    pub version: String,
    pub running: bool,
    pub configured: bool,
    pub process_id: u32,
    pub port: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmtrimUninstallResponse {
    pub path: String,
    pub running: bool,
    pub configured: bool,
    pub directory_exists: bool,
    pub state_dir_exists: bool,
    pub tray_running: bool,
    pub residual: bool,
    pub process_id: u32,
    pub tray_process_id: u32,
    pub port: String,
    pub message: String,
    pub warnings: Vec<String>,
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
    pub per_page: usize,
    pub has_more: bool,
}

#[derive(Debug, Clone, Default)]
pub struct LlmtrimActivationSnapshot {
    pub installed: bool,
    pub directory_exists: bool,
    pub configured_path: String,
    pub configured_path_available: bool,
    pub running: bool,
    pub tray_running: bool,
    pub process_id: u32,
    pub port_open: bool,
    pub desired_running: bool,
    pub recorded_running: bool,
    pub windows_configured: bool,
    pub state_read_failed: bool,
    pub state_needs_attention: bool,
}
