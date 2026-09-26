use hex;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::hook_config::{
    snip_agent_directories, snip_agent_hook_file,
    snip_hook_command_targets_agent,
};
use super::types::{
    SnipAgentState, SnipCodexTrustInfo, SNIP_TRUST_DISABLED, SNIP_TRUST_NOT_APPLICABLE,
    SNIP_TRUST_TRUSTED, SNIP_TRUST_UNKNOWN, SNIP_TRUST_UNTRUSTED,
};
use super::windows::launch_visible_powershell;
use crate::common::process::new_silent_command;
use crate::common::windows::{
    canonicalize_clean, clean_path_str, get_user_profile_dir, strip_windows_verbatim_prefix,
};

const SNIP_MINIMUM_CODEX_MAJOR: usize = 0;
const SNIP_MINIMUM_CODEX_MINOR: usize = 131;
const SNIP_MINIMUM_CODEX_PATCH: usize = 0;

const CODEX_DEFAULT_HOOK_TIMEOUT: u64 = 600;
const CODEX_DEFAULT_CONTEXT_LIMIT: u64 = 2500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodexCLIVersion {
    pub major: usize,
    pub minor: usize,
    pub patch: usize,
}

impl CodexCLIVersion {
    pub fn at_least(&self, minimum: CodexCLIVersion) -> bool {
        if self.major != minimum.major {
            return self.major > minimum.major;
        }
        if self.minor != minimum.minor {
            return self.minor > minimum.minor;
        }
        self.patch >= minimum.patch
    }
}

pub fn parse_semver_three_parts(text: &str) -> Result<(usize, usize, usize), String> {
    for part in text.split_whitespace() {
        let trimmed = part.trim_start_matches(|c: char| !c.is_ascii_digit());
        let pieces: Vec<&str> = trimmed.split('.').collect();
        if pieces.len() >= 3 {
            let major = pieces[0].parse::<usize>().ok();
            let minor = pieces[1].parse::<usize>().ok();
            let patch_str = pieces[2].split(|c: char| !c.is_ascii_digit()).next().unwrap_or("");
            let patch = patch_str.parse::<usize>().ok();
            if let (Some(maj), Some(min), Some(pat)) = (major, minor, patch) {
                return Ok((maj, min, pat));
            }
        }
    }
    for line in text.lines() {
        let trimmed = line.trim();
        let pieces: Vec<&str> = trimmed.split('.').collect();
        if pieces.len() >= 3 {
            let first = pieces[0].trim_start_matches(|c: char| !c.is_ascii_digit());
            if let (Ok(maj), Ok(min)) = (first.parse::<usize>(), pieces[1].parse::<usize>()) {
                let pat_str = pieces[2].split(|c: char| !c.is_ascii_digit()).next().unwrap_or("");
                if let Ok(pat) = pat_str.parse::<usize>() {
                    return Ok((maj, min, pat));
                }
            }
        }
    }
    Err(format!("未识别到 x.y.z 版本号: {}", text))
}

pub fn parse_codex_cli_version(text: &str) -> Result<CodexCLIVersion, String> {
    let (major, minor, patch) = parse_semver_three_parts(text)?;
    Ok(CodexCLIVersion {
        major,
        minor,
        patch,
    })
}

pub fn locate_codex_executable() -> Result<PathBuf, String> {
    // 1. Check in PATH
    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            for name in &["codex.exe", "codex.cmd", "codex"] {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    let canonical = canonicalize_clean(&candidate)
                        .unwrap_or_else(|_| strip_windows_verbatim_prefix(&candidate));
                    return Ok(canonical);
                }
            }
        }
    }

    // 2. Scan %LOCALAPPDATA%\OpenAI\Codex\bin\*\codex.exe
    let local_appdata = match std::env::var("LOCALAPPDATA") {
        Ok(v) if !v.trim().is_empty() => PathBuf::from(v),
        _ => get_user_profile_dir().join("AppData").join("Local"),
    };

    let bin_dir = local_appdata.join("OpenAI").join("Codex").join("bin");
    if bin_dir.is_dir() {
        let mut candidates = Vec::new();
        if let Ok(entries) = fs::read_dir(&bin_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    let exe = p.join("codex.exe");
                    if exe.is_file() {
                        if let Ok(meta) = fs::metadata(&exe) {
                            let mod_time = meta
                                .modified()
                                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                            candidates.push((exe, mod_time));
                        }
                    }
                }
            }
        }
        if !candidates.is_empty() {
            candidates.sort_by(|a, b| b.1.cmp(&a.1));
            return Ok(candidates[0].0.clone());
        }
    }

    Err("未找到 codex.exe，请先安装 Codex CLI".to_string())
}

