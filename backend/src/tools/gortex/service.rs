use super::types::{
    GortexDiagnosticsResponse, GortexReleaseListResponse, GortexReleaseOption,
    GortexStatusResponse,
};
use crate::common::downloader::{download_and_extract_zip, fetch_github_releases};
use crate::common::process::{find_process_id, is_process_running};
use crate::common::windows::{add_to_user_path, get_app_data_dir, get_user_profile_dir, is_in_user_path, remove_from_user_path};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use tracing::info;

const GORTEX_REPO: &str = "zzet/gortex";
const GORTEX_PROMPT_DOC: &str = include_str!("../../../assets/gortex提示词.md");

static TRACKED: OnceLock<Arc<Mutex<Vec<String>>>> = OnceLock::new();

fn get_tracked() -> &'static Arc<Mutex<Vec<String>>> {
    TRACKED.get_or_init(|| {
        let default_list = vec!["C:\\EXEXX\\edit-rust".to_string()];
        let state_file = GortexService::get_install_dir().join("tracked_projects.json");
        if let Ok(content) = fs::read_to_string(state_file) {
            if let Ok(list) = serde_json::from_str::<Vec<String>>(&content) {
                return Arc::new(Mutex::new(list));
            }
        }
        Arc::new(Mutex::new(default_list))
    })
}

fn save_tracked() {
    let list = get_tracked().lock().unwrap().clone();
    let state_file = GortexService::get_install_dir().join("tracked_projects.json");
    if let Some(p) = state_file.parent() {
        let _ = fs::create_dir_all(p);
    }
    if let Ok(data) = serde_json::to_string_pretty(&list) {
        let _ = fs::write(state_file, data);
    }
}

pub struct GortexService;

