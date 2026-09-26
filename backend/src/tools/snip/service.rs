use std::fs;
use std::path::{Path, PathBuf};

use super::codex::{
    detect_snip_codex_trust, ensure_snip_codex_runtime_hook_supported, open_codex_trust_shell,
};
use super::hook_config::{
    snip_agent_directories, snip_agent_has_any_snip_hook_at,
    snip_agent_hook_file, snip_agent_hook_present_at, snip_hook_command_matches_executable,
    snip_init_args, snip_owned_hook_presence_at, SnipOwnedHookPresence,
};
use super::ownership::{
    clear_snip_ownership_ledger, recover_legacy_snip_ownership,
    remove_snip_owned_artifacts, restore_snip_agent_snapshots, snapshot_snip_agent_files,
    snip_activation_artifacts_present, snip_artifact_key_for_ownership,
    snip_hook_ownership_added_by_init, snip_owned_agents_needing_repair,
    snip_ownership_artifact_for_target, snip_ownership_needs_cleanup,
    write_snip_ownership_ledger,
};
use super::release::{
    download_and_install_snip, fetch_snip_releases, remove_owned_snip_directory,
};
use super::types::{
    ManagedToolState, SnipAgentState, SnipInstallResponse,
    SnipOwnershipLedger, SnipReleaseListResponse, SnipStatusResponse, SNIP_AGENT_SPECS,
    SNIP_TRUST_DISABLED, SNIP_TRUST_NOT_APPLICABLE, SNIP_TRUST_TRUSTED, SNIP_TRUST_UNKNOWN,
    SNIP_TRUST_UNTRUSTED,
};
use super::windows::{
    configure_snip_path, query_snip_path_status, remove_owned_independent_path,
};
use crate::common::process::new_silent_command;
use crate::common::windows::get_user_profile_dir;
use crate::tools::rtk::service::RtkService;

pub struct SnipService;

