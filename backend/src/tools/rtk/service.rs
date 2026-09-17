use super::types::{RtkReleaseListResponse, RtkReleaseOption, RtkStatusResponse};
use crate::common::downloader::{download_and_extract_zip, fetch_github_releases};
use crate::common::windows::{add_to_user_path, get_user_profile_dir, is_in_user_path, remove_from_user_path};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tracing::info;

const RTK_REPO: &str = "rtk-ai/rtk";
const RTK_ASSET_NAME: &str = "rtk-x86_64-pc-windows-msvc.zip";

const RTK_CODEX_COMMANDS: &str = include_str!("../../../assets/RTK-Codex-commands.md");
const RTK_CODEX_AGENT_INSTRUCTIONS: &str = include_str!("../../../assets/RTK-Codex-agent-instructions.md");
const RTK_CLAUDE_AGENT_INSTRUCTIONS: &str = include_str!("../../../assets/RTK-Claude-agent-instructions.md");

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct RtkToolState {
    pub running: bool,
    pub desired_running: bool,
    pub user_path: bool,
    pub system_path: bool,
}

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
        Self::get_install_dir().join(".rtk-state.json")
    }

    pub fn read_state() -> RtkToolState {
        let path = Self::state_file_path();
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(state) = serde_json::from_str::<RtkToolState>(&content) {
                return state;
            }
        }
        RtkToolState::default()
    }

    pub fn write_state(state: &RtkToolState) -> Result<(), String> {
        let path = Self::state_file_path();
        if let Some(p) = path.parent() {
            let _ = fs::create_dir_all(p);
        }
        let data = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
        fs::write(path, data).map_err(|e| e.to_string())
    }

    pub fn get_status() -> RtkStatusResponse {
        let exe_path = Self::get_executable_path();
        let install_dir = Self::get_install_dir();
        let installed = exe_path.exists() && exe_path.is_file();
        let directory_exists = install_dir.is_dir();

        let state = Self::read_state();
        let user_path = is_in_user_path(&install_dir);

        let user_profile = get_user_profile_dir();

        // Codex detection
        let codex_dir = user_profile.join(".codex");
        let codex_available = codex_dir.is_dir();
        let codex_cmd_file = install_dir.join("RTK-Codex-commands.md");
        let codex_configured = codex_cmd_file.exists();

        // Claude detection
        let claude_dir = user_profile.join(".claude");
        let claude_available = claude_dir.is_dir();
        let claude_settings = claude_dir.join("settings.json");
        let mut claude_hook_configured = false;
        if claude_settings.exists() {
            if let Ok(content) = fs::read_to_string(&claude_settings) {
                claude_hook_configured = content.contains("rtk hook claude") || content.contains("rtk.exe");
            }
        }
        let claude_prompt_file = install_dir.join("RTK-Claude-agent-instructions.md");
        let claude_prompt_configured = claude_prompt_file.exists();
        let claude_configured = claude_hook_configured && claude_prompt_configured;

        // Copilot detection
        let copilot_dir = user_profile.join(".copilot");
        let copilot_available = copilot_dir.is_dir();
        let copilot_hook_file = copilot_dir.join("hooks").join("rtk-rewrite.json");
        let copilot_configured = copilot_hook_file.exists();

        // Cursor detection
        let cursor_dir = user_profile.join(".cursor");
        let cursor_available = cursor_dir.is_dir();
        let cursor_configured = installed;

        let version = if installed {
            let version_file = install_dir.join(".rtk-version");
            fs::read_to_string(version_file).unwrap_or_else(|_| "v1.2.0".to_string()).trim().to_string()
        } else {
            String::new()
        };

        let running = installed && (state.running || user_path);
        let activation_state = if !installed {
            "not_installed".to_string()
        } else if running {
            "running".to_string()
        } else {
            "installed_stopped".to_string()
        };

        let mut modified_agents = Vec::new();
        if codex_configured {
            modified_agents.push("codex".to_string());
        }
        if claude_configured {
            modified_agents.push("claude".to_string());
        }
        if copilot_configured {
            modified_agents.push("copilot".to_string());
        }

        let message = if !installed {
            "RTK 尚未安装。".to_string()
        } else if running {
            "RTK 运行中，各编辑器规则与 PATH 已生效。".to_string()
        } else {
            "RTK 已安装但未启动。".to_string()
        };

        RtkStatusResponse {
            path: exe_path.to_string_lossy().to_string(),
            installed,
            directory_exists,
            version,
            user_path,
            system_path: false,
            codex_available,
            codex_configured,
            codex_residual: false,
            claude_available,
            claude_hook_configured,
            claude_prompt_configured,
            claude_configured,
            claude_residual: false,
            copilot_available,
            copilot_configured,
            cursor_available,
            cursor_configured,
            running,
            desired_running: state.desired_running,
            activation_state,
            blocked_by: String::new(),
            modified_agents,
            message,
        }
    }

    pub async fn fetch_releases(page: usize, proxy_url: Option<&str>) -> Result<RtkReleaseListResponse, String> {
        let releases = fetch_github_releases(RTK_REPO, page, 5, proxy_url).await?;
        let options = releases
            .into_iter()
            .map(|rel| {
                let asset = rel.assets.iter().find(|a| a.name.ends_with(".zip") || a.name.contains("windows"));
                let (available, asset_name) = match asset {
                    Some(a) => (true, a.name.clone()),
                    None => (false, String::new()),
                };

                RtkReleaseOption {
                    tag_name: rel.tag_name,
                    name: rel.name.unwrap_or_default(),
                    published_at: rel.published_at.unwrap_or_default(),
                    prerelease: rel.prerelease.unwrap_or(false),
                    available,
                    asset_name,
                }
            })
            .collect();

        Ok(RtkReleaseListResponse {
            releases: options,
            page,
            has_more: true,
        })
    }

    pub async fn install(tag_name: Option<String>, proxy_url: Option<&str>) -> Result<(), String> {
        let install_dir = Self::get_install_dir();
        fs::create_dir_all(&install_dir).map_err(|e| format!("创建 RTK 目录失败: {}", e))?;

        // 1. Fetch releases and find asset
        let releases = fetch_github_releases(RTK_REPO, 1, 10, proxy_url).await?;
        let target_release = if let Some(tag) = &tag_name {
            releases.into_iter().find(|r| r.tag_name == *tag)
        } else {
            releases.into_iter().next()
        }.ok_or_else(|| "未找到匹配的 RTK 发布版本".to_string())?;

        let asset = target_release
            .assets
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(RTK_ASSET_NAME) || (a.name.contains("windows") && a.name.ends_with(".zip")))
            .ok_or_else(|| "未找到适用于 Windows 的 RTK 资产包".to_string())?;

        // 2. Download and unpack
        info!("Downloading RTK from {}", asset.browser_download_url);
        download_and_extract_zip(&asset.browser_download_url, &install_dir, proxy_url).await?;

        // 3. Write embedded documents
        let _ = fs::write(install_dir.join("RTK-Codex-commands.md"), RTK_CODEX_COMMANDS);
        let _ = fs::write(install_dir.join("RTK-Codex-agent-instructions.md"), RTK_CODEX_AGENT_INSTRUCTIONS);
        let _ = fs::write(install_dir.join("RTK-Claude-agent-instructions.md"), RTK_CLAUDE_AGENT_INSTRUCTIONS);
        let _ = fs::write(install_dir.join(".rtk-version"), &target_release.tag_name);

        // 4. Update PATH
        let _ = add_to_user_path(&install_dir);

        let mut state = Self::read_state();
        state.running = true;
        state.desired_running = true;
        state.user_path = true;
        let _ = Self::write_state(&state);

        Ok(())
    }

    pub fn start() -> Result<(), String> {
        let install_dir = Self::get_install_dir();
        let exe = Self::get_executable_path();
        if !exe.exists() {
            return Err("rtk.exe 不存在，请先安装 RTK".to_string());
        }

        // Add to user PATH
        add_to_user_path(&install_dir)?;

        // Write docs
        let _ = fs::write(install_dir.join("RTK-Codex-commands.md"), RTK_CODEX_COMMANDS);
        let _ = fs::write(install_dir.join("RTK-Codex-agent-instructions.md"), RTK_CODEX_AGENT_INSTRUCTIONS);
        let _ = fs::write(install_dir.join("RTK-Claude-agent-instructions.md"), RTK_CLAUDE_AGENT_INSTRUCTIONS);

        // Configure Claude settings if available
        let user_profile = get_user_profile_dir();
        let claude_dir = user_profile.join(".claude");
        if claude_dir.is_dir() {
            let settings_file = claude_dir.join("settings.json");
            let mut root: serde_json::Value = if settings_file.exists() {
                fs::read_to_string(&settings_file)
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_else(|| serde_json::json!({}))
            } else {
                serde_json::json!({})
            };

            let hook_cmd = format!("\"{}\" hook claude", exe.to_string_lossy());
            let hook_entry = serde_json::json!({
                "matcher": ".*",
                "command": hook_cmd
            });

            let hooks = root.as_object_mut().and_then(|m| {
                if !m.contains_key("hooks") {
                    m.insert("hooks".to_string(), serde_json::json!({}));
                }
                m.get_mut("hooks")?.as_object_mut()
            });

            if let Some(h) = hooks {
                h.insert("PreToolUse".to_string(), serde_json::json!([hook_entry]));
            }

            if let Ok(serialized) = serde_json::to_string_pretty(&root) {
                let _ = fs::write(&settings_file, serialized);
            }
        }

        let mut state = Self::read_state();
        state.running = true;
        state.desired_running = true;
        state.user_path = true;
        Self::write_state(&state)?;

        Ok(())
    }

    pub fn stop() -> Result<(), String> {
        let install_dir = Self::get_install_dir();

        // Remove from PATH
        let _ = remove_from_user_path(&install_dir);

        // Remove Claude hook if present
        let user_profile = get_user_profile_dir();
        let claude_settings = user_profile.join(".claude").join("settings.json");
        if claude_settings.exists() {
            if let Ok(content) = fs::read_to_string(&claude_settings) {
                if let Ok(mut root) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(hooks) = root.get_mut("hooks").and_then(|h| h.as_object_mut()) {
                        hooks.remove("PreToolUse");
                    }
                    if let Ok(serialized) = serde_json::to_string_pretty(&root) {
                        let _ = fs::write(&claude_settings, serialized);
                    }
                }
            }
        }

        let mut state = Self::read_state();
        state.running = false;
        state.desired_running = false;
        state.user_path = false;
        Self::write_state(&state)?;

        Ok(())
    }

    pub fn uninstall() -> Result<(), String> {
        let _ = Self::stop();
        let install_dir = Self::get_install_dir();
        if install_dir.exists() {
            fs::remove_dir_all(&install_dir).map_err(|e| format!("删除 RTK 目录失败: {}", e))?;
        }
        Ok(())
    }
}
