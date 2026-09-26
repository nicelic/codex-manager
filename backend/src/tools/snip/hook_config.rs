use hex;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::types::{
    SnipAgentSpec, SnipHookOwnership, SNIP_AGENT_SPECS,
};
use crate::common::windows::{
    canonicalize_clean, clean_path_str, get_user_profile_dir, strip_windows_verbatim_prefix,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnipHookLocation {
    pub agent: String,
    pub target_path: String,
    pub event: String,
    pub group_index: i32,
    pub handler_index: usize,
    pub group_fingerprint: Option<String>,
    pub command: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnipOwnedHookPresence {
    Absent,
    Exact,
    Moved,
}

pub fn snip_init_args(name: &str) -> Vec<String> {
    let mut args = vec!["init".to_string()];
    if name != "claude-code" {
        args.push("--agent".to_string());
        args.push(name.to_string());
    }
    args
}

pub fn snip_agent_directory(spec: &SnipAgentSpec, home: &Path) -> PathBuf {
    if spec.name == "claude-code" {
        if let Ok(configured) = std::env::var("CLAUDE_CONFIG_DIR") {
            let trimmed = configured.trim();
            if !trimmed.is_empty() {
                return PathBuf::from(trimmed);
            }
        }
    }
    home.join(spec.home_dir)
}

pub fn snip_agent_directories() -> HashMap<&'static str, PathBuf> {
    let home = get_user_profile_dir();
    let mut result = HashMap::new();
    for spec in &SNIP_AGENT_SPECS {
        result.insert(spec.name, snip_agent_directory(spec, &home));
    }
    result
}

pub fn snip_agent_hook_file(name: &str, directory: &Path) -> PathBuf {
    for spec in &SNIP_AGENT_SPECS {
        if spec.name == name {
            return directory.join(spec.target_file);
        }
    }
    PathBuf::new()
}

pub fn snip_hook_event_for_agent(name: &str) -> Option<(&'static str, bool)> {
    match name {
        "codex" | "claude-code" => Some(("PreToolUse", true)),
        "cursor" => Some(("beforeShellExecution", true)),
        "copilot" => Some(("preToolUse", false)),
        _ => None,
    }
}

pub fn read_snip_hook_json(path: &Path) -> Result<Option<Value>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let data = fs::read(path).map_err(|e| format!("读取 {} 失败: {}", path.display(), e))?;
    let val: Value = serde_json::from_slice(&data)
        .map_err(|e| format!("解析 {} 失败: {}", path.display(), e))?;
    Ok(Some(val))
}

pub fn write_snip_hook_json(path: &Path, config: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut data = serde_json::to_vec_pretty(config)
        .map_err(|e| format!("格式化 JSON 失败: {}", e))?;
    data.push(b'\n');
    replace_utf8_file(path, &data)
}

pub fn replace_utf8_file(path: &Path, data: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let temp_file = path.with_extension(format!("tmp.{}", chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)));
    fs::write(&temp_file, data).map_err(|e| format!("写入临时文件失败: {}", e))?;
    if let Err(e) = fs::rename(&temp_file, path) {
        let _ = fs::remove_file(&temp_file);
        fs::write(path, data).map_err(|write_err| {
            format!("重命名失败 ({}) 且直接写入失败: {}", e, write_err)
        })?;
    }
    Ok(())
}

pub fn snip_raw_hook_command(raw: &Value, field: &str) -> Result<Option<String>, String> {
    let obj = match raw.as_object() {
        Some(o) => o,
        None => return Err("处理器必须是对象".to_string()),
    };

    if obj.get("type").and_then(|v| v.as_str()) != Some("command") {
        return Ok(None);
    }

    if field == "command" {
        if let Some(command_win) = obj.get("commandWindows").and_then(|v| v.as_str()) {
            if !command_win.trim().is_empty() {
                return Ok(Some(command_win.to_string()));
            }
        }
    }

    match obj.get(field).and_then(|v| v.as_str()) {
        Some(cmd) => Ok(Some(cmd.to_string())),
        None => Err(format!("{} 必须是字符串", field)),
    }
}

