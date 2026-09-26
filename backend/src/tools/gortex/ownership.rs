use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use serde_json::Value;

use super::types::{GortexMCPOwnership, GortexOwnedMCP, GortexOwnedProjectMCP};

const GORTEX_OWNERSHIP_FILE: &str = "mcp-ownership.json";

pub fn gortex_ownership_path(install_root: &Path) -> PathBuf {
    install_root.join("config").join(GORTEX_OWNERSHIP_FILE)
}

pub fn canonical_json_value(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut sorted = serde_json::Map::new();
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for k in keys {
                sorted.insert(k.clone(), canonical_json_value(&map[k]));
            }
            Value::Object(sorted)
        }
        Value::Array(arr) => {
            let items: Vec<Value> = arr.iter().map(canonical_json_value).collect();
            Value::Array(items)
        }
        other => other.clone(),
    }
}

pub fn gortex_artifact_fingerprint(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

pub fn gortex_json_artifact_fingerprint(value: &Value) -> String {
    let canonical = canonical_json_value(value);
    let bytes = serde_json::to_vec(&canonical).unwrap_or_default();
    gortex_artifact_fingerprint(&bytes)
}

pub fn gortex_ownership_key(agent: &str, path: &Path) -> String {
    let clean = path.to_string_lossy().to_string();
    let norm = clean.replace('/', "\\").to_ascii_lowercase();
    format!("{}|{}", agent, norm)
}

pub fn gortex_project_mcp_key(agent: &str, project: &str) -> String {
    let norm = project.replace('/', "\\").to_ascii_lowercase();
    format!("{}|{}", agent, norm)
}

pub fn read_gortex_ownership(install_root: &Path) -> Result<GortexMCPOwnership, String> {
    let path = gortex_ownership_path(install_root);
    if !path.exists() {
        return Ok(GortexMCPOwnership::default());
    }
    let data = fs::read_to_string(&path)
        .map_err(|e| format!("读取 Gortex 归属账本失败: {}", e))?;
    if data.trim().is_empty() {
        return Ok(GortexMCPOwnership::default());
    }
    let ownership: GortexMCPOwnership = serde_json::from_str(&data)
        .map_err(|e| format!("解析 Gortex 归属账本失败: {}", e))?;
    Ok(ownership)
}

pub fn write_gortex_ownership(install_root: &Path, ownership: &GortexMCPOwnership) -> Result<(), String> {
    let path = gortex_ownership_path(install_root);
    if ownership.platforms.is_empty()
        && ownership.project_mcp.is_empty()
        && !ownership.project_mcp_enabled
        && ownership.artifacts.is_empty()
        && !ownership.user_path
        && !ownership.system_path
    {
        if path.exists() {
            let _ = fs::remove_file(&path);
        }
        return Ok(());
    }

    if let Some(p) = path.parent() {
        let _ = fs::create_dir_all(p);
    }

    let data = serde_json::to_string_pretty(ownership)
        .map_err(|e| format!("序列化 Gortex 归属账本失败: {}", e))?;

    let final_data = format!("{}\n", data);
    fs::write(&path, final_data)
        .map_err(|e| format!("写入 Gortex 归属账本失败: {}", e))?;
    Ok(())
}

pub fn gortex_mcp_entry_looks_managed(value: &Value) -> bool {
    let obj = match value.as_object() {
        Some(o) => o,
        None => return false,
    };
    let command = obj.get("command").and_then(|c| c.as_str()).unwrap_or("");
    let base = Path::new(command)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let first_arg = obj.get("args").and_then(|a| {
        if let Some(arr) = a.as_array() {
            arr.first().and_then(|v| v.as_str())
        } else {
            None
        }
    }).unwrap_or("");

    if first_arg == "gortex-bridge" {
        return true;
    }
    if base == "gortex" && first_arg == "mcp" {
        return true;
    }
    if (base.contains("code-manager") || base.contains("llmtrim") || base.contains("edit_rust"))
        && (first_arg == "gortex-bridge" || first_arg == "mcp")
    {
        return true;
    }
    false
}

pub fn gortex_opencode_entry_looks_managed(value: &Value) -> bool {
    let obj = match value.as_object() {
        Some(o) => o,
        None => return false,
    };
    if let Some(cmd) = obj.get("command").and_then(|c| c.as_array()) {
        if cmd.len() >= 2 {
            let first = cmd[0].as_str().unwrap_or("");
            let second = cmd[1].as_str().unwrap_or("");
            if second == "mcp" {
                let base = Path::new(first)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if base == "gortex" || base.contains("code-manager") {
                    return true;
                }
            }
        }
    }
    false
}

pub fn gortex_mcp_registration_allowed(existing: &Value, desired: &Value, owned: &GortexOwnedMCP) -> bool {
    let existing_fp = gortex_json_artifact_fingerprint(existing);
    let desired_fp = gortex_json_artifact_fingerprint(desired);
    if existing_fp == desired_fp {
        return true;
    }
    if !owned.fingerprint.is_empty() && existing_fp == owned.fingerprint {
        return true;
    }
    gortex_mcp_entry_looks_managed(existing)
}

pub fn gortex_project_mcp_record(
    ownership: &mut GortexMCPOwnership,
    agent: &str,
    project: &str,
    path: &str,
    entry: &Value,
) {
    let fp = gortex_json_artifact_fingerprint(entry);
    let key = gortex_project_mcp_key(agent, project);
    ownership.project_mcp.insert(
        key,
        GortexOwnedProjectMCP {
            agent: agent.to_string(),
            project: project.to_string(),
            path: path.to_string(),
            fingerprint: fp,
        },
    );
}

pub fn gortex_delete_project_mcp_record(
    ownership: &mut GortexMCPOwnership,
    agent: &str,
    project: &str,
) {
    let key = gortex_project_mcp_key(agent, project);
    ownership.project_mcp.remove(&key);
}
