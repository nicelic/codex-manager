use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};
use tracing::{info, warn};

use super::service::GortexService;
use crate::common::windows::clean_path_str;

const GORTEX_WATCH_DEBOUNCE_MS: u64 = 100;

pub fn clean_gortex_project_slug(project_path: &str) -> String {
    let clean = clean_path_str(project_path);
    let p = Path::new(&clean);
    let mut base = p
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("project")
        .to_string();

    for sep in &["-0.", "-1.", "-2.", "-v", "@"] {
        if let Some(idx) = base.find(sep) {
            base.truncate(idx);
            break;
        }
    }

    let mut slug = String::new();
    for c in base.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            slug.push(c);
        } else if c == '.' {
            slug.push('-');
        }
    }

    let trimmed = slug.trim_matches(|c| c == '-' || c == '_').to_lowercase();
    if trimmed.is_empty() {
        "project".to_string()
    } else {
        trimmed
    }
}

pub fn ensure_gortex_watch_config(project: &str) -> Result<bool, String> {
    let clean = clean_path_str(project);
    let p = Path::new(&clean);
    if !p.is_dir() {
        return Ok(false);
    }

    let yaml_path = p.join(".gortex.yaml");
    let content = if yaml_path.is_file() {
        fs::read_to_string(&yaml_path).unwrap_or_default()
    } else {
        String::new()
    };

    let mut doc: serde_yaml::Value = if content.trim().is_empty() {
        serde_yaml::Value::Mapping(serde_yaml::Mapping::new())
    } else {
        serde_yaml::from_str(&content).unwrap_or_else(|_| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()))
    };

    let mut changed = false;

    if let serde_yaml::Value::Mapping(ref mut map) = doc {
        // 1. watch mapping
        let watch_key = serde_yaml::Value::String("watch".to_string());
        if !map.contains_key(&watch_key) || !map[&watch_key].is_mapping() {
            map.insert(watch_key.clone(), serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));
            changed = true;
        }
        if let Some(watch_map) = map.get_mut(&watch_key).and_then(|v| v.as_mapping_mut()) {
            let enabled_key = serde_yaml::Value::String("enabled".to_string());
            if watch_map.get(&enabled_key) != Some(&serde_yaml::Value::Bool(true)) {
                watch_map.insert(enabled_key, serde_yaml::Value::Bool(true));
                changed = true;
            }
            let debounce_key = serde_yaml::Value::String("debounce_ms".to_string());
            let expected_debounce = serde_yaml::Value::Number(GORTEX_WATCH_DEBOUNCE_MS.into());
            if watch_map.get(&debounce_key) != Some(&expected_debounce) {
                watch_map.insert(debounce_key, expected_debounce);
                changed = true;
            }
        }

        // 2. search mapping
        let search_key = serde_yaml::Value::String("search".to_string());
        if !map.contains_key(&search_key) || !map[&search_key].is_mapping() {
            map.insert(search_key.clone(), serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));
            changed = true;
        }
        if let Some(search_map) = map.get_mut(&search_key).and_then(|v| v.as_mapping_mut()) {
            let index_prose_key = serde_yaml::Value::String("index_prose".to_string());
            if search_map.get(&index_prose_key) != Some(&serde_yaml::Value::Bool(true)) {
                search_map.insert(index_prose_key, serde_yaml::Value::Bool(true));
                changed = true;
            }
        }

        // 3. workspace scalar
        let ws_key = serde_yaml::Value::String("workspace".to_string());
        let ws_val = serde_yaml::Value::String("default".to_string());
        if map.get(&ws_key) != Some(&ws_val) {
            map.insert(ws_key, ws_val);
            changed = true;
        }
    }

    if changed || !yaml_path.is_file() {
        let serialized = serde_yaml::to_string(&doc).map_err(|e| format!("序列化 .gortex.yaml 失败: {}", e))?;
        fs::write(&yaml_path, serialized).map_err(|e| format!("写入 .gortex.yaml 失败: {}", e))?;
        return Ok(true);
    }

    Ok(false)
}

