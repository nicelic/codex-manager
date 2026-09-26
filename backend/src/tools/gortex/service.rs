use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tracing::info;

use super::integrations::*;
use super::ownership::*;
use super::project_mcp::*;
use super::reconcile::{ensure_gortex_watch_config, reconcile_gortex_workspaces};
use super::types::*;
use super::windows::{configure_gortex_path, query_gortex_path_status, remove_owned_gortex_path};
use crate::common::downloader::fetch_github_releases;
use crate::common::process::{
    find_all_process_ids, is_pid_alive, is_process_running,
    new_silent_command,
};
use crate::common::windows::{
    canonicalize_clean, clean_path_str, get_user_profile_dir,
    strip_windows_verbatim_prefix,
};

const GORTEX_REPO: &str = "zzet/gortex";
const GORTEX_EXECUTABLE_NAME: &str = "gortex.exe";
const GORTEX_WINDOWS_ASSET_NAME: &str = "gortex_windows_amd64.zip";
const GORTEX_PROCESS_STOP_TIMEOUT: Duration = Duration::from_secs(15);
const GORTEX_UNTRACK_TIMEOUT: Duration = Duration::from_secs(600);

static CACHED_VERSION: Mutex<Option<(Instant, String)>> = Mutex::new(None);
static CACHED_TRACKED_STATUS: Mutex<Option<(Instant, Vec<String>)>> = Mutex::new(None);
static ACTIVE_TASK: Mutex<Option<GortexActiveTask>> = Mutex::new(None);
static INSTALLING: Mutex<bool> = Mutex::new(false);

pub struct GortexService;