pub fn ensure_snip_codex_runtime_hook_supported(agents: &[SnipAgentState]) -> Result<(), String> {
    if !agents.iter().any(|a| a.name == "codex") {
        return Ok(());
    }
    let codex_path = locate_codex_executable()
        .map_err(|e| format!("检测到 Codex 用户目录，但无法定位 Codex CLI 以确认 Hook 兼容性: {}", e))?;

    let output = new_silent_command(&codex_path)
        .arg("--version")
        .output()
        .map_err(|e| format!("无法读取 Codex CLI 版本 ({}): {}", codex_path.display(), e))?;

    let text = String::from_utf8_lossy(&output.stdout);
    let version = parse_codex_cli_version(&text)
        .map_err(|e| format!("无法识别 Codex CLI 版本 ({}): {}", text.trim(), e))?;

    let minimum = CodexCLIVersion {
        major: SNIP_MINIMUM_CODEX_MAJOR,
        minor: SNIP_MINIMUM_CODEX_MINOR,
        patch: SNIP_MINIMUM_CODEX_PATCH,
    };

    if !version.at_least(minimum) {
        return Err(format!(
            "Codex CLI {}.{}.{} 低于 Snip 原生 Hook 所需的 {}.{}.{}，请先升级 Codex 后再启动 Snip",
            version.major, version.minor, version.patch, minimum.major, minimum.minor, minimum.patch
        ));
    }

    Ok(())
}

#[derive(Debug, Clone)]
pub struct CodexSnipHookIdentity {
    pub group_index: usize,
    pub handler_index: usize,
    pub matcher: Option<String>,
    pub command: String,
    pub timeout: u64,
    pub async_handler: bool,
    pub status_message: Option<String>,
    pub additional_context_limit: Option<u64>,
}

pub fn codex_snip_hook_identities(data: &[u8]) -> Result<Vec<CodexSnipHookIdentity>, String> {
    let config: Value = serde_json::from_slice(data)
        .map_err(|e| format!("解析 hooks.json 失败: {}", e))?;

    let hooks = match config.get("hooks").and_then(|h| h.as_object()) {
        Some(h) => h,
        None => return Ok(Vec::new()),
    };

    let raw_groups = match hooks.get("PreToolUse").and_then(|g| g.as_array()) {
        Some(g) => g,
        None => return Ok(Vec::new()),
    };

    let mut identities = Vec::new();
    for (group_idx, raw_group) in raw_groups.iter().enumerate() {
        let group = match raw_group.as_object() {
            Some(g) => g,
            None => continue,
        };

        let matcher = group.get("matcher").and_then(|m| m.as_str()).map(|s| s.to_string());
        let raw_handlers = match group.get("hooks").and_then(|h| h.as_array()) {
            Some(h) => h,
            None => continue,
        };

        for (handler_idx, raw_handler) in raw_handlers.iter().enumerate() {
            let handler = match raw_handler.as_object() {
                Some(h) => h,
                None => continue,
            };

            if handler.get("type").and_then(|t| t.as_str()) != Some("command") {
                continue;
            }

            let mut command = match handler.get("command").and_then(|c| c.as_str()) {
                Some(c) => c.to_string(),
                None => continue,
            };

            if let Some(cmd_win) = handler.get("commandWindows").and_then(|c| c.as_str()) {
                if !cmd_win.trim().is_empty() {
                    command = cmd_win.to_string();
                }
            }

            if command.trim().is_empty() || !snip_hook_command_targets_agent(&command, "codex") {
                continue;
            }

            let timeout = handler
                .get("timeout")
                .and_then(|t| t.as_u64())
                .unwrap_or(CODEX_DEFAULT_HOOK_TIMEOUT);

            let timeout = if timeout == 0 { 1 } else { timeout };

            let async_handler = handler
                .get("async")
                .and_then(|a| a.as_bool())
                .unwrap_or(false);

            let status_message = handler
                .get("statusMessage")
                .and_then(|s| s.as_str())
                .map(|s| s.to_string());

            let additional_context_limit = handler
                .get("additionalContextLimit")
                .and_then(|l| l.as_u64())
                .and_then(|l| if l == CODEX_DEFAULT_CONTEXT_LIMIT { None } else { Some(l) });

            identities.push(CodexSnipHookIdentity {
                group_index: group_idx,
                handler_index: handler_idx,
                matcher: matcher.clone(),
                command,
                timeout,
                async_handler,
                status_message,
                additional_context_limit,
            });
        }
    }

    Ok(identities)
}

