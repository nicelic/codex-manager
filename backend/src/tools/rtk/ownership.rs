use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use crate::common::windows::{canonicalize_clean, clean_path_str, strip_windows_verbatim_prefix};
use super::assistant::{
    discover_assistant_targets, prompt_fingerprint, replace_utf8_file, rtk_command_block_bodies,
    strip_rtk_command_blocks_where,
};
use super::integrations::{
    claude_hook_entries_contain_command, claude_settings_file, copilot_hook_file, cursor_hooks_file,
    remove_claude_hook_command, remove_copilot_hook_command, remove_cursor_hook_command,
};
use super::types::ManagedToolState;

pub const RTK_OWNERSHIP_METADATA_KEY: &str = "rtk_ownership_v1";
pub const RTK_OWNERSHIP_VERSION: usize = 1;

pub const RTK_ARTIFACT_CODEX_PROMPT: &str = "codex-prompt";
pub const RTK_ARTIFACT_CLAUDE_PROMPT: &str = "claude-prompt";
pub const RTK_ARTIFACT_CLAUDE_HOOK: &str = "claude-hook";
pub const RTK_ARTIFACT_COPILOT_HOOK: &str = "copilot-hook";
pub const RTK_ARTIFACT_CURSOR_HOOK: &str = "cursor-hook";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RtkArtifactOwnership {
    pub agent: String,
    pub target_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RtkOwnershipLedger {
    pub version: usize,
    pub artifacts: HashMap<String, RtkArtifactOwnership>,
}

impl Default for RtkOwnershipLedger {
    fn default() -> Self {
        Self {
            version: RTK_OWNERSHIP_VERSION,
            artifacts: HashMap::new(),
        }
    }
}

pub fn read_ownership_ledger(state: &ManagedToolState) -> Result<RtkOwnershipLedger, String> {
    if let Some(val) = state.metadata.get(RTK_OWNERSHIP_METADATA_KEY) {
        if !val.trim().is_empty() {
            let ledger: RtkOwnershipLedger = serde_json::from_str(val)
                .map_err(|e| format!("解析 RTK 精确归属账本失败: {}", e))?;
            return Ok(ledger);
        }
    }
    Ok(RtkOwnershipLedger::default())
}

pub fn write_ownership_ledger(state: &mut ManagedToolState, ledger: &RtkOwnershipLedger) -> Result<(), String> {
    let encoded = serde_json::to_string(ledger).map_err(|e| e.to_string())?;
    state.metadata.insert(RTK_OWNERSHIP_METADATA_KEY.to_string(), encoded);

    let mut agents: Vec<String> = ledger
        .artifacts
        .values()
        .map(|a| a.agent.clone())
        .collect();
    agents.sort();
    agents.dedup();
    state.owned_agents = agents;

    Ok(())
}

pub fn clear_ownership_ledger(state: &mut ManagedToolState) {
    state.owned_agents.clear();
    state.metadata.remove(RTK_OWNERSHIP_METADATA_KEY);
}

pub fn canonical_target_path(path: &Path) -> String {
    let canonical = canonicalize_clean(path).unwrap_or_else(|_| strip_windows_verbatim_prefix(path));
    clean_path_str(&canonical.to_string_lossy())
}

pub fn prompt_artifact_ownership(agent: &str, target_path: &Path, payload: &str) -> RtkArtifactOwnership {
    RtkArtifactOwnership {
        agent: agent.to_string(),
        target_path: canonical_target_path(target_path),
        fingerprint: Some(prompt_fingerprint(payload)),
        command: None,
    }
}

pub fn hook_artifact_ownership(agent: &str, target_path: &Path, command: &str) -> RtkArtifactOwnership {
    RtkArtifactOwnership {
        agent: agent.to_string(),
        target_path: canonical_target_path(target_path),
        fingerprint: None,
        command: Some(command.trim().to_string()),
    }
}

pub fn artifact_present(name: &str, artifact: &RtkArtifactOwnership) -> Result<bool, String> {
    match name {
        RTK_ARTIFACT_CODEX_PROMPT | RTK_ARTIFACT_CLAUDE_PROMPT => {
            let path = Path::new(&artifact.target_path);
            if !path.exists() {
                return Ok(false);
            }
            let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
            let bodies = rtk_command_block_bodies(&content)?;
            if let Some(fp) = &artifact.fingerprint {
                return Ok(bodies.iter().any(|b| prompt_fingerprint(b) == *fp));
            }
            Ok(false)
        }
        RTK_ARTIFACT_CLAUDE_HOOK => {
            let path = claude_settings_file();
            if !path.exists() {
                return Ok(false);
            }
            let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
            let root: serde_json::Value = serde_json::from_str(&content).unwrap_or(serde_json::Value::Null);
            if let Some(arr) = root.get("hooks").and_then(|h| h.get("PreToolUse")).and_then(|p| p.as_array()) {
                if let Some(cmd) = &artifact.command {
                    return Ok(claude_hook_entries_contain_command(arr, cmd));
                }
            }
            Ok(false)
        }
        RTK_ARTIFACT_COPILOT_HOOK => {
            let path = copilot_hook_file();
            if !path.exists() {
                return Ok(false);
            }
            let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
            let root: serde_json::Value = serde_json::from_str(&content).unwrap_or(serde_json::Value::Null);
            if let Some(arr) = root.get("hooks").and_then(|h| h.get("PreToolUse")).and_then(|p| p.as_array()) {
                if let Some(cmd) = &artifact.command {
                    let cmd_norm = cmd.trim().to_lowercase();
                    return Ok(arr.iter().any(|item| {
                        item.get("command").and_then(|c| c.as_str()).map(|c| c.trim().to_lowercase() == cmd_norm).unwrap_or(false)
                    }));
                }
            }
            Ok(false)
        }
        RTK_ARTIFACT_CURSOR_HOOK => {
            let path = cursor_hooks_file();
            if !path.exists() {
                return Ok(false);
            }
            let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
            let root: serde_json::Value = serde_json::from_str(&content).unwrap_or(serde_json::Value::Null);
            if let Some(arr) = root.get("hooks").and_then(|h| h.get("preToolUse")).and_then(|p| p.as_array()) {
                if let Some(cmd) = &artifact.command {
                    let cmd_norm = cmd.trim().to_lowercase();
                    return Ok(arr.iter().any(|item| {
                        item.get("command").and_then(|c| c.as_str()).map(|c| c.trim().to_lowercase() == cmd_norm).unwrap_or(false)
                    }));
                }
            }
            Ok(false)
        }
        _ => Err(format!("未知 RTK 归属项目: {}", name)),
    }
}

pub fn remove_artifact(name: &str, artifact: &RtkArtifactOwnership) -> Result<(), String> {
    match name {
        RTK_ARTIFACT_CODEX_PROMPT | RTK_ARTIFACT_CLAUDE_PROMPT => {
            let path = Path::new(&artifact.target_path);
            if !path.exists() {
                return Ok(());
            }
            let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
            if let Some(fp) = &artifact.fingerprint {
                let (updated, removed) = strip_rtk_command_blocks_where(&content, |body| {
                    prompt_fingerprint(body) == *fp
                })?;
                if removed > 0 {
                    replace_utf8_file(path, &updated)?;
                }
            }
            Ok(())
        }
        RTK_ARTIFACT_CLAUDE_HOOK => {
            if let Some(cmd) = &artifact.command {
                remove_claude_hook_command(cmd)?;
            }
            Ok(())
        }
        RTK_ARTIFACT_COPILOT_HOOK => {
            if let Some(cmd) = &artifact.command {
                remove_copilot_hook_command(cmd)?;
            }
            Ok(())
        }
        RTK_ARTIFACT_CURSOR_HOOK => {
            if let Some(cmd) = &artifact.command {
                remove_cursor_hook_command(cmd)?;
            }
            Ok(())
        }
        _ => Err(format!("未知 RTK 归属项目: {}", name)),
    }
}

pub fn remove_owned_artifacts(ledger: &RtkOwnershipLedger) -> Result<(), String> {
    let order = [
        RTK_ARTIFACT_CODEX_PROMPT,
        RTK_ARTIFACT_CLAUDE_PROMPT,
        RTK_ARTIFACT_CLAUDE_HOOK,
        RTK_ARTIFACT_COPILOT_HOOK,
        RTK_ARTIFACT_CURSOR_HOOK,
    ];

    for name in order {
        if let Some(artifact) = ledger.artifacts.get(name) {
            remove_artifact(name, artifact)
                .map_err(|e| format!("清理 {} 失败: {}", name, e))?;
        }
    }

    Ok(())
}

pub fn modified_agent_names(ledger: &RtkOwnershipLedger) -> Vec<String> {
    let mut agents = Vec::new();
    for (name, artifact) in &ledger.artifacts {
        if let Ok(true) = artifact_present(name, artifact) {
            agents.push(artifact.agent.clone());
        }
    }
    agents.sort();
    agents.dedup();
    agents
}

// ----------------------------------------------------
// Snapshots and Rollback
// ----------------------------------------------------

#[derive(Debug, Clone)]
pub struct IntegrationSnapshot {
    pub agent_name: &'static str,
    pub path: PathBuf,
    pub exists: bool,
    pub data: Vec<u8>,
}

pub fn snapshot_integrations() -> HashMap<String, IntegrationSnapshot> {
    let mut snapshots = HashMap::new();

    for target in discover_assistant_targets() {
        let mut snap = IntegrationSnapshot {
            agent_name: target.agent_name,
            path: target.file_path.clone(),
            exists: false,
            data: Vec::new(),
        };
        if let Ok(bytes) = fs::read(&target.file_path) {
            snap.exists = true;
            snap.data = bytes;
        }
        snapshots.insert(target.snapshot_key.to_string(), snap);
    }

    let extra = [
        ("claude-code-hook", "claude-code", claude_settings_file()),
        ("copilot", "copilot", copilot_hook_file()),
        ("cursor", "cursor", cursor_hooks_file()),
    ];

    for (key, agent, path) in extra {
        let mut snap = IntegrationSnapshot {
            agent_name: agent,
            path: path.clone(),
            exists: false,
            data: Vec::new(),
        };
        if let Ok(bytes) = fs::read(&path) {
            snap.exists = true;
            snap.data = bytes;
        }
        snapshots.insert(key.to_string(), snap);
    }

    snapshots
}

pub fn restore_integration_snapshots(snapshots: &HashMap<String, IntegrationSnapshot>) -> Result<(), String> {
    for snap in snapshots.values() {
        if !snap.exists {
            if snap.path.exists() {
                let _ = fs::remove_file(&snap.path);
            }
            continue;
        }
        if let Some(parent) = snap.path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&snap.path, &snap.data);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ownership_ledger_roundtrip() {
        let mut state = ManagedToolState::default();
        let mut ledger = RtkOwnershipLedger::default();
        ledger.artifacts.insert(
            RTK_ARTIFACT_CODEX_PROMPT.to_string(),
            prompt_artifact_ownership("codex", Path::new("dummy/AGENTS.md"), "test prompt"),
        );
        write_ownership_ledger(&mut state, &ledger).unwrap();
        assert_eq!(state.owned_agents, vec!["codex"]);

        let read_back = read_ownership_ledger(&state).unwrap();
        assert_eq!(read_back.artifacts.len(), 1);
        assert!(read_back.artifacts.contains_key(RTK_ARTIFACT_CODEX_PROMPT));
    }
}
