use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use crate::common::windows::get_user_profile_dir;

pub const RTK_COMMAND_BLOCK_START: &str = "---RTK命令_开始---";
pub const RTK_COMMAND_BLOCK_END: &str = "---RTK命令_结束---";

pub const RTK_CODEX_COMMANDS: &str = include_str!("../../../assets/RTK-Codex-commands.md");
pub const RTK_CODEX_AGENT_INSTRUCTIONS: &str = include_str!("../../../assets/RTK-Codex-agent-instructions.md");
pub const RTK_CLAUDE_AGENT_INSTRUCTIONS: &str = include_str!("../../../assets/RTK-Claude-agent-instructions.md");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssistantKind {
    Codex,
    ClaudeCode,
}

#[derive(Debug, Clone)]
pub struct AssistantTarget {
    pub kind: AssistantKind,
    pub agent_name: &'static str,
    pub snapshot_key: &'static str,
    pub file_path: PathBuf,
    pub create_if_parent_exists: bool,
}

#[derive(Debug, Clone, Default)]
pub struct AssistantFileState {
    pub exists: bool,
    pub block_count: usize,
    pub malformed: bool,
    pub matching_current: bool,
    pub legacy_managed: bool,
    pub unmanaged: bool,
}

pub fn normalize_block_body(value: &str) -> String {
    let replaced = value.replace("\r\n", "\n").replace('\r', "\n");
    replaced.trim_end_matches('\n').to_string()
}

pub fn prompt_fingerprint(payload: &str) -> String {
    let normalized = normalize_block_body(payload);
    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn get_assistant_payload(kind: AssistantKind) -> &'static str {
    match kind {
        AssistantKind::Codex => RTK_CODEX_AGENT_INSTRUCTIONS,
        AssistantKind::ClaudeCode => RTK_CLAUDE_AGENT_INSTRUCTIONS,
    }
}

pub fn discover_assistant_targets() -> Vec<AssistantTarget> {
    let user_profile = get_user_profile_dir();

    let codex_dir = if let Ok(val) = std::env::var("CODEX_HOME") {
        if !val.trim().is_empty() {
            PathBuf::from(val.trim())
        } else {
            user_profile.join(".codex")
        }
    } else {
        user_profile.join(".codex")
    };

    let claude_dir = if let Ok(val) = std::env::var("CLAUDE_CONFIG_DIR") {
        if !val.trim().is_empty() {
            PathBuf::from(val.trim())
        } else {
            user_profile.join(".claude")
        }
    } else {
        user_profile.join(".claude")
    };

    vec![
        AssistantTarget {
            kind: AssistantKind::Codex,
            agent_name: "codex",
            snapshot_key: "codex",
            file_path: codex_dir.join("AGENTS.md"),
            create_if_parent_exists: false,
        },
        AssistantTarget {
            kind: AssistantKind::ClaudeCode,
            agent_name: "claude-code",
            snapshot_key: "claude-code-prompt",
            file_path: claude_dir.join("CLAUDE.md"),
            create_if_parent_exists: true,
        },
    ]
}

pub fn assistant_prompt_block_matches(kind: AssistantKind, body: &str) -> bool {
    let payload = get_assistant_payload(kind);
    if normalize_block_body(body) == normalize_block_body(payload) {
        return true;
    }
    assistant_prompt_block_looks_legacy(kind, body)
}

pub fn assistant_prompt_block_looks_legacy(kind: AssistantKind, body: &str) -> bool {
    let normalized = normalize_block_body(body);
    match kind {
        AssistantKind::Codex => {
            normalized.starts_with("# RTK：Codex 常驻高密度命令规则")
                || normalized.starts_with("# RTK: Codex 常驻高密度命令规则")
                || normalized.starts_with("# RTK：Codex 执行规则、源码审计与完整命令参考")
                || normalized.starts_with("# RTK: Codex 执行规则、源码审计与完整命令参考")
                || normalized.starts_with("# RTK - Rust Token Killer (Codex CLI)")
        }
        AssistantKind::ClaudeCode => {
            normalized.contains("Claude Code 的 `PreToolUse` Hook") && normalized.contains("`rtk`")
        }
    }
}

pub fn rtk_command_block_bodies(content: &str) -> Result<Vec<String>, String> {
    let clean = content.trim_start_matches('\u{feff}');
    let mut bodies = Vec::new();
    let mut in_block = false;
    let mut current_body = String::new();

    for line in clean.split_inclusive('\n') {
        let trimmed_marker = line.trim().trim_start_matches('\u{feff}');
        match trimmed_marker {
            RTK_COMMAND_BLOCK_START => {
                if in_block {
                    return Err("RTK 命令标记段嵌套".to_string());
                }
                in_block = true;
                current_body.clear();
            }
            RTK_COMMAND_BLOCK_END => {
                if !in_block {
                    return Err("RTK 命令结束标记缺少开始标记".to_string());
                }
                bodies.push(current_body.clone());
                in_block = false;
            }
            _ => {
                if in_block {
                    current_body.push_str(line);
                }
            }
        }
    }

    if in_block {
        return Err("RTK 命令开始标记缺少结束标记".to_string());
    }

    Ok(bodies)
}

