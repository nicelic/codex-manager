use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use crate::common::windows::get_user_profile_dir;

pub fn quote_windows_executable(path: &Path) -> String {
    let clean = path.to_string_lossy().to_string();
    format!("\"{}\"", clean.replace('\"', "\\\""))
}

pub fn claude_hook_command(exe: &Path) -> String {
    format!("{} hook claude", quote_windows_executable(exe))
}

pub fn copilot_hook_command(exe: &Path) -> String {
    format!("{} hook copilot", quote_windows_executable(exe))
}

pub fn cursor_hook_command(exe: &Path) -> String {
    format!("{} hook cursor", quote_windows_executable(exe))
}

// ----------------------------------------------------
// Claude Code Integration
// ----------------------------------------------------

pub fn claude_directory() -> PathBuf {
    if let Ok(val) = std::env::var("CLAUDE_CONFIG_DIR") {
        if !val.trim().is_empty() {
            return PathBuf::from(val.trim());
        }
    }
    get_user_profile_dir().join(".claude")
}

pub fn claude_settings_file() -> PathBuf {
    claude_directory().join("settings.json")
}

pub fn query_claude_available() -> bool {
    let dir = claude_directory();
    dir.exists() && dir.is_dir()
}

pub fn is_rtk_hook_command(command: &str, agent: &str) -> bool {
    let trimmed = command.trim();
    let suffix = format!(" hook {}", agent);
    if trimmed.len() <= suffix.len() || !trimmed.to_lowercase().ends_with(&suffix.to_lowercase()) {
        return false;
    }
    let prefix = trimmed[..trimmed.len() - suffix.len()].trim().trim_matches('"').trim_matches('\'');
    let p = Path::new(prefix);
    let stem = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_lowercase();
    stem == "rtk.exe" || stem == "rtk"
}

pub fn is_rtk_claude_hook_command(command: &str) -> bool {
    is_rtk_hook_command(command, "claude")
}

pub fn is_rtk_claude_hook_entry(item: &Value) -> bool {
    if let Some(nested) = item.get("hooks").and_then(|h| h.as_array()) {
        for hook in nested {
            if let Some(cmd) = hook.get("command").and_then(|c| c.as_str()) {
                if is_rtk_claude_hook_command(cmd) {
                    return true;
                }
            }
        }
    }
    false
}

pub fn claude_hook_entries_contain_command(items: &[Value], want_command: &str) -> bool {
    let want_norm = want_command.trim().to_lowercase();
    for item in items {
        if let Some(nested) = item.get("hooks").and_then(|h| h.as_array()) {
            for hook in nested {
                if let Some(cmd) = hook.get("command").and_then(|c| c.as_str()) {
                    if cmd.trim().to_lowercase() == want_norm {
                        return true;
                    }
                }
            }
        }
    }
    false
}

pub fn query_claude_hook_configured(exe: &Path) -> bool {
    let file = claude_settings_file();
    if !file.exists() {
        return false;
    }
    let content = match fs::read_to_string(&file) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let root: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let pre_tool_use = match root.get("hooks").and_then(|h| h.get("PreToolUse")).and_then(|p| p.as_array()) {
        Some(a) => a,
        None => return false,
    };
    let cmd = claude_hook_command(exe);
    claude_hook_entries_contain_command(pre_tool_use, &cmd)
}

pub fn claude_hook_residual_status() -> (bool, Option<String>) {
    let file = claude_settings_file();
    if !file.exists() {
        return (false, None);
    }
    let content = match fs::read_to_string(&file) {
        Ok(c) => c,
        Err(e) => return (true, Some(format!("读取 Claude settings.json 失败: {}", e))),
    };
    let root: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => return (true, Some(format!("解析 Claude settings.json 失败: {}", e))),
    };
    let pre_tool_use = match root.get("hooks").and_then(|h| h.get("PreToolUse")).and_then(|p| p.as_array()) {
        Some(a) => a,
        None => return (false, None),
    };
    let mut count = 0;
    for item in pre_tool_use {
        if is_rtk_claude_hook_entry(item) {
            count += 1;
        }
    }
    if count > 1 {
        return (true, Some("Claude settings.json 存在多个 RTK Hook".to_string()));
    }
    (false, None)
}