impl GortexService {
    pub fn get_install_dir() -> PathBuf {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                return strip_windows_verbatim_prefix(parent).join("Gortex");
            }
        }
        strip_windows_verbatim_prefix(get_user_profile_dir()).join(".gortex")
    }

    pub fn get_managed_executable_path() -> Option<PathBuf> {
        let managed = Self::get_install_dir().join("bin").join(GORTEX_EXECUTABLE_NAME);
        if managed.is_file() {
            return Some(managed);
        }
        None
    }

    pub fn get_executable_candidates() -> Vec<PathBuf> {
        let mut candidates = Vec::new();
        if let Some(managed) = Self::get_managed_executable_path() {
            candidates.push(managed);
        }

        if let Ok(path_var) = std::env::var("PATH") {
            for dir in std::env::split_paths(&path_var) {
                let candidate = dir.join(GORTEX_EXECUTABLE_NAME);
                if candidate.is_file() {
                    candidates.push(candidate);
                }
                let candidate_no_ext = dir.join("gortex");
                if candidate_no_ext.is_file() {
                    candidates.push(candidate_no_ext);
                }
            }
        }

        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            candidates.push(PathBuf::from(local).join("Programs").join("gortex").join(GORTEX_EXECUTABLE_NAME));
        }

        let profile = get_user_profile_dir();
        candidates.push(profile.join("bin").join(GORTEX_EXECUTABLE_NAME));
        candidates.push(profile.join(".local").join("bin").join(GORTEX_EXECUTABLE_NAME));

        if let Ok(exe) = std::env::current_exe() {
            if let Some(base) = exe.parent() {
                candidates.push(base.join("Gortex").join("bin").join(GORTEX_EXECUTABLE_NAME));
                candidates.push(base.join("Gortex").join(GORTEX_EXECUTABLE_NAME));
                candidates.push(base.join(GORTEX_EXECUTABLE_NAME));
            }
        }

        let mut unique = Vec::new();
        for c in candidates {
            if let Ok(canonical) = canonicalize_clean(&c) {
                if canonical.is_file() && !unique.contains(&canonical) {
                    unique.push(canonical);
                }
            } else {
                let clean = strip_windows_verbatim_prefix(&c);
                if clean.is_file() && !unique.contains(&clean) {
                    unique.push(clean);
                }
            }
        }
        unique
    }

    pub fn get_executable_path() -> PathBuf {
        if let Some(managed) = Self::get_managed_executable_path() {
            return managed;
        }
        for candidate in Self::get_executable_candidates() {
            if candidate.is_file() {
                return candidate;
            }
        }
        Self::get_install_dir().join("bin").join(GORTEX_EXECUTABLE_NAME)
    }

    pub fn get_managed_env_vars(exe: &Path) -> Vec<(String, String)> {
        let root = if exe.starts_with(Self::get_install_dir()) {
            Self::get_install_dir()
        } else if let Some(parent) = exe.parent() {
            if parent.file_name().and_then(|s| s.to_str()).map(|s| s.eq_ignore_ascii_case("bin")).unwrap_or(false) {
                parent.parent().unwrap_or(parent).to_path_buf()
            } else {
                parent.to_path_buf()
            }
        } else {
            Self::get_install_dir()
        };

        vec![
            ("XDG_CONFIG_HOME".to_string(), root.join("config").to_string_lossy().to_string()),
            ("XDG_DATA_HOME".to_string(), root.join("data").to_string_lossy().to_string()),
            ("XDG_CACHE_HOME".to_string(), root.join("cache").to_string_lossy().to_string()),
            ("GORTEX_DAEMON_SOCKET".to_string(), root.join("run").join("daemon.sock").to_string_lossy().to_string()),
            ("GORTEX_DAEMON_PIDFILE".to_string(), root.join("run").join("daemon.pid").to_string_lossy().to_string()),
            ("GORTEX_DAEMON_LOGFILE".to_string(), root.join("run").join("daemon.log").to_string_lossy().to_string()),
            ("GORTEX_DAEMON_STATEFILE".to_string(), root.join("run").join("daemon.state.json").to_string_lossy().to_string()),
            ("GORTEX_RECONCILE_INTERVAL".to_string(), "1h".to_string()),
            ("GORTEX_DAEMON_IDLE_TIMEOUT".to_string(), "0".to_string()),
        ]
    }

    pub async fn run_gortex_command_with_timeout(
        timeout: Duration,
        executable: &Path,
        args: &[&str],
    ) -> Result<String, String> {
        let exe = executable.to_path_buf();
        let args_vec: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let env_vars = Self::get_managed_env_vars(&exe);

        let mut cmd = tokio::process::Command::new(&exe);
        cmd.args(&args_vec);
        for (k, v) in env_vars {
            cmd.env(k, v);
        }
        #[cfg(target_os = "windows")]
        {
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        cmd.kill_on_drop(true);

        match tokio::time::timeout(timeout, cmd.output()).await {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                if output.status.success() {
                    Ok(stdout)
                } else if !stderr.is_empty() {
                    Err(stderr)
                } else if !stdout.is_empty() {
                    Err(stdout)
                } else {
                    Err(format!("命令退出码 {}", output.status.code().unwrap_or(-1)))
                }
            }
            Ok(Err(e)) => Err(e.to_string()),
            Err(_) => Err(format!("命令执行超时（{}秒）", timeout.as_secs())),
        }
    }

    pub fn get_daemon_status() -> (bool, Option<u32>) {
        let install_root = Self::get_install_dir();
        let pid_file = install_root.join("run").join("daemon.pid");
        let sock_file = install_root.join("run").join("daemon.sock");

        if !sock_file.exists() && !pid_file.exists() {
            return (false, None);
        }

        if let Ok(pid_str) = fs::read_to_string(&pid_file) {
            if let Ok(pid) = pid_str.trim().parse::<u32>() {
                if is_pid_alive(pid) {
                    return (true, Some(pid));
                }
            }
        }

        // Check if any process matches daemon pattern
        if sock_file.exists() {
            let pids = find_all_process_ids(GORTEX_EXECUTABLE_NAME);
            if !pids.is_empty() {
                return (true, pids.first().copied());
            }
        }

        (false, None)
    }

    pub fn query_version(executable: &Path) -> String {
        let mut lock = CACHED_VERSION.lock().unwrap();
        if let Some((instant, ref ver)) = *lock {
            if instant.elapsed() < Duration::from_secs(5) {
                return ver.clone();
            }
        }

        let res = new_silent_command(executable).arg("version").output();
        let version = match res {
            Ok(out) if out.status.success() => {
                let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
                text.lines().next().unwrap_or("").trim().to_string()
            }
            _ => String::new(),
        };

        *lock = Some((Instant::now(), version.clone()));
        version
    }

    pub fn query_daemon_tracked_projects(executable: &str) -> Vec<String> {
        let mut lock = CACHED_TRACKED_STATUS.lock().unwrap();
        if let Some((instant, ref list)) = *lock {
            if instant.elapsed() < Duration::from_secs(5) {
                return list.clone();
            }
        }

        let mut projects = Vec::new();
        let res = new_silent_command(executable).arg("status").output();
        if let Ok(out) = res {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.to_lowercase().starts_with("tracked repos:") {
                    continue;
                }
                if let Some(idx) = trimmed.find("  (") {
                    let mut candidate = trimmed[..idx].trim().to_string();
                    if let Some(drive_idx) = candidate.find(":\\") {
                        if drive_idx > 0 {
                            candidate = candidate[drive_idx - 1..].to_string();
                        }
                    } else {
                        let fields: Vec<&str> = candidate.split_whitespace().collect();
                        if fields.len() > 1 {
                            candidate = fields[1..].join(" ");
                        }
                    }
                    let p = Path::new(&candidate);
                    if p.is_absolute() {
                        let clean = clean_path_str(&candidate);
                        if !clean.is_empty() && !projects.iter().any(|existing: &String| existing.eq_ignore_ascii_case(&clean)) {
                            projects.push(clean);
                        }
                    }
                }
            }
        }

        *lock = Some((Instant::now(), projects.clone()));
        projects
    }

    pub fn get_project_registry_path() -> PathBuf {
        Self::get_install_dir().join("config").join("projects.json")
    }

    pub fn get_tracked_projects() -> Vec<String> {
        let path = Self::get_project_registry_path();
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(reg) = serde_json::from_str::<GortexProjectRegistry>(&content) {
                let mut needs_save = false;
                let mut clean: Vec<String> = Vec::new();
                for p in reg.projects {
                    let c = clean_path_str(&p);
                    if !c.is_empty() {
                        if c != p {
                            needs_save = true;
                        }
                        clean.push(c);
                    }
                }
                clean.sort();
                clean.dedup();
                if needs_save {
                    let _ = Self::save_tracked_projects(&clean);
                }
                return clean;
            }
        }

        // Legacy fallback
        let legacy = crate::common::config::runtime_config_dir().join("gortex-projects.json");
        if let Ok(content) = fs::read_to_string(&legacy) {
            if let Ok(reg) = serde_json::from_str::<GortexProjectRegistry>(&content) {
                let mut clean: Vec<String> = reg
                    .projects
                    .into_iter()
                    .map(|p| clean_path_str(&p))
                    .filter(|p| !p.is_empty())
                    .collect();
                clean.sort();
                clean.dedup();
                return clean;
            }
        }

        Vec::new()
    }

    pub fn save_tracked_projects(projects: &[String]) -> Result<(), String> {
        let path = Self::get_project_registry_path();
        if let Some(p) = path.parent() {
            let _ = fs::create_dir_all(p);
        }
        let mut clean: Vec<String> = projects
            .iter()
            .map(|p| clean_path_str(p))
            .filter(|p| !p.is_empty())
            .collect();
        clean.sort();
        clean.dedup();

        let reg = GortexProjectRegistry { projects: clean };
        let data = serde_json::to_string_pretty(&reg).map_err(|e| format!("序列化项目列表失败: {}", e))?;
        fs::write(&path, format!("{}\n", data)).map_err(|e| format!("写入项目列表失败: {}", e))?;
        Ok(())
    }

    pub fn set_active_task(path: &str, action: &str) {
        let mut lock = ACTIVE_TASK.lock().unwrap();
        *lock = Some(GortexActiveTask {
            path: clean_path_str(path),
            action: action.to_string(),
        });
    }

    pub fn clear_active_task() {
        let mut lock = ACTIVE_TASK.lock().unwrap();
        *lock = None;
    }

    pub fn get_active_task() -> Option<GortexActiveTask> {
        let lock = ACTIVE_TASK.lock().unwrap();
        lock.clone()
    }

    pub fn get_status() -> GortexStatusResponse {
        let install_root = Self::get_install_dir();
        let exe_path = Self::get_executable_path();
        let managed_path = Self::get_managed_executable_path();
        let managed_installed = managed_path.is_some();
        let installed = exe_path.is_file();
        let is_installing = *INSTALLING.lock().unwrap();

        let (running, process_id) = Self::get_daemon_status();
        let any_process_running = is_process_running(GORTEX_EXECUTABLE_NAME);
        let unmanaged_process_running = any_process_running && !managed_installed;

        let version = if installed && !is_installing {
            Self::query_version(&exe_path)
        } else {
            String::new()
        };

        let (user_path, system_path, _) = query_gortex_path_status(&install_root.join("bin"));

        // Platform Availability
        let codex_available = gortex_agent_available("codex");
        let claude_available = gortex_agent_available("claude");
        let cursor_available = gortex_agent_available("cursor");
        let copilot_available = gortex_agent_available("copilot");
        let opencode_available = gortex_agent_available("opencode");
        let antigravity_available = gortex_agent_available("antigravity");
        let gemini_available = gortex_agent_available("gemini");

        // Platform MCP Configured
        let check_json_mcp = |agent: &str| -> (bool, bool) {
            if let Some(p) = gortex_config_path(agent) {
                if p.is_file() {
                    if let Ok(c) = fs::read_to_string(&p) {
                        let configured = c.contains("\"gortex\"");
                        return (configured, configured);
                    }
                }
            }
            (false, false)
        };

        let codex_file = gortex_config_path("codex");
        let codex_configured = codex_file.as_ref().map(|p| p.is_file() && fs::read_to_string(p).map(|s| s.contains("[mcp_servers.gortex]")).unwrap_or(false)).unwrap_or(false);
        let codex_complete = codex_configured;

        let (claude_configured, claude_complete) = check_json_mcp("claude");
        let (cursor_configured, cursor_complete) = check_json_mcp("cursor");
        let (copilot_configured, copilot_complete) = check_json_mcp("copilot");
        let (opencode_configured, opencode_complete) = {
            if let Some(p) = gortex_config_path("opencode") {
                if p.is_file() {
                    let configured = fs::read_to_string(&p).map(|s| s.contains("\"gortex\"")).unwrap_or(false);
                    (configured, configured)
                } else {
                    (false, false)
                }
            } else {
                (false, false)
            }
        };
        let (antigravity_configured, antigravity_complete) = check_json_mcp("antigravity");
        let (gemini_configured, gemini_complete) = check_json_mcp("gemini");

        // Prompts
        let check_prompt = |agent: &str| -> (bool, bool) {
            for p in gortex_prompt_paths(agent) {
                if p.is_file() {
                    if let Ok(data) = fs::read(&p) {
                        if gortex_marked_block(&data).is_some() {
                            return (true, true);
                        }
                    }
                }
            }
            (false, false)
        };

        let (codex_prompt, codex_prompt_complete) = check_prompt("codex");
        let (claude_prompt, claude_prompt_complete) = check_prompt("claude");
        let (copilot_prompt, copilot_prompt_complete) = check_prompt("copilot");
        let (opencode_prompt, opencode_prompt_complete) = check_prompt("opencode");
        let (antigravity_prompt, antigravity_prompt_complete) = check_prompt("antigravity");
        let (gemini_prompt, gemini_prompt_complete) = check_prompt("gemini");

        // Hooks
        let codex_hook = codex_file.as_ref().map(|p| p.is_file() && fs::read_to_string(p).map(|s| s.contains("[hooks.PreToolUse]")).unwrap_or(false)).unwrap_or(false);
        let codex_hook_complete = codex_hook;

        let claude_hook = gortex_hook_path("claude").map(|p| p.is_file() && fs::read_to_string(p).map(|s| s.contains("\"SessionStart\"")).unwrap_or(false)).unwrap_or(false);
        let claude_hook_complete = claude_hook;

        let copilot_hook = gortex_hook_path("copilot").map(|p| p.is_file() && fs::read_to_string(p).map(|s| s.contains("gortex hook")).unwrap_or(false)).unwrap_or(false);
        let copilot_hook_complete = copilot_hook;

        let opencode_hook = gortex_hook_path("opencode").map(|p| p.is_file() && fs::read_to_string(p).map(|s| s.contains(GORTEX_OPENCODE_PLUGIN_MARKER)).unwrap_or(false)).unwrap_or(false);
        let opencode_hook_complete = opencode_hook;

        let antigravity_hook = gortex_hook_path("antigravity").map(|p| p.is_file() && fs::read_to_string(p).map(|s| s.contains("\"gortex\"")).unwrap_or(false)).unwrap_or(false);
        let antigravity_hook_complete = antigravity_hook;

        let gemini_hook = antigravity_hook;
        let gemini_hook_complete = antigravity_hook_complete;

        let trust_info = query_codex_trust_status(&exe_path.to_string_lossy());

        let mut tracked_projects = Self::get_tracked_projects();
        if running {
            let daemon_projects = Self::query_daemon_tracked_projects(&exe_path.to_string_lossy());
            for dp in daemon_projects {
                if !tracked_projects.iter().any(|p| p.eq_ignore_ascii_case(&dp)) {
                    tracked_projects.push(dp);
                }
            }
        }
        tracked_projects.sort();
        tracked_projects.dedup();

        let mut cursor_prompt = false;
        let mut cursor_prompt_complete = !tracked_projects.is_empty();
        for project in &tracked_projects {
            let rule_path = gortex_cursor_rule_path(project);
            if rule_path.is_file() {
                if let Ok(data) = fs::read(&rule_path) {
                    if gortex_marked_block(&data).is_some() {
                        cursor_prompt = true;
                        continue;
                    }
                }
            }
            cursor_prompt_complete = false;
        }

        let ownership = read_gortex_ownership(&install_root).unwrap_or_default();
        let project_mcp_projects = gortex_project_mcp_projects_from_ownership(&ownership);
        let project_mcp_enabled = ownership.project_mcp_enabled;

        let integration_present = codex_configured || claude_configured || cursor_configured || copilot_configured
            || opencode_configured || antigravity_configured || gemini_configured
            || codex_prompt || claude_prompt || cursor_prompt || copilot_prompt || opencode_prompt || antigravity_prompt || gemini_prompt
            || codex_hook || claude_hook || copilot_hook || opencode_hook || antigravity_hook || gemini_hook
            || user_path || system_path || !tracked_projects.is_empty() || !ownership.platforms.is_empty() || !ownership.artifacts.is_empty();

        let activation_state = if running {
            "running".to_string()
        } else if managed_installed {
            "installed_stopped".to_string()
        } else {
            "not_installed".to_string()
        };

        let message = if running {
            "Gortex daemon 正在运行。".to_string()
        } else if managed_installed {
            "Gortex 已安装，daemon 处于停止状态。".to_string()
        } else if any_process_running {
            "检测到未受管 Gortex 进程；仅用于检测，请先安装受管版本。".to_string()
        } else {
            "未检测到受管 Gortex；请先安装受管版本。".to_string()
        };

        let default_project = std::env::current_dir()
            .map(|d| clean_path_str(&d.to_string_lossy()))
            .unwrap_or_default();

        GortexStatusResponse {
            path: exe_path.to_string_lossy().to_string(),
            managed_root: install_root.to_string_lossy().to_string(),
            managed_root_exists: install_root.is_dir(),
            version,
            installed,
            managed_installed,
            installing: is_installing,
            running,
            any_process_running,
            unmanaged_process_running,
            process_id,
            activation_state,

            codex_available,
            codex_configured,
            codex_complete,
            claude_available,
            claude_configured,
            claude_complete,
            cursor_available,
            cursor_configured,
            cursor_complete,
            copilot_available,
            copilot_configured,
            copilot_complete,
            opencode_available,
            opencode_configured,
            opencode_complete,
            antigravity_available,
            antigravity_configured,
            antigravity_complete,
            gemini_available,
            gemini_configured,
            gemini_complete,

            codex_prompt,
            codex_prompt_complete,
            claude_prompt,
            claude_prompt_complete,
            cursor_prompt,
            cursor_prompt_complete,
            copilot_prompt,
            copilot_prompt_complete,
            opencode_prompt,
            opencode_prompt_complete,
            antigravity_prompt,
            antigravity_prompt_complete,
            gemini_prompt,
            gemini_prompt_complete,

            codex_hook,
            codex_hook_complete,
            claude_hook,
            claude_hook_complete,
            copilot_hook,
            copilot_hook_complete,
            opencode_hook,
            opencode_hook_complete,
            antigravity_hook,
            antigravity_hook_complete,
            gemini_hook,
            gemini_hook_complete,

            user_path,
            system_path,
            codex_trust_status: Some(trust_info.status),
            codex_trust_required: trust_info.required,
            codex_trust_notice: Some(trust_info.notice),
            codex_trust_steps: Some(trust_info.steps),

            tracked_projects,
            project_mcp_enabled,
            project_mcp_projects: Some(project_mcp_projects),
            default_project,
            integration_present,
            active_task: Self::get_active_task(),
            message,
        }
    }

    pub fn stop_all_processes(timeout: Duration) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        while is_process_running(GORTEX_EXECUTABLE_NAME) {
            let _ = crate::common::process::kill_process_by_name(GORTEX_EXECUTABLE_NAME);
            if Instant::now() >= deadline {
                if is_process_running(GORTEX_EXECUTABLE_NAME) {
                    return Err("部分 Gortex 进程仍未退出".to_string());
                }
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        Ok(())
    }

    pub async fn fetch_releases(
        page: usize,
        proxy_url: Option<&str>,
    ) -> Result<GortexReleaseListResponse, String> {
        let releases = fetch_github_releases(GORTEX_REPO, page, 5, proxy_url).await?;
        let options = releases
            .into_iter()
            .filter(|r| !r.tag_name.trim().is_empty())
            .map(|rel| {
                let asset = rel.assets.iter().find(|a| a.name == GORTEX_WINDOWS_ASSET_NAME);
                let (available, asset_name) = match asset {
                    Some(a) => (true, a.name.clone()),
                    None => (false, String::new()),
                };

                GortexReleaseOption {
                    tag_name: rel.tag_name,
                    name: rel.name.unwrap_or_default(),
                    published_at: rel.published_at.unwrap_or_default(),
                    prerelease: rel.prerelease.unwrap_or(false),
                    available,
                    asset_name,
                }
            })
            .collect();

        Ok(GortexReleaseListResponse {
            releases: options,
            page,
            per_page: 5,
            has_more: true,
        })
    }

    pub async fn install(tag_name: Option<String>, proxy_url: Option<&str>) -> Result<(), String> {
        {
            let mut lock = INSTALLING.lock().unwrap();
            if *lock {
                return Err("Gortex 正在安装中，请等待当前任务完成".to_string());
            }
            *lock = true;
        }

        let res = Self::install_inner(tag_name, proxy_url).await;
        let mut lock = INSTALLING.lock().unwrap();
        *lock = false;
        res
    }

    async fn install_inner(tag_name: Option<String>, proxy_url: Option<&str>) -> Result<(), String> {
        let install_root = Self::get_install_dir();
        let bin_dir = install_root.join("bin");

        for dir in &[
            &install_root,
            &bin_dir,
            &install_root.join("config"),
            &install_root.join("data"),
            &install_root.join("cache"),
            &install_root.join("run"),
        ] {
            fs::create_dir_all(dir).map_err(|e| format!("创建目录失败 ({}): {}", dir.display(), e))?;
        }

        let _ = Self::stop_all_processes(GORTEX_PROCESS_STOP_TIMEOUT);

        let releases = fetch_github_releases(GORTEX_REPO, 1, 10, proxy_url).await?;
        let target_release = if let Some(tag) = &tag_name {
            releases.into_iter().find(|r| r.tag_name == *tag)
        } else {
            releases.into_iter().next()
        }.ok_or_else(|| "未找到匹配的 Gortex 发布版本".to_string())?;

        let zip_asset = target_release
            .assets
            .iter()
            .find(|a| a.name == GORTEX_WINDOWS_ASSET_NAME)
            .ok_or_else(|| format!("版本 {} 没有 Windows 安装包 {}", target_release.tag_name, GORTEX_WINDOWS_ASSET_NAME))?;

        let checksum_asset = target_release
            .assets
            .iter()
            .find(|a| a.name == "checksums.txt");

        let client = reqwest::Client::builder();
        let client = if let Some(p) = proxy_url {
            client.proxy(reqwest::Proxy::all(p).map_err(|e| format!("代理配置错误: {}", e))?)
        } else {
            client
        };
        let client = client.build().map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;

        // 1. Download ZIP to temp dir
        let temp_dir = std::env::temp_dir();
        let temp_zip = temp_dir.join(format!("gortex-{}.zip", std::process::id()));
        info!("Downloading Gortex ZIP from {}", zip_asset.browser_download_url);

        let zip_resp = client
            .get(&zip_asset.browser_download_url)
            .header("User-Agent", "code-Manager-rust")
            .send()
            .await
            .map_err(|e| format!("下载 Gortex ZIP 失败: {}", e))?;

        let zip_bytes = zip_resp.bytes().await.map_err(|e| format!("读取 Gortex ZIP 数据失败: {}", e))?;
        fs::write(&temp_zip, &zip_bytes).map_err(|e| format!("写入临时 ZIP 文件失败: {}", e))?;

        // 2. Validate Checksum
        if let Some(cs_asset) = checksum_asset {
            if let Ok(cs_resp) = client.get(&cs_asset.browser_download_url).header("User-Agent", "code-Manager-rust").send().await {
                if let Ok(cs_text) = cs_resp.text().await {
                    for line in cs_text.lines() {
                        if line.contains(GORTEX_WINDOWS_ASSET_NAME) {
                            let parts: Vec<&str> = line.split_whitespace().collect();
                            if let Some(expected_hash) = parts.first() {
                                let mut hasher = Sha256::new();
                                hasher.update(&zip_bytes);
                                let actual_hash = hex::encode(hasher.finalize());
                                if !actual_hash.eq_ignore_ascii_case(expected_hash) {
                                    let _ = fs::remove_file(&temp_zip);
                                    return Err(format!("Gortex 安装包 SHA-256 校验失败: 期望 {}, 实际 {}", expected_hash, actual_hash));
                                }
                                info!("Gortex ZIP checksum verification passed.");
                                break;
                            }
                        }
                    }
                }
            }
        }

        // 3. Extract to staging directory
        let staging_dir = bin_dir.join(format!(".gortex-install-{}", std::process::id()));
        let _ = fs::create_dir_all(&staging_dir);

        let file = fs::File::open(&temp_zip).map_err(|e| format!("打开临时 ZIP 失败: {}", e))?;
        let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("解析 Gortex ZIP 失败: {}", e))?;

        let mut found_exe = false;
        let staged_exe = staging_dir.join(GORTEX_EXECUTABLE_NAME);

        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(|e| format!("读取 ZIP 条目失败: {}", e))?;
            let name = entry.name().to_string();
            if name.ends_with(GORTEX_EXECUTABLE_NAME) {
                let mut out_file = fs::File::create(&staged_exe).map_err(|e| format!("解压 Gortex 可执行文件失败: {}", e))?;
                std::io::copy(&mut entry, &mut out_file).map_err(|e| format!("写入解压文件失败: {}", e))?;
                found_exe = true;
                break;
            }
        }
        let _ = fs::remove_file(&temp_zip);

        if !found_exe || !staged_exe.is_file() {
            let _ = fs::remove_dir_all(&staging_dir);
            return Err("Gortex ZIP 中未找到 gortex.exe".to_string());
        }

        // 4. Atomic replacement with backup
        let target_exe = bin_dir.join(GORTEX_EXECUTABLE_NAME);
        let backup_exe = bin_dir.join(format!(".gortex-backup-{}", std::process::id()));

        if target_exe.is_file() {
            let _ = fs::rename(&target_exe, &backup_exe);
        }

        if let Err(e) = fs::rename(&staged_exe, &target_exe) {
            if backup_exe.is_file() {
                let _ = fs::rename(&backup_exe, &target_exe);
            }
            let _ = fs::remove_dir_all(&staging_dir);
            return Err(format!("替换 Gortex 可执行文件失败: {}", e));
        }

        if backup_exe.is_file() {
            let _ = fs::remove_file(&backup_exe);
        }
        let _ = fs::remove_dir_all(&staging_dir);

        // 5. Release prompts and configure PATH
        let _ = fs::write(install_root.join("gortex提示词.md"), GORTEX_PROMPT_DOC);
        let _ = fs::write(install_root.join(".gortex-version"), &target_release.tag_name);
        let _ = configure_gortex_path(&bin_dir);

        // 6. Invalidate caches
        {
            let mut lock = CACHED_VERSION.lock().unwrap();
            *lock = None;
        }

        Ok(())
    }

    pub fn start_daemon() -> Result<(), String> {
        let exe = Self::get_executable_path();
        if !exe.is_file() {
            return Err("gortex.exe 不存在，请先安装 Gortex".to_string());
        }

        let mut cmd = new_silent_command(&exe);
        cmd.args(["daemon", "start", "--detach"]);
        for (k, v) in Self::get_managed_env_vars(&exe) {
            cmd.env(k, v);
        }

        cmd.spawn().map_err(|e| format!("启动 Gortex daemon 失败: {}", e))?;

        // Wait up to 15 seconds for socket/pid
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            let (running, _) = Self::get_daemon_status();
            if running {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(200));
        }

        Ok(())
    }

    pub fn stop_daemon() -> Result<(), String> {
        let exe = Self::get_executable_path();
        if exe.is_file() {
            let mut cmd = new_silent_command(&exe);
            cmd.args(["daemon", "stop"]);
            for (k, v) in Self::get_managed_env_vars(&exe) {
                cmd.env(k, v);
            }
            let _ = cmd.output();
        }

        let _ = Self::stop_all_processes(GORTEX_PROCESS_STOP_TIMEOUT);
        Ok(())
    }

    pub fn register_mcp() -> Result<Vec<String>, String> {
        let exe = Self::get_executable_path();
        let exe_str = exe.to_string_lossy().to_string();
        let install_root = Self::get_install_dir();

        let mut warnings = Vec::new();

        // 1. Release prompt document
        let _ = fs::write(install_root.join("gortex提示词.md"), GORTEX_PROMPT_DOC);

        let available = gortex_detected_mcp_agents();

        // 2. Register Codex (.codex/config.toml)
        if available.get("codex").copied().unwrap_or(false) {
            if let Err(e) = update_codex_mcp_config_owned(&exe_str, &install_root, false) {
                warnings.push(format!("Codex MCP: {}", e));
            }
        }

        // 3. Register JSON configs (Claude, Cursor, Copilot, Antigravity, Gemini)
        for agent in &["claude", "cursor", "copilot", "antigravity", "gemini"] {
            if available.get(*agent).copied().unwrap_or(false) {
                if let Err(e) = update_json_mcp_config_owned(agent, &exe_str, &install_root, false) {
                    warnings.push(format!("{} MCP: {}", agent, e));
                }
            }
        }

        // 4. Clean legacy Antigravity
        let _ = remove_legacy_antigravity_mcp(&install_root);

        // 5. Register OpenCode
        if available.get("opencode").copied().unwrap_or(false) {
            if let Err(e) = update_opencode_mcp_config_owned(&exe_str, &install_root, false) {
                warnings.push(format!("OpenCode MCP: {}", e));
            }
        }

        // 6. Register Prompts
        for agent in GORTEX_AGENTS {
            if available.get(*agent).copied().unwrap_or(false) {
                for p in gortex_prompt_paths(agent) {
                    if let Err(e) = upsert_gortex_prompt(&p, GORTEX_PROMPT_DOC) {
                        warnings.push(format!("{} 提示词: {}", agent, e));
                    }
                }
            }
        }

        // 7. Register Hooks
        if available.get("claude").copied().unwrap_or(false) {
            if let Some(p) = gortex_hook_path("claude") {
                if let Err(e) = upsert_claude_hooks(&p, &exe_str) {
                    warnings.push(format!("Claude Hook: {}", e));
                }
            }
        }
        if available.get("codex").copied().unwrap_or(false) {
            if let Some(p) = gortex_hook_path("codex") {
                if let Err(e) = upsert_codex_hooks(&p, &exe_str) {
                    warnings.push(format!("Codex Hook: {}", e));
                }
            }
        }
        if available.get("copilot").copied().unwrap_or(false) {
            if let Some(p) = gortex_hook_path("copilot") {
                if let Err(e) = upsert_copilot_hooks(&p, &exe_str) {
                    warnings.push(format!("Copilot Hook: {}", e));
                }
            }
        }
        if available.get("opencode").copied().unwrap_or(false) {
            if let Some(p) = gortex_hook_path("opencode") {
                if let Err(e) = upsert_opencode_plugin(&p, &exe_str) {
                    warnings.push(format!("OpenCode Hook: {}", e));
                }
            }
        }
        if available.get("antigravity").copied().unwrap_or(false) {
            if let Some(p) = gortex_hook_path("antigravity") {
                if let Err(e) = upsert_gemini_hooks(&p, "antigravity", &exe_str) {
                    warnings.push(format!("Antigravity Hook: {}", e));
                }
            }
        }
        if available.get("gemini").copied().unwrap_or(false) {
            if let Some(p) = gortex_hook_path("gemini") {
                if let Err(e) = upsert_gemini_hooks(&p, "gemini", &exe_str) {
                    warnings.push(format!("Gemini Hook: {}", e));
                }
            }
        }

        // 8. Enable project MCP and register for all tracked projects
        let mut ownership = read_gortex_ownership(&install_root)?;
        ownership.project_mcp_enabled = true;
        let _ = write_gortex_ownership(&install_root, &ownership);

        let tracked = Self::get_tracked_projects();
        for proj in &tracked {
            warnings.extend(gortex_register_project_mcp_for_project(proj, &exe_str, &install_root, Some(&available)));
            if available.get("cursor").copied().unwrap_or(false) {
                let rule_path = gortex_cursor_rule_path(proj);
                if let Err(e) = upsert_gortex_cursor_prompt(&rule_path, GORTEX_PROMPT_DOC) {
                    warnings.push(format!("Cursor 项目规则: {}", e));
                }
            }
        }

        Ok(warnings)
    }

    pub fn remove_mcp() -> Result<Vec<String>, String> {
        let exe = Self::get_executable_path();
        let exe_str = exe.to_string_lossy().to_string();
        let install_root = Self::get_install_dir();

        let mut warnings = Vec::new();

        // 1. Remove project-level MCP from all tracked projects
        let tracked = Self::get_tracked_projects();
        for proj in &tracked {
            warnings.extend(gortex_remove_project_mcp(&exe_str, proj, &install_root));
            let rule_path = gortex_cursor_rule_path(proj);
            let _ = remove_gortex_prompt(&rule_path, "");
        }

        // 2. Remove Codex MCP
        if let Err(e) = update_codex_mcp_config_owned("", &install_root, true) {
            warnings.push(format!("Codex: {}", e));
        }

        // 3. Remove JSON MCPs
        for agent in &["claude", "cursor", "copilot", "antigravity", "gemini"] {
            if let Err(e) = update_json_mcp_config_owned(agent, "", &install_root, true) {
                warnings.push(format!("{}: {}", agent, e));
            }
        }
        let _ = remove_legacy_antigravity_mcp(&install_root);

        // 4. Remove OpenCode MCP
        if let Err(e) = update_opencode_mcp_config_owned("", &install_root, true) {
            warnings.push(format!("OpenCode: {}", e));
        }

        // 5. Remove Prompts
        for agent in GORTEX_AGENTS {
            for p in gortex_prompt_paths(agent) {
                let _ = remove_gortex_prompt(&p, "");
            }
        }

        // 6. Remove Hooks
        if let Some(p) = gortex_hook_path("claude") {
            let _ = remove_claude_hooks(&p);
        }
        if let Some(p) = gortex_hook_path("codex") {
            let _ = remove_codex_hooks(&p);
        }
        if let Some(p) = gortex_hook_path("copilot") {
            let _ = remove_copilot_hooks(&p);
        }
        if let Some(p) = gortex_hook_path("opencode") {
            let _ = remove_opencode_plugin(&p);
        }
        if let Some(p) = gortex_hook_path("antigravity") {
            let _ = remove_gemini_hooks(&p);
        }

        let mut ownership = read_gortex_ownership(&install_root).unwrap_or_default();
        ownership.project_mcp_enabled = false;
        let _ = write_gortex_ownership(&install_root, &ownership);

        Ok(warnings)
    }

    pub fn run_diagnostics() -> GortexDiagnosticsResponse {
        let exe = Self::get_executable_path();
        if !exe.is_file() {
            return GortexDiagnosticsResponse {
                doctor_ok: false,
                doctor_output: String::new(),
                doctor_error: "未找到 gortex.exe，请先安装 Gortex".to_string(),
                status_ok: false,
                status_output: String::new(),
                status_error: "未找到 gortex.exe，请先安装 Gortex".to_string(),
                message: "未找到 gortex.exe，请先安装".to_string(),
            };
        }

        let doctor_res = new_silent_command(&exe).args(["doctor", "--json"]).output();
        let (doctor_ok, doctor_output, doctor_error) = match doctor_res {
            Ok(out) => (
                out.status.success(),
                String::from_utf8_lossy(&out.stdout).to_string(),
                String::from_utf8_lossy(&out.stderr).to_string(),
            ),
            Err(e) => (false, String::new(), e.to_string()),
        };

        let status_res = new_silent_command(&exe).arg("status").output();
        let (status_ok, status_output, status_error) = match status_res {
            Ok(out) => (
                out.status.success(),
                String::from_utf8_lossy(&out.stdout).to_string(),
                String::from_utf8_lossy(&out.stderr).to_string(),
            ),
            Err(e) => (false, String::new(), e.to_string()),
        };

        GortexDiagnosticsResponse {
            doctor_ok,
            doctor_output,
            doctor_error,
            status_ok,
            status_output,
            status_error,
            message: "已执行 gortex doctor --json 与 gortex status。".to_string(),
        }
    }

    pub async fn track_project(path_str: &str) -> Result<GortexOperationResponse, String> {
        let clean_input = clean_path_str(path_str);
        let p = Path::new(&clean_input);
        if !p.is_dir() {
            return Err(format!("目标项目路径不是有效目录: {}", clean_input));
        }

        let canonical = canonicalize_clean(p).map_err(|e| format!("解析路径失败: {}", e))?;
        let clean_path = clean_path_str(&canonical.to_string_lossy());

        Self::set_active_task(&clean_path, "track");
        let res = Self::track_project_inner(&clean_path).await;
        Self::clear_active_task();
        res
    }

    async fn track_project_inner(clean_path: &str) -> Result<GortexOperationResponse, String> {
        let exe = Self::get_managed_executable_path()
            .ok_or_else(|| "未检测到受管 Gortex，请先安装到同级目录".to_string())?;

        let (running, _) = Self::get_daemon_status();
        if !running {
            Self::start_daemon()?;
        }

        info!("Starting gortex track on {}", clean_path);

        // 1. 立即持久化登记并同步配置，让 Daemon 的权威列表能及时读取到此项目（打破循环死锁）
        let mut tracked = Self::get_tracked_projects();
        if !tracked.iter().any(|p| clean_path_str(p).eq_ignore_ascii_case(clean_path)) {
            tracked.push(clean_path.to_string());
        }
        Self::save_tracked_projects(&tracked)?;
        let _ = reconcile_gortex_workspaces("default");

        // 2. 调用 gortex track 通知 daemon 执行建图（去除 --wait 死锁参数，设置 60 秒安全时限）
        let track_res = Self::run_gortex_command_with_timeout(
            Duration::from_secs(60),
            &exe,
            &["track", clean_path],
        ).await;

        if let Err(e) = track_res {
            tracing::warn!("gortex track trigger output: {}", e);
        }

        let mut warnings = Vec::new();
        if let Err(e) = ensure_gortex_watch_config(clean_path) {
            warnings.push(format!("自动监视配置: {}", e));
        }

        let _ = reconcile_gortex_workspaces("default");

        let install_root = Self::get_install_dir();
        let ownership = read_gortex_ownership(&install_root).unwrap_or_default();
        let available = gortex_detected_mcp_agents();
        let exe_str = exe.to_string_lossy().to_string();

        if ownership.project_mcp_enabled {
            warnings.extend(gortex_register_project_mcp_for_project(clean_path, &exe_str, &install_root, Some(&available)));
        }

        if available.get("cursor").copied().unwrap_or(false) {
            let rule_path = gortex_cursor_rule_path(clean_path);
            if let Err(e) = upsert_gortex_cursor_prompt(&rule_path, GORTEX_PROMPT_DOC) {
                warnings.push(format!("Cursor 项目规则: {}", e));
            }
        }

        let msg = if warnings.is_empty() {
            "已建立 Gortex 项目代码图谱；后续深度分析由 AI 按任务需要调用 MCP。".to_string()
        } else {
            "已建立 Gortex 项目代码图谱，但部分接入配置需要处理。".to_string()
        };

        Ok(GortexOperationResponse {
            message: msg,
            warnings: if warnings.is_empty() { None } else { Some(warnings) },
        })
    }

    pub async fn untrack_project(path_str: &str) -> Result<GortexOperationResponse, String> {
        let clean = clean_path_str(path_str);
        Self::set_active_task(&clean, "untrack");
        let res = Self::untrack_project_inner(&clean).await;
        Self::clear_active_task();
        res
    }

    async fn untrack_project_inner(clean_path: &str) -> Result<GortexOperationResponse, String> {
        let exe = Self::get_managed_executable_path()
            .ok_or_else(|| "未检测到受管 Gortex，请先安装到同级目录".to_string())?;

        let _ = Self::run_gortex_command_with_timeout(
            GORTEX_UNTRACK_TIMEOUT,
            &exe,
            &["untrack", clean_path],
        ).await;

        let verbatim_path = format!(r"\\?\{}", clean_path);
        let _ = Self::run_gortex_command_with_timeout(
            Duration::from_secs(10),
            &exe,
            &["untrack", &verbatim_path],
        ).await;

        let mut tracked = Self::get_tracked_projects();
        tracked.retain(|p| {
            let cp = clean_path_str(p);
            !cp.eq_ignore_ascii_case(clean_path) && !p.eq_ignore_ascii_case(clean_path)
        });
        Self::save_tracked_projects(&tracked)?;

        let install_root = Self::get_install_dir();
        let exe_str = exe.to_string_lossy().to_string();
        let warnings = gortex_remove_project_mcp(&exe_str, clean_path, &install_root);

        let rule_path = gortex_cursor_rule_path(clean_path);
        let _ = remove_gortex_prompt(&rule_path, "");

        let _ = reconcile_gortex_workspaces("default");

        let msg = if warnings.is_empty() {
            "已取消该项目的 Gortex track；不会删除项目文件。".to_string()
        } else {
            "已取消该项目的 Gortex track，但部分项目接入配置需要处理。".to_string()
        };

        Ok(GortexOperationResponse {
            message: msg,
            warnings: if warnings.is_empty() { None } else { Some(warnings) },
        })
    }

    pub fn trust_codex() -> Result<GortexOperationResponse, String> {
        open_gortex_codex_trust_shell()?;
        Ok(GortexOperationResponse {
            message: "已打开 Codex 控制台，请在提示 Hooks need review 时选择 Trust all and continue 并按回车。".to_string(),
            warnings: None,
        })
    }

    pub fn uninstall() -> Result<GortexOperationResponse, String> {
        let (running, _) = Self::get_daemon_status();
        if running {
            return Err("Gortex daemon 正在运行，请先停止 daemon 后再卸载".to_string());
        }

        let mut warnings = Vec::new();

        // 1. Remove all MCP and hook integrations
        if let Ok(w) = Self::remove_mcp() {
            warnings.extend(w);
        }

        // 2. Stop all processes
        let _ = Self::stop_all_processes(GORTEX_PROCESS_STOP_TIMEOUT);

        // 3. Clear tracked projects registry
        let _ = Self::save_tracked_projects(&[]);

        // 4. Remove PATH entries
        let install_root = Self::get_install_dir();
        let bin_dir = install_root.join("bin");
        let (user_path, system_path, _) = query_gortex_path_status(&bin_dir);
        let _ = remove_owned_gortex_path(&bin_dir, user_path, system_path);

        // 5. Remove Gortex directory
        if install_root.is_dir() {
            if let Err(e) = fs::remove_dir_all(&install_root) {
                warnings.push(format!("删除 Gortex 目录失败: {}", e));
            }
        }

        Ok(GortexOperationResponse {
            message: "Gortex MCP、daemon、项目记录及同级 Gortex 目录已清理；其他 MCP 和项目文件保留。".to_string(),
            warnings: if warnings.is_empty() { None } else { Some(warnings) },
        })
    }
}