pub fn codex_snip_hook_hash(identity: &CodexSnipHookIdentity) -> Result<String, String> {
    // In Go, map keys are alphabetically ordered when serialized:
    // Inside handler:
    // "additionalContextLimit" (if present), "async", "command", "statusMessage" (if present), "timeout", "type"
    let mut handler = BTreeMap::new();
    if let Some(limit) = identity.additional_context_limit {
        handler.insert("additionalContextLimit".to_string(), Value::from(limit));
    }
    handler.insert("async".to_string(), Value::from(identity.async_handler));
    handler.insert("command".to_string(), Value::from(identity.command.clone()));
    if let Some(status) = &identity.status_message {
        handler.insert("statusMessage".to_string(), Value::from(status.clone()));
    }
    handler.insert("timeout".to_string(), Value::from(identity.timeout));
    handler.insert("type".to_string(), Value::from("command"));

    // In value:
    // "event_name", "hooks", "matcher" (if present)
    let mut value = BTreeMap::new();
    value.insert("event_name".to_string(), Value::from("pre_tool_use"));
    value.insert("hooks".to_string(), Value::Array(vec![Value::Object(Map::from_iter(handler))]));
    if let Some(m) = &identity.matcher {
        value.insert("matcher".to_string(), Value::from(m.clone()));
    }

    let serialized = serde_json::to_vec(&Value::Object(Map::from_iter(value)))
        .map_err(|e| format!("序列化 Hook 哈希结构失败: {}", e))?;

    let mut hasher = Sha256::new();
    hasher.update(&serialized);
    Ok(format!("sha256:{}", hex::encode(hasher.finalize())))
}

pub fn normalize_trust_path(value: &str) -> String {
    let s = value.replace("\\\\", "\\").replace('/', "\\");
    s.trim().to_ascii_lowercase()
}

pub fn find_codex_feature_hook_toggle(text: &str, want_value: bool) -> Option<String> {
    let mut current_section = "";
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            current_section = trimmed.trim_start_matches('[').trim_end_matches(']').trim();
            continue;
        }
        if current_section != "features" {
            continue;
        }
        if let Some((k, v)) = trimmed.split_once('=') {
            let key = k.trim();
            if key == "hooks" || key == "codex_hooks" {
                let val_part = v.split('#').next().unwrap_or("").trim();
                if (val_part == "true") == want_value {
                    return Some(key.to_string());
                }
            }
        }
    }
    None
}

pub fn codex_trusted_hash_from_toml(
    config_text: &str,
    hook_path: &Path,
    group_index: usize,
    handler_index: usize,
) -> Option<String> {
    let suffix = format!(":pre_tool_use:{}:{}", group_index, handler_index);
    let target_norm = normalize_trust_path(&hook_path.to_string_lossy());

    let mut inside_matching_state = false;

    for line in config_text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let section = trimmed.trim_start_matches('[').trim_end_matches(']').trim();
            if let Some(sub) = section.strip_prefix("hooks.state.") {
                let clean_key = sub.trim().trim_matches(['\'', '"']);
                if clean_key.ends_with(&suffix) {
                    let source = clean_key.strip_suffix(&suffix).unwrap_or("");
                    if normalize_trust_path(source) == target_norm {
                        inside_matching_state = true;
                        continue;
                    }
                }
            }
            inside_matching_state = false;
            continue;
        }

        if inside_matching_state {
            if let Some((k, v)) = trimmed.split_once('=') {
                if k.trim() == "trusted_hash" {
                    let val = v.split('#').next().unwrap_or("").trim().trim_matches(['"', '\'']);
                    return Some(val.to_string());
                }
            }
        }
    }
    None
}

pub fn codex_snip_hooks_trusted(
    config_text: &str,
    hook_path: &Path,
    identities: &[CodexSnipHookIdentity],
) -> Result<(bool, bool), String> {
    if identities.is_empty() {
        return Ok((false, false));
    }

    let mut trusted = true;
    let mut modified = false;

    for identity in identities {
        let current_hash = codex_snip_hook_hash(identity)?;
        let stored_hash = codex_trusted_hash_from_toml(
            config_text,
            hook_path,
            identity.group_index,
            identity.handler_index,
        );

        match stored_hash {
            Some(stored) if !stored.trim().is_empty() => {
                if stored != current_hash {
                    trusted = false;
                    modified = true;
                }
            }
            _ => {
                trusted = false;
            }
        }
    }

    Ok((trusted, modified))
}

pub fn codex_trust_steps() -> Vec<String> {
    vec![
        "等待 PowerShell 中的 Codex CLI 界面显示 Hooks need review。不要输入 /hook 或 /hooks。".to_string(),
        "在当前 PowerShell 窗口选择第 2 项 Trust all and continue；不是输入数字 2。".to_string(),
        "按键盘 Enter（回车）确认；不需要先进入 Review hooks，也不需要输入 t。".to_string(),
        "回到 code-Manager 点击“刷新状态”，确认 Codex Hook 显示“已信任”。".to_string(),
    ]
}