impl GortexService {
    pub fn get_install_dir() -> PathBuf {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                return parent.join("Gortex");
            }
        }
        get_user_profile_dir().join(".gortex")
    }

    pub fn get_executable_path() -> PathBuf {
        let root = Self::get_install_dir();
        let bin_exe = root.join("bin").join("gortex.exe");
        if bin_exe.exists() {
            return bin_exe;
        }
        let direct_exe = root.join("gortex.exe");
        if direct_exe.exists() {
            return direct_exe;
        }
        bin_exe
    }

    fn config_path_for_agent(agent: &str) -> Option<PathBuf> {
        let profile = get_user_profile_dir();
        let appdata = get_app_data_dir();

        match agent {
            "codex" => Some(profile.join(".codex").join("config.toml")),
            "claude" => {
                let desktop = appdata.join("Claude").join("claude_desktop_config.json");
                if desktop.exists() {
                    Some(desktop)
                } else {
                    Some(profile.join(".claude.json"))
                }
            }
            "cursor" => Some(profile.join(".cursor").join("mcp.json")),
            "copilot" => Some(profile.join(".copilot").join("mcp-config.json")),
            "opencode" => Some(profile.join(".opencode").join("opencode.json")),
            "antigravity" => Some(profile.join(".gemini").join("antigravity").join("mcp_config.json")),
            "gemini" => Some(profile.join(".gemini").join("settings.json")),
            _ => None,
        }
    }

    pub fn get_status() -> GortexStatusResponse {
        let exe_path = Self::get_executable_path();
        let install_dir = Self::get_install_dir();
        let managed_installed = exe_path.exists() && exe_path.is_file();
        let running = is_process_running("gortex.exe");
        let pid = find_process_id("gortex.exe").unwrap_or(0);

        let projects = get_tracked().lock().unwrap().clone();
        let user_path = is_in_user_path(&install_dir) || is_in_user_path(&install_dir.join("bin"));

        // Helper to check MCP integration in a JSON file
        let check_json_mcp = |path: Option<PathBuf>| -> (bool, bool) {
            if let Some(p) = path {
                let dir_avail = p.parent().map(|parent| parent.is_dir()).unwrap_or(false);
                let configured = p.exists() && fs::read_to_string(&p).map(|s| s.contains("\"gortex\"")).unwrap_or(false);
                (dir_avail, configured)
            } else {
                (false, false)
            }
        };

        // Check Codex (TOML)
        let codex_file = Self::config_path_for_agent("codex");
        let codex_available = codex_file.as_ref().and_then(|p| p.parent()).map(|p| p.is_dir()).unwrap_or(false);
        let codex_configured = codex_file.as_ref().map(|p| p.exists() && fs::read_to_string(p).map(|s| s.contains("[mcp_servers.gortex]")).unwrap_or(false)).unwrap_or(false);

        let (claude_avail, claude_conf) = check_json_mcp(Self::config_path_for_agent("claude"));
        let (cursor_avail, cursor_conf) = check_json_mcp(Self::config_path_for_agent("cursor"));
        let (copilot_avail, copilot_conf) = check_json_mcp(Self::config_path_for_agent("copilot"));
        let (opencode_avail, opencode_conf) = check_json_mcp(Self::config_path_for_agent("opencode"));
        let (antigravity_avail, antigravity_conf) = check_json_mcp(Self::config_path_for_agent("antigravity"));
        let (gemini_avail, gemini_conf) = check_json_mcp(Self::config_path_for_agent("gemini"));

        let prompt_file = install_dir.join("gortex提示词.md");
        let prompt_installed = prompt_file.exists();

        let message = if running {
            "Gortex daemon 正在运行。".to_string()
        } else if managed_installed {
            "Gortex 已安装，daemon 处于停止状态。".to_string()
        } else {
            "未检测到受管 Gortex；请先安装受管版本。".to_string()
        };

        let trust_steps = vec![
            "等待 PowerShell 中的 Codex CLI 界面显示 Hooks need review。".to_string(),
            "选择 Trust all and continue 并按回车。".to_string(),
        ];

        GortexStatusResponse {
            path: exe_path.to_string_lossy().to_string(),
            managed_installed,
            managed_root_exists: install_dir.exists(),
            version: if managed_installed { "0.64.4".to_string() } else { String::new() },
            installing: false,
            installed: managed_installed,
            running,
            any_process_running: running,
            unmanaged_process_running: false,
            integration_present: codex_configured || claude_conf || cursor_conf,
            process_id: pid,
            activation_state: if running {
                "running".to_string()
            } else if managed_installed {
                "installed_stopped".to_string()
            } else {
                "not_installed".to_string()
            },

            codex_available,
            codex_configured,
            codex_complete: codex_configured,
            claude_available: claude_avail,
            claude_configured: claude_conf,
            claude_complete: claude_conf,
            cursor_available: cursor_avail,
            cursor_configured: cursor_conf,
            cursor_complete: cursor_conf,
            copilot_available: copilot_avail,
            copilot_configured: copilot_conf,
            copilot_complete: copilot_conf,
            open_code_available: opencode_avail,
            open_code_configured: opencode_conf,
            open_code_complete: opencode_conf,
            antigravity_available: antigravity_avail,
            antigravity_configured: antigravity_conf,
            antigravity_complete: antigravity_conf,
            gemini_available: gemini_avail,
            gemini_configured: gemini_conf,
            gemini_complete: gemini_conf,

            codex_prompt: prompt_installed,
            codex_prompt_complete: prompt_installed,
            claude_prompt: prompt_installed,
            claude_prompt_complete: prompt_installed,
            cursor_prompt: prompt_installed,
            cursor_prompt_complete: prompt_installed,
            copilot_prompt: prompt_installed,
            copilot_prompt_complete: prompt_installed,
            open_code_prompt: prompt_installed,
            open_code_prompt_complete: prompt_installed,
            antigravity_prompt: prompt_installed,
            antigravity_prompt_complete: prompt_installed,
            gemini_prompt: prompt_installed,
            gemini_prompt_complete: prompt_installed,

            codex_hook: codex_configured,
            codex_hook_complete: codex_configured,
            claude_hook: claude_conf,
            claude_hook_complete: claude_conf,
            copilot_hook: copilot_conf,
            copilot_hook_complete: copilot_conf,
            open_code_hook: opencode_conf,
            open_code_hook_complete: opencode_conf,
            antigravity_hook: antigravity_conf,
            antigravity_hook_complete: antigravity_conf,
            gemini_hook: gemini_conf,
            gemini_hook_complete: gemini_conf,

            user_path,
            system_path: false,
            codex_trust_status: "trusted".to_string(),
            codex_trust_required: false,
            codex_trust_notice: String::new(),
            codex_trust_steps: trust_steps,

            tracked_projects: projects.clone(),
            project_mcp_enabled: true,
            project_mcp_projects: projects,
            default_project: "C:\\EXEXX\\edit-rust".to_string(),
            status_known: true,
            message,
        }
    }

    pub async fn fetch_releases(page: usize, proxy_url: Option<&str>) -> Result<GortexReleaseListResponse, String> {
        let releases = fetch_github_releases(GORTEX_REPO, page, 5, proxy_url).await?;
        let options = releases
            .into_iter()
            .map(|rel| {
                let asset = rel.assets.iter().find(|a| a.name.ends_with(".zip") && (a.name.contains("windows") || a.name.contains("amd64")));
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
        let install_dir = Self::get_install_dir();
        fs::create_dir_all(&install_dir).map_err(|e| format!("创建 Gortex 目录失败: {}", e))?;

        let releases = fetch_github_releases(GORTEX_REPO, 1, 10, proxy_url).await?;
        let target_release = if let Some(tag) = &tag_name {
            releases.into_iter().find(|r| r.tag_name == *tag)
        } else {
            releases.into_iter().next()
        }.ok_or_else(|| "未找到匹配的 Gortex 发布版本".to_string())?;

        let asset = target_release
            .assets
            .iter()
            .find(|a| a.name.ends_with(".zip") && (a.name.contains("windows") || a.name.contains("amd64")))
            .ok_or_else(|| "未找到适用于 Windows 的 Gortex 资产包".to_string())?;

        info!("Downloading Gortex from {}", asset.browser_download_url);
        download_and_extract_zip(&asset.browser_download_url, &install_dir, proxy_url).await?;

        let _ = fs::write(install_dir.join("gortex提示词.md"), GORTEX_PROMPT_DOC);
        let _ = fs::write(install_dir.join(".gortex-version"), &target_release.tag_name);
        let _ = add_to_user_path(&install_dir);

        Ok(())
    }

    pub fn start_daemon() -> Result<(), String> {
        let exe = Self::get_executable_path();
        if !exe.exists() {
            return Err("gortex.exe 不存在，请先安装 Gortex".to_string());
        }

        let mut cmd = std::process::Command::new(&exe);
        cmd.args(["daemon", "start"]);

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }

        cmd.spawn().map_err(|e| format!("启动 Gortex daemon 失败: {}", e))?;
        Ok(())
    }

    pub fn stop_daemon() -> Result<(), String> {
        let exe = Self::get_executable_path();
        if exe.exists() {
            let _ = std::process::Command::new(&exe).args(["daemon", "stop"]).output();
        }

        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/IM", "gortex.exe"])
            .output();

        Ok(())
    }

    pub fn register_mcp() -> Result<Vec<String>, String> {
        let exe = Self::get_executable_path();
        let exe_str = exe.to_string_lossy().to_string();
        let install_dir = Self::get_install_dir();

        let _ = fs::write(install_dir.join("gortex提示词.md"), GORTEX_PROMPT_DOC);

        let mut warnings = Vec::new();

        // 1. Register Codex (.codex/config.toml)
        if let Some(codex_file) = Self::config_path_for_agent("codex") {
            if let Some(parent) = codex_file.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let mut content = fs::read_to_string(&codex_file).unwrap_or_default();
            if !content.contains("[mcp_servers.gortex]") {
                let toml_block = format!(
                    "\r\n[mcp_servers.gortex]\r\ncommand = \"{}\"\r\nargs = [\"mcp\"]\r\n",
                    exe_str.replace('\\', "\\\\")
                );
                content.push_str(&toml_block);
                if let Err(e) = fs::write(&codex_file, content) {
                    warnings.push(format!("Codex: {}", e));
                }
            }
        }

        // 2. Register JSON configs (Claude, Cursor, Copilot, OpenCode, Antigravity, Gemini)
        for agent in &["claude", "cursor", "copilot", "opencode", "antigravity", "gemini"] {
            if let Some(path) = Self::config_path_for_agent(agent) {
                if let Some(parent) = path.parent() {
                    let _ = fs::create_dir_all(parent);
                }

                let mut root: Value = if path.exists() {
                    fs::read_to_string(&path)
                        .ok()
                        .and_then(|s| serde_json::from_str(&s).ok())
                        .unwrap_or_else(|| serde_json::json!({}))
                } else {
                    serde_json::json!({})
                };

                let entry = serde_json::json!({
                    "command": exe_str,
                    "args": ["mcp"]
                });

                let servers = root.as_object_mut().and_then(|obj| {
                    if !obj.contains_key("mcpServers") {
                        obj.insert("mcpServers".to_string(), serde_json::json!({}));
                    }
                    obj.get_mut("mcpServers")?.as_object_mut()
                });

                if let Some(s) = servers {
                    s.insert("gortex".to_string(), entry);
                }

                if let Ok(serialized) = serde_json::to_string_pretty(&root) {
                    if let Err(e) = fs::write(&path, serialized) {
                        warnings.push(format!("{}: {}", agent, e));
                    }
                }
            }
        }

        Ok(warnings)
    }

    pub fn remove_mcp() -> Result<Vec<String>, String> {
        let warnings = Vec::new();

        // 1. Remove from Codex
        if let Some(codex_file) = Self::config_path_for_agent("codex") {
            if codex_file.exists() {
                if let Ok(content) = fs::read_to_string(&codex_file) {
                    if content.contains("[mcp_servers.gortex]") {
                        let lines: Vec<&str> = content.lines().collect();
                        let mut filtered = Vec::new();
                        let mut skipping = false;
                        for line in lines {
                            if line.trim().starts_with("[mcp_servers.gortex]") {
                                skipping = true;
                                continue;
                            }
                            if skipping && line.trim().starts_with('[') {
                                skipping = false;
                            }
                            if !skipping {
                                filtered.push(line);
                            }
                        }
                        let _ = fs::write(&codex_file, filtered.join("\r\n"));
                    }
                }
            }
        }

        // 2. Remove from JSON configs
        for agent in &["claude", "cursor", "copilot", "opencode", "antigravity", "gemini"] {
            if let Some(path) = Self::config_path_for_agent(agent) {
                if path.exists() {
                    if let Ok(content) = fs::read_to_string(&path) {
                        if let Ok(mut root) = serde_json::from_str::<Value>(&content) {
                            if let Some(servers) = root.get_mut("mcpServers").and_then(|s| s.as_object_mut()) {
                                servers.remove("gortex");
                            }
                            if let Ok(serialized) = serde_json::to_string_pretty(&root) {
                                let _ = fs::write(&path, serialized);
                            }
                        }
                    }
                }
            }
        }

        Ok(warnings)
    }

    pub fn run_diagnostics() -> GortexDiagnosticsResponse {
        let exe = Self::get_executable_path();
        if !exe.exists() {
            return GortexDiagnosticsResponse {
                doctor_ok: false,
                doctor_output: String::new(),
                doctor_error: "未找到 gortex.exe，请先安装 Gortex".to_string(),
                status_ok: false,
                status_output: String::new(),
                status_error: "未找到 gortex.exe，请先安装 Gortex".to_string(),
            };
        }

        let doctor_res = std::process::Command::new(&exe).arg("doctor").output();
        let (doctor_ok, doctor_output, doctor_error) = match doctor_res {
            Ok(out) => (
                out.status.success(),
                String::from_utf8_lossy(&out.stdout).to_string(),
                String::from_utf8_lossy(&out.stderr).to_string(),
            ),
            Err(e) => (false, String::new(), e.to_string()),
        };

        let status_res = std::process::Command::new(&exe).arg("status").output();
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
        }
    }

    pub fn track_project(path: &str) -> Result<(), String> {
        let p = Path::new(path);
        if !p.exists() {
            return Err(format!("目标项目路径不存在: {}", path));
        }

        let canonical = p.canonicalize().map_err(|e| format!("解析路径失败: {}", e))?;
        let path_str = canonical.to_string_lossy().to_string();

        {
            let mut list = get_tracked().lock().unwrap();
            if !list.contains(&path_str) {
                list.push(path_str.clone());
            }
        }
        save_tracked();

        let exe = Self::get_executable_path();
        if exe.exists() {
            let _ = std::process::Command::new(&exe).args(["project", "track", &path_str]).output();
        }

        Ok(())
    }

    pub fn untrack_project(path: &str) -> Result<(), String> {
        {
            let mut list = get_tracked().lock().unwrap();
            list.retain(|item| !item.eq_ignore_ascii_case(path));
        }
        save_tracked();

        let exe = Self::get_executable_path();
        if exe.exists() {
            let _ = std::process::Command::new(&exe).args(["project", "untrack", path]).output();
        }

        Ok(())
    }

    pub fn uninstall() -> Result<(), String> {
        let _ = Self::stop_daemon();
        let _ = Self::remove_mcp();

        let install_dir = Self::get_install_dir();
        let _ = remove_from_user_path(&install_dir);
        let _ = remove_from_user_path(&install_dir.join("bin"));

        if install_dir.exists() {
            fs::remove_dir_all(&install_dir).map_err(|e| format!("删除 Gortex 目录失败: {}", e))?;
        }

        Ok(())
    }
}
