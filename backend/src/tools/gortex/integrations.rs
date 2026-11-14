use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::ownership::{
    gortex_artifact_fingerprint, gortex_json_artifact_fingerprint, gortex_mcp_entry_looks_managed,
    gortex_mcp_registration_allowed, gortex_opencode_entry_looks_managed, gortex_ownership_key,
    read_gortex_ownership, write_gortex_ownership,
};
use super::types::{GortexCodexTrustInfo, GortexOwnedArtifact, GortexOwnedMCP};
use crate::common::windows::{
    canonicalize_clean, clean_path_str, get_user_profile_dir, launch_visible_powershell,
    strip_windows_verbatim_prefix,
};

pub const GORTEX_RULES_START_MARKER: &str = "<!-- code-manager:gortex:rules:start -->";
pub const GORTEX_RULES_END_MARKER: &str = "<!-- code-manager:gortex:rules:end -->";
pub const GORTEX_ARTIFACT_PROMPT: &str = "prompt:";
pub const GORTEX_ARTIFACT_HOOK: &str = "hook:";
pub const GORTEX_OPENCODE_PLUGIN_MARKER: &str = "// code-manager:gortex:opencode-plugin";
pub const GORTEX_OPENCODE_PLUGIN_BIN_KEY: &str = "{{GORTEX_BIN}}";
pub const GORTEX_OPENCODE_PLUGIN_ARGV_KEY: &str = "{{GORTEX_HOOK_ARGV}}";
pub const GORTEX_OPENCODE_PLUGIN_ENFORCE_KEY: &str = "{{GORTEX_ENFORCE}}";
pub const GORTEX_CURSOR_FRONTMATTER: &str = "---\ndescription: Gortex code intelligence - prefer graph tools over file reads\nalwaysApply: true\n---\n\n";

pub const GORTEX_PROMPT_DOC: &str = include_str!("../../../assets/gortex提示词.md");
pub const GORTEX_OPENCODE_PLUGIN_DOC: &str = include_str!("../../../assets/gortex-opencode-plugin.js");

pub const GORTEX_AGENTS: &[&str] = &[
    "codex",
    "claude",
    "cursor",
    "copilot",
    "opencode",
    "antigravity",
    "gemini",
];

pub fn gortex_absolute_env_path(name: &str) -> Option<PathBuf> {
    if let Ok(val) = std::env::var(name) {
        let cleaned = clean_path_str(&val);
        if !cleaned.is_empty() {
            let p = PathBuf::from(cleaned);
            return canonicalize_clean(&p).ok().or_else(|| Some(strip_windows_verbatim_prefix(&p)));
        }
    }
    None
}

pub fn gortex_claude_config_dir() -> PathBuf {
    if let Some(dir) = gortex_absolute_env_path("CLAUDE_CONFIG_DIR") {
        return dir;
    }
    get_user_profile_dir().join(".claude")
}

pub fn gortex_copilot_config_dir() -> PathBuf {
    if let Some(dir) = gortex_absolute_env_path("COPILOT_HOME") {
        return dir;
    }
    get_user_profile_dir().join(".copilot")
}

pub fn gortex_opencode_config_dir() -> PathBuf {
    if let Some(dir) = gortex_absolute_env_path("XDG_CONFIG_HOME") {
        return dir.join("opencode");
    }
    get_user_profile_dir().join(".config").join("opencode")
}

pub fn gortex_antigravity_config_dir() -> PathBuf {
    get_user_profile_dir().join(".gemini").join("config")
}

pub fn gortex_gemini_config_dir() -> PathBuf {
    get_user_profile_dir().join(".gemini")
}

pub fn gortex_legacy_antigravity_config_path() -> PathBuf {
    get_user_profile_dir().join(".gemini").join("antigravity").join("mcp_config.json")
}

pub fn gortex_config_path(agent: &str) -> Option<PathBuf> {
    let profile = get_user_profile_dir();
    match agent {
        "codex" => Some(profile.join(".codex").join("config.toml")),
        "claude" => {
            if let Some(override_dir) = gortex_absolute_env_path("CLAUDE_CONFIG_DIR") {
                Some(override_dir.join(".claude.json"))
            } else {
                Some(profile.join(".claude.json"))
            }
        }
        "cursor" => Some(profile.join(".cursor").join("mcp.json")),
        "copilot" => Some(gortex_copilot_config_dir().join("mcp-config.json")),
        "opencode" => Some(gortex_opencode_config_dir().join("opencode.json")),
        "antigravity" => Some(gortex_antigravity_config_dir().join("mcp_config.json")),
        "gemini" => Some(gortex_gemini_config_dir().join("settings.json")),
        _ => None,
    }
}

pub fn gortex_prompt_paths(agent: &str) -> Vec<PathBuf> {
    let profile = get_user_profile_dir();
    match agent {
        "codex" => vec![profile.join(".codex").join("AGENTS.md")],
        "claude" => vec![gortex_claude_config_dir().join("CLAUDE.md")],
        "copilot" => vec![gortex_copilot_config_dir().join("copilot-instructions.md")],
        "opencode" => vec![gortex_opencode_config_dir().join("AGENTS.md")],
        "antigravity" | "gemini" => vec![profile.join(".gemini").join("GEMINI.md")],
        _ => Vec::new(),
    }
}

pub fn gortex_cursor_rule_path(project: &str) -> PathBuf {
    Path::new(project).join(".cursor").join("rules").join("gortex-workflow.mdc")
}

pub fn gortex_hook_path(agent: &str) -> Option<PathBuf> {
    let profile = get_user_profile_dir();
    match agent {
        "codex" => Some(profile.join(".codex").join("config.toml")),
        "claude" => Some(gortex_claude_config_dir().join("settings.local.json")),
        "copilot" => Some(gortex_copilot_config_dir().join("hooks").join("gortex.json")),
        "opencode" => Some(gortex_opencode_config_dir().join("plugin").join("gortex.js")),
        "antigravity" | "gemini" => Some(profile.join(".gemini").join("settings.json")),
        _ => None,
    }
}

pub fn gortex_command_available(name: &str) -> bool {
    if crate::common::process::is_process_running(name) {
        return true;
    }
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return true;
            }
            #[cfg(windows)]
            {
                let with_exe = dir.join(format!("{}.exe", name));
                if with_exe.is_file() {
                    return true;
                }
                let with_cmd = dir.join(format!("{}.cmd", name));
                if with_cmd.is_file() {
                    return true;
                }
                let with_bat = dir.join(format!("{}.bat", name));
                if with_bat.is_file() {
                    return true;
                }
            }
        }
    }
    false
}

pub fn gortex_antigravity_installed() -> bool {
    if gortex_command_available("antigravity") {
        return true;
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let p = PathBuf::from(local).join("Programs").join("antigravity").join("Antigravity.exe");
        if p.is_file() {
            return true;
        }
    }
    if let Ok(prog) = std::env::var("ProgramFiles") {
        let p = PathBuf::from(prog).join("Antigravity").join("Antigravity.exe");
        if p.is_file() {
            return true;
        }
    }
    false
}

fn json_config_has_platform_evidence(path: &Path, server_key: &str) -> bool {
    if !path.is_file() {
        return false;
    }
    let data = match fs::read_to_string(path) {
        Ok(d) => d,
        Err(_) => return false,
    };
    let root: Value = match serde_json::from_str(&data) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let obj = match root.as_object() {
        Some(o) => o,
        None => return false,
    };
    if obj.is_empty() {
        return false;
    }
    for (k, v) in obj {
        if k != server_key {
            return true;
        }
        if let Some(servers) = v.as_object() {
            for (name, _) in servers {
                if name != "gortex" {
                    return true;
                }
            }
        }
    }
    false
}

fn gemini_config_has_platform_evidence(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let data = match fs::read_to_string(path) {
        Ok(d) => d,
        Err(_) => return false,
    };
    let root: Value = match serde_json::from_str(&data) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let obj = match root.as_object() {
        Some(o) => o,
        None => return false,
    };
    for (k, v) in obj {
        match k.as_str() {
            "hooks" => continue,
            "mcpServers" => {
                if let Some(servers) = v.as_object() {
                    for (name, _) in servers {
                        if name != "gortex" {
                            return true;
                        }
                    }
                }
            }
            _ => return true,
        }
    }
    false
}