pub fn install_claude_hook(exe: &Path) -> Result<(), String> {
    let dir = claude_directory();
    if !dir.exists() {
        return Ok(());
    }
    if !dir.is_dir() {
        return Err(format!("Claude 配置路径不是目录: {}", dir.display()));
    }
    let file = claude_settings_file();
    let mut root: Value = if file.exists() {
        let content = fs::read_to_string(&file).map_err(|e| format!("读取 Claude settings.json 失败: {}", e))?;
        if content.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(&content).map_err(|e| format!("解析 Claude settings.json 失败: {}", e))?
        }
    } else {
        json!({})
    };

    if !root.is_object() {
        return Err("Claude settings.json 顶层不是对象，拒绝覆盖用户配置".to_string());
    }

    let root_obj = root.as_object_mut().unwrap();
    if !root_obj.contains_key("hooks") {
        root_obj.insert("hooks".to_string(), json!({}));
    }
    let hooks = match root_obj.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        Some(h) => h,
        None => return Err("Claude settings.hooks 不是对象，拒绝覆盖用户配置".to_string()),
    };

    let mut items = match hooks.get("PreToolUse").and_then(|p| p.as_array()) {
        Some(arr) => arr.clone(),
        None => Vec::new(),
    };

    let cmd = claude_hook_command(exe);
    if !claude_hook_entries_contain_command(&items, &cmd) {
        items.push(json!({
            "matcher": "Bash",
            "hooks": [
                {
                    "type": "command",
                    "command": cmd
                }
            ]
        }));
        hooks.insert("PreToolUse".to_string(), Value::Array(items));
        let data = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
        fs::write(&file, data + "\n").map_err(|e| format!("写入 Claude settings.json 失败: {}", e))?;
    }

    Ok(())
}

pub fn remove_claude_hook_command(command: &str) -> Result<(), String> {
    let file = claude_settings_file();
    if !file.exists() {
        return Ok(());
    }
    let content = fs::read_to_string(&file).map_err(|e| format!("读取 Claude settings.json 失败: {}", e))?;
    let mut root: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };
    let root_obj = match root.as_object_mut() {
        Some(o) => o,
        None => return Ok(()),
    };
    let hooks = match root_obj.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        Some(h) => h,
        None => return Ok(()),
    };
    let pre_tool_use = match hooks.get_mut("PreToolUse").and_then(|p| p.as_array_mut()) {
        Some(arr) => arr,
        None => return Ok(()),
    };

    let cmd_norm = command.trim().to_lowercase();
    let mut changed = false;

    pre_tool_use.retain_mut(|entry| {
        if let Some(nested) = entry.get_mut("hooks").and_then(|h| h.as_array_mut()) {
            let before_len = nested.len();
            nested.retain(|h| {
                if let Some(c) = h.get("command").and_then(|s| s.as_str()) {
                    if c.trim().to_lowercase() == cmd_norm {
                        return false;
                    }
                }
                true
            });
            if nested.len() != before_len {
                changed = true;
            }
            !nested.is_empty()
        } else {
            true
        }
    });

    if changed {
        if pre_tool_use.is_empty() {
            hooks.remove("PreToolUse");
        }
        if hooks.is_empty() {
            root_obj.remove("hooks");
        }
        let data = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
        fs::write(&file, data + "\n").map_err(|e| format!("更新 Claude settings.json 失败: {}", e))?;
    }

    Ok(())
}

// ----------------------------------------------------
// GitHub Copilot Integration
// ----------------------------------------------------

pub fn copilot_directory() -> PathBuf {
    if let Ok(val) = std::env::var("COPILOT_HOME") {
        if !val.trim().is_empty() {
            return PathBuf::from(val.trim());
        }
    }
    get_user_profile_dir().join(".copilot")
}

pub fn copilot_hook_file() -> PathBuf {
    copilot_directory().join("hooks").join("rtk-rewrite.json")
}

pub fn query_copilot_available() -> bool {
    let dir = copilot_directory();
    dir.exists() && dir.is_dir()
}