pub fn inspect_assistant_file(target: &AssistantTarget) -> AssistantFileState {
    let mut state = AssistantFileState::default();
    if !target.file_path.exists() {
        return state;
    }
    state.exists = true;

    let content = match fs::read_to_string(&target.file_path) {
        Ok(c) => c,
        Err(_) => return state,
    };

    let bodies = match rtk_command_block_bodies(&content) {
        Ok(b) => b,
        Err(_) => {
            state.malformed = true;
            return state;
        }
    };

    state.block_count = bodies.len();
    let current_payload = get_assistant_payload(target.kind);
    let normalized_current = normalize_block_body(current_payload);

    for body in &bodies {
        let norm = normalize_block_body(body);
        if norm == normalized_current {
            state.matching_current = true;
            continue;
        }
        if assistant_prompt_block_looks_legacy(target.kind, body) {
            state.legacy_managed = true;
            continue;
        }
        state.unmanaged = true;
    }

    state
}

pub fn strip_rtk_command_blocks_where<F>(content: &str, should_remove: F) -> Result<(String, usize), String>
where
    F: Fn(&str) -> bool,
{
    let has_bom = content.starts_with('\u{feff}');
    let clean = content.trim_start_matches('\u{feff}');
    let mut builder = String::new();
    let mut in_block = false;
    let mut removed = 0;
    let mut start_line = String::new();
    let mut block_body = String::new();

    for line in clean.split_inclusive('\n') {
        let trimmed_marker = line.trim().trim_start_matches('\u{feff}');
        match trimmed_marker {
            RTK_COMMAND_BLOCK_START => {
                if in_block {
                    return Err("RTK 命令标记段嵌套".to_string());
                }
                in_block = true;
                start_line = line.to_string();
                block_body.clear();
            }
            RTK_COMMAND_BLOCK_END => {
                if !in_block {
                    return Err("RTK 命令结束标记缺少开始标记".to_string());
                }
                if should_remove(&block_body) {
                    removed += 1;
                } else {
                    builder.push_str(&start_line);
                    builder.push_str(&block_body);
                    builder.push_str(line);
                }
                in_block = false;
            }
            _ => {
                if in_block {
                    block_body.push_str(line);
                } else {
                    builder.push_str(line);
                }
            }
        }
    }

    if in_block {
        return Err("RTK 命令开始标记缺少结束标记".to_string());
    }

    let mut result = builder;
    if has_bom {
        result.insert(0, '\u{feff}');
    }

    Ok((result, removed))
}

pub fn append_rtk_command_block(base: &str, payload: &str) -> String {
    let mut base_text = base.to_string();
    let newline = if base.contains("\r\n") { "\r\n" } else { "\n" };

    if !base_text.is_empty() {
        if !base_text.ends_with('\n') {
            base_text.push_str(newline);
        }
        let double_nl = format!("{}{}", newline, newline);
        if !base_text.ends_with(&double_nl) {
            base_text.push_str(newline);
        }
    }

    let payload_normalized = if newline == "\r\n" {
        payload.replace("\r\n", "\n").replace('\r', "\n").replace('\n', "\r\n")
    } else {
        payload.replace("\r\n", "\n").replace('\r', "\n")
    };

    let mut payload_text = payload_normalized;
    if !payload_text.ends_with(newline) {
        payload_text.push_str(newline);
    }

    format!(
        "{}{}{}{}{}{}",
        base_text, RTK_COMMAND_BLOCK_START, newline, payload_text, RTK_COMMAND_BLOCK_END, newline
    )
}

pub fn upsert_assistant_prompt(target: &AssistantTarget, data: &str) -> Result<(String, bool, bool), String> {
    let bodies = rtk_command_block_bodies(data)?;
    let payload = get_assistant_payload(target.kind);

    if bodies.is_empty() {
        return Ok((append_rtk_command_block(data, payload), true, false));
    }

    let mut known = false;
    for body in &bodies {
        if assistant_prompt_block_matches(target.kind, body) {
            known = true;
            continue;
        }
        return Ok((data.to_string(), false, true));
    }

    if !known {
        return Ok((data.to_string(), false, true));
    }

    let (base, _) = strip_rtk_command_blocks_where(data, |body| assistant_prompt_block_matches(target.kind, body))?;
    Ok((append_rtk_command_block(&base, payload), true, false))
}