pub fn ensure_gortex_global_config_workspaces_at_path(
    path: &Path,
    default_workspace: &str,
    ui_projects: &[String],
) -> Result<bool, String> {
    let ws_name = if default_workspace.is_empty() {
        "default"
    } else {
        default_workspace
    };

    let content = if path.is_file() {
        fs::read_to_string(path).unwrap_or_default()
    } else {
        String::new()
    };

    let mut doc: serde_yaml::Value = if content.trim().is_empty() {
        serde_yaml::Value::Mapping(serde_yaml::Mapping::new())
    } else {
        serde_yaml::from_str(&content).unwrap_or_else(|_| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()))
    };

    let mut changed = false;

    let repos_key = serde_yaml::Value::String("repos".to_string());

    let mut ui_map = BTreeMap::new();
    for p in ui_projects {
        let clean = clean_path_str(p);
        if clean.is_empty() {
            continue;
        }
        let norm = clean.to_ascii_lowercase();
        ui_map.insert(norm, clean);
    }

    let mut new_repos = Vec::new();
    let mut seen_in_config = BTreeMap::new();

    if let serde_yaml::Value::Mapping(ref mut root_map) = doc {
        if let Some(existing_repos) = root_map.get(&repos_key).and_then(|v| v.as_sequence()) {
            for item in existing_repos {
                if let Some(item_map) = item.as_mapping() {
                    let raw_path = item_map
                        .get(&serde_yaml::Value::String("path".to_string()))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .trim();
                    let path_val = clean_path_str(raw_path);
                    let norm = path_val.to_ascii_lowercase();

                    if let Some(orig_ui_path) = ui_map.get(&norm) {
                        seen_in_config.insert(norm.clone(), true);
                        let mut updated_item = item_map.clone();

                        let path_k = serde_yaml::Value::String("path".to_string());
                        let path_v = serde_yaml::Value::String(orig_ui_path.clone());
                        if updated_item.get(&path_k) != Some(&path_v) {
                            updated_item.insert(path_k, path_v);
                            changed = true;
                        }

                        let ws_k = serde_yaml::Value::String("workspace".to_string());
                        let ws_v = serde_yaml::Value::String(ws_name.to_string());
                        if updated_item.get(&ws_k) != Some(&ws_v) {
                            updated_item.insert(ws_k, ws_v);
                            changed = true;
                        }

                        let proj_k = serde_yaml::Value::String("project".to_string());
                        let slug = clean_gortex_project_slug(orig_ui_path);
                        let slug_v = serde_yaml::Value::String(slug);
                        if updated_item.get(&proj_k) != Some(&slug_v) {
                            updated_item.insert(proj_k, slug_v);
                            changed = true;
                        }

                        new_repos.push(serde_yaml::Value::Mapping(updated_item));
                    } else {
                        // Project was untracked, exclude it
                        changed = true;
                    }
                }
            }
        }

        // Add newly tracked projects not yet in config
        for (norm, orig_ui_path) in &ui_map {
            if !seen_in_config.contains_key(norm) {
                let mut item_map = serde_yaml::Mapping::new();
                item_map.insert(
                    serde_yaml::Value::String("path".to_string()),
                    serde_yaml::Value::String(orig_ui_path.clone()),
                );
                item_map.insert(
                    serde_yaml::Value::String("workspace".to_string()),
                    serde_yaml::Value::String(ws_name.to_string()),
                );
                let slug = clean_gortex_project_slug(orig_ui_path);
                item_map.insert(
                    serde_yaml::Value::String("project".to_string()),
                    serde_yaml::Value::String(slug),
                );
                new_repos.push(serde_yaml::Value::Mapping(item_map));
                changed = true;
            }
        }

        root_map.insert(repos_key, serde_yaml::Value::Sequence(new_repos));
    }

    if changed || !path.is_file() {
        if let Some(p) = path.parent() {
            let _ = fs::create_dir_all(p);
        }
        let serialized = serde_yaml::to_string(&doc).map_err(|e| format!("序列化 config.yaml 失败: {}", e))?;
        fs::write(path, serialized).map_err(|e| format!("写入 config.yaml 失败: {}", e))?;
        return Ok(true);
    }

    Ok(false)
}