pub fn query_copilot_configured(exe: &Path) -> bool {
    let file = copilot_hook_file();
    if !file.exists() {
        return false;
    }
    let content = match fs::read_to_string(&file) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let root: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let pre_tool_use = match root.get("hooks").and_then(|h| h.get("PreToolUse")).and_then(|p| p.as_array()) {
        Some(a) => a,
        None => return false,
    };
    let cmd_norm = copilot_hook_command(exe).trim().to_lowercase();
    for item in pre_tool_use {
        if let Some(c) = item.get("command").and_then(|s| s.as_str()) {
            if c.trim().to_lowercase() == cmd_norm {
                return true;
            }
        }
    }
    false
}

pub fn install_copilot_hook(exe: &Path) -> Result<(), String> {
    let dir = copilot_directory();
    if !dir.exists() {
        return Ok(());
    }
    if !dir.is_dir() {
        return Err(format!("Copilot 配置路径不是目录: {}", dir.display()));
    }
    let file = copilot_hook_file();
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建 Copilot hooks 目录失败: {}", e))?;
    }

    let mut root: Value = if file.exists() {
        let content = fs::read_to_string(&file).map_err(|e| format!("读取 Copilot hooks 失败: {}", e))?;
        if content.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(&content).map_err(|e| format!("解析 Copilot hooks 失败: {}", e))?
        }
    } else {
        json!({})
    };

    let root_obj = match root.as_object_mut() {
        Some(o) => o,
        None => return Err("Copilot hook 文件不是对象，拒绝覆盖用户配置".to_string()),
    };

    if !root_obj.contains_key("hooks") {
        root_obj.insert("hooks".to_string(), json!({}));
    }
    let hooks = match root_obj.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        Some(h) => h,
        None => return Err("Copilot hooks 不是对象，拒绝覆盖用户配置".to_string()),
    };

    let mut items = match hooks.get("PreToolUse").and_then(|p| p.as_array()) {
        Some(arr) => arr.clone(),
        None => Vec::new(),
    };

    let cmd = copilot_hook_command(exe);
    let cmd_norm = cmd.trim().to_lowercase();
    let found = items.iter().any(|item| {
        item.get("command").and_then(|c| c.as_str()).map(|c| c.trim().to_lowercase() == cmd_norm).unwrap_or(false)
    });

    if !found {
        items.push(json!({
            "type": "command",
            "command": cmd,
            "cwd": ".",
            "timeout": 5
        }));
        hooks.insert("PreToolUse".to_string(), Value::Array(items));
        root_obj.insert("version".to_string(), json!(1));
        let data = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
        fs::write(&file, data + "\n").map_err(|e| format!("写入 Copilot hook 失败: {}", e))?;
    }

    Ok(())
}

pub fn remove_copilot_hook_command(command: &str) -> Result<(), String> {
    let file = copilot_hook_file();
    if !file.exists() {
        return Ok(());
    }
    let content = fs::read_to_string(&file).map_err(|e| format!("读取 Copilot hook 失败: {}", e))?;
    let mut root: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };
    let root_obj = match root.as_object_mut() {
        Some(o) => o,
        None => return Ok(()),
    };
    let hooks = match root_obj.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        Some(h) => h,
        None => return Ok(()),
    };
    let pre_tool_use = match hooks.get_mut("PreToolUse").and_then(|p| p.as_array_mut()) {
        Some(arr) => arr,
        None => return Ok(()),
    };

    let cmd_norm = command.trim().to_lowercase();
    pre_tool_use.retain(|item| {
        item.get("command").and_then(|c| c.as_str()).map(|c| c.trim().to_lowercase() != cmd_norm).unwrap_or(true)
    });

    if pre_tool_use.is_empty() {
        hooks.remove("PreToolUse");
    }
    if hooks.is_empty() {
        let _ = fs::remove_file(&file);
    } else {
        let data = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
        fs::write(&file, data + "\n").map_err(|e| format!("更新 Copilot hook 失败: {}", e))?;
    }

    Ok(())
}

// ----------------------------------------------------
// Cursor Integration
// ----------------------------------------------------

pub fn cursor_directory() -> PathBuf {
    get_user_profile_dir().join(".cursor")
}

pub fn cursor_hooks_file() -> PathBuf {
    cursor_directory().join("hooks.json")
}

pub fn query_cursor_available() -> bool {
    let dir = cursor_directory();
    dir.exists() && dir.is_dir()
}