pub fn replace_utf8_file(file_path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建父目录失败: {}", e))?;
    }

    let temp_name = format!(".codex-rtk-{}.tmp", chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0));
    let temp_path = file_path.parent().unwrap_or(Path::new(".")).join(temp_name);

    fs::write(&temp_path, content).map_err(|e| format!("写入临时文件失败: {}", e))?;

    let had_existing = file_path.exists();
    let backup_path = if had_existing {
        let b_name = format!(".codex-rtk-backup-{}.tmp", chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0));
        let b_path = file_path.parent().unwrap_or(Path::new(".")).join(b_name);
        if let Err(e) = fs::rename(file_path, &b_path) {
            let _ = fs::remove_file(&temp_path);
            return Err(format!("创建备份文件失败: {}", e));
        }
        Some(b_path)
    } else {
        None
    };

    if let Err(e) = fs::rename(&temp_path, file_path) {
        if let Some(b) = &backup_path {
            let _ = fs::rename(b, file_path);
        }
        let _ = fs::remove_file(&temp_path);
        return Err(format!("替换目标文件失败: {}", e));
    }

    if let Some(b) = backup_path {
        let _ = fs::remove_file(b);
    }

    Ok(())
}

pub fn query_codex_configured() -> bool {
    let targets = discover_assistant_targets();
    for target in targets {
        if target.kind == AssistantKind::Codex {
            if let Ok(content) = fs::read_to_string(&target.file_path) {
                if let Ok(bodies) = rtk_command_block_bodies(&content) {
                    let norm_payload = normalize_block_body(RTK_CODEX_AGENT_INSTRUCTIONS);
                    return bodies.iter().any(|b| normalize_block_body(b) == norm_payload);
                }
            }
        }
    }
    false
}

pub fn query_claude_prompt_configured() -> bool {
    let targets = discover_assistant_targets();
    for target in targets {
        if target.kind == AssistantKind::ClaudeCode {
            if let Ok(content) = fs::read_to_string(&target.file_path) {
                if let Ok(bodies) = rtk_command_block_bodies(&content) {
                    let norm_payload = normalize_block_body(RTK_CLAUDE_AGENT_INSTRUCTIONS);
                    return bodies.iter().any(|b| normalize_block_body(b) == norm_payload);
                }
            }
        }
    }
    false
}

pub fn codex_residual_status() -> (bool, Option<String>) {
    let targets = discover_assistant_targets();
    for target in targets {
        if target.kind == AssistantKind::Codex {
            let state = inspect_assistant_file(&target);
            if state.malformed {
                return (true, Some("Codex AGENTS.md 的 RTK 标记段不完整".to_string()));
            }
            if state.unmanaged {
                return (true, Some("Codex AGENTS.md 存在未受管的 RTK 标记段，已保留且不会自动删除".to_string()));
            }
            if state.legacy_managed {
                return (true, Some("Codex AGENTS.md 存在旧版 RTK 提示词，请点击启动 RTK 迁移".to_string()));
            }
            if state.block_count > 1 {
                return (true, Some("Codex AGENTS.md 存在多个 RTK 命令标记段".to_string()));
            }
            return (false, None);
        }
    }
    (false, None)
}

pub fn claude_prompt_residual_status() -> (bool, Option<String>) {
    let targets = discover_assistant_targets();
    for target in targets {
        if target.kind == AssistantKind::ClaudeCode {
            let state = inspect_assistant_file(&target);
            if state.malformed {
                return (true, Some("Claude Code CLAUDE.md 的 RTK 标记段不完整".to_string()));
            }
            if state.unmanaged {
                return (true, Some("Claude Code CLAUDE.md 存在未受管的 RTK 标记段，已保留且不会自动删除".to_string()));
            }
            if state.legacy_managed {
                return (true, Some("Claude Code CLAUDE.md 存在旧版 RTK 提示词，请点击启动 RTK 迁移".to_string()));
            }
            if state.block_count > 1 {
                return (true, Some("Claude Code CLAUDE.md 存在多个 RTK 命令标记段".to_string()));
            }
            return (false, None);
        }
    }
    (false, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rtk_command_block_parsing_and_upsert() {
        let sample = "# User Header\n\nSome custom user rules\n";
        let target = AssistantTarget {
            kind: AssistantKind::Codex,
            agent_name: "codex",
            snapshot_key: "codex",
            file_path: PathBuf::from("dummy/AGENTS.md"),
            create_if_parent_exists: false,
        };

        let (upserted, managed, unmanaged) = upsert_assistant_prompt(&target, sample).unwrap();
        assert!(managed);
        assert!(!unmanaged);
        assert!(upserted.contains(RTK_COMMAND_BLOCK_START));
        assert!(upserted.contains(RTK_COMMAND_BLOCK_END));
        assert!(upserted.contains("# User Header"));

        let bodies = rtk_command_block_bodies(&upserted).unwrap();
        assert_eq!(bodies.len(), 1);

        let (stripped, removed) = strip_rtk_command_blocks_where(&upserted, |_| true).unwrap();
        assert_eq!(removed, 1);
        assert!(!stripped.contains(RTK_COMMAND_BLOCK_START));
        assert!(stripped.contains("# User Header"));
    }
}