pub fn snip_hook_handler_fingerprint(raw: &Value) -> Result<String, String> {
    let data = serde_json::to_vec(raw).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    hasher.update(&data);
    Ok(hex::encode(hasher.finalize()))
}

pub fn snip_hook_group_fingerprint(group: &Map<String, Value>) -> Result<String, String> {
    let mut context = group.clone();
    context.remove("hooks");
    let data = serde_json::to_vec(&Value::Object(context)).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    hasher.update(&data);
    Ok(hex::encode(hasher.finalize()))
}

pub fn snip_hook_locations_from_config<F>(
    name: &str,
    path: &Path,
    config: &Value,
    matches: F,
) -> Result<Vec<SnipHookLocation>, String>
where
    F: Fn(&str) -> bool,
{
    let (event, grouped) = match snip_hook_event_for_agent(name) {
        Some(pair) => pair,
        None => return Err(format!("未知 Snip Agent: {}", name)),
    };

    let hooks = match config.get("hooks").and_then(|h| h.as_object()) {
        Some(h) => h,
        None => return Ok(Vec::new()),
    };

    let raw_event = match hooks.get(event) {
        Some(e) => e,
        None => return Ok(Vec::new()),
    };

    let mut locations = Vec::new();
    let target_path_str = path.to_string_lossy().to_string();

    if grouped {
        let groups = match raw_event.as_array() {
            Some(g) => g,
            None => return Err(format!("hooks.{} 必须是数组", event)),
        };

        for (group_idx, raw_group) in groups.iter().enumerate() {
            let group = match raw_group.as_object() {
                Some(g) => g,
                None => return Err(format!("hooks.{}[{}] 必须是对象", event, group_idx)),
            };

            let group_fingerprint = snip_hook_group_fingerprint(group)?;
            let raw_handlers = match group.get("hooks").and_then(|h| h.as_array()) {
                Some(h) => h,
                None => continue,
            };

            for (handler_idx, raw_handler) in raw_handlers.iter().enumerate() {
                let cmd = match snip_raw_hook_command(raw_handler, "command")? {
                    Some(c) => c,
                    None => continue,
                };

                if !matches(&cmd) {
                    continue;
                }

                let fingerprint = snip_hook_handler_fingerprint(raw_handler)?;
                locations.push(SnipHookLocation {
                    agent: name.to_string(),
                    target_path: target_path_str.clone(),
                    event: event.to_string(),
                    group_index: group_idx as i32,
                    handler_index: handler_idx,
                    group_fingerprint: Some(group_fingerprint.clone()),
                    command: cmd,
                    fingerprint,
                });
            }
        }
    } else {
        let handlers = match raw_event.as_array() {
            Some(h) => h,
            None => return Err(format!("hooks.{} 必须是数组", event)),
        };

        for (handler_idx, raw_handler) in handlers.iter().enumerate() {
            let cmd = match snip_raw_hook_command(raw_handler, "bash")? {
                Some(c) => c,
                None => continue,
            };

            if !matches(&cmd) {
                continue;
            }

            let fingerprint = snip_hook_handler_fingerprint(raw_handler)?;
            locations.push(SnipHookLocation {
                agent: name.to_string(),
                target_path: target_path_str.clone(),
                event: event.to_string(),
                group_index: -1,
                handler_index: handler_idx,
                group_fingerprint: None,
                command: cmd,
                fingerprint,
            });
        }
    }

    Ok(locations)
}

pub fn snip_hook_locations_at<F>(
    name: &str,
    path: &Path,
    matches: F,
) -> Result<(Vec<SnipHookLocation>, bool), String>
where
    F: Fn(&str) -> bool,
{
    match read_snip_hook_json(path)? {
        Some(config) => {
            let locs = snip_hook_locations_from_config(name, path, &config, matches)?;
            Ok((locs, true))
        }
        None => Ok((Vec::new(), false)),
    }
}

pub fn snip_agent_hook_present_at(name: &str, path: &Path) -> Result<bool, String> {
    let (locations, exists) = snip_hook_locations_at(name, path, |command| {
        snip_hook_command_targets_agent(command, name)
    })?;
    Ok(exists && !locations.is_empty())
}