pub fn query_cursor_configured(exe: &Path) -> bool {
    let file = cursor_hooks_file();
    if !file.exists() {
        return false;
    }
    let content = match fs::read_to_string(&file) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let root: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let pre_tool_use = match root.get("hooks").and_then(|h| h.get("preToolUse")).and_then(|p| p.as_array()) {
        Some(a) => a,
        None => return false,
    };
    let cmd_norm = cursor_hook_command(exe).trim().to_lowercase();
    for item in pre_tool_use {
        if let Some(c) = item.get("command").and_then(|s| s.as_str()) {
            if c.trim().to_lowercase() == cmd_norm {
                return true;
            }
        }
    }
    false
}

pub fn install_cursor_hook(exe: &Path) -> Result<(), String> {
    let dir = cursor_directory();
    if !dir.exists() {
        return Ok(());
    }
    if !dir.is_dir() {
        return Err(format!("Cursor 配置路径不是目录: {}", dir.display()));
    }
    let file = cursor_hooks_file();
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建 Cursor 目录失败: {}", e))?;
    }

    let mut root: Value = if file.exists() {
        let content = fs::read_to_string(&file).map_err(|e| format!("读取 Cursor hooks.json 失败: {}", e))?;
        if content.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(&content).map_err(|e| format!("解析 Cursor hooks.json 失败: {}", e))?
        }
    } else {
        json!({})
    };

    let root_obj = match root.as_object_mut() {
        Some(o) => o,
        None => return Err("Cursor hooks.json 顶层不是对象，拒绝覆盖用户配置".to_string()),
    };

    if !root_obj.contains_key("hooks") {
        root_obj.insert("hooks".to_string(), json!({}));
    }
    let hooks = match root_obj.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        Some(h) => h,
        None => return Err("Cursor hooks 不是对象，拒绝覆盖用户配置".to_string()),
    };

    let mut items = match hooks.get("preToolUse").and_then(|p| p.as_array()) {
        Some(arr) => arr.clone(),
        None => Vec::new(),
    };

    let cmd = cursor_hook_command(exe);
    let cmd_norm = cmd.trim().to_lowercase();
    let found = items.iter().any(|item| {
        item.get("command").and_then(|c| c.as_str()).map(|c| c.trim().to_lowercase() == cmd_norm).unwrap_or(false)
    });

    if !found {
        items.push(json!({
            "command": cmd
        }));
        hooks.insert("preToolUse".to_string(), Value::Array(items));
        let data = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
        fs::write(&file, data + "\n").map_err(|e| format!("写入 Cursor hooks.json 失败: {}", e))?;
    }

    Ok(())
}

pub fn remove_cursor_hook_command(command: &str) -> Result<(), String> {
    let file = cursor_hooks_file();
    if !file.exists() {
        return Ok(());
    }
    let content = fs::read_to_string(&file).map_err(|e| format!("读取 Cursor hooks.json 失败: {}", e))?;
    let mut root: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };
    let root_obj = match root.as_object_mut() {
        Some(o) => o,
        None => return Ok(()),
    };
    let hooks = match root_obj.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        Some(h) => h,
        None => return Ok(()),
    };
    let pre_tool_use = match hooks.get_mut("preToolUse").and_then(|p| p.as_array_mut()) {
        Some(arr) => arr,
        None => return Ok(()),
    };

    let cmd_norm = command.trim().to_lowercase();
    pre_tool_use.retain(|item| {
        item.get("command").and_then(|c| c.as_str()).map(|c| c.trim().to_lowercase() != cmd_norm).unwrap_or(true)
    });

    if pre_tool_use.is_empty() {
        hooks.remove("preToolUse");
    }
    if hooks.is_empty() {
        let _ = fs::remove_file(&file);
    } else {
        let data = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
        fs::write(&file, data + "\n").map_err(|e| format!("更新 Cursor hooks.json 失败: {}", e))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hook_command_recognition() {
        assert!(is_rtk_claude_hook_command(r#""C:\tools\RTK-AI\rtk.exe" hook claude"#));
        assert!(is_rtk_hook_command(r#"rtk.exe hook copilot"#, "copilot"));
        assert!(is_rtk_hook_command(r#""D:\my-rtk\rtk" hook cursor"#, "cursor"));
        assert!(!is_rtk_claude_hook_command(r#"other_tool hook claude"#));
    }
}