pub fn detect_snip_codex_trust() -> Result<SnipCodexTrustInfo, String> {
    let directories = snip_agent_directories();
    let codex_dir = match directories.get("codex") {
        Some(d) => d,
        None => {
            return Ok(SnipCodexTrustInfo {
                status: SNIP_TRUST_NOT_APPLICABLE.to_string(),
                hook_path: String::new(),
                codex_path: String::new(),
                command: String::new(),
                notice: String::new(),
                steps: codex_trust_steps(),
                shell_opened: false,
            });
        }
    };

    let hook_path = snip_agent_hook_file("codex", codex_dir);
    let mut info = SnipCodexTrustInfo {
        status: SNIP_TRUST_NOT_APPLICABLE.to_string(),
        hook_path: hook_path.to_string_lossy().to_string(),
        codex_path: String::new(),
        command: String::new(),
        notice: String::new(),
        steps: codex_trust_steps(),
        shell_opened: false,
    };

    if !hook_path.exists() {
        return Ok(info);
    }

    let hook_data = match fs::read(&hook_path) {
        Ok(d) => d,
        Err(_e) => {
            info.status = SNIP_TRUST_UNKNOWN.to_string();
            info.notice = "无法读取 Codex Snip Hook 文件，请修复后再进行信任审核。".to_string();
            return Ok(info);
        }
    };

    let identities = match codex_snip_hook_identities(&hook_data) {
        Ok(i) => i,
        Err(_) => {
            info.status = SNIP_TRUST_UNKNOWN.to_string();
            info.notice = "无法解析 Codex Snip Hook 文件，请修复 hooks.json 后再进行信任审核。".to_string();
            return Ok(info);
        }
    };

    if identities.is_empty() {
        return Ok(info);
    }

    info.status = SNIP_TRUST_UNTRUSTED.to_string();
    info.notice = "Codex Hook 尚未检测到持久信任。请点击“添加信任”，在 PowerShell 审核界面选择第 2 项，然后按键盘 Enter（回车）。".to_string();

    if let Ok(codex_path) = locate_codex_executable() {
        info.codex_path = codex_path.to_string_lossy().to_string();
        if let Ok((cmd, _)) = codex_trust_command(&codex_path) {
            info.command = cmd;
        }
    }

    let config_path = codex_dir.join("config.toml");
    if !config_path.exists() {
        return Ok(info);
    }

    let config_text = match fs::read_to_string(&config_path) {
        Ok(t) => t,
        Err(_) => {
            info.status = SNIP_TRUST_UNKNOWN.to_string();
            info.notice = "无法读取 Codex 信任记录，请点击“添加信任”，并在 PowerShell 的 Hooks need review 界面选择第 2 项后按键盘 Enter（回车）。".to_string();
            return Ok(info);
        }
    };

    if let Some(_) = find_codex_feature_hook_toggle(&config_text, false) {
        info.status = SNIP_TRUST_DISABLED.to_string();
        info.notice = "Codex Hook 已明确关闭，请先在 Codex 配置中开启，再进行信任审核。".to_string();
        return Ok(info);
    }

    match codex_snip_hooks_trusted(&config_text, &hook_path, &identities) {
        Ok((trusted, modified)) => {
            if trusted {
                info.status = SNIP_TRUST_TRUSTED.to_string();
                info.notice = "Codex Hook 已检测到持久信任。".to_string();
            } else if modified {
                info.notice = "Codex Snip Hook 内容已变化，原有 trusted_hash 不再匹配；请点击“添加信任”重新审核。".to_string();
            }
        }
        Err(_) => {
            info.status = SNIP_TRUST_UNKNOWN.to_string();
            info.notice = "无法读取 Codex 信任记录，请点击“添加信任”，并在 PowerShell 的 Hooks need review 界面选择第 2 项后按键盘 Enter（回车）。".to_string();
        }
    }

    Ok(info)
}

pub fn codex_trust_command(codex_path: &Path) -> Result<(String, PathBuf), String> {
    let project_dir = if let Ok(exe) = std::env::current_exe() {
        exe.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."))
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    };
    let project_dir_abs = canonicalize_clean(&project_dir)
        .unwrap_or_else(|_| strip_windows_verbatim_prefix(&project_dir));
    let clean_project_dir = clean_path_str(&project_dir_abs.to_string_lossy());
    let clean_codex = clean_path_str(&codex_path.to_string_lossy());

    let cmd = format!(
        "& '{}' -C '{}'",
        clean_codex.replace('\'', "''"),
        clean_project_dir.replace('\'', "''")
    );
    Ok((cmd, PathBuf::from(clean_project_dir)))
}

pub fn open_codex_trust_shell(info: &mut SnipCodexTrustInfo) -> Result<(), String> {
    let codex_path = locate_codex_executable()?;
    let (cmd, workdir) = codex_trust_command(&codex_path)?;
    launch_visible_powershell(&cmd, &workdir)?;
    info.codex_path = codex_path.to_string_lossy().to_string();
    info.command = cmd;
    info.shell_opened = true;
    Ok(())
}