pub fn snip_agent_has_any_snip_hook_at(name: &str, path: &Path) -> Result<bool, String> {
    let (locations, exists) = snip_hook_locations_at(name, path, |command| {
        if let Some((executable, _)) = split_snip_hook_command(command) {
            is_snip_executable_name(&executable)
        } else {
            false
        }
    })?;
    Ok(exists && !locations.is_empty())
}

pub fn snip_owned_hook_presence_at(artifact: &SnipHookOwnership) -> Result<SnipOwnedHookPresence, String> {
    let path = Path::new(&artifact.target_path);
    let (locations, _) = snip_hook_locations_at(&artifact.agent, path, |_| true)?;

    let mut found_elsewhere = false;
    for location in locations {
        if location.event != artifact.event
            || location.fingerprint != artifact.fingerprint
            || location.command.trim() != artifact.command.trim()
        {
            continue;
        }

        let group_matches = if artifact.group_index < 0 {
            location.group_index < 0
        } else {
            location.group_index == artifact.group_index
                && location.group_fingerprint == artifact.group_fingerprint
        };

        if group_matches && location.handler_index == artifact.handler_index {
            return Ok(SnipOwnedHookPresence::Exact);
        }
        found_elsewhere = true;
    }

    if found_elsewhere {
        Ok(SnipOwnedHookPresence::Moved)
    } else {
        Ok(SnipOwnedHookPresence::Absent)
    }
}

pub fn remove_snip_owned_hook(artifact: &SnipHookOwnership) -> Result<bool, String> {
    let presence = snip_owned_hook_presence_at(artifact)?;
    if presence == SnipOwnedHookPresence::Absent {
        return Ok(false);
    }
    if presence == SnipOwnedHookPresence::Moved {
        return Err("Hook 已被人工移动或重新排序，无法安全确认原处理器；请手工处理后再清理".to_string());
    }

    let path = Path::new(&artifact.target_path);
    let mut config = match read_snip_hook_json(path)? {
        Some(c) => c,
        None => return Ok(false),
    };

    if config.get("hooks").and_then(|h| h.as_object()).is_none() {
        return Ok(false);
    }

    if artifact.group_index < 0 {
        remove_snip_owned_ungrouped_hook(path, &mut config, artifact)
    } else {
        remove_snip_owned_grouped_hook(path, &mut config, artifact)
    }
}

fn remove_snip_owned_grouped_hook(
    path: &Path,
    config: &mut Value,
    artifact: &SnipHookOwnership,
) -> Result<bool, String> {
    let hooks = match config.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        Some(h) => h,
        None => return Ok(false),
    };

    let raw_groups = match hooks.get_mut(&artifact.event).and_then(|g| g.as_array_mut()) {
        Some(g) => g,
        None => return Ok(false),
    };

    let group_idx = artifact.group_index as usize;
    if group_idx >= raw_groups.len() {
        return Ok(false);
    }

    let group = match raw_groups[group_idx].as_object_mut() {
        Some(g) => g,
        None => return Err(format!("hooks.{}[{}] 必须是对象", artifact.event, group_idx)),
    };

    let group_fp = snip_hook_group_fingerprint(group)?;
    if Some(&group_fp) != artifact.group_fingerprint.as_ref() {
        return Ok(false);
    }

    let raw_handlers = match group.get_mut("hooks").and_then(|h| h.as_array_mut()) {
        Some(h) => h,
        None => return Ok(false),
    };

    if artifact.handler_index >= raw_handlers.len() {
        return Ok(false);
    }

    let handler_fp = snip_hook_handler_fingerprint(&raw_handlers[artifact.handler_index])?;
    if handler_fp != artifact.fingerprint {
        return Ok(false);
    }

    raw_handlers.remove(artifact.handler_index);

    if raw_handlers.is_empty() {
        raw_groups.remove(group_idx);
        if raw_groups.is_empty() {
            hooks.remove(&artifact.event);
        }
    }

    if hooks.is_empty() {
        if let Some(root) = config.as_object_mut() {
            root.remove("hooks");
        }
    }

    write_snip_hook_json(path, config)?;
    Ok(true)
}

