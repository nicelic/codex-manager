use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

pub use crate::tools::rtk::types::ManagedToolState;

pub const SNIP_OWNERSHIP_METADATA_KEY: &str = "snip_ownership_v1";
pub const SNIP_OWNERSHIP_VERSION: usize = 1;

pub const SNIP_ARTIFACT_CODEX_HOOK: &str = "codex-hook";
pub const SNIP_ARTIFACT_CLAUDE_HOOK: &str = "claude-code-hook";
pub const SNIP_ARTIFACT_CURSOR_HOOK: &str = "cursor-hook";
pub const SNIP_ARTIFACT_COPILOT_HOOK: &str = "copilot-hook";

pub const SNIP_TRUST_NOT_APPLICABLE: &str = "not_applicable";
pub const SNIP_TRUST_TRUSTED: &str = "trusted";
pub const SNIP_TRUST_UNTRUSTED: &str = "untrusted";
pub const SNIP_TRUST_UNKNOWN: &str = "unknown";
pub const SNIP_TRUST_DISABLED: &str = "disabled";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnipAgentState {
    pub name: String,
    pub directory: String,
    pub target_file: String,
    pub available: bool,
    pub hook_exists: bool,
    pub configured: bool,
    pub modified: bool,
    pub repair_needed: bool,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub trust: String,
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
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub blocked_by: String,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub trust_notice: String,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub trust_status: String,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub trust_command: String,
    #[serde(default)]
    pub trust_required: bool,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub trust_steps: Vec<String>,
    #[serde(default)]
    pub trust_shell_open: bool,
    #[serde(default)]
    pub cleanup_required: bool,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub modified_agents: Vec<String>,
    pub agents: Vec<SnipAgentState>,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct SnipInstallRequest {
    pub tag_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnipInstallResponse {
    pub path: String,
    pub version: String,
    pub running: bool,
    pub desired_running: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnipReleaseOption {
    pub tag_name: String,
    pub name: String,
    pub published_at: String,
    pub prerelease: bool,
    pub available: bool,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub asset_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnipReleaseListResponse {
    pub releases: Vec<SnipReleaseOption>,
    pub page: usize,
    pub per_page: usize,
    pub has_more: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct SnipAgentSpec {
    pub name: &'static str,
    pub home_dir: &'static str,
    pub target_file: &'static str,
}

pub const SNIP_AGENT_SPECS: [SnipAgentSpec; 4] = [
    SnipAgentSpec {
        name: "codex",
        home_dir: ".codex",
        target_file: "hooks.json",
    },
    SnipAgentSpec {
        name: "claude-code",
        home_dir: ".claude",
        target_file: "settings.json",
    },
    SnipAgentSpec {
        name: "cursor",
        home_dir: ".cursor",
        target_file: "hooks.json",
    },
    SnipAgentSpec {
        name: "copilot",
        home_dir: ".copilot",
        target_file: "hooks/snip.json",
    },
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnipHookOwnership {
    pub agent: String,
    pub target_path: String,
    pub event: String,
    pub group_index: i32,
    pub handler_index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_fingerprint: Option<String>,
    pub fingerprint: String,
    pub command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnipOwnershipLedger {
    pub version: usize,
    pub artifacts: HashMap<String, SnipHookOwnership>,
}

impl Default for SnipOwnershipLedger {
    fn default() -> Self {
        Self {
            version: SNIP_OWNERSHIP_VERSION,
            artifacts: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SnipCodexTrustInfo {
    pub status: String,
    pub hook_path: String,
    pub codex_path: String,
    pub command: String,
    pub notice: String,
    pub steps: Vec<String>,
    pub shell_opened: bool,
}

#[derive(Debug, Clone)]
pub struct SnipAgentSnapshot {
    pub path: PathBuf,
    pub exists: bool,
    pub data: Vec<u8>,
    pub created_directories: Vec<PathBuf>,
}
