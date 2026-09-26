use std::fs;
use std::path::{Path, PathBuf};
use tracing::info;
use url::Url;

use crate::common::windows::{canonicalize_clean, strip_windows_verbatim_prefix};

pub const LLMTRIM_CONFIG_DIR_NAME: &str = "llmtrim";
pub const LLMTRIM_MARKER_FILE: &str = ".code-manager-managed";
pub const LLMTRIM_MARKER_CONTENT: &str = "code-manager-llmtrim-config-v1\n";
pub const LLMTRIM_TRACKING_DB_NAME: &str = "tracking.db";

pub fn managed_config_dir() -> Result<PathBuf, String> {
    let user_profile = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map_err(|_| "未设置 USERPROFILE 或 HOME 环境变量，无法定位 llmtrim 配置目录".to_string())?;
    Ok(PathBuf::from(user_profile).join(".config").join(LLMTRIM_CONFIG_DIR_NAME))
}

pub fn tracking_db_path(install_dir: &Path) -> Result<PathBuf, String> {
    let clean = canonicalize_clean(install_dir).unwrap_or_else(|_| strip_windows_verbatim_prefix(install_dir));
    let base = clean
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    if !base.eq_ignore_ascii_case("llmtrim") {
        return Err(format!("llmtrim 统计目录必须命名为 llmtrim: {:?}", clean));
    }
    Ok(clean.join(LLMTRIM_TRACKING_DB_NAME))
}

pub fn default_tracking_db_path() -> Result<PathBuf, String> {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        if !xdg.trim().is_empty() {
            return Ok(PathBuf::from(xdg).join("llmtrim").join(LLMTRIM_TRACKING_DB_NAME));
        }
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map_err(|_| "未设置 USERPROFILE 或 HOME，无法定位 llmtrim 默认统计数据库".to_string())?;
    Ok(PathBuf::from(home)
        .join(".local")
        .join("share")
        .join("llmtrim")
        .join(LLMTRIM_TRACKING_DB_NAME))
}

pub fn extract_upstream_host(raw_url: &str) -> Result<String, String> {
    let trimmed = raw_url.trim();
    if trimmed.is_empty() {
        return Err("upstream_base_url 不能为空".to_string());
    }
    let parsed = Url::parse(trimmed).map_err(|e| format!("解析 URL 失败: {}", e))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| "upstream_base_url 中未包含有效主机名".to_string())?;
    let clean = host.trim_matches(['[', ']']).to_lowercase();
    Ok(clean)
}

pub fn format_managed_toml(host: &str, db_path: Option<&Path>) -> String {
    let mut toml = format!("# Managed by code-Manager.\nextra_hosts = [\"{}\"]\n", host);
    if let Some(db) = db_path {
        let db_str = db.to_string_lossy().replace('\\', "/");
        toml.push_str(&format!("db_path = \"{}\"\n", db_str));
    }
    toml
}

pub fn sync_managed_llmtrim_config(upstream_url: &str, install_dir: Option<&Path>) -> Result<(), String> {
    let host = extract_upstream_host(upstream_url)?;
    let config_dir = managed_config_dir()?;
    let _ = fs::create_dir_all(&config_dir);

    let db_path = install_dir.and_then(|dir| tracking_db_path(dir).ok());
    let toml_content = format_managed_toml(&host, db_path.as_deref());

    let config_toml_path = config_dir.join("config.toml");
    let marker_path = config_dir.join(LLMTRIM_MARKER_FILE);

    fs::write(&config_toml_path, toml_content)
        .map_err(|e| format!("写入 llmtrim config.toml 失败: {}", e))?;
    fs::write(&marker_path, LLMTRIM_MARKER_CONTENT)
        .map_err(|e| format!("写入 llmtrim 归属标记失败: {}", e))?;

    info!("已同步 llmtrim 受管配置至 {:?} (extra_hosts = [{}])", config_toml_path, host);
    Ok(())
}

pub fn is_config_managed() -> bool {
    if let Ok(config_dir) = managed_config_dir() {
        let marker_path = config_dir.join(LLMTRIM_MARKER_FILE);
        if let Ok(content) = fs::read_to_string(&marker_path) {
            return content.trim() == LLMTRIM_MARKER_CONTENT.trim();
        }
    }
    false
}

pub fn remove_managed_config_dir() -> Result<(), String> {
    let config_dir = managed_config_dir()?;
    if !config_dir.exists() {
        return Ok(());
    }
    let marker_path = config_dir.join(LLMTRIM_MARKER_FILE);
    if !marker_path.exists() {
        return Err("未找到 llmtrim 配置归属标记，跳过删除未受管配置目录".to_string());
    }
    let content = fs::read_to_string(&marker_path)
        .map_err(|e| format!("读取 llmtrim 归属标记失败: {}", e))?;
    if content.trim() != LLMTRIM_MARKER_CONTENT.trim() {
        return Err("llmtrim 配置目录非 code-Manager 受管，拒绝删除".to_string());
    }
    fs::remove_dir_all(&config_dir)
        .map_err(|e| format!("删除受管 llmtrim 配置目录失败: {}", e))?;
    Ok(())
}

pub fn remove_tracking_database(db_path: &Path) -> Result<bool, String> {
    let base_name = db_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    if !base_name.eq_ignore_ascii_case(LLMTRIM_TRACKING_DB_NAME) {
        return Err(format!("不安全的 llmtrim 统计数据库路径: {:?}", db_path));
    }

    let mut removed = false;
    let path_str = db_path.to_string_lossy().to_string();
    let candidates = [
        db_path.to_path_buf(),
        PathBuf::from(format!("{}-wal", path_str)),
        PathBuf::from(format!("{}-shm", path_str)),
    ];

    for candidate in &candidates {
        if candidate.exists() && candidate.is_file() {
            if let Err(e) = fs::remove_file(candidate) {
                return Err(format!("删除统计数据库文件 {:?} 失败: {}", candidate, e));
            }
            removed = true;
        }
    }
    Ok(removed)
}

pub fn remove_default_tracking_database() -> Result<bool, String> {
    if let Ok(path) = default_tracking_db_path() {
        if path.exists() {
            return remove_tracking_database(&path);
        }
    }
    Ok(false)
}
