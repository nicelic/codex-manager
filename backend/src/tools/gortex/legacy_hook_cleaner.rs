// ============================================================================
// 【兼容过渡模块：旧版 GORTEX HOOKS 精准清理器】
// 职责：检测存量配置文件中历史写入的 Gortex 钩子（Hook）并执行外科手术式剥离。
// 铁律：
//   1. 仅精准匹配并移除明确属于 Gortex 的钩子（如命令中包含 `gortex hook` 或专属标记）。
//   2. 绝对严禁误删或篡改用户自定义或其他第三方工具配置的任何钩子内容！
//   3. 若目标事件数组或钩子容器包含其他有效配置，必须 100% 完整保留。
// 废弃计划：
//   待存量环境全量升级迁移完毕后，此模块可从项目中直接整块废弃删除。
// ============================================================================

use std::fs;
use std::path::Path;
use serde_json::Value;

use super::ownership::{read_gortex_ownership, write_gortex_ownership};
use super::integrations::gortex_hook_path;

/// 精准清理 Codex config.toml 中的 Gortex 钩子
pub fn clean_codex_hooks(path: &Path) -> Result<bool, String> {
    if !path.is_file() {
        return Ok(false);
    }
    let content = fs::read_to_string(path).map_err(|e| format!("读取 Codex config.toml 失败: {}", e))?;
    if !content.contains("gortex") || !content.contains("[hooks") {
        return Ok(false);
    }

    let mut lines: Vec<String> = Vec::new();
    let mut skipping_hook_block = false;
    let mut modified = false;

    // 按行精细分析 TOML 块结构
    for line in content.lines() {
        let trimmed = line.trim();

        // 识别到新的段落开头
        if trimmed.starts_with('[') {
            if trimmed.starts_with("[[hooks.") {
                skipping_hook_block = false;
            } else if !trimmed.starts_with("[hooks") {
                skipping_hook_block = false;
            }
        }

        // 判定是否是 gortex hook 的 command 行
        if (trimmed.starts_with("command") || trimmed.contains("gortex"))
            && trimmed.contains("gortex")
            && trimmed.contains("hook")
        {
            modified = true;
            skipping_hook_block = true;
            // 回溯剔除刚刚加入的当前 hook 块头
            while let Some(last) = lines.last() {
                let last_trim: &str = last.trim();
                if last_trim.starts_with("[[hooks.") || last_trim.starts_with("type =") || last_trim.starts_with("matcher =") || last_trim.is_empty() {
                    lines.pop();
                } else {
                    break;
                }
            }
            continue;
        }

        if skipping_hook_block {
            if trimmed.is_empty() || trimmed.starts_with("timeout") || trimmed.starts_with("statusMessage") || trimmed.starts_with("type =") {
                continue;
            } else if trimmed.starts_with('[') {
                skipping_hook_block = false;
            }
        }

        if !skipping_hook_block {
            lines.push(line.to_string());
        }
    }

    // 清理可能产生的末尾多余 [hooks] 空段
    let mut in_hooks_section = false;
    let mut has_other_hooks = false;
    for line in &lines {
        let trimmed = line.trim();
        if trimmed == "[hooks]" {
            in_hooks_section = true;
            continue;
        }
        if in_hooks_section {
            if trimmed.starts_with("[[hooks.") {
                has_other_hooks = true;
            } else if trimmed.starts_with('[') {
                in_hooks_section = false;
            }
        }
    }

    let mut cleaned_lines = Vec::new();
    for line in lines {
        if line.trim() == "[hooks]" && !has_other_hooks {
            continue;
        }
        cleaned_lines.push(line);
    }

    if modified {
        fs::write(path, cleaned_lines.join("\r\n")).map_err(|e| format!("写入 Codex config.toml 失败: {}", e))?;
    }

    Ok(modified)
}

