use std::fs;
use std::path::PathBuf;
use tracing::info;

use super::assistant::{
    discover_assistant_targets, query_claude_prompt_configured,
    query_codex_configured, replace_utf8_file, upsert_assistant_prompt, AssistantKind,
    RTK_CLAUDE_AGENT_INSTRUCTIONS, RTK_CODEX_AGENT_INSTRUCTIONS, RTK_CODEX_COMMANDS,
    codex_residual_status, claude_prompt_residual_status,
};
use super::integrations::{
    claude_hook_command, claude_hook_residual_status, claude_settings_file,
    copilot_hook_command, copilot_hook_file, cursor_hook_command,
    cursor_hooks_file, install_claude_hook, install_copilot_hook, install_cursor_hook,
    query_claude_available, query_claude_hook_configured, query_copilot_available,
    query_copilot_configured, query_cursor_available, query_cursor_configured,
};
use super::ownership::{
    clear_ownership_ledger, hook_artifact_ownership, modified_agent_names, prompt_artifact_ownership,
    read_ownership_ledger, remove_owned_artifacts, restore_integration_snapshots, snapshot_integrations,
    write_ownership_ledger, RTK_ARTIFACT_CLAUDE_HOOK, RTK_ARTIFACT_CLAUDE_PROMPT,
    RTK_ARTIFACT_CODEX_PROMPT, RTK_ARTIFACT_COPILOT_HOOK, RTK_ARTIFACT_CURSOR_HOOK,
};
use super::release::{download_and_install_rtk, fetch_rtk_releases};
use super::types::{
    ManagedToolState, RtkInstallResponse, RtkReleaseListResponse, RtkStatusResponse, RtkUninstallResponse,
};
use super::windows::{
    configure_rtk_path, query_rtk_path_status, remove_owned_rtk_path, remove_rtk_path,
};
use crate::common::windows::get_user_profile_dir;
use crate::tools::snip::service::SnipService;

pub struct RtkService;