fn remove_snip_owned_ungrouped_hook(
    path: &Path,
    config: &mut Value,
    artifact: &SnipHookOwnership,
) -> Result<bool, String> {
    let hooks = match config.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        Some(h) => h,
        None => return Ok(false),
    };

    let raw_handlers = match hooks.get_mut(&artifact.event).and_then(|g| g.as_array_mut()) {
        Some(g) => g,
        None => return Ok(false),
    };

    if artifact.handler_index >= raw_handlers.len() {
        return Ok(false);
    }

    let handler_fp = snip_hook_handler_fingerprint(&raw_handlers[artifact.handler_index])?;
    if handler_fp != artifact.fingerprint {
        return Ok(false);
    }

    raw_handlers.remove(artifact.handler_index);
    if raw_handlers.is_empty() {
        hooks.remove(&artifact.event);
    }

    if hooks.is_empty() {
        if let Some(root) = config.as_object_mut() {
            root.remove("hooks");
        }
    }

    write_snip_hook_json(path, config)?;
    Ok(true)
}

pub fn split_snip_hook_command(command: &str) -> Option<(String, Vec<String>)> {
    let mut s = command.trim();
    if let Some(rest) = s.strip_prefix('&') {
        s = rest.trim();
    }
    if s.is_empty() {
        return None;
    }

    let (executable, remaining) = if s.starts_with(r#"\""#) {
        // Handle Windows escaped quote: \"C:/Program Files/.../snip.exe\"
        let after_prefix = &s[2..];
        let closing = after_prefix.find(r#"\""#)?;
        let exe = &after_prefix[..closing];
        let rem = after_prefix[closing + 2..].trim();
        (exe.to_string(), rem)
    } else if s.starts_with('"') || s.starts_with('\'') {
        let quote = s.chars().next()?;
        let after_quote = &s[1..];
        let closing = after_quote.find(quote)?;
        let exe = &after_quote[..closing];
        let rem = after_quote[closing + 1..].trim();
        (exe.to_string(), rem)
    } else {
        let parts: Vec<&str> = s.split_whitespace().collect();
        if parts.is_empty() {
            return None;
        }
        let exe = parts[0];
        let rem = s.strip_prefix(exe).unwrap_or("").trim();
        (exe.to_string(), rem)
    };

    let args: Vec<String> = remaining
        .split_whitespace()
        .map(|w| w.to_string())
        .collect();

    Some((executable, args))
}

pub fn snip_hook_arguments_match(args: &[String], agent: &str) -> bool {
    let mut want = vec!["hook".to_string()];
    if agent == "codex" || agent == "copilot" {
        want.push(agent.to_string());
    }
    args == want.as_slice()
}

pub fn is_snip_executable_name(value: &str) -> bool {
    let normalized = value.replace('/', "\\");
    let path = Path::new(&normalized);
    if let Some(file_name) = path.file_name().and_then(|f| f.to_str()) {
        let lower = file_name.to_ascii_lowercase();
        return lower == "snip" || lower == "snip.exe";
    }
    false
}

pub fn normalize_snip_executable_path(value: &str) -> String {
    let clean = clean_path_str(value);
    let p = Path::new(&clean);
    let canonical = canonicalize_clean(p).unwrap_or_else(|_| strip_windows_verbatim_prefix(p));
    clean_path_str(&canonical.to_string_lossy())
}

pub fn snip_hook_command_targets_agent(command: &str, agent: &str) -> bool {
    if let Some((exe, args)) = split_snip_hook_command(command) {
        if is_snip_executable_name(&exe) {
            return snip_hook_arguments_match(&args, agent);
        }
    }
    false
}

pub fn snip_hook_command_matches_executable(command: &str, agent: &str, executable: &str) -> bool {
    if let Some((command_exe, args)) = split_snip_hook_command(command) {
        if !snip_hook_arguments_match(&args, agent) {
            return false;
        }
        return normalize_snip_executable_path(&command_exe)
            .eq_ignore_ascii_case(&normalize_snip_executable_path(executable));
    }
    false
}