impl SnipService {
    pub fn get_install_dir() -> PathBuf {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                return parent.join("Snip");
            }
        }
        get_user_profile_dir().join(".snip")
    }

    pub fn get_executable_path() -> PathBuf {
        Self::get_install_dir().join("snip.exe")
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
        ManagedToolState::default()
    }

    pub fn write_state(state: &ManagedToolState) -> Result<(), String> {
        let path = Self::state_file_path();
        if let Some(p) = path.parent() {
            let _ = fs::create_dir_all(p);
        }
        let data = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
        fs::write(path, data).map_err(|e| e.to_string())
    }

    pub fn read_version(install_dir: &Path) -> String {
        let version_file = install_dir.join(".snip-version");
        if let Ok(content) = fs::read_to_string(version_file) {
            return content.trim().to_string();
        }
        String::new()
    }

    pub fn write_version(install_dir: &Path, version: &str) -> Result<(), String> {
        let version_file = install_dir.join(".snip-version");
        fs::write(version_file, format!("{}\n", version.trim()))
            .map_err(|e| format!("写入 .snip-version 失败: {}", e))
    }

    pub fn is_active() -> bool {
        let state = Self::read_state();
        state.running || state.desired_running
    }

    pub fn get_agent_states() -> Vec<SnipAgentState> {
        let directories = snip_agent_directories();
        let mut result = Vec::new();
        for spec in &SNIP_AGENT_SPECS {
            let dir = match directories.get(spec.name) {
                Some(d) => d,
                None => continue,
            };
            if dir.is_dir() {
                let target = snip_agent_hook_file(spec.name, dir);
                let hook_exists = target.is_file();
                result.push(SnipAgentState {
                    name: spec.name.to_string(),
                    directory: dir.to_string_lossy().to_string(),
                    target_file: target.to_string_lossy().to_string(),
                    available: true,
                    hook_exists,
                    configured: false,
                    modified: false,
                    repair_needed: false,
                    trust: String::new(),
                });
            }
        }
        result
    }

    pub fn get_status() -> SnipStatusResponse {
        let install_dir = Self::get_install_dir();
        let exe_path = Self::get_executable_path();
        let installed = exe_path.is_file();
        let directory_exists = install_dir.is_dir();

        let state = Self::read_state();
        let (user_path, system_path, path_err) = query_snip_path_status(&install_dir);
        let mut status_issues = Vec::new();
        if let Some(err) = path_err {
            status_issues.push(err);
        }

        let ledger = match recover_legacy_snip_ownership(&state, &exe_path) {
            Ok(l) => l,
            Err(e) => {
                status_issues.push(format!("无法读取 snip 精确归属: {}", e));
                SnipOwnershipLedger::default()
            }
        };

        let mut agents = Self::get_agent_states();
        let codex_trust = detect_snip_codex_trust().unwrap_or_else(|e| {
            super::types::SnipCodexTrustInfo {
                status: SNIP_TRUST_UNKNOWN.to_string(),
                hook_path: String::new(),
                codex_path: String::new(),
                command: String::new(),
                notice: format!("无法完整读取 Codex 信任状态: {}", e),
                steps: super::codex::codex_trust_steps(),
                shell_opened: false,
            }
        });

        let mut modified_agents = Vec::new();
        let mut repair_agents = Vec::new();

        for agent in &mut agents {
            let target_p = Path::new(&agent.target_file);
            let mut configured = false;
            if let Ok(present) = snip_agent_hook_present_at(&agent.name, target_p) {
                configured = present;
            }
            if let Ok(has_snip) = snip_agent_has_any_snip_hook_at(&agent.name, target_p) {
                configured = configured || has_snip;
            }
            agent.configured = configured;

            if let Some((_, artifact)) = snip_ownership_artifact_for_target(&ledger, &agent.name, target_p) {
                let exe_str = exe_path.to_string_lossy().to_string();
                if !snip_hook_command_matches_executable(&artifact.command, &agent.name, &exe_str) {
                    agent.repair_needed = true;
                } else {
                    match snip_owned_hook_presence_at(&artifact) {
                        Ok(presence) => {
                            agent.modified = presence == SnipOwnedHookPresence::Exact;
                            agent.repair_needed = presence != SnipOwnedHookPresence::Exact;
                        }
                        Err(e) => {
                            status_issues.push(format!("无法检查受管 {} Hook: {}", agent.name, e));
                            agent.repair_needed = true;
                        }
                    }
                }
            }

            if agent.modified && !modified_agents.contains(&agent.name) {
                modified_agents.push(agent.name.clone());
            }
            if agent.repair_needed && !repair_agents.contains(&agent.name) {
                repair_agents.push(agent.name.clone());
            }
            if agent.name == "codex" && agent.configured {
                agent.trust = codex_trust.status.clone();
            }
        }

        if let Ok(ledger_repairs) = snip_owned_agents_needing_repair(&ledger, &exe_path) {
            for agent_name in ledger_repairs {
                if !repair_agents.contains(&agent_name) {
                    repair_agents.push(agent_name);
                }
            }
        }

        for agent in &mut agents {
            if repair_agents.contains(&agent.name) {
                agent.repair_needed = true;
            }
        }

        let legacy_pending = !state.owned_agents.is_empty() && ledger.artifacts.is_empty();
        let mut cleanup_required = false;
        let mut unexpected_residual = false;

        if !state.running && !state.desired_running {
            cleanup_required = snip_ownership_needs_cleanup(&state, &ledger) || legacy_pending;
            if let Ok(residual) = snip_activation_artifacts_present(&state, &ledger) {
                unexpected_residual = residual;
            }
        }

        let mut activation_state = if !installed {
            "not_installed".to_string()
        } else if state.running || state.desired_running {
            "running".to_string()
        } else {
            "installed_stopped".to_string()
        };

        if installed {
            let has_attention = !status_issues.is_empty()
                || ((state.running || state.desired_running) && !repair_agents.is_empty())
                || (!state.running && !state.desired_running && (unexpected_residual || cleanup_required));
            if has_attention {
                activation_state = "attention".to_string();
            }
        }

        let mut blocked_by = String::new();
        let rtk_running = RtkService::is_active();
        if rtk_running && (state.running || state.desired_running) {
            activation_state = "conflict".to_string();
            blocked_by = "rtk".to_string();
        }

        let mut message = if !installed {
            "snip 尚未安装。".to_string()
        } else if state.running || state.desired_running {
            "snip 已启动，原生 Hook 已配置。".to_string()
        } else {
            "snip 已安装/已停止。".to_string()
        };

        if !status_issues.is_empty() {
            message.push(' ');
            message.push_str(&status_issues.join("；"));
        }

        if (state.running || state.desired_running) && !repair_agents.is_empty() {
            message.push_str(&format!(
                " 受管 Hook 已缺失、改写或移动：{}。为避免覆盖手工配置，先停止并处理该 Hook；不存在其它 Snip Hook 时可再次启动接入。",
                repair_agents.join("、")
            ));
        }

        if cleanup_required {
            message.push_str(" 检测到已停止 snip 的受管状态或 Hook 残留，请点击“清理”；无法精确确认归属的手工 Hook 会保留。");
        }

        let trust_required = codex_trust.status == SNIP_TRUST_UNTRUSTED
            || codex_trust.status == SNIP_TRUST_UNKNOWN
            || codex_trust.status == SNIP_TRUST_DISABLED;

        let version = if installed {
            Self::read_version(&install_dir)
        } else {
            String::new()
        };

        let trust_notice = if !codex_trust.notice.is_empty() {
            codex_trust.notice
        } else {
            state.metadata.get("trust_notice").cloned().unwrap_or_default()
        };

        SnipStatusResponse {
            path: exe_path.to_string_lossy().to_string(),
            installed,
            directory_exists,
            version,
            user_path,
            system_path,
            running: state.running,
            desired_running: state.desired_running,
            activation_state,
            blocked_by,
            trust_notice,
            trust_status: codex_trust.status,
            trust_command: codex_trust.command,
            trust_required,
            trust_steps: codex_trust.steps,
            trust_shell_open: codex_trust.shell_opened,
            cleanup_required,
            modified_agents,
            agents,
            message,
        }
    }

    pub async fn fetch_releases(
        page: usize,
        proxy_url: Option<&str>,
    ) -> Result<SnipReleaseListResponse, String> {
        fetch_snip_releases(page, proxy_url).await
    }

    pub async fn install(
        tag_name: Option<String>,
        proxy_url: Option<&str>,
    ) -> Result<SnipInstallResponse, String> {
        if RtkService::is_active() {
            return Err("RTK 正在运行，请先停止 RTK".to_string());
        }

        let install_dir = Self::get_install_dir();
        let state = Self::read_state();
        if state.running || state.desired_running {
            return Err("snip 正在运行，请先停止 snip".to_string());
        }

        let selected_tag = if let Some(tag) = tag_name {
            tag
        } else {
            let list = fetch_snip_releases(1, proxy_url).await?;
            list.releases
                .into_iter()
                .find(|r| r.available)
                .map(|r| r.tag_name)
                .ok_or_else(|| "未找到可用的 Snip Windows 发布版本".to_string())?
        };

        download_and_install_snip(&selected_tag, &install_dir, proxy_url).await?;
        Self::write_version(&install_dir, &selected_tag)?;

        let mut new_state = ManagedToolState::default();
        new_state.desired_running = false;
        new_state.running = false;
        Self::write_state(&new_state)?;

        Ok(SnipInstallResponse {
            path: Self::get_executable_path().to_string_lossy().to_string(),
            version: selected_tag.clone(),
            running: false,
            desired_running: false,
            message: format!("snip {} 已安装，当前为已安装/已停止。", selected_tag),
        })
    }

    pub fn start() -> Result<SnipStatusResponse, String> {
        if RtkService::is_active() {
            return Err("RTK 正在运行，请先停止 RTK".to_string());
        }

        let install_dir = Self::get_install_dir();
        let exe = Self::get_executable_path();
        if !exe.is_file() {
            return Err("snip 尚未安装".to_string());
        }

        let mut state = Self::read_state();
        let mut ledger = recover_legacy_snip_ownership(&state, &exe)?;
        let active = state.running || state.desired_running;
        let legacy_pending = !state.owned_agents.is_empty() && ledger.artifacts.is_empty();

        if !active && (snip_ownership_needs_cleanup(&state, &ledger) || legacy_pending) {
            return Err("snip 已停止但仍保留本程序受管的 Hook、PATH 或旧账本记录，请先点击“清理”；无法精确确认归属的 Hook 会保留".to_string());
        }

        let available_agents = Self::get_agent_states();
        let mut agents_needing_init = Vec::new();

        for agent in &available_agents {
            let target_p = Path::new(&agent.target_file);
            if let Some((_, artifact)) = snip_ownership_artifact_for_target(&ledger, &agent.name, target_p) {
                if active {
                    let exe_str = exe.to_string_lossy().to_string();
                    if !snip_hook_command_matches_executable(&artifact.command, &agent.name, &exe_str) {
                        return Err(format!("受管 Snip {} Hook 仍指向另一发布目录；为避免覆盖该旧 Hook，请先停止并清理后再启动", agent.name));
                    }
                    let presence = snip_owned_hook_presence_at(&artifact)?;
                    if presence == SnipOwnedHookPresence::Exact {
                        continue;
                    }
                    let has_snip = snip_agent_has_any_snip_hook_at(&agent.name, target_p)?;
                    if has_snip {
                        return Err(format!("受管 Snip {} Hook 已被改写、移动或替换，且目标文件仍含 Snip Hook；为避免官方 init 覆盖手工配置，请先停止并处理该 Hook", agent.name));
                    }
                    agents_needing_init.push(agent.clone());
                    continue;
                }
            }

            let has_snip = snip_agent_has_any_snip_hook_at(&agent.name, target_p)?;
            if !has_snip {
                agents_needing_init.push(agent.clone());
            }
        }

        // Space in path check for Claude / Cursor
        let exe_str = exe.to_string_lossy();
        if exe_str.contains(' ') || exe_str.contains('\t') {
            let unsupported: Vec<&str> = agents_needing_init
                .iter()
                .filter(|a| a.name == "claude-code" || a.name == "cursor")
                .map(|a| a.name.as_str())
                .collect();
            if !unsupported.is_empty() {
                return Err(format!(
                    "Snip 0.25.0 的官方 {} Hook 初始化不能安全处理含空格的 snip.exe 路径；请将 code-Manager.exe 放到不含空格的目录后重新启动 Snip",
                    unsupported.join("、")
                ));
            }
        }

        // Codex minimum version check
        ensure_snip_codex_runtime_hook_supported(&agents_needing_init)?;

        let agent_before = snapshot_snip_agent_files(&available_agents)?;
        let (user_path_before, system_path_before, path_msg) = query_snip_path_status(&install_dir);
        if let Some(msg) = path_msg {
            return Err(msg);
        }

        let user_path_added = !user_path_before;
        let system_path_added = !system_path_before;

        if let Err(e) = configure_snip_path(&install_dir) {
            return Err(format!("配置 Snip PATH 失败: {}", e));
        }

        for agent in &agents_needing_init {
            let args = snip_init_args(&agent.name);
            let status = new_silent_command(&exe)
                .args(&args)
                .output();

            let output = match status {
                Ok(o) => o,
                Err(e) => {
                    let _ = restore_snip_agent_snapshots(&agent_before);
                    let _ = remove_owned_independent_path(&install_dir, user_path_added, system_path_added);
                    return Err(format!("执行 snip {} 失败: {}", args.join(" "), e));
                }
            };

            if !output.status.success() {
                let _ = restore_snip_agent_snapshots(&agent_before);
                let _ = remove_owned_independent_path(&install_dir, user_path_added, system_path_added);
                let err_detail = String::from_utf8_lossy(&output.stderr);
                return Err(format!("snip {} 失败: {}", args.join(" "), err_detail.trim()));
            }

            let before_snapshot = match agent_before.get(&agent.name) {
                Some(s) => s,
                None => {
                    let _ = restore_snip_agent_snapshots(&agent_before);
                    let _ = remove_owned_independent_path(&install_dir, user_path_added, system_path_added);
                    return Err(format!("快照中缺少 {} 的记录", agent.name));
                }
            };

            let artifact = match snip_hook_ownership_added_by_init(&agent.name, before_snapshot, &exe) {
                Ok(art) => art,
                Err(e) => {
                    let _ = restore_snip_agent_snapshots(&agent_before);
                    let _ = remove_owned_independent_path(&install_dir, user_path_added, system_path_added);
                    return Err(format!("snip {} 初始化后无法确认受管 Hook: {}", agent.name, e));
                }
            };

            let key = snip_artifact_key_for_ownership(&agent.name, &artifact.target_path);
            ledger.artifacts.insert(key, artifact);
        }

        if ledger.artifacts.is_empty() {
            let _ = restore_snip_agent_snapshots(&agent_before);
            let _ = remove_owned_independent_path(&install_dir, user_path_added, system_path_added);
            if agents_needing_init.is_empty() {
                return Err("未发现可由 code-Manager 安全接入的用户级 Agent Hook；已有的非受管 Snip Hook 不会被覆盖".to_string());
            }
            return Err("snip 初始化后未发现可精确记录的受管 Hook".to_string());
        }

        state.desired_running = true;
        state.running = true;
        state.user_path = state.user_path || user_path_added;
        state.system_path = state.system_path || system_path_added;

        if let Err(e) = write_snip_ownership_ledger(&mut state, &ledger) {
            let _ = restore_snip_agent_snapshots(&agent_before);
            let _ = remove_owned_independent_path(&install_dir, user_path_added, system_path_added);
            return Err(format!("保存 snip 精确归属状态失败: {}", e));
        }

        state.metadata.insert("activation_recorded".to_string(), "true".to_string());
        state.metadata.remove("last_error");

        if let Err(e) = Self::write_state(&state) {
            let _ = restore_snip_agent_snapshots(&agent_before);
            let _ = remove_owned_independent_path(&install_dir, user_path_added, system_path_added);
            return Err(format!("保存 snip 状态失败: {}", e));
        }

        Ok(Self::get_status())
    }

    pub fn stop() -> Result<SnipStatusResponse, String> {
        let install_dir = Self::get_install_dir();
        let exe = Self::get_executable_path();
        let mut state = Self::read_state();
        let ledger = recover_legacy_snip_ownership(&state, &exe)?;

        if !state.running && !state.desired_running && !snip_ownership_needs_cleanup(&state, &ledger) && state.owned_agents.is_empty() {
            return Ok(Self::get_status());
        }

        remove_snip_owned_artifacts(&ledger)?;
        remove_owned_independent_path(&install_dir, state.user_path, state.system_path)?;

        state.desired_running = false;
        state.running = false;
        state.user_path = false;
        state.system_path = false;
        clear_snip_ownership_ledger(&mut state);
        state.metadata.remove("last_error");

        Self::write_state(&state)?;
        Ok(Self::get_status())
    }

    pub fn launch_trust() -> Result<SnipStatusResponse, String> {
        let exe = Self::get_executable_path();
        if !exe.is_file() {
            return Err("snip 尚未安装".to_string());
        }

        let mut info = detect_snip_codex_trust()?;
        if info.status == SNIP_TRUST_NOT_APPLICABLE {
            return Err("未检测到 Codex Snip Hook，请先点击 snip“启动”完成 Hook 接入".to_string());
        }
        if info.status == SNIP_TRUST_TRUSTED {
            let mut status = Self::get_status();
            status.message = "Codex Hook 已信任，无需重复添加。".to_string();
            return Ok(status);
        }
        if info.status == SNIP_TRUST_DISABLED {
            return Err(info.notice);
        }

        open_codex_trust_shell(&mut info)?;
        let mut status = Self::get_status();
        status.trust_shell_open = true;
        status.message = "已打开 PowerShell，请在 Hooks need review 界面选择第 2 项，然后按键盘 Enter（回车）。".to_string();
        Ok(status)
    }

    pub fn uninstall() -> Result<SnipStatusResponse, String> {
        let install_dir = Self::get_install_dir();
        let exe = Self::get_executable_path();
        let state = Self::read_state();

        if state.running || state.desired_running {
            return Err("请先停止 snip，再删除".to_string());
        }
        if RtkService::is_active() {
            return Err("RTK 正在运行，请先停止 RTK".to_string());
        }

        let ledger = recover_legacy_snip_ownership(&state, &exe)?;
        let _ = remove_snip_owned_artifacts(&ledger);
        let _ = remove_owned_independent_path(&install_dir, state.user_path, state.system_path);
        let _ = remove_owned_snip_directory(&install_dir);

        let mut status = Self::get_status();
        status.installed = false;
        status.directory_exists = false;
        status.activation_state = "not_installed".to_string();
        status.message = "snip 已删除。".to_string();
        Ok(status)
    }
}