impl RtkService {
    pub fn get_install_dir() -> PathBuf {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                return parent.join("RTK-AI");
            }
        }
        get_user_profile_dir().join(".rtk")
    }

    pub fn get_executable_path() -> PathBuf {
        Self::get_install_dir().join("rtk.exe")
    }

    fn state_file_path() -> PathBuf {
        Self::get_install_dir().join(".code-manager-state.json")
    }

    pub fn read_state() -> ManagedToolState {
        let path = Self::state_file_path();
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(state) = serde_json::from_str::<ManagedToolState>(&content) {
                return state;
            }
        }
        // 兼容旧版 .rtk-state.json
        let legacy_path = Self::get_install_dir().join(".rtk-state.json");
        if let Ok(content) = fs::read_to_string(&legacy_path) {
            if let Ok(state) = serde_json::from_str::<ManagedToolState>(&content) {
                return state;
            }
        }
        ManagedToolState::default()
    }

    pub fn write_state(state: &ManagedToolState) -> Result<(), String> {
        let path = Self::state_file_path();
        if let Some(p) = path.parent() {
            let _ = fs::create_dir_all(p);
        }
        let mut st = state.clone();
        st.updated_at = Some(chrono::Utc::now());
        let data = serde_json::to_string_pretty(&st).map_err(|e| e.to_string())?;
        fs::write(path, data + "\n").map_err(|e| e.to_string())
    }

    pub fn is_active() -> bool {
        let state = Self::read_state();
        state.running || state.desired_running
    }

    pub fn get_status() -> RtkStatusResponse {
        let exe_path = Self::get_executable_path();
        let install_dir = Self::get_install_dir();
        let installed = exe_path.exists() && exe_path.is_file();
        let directory_exists = install_dir.is_dir();

        let (user_path, system_path, path_err) = query_rtk_path_status(&install_dir);
        let state = Self::read_state();
        let ledger = read_ownership_ledger(&state).unwrap_or_default();

        let version = if installed {
            let version_file = install_dir.join(".rtk-version");
            fs::read_to_string(version_file)
                .unwrap_or_default()
                .trim()
                .to_string()
        } else {
            String::new()
        };

        // Codex 状态
        let codex_targets = discover_assistant_targets();
        let codex_target = codex_targets.iter().find(|t| t.kind == AssistantKind::Codex);
        let codex_available = codex_target.map(|t| t.file_path.parent().map(|p| p.exists()).unwrap_or(false) || t.file_path.exists()).unwrap_or(false);
        let codex_configured = query_codex_configured();
        let (codex_residual, _codex_residual_msg) = codex_residual_status();

        // Claude 状态
        let claude_available = query_claude_available();
        let claude_hook_configured = query_claude_hook_configured(&exe_path);
        let claude_prompt_configured = query_claude_prompt_configured();
        let claude_configured = claude_hook_configured && claude_prompt_configured;
        let (claude_hook_res, _claude_hook_msg) = claude_hook_residual_status();
        let (claude_prompt_res, _claude_prompt_msg) = claude_prompt_residual_status();
        let claude_residual = claude_hook_res || claude_prompt_res;

        // Copilot 状态
        let copilot_available = query_copilot_available();
        let copilot_configured = query_copilot_configured(&exe_path);

        // Cursor 状态
        let cursor_available = query_cursor_available();
        let cursor_configured = query_cursor_configured(&exe_path);

        // 互斥冲突检查
        let snip_status = SnipService::get_status();
        let snip_running = snip_status.running;

        let mut activation_state = "not_installed".to_string();
        let mut blocked_by = String::new();

        if installed {
            if state.running {
                activation_state = "running".to_string();
            } else {
                activation_state = "installed_stopped".to_string();
            }
        }

        if (state.running || state.desired_running) && snip_running {
            activation_state = "conflict".to_string();
            blocked_by = "snip".to_string();
        }

        let modified_agents = modified_agent_names(&ledger);

        // 状态说明文本
        let mut message = if !installed {
            if directory_exists || user_path || system_path || codex_residual || claude_residual {
                "检测到 RTK 残留环境，请先删除后再安装。".to_string()
            } else {
                "RTK 尚未安装。".to_string()
            }
        } else if state.running {
            let mut msg = "RTK 已激活；这只表示激活流程已完成，不代表四个平台都已接入。仅检测到且可安全写入的平台会尝试接入，请以各平台状态为准。".to_string();
            if codex_available {
                if codex_configured {
                    msg.push_str(" Codex AGENTS.md 已集成。");
                } else {
                    msg.push_str(" Codex AGENTS.md 集成未完成。");
                }
            }
            if claude_available {
                msg.push_str(&format!(
                    " Claude Hook{}，提示词{}。",
                    if claude_hook_configured { "已配置" } else { "未配置" },
                    if claude_prompt_configured { "已集成" } else { "未集成" }
                ));
            }
            msg
        } else {
            if user_path && system_path {
                "RTK 已安装，用户和系统 PATH 已配置。".to_string()
            } else {
                "RTK 已安装，但 Windows PATH 尚未完整配置。".to_string()
            }
        };

        if let Some(err_str) = path_err {
            message.push_str(&format!(" {}", err_str));
        }

        RtkStatusResponse {
            path: exe_path.to_string_lossy().to_string(),
            installed,
            directory_exists,
            version,
            user_path,
            system_path,
            codex_available,
            codex_configured,
            codex_residual,
            claude_available,
            claude_hook_configured,
            claude_prompt_configured,
            claude_configured,
            claude_residual,
            copilot_available,
            copilot_configured,
            cursor_available,
            cursor_configured,
            running: state.running,
            desired_running: state.desired_running,
            activation_state,
            blocked_by,
            trust_notice: String::new(),
            modified_agents,
            message,
        }
    }

    pub async fn fetch_releases(page: usize, proxy_url: Option<&str>) -> Result<RtkReleaseListResponse, String> {
        fetch_rtk_releases(page, proxy_url).await
    }

    pub async fn install(tag_name: Option<String>, proxy_url: Option<&str>) -> Result<RtkInstallResponse, String> {
        // 互斥检查
        if SnipService::get_status().running {
            return Err("snip 正在运行，请先停止 snip".to_string());
        }

        let install_dir = Self::get_install_dir();

        let state = Self::read_state();
        if state.running || state.desired_running {
            return Err("RTK 正在运行，请先停止 RTK".to_string());
        }

        let tag = tag_name.unwrap_or_default();
        download_and_install_rtk(&tag, &install_dir, proxy_url).await?;

        let mut init_state = ManagedToolState::default();
        init_state.desired_running = false;
        init_state.running = false;
        Self::write_state(&init_state)?;

        let status = Self::get_status();
        let message = format!("RTK {} 已安装到 RTK-AI，当前为已安装/已停止；启动后才配置 PATH 和助手集成。", status.version);

        Ok(RtkInstallResponse {
            path: status.path,
            version: status.version,
            user_path: false,
            system_path: false,
            codex_available: status.codex_available,
            codex_configured: status.codex_configured,
            claude_available: status.claude_available,
            claude_hook_configured: status.claude_hook_configured,
            claude_prompt_configured: status.claude_prompt_configured,
            claude_configured: status.claude_configured,
            running: false,
            desired_running: false,
            message,
            warnings: Vec::new(),
        })
    }

    pub fn start() -> Result<(), String> {
        // 互斥检查
        if SnipService::get_status().running {
            return Err("snip 正在运行，无法启动 RTK".to_string());
        }

        let install_dir = Self::get_install_dir();
        let exe = Self::get_executable_path();
        if !exe.exists() {
            return Err("RTK 尚未安装".to_string());
        }

        let mut state = Self::read_state();
        let mut ledger = read_ownership_ledger(&state).unwrap_or_default();

        let snapshots = snapshot_integrations();
        let claude_hook_before = query_claude_hook_configured(&exe);
        let copilot_hook_before = query_copilot_configured(&exe);
        let cursor_hook_before = query_cursor_configured(&exe);
        let (user_path_before, system_path_before, _) = query_rtk_path_status(&install_dir);

        // 1. 配置 PATH
        if let Err(e) = configure_rtk_path(&install_dir) {
            return Err(e);
        }

        // 2. 写入内置命令文档
        let _ = fs::write(install_dir.join("RTK-Codex-commands.md"), RTK_CODEX_COMMANDS);
        let _ = fs::write(install_dir.join("RTK-Codex-agent-instructions.md"), RTK_CODEX_AGENT_INSTRUCTIONS);
        let _ = fs::write(install_dir.join("RTK-Claude-agent-instructions.md"), RTK_CLAUDE_AGENT_INSTRUCTIONS);

        // 3. 接入 Codex & Claude 规则段
        let targets = discover_assistant_targets();
        let mut unmanaged = Vec::new();
        let mut updates: Vec<(PathBuf, String)> = Vec::new();
        let mut new_prompt_artifacts = Vec::new();

        for target in &targets {
            let content = if target.file_path.exists() {
                match fs::read_to_string(&target.file_path) {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = restore_integration_snapshots(&snapshots);
                        let _ = remove_rtk_path(&install_dir);
                        return Err(format!("读取 {} 失败: {}", target.file_path.display(), e));
                    }
                }
            } else {
                if !target.create_if_parent_exists || target.file_path.parent().map(|p| !p.exists()).unwrap_or(true) {
                    continue;
                }
                String::new()
            };

            match upsert_assistant_prompt(target, &content) {
                Ok((updated, managed, is_unmanaged)) => {
                    if is_unmanaged {
                        unmanaged.push(match target.kind {
                            AssistantKind::Codex => "Codex",
                            AssistantKind::ClaudeCode => "Claude Code",
                        });
                        continue;
                    }
                    if managed && updated != content {
                        let payload = match target.kind {
                            AssistantKind::Codex => RTK_CODEX_AGENT_INSTRUCTIONS,
                            AssistantKind::ClaudeCode => RTK_CLAUDE_AGENT_INSTRUCTIONS,
                        };
                        let artifact_key = match target.kind {
                            AssistantKind::Codex => RTK_ARTIFACT_CODEX_PROMPT,
                            AssistantKind::ClaudeCode => RTK_ARTIFACT_CLAUDE_PROMPT,
                        };
                        new_prompt_artifacts.push((
                            artifact_key.to_string(),
                            prompt_artifact_ownership(target.agent_name, &target.file_path, payload),
                        ));
                    }
                    if updated != content {
                        updates.push((target.file_path.clone(), updated));
                    }
                }
                Err(e) => {
                    let _ = restore_integration_snapshots(&snapshots);
                    let _ = remove_rtk_path(&install_dir);
                    return Err(format!("检查 {} 失败: {}", target.file_path.display(), e));
                }
            }
        }

        if !unmanaged.is_empty() {
            let _ = restore_integration_snapshots(&snapshots);
            let _ = remove_rtk_path(&install_dir);
            return Err(format!("检测到未受管的 RTK 提示词标记段（{}）；为避免覆盖或误删，请先手工处理后再启动", unmanaged.join("、")));
        }

        for (path, content) in updates {
            if let Err(e) = replace_utf8_file(&path, &content) {
                let _ = restore_integration_snapshots(&snapshots);
                let _ = remove_rtk_path(&install_dir);
                return Err(format!("写入 {} 失败: {}", path.display(), e));
            }
        }

        // 4. 接入 Hooks
        if let Err(e) = install_claude_hook(&exe) {
            let _ = restore_integration_snapshots(&snapshots);
            let _ = remove_rtk_path(&install_dir);
            return Err(format!("配置 Claude Hook 失败: {}", e));
        }

        if let Err(e) = install_copilot_hook(&exe) {
            let _ = restore_integration_snapshots(&snapshots);
            let _ = remove_rtk_path(&install_dir);
            return Err(format!("配置 Copilot Hook 失败: {}", e));
        }

        if let Err(e) = install_cursor_hook(&exe) {
            let _ = restore_integration_snapshots(&snapshots);
            let _ = remove_rtk_path(&install_dir);
            return Err(format!("配置 Cursor Hook 失败: {}", e));
        }

        // 5. 更新归属账本
        for (k, artifact) in new_prompt_artifacts {
            ledger.artifacts.insert(k, artifact);
        }

        if !claude_hook_before && query_claude_hook_configured(&exe) {
            ledger.artifacts.insert(
                RTK_ARTIFACT_CLAUDE_HOOK.to_string(),
                hook_artifact_ownership("claude-code", &claude_settings_file(), &claude_hook_command(&exe)),
            );
        }

        if !copilot_hook_before && query_copilot_configured(&exe) {
            ledger.artifacts.insert(
                RTK_ARTIFACT_COPILOT_HOOK.to_string(),
                hook_artifact_ownership("copilot", &copilot_hook_file(), &copilot_hook_command(&exe)),
            );
        }

        if !cursor_hook_before && query_cursor_configured(&exe) {
            ledger.artifacts.insert(
                RTK_ARTIFACT_CURSOR_HOOK.to_string(),
                hook_artifact_ownership("cursor", &cursor_hooks_file(), &cursor_hook_command(&exe)),
            );
        }

        state.desired_running = true;
        state.running = true;
        state.user_path = state.user_path || !user_path_before;
        state.system_path = state.system_path || !system_path_before;

        if let Err(e) = write_ownership_ledger(&mut state, &ledger) {
            let _ = restore_integration_snapshots(&snapshots);
            let _ = remove_owned_rtk_path(&install_dir, state.user_path, state.system_path);
            return Err(format!("保存 RTK 归属账本失败: {}", e));
        }

        if let Err(e) = Self::write_state(&state) {
            let _ = restore_integration_snapshots(&snapshots);
            let _ = remove_owned_rtk_path(&install_dir, state.user_path, state.system_path);
            return Err(format!("保存 RTK 状态失败: {}", e));
        }

        info!("RTK started and configured successfully");
        Ok(())
    }

    pub fn stop() -> Result<(), String> {
        let install_dir = Self::get_install_dir();
        let mut state = Self::read_state();
        let ledger = read_ownership_ledger(&state).unwrap_or_default();

        if !state.running && !state.desired_running && ledger.artifacts.is_empty() && !state.user_path && !state.system_path {
            return Ok(());
        }

        let mut failures = Vec::new();

        if let Err(e) = remove_owned_rtk_path(&install_dir, state.user_path, state.system_path) {
            failures.push(format!("移除 PATH 失败: {}", e));
        }

        if let Err(e) = remove_owned_artifacts(&ledger) {
            failures.push(format!("清理受管接入失败: {}", e));
        }

        state.desired_running = false;
        state.running = false;
        state.user_path = false;
        state.system_path = false;
        clear_ownership_ledger(&mut state);

        if let Err(e) = Self::write_state(&state) {
            failures.push(format!("更新状态失败: {}", e));
        }

        if !failures.is_empty() {
            return Err(failures.join("；"));
        }

        info!("RTK stopped and cleaned up successfully");
        Ok(())
    }

    pub fn uninstall() -> Result<RtkUninstallResponse, String> {
        let _ = Self::stop();
        let install_dir = Self::get_install_dir();

        let _ = remove_rtk_path(&install_dir);

        if install_dir.exists() {
            fs::remove_dir_all(&install_dir).map_err(|e| format!("删除 RTK 目录失败: {}", e))?;
        }

        let status = Self::get_status();
        Ok(RtkUninstallResponse {
            path: status.path,
            installed: false,
            directory_exists: false,
            user_path: status.user_path,
            system_path: status.system_path,
            codex_available: status.codex_available,
            codex_configured: status.codex_configured,
            codex_residual: status.codex_residual,
            claude_available: status.claude_available,
            claude_hook_configured: status.claude_hook_configured,
            claude_prompt_configured: status.claude_prompt_configured,
            claude_configured: status.claude_configured,
            claude_residual: status.claude_residual,
            running: false,
            desired_running: false,
            message: "RTK 已删除；已清理 RTK-AI 目录、受管 PATH 及账本明确归属的提示词和 Hook。未受管或手工内容会保留。".to_string(),
            warnings: Vec::new(),
        })
    }

    pub fn repair_installed_rtk_path() {
        let install_dir = Self::get_install_dir();
        let exe = Self::get_executable_path();
        if !exe.exists() || !exe.is_file() {
            return;
        }
        let state = Self::read_state();
        if !state.running && !state.desired_running {
            return;
        }
        let (user_path, system_path, err_msg) = query_rtk_path_status(&install_dir);
        if let Some(err) = err_msg {
            tracing::warn!("启动时读取 RTK PATH 状态失败: {}", err);
            return;
        }
        if user_path && system_path {
            return;
        }
        match configure_rtk_path(&install_dir) {
            Ok((u, s)) => {
                info!("检测到已激活的 RTK，已自动补齐 PATH: 用户={}, 系统={}, 目录={:?}", u, s, install_dir);
            }
            Err(e) => {
                tracing::warn!("检测到已激活的 RTK 但 PATH 不完整，自动补齐失败: {}", e);
            }
        }
    }
}