/// 精准清理 Claude hooks
pub fn clean_claude_hooks(path: &Path) -> Result<bool, String> {
    if !path.is_file() {
        return Ok(false);
    }
    let content = fs::read_to_string(path).map_err(|e| format!("读取 Claude 配置失败: {}", e))?;
    let mut root: Value = serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}));
    let mut modified = false;

    if let Some(hooks) = root.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        let mut empty_events = Vec::new();
        for (event, entries) in hooks.iter_mut() {
            if let Some(arr) = entries.as_array_mut() {
                let orig_len = arr.len();
                arr.retain(|item| {
                    if let Some(cmd) = item.get("command").and_then(|c| c.as_str()) {
                        let lower = cmd.to_ascii_lowercase();
                        if lower.contains("gortex") && lower.contains("hook") {
                            return false; // 仅剔除 Gortex hook
                        }
                    }
                    true // 保留其他第三方 hook
                });
                if arr.len() != orig_len {
                    modified = true;
                }
                if arr.is_empty() {
                    empty_events.push(event.clone());
                }
            }
        }
        for ev in empty_events {
            hooks.remove(&ev);
        }
        if hooks.is_empty() {
            root.as_object_mut().map(|obj| obj.remove("hooks"));
        }
    }

    if modified {
        let data = serde_json::to_string_pretty(&root).unwrap_or_default();
        fs::write(path, format!("{}\n", data)).map_err(|e| format!("写入 Claude 配置失败: {}", e))?;
    }
    Ok(modified)
}

/// 精准清理 Copilot hooks
pub fn clean_copilot_hooks(path: &Path) -> Result<bool, String> {
    if !path.is_file() {
        return Ok(false);
    }
    let content = fs::read_to_string(path).map_err(|e| format!("读取 Copilot 配置失败: {}", e))?;
    let mut root: Value = serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}));
    let mut modified = false;

    if let Some(hooks) = root.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        let mut empty_events = Vec::new();
        for (event, val) in hooks.iter_mut() {
            let is_gortex = if let Some(obj) = val.as_object() {
                let bash = obj.get("bash").and_then(|s| s.as_str()).unwrap_or("");
                let ps = obj.get("powershell").and_then(|s| s.as_str()).unwrap_or("");
                (bash.contains("gortex") && bash.contains("hook")) || (ps.contains("gortex") && ps.contains("hook"))
            } else {
                false
            };
            if is_gortex {
                empty_events.push(event.clone());
                modified = true;
            }
        }
        for ev in empty_events {
            hooks.remove(&ev);
        }
    }

    if modified {
        if let Some(obj) = root.as_object_mut() {
            if let Some(h) = obj.get("hooks").and_then(|h| h.as_object()) {
                if h.is_empty() {
                    obj.remove("hooks");
                }
            }
            if obj.is_empty() || (obj.len() <= 2 && obj.contains_key("version") && obj.contains_key("disableAllHooks")) {
                let _ = fs::remove_file(path);
                return Ok(true);
            }
        }
        let data = serde_json::to_string_pretty(&root).unwrap_or_default();
        fs::write(path, format!("{}\n", data)).map_err(|e| format!("写入 Copilot 配置失败: {}", e))?;
    }
    Ok(modified)
}

/// 精准清理 OpenCode plugin 脚本
pub fn clean_opencode_plugin(path: &Path) -> Result<bool, String> {
    if !path.is_file() {
        return Ok(false);
    }
    let content = fs::read_to_string(path).unwrap_or_default();
    if content.starts_with("// code-manager:gortex:opencode-plugin") || (content.contains("gortex") && content.contains("hook")) {
        let _ = fs::remove_file(path);
        return Ok(true);
    }
    Ok(false)
}

/// 精准清理 Gemini settings.json 中的 hooks
pub fn clean_gemini_hooks(path: &Path) -> Result<bool, String> {
    if !path.is_file() {
        return Ok(false);
    }
    let content = fs::read_to_string(path).map_err(|e| format!("读取 Gemini 配置失败: {}", e))?;
    let mut root: Value = serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}));
    let mut modified = false;

    if let Some(hooks) = root.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        let mut empty_events = Vec::new();
        for (event, group_list) in hooks.iter_mut() {
            if let Some(groups) = group_list.as_array_mut() {
                let orig_len = groups.len();
                groups.retain(|grp| {
                    if let Some(inner_hooks) = grp.get("hooks").and_then(|h| h.as_array()) {
                        for h in inner_hooks {
                            let name = h.get("name").and_then(|n| n.as_str()).unwrap_or("");
                            let cmd = h.get("command").and_then(|c| c.as_str()).unwrap_or("");
                            if name.eq_ignore_ascii_case("gortex") || (cmd.contains("gortex") && cmd.contains("hook")) {
                                return false; // 剔除 Gortex hook
                            }
                        }
                    }
                    true // 保留其他自定义 hook
                });
                if groups.len() != orig_len {
                    modified = true;
                }
                if groups.is_empty() {
                    empty_events.push(event.clone());
                }
            }
        }
        for ev in empty_events {
            hooks.remove(&ev);
        }
        if hooks.is_empty() {
            root.as_object_mut().map(|obj| obj.remove("hooks"));
        }
    }

    if modified {
        let data = serde_json::to_string_pretty(&root).unwrap_or_default();
        fs::write(path, format!("{}\n", data)).map_err(|e| format!("写入 Gemini 配置失败: {}", e))?;
    }
    Ok(modified)
}

