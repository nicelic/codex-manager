use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GortexStatusResponse {
    pub path: String,
    pub managed_root: String,
    pub managed_root_exists: bool,
    pub version: String,
    pub installed: bool,
    pub managed_installed: bool,
    pub installing: bool,
    pub running: bool,
    pub any_process_running: bool,
    pub unmanaged_process_running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process_id: Option<u32>,
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
    pub opencode_available: bool,
    pub opencode_configured: bool,
    pub opencode_complete: bool,
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
    pub opencode_prompt: bool,
    pub opencode_prompt_complete: bool,
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
    pub opencode_hook: bool,
    pub opencode_hook_complete: bool,
    pub antigravity_hook: bool,
    pub antigravity_hook_complete: bool,
    pub gemini_hook: bool,
    pub gemini_hook_complete: bool,

    pub user_path: bool,
    pub system_path: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codex_trust_status: Option<String>,
    pub codex_trust_required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codex_trust_notice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codex_trust_steps: Option<Vec<String>>,

    pub tracked_projects: Vec<String>,
    pub project_mcp_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_mcp_projects: Option<Vec<String>>,
    pub default_project: String,
    pub integration_present: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_task: Option<GortexActiveTask>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GortexActiveTask {
    pub path: String,
    pub action: String,
}

#[derive(Debug, Deserialize)]
pub struct GortexOperationRequest {
    pub path: String,
}

pub type GortexTrackRequest = GortexOperationRequest;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GortexOperationResponse {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct GortexInstallRequest {
    pub tag_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GortexProjectRegistry {
    pub projects: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GortexReleaseOption {
    pub tag_name: String,
    pub name: String,
    pub published_at: String,
    pub prerelease: bool,
    pub available: bool,
    #[serde(skip_serializing_if = "String::is_empty", default)]
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
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GortexOwnedMCP {
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GortexOwnedProjectMCP {
    pub agent: String,
    pub project: String,
    pub path: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GortexOwnedArtifact {
    pub kind: String,
    pub agent: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
    pub fingerprint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handler_fingerprint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GortexMCPOwnership {
    #[serde(default)]
    pub platforms: BTreeMap<String, GortexOwnedMCP>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub project_mcp: BTreeMap<String, GortexOwnedProjectMCP>,
    #[serde(default)]
    pub project_mcp_enabled: bool,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub artifacts: BTreeMap<String, GortexOwnedArtifact>,
    #[serde(default)]
    pub user_path: bool,
    #[serde(default)]
    pub system_path: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GortexCodexTrustInfo {
    pub status: String,
    pub required: bool,
    pub notice: String,
    pub command: String,
    pub steps: Vec<String>,
    pub shell_opened: bool,
}