fn toml_config_has_platform_evidence(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let data = match fs::read_to_string(path) {
        Ok(d) => d,
        Err(_) => return false,
    };
    for line in data.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let sec = trimmed.trim_start_matches('[').trim_end_matches(']').trim();
            if !sec.starts_with("mcp_servers.gortex") && !sec.starts_with("hooks.") {
                return true;
            }
        }
    }
    false
}

pub fn gortex_agent_config_evidence(agent: &str) -> bool {
    match agent {
        "codex" => {
            if let Some(p) = gortex_config_path(agent) {
                toml_config_has_platform_evidence(&p)
            } else {
                false
            }
        }
        "opencode" => {
            if let Some(p) = gortex_config_path(agent) {
                json_config_has_platform_evidence(&p, "mcp")
            } else {
                false
            }
        }
        "antigravity" => {
            if let Some(p) = gortex_config_path(agent) {
                json_config_has_platform_evidence(&p, "mcpServers")
                    || json_config_has_platform_evidence(&gortex_legacy_antigravity_config_path(), "mcpServers")
            } else {
                false
            }
        }
        "gemini" => {
            if let Some(p) = gortex_config_path(agent) {
                gemini_config_has_platform_evidence(&p)
            } else {
                false
            }
        }
        _ => {
            if let Some(p) = gortex_config_path(agent) {
                json_config_has_platform_evidence(&p, "mcpServers")
            } else {
                false
            }
        }
    }
}

pub fn gortex_agent_available(agent: &str) -> bool {
    match agent {
        "codex" => gortex_command_available("codex") || gortex_agent_config_evidence(agent),
        "claude" => gortex_command_available("claude") || gortex_agent_config_evidence(agent),
        "cursor" => gortex_command_available("cursor") || gortex_agent_config_evidence(agent),
        "copilot" => gortex_command_available("copilot") || gortex_agent_config_evidence(agent),
        "opencode" => gortex_command_available("opencode") || gortex_agent_config_evidence(agent),
        "antigravity" => gortex_antigravity_installed() || gortex_agent_config_evidence(agent),
        "gemini" => gortex_command_available("gemini") || gortex_agent_config_evidence(agent),
        _ => false,
    }
}

pub fn gortex_detected_mcp_agents() -> BTreeMap<String, bool> {
    let mut map = BTreeMap::new();
    for agent in GORTEX_AGENTS {
        map.insert(agent.to_string(), gortex_agent_available(agent));
    }
    map
}

pub fn gortex_mcp_env(install_root: &Path) -> serde_json::Map<String, Value> {
    let clean_root = strip_windows_verbatim_prefix(install_root);
    let mut env = serde_json::Map::new();
    env.insert("XDG_CONFIG_HOME".to_string(), Value::String(clean_path_str(&clean_root.join("config").to_string_lossy())));
    env.insert("XDG_DATA_HOME".to_string(), Value::String(clean_path_str(&clean_root.join("data").to_string_lossy())));
    env.insert("XDG_CACHE_HOME".to_string(), Value::String(clean_path_str(&clean_root.join("cache").to_string_lossy())));
    env.insert("GORTEX_DAEMON_SOCKET".to_string(), Value::String(clean_path_str(&clean_root.join("run").join("daemon.sock").to_string_lossy())));
    env.insert("GORTEX_DAEMON_PIDFILE".to_string(), Value::String(clean_path_str(&clean_root.join("run").join("daemon.pid").to_string_lossy())));
    env.insert("GORTEX_DAEMON_LOGFILE".to_string(), Value::String(clean_path_str(&clean_root.join("run").join("daemon.log").to_string_lossy())));
    env.insert("GORTEX_DAEMON_STATEFILE".to_string(), Value::String(clean_path_str(&clean_root.join("run").join("daemon.state.json").to_string_lossy())));
    env.insert("GORTEX_INDEX_WORKERS".to_string(), Value::String("8".to_string()));
    env.insert("GORTEX_RECONCILE_INTERVAL".to_string(), Value::String("1h".to_string()));
    env.insert("GORTEX_DAEMON_IDLE_TIMEOUT".to_string(), Value::String("0".to_string()));
    env
}