pub fn reload_gortex_daemon_async() {
    tokio::spawn(async {
        if let Some(exe) = GortexService::get_managed_executable_path() {
            let _ = GortexService::run_gortex_command_with_timeout(
                Duration::from_secs(60),
                &exe,
                &["daemon", "reload"],
            ).await;
        }
    });
}

static LAST_CONFIG_MOD: Mutex<Option<(SystemTime, u64)>> = Mutex::new(None);
static LAST_PROJECTS_MOD: Mutex<Option<(SystemTime, u64)>> = Mutex::new(None);

pub fn reconcile_gortex_workspaces(default_workspace: &str) -> Result<(), String> {
    let install_root = GortexService::get_install_dir();
    let config_path = install_root.join("config").join("gortex").join("config.yaml");
    let projects = GortexService::get_tracked_projects();

    let config_changed = ensure_gortex_global_config_workspaces_at_path(
        &config_path,
        default_workspace,
        &projects,
    )?;

    if let Ok(meta) = fs::metadata(&config_path) {
        if let Ok(mtime) = meta.modified() {
            let mut lock = LAST_CONFIG_MOD.lock().unwrap();
            *lock = Some((mtime, meta.len()));
        }
    }

    let registry_path = GortexService::get_project_registry_path();
    if let Ok(meta) = fs::metadata(&registry_path) {
        if let Ok(mtime) = meta.modified() {
            let mut lock = LAST_PROJECTS_MOD.lock().unwrap();
            *lock = Some((mtime, meta.len()));
        }
    }

    let mut watch_changed = false;
    for project in &projects {
        if let Ok(c) = ensure_gortex_watch_config(project) {
            if c {
                watch_changed = true;
            }
        }
    }

    if config_changed || watch_changed {
        reload_gortex_daemon_async();
    }

    Ok(())
}

pub fn start_gortex_watch_enforcer() {
    // 1. Initial reconciliation on startup
    if let Err(e) = reconcile_gortex_workspaces("default") {
        warn!("Gortex 工作区初始自愈失败: {}", e);
    } else {
        info!("Gortex 工作区初始自愈完成。");
    }

    // 2. Track 1: Fast file-stat check (every 2 seconds)
    tokio::spawn(async {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        loop {
            interval.tick().await;
            let install_root = GortexService::get_install_dir();
            let config_path = install_root.join("config").join("gortex").join("config.yaml");
            let registry_path = GortexService::get_project_registry_path();

            let mut need_reconcile = false;

            if let Ok(meta) = fs::metadata(&config_path) {
                if let Ok(mtime) = meta.modified() {
                    let lock = LAST_CONFIG_MOD.lock().unwrap();
                    if let Some((last_mtime, last_size)) = *lock {
                        if mtime != last_mtime || meta.len() != last_size {
                            need_reconcile = true;
                        }
                    } else {
                        need_reconcile = true;
                    }
                }
            }

            if !need_reconcile {
                if let Ok(meta) = fs::metadata(&registry_path) {
                    if let Ok(mtime) = meta.modified() {
                        let lock = LAST_PROJECTS_MOD.lock().unwrap();
                        if let Some((last_mtime, last_size)) = *lock {
                            if mtime != last_mtime || meta.len() != last_size {
                                need_reconcile = true;
                            }
                        } else {
                            need_reconcile = true;
                        }
                    }
                }
            }

            if need_reconcile {
                let _ = reconcile_gortex_workspaces("default");
            }
        }
    });

    // 3. Track 2: Periodic full heartbeat check (every 10 seconds)
    tokio::spawn(async {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        loop {
            interval.tick().await;
            let _ = reconcile_gortex_workspaces("default");
        }
    });
}
