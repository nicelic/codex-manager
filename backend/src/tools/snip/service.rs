use super::types::{SnipAgentState, SnipReleaseListResponse, SnipReleaseOption, SnipStatusResponse};
use crate::common::downloader::{download_and_extract_zip, fetch_github_releases};
use crate::common::windows::{add_to_user_path, get_user_profile_dir, is_in_user_path, launch_visible_powershell, remove_from_user_path};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tracing::info;

const SNIP_REPO: &str = "edouard-claude/snip";

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SnipToolState {
    pub running: bool,
    pub desired_running: bool,
    pub user_path: bool,
}

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
        Self::get_install_dir().join(".snip-state.json")
    }

    pub fn read_state() -> SnipToolState {
        let path = Self::state_file_path();
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(state) = serde_json::from_str::<SnipToolState>(&content) {
                return state;
            }
        }
        SnipToolState::default()
    }

    pub fn write_state(state: &SnipToolState) -> Result<(), String> {
        let path = Self::state_file_path();
        if let Some(p) = path.parent() {
            let _ = fs::create_dir_all(p);
        }
        let data = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
        fs::write(path, data).map_err(|e| e.to_string())
    }

    pub fn get_status() -> SnipStatusResponse {
        let exe_path = Self::get_executable_path();
        let install_dir = Self::get_install_dir();
        let installed = exe_path.exists() && exe_path.is_file();
        let directory_exists = install_dir.is_dir();

        let state = Self::read_state();
        let user_path = is_in_user_path(&install_dir);

        let user_profile = get_user_profile_dir();

        // Agents detection
        let codex_file = user_profile.join(".codex").join("config.toml");
        let codex_hook = codex_file.exists() && fs::read_to_string(&codex_file).map(|s| s.contains("snip")).unwrap_or(false);

        let claude_file = user_profile.join(".claude").join("settings.json");
        let claude_hook = claude_file.exists() && fs::read_to_string(&claude_file).map(|s| s.contains("snip")).unwrap_or(false);

        let agents = vec![
            SnipAgentState {
                name: "codex".to_string(),
                hook_exists: codex_hook,
                repair_needed: false,
                modified: codex_hook,
                trust: if codex_hook { "trusted".to_string() } else { "not_applicable".to_string() },
                configured: codex_hook,
                target_file: codex_file.to_string_lossy().to_string(),
            },
            SnipAgentState {
                name: "claude-code".to_string(),
                hook_exists: claude_hook,
                repair_needed: false,
                modified: claude_hook,
                trust: "trusted".to_string(),
                configured: claude_hook,
                target_file: claude_file.to_string_lossy().to_string(),
            },
        ];

        let version = if installed {
            let version_file = install_dir.join(".snip-version");
            fs::read_to_string(version_file).unwrap_or_else(|_| "v0.9.5".to_string()).trim().to_string()
        } else {
            String::new()
        };

        let running = installed && (state.running || codex_hook || claude_hook);
        let activation_state = if !installed {
            "not_installed".to_string()
        } else if running {
            "running".to_string()
        } else {
            "installed_stopped".to_string()
        };

        let mut modified_agents = Vec::new();
        if codex_hook {
            modified_agents.push("codex".to_string());
        }
        if claude_hook {
            modified_agents.push("claude-code".to_string());
        }

        let message = if !installed {
            "Snip 尚未安装。".to_string()
        } else if running {
            "Snip 运行中，已注入代码审核/裁剪 Hook。".to_string()
        } else {
            "Snip 已安装但未激活。".to_string()
        };

        let trust_steps = vec![
            "等待 PowerShell 中的 Codex CLI 界面显示 Hooks need review。不要输入 /hook 或 /hooks。".to_string(),
            "在当前 PowerShell 窗口选择第 2 项 Trust all and continue；不是输入数字 2。".to_string(),
            "按键盘 Enter（回车）确认；不需要先进入 Review hooks，也不需要输入 t。".to_string(),
            "回到 code-Manager 点击“刷新状态”，确认 Codex Hook 显示“已信任”。".to_string(),
        ];

        SnipStatusResponse {
            path: exe_path.to_string_lossy().to_string(),
            installed,
            directory_exists,
            version,
            user_path,
            system_path: false,
            running,
            desired_running: state.desired_running,
            activation_state,
            blocked_by: String::new(),
            trust_notice: String::new(),
            trust_status: "trusted".to_string(),
            trust_command: "codex".to_string(),
            trust_required: false,
            trust_steps,
            cleanup_required: false,
            modified_agents,
            agents,
            message,
        }
    }

    pub async fn fetch_releases(page: usize, proxy_url: Option<&str>) -> Result<SnipReleaseListResponse, String> {
        let releases = fetch_github_releases(SNIP_REPO, page, 5, proxy_url).await?;
        let options = releases
            .into_iter()
            .map(|rel| {
                let asset = rel.assets.iter().find(|a| a.name.ends_with(".zip") && (a.name.contains("windows") || a.name.contains("amd64")));
                let (available, asset_name) = match asset {
                    Some(a) => (true, a.name.clone()),
                    None => (false, String::new()),
                };

                SnipReleaseOption {
                    tag_name: rel.tag_name,
                    name: rel.name.unwrap_or_default(),
                    published_at: rel.published_at.unwrap_or_default(),
                    prerelease: rel.prerelease.unwrap_or(false),
                    available,
                    asset_name,
                }
            })
            .collect();

        Ok(SnipReleaseListResponse {
            releases: options,
            page,
            has_more: true,
        })
    }

    pub async fn install(tag_name: Option<String>, proxy_url: Option<&str>) -> Result<(), String> {
        let install_dir = Self::get_install_dir();
        fs::create_dir_all(&install_dir).map_err(|e| format!("创建 Snip 目录失败: {}", e))?;

        let releases = fetch_github_releases(SNIP_REPO, 1, 10, proxy_url).await?;
        let target_release = if let Some(tag) = &tag_name {
            releases.into_iter().find(|r| r.tag_name == *tag)
        } else {
            releases.into_iter().next()
        }.ok_or_else(|| "未找到匹配的 Snip 发布版本".to_string())?;

        let asset = target_release
            .assets
            .iter()
            .find(|a| a.name.ends_with(".zip") && (a.name.contains("windows") || a.name.contains("amd64")))
            .ok_or_else(|| "未找到适用于 Windows 的 Snip 资产包".to_string())?;

        info!("Downloading Snip from {}", asset.browser_download_url);
        download_and_extract_zip(&asset.browser_download_url, &install_dir, proxy_url).await?;

        let _ = fs::write(install_dir.join(".snip-version"), &target_release.tag_name);
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
            return Err("snip.exe 不存在，请先安装 Snip".to_string());
        }

        add_to_user_path(&install_dir)?;

        // If snip executable can run `snip init` or `snip hook install`, we can spawn it
        let _ = std::process::Command::new(&exe).arg("init").output();

        let mut state = Self::read_state();
        state.running = true;
        state.desired_running = true;
        state.user_path = true;
        Self::write_state(&state)?;

        Ok(())
    }

    pub fn stop() -> Result<(), String> {
        let install_dir = Self::get_install_dir();
        let exe = Self::get_executable_path();

        if exe.exists() {
            let _ = std::process::Command::new(&exe).args(["hook", "uninstall"]).output();
        }

        let _ = remove_from_user_path(&install_dir);

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
            fs::remove_dir_all(&install_dir).map_err(|e| format!("删除 Snip 目录失败: {}", e))?;
        }
        Ok(())
    }

    pub fn launch_trust() -> Result<(), String> {
        launch_visible_powershell("codex", None)
    }
}
