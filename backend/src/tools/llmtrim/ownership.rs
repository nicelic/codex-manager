use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

pub const TOOL_STATE_FILE: &str = ".code-manager-state.json";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ManagedToolState {
    pub desired_running: bool,
    pub running: bool,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

pub fn read_managed_tool_state(install_dir: &Path) -> Result<ManagedToolState, String> {
    let state_file = install_dir.join(TOOL_STATE_FILE);
    if !state_file.exists() {
        return Ok(ManagedToolState::default());
    }
    let content = fs::read_to_string(&state_file)
        .map_err(|e| format!("读取状态账本失败: {}", e))?;
    serde_json::from_str(&content).map_err(|e| format!("解析状态账本失败: {}", e))
}

pub fn write_managed_tool_state(install_dir: &Path, state: &ManagedToolState) -> Result<(), String> {
    let _ = fs::create_dir_all(install_dir);
    let state_file = install_dir.join(TOOL_STATE_FILE);
    let content = serde_json::to_string_pretty(state)
        .map_err(|e| format!("序列化状态账本失败: {}", e))?;
    fs::write(&state_file, content).map_err(|e| format!("写入状态账本失败: {}", e))
}

pub fn remove_managed_tool_state(install_dir: &Path) -> Result<(), String> {
    let state_file = install_dir.join(TOOL_STATE_FILE);
    if state_file.exists() {
        fs::remove_file(&state_file).map_err(|e| format!("删除状态账本失败: {}", e))?;
    }
    Ok(())
}

pub fn mark_managed_tool_attention(install_dir: &Path, cause: &str) -> Result<(), String> {
    let mut state = read_managed_tool_state(install_dir).unwrap_or_default();
    state.metadata.insert("last_error".to_string(), cause.to_string());
    state.metadata.insert("attention_needed".to_string(), "true".to_string());
    write_managed_tool_state(install_dir, &state)
}