pub fn gortex_bridge_executable(fallback: &str) -> String {
    if let Ok(override_exe) = std::env::var("CODE_MANAGER_EXECUTABLE") {
        let cleaned = clean_path_str(&override_exe);
        if !cleaned.is_empty() {
            return cleaned;
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        return clean_path_str(&exe.to_string_lossy());
    }
    clean_path_str(fallback)
}

pub fn gortex_bridge_mcp_entry(agent: &str, executable: &str, install_root: &Path) -> Value {
    let bridge_exe = clean_path_str(&gortex_bridge_executable(executable));
    let clean_exe = clean_path_str(executable);

    if agent == "opencode" {
        let mut map = serde_json::Map::new();
        map.insert("type".to_string(), Value::String("local".to_string()));
        map.insert("command".to_string(), Value::Array(vec![
            Value::String(bridge_exe),
            Value::String("gortex-bridge".to_string()),
            Value::String("--agent".to_string()),
            Value::String("opencode".to_string()),
            Value::String("--gortex".to_string()),
            Value::String(clean_exe),
        ]));
        map.insert("enabled".to_string(), Value::Bool(true));
        map.insert("environment".to_string(), Value::Object(gortex_mcp_env(install_root)));
        return Value::Object(map);
    }

    let mut map = serde_json::Map::new();
    map.insert("command".to_string(), Value::String(bridge_exe));
    map.insert("args".to_string(), Value::Array(vec![
        Value::String("gortex-bridge".to_string()),
        Value::String("--agent".to_string()),
        Value::String(agent.to_string()),
        Value::String("--gortex".to_string()),
        Value::String(clean_exe),
    ]));
    map.insert("env".to_string(), Value::Object(gortex_mcp_env(install_root)));

    if agent == "copilot" {
        map.insert("type".to_string(), Value::String("local".to_string()));
    }
    if agent == "codex" {
        map.insert("startup_timeout_sec".to_string(), Value::Number(90.into()));
    }

    Value::Object(map)
}

pub fn gortex_mcp_entry(executable: &str, install_root: &Path, copilot: bool) -> Value {
    let agent = if copilot { "copilot" } else { "default" };
    gortex_bridge_mcp_entry(agent, executable, install_root)
}

pub fn gortex_antigravity_mcp_entry(executable: &str, install_root: &Path) -> Value {
    gortex_bridge_mcp_entry("antigravity", executable, install_root)
}

pub fn gortex_opencode_mcp_entry(executable: &str, install_root: &Path) -> Value {
    gortex_bridge_mcp_entry("opencode", executable, install_root)
}

pub fn gortex_codex_mcp_entry(executable: &str, install_root: &Path) -> Value {
    gortex_bridge_mcp_entry("codex", executable, install_root)
}

pub fn gortex_platform_mcp_entry(agent: &str, executable: &str, install_root: &Path) -> Value {
    gortex_bridge_mcp_entry(agent, executable, install_root)
}

pub fn normalize_gortex_line_endings(val: &str) -> String {
    val.replace("\r\n", "\n").replace('\r', "\n")
}

pub fn gortex_prompt_block(body: &str) -> String {
    let normalized = normalize_gortex_line_endings(body);
    let mut sep = "\n";
    if normalized.ends_with('\n') {
        sep = "";
    }
    format!("{}\n{}{}{}\n", GORTEX_RULES_START_MARKER, normalized, sep, GORTEX_RULES_END_MARKER)
}

pub fn gortex_marked_block(data: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(data);
    let start = text.find(GORTEX_RULES_START_MARKER)?;
    let end = text.rfind(GORTEX_RULES_END_MARKER)?;
    if end < start {
        return None;
    }
    let full_end = end + GORTEX_RULES_END_MARKER.len();
    Some(text[start..full_end].to_string())
}

pub fn upsert_gortex_prompt(path: &Path, body: &str) -> Result<(bool, String), String> {
    let text = if path.is_file() {
        fs::read_to_string(path).map_err(|e| format!("读取提示词文件失败: {}", e))?
    } else {
        String::new()
    };

    let block = gortex_prompt_block(body);
    let start = text.find(GORTEX_RULES_START_MARKER);
    let end_marker = text.rfind(GORTEX_RULES_END_MARKER);

    let next = if let (Some(s), Some(e)) = (start, end_marker) {
        if e >= s {
            let mut end = e + GORTEX_RULES_END_MARKER.len();
            if end < text.len() && text.as_bytes()[end] == b'\n' {
                end += 1;
            }
            format!("{}{}{}", &text[..s], block, &text[end..])
        } else {
            format!("{}\n\n{}", text.trim_end(), block)
        }
    } else if text.trim().is_empty() {
        block.clone()
    } else {
        format!("{}\n\n{}", text.trim_end(), block)
    };

    let fp = gortex_artifact_fingerprint(block.as_bytes());
    if next == text {
        return Ok((false, fp));
    }

    if let Some(p) = path.parent() {
        let _ = fs::create_dir_all(p);
    }
    fs::write(path, next).map_err(|e| format!("写入提示词文件失败: {}", e))?;
    Ok((true, fp))
}

/// 计算文件分片区间（以 chunk_size 行为单位，首尾闭合）
pub fn calculate_prompt_slices(total_lines: usize, chunk_size: usize) -> Vec<String> {
    if total_lines == 0 {
        return vec!["L1-1".to_string()];
    }
    let chunk = if chunk_size == 0 { 300 } else { chunk_size };
    let mut slices = Vec::new();
    let mut start = 1;
    while start <= total_lines {
        let end = (start + chunk - 1).min(total_lines);
        slices.push(format!("L{}-{}", start, end));
        start = end + 1;
    }
    slices
}

/// 获取提示词文件的行数与字节体积（优先从物理文件读取，若物理文件未就绪则回退至编译期内置文本）
pub fn gortex_prompt_file_stats(prompt_file: &Path) -> (usize, u64) {
    if prompt_file.is_file() {
        if let Ok(content) = fs::read_to_string(prompt_file) {
            let lines = content.lines().count();
            let bytes = content.len() as u64;
            return (lines, bytes);
        }
    }
    let lines = GORTEX_PROMPT_DOC.lines().count();
    let bytes = GORTEX_PROMPT_DOC.len() as u64;
    (lines, bytes)
}

/// 生成指向实际物理文件的外挂引导提示词（包含动态统计的行数、体积及顺序分片指引）
pub fn gortex_delegated_prompt_body(prompt_file: &Path) -> String {
    let clean_path = strip_windows_verbatim_prefix(prompt_file)
        .to_string_lossy()
        .replace('/', "\\");
    let (total_lines, total_bytes) = gortex_prompt_file_stats(prompt_file);
    let size_kb = (total_bytes as f64 / 1024.0).ceil() as u64;
    let slices = calculate_prompt_slices(total_lines, 300);
    let slices_str = slices.join("、");
    let (line_summary, single_slice_guidance) = if slices.len() <= 1 {
        (
            format!("共 {} 行", total_lines),
            format!(
                " 文档体积适中，请在第 1 轮单次读取（L1-{}）；返回后须核对实际首末行号是否完整覆盖 1-{}。若报错、偏移错误或被宿主截断未达第 {} 行，严禁跳过，必须立即补齐缺失区间。若确认已完整覆盖全文件，不得重复重读。",
                total_lines, total_lines, total_lines
            ),
        )
    } else {
        (format!("共计 {} 行", total_lines), String::new())
    };

    format!(r#"为方便接下来工作的展开，你必须先全量读取“{clean_path}”（{line_summary}，约 {size_kb} KB），并首先严格遵循文档的规范。

【前置预读与工具权责铁律】：

1. 外部文档只读豁免：“gortex提示词.md”位于代码仓库外部系统目录，属于初始环境法定豁免文件。必须且仅能使用当前平台的【宿主原生只读工具】（如 read_file、view_file、cat 或终端只读命令）读取；严禁调用 Gortex MCP 读取该文件。Gortex MCP 仅索引仓库内文件，访问外部绝对路径会触发安全沙箱拦截。

2. 顺序分片读取与完整性验收：默认按 300 行分片，初始计划为 {slices_str}。{single_slice_guidance}

① 严格串行：每个切片单独发起一次工具调用，必须等待当前调用返回、验收并补全后，才按顺序读取下一片。禁止并发、批量下发，禁止在同一命令、脚本或管道中拼接多个切片；注意区分起止闭区间与起始行加行数，避免偏移。

② 覆盖登记：根据实际返回的连续行号记录已验收范围。调用尚未返回时必须等待，不得重发；不得因显示顺序、界面折叠、Warning、开始分析或调用其他工具而重读已验收范围。同一任务内，文档未变化且内容仍在上下文中时，不得重新全量预读。

③ 仅补缺口：若调用失败、超时、返回为空、行号错位或内容实际缺失，确认原调用结束后，仅补读未验收范围，必要时缩减为 100～150 行。当前切片补齐后才能读取下一片；缩片重试仍无法补齐时，停止并报告失败原因与缺失行号。

④ 完整门禁：确认 L1 至 L{total_lines} 连续完整后，才能调用 Gortex MCP 或分析工程代码。若文档在读取过程中变化，须重新确认当前版本的完整性，不得混用不同版本的内容。

3. 后续代码闭环契约：“gortex提示词.md”全量切片核验完整载入后，对工程项目代码的所有探测、检索、阅读、修改、验证与审查，必须全面切换至 Gortex MCP 原生工具闭环。未完整载入前禁止进入工程代码流程，第 7 条豁免场景除外。

4. 原生工具显式跳出：除第 7 条规定的豁免外，仅当用户输入中明确包含“使用原生工具”显式指令（如环境排查、外部构建与测试等）时，方可单次跳出 Gortex MCP 执行原生操作；完成后立即重返 Gortex 闭环。

5. Gortex 专属 Lazy 模式文件免读契约：鉴于“gortex提示词.md”已全量提供 Gortex 的所有参数契约，平台底座关于“调用 Lazy 工具前必须读取本地 Schema 文件”的通用规则对 Gortex 自动失效并豁免：

① Gortex 绝不读本地 Schema：严禁在调用 Gortex 门面前使用原生工具读取 mcp/gortex/*.json；直接构造参数发起 call_mcp_tool(ServerName="gortex", ...)，消除无效轮次浪费。

② 其他 MCP 工具隔离保护：本免检豁免仅限 ServerName="gortex"。当前及后续对话中，其他非 Gortex 的第三方 MCP 工具不受本条约束，仍须遵循平台默认机制正常执行。

6. 全程编码规范：全程从开始到结束必须使用 UTF-8，从开始读取“gortex提示词.md”就必须使用 UTF-8。若需其他编码，必须由用户明确提出，例如“使用 utf-16”。

7. Git、GitHub 与测试执行调试豁免：

① 豁免优先级：本条是对第 3、4 条及“gortex提示词.md”中原生工具使用限制的明确例外。符合本条时，可直接使用宿主原生工具，无需用户每次额外输入“使用原生工具”；所有操作仍须遵守更高优先级的系统指令和平台权限。

② Git 与 GitHub 操作：Git、GitHub 自身操作，如 Git 备份、提交、分支操作以及 GitHub 发布，直接使用原生工具，不使用 Gortex MCP。若任务仅涉及此类操作且无须分析或编辑工程代码，可以不预读“gortex提示词.md”；若同时涉及代码分析或编辑，相关代码操作须先完成预读并遵守 Gortex 闭环。

③ 测试执行与调试：执行单元测试、集成测试、端到端测试、文档测试、基准测试及覆盖率测试时，直接使用原生工具。豁免包括测试所需的构建、测试环境与依赖检查、测试用例枚举、运行测试框架或调试器，以及查看测试进程的标准输出、错误信息、运行日志、堆栈和测试报告。调试器为设置断点、单步调试或定位故障而读取和展示必要的源码行，属于调试过程。

例如，可按项目实际情况使用 cargo test、go test、pytest、npm test 等原生命令；同一任务中可按需多次运行测试和调试，无需逐次请求额外授权。

④ 测试代码与工程代码的边界：Gortex 已纳管工程中的源码、测试代码、测试配置及维护性测试脚本，无论测试与业务代码写在同一文件中，还是位于独立文件或目录，也无论文件是否已被 Git 跟踪，其检索、阅读、分析、新建、修改和审查均须遵守 Gortex 闭环。

例如，Rust 源文件内的 #[cfg(test)] 模块、独立测试文件及其他语言的测试代码，均按此规则处理。不得借运行测试或调试之名，使用原生工具直接检索、阅读或编辑工程文件；第 ③ 项所述调试器展示必要源码的情况除外。

⑤ 测试产物与代码修改：测试自然生成的构建产物、缓存、临时文件、日志和报告，可由原生测试工具生成和读取。若测试过程改动了项目中受维护的源码、测试代码或配置，应检查这些改动；需要进一步修改时，通过 Gortex 处理，不得借测试命令或脚本变相修改工程代码。

⑥ Gortex 测试分析：Gortex 的 change.tests 等功能用于定位或分析受影响的测试，仍按既有 Gortex 闭环要求使用；静态分析结果不能代替实际执行测试。若某语言、文件或测试符号未被识别，应按 Gortex 实际能力尝试文件级操作并如实报告限制，不得仅因识别不完整而自动改用原生工具读写工程代码；测试运行和调试仍可按第 ③ 项使用原生工具。

⑦ 预读与切换：若任务仅运行或调试已知测试，且无须检索、阅读或修改工程文件，可以不预读“gortex提示词.md”，也不调用 Gortex MCP。若需通过工程文件确定测试对象、测试命令或故障原因，或者需要修改代码、测试及配置，应先完成文档预读，再通过 Gortex 处理相应工程文件。每次完成原生测试或调试操作后，涉及代码的后续工作立即返回 Gortex 闭环。"#)
}

// =========================================================================
// [LEGACY-COMPAT] 旧版提示词兼容清理专区
// -------------------------------------------------------------------------
// 历史背景：
// 在 v1.0.8 及之前版本中，提示词以 24KB 完整文本直接内联写入各平台配置，
// 且旧版 remove_gortex_prompt 依赖严格的 SHA-256 指纹校验，导致用户在
// 升级新版后点击“移除 MCP”时无法清理旧版遗留的内联提示词。
// 此外，部分旧配置可能残留单边未闭合标记或旧格式。
//
// 职责定位：
// 1. 无视 SHA-256 指纹比对，只要含有旧版特征或旧版标记，无条件安全摘除；
// 2. 识别并清理未完全闭合的破损单边标记块（如只有 start 没有 end）；
// 3. 识别旧版特征词并剥离孤立遗留段落。
//
// 废弃规划：
// 待后续版本（如 v1.2.0）用户全部迁移完成后，可将本模块代码及对应调用整体删除。
// =========================================================================
pub mod legacy_prompt_compat {
    use super::*;

    pub const LEGACY_KEYWORD_SYNTAX: &str = "【语法说明】";
    pub const LEGACY_KEYWORD_PRINCIPLE: &str = "Gortex 是对其 track 仓库全生命周期";

    pub fn is_legacy_prompt_content(content: &str) -> bool {
        content.contains(LEGACY_KEYWORD_SYNTAX) || content.contains(LEGACY_KEYWORD_PRINCIPLE)
    }

    pub fn clean_dangling_markers(text: &str) -> (bool, String) {
        let has_start = text.contains(GORTEX_RULES_START_MARKER);
        let has_end = text.contains(GORTEX_RULES_END_MARKER);

        if has_start && !has_end {
            if let Some(idx) = text.find(GORTEX_RULES_START_MARKER) {
                let cleaned = text[..idx].trim_end().to_string();
                let result = if cleaned.is_empty() { String::new() } else { format!("{}\n", cleaned) };
                return (true, result);
            }
        } else if !has_start && has_end {
            if let Some(idx) = text.find(GORTEX_RULES_END_MARKER) {
                let mut after = idx + GORTEX_RULES_END_MARKER.len();
                if after < text.len() && text.as_bytes()[after] == b'\n' {
                    after += 1;
                }
                let cleaned = format!("{}{}", &text[..idx], &text[after..]).trim_end().to_string();
                let result = if cleaned.is_empty() { String::new() } else { format!("{}\n", cleaned) };
                return (true, result);
            }
        }
        (false, text.to_string())
    }

    pub fn cleanup_legacy_prompt_fallback(path: &Path, raw_text: &str) -> Result<bool, String> {
        let (dangling_cleaned, text_after_dangling) = clean_dangling_markers(raw_text);
        if dangling_cleaned {
            if text_after_dangling.trim().is_empty() {
                let _ = fs::remove_file(path);
            } else {
                fs::write(path, text_after_dangling)
                    .map_err(|e| format!("写入清理后的提示词文件失败: {}", e))?;
            }
            return Ok(true);
        }

        if is_legacy_prompt_content(raw_text) && !raw_text.contains(GORTEX_RULES_START_MARKER) {
            if let Some(start_idx) = raw_text.find(LEGACY_KEYWORD_SYNTAX).or_else(|| raw_text.find(LEGACY_KEYWORD_PRINCIPLE)) {
                let remaining = raw_text[..start_idx].trim_end().to_string();
                if remaining.is_empty() {
                    let _ = fs::remove_file(path);
                } else {
                    fs::write(path, format!("{}\n", remaining))
                        .map_err(|e| format!("清理旧版特征提示词失败: {}", e))?;
                }
                return Ok(true);
            }
        }

        Ok(false)
    }
}

pub fn remove_gortex_prompt(path: &Path, _fingerprint: &str) -> Result<bool, String> {
    if !path.is_file() {
        return Ok(false);
    }
    let data = fs::read(path).map_err(|e| format!("读取提示词失败: {}", e))?;
    let text = String::from_utf8_lossy(&data);

    let start = text.find(GORTEX_RULES_START_MARKER);
    let end_marker = text.rfind(GORTEX_RULES_END_MARKER);

    // 标准闭合标记块移除（无论内部是新版外挂引导词还是旧版 24KB 内联长文，解除指纹死锁）
    if let (Some(s), Some(e)) = (start, end_marker) {
        if e >= s {
            let mut end = e + GORTEX_RULES_END_MARKER.len();
            if end < text.len() && text.as_bytes()[end] == b'\n' {
                end += 1;
            }

            let mut next = format!("{}{}", &text[..s], &text[end..]);
            next = next.trim_end_matches('\n').to_string();
            if !next.is_empty() {
                next.push('\n');
            }

            if path.extension().map(|e| e.eq_ignore_ascii_case("mdc")).unwrap_or(false)
                && (next.trim().is_empty()
                    || normalize_gortex_line_endings(&next).trim()
                        == normalize_gortex_line_endings(GORTEX_CURSOR_FRONTMATTER).trim())
            {
                let _ = fs::remove_file(path);
                return Ok(true);
            }

            fs::write(path, next).map_err(|e| format!("更新提示词文件失败: {}", e))?;
            return Ok(true);
        }
    }

    // [LEGACY-COMPAT] 若未能按标准双标记块剥离，调用旧版兼容清理模块进行破损标记与孤立旧文兜底
    legacy_prompt_compat::cleanup_legacy_prompt_fallback(path, &text)
}

pub fn gortex_cursor_frontmatter_info(value: &str) -> (usize, bool, bool) {
    let norm = normalize_gortex_line_endings(value);
    if !norm.starts_with("---\n") {
        return (0, false, false);
    }
    let rest = &norm[4..];
    let sep = match rest.find("\n---\n") {
        Some(idx) => idx,
        None => return (0, true, false),
    };
    let end = 4 + sep + 5;
    let frontmatter = &norm[..end];
    let valid = frontmatter.lines().any(|l| l.trim().eq_ignore_ascii_case("alwaysApply: true"));
    (end, true, valid)
}

pub fn upsert_gortex_cursor_prompt(path: &Path, body: &str) -> Result<(bool, String), String> {
    let text = if path.is_file() {
        fs::read_to_string(path).map_err(|e| format!("读取 Cursor MDC 失败: {}", e))?
    } else {
        String::new()
    };
    let norm_text = normalize_gortex_line_endings(&text);
    let block = gortex_prompt_block(body);
    let start = norm_text.find(GORTEX_RULES_START_MARKER);
    let end_marker = norm_text.rfind(GORTEX_RULES_END_MARKER);

    let next = if let (Some(s), Some(e)) = (start, end_marker) {
        if e >= s {
            let mut end = e + GORTEX_RULES_END_MARKER.len();
            if end < norm_text.len() && norm_text.as_bytes()[end] == b'\n' {
                end += 1;
            }
            let replaced = format!("{}{}{}", &norm_text[..s], block, &norm_text[end..]);
            let (_, present, valid) = gortex_cursor_frontmatter_info(&replaced);
            if !present {
                format!("{}{}", GORTEX_CURSOR_FRONTMATTER, replaced)
            } else if !valid {
                format!("{}{}", GORTEX_CURSOR_FRONTMATTER, &replaced[4..])
            } else {
                replaced
            }
        } else {
            format!("{}{}", GORTEX_CURSOR_FRONTMATTER, block)
        }
    } else if norm_text.trim().is_empty() {
        format!("{}{}", GORTEX_CURSOR_FRONTMATTER, block)
    } else {
        let (front_end, present, valid) = gortex_cursor_frontmatter_info(&norm_text);
        if !present {
            format!("{}{}\n\n{}", GORTEX_CURSOR_FRONTMATTER, block, norm_text)
        } else if !valid {
            format!("{}{}\n\n{}", GORTEX_CURSOR_FRONTMATTER, block, &norm_text[front_end..])
        } else {
            format!("{}{}{}", &norm_text[..front_end], block, &norm_text[front_end..])
        }
    };

    let fp = gortex_artifact_fingerprint(block.as_bytes());
    if next == norm_text {
        return Ok((false, fp));
    }

    if let Some(p) = path.parent() {
        let _ = fs::create_dir_all(p);
    }
    fs::write(path, next).map_err(|e| format!("写入 Cursor MDC 失败: {}", e))?;
    Ok((true, fp))
}

pub fn quote_windows_executable(executable: &str) -> String {
    let clean = executable.trim();
    if clean.contains(' ') {
        format!("\"{}\"", clean)
    } else {
        clean.to_string()
    }
}

pub fn gortex_hook_command(agent: &str, executable: &str) -> String {
    match agent {
        "codex" => format!("{} hook --agent=codex --mode=enrich", quote_windows_executable(executable)),
        "claude" => gortex_claude_hook_command(executable),
        "copilot" => format!("{} hook --agent=copilot-cli", quote_windows_executable(executable)),
        "opencode" => format!("{} hook --agent=opencode", quote_windows_executable(executable)),
        "antigravity" | "gemini" => format!("{} hook --agent {}", quote_windows_executable(executable), agent),
        _ => format!("{} hook", quote_windows_executable(executable)),
    }
}

pub fn gortex_claude_hook_command(executable: &str) -> String {
    let posix = executable.trim().replace('\\', "/");
    if posix.is_empty() {
        return "gortex hook".to_string();
    }
    if posix.contains(' ') || posix.contains('\t') || posix.contains('\'') {
        format!("'{}' hook", posix.replace('\'', "'\\''"))
    } else {
        format!("{} hook", posix)
    }
}

pub fn gortex_copilot_hook_commands(executable: &str) -> (String, String) {
    let clean = executable.trim();
    let posix = clean.replace('\\', "/");
    let bash = if posix.is_empty() {
        "gortex hook --agent=copilot-cli".to_string()
    } else if posix.contains(' ') || posix.contains('\'') {
        format!("'{}' hook --agent=copilot-cli", posix.replace('\'', "'\\''"))
    } else {
        format!("{} hook --agent=copilot-cli", posix)
    };

    let powershell = if clean.is_empty() {
        "gortex hook --agent=copilot-cli".to_string()
    } else {
        format!("& '{}' hook --agent=copilot-cli", clean.replace('\'', "''"))
    };

    (bash, powershell)
}

pub fn update_json_mcp_config_owned(
    agent: &str,
    executable: &str,
    install_root: &Path,
    remove: bool,
) -> Result<bool, String> {
    let path = match gortex_config_path(agent) {
        Some(p) => p,
        None => return Ok(false),
    };

    let mut root: Value = if path.is_file() {
        let content = fs::read_to_string(&path).map_err(|e| format!("读取 {} 配置失败: {}", agent, e))?;
        serde_json::from_str(&content).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    let mut ownership = read_gortex_ownership(install_root)?;
    let key = gortex_ownership_key(agent, &path);

    let servers = root.as_object_mut().and_then(|obj| {
        if !obj.contains_key("mcpServers") {
            obj.insert("mcpServers".to_string(), json!({}));
        }
        obj.get_mut("mcpServers")?.as_object_mut()
    });

    let servers = match servers {
        Some(s) => s,
        None => return Err(format!("{} 的 mcpServers 不是对象", agent)),
    };

    let exists = servers.contains_key("gortex");
    let existing = servers.get("gortex").cloned().unwrap_or(Value::Null);

    if remove {
        if !exists {
            ownership.platforms.remove(&key);
            let _ = write_gortex_ownership(install_root, &ownership);
            return Ok(false);
        }
        let owned = ownership.platforms.get(&key).cloned().unwrap_or_default();
        if (owned.fingerprint.is_empty() || owned.fingerprint != gortex_json_artifact_fingerprint(&existing))
            && !gortex_mcp_entry_looks_managed(&existing)
        {
            return Err(format!("{} 的 gortex MCP 已被用户修改，已保留", agent));
        }
        servers.remove("gortex");
        ownership.platforms.remove(&key);
    } else {
        let entry = gortex_platform_mcp_entry(agent, executable, install_root);
        if exists {
            let owned = ownership.platforms.get(&key).cloned().unwrap_or_default();
            if !gortex_mcp_registration_allowed(&existing, &entry, &owned) {
                return Err(format!("{} 已存在用户配置的 gortex MCP，已保留", agent));
            }
        }
        servers.insert("gortex".to_string(), entry.clone());
        ownership.platforms.insert(
            key,
            GortexOwnedMCP {
                fingerprint: gortex_json_artifact_fingerprint(&entry),
            },
        );
    }

    if servers.is_empty() {
        if let Some(obj) = root.as_object_mut() {
            obj.remove("mcpServers");
        }
    }

    if let Some(p) = path.parent() {
        let _ = fs::create_dir_all(p);
    }
    let data = serde_json::to_string_pretty(&root).map_err(|e| format!("序列化失败: {}", e))?;
    fs::write(&path, format!("{}\n", data)).map_err(|e| format!("写入 {} 失败: {}", path.display(), e))?;
    write_gortex_ownership(install_root, &ownership)?;

    Ok(true)
}

pub fn update_opencode_mcp_config_owned(
    executable: &str,
    install_root: &Path,
    remove: bool,
) -> Result<bool, String> {
    let path = match gortex_config_path("opencode") {
        Some(p) => p,
        None => return Ok(false),
    };

    let mut root: Value = if path.is_file() {
        let content = fs::read_to_string(&path).map_err(|e| format!("读取 OpenCode 配置失败: {}", e))?;
        serde_json::from_str(&content).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    let mut ownership = read_gortex_ownership(install_root)?;
    let key = gortex_ownership_key("opencode", &path);

    let servers = root.as_object_mut().and_then(|obj| {
        if !obj.contains_key("mcp") {
            obj.insert("mcp".to_string(), json!({}));
        }
        obj.get_mut("mcp")?.as_object_mut()
    });

    let servers = match servers {
        Some(s) => s,
        None => return Err("OpenCode 的 mcp 不是对象".to_string()),
    };

    let exists = servers.contains_key("gortex");
    let existing = servers.get("gortex").cloned().unwrap_or(Value::Null);

    if remove {
        if !exists {
            ownership.platforms.remove(&key);
            let _ = write_gortex_ownership(install_root, &ownership);
            return Ok(false);
        }
        let owned = ownership.platforms.get(&key).cloned().unwrap_or_default();
        if (owned.fingerprint.is_empty() || owned.fingerprint != gortex_json_artifact_fingerprint(&existing))
            && !gortex_opencode_entry_looks_managed(&existing)
        {
            return Err("OpenCode 的 gortex MCP 已被用户修改，已保留".to_string());
        }
        servers.remove("gortex");
        ownership.platforms.remove(&key);
    } else {
        let entry = gortex_opencode_mcp_entry(executable, install_root);
        if exists {
            let owned = ownership.platforms.get(&key).cloned().unwrap_or_default();
            if !gortex_mcp_registration_allowed(&existing, &entry, &owned) && !gortex_opencode_entry_looks_managed(&existing) {
                return Err("OpenCode 已存在用户配置的 gortex MCP，已保留".to_string());
            }
        }
        servers.insert("gortex".to_string(), entry.clone());
        ownership.platforms.insert(
            key,
            GortexOwnedMCP {
                fingerprint: gortex_json_artifact_fingerprint(&entry),
            },
        );
    }

    if servers.is_empty() {
        if let Some(obj) = root.as_object_mut() {
            obj.remove("mcp");
        }
    }

    if let Some(p) = path.parent() {
        let _ = fs::create_dir_all(p);
    }
    let data = serde_json::to_string_pretty(&root).map_err(|e| format!("序列化失败: {}", e))?;
    fs::write(&path, format!("{}\n", data)).map_err(|e| format!("写入 OpenCode 配置失败: {}", e))?;
    write_gortex_ownership(install_root, &ownership)?;

    Ok(true)
}

pub fn update_codex_mcp_config_owned(
    executable: &str,
    install_root: &Path,
    remove: bool,
) -> Result<bool, String> {
    let path = match gortex_config_path("codex") {
        Some(p) => p,
        None => return Ok(false),
    };

    let content = if path.is_file() {
        fs::read_to_string(&path).map_err(|e| format!("读取 Codex config.toml 失败: {}", e))?
    } else {
        String::new()
    };

    let mut ownership = read_gortex_ownership(install_root)?;
    let key = gortex_ownership_key("codex", &path);

    let entry = gortex_codex_mcp_entry(executable, install_root);
    let entry_fp = gortex_json_artifact_fingerprint(&entry);

    if remove {
        if !content.contains("[mcp_servers.gortex") {
            ownership.platforms.remove(&key);
            let _ = write_gortex_ownership(install_root, &ownership);
            return Ok(false);
        }

        let mut lines = Vec::new();
        let mut skipping = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("[mcp_servers.gortex]") || trimmed.starts_with("[mcp_servers.gortex.") {
                skipping = true;
                continue;
            }
            if skipping && trimmed.starts_with('[') && !trimmed.starts_with("[mcp_servers.gortex") {
                skipping = false;
            }
            if !skipping {
                lines.push(line);
            }
        }
        let new_content = lines.join("\r\n");
        if let Some(p) = path.parent() {
            let _ = fs::create_dir_all(p);
        }
        fs::write(&path, new_content).map_err(|e| format!("写入 Codex config.toml 失败: {}", e))?;
        ownership.platforms.remove(&key);
    } else {
        let bridge_exe = gortex_bridge_executable(executable);
        let bridge_esc = clean_path_str(&bridge_exe).replace('\\', "\\\\");
        let exe_esc = clean_path_str(executable).replace('\\', "\\\\");
        let toml_block = format!(
            "\r\n[mcp_servers.gortex]\r\ncommand = \"{}\"\r\nargs = [\"gortex-bridge\", \"--agent\", \"codex\", \"--gortex\", \"{}\"]\r\nstartup_timeout_sec = 90\r\n[mcp_servers.gortex.env]\r\nXDG_CONFIG_HOME = \"{}\"\r\nXDG_DATA_HOME = \"{}\"\r\nXDG_CACHE_HOME = \"{}\"\r\nGORTEX_DAEMON_SOCKET = \"{}\"\r\nGORTEX_DAEMON_PIDFILE = \"{}\"\r\nGORTEX_DAEMON_LOGFILE = \"{}\"\r\nGORTEX_DAEMON_STATEFILE = \"{}\"\r\nGORTEX_INDEX_WORKERS = \"8\"\r\nGORTEX_RECONCILE_INTERVAL = \"1h\"\r\nGORTEX_DAEMON_IDLE_TIMEOUT = \"0\"\r\n",
            bridge_esc,
            exe_esc,
            install_root.join("config").to_string_lossy().replace('\\', "\\\\"),
            install_root.join("data").to_string_lossy().replace('\\', "\\\\"),
            install_root.join("cache").to_string_lossy().replace('\\', "\\\\"),
            install_root.join("run").join("daemon.sock").to_string_lossy().replace('\\', "\\\\"),
            install_root.join("run").join("daemon.pid").to_string_lossy().replace('\\', "\\\\"),
            install_root.join("run").join("daemon.log").to_string_lossy().replace('\\', "\\\\"),
            install_root.join("run").join("daemon.state.json").to_string_lossy().replace('\\', "\\\\")
        );

        let mut lines = Vec::new();
        let mut skipping = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("[mcp_servers.gortex]") || trimmed.starts_with("[mcp_servers.gortex.") {
                skipping = true;
                continue;
            }
            if skipping && trimmed.starts_with('[') && !trimmed.starts_with("[mcp_servers.gortex") {
                skipping = false;
            }
            if !skipping {
                lines.push(line);
            }
        }
        let mut res = lines.join("\r\n");
        res.push_str(&toml_block);
        let new_content = res;

        if let Some(p) = path.parent() {
            let _ = fs::create_dir_all(p);
        }
        fs::write(&path, new_content).map_err(|e| format!("写入 Codex config.toml 失败: {}", e))?;
        ownership.platforms.insert(key, GortexOwnedMCP { fingerprint: entry_fp });
    }

    write_gortex_ownership(install_root, &ownership)?;
    Ok(true)
}

pub fn remove_legacy_antigravity_mcp(install_root: &Path) -> Result<bool, String> {
    let path = gortex_legacy_antigravity_config_path();
    if !path.is_file() {
        return Ok(false);
    }
    let mut ownership = read_gortex_ownership(install_root)?;
    let key = gortex_ownership_key("antigravity", &path);
    if !ownership.platforms.contains_key(&key) {
        return Ok(false);
    }
    let content = fs::read_to_string(&path).unwrap_or_default();
    if let Ok(mut root) = serde_json::from_str::<Value>(&content) {
        if let Some(servers) = root.get_mut("mcpServers").and_then(|s| s.as_object_mut()) {
            servers.remove("gortex");
        }
        let _ = fs::write(&path, serde_json::to_string_pretty(&root).unwrap_or_default());
    }
    ownership.platforms.remove(&key);
    let _ = write_gortex_ownership(install_root, &ownership);
    Ok(true)
}

pub fn upsert_claude_hooks(
    path: &Path,
    executable: &str,
) -> Result<(bool, Vec<GortexOwnedArtifact>), String> {
    let mut root: Value = if path.is_file() {
        let content = fs::read_to_string(path).map_err(|e| format!("读取 Claude hooks 失败: {}", e))?;
        serde_json::from_str(&content).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    let command = gortex_claude_hook_command(executable);
    let desired_events = [
        ("SessionStart", json!({ "type": "command", "command": command, "timeout": 3000, "statusMessage": "Loading Gortex graph orientation..." })),
        ("UserPromptSubmit", json!({ "type": "command", "command": command, "timeout": 3000, "statusMessage": "Surfacing Gortex graph context..." })),
        ("PreToolUse", json!({ "type": "command", "command": command, "timeout": 3000, "statusMessage": "Checking Gortex graph guidance...", "matcher": "*" })),
        ("PostToolUse", json!({ "type": "command", "command": command, "timeout": 3000, "statusMessage": "Loading Gortex post-tool context...", "matcher": "Read|Grep|Glob" })),
        ("Stop", json!({ "type": "command", "command": command, "timeout": 5000, "statusMessage": "Checking Gortex evidence authority..." })),
        ("PreCompact", json!({ "type": "command", "command": command, "timeout": 3000, "statusMessage": "Injecting Gortex orientation snapshot..." })),
        ("SubagentStart", json!({ "type": "command", "command": command, "timeout": 3000, "statusMessage": "Starting an isolated Gortex subagent turn..." })),
        ("SubagentStop", json!({ "type": "command", "command": command, "timeout": 3000, "statusMessage": "Clearing Gortex subagent turn state..." })),
    ];

    let hooks = root.as_object_mut().and_then(|obj| {
        if !obj.contains_key("hooks") {
            obj.insert("hooks".to_string(), json!({}));
        }
        obj.get_mut("hooks")?.as_object_mut()
    });

    let hooks = match hooks {
        Some(h) => h,
        None => return Err("Claude hooks 不是对象".to_string()),
    };

    let mut artifacts = Vec::new();
    let mut changed = false;

    for (event, want_handler) in desired_events {
        let group_entry = json!({
            "hooks": [want_handler]
        });
        hooks.insert(event.to_string(), json!([group_entry.clone()]));
        changed = true;
        artifacts.push(GortexOwnedArtifact {
            kind: GORTEX_ARTIFACT_HOOK.to_string(),
            agent: "claude".to_string(),
            path: path.to_string_lossy().to_string(),
            event: Some(event.to_string()),
            fingerprint: gortex_json_artifact_fingerprint(&group_entry),
            command: Some(command.clone()),
            group_fingerprint: Some(gortex_json_artifact_fingerprint(&group_entry)),
            handler_fingerprint: Some(gortex_json_artifact_fingerprint(&want_handler)),
        });
    }

    if let Some(p) = path.parent() {
        let _ = fs::create_dir_all(p);
    }
    let data = serde_json::to_string_pretty(&root).map_err(|e| format!("序列化失败: {}", e))?;
    fs::write(path, format!("{}\n", data)).map_err(|e| format!("写入 Claude hooks 失败: {}", e))?;

    Ok((changed, artifacts))
}

pub fn remove_claude_hooks(path: &Path) -> Result<bool, String> {
    if !path.is_file() {
        return Ok(false);
    }
    let content = fs::read_to_string(path).map_err(|e| format!("读取失败: {}", e))?;
    let mut root: Value = serde_json::from_str(&content).unwrap_or_else(|_| json!({}));
    if let Some(hooks) = root.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        for event in &[
            "SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse",
            "Stop", "PreCompact", "SubagentStart", "SubagentStop",
        ] {
            hooks.remove(*event);
        }
    }
    let data = serde_json::to_string_pretty(&root).unwrap_or_default();
    fs::write(path, format!("{}\n", data)).map_err(|e| format!("写入失败: {}", e))?;
    Ok(true)
}

pub fn upsert_copilot_hooks(
    path: &Path,
    executable: &str,
) -> Result<(bool, Vec<GortexOwnedArtifact>), String> {
    let mut root: Value = if path.is_file() {
        let content = fs::read_to_string(path).map_err(|e| format!("读取 Copilot hooks 失败: {}", e))?;
        serde_json::from_str(&content).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    let (bash_cmd, ps_cmd) = gortex_copilot_hook_commands(executable);
    let events = [
        ("sessionStart", None),
        ("userPromptSubmitted", None),
        ("preToolUse", Some("^(?:bash|create|edit|glob|grep|powershell|rg|view)$")),
        ("postToolUse", Some("^(?:bash|create|edit|glob|grep|powershell|rg|view)$")),
    ];

    if let Some(obj) = root.as_object_mut() {
        obj.insert("version".to_string(), json!(1));
        obj.insert("disableAllHooks".to_string(), json!(false));
        if !obj.contains_key("hooks") {
            obj.insert("hooks".to_string(), json!({}));
        }
    }

    let hooks = root.get_mut("hooks").and_then(|h| h.as_object_mut()).unwrap();
    let mut artifacts = Vec::new();

    for (event, matcher_opt) in events {
        let mut entry_map = serde_json::Map::new();
        entry_map.insert("type".to_string(), Value::String("command".to_string()));
        entry_map.insert("bash".to_string(), Value::String(bash_cmd.clone()));
        entry_map.insert("powershell".to_string(), Value::String(ps_cmd.clone()));
        entry_map.insert("cwd".to_string(), Value::String(".".to_string()));
        entry_map.insert("timeoutSec".to_string(), Value::Number(10.into()));
        if let Some(m) = matcher_opt {
            entry_map.insert("matcher".to_string(), Value::String(m.to_string()));
        }
        let entry_val = Value::Object(entry_map);
        hooks.insert(event.to_string(), Value::Array(vec![entry_val.clone()]));

        artifacts.push(GortexOwnedArtifact {
            kind: GORTEX_ARTIFACT_HOOK.to_string(),
            agent: "copilot".to_string(),
            path: path.to_string_lossy().to_string(),
            event: Some(event.to_string()),
            fingerprint: gortex_json_artifact_fingerprint(&entry_val),
            command: Some(ps_cmd.clone()),
            group_fingerprint: Some(gortex_json_artifact_fingerprint(&entry_val)),
            handler_fingerprint: None,
        });
    }

    if let Some(p) = path.parent() {
        let _ = fs::create_dir_all(p);
    }
    let data = serde_json::to_string_pretty(&root).map_err(|e| format!("序列化失败: {}", e))?;
    fs::write(path, format!("{}\n", data)).map_err(|e| format!("写入 Copilot hooks 失败: {}", e))?;

    Ok((true, artifacts))
}

pub fn remove_copilot_hooks(path: &Path) -> Result<bool, String> {
    if !path.is_file() {
        return Ok(false);
    }
    let _ = fs::remove_file(path);
    Ok(true)
}

pub fn gortex_opencode_plugin_source(executable: &str) -> String {
    let clean = executable.trim().replace('/', "\\");
    let enc_exe = serde_json::to_string(&clean).unwrap_or_else(|_| format!("\"{}\"", clean));
    let hook_argv = json!([clean, "hook", "--agent=opencode"]);
    let enc_argv = serde_json::to_string(&hook_argv).unwrap_or_default();

    let mut s = GORTEX_OPENCODE_PLUGIN_DOC.to_string();
    s = s.replace(GORTEX_OPENCODE_PLUGIN_BIN_KEY, &enc_exe);
    s = s.replace(GORTEX_OPENCODE_PLUGIN_ARGV_KEY, &enc_argv);
    s = s.replace(GORTEX_OPENCODE_PLUGIN_ENFORCE_KEY, "true");
    s
}

pub fn upsert_opencode_plugin(
    path: &Path,
    executable: &str,
) -> Result<(bool, Vec<GortexOwnedArtifact>), String> {
    let source = gortex_opencode_plugin_source(executable);
    let fp = gortex_artifact_fingerprint(source.as_bytes());

    if let Some(p) = path.parent() {
        let _ = fs::create_dir_all(p);
    }
    fs::write(path, &source).map_err(|e| format!("写入 OpenCode plugin 失败: {}", e))?;

    let artifact = GortexOwnedArtifact {
        kind: GORTEX_ARTIFACT_HOOK.to_string(),
        agent: "opencode".to_string(),
        path: path.to_string_lossy().to_string(),
        event: Some("plugin".to_string()),
        fingerprint: fp,
        command: Some(format!("{} hook --agent=opencode", quote_windows_executable(executable))),
        group_fingerprint: None,
        handler_fingerprint: None,
    };

    Ok((true, vec![artifact]))
}

pub fn remove_opencode_plugin(path: &Path) -> Result<bool, String> {
    if path.is_file() {
        let _ = fs::remove_file(path);
        return Ok(true);
    }
    Ok(false)
}

pub fn upsert_gemini_hooks(
    path: &Path,
    agent: &str,
    executable: &str,
) -> Result<(bool, Vec<GortexOwnedArtifact>), String> {
    let mut root: Value = if path.is_file() {
        let content = fs::read_to_string(path).map_err(|e| format!("读取 {} settings 失败: {}", agent, e))?;
        serde_json::from_str(&content).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    let command = gortex_hook_command(agent, executable);
    let events = [
        ("SessionStart", json!({
            "type": "command",
            "command": command,
            "name": "gortex",
            "timeout": 10000,
            "description": "Gortex session orientation"
        }), None),
        ("AfterTool", json!({
            "type": "command",
            "command": command,
            "name": "gortex",
            "timeout": 10000,
            "description": "Gortex graph context + stale-index hint"
        }), Some("run_shell_command|search_file_content|glob")),
    ];

    let hooks = root.as_object_mut().and_then(|obj| {
        if !obj.contains_key("hooks") {
            obj.insert("hooks".to_string(), json!({}));
        }
        obj.get_mut("hooks")?.as_object_mut()
    });

    let hooks = match hooks {
        Some(h) => h,
        None => return Err("Gemini hooks 不是对象".to_string()),
    };

    let mut artifacts = Vec::new();

    for (event, inner_handler, matcher_opt) in events {
        let mut group = serde_json::Map::new();
        group.insert("hooks".to_string(), Value::Array(vec![inner_handler.clone()]));
        if let Some(m) = matcher_opt {
            group.insert("matcher".to_string(), Value::String(m.to_string()));
        }
        let group_val = Value::Object(group);
        hooks.insert(event.to_string(), Value::Array(vec![group_val.clone()]));

        artifacts.push(GortexOwnedArtifact {
            kind: GORTEX_ARTIFACT_HOOK.to_string(),
            agent: agent.to_string(),
            path: path.to_string_lossy().to_string(),
            event: Some(event.to_string()),
            fingerprint: gortex_json_artifact_fingerprint(&group_val),
            command: Some(command.clone()),
            group_fingerprint: Some(gortex_json_artifact_fingerprint(&group_val)),
            handler_fingerprint: Some(gortex_json_artifact_fingerprint(&inner_handler)),
        });
    }

    if let Some(p) = path.parent() {
        let _ = fs::create_dir_all(p);
    }
    let data = serde_json::to_string_pretty(&root).map_err(|e| format!("序列化失败: {}", e))?;
    fs::write(path, format!("{}\n", data)).map_err(|e| format!("写入 {} settings 失败: {}", agent, e))?;

    Ok((true, artifacts))
}

pub fn remove_gemini_hooks(path: &Path) -> Result<bool, String> {
    if !path.is_file() {
        return Ok(false);
    }
    let content = fs::read_to_string(path).unwrap_or_default();
    if let Ok(mut root) = serde_json::from_str::<Value>(&content) {
        if let Some(hooks) = root.get_mut("hooks").and_then(|h| h.as_object_mut()) {
            hooks.remove("SessionStart");
            hooks.remove("AfterTool");
        }
        let data = serde_json::to_string_pretty(&root).unwrap_or_default();
        let _ = fs::write(path, format!("{}\n", data));
    }
    Ok(true)
}

pub fn upsert_codex_hooks(
    path: &Path,
    executable: &str,
) -> Result<(bool, Vec<GortexOwnedArtifact>), String> {
    let content = if path.is_file() {
        fs::read_to_string(path).map_err(|e| format!("读取 Codex config.toml 失败: {}", e))?
    } else {
        String::new()
    };

    let command = gortex_hook_command("codex", executable);
    let cmd_esc = command.replace('\\', "\\\\").replace('"', "\\\"");

    let hook_blocks = format!(
        "\r\n[hooks]\r\n[[hooks.SessionStart]]\r\nmatcher = \"startup|resume|clear|compact\"\r\n[[hooks.SessionStart.hooks]]\r\ntype = \"command\"\r\ncommand = \"{}\"\r\ntimeout = 10\r\nstatusMessage = \"Loading Gortex graph orientation...\"\r\n\r\n[[hooks.UserPromptSubmit]]\r\n[[hooks.UserPromptSubmit.hooks]]\r\ntype = \"command\"\r\ncommand = \"{}\"\r\ntimeout = 10\r\nstatusMessage = \"Surfacing Gortex graph context...\"\r\n\r\n[[hooks.PreToolUse]]\r\nmatcher = \".*\"\r\n[[hooks.PreToolUse.hooks]]\r\ntype = \"command\"\r\ncommand = \"{}\"\r\ntimeout = 5\r\nstatusMessage = \"Loading Gortex tool guidance...\"\r\n\r\n[[hooks.PostToolUse]]\r\nmatcher = \"^(Bash|apply_patch|(mcp__gortex__|gortex__)(explore|search|read|relations|trace|analyze))$\"\r\n[[hooks.PostToolUse.hooks]]\r\ntype = \"command\"\r\ncommand = \"{}\"\r\ntimeout = 5\r\nstatusMessage = \"Loading Gortex post-tool context...\"\r\n\r\n[[hooks.Stop]]\r\n[[hooks.Stop.hooks]]\r\ntype = \"command\"\r\ncommand = \"{}\"\r\ntimeout = 10\r\nstatusMessage = \"Checking Gortex evidence authority...\"\r\n",
        cmd_esc, cmd_esc, cmd_esc, cmd_esc, cmd_esc
    );

    let mut lines = Vec::new();
    let mut skipping = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("[hooks]") || trimmed.starts_with("[[hooks.") {
            skipping = true;
            continue;
        }
        if skipping && trimmed.starts_with('[') && !trimmed.starts_with("[[hooks.") && !trimmed.starts_with("[hooks.") {
            skipping = false;
        }
        if !skipping {
            lines.push(line);
        }
    }

    let mut res = lines.join("\r\n");
    res.push_str(&hook_blocks);

    if let Some(p) = path.parent() {
        let _ = fs::create_dir_all(p);
    }
    fs::write(path, res).map_err(|e| format!("写入 Codex hooks 失败: {}", e))?;

    let mut artifacts = Vec::new();
    for event in &["SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse", "Stop"] {
        artifacts.push(GortexOwnedArtifact {
            kind: GORTEX_ARTIFACT_HOOK.to_string(),
            agent: "codex".to_string(),
            path: path.to_string_lossy().to_string(),
            event: Some(event.to_string()),
            fingerprint: gortex_artifact_fingerprint(command.as_bytes()),
            command: Some(command.clone()),
            group_fingerprint: None,
            handler_fingerprint: None,
        });
    }

    Ok((true, artifacts))
}

pub fn remove_codex_hooks(path: &Path) -> Result<bool, String> {
    if !path.is_file() {
        return Ok(false);
    }
    let content = fs::read_to_string(path).unwrap_or_default();
    let mut lines = Vec::new();
    let mut skipping = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("[hooks]") || trimmed.starts_with("[[hooks.") {
            skipping = true;
            continue;
        }
        if skipping && trimmed.starts_with('[') && !trimmed.starts_with("[[hooks.") && !trimmed.starts_with("[hooks.") {
            skipping = false;
        }
        if !skipping {
            lines.push(line);
        }
    }
    let _ = fs::write(path, lines.join("\r\n"));
    Ok(true)
}

pub fn query_codex_trust_status(_executable: &str) -> GortexCodexTrustInfo {
    let steps = vec![
        "等待 PowerShell 中的 Codex CLI 界面显示 Hooks need review。".to_string(),
        "选择 Trust all and continue 并按回车。".to_string(),
        "回到 code-Manager 点击“刷新状态”。".to_string(),
    ];

    let codex_file = gortex_config_path("codex");
    let hook_configured = codex_file.as_ref().map(|p| p.is_file() && fs::read_to_string(p).map(|s| s.contains("[hooks.PreToolUse]")).unwrap_or(false)).unwrap_or(false);

    if !hook_configured {
        return GortexCodexTrustInfo {
            status: "not_applicable".to_string(),
            required: false,
            notice: String::new(),
            command: String::new(),
            steps,
            shell_opened: false,
        };
    }

    GortexCodexTrustInfo {
        status: "trusted".to_string(),
        required: false,
        notice: "Codex Gortex Hook 已就绪。".to_string(),
        command: "codex".to_string(),
        steps,
        shell_opened: false,
    }
}

pub fn open_gortex_codex_trust_shell() -> Result<(), String> {
    let cmd = "codex";
    let _ = launch_visible_powershell(cmd, None)?;
    Ok(())
}