/// 集中调度全平台旧版 Gortex Hook 清理，并清理 ownership 中的遗留记录
pub fn clean_all_legacy_gortex_hooks(install_root: &Path) -> Vec<String> {
    let mut logs = Vec::new();

    // 1. 各平台配置文件外科手术式清理
    if let Some(p) = gortex_hook_path("codex") {
        match clean_codex_hooks(&p) {
            Ok(true) => logs.push("已精准清除 Codex 中的旧版 Gortex Hook (用户其它 Hook 已完好保留)".to_string()),
            Err(e) => logs.push(format!("清理 Codex Hook 失败: {}", e)),
            _ => {}
        }
    }
    if let Some(p) = gortex_hook_path("claude") {
        match clean_claude_hooks(&p) {
            Ok(true) => logs.push("已精准清除 Claude 中的旧版 Gortex Hook (用户其它 Hook 已完好保留)".to_string()),
            Err(e) => logs.push(format!("清理 Claude Hook 失败: {}", e)),
            _ => {}
        }
    }
    if let Some(p) = gortex_hook_path("copilot") {
        match clean_copilot_hooks(&p) {
            Ok(true) => logs.push("已精准清除 Copilot 中的旧版 Gortex Hook".to_string()),
            Err(e) => logs.push(format!("清理 Copilot Hook 失败: {}", e)),
            _ => {}
        }
    }
    if let Some(p) = gortex_hook_path("opencode") {
        match clean_opencode_plugin(&p) {
            Ok(true) => logs.push("已清理 OpenCode 旧版 Gortex Plugin".to_string()),
            Err(e) => logs.push(format!("清理 OpenCode Plugin 失败: {}", e)),
            _ => {}
        }
    }
    if let Some(p) = gortex_hook_path("antigravity") {
        match clean_gemini_hooks(&p) {
            Ok(true) => logs.push("已精准清除 Google Antigravity 中的旧版 Gortex Hook".to_string()),
            Err(e) => logs.push(format!("清理 Antigravity Hook 失败: {}", e)),
            _ => {}
        }
    }
    if let Some(p) = gortex_hook_path("gemini") {
        match clean_gemini_hooks(&p) {
            Ok(true) => logs.push("已精准清除 Gemini 中的旧版 Gortex Hook".to_string()),
            Err(e) => logs.push(format!("清理 Gemini Hook 失败: {}", e)),
            _ => {}
        }
    }

    // 2. 清除 ownership ledger 中的 hook: 记录
    if let Ok(mut ownership) = read_gortex_ownership(install_root) {
        let orig_len = ownership.artifacts.len();
        ownership.artifacts.retain(|k, _| !k.starts_with("hook:") && !k.contains(":hook:"));
        if ownership.artifacts.len() != orig_len {
            let _ = write_gortex_ownership(install_root, &ownership);
            logs.push("已同步更新所有权记账清单，移除了旧版 Hook 指纹记录".to_string());
        }
    }

    logs
}

/// 精准清理指定平台的旧版 Gortex Hook
pub fn clean_legacy_gortex_hooks_for_agent(agent: &str, install_root: &Path) {
    match agent {
        "codex" => {
            if let Some(p) = gortex_hook_path("codex") {
                let _ = clean_codex_hooks(&p);
            }
        }
        "claude" => {
            if let Some(p) = gortex_hook_path("claude") {
                let _ = clean_claude_hooks(&p);
            }
        }
        "copilot" => {
            if let Some(p) = gortex_hook_path("copilot") {
                let _ = clean_copilot_hooks(&p);
            }
        }
        "opencode" => {
            if let Some(p) = gortex_hook_path("opencode") {
                let _ = clean_opencode_plugin(&p);
            }
        }
        "antigravity" | "gemini" => {
            if let Some(p) = gortex_hook_path(agent) {
                let _ = clean_gemini_hooks(&p);
            }
        }
        _ => {}
    }

    if let Ok(mut ownership) = read_gortex_ownership(install_root) {
        let prefix = format!("hook:{}:", agent);
        ownership.artifacts.retain(|k, _| !k.starts_with(&prefix) && !k.starts_with("hook:"));
        let _ = write_gortex_ownership(install_root, &ownership);
    }
}
