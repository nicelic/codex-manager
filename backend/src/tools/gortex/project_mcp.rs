use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::integrations::{
    gortex_agent_available,
    GORTEX_AGENTS,
};
use super::ownership::{
    gortex_delete_project_mcp_record, gortex_json_artifact_fingerprint,
    gortex_mcp_entry_looks_managed, gortex_opencode_entry_looks_managed,
    gortex_project_mcp_key, gortex_project_mcp_record, read_gortex_ownership,
    write_gortex_ownership,
};
use crate::common::windows::clean_path_str;

pub fn gortex_project_mcp_config_path(agent: &str, project: &str) -> Option<PathBuf> {
    let clean = clean_path_str(project);
    let p = Path::new(&clean);
    match agent {
        "codex" => Some(p.join(".codex").join("config.toml")),
        "claude" => Some(p.join(".mcp.json")),
        "cursor" => Some(p.join(".cursor").join("mcp.json")),
        "copilot" => Some(p.join(".github").join("mcp.json")),
        "opencode" => Some(p.join("opencode.json")),
        // Antigravity IDE does not load per-project `.agents/mcp_config.json`; it uses the global config + gortex-bridge.
        "antigravity" => None,
        "gemini" => Some(p.join(".gemini").join("settings.json")),
        _ => None,
    }
}

pub fn gortex_project_mcp_uses_cwd(agent: &str) -> bool {
    matches!(agent, "codex" | "opencode" | "gemini")
}

pub fn gortex_project_mcp_entry(agent: &str, executable: &str, project: &str, install_root: &Path) -> Value {
    let clean_project = clean_path_str(project);
    let clean_exe = clean_path_str(executable);
    let (bridge_exe, mut bridge_args) =
        crate::tools::gortex::integrations::gortex_bridge_command_args(agent, &clean_exe);
    bridge_args.push("--cwd".to_string());
    bridge_args.push(clean_project.clone());

    if agent == "opencode" {
        let mut cmd_arr = vec![Value::String(bridge_exe)];
        cmd_arr.extend(bridge_args.into_iter().map(Value::String));

        let mut map = serde_json::Map::new();
        map.insert("type".to_string(), Value::String("local".to_string()));
        map.insert("command".to_string(), Value::Array(cmd_arr));
        map.insert("enabled".to_string(), Value::Bool(true));
        map.insert("environment".to_string(), Value::Object(crate::tools::gortex::integrations::gortex_mcp_env(install_root)));
        return Value::Object(map);
    }

    let mut map = serde_json::Map::new();
    map.insert("command".to_string(), Value::String(bridge_exe));
    map.insert(
        "args".to_string(),
        Value::Array(bridge_args.into_iter().map(Value::String).collect()),
    );
    map.insert("cwd".to_string(), Value::String(clean_project.clone()));
    let mut env = crate::tools::gortex::integrations::gortex_mcp_env(install_root);
    env.insert("WORKSPACE".to_string(), Value::String(clean_project));
    map.insert("env".to_string(), Value::Object(env));

    if agent == "copilot" {
        map.insert("type".to_string(), Value::String("local".to_string()));
    }
    if agent == "codex" {
        map.insert("startup_timeout_sec".to_string(), Value::Number(90.into()));
    }

    entry_apply_cwd(agent, map)
}

fn entry_apply_cwd(agent: &str, mut map: serde_json::Map<String, Value>) -> Value {
    if !gortex_project_mcp_uses_cwd(agent) {
        map.remove("cwd");
    }
    Value::Object(map)
}

pub fn update_project_json_mcp_config_owned(
    agent: &str,
    project: &str,
    executable: &str,
    install_root: &Path,
    remove: bool,
) -> Result<bool, String> {
    let project_clean = clean_path_str(project);
    let project = &project_clean;
    let exe_clean = clean_path_str(executable);
    let executable = &exe_clean;

    let path = match gortex_project_mcp_config_path(agent, project) {
        Some(p) => p,
        None => return Ok(false),
    };

    let mut root: Value = if path.is_file() {
        let content = fs::read_to_string(&path).map_err(|e| format!("读取项目 {} 配置失败: {}", agent, e))?;
        serde_json::from_str(&content).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    let mut ownership = read_gortex_ownership(install_root)?;
    let key = gortex_project_mcp_key(agent, project);

    let servers = root.as_object_mut().and_then(|obj| {
        if !obj.contains_key("mcpServers") {
            obj.insert("mcpServers".to_string(), json!({}));
        }
        obj.get_mut("mcpServers")?.as_object_mut()
    });

    let servers = match servers {
        Some(s) => s,
        None => return Err(format!("{} 项目配置中的 mcpServers 不是对象", agent)),
    };

    let exists = servers.contains_key("gortex");
    let existing = servers.get("gortex").cloned().unwrap_or(Value::Null);

    if remove {
        if !exists {
            gortex_delete_project_mcp_record(&mut ownership, agent, project);
            let _ = write_gortex_ownership(install_root, &ownership);
            return Ok(false);
        }
        let record = ownership.project_mcp.get(&key).cloned().unwrap_or_default();
        if (record.fingerprint.is_empty() || record.fingerprint != gortex_json_artifact_fingerprint(&existing))
            && !gortex_mcp_entry_looks_managed(&existing)
        {
            return Err(format!("{} 项目级 gortex MCP 已被用户修改，已保留", agent));
        }
        servers.remove("gortex");
        gortex_delete_project_mcp_record(&mut ownership, agent, project);
    } else {
        let entry = gortex_project_mcp_entry(agent, executable, project, install_root);
        servers.insert("gortex".to_string(), entry.clone());
        gortex_project_mcp_record(&mut ownership, agent, project, &path.to_string_lossy(), &entry);
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
    fs::write(&path, format!("{}\n", data)).map_err(|e| format!("写入项目 {} 失败: {}", path.display(), e))?;
    write_gortex_ownership(install_root, &ownership)?;

    Ok(true)
}

pub fn update_project_opencode_mcp_config_owned(
    project: &str,
    executable: &str,
    install_root: &Path,
    remove: bool,
) -> Result<bool, String> {
    let project_clean = clean_path_str(project);
    let project = &project_clean;
    let exe_clean = clean_path_str(executable);
    let executable = &exe_clean;

    let path = match gortex_project_mcp_config_path("opencode", project) {
        Some(p) => p,
        None => return Ok(false),
    };

    let mut root: Value = if path.is_file() {
        let content = fs::read_to_string(&path).map_err(|e| format!("读取 OpenCode 项目配置失败: {}", e))?;
        serde_json::from_str(&content).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    let mut ownership = read_gortex_ownership(install_root)?;
    let key = gortex_project_mcp_key("opencode", project);

    let servers = root.as_object_mut().and_then(|obj| {
        if !obj.contains_key("mcp") {
            obj.insert("mcp".to_string(), json!({}));
        }
        obj.get_mut("mcp")?.as_object_mut()
    });

    let servers = match servers {
        Some(s) => s,
        None => return Err("OpenCode 项目配置中的 mcp 不是对象".to_string()),
    };

    let exists = servers.contains_key("gortex");
    let existing = servers.get("gortex").cloned().unwrap_or(Value::Null);

    if remove {
        if !exists {
            gortex_delete_project_mcp_record(&mut ownership, "opencode", project);
            let _ = write_gortex_ownership(install_root, &ownership);
            return Ok(false);
        }
        let record = ownership.project_mcp.get(&key).cloned().unwrap_or_default();
        if (record.fingerprint.is_empty() || record.fingerprint != gortex_json_artifact_fingerprint(&existing))
            && !gortex_opencode_entry_looks_managed(&existing)
        {
            return Err("OpenCode 项目级 gortex MCP 已被用户修改，已保留".to_string());
        }
        servers.remove("gortex");
        gortex_delete_project_mcp_record(&mut ownership, "opencode", project);
    } else {
        let mut entry = gortex_project_mcp_entry("opencode", executable, project, install_root);
        if let Some(obj) = entry.as_object_mut() {
            obj.insert("cwd".to_string(), Value::String(project.to_string()));
        }
        servers.insert("gortex".to_string(), entry.clone());
        gortex_project_mcp_record(&mut ownership, "opencode", project, &path.to_string_lossy(), &entry);
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
    fs::write(&path, format!("{}\n", data)).map_err(|e| format!("写入 OpenCode 项目配置失败: {}", e))?;
    write_gortex_ownership(install_root, &ownership)?;

    Ok(true)
}

pub fn update_project_codex_mcp_config_owned(
    project: &str,
    executable: &str,
    install_root: &Path,
    remove: bool,
) -> Result<bool, String> {
    let project_clean = clean_path_str(project);
    let project = &project_clean;
    let exe_clean = clean_path_str(executable);
    let executable = &exe_clean;

    let path = match gortex_project_mcp_config_path("codex", project) {
        Some(p) => p,
        None => return Ok(false),
    };

    let content = if path.is_file() {
        fs::read_to_string(&path).map_err(|e| format!("读取 Codex 项目 config.toml 失败: {}", e))?
    } else {
        String::new()
    };

    let mut ownership = read_gortex_ownership(install_root)?;
    let _key = gortex_project_mcp_key("codex", project);

    let entry = gortex_project_mcp_entry("codex", executable, project, install_root);

    if remove {
        if !content.contains("[mcp_servers.gortex") {
            gortex_delete_project_mcp_record(&mut ownership, "codex", project);
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
        fs::write(&path, new_content).map_err(|e| format!("写入 Codex 项目 config.toml 失败: {}", e))?;
        gortex_delete_project_mcp_record(&mut ownership, "codex", project);
    } else {
        let (bridge_exe, mut bridge_args) =
            crate::tools::gortex::integrations::gortex_bridge_command_args("codex", executable);
        bridge_args.push("--cwd".to_string());
        bridge_args.push(project.to_string());
        let bridge_esc = bridge_exe.replace('\\', "\\\\");
        let args_toml = bridge_args
            .iter()
            .map(|a| format!("\"{}\"", a.replace('\\', "\\\\").replace('"', "\\\"")))
            .collect::<Vec<_>>()
            .join(", ");
        let proj_esc = project.replace('\\', "\\\\");
        let toml_block = format!(
            "\r\n[mcp_servers.gortex]\r\ncommand = \"{}\"\r\nargs = [{}]\r\ncwd = \"{}\"\r\nstartup_timeout_sec = 90\r\n[mcp_servers.gortex.env]\r\nXDG_CONFIG_HOME = \"{}\"\r\nXDG_DATA_HOME = \"{}\"\r\nXDG_CACHE_HOME = \"{}\"\r\nGORTEX_DAEMON_SOCKET = \"{}\"\r\nGORTEX_DAEMON_PIDFILE = \"{}\"\r\nGORTEX_DAEMON_LOGFILE = \"{}\"\r\nGORTEX_DAEMON_STATEFILE = \"{}\"\r\nGORTEX_INDEX_WORKERS = \"8\"\r\nGORTEX_RECONCILE_INTERVAL = \"1h\"\r\nGORTEX_DAEMON_IDLE_TIMEOUT = \"0\"\r\n",
            bridge_esc,
            args_toml,
            proj_esc,
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
        let mut new_content = lines.join("\r\n");
        new_content.push_str(&toml_block);

        if let Some(p) = path.parent() {
            let _ = fs::create_dir_all(p);
        }
        fs::write(&path, new_content).map_err(|e| format!("写入 Codex 项目 config.toml 失败: {}", e))?;
        gortex_project_mcp_record(&mut ownership, "codex", project, &path.to_string_lossy(), &entry);
    }

    write_gortex_ownership(install_root, &ownership)?;
    Ok(true)
}

pub fn cleanup_legacy_project_antigravity_mcp(
    project: &str,
    install_root: &Path,
) -> Result<bool, String> {
    let project_clean = clean_path_str(project);
    if project_clean.is_empty() {
        return Ok(false);
    }

    let legacy_path = Path::new(&project_clean).join(".agents").join("mcp_config.json");
    let mut ownership = read_gortex_ownership(install_root)?;
    let key = gortex_project_mcp_key("antigravity", &project_clean);
    let owned_record = ownership.project_mcp.get(&key).cloned();

    if !legacy_path.is_file() {
        if owned_record.is_some() {
            gortex_delete_project_mcp_record(&mut ownership, "antigravity", &project_clean);
            let _ = write_gortex_ownership(install_root, &ownership);
        }
        return Ok(false);
    }

    let content = fs::read_to_string(&legacy_path)
        .map_err(|e| format!("读取旧版 Antigravity 项目 MCP 配置失败: {}", e))?;
    let mut root: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => {
            gortex_delete_project_mcp_record(&mut ownership, "antigravity", &project_clean);
            let _ = write_gortex_ownership(install_root, &ownership);
            return Ok(false);
        }
    };

    let existing = root
        .get("mcpServers")
        .and_then(|s| s.get("gortex"))
        .cloned()
        .unwrap_or(Value::Null);
    if existing.is_null() {
        if owned_record.is_some() {
            gortex_delete_project_mcp_record(&mut ownership, "antigravity", &project_clean);
            let _ = write_gortex_ownership(install_root, &ownership);
        }
        return Ok(false);
    }

    let is_managed = owned_record
        .as_ref()
        .map(|rec| {
            !rec.fingerprint.is_empty()
                && rec.fingerprint == gortex_json_artifact_fingerprint(&existing)
        })
        .unwrap_or(false)
        || gortex_mcp_entry_looks_managed(&existing);
    if !is_managed {
        return Ok(false);
    }

    if let Some(servers) = root.get_mut("mcpServers").and_then(|s| s.as_object_mut()) {
        servers.remove("gortex");
        if servers.is_empty() {
            if let Some(obj) = root.as_object_mut() {
                obj.remove("mcpServers");
            }
        }
    }

    if root.as_object().map(|o| o.is_empty()).unwrap_or(false) {
        let _ = fs::remove_file(&legacy_path);
        if let Some(parent) = legacy_path.parent() {
            if parent.file_name().and_then(|s| s.to_str()) == Some(".agents") {
                let _ = fs::remove_dir(parent);
            }
        }
    } else if let Ok(data) = serde_json::to_string_pretty(&root) {
        let _ = fs::write(&legacy_path, format!("{}\n", data));
    }

    gortex_delete_project_mcp_record(&mut ownership, "antigravity", &project_clean);
    let _ = write_gortex_ownership(install_root, &ownership);
    Ok(true)
}

pub fn gortex_register_project_mcp_for_single_agent(
    project: &str,
    agent: &str,
    executable: &str,
    install_root: &Path,
    available: Option<&BTreeMap<String, bool>>,
) -> Vec<String> {
    let mut warnings = Vec::new();
    let p = Path::new(project);
    if !p.is_dir() {
        return vec![format!("项目级 MCP: 项目目录不存在，已跳过 {}", project)];
    }

    if agent == "antigravity" {
        if let Err(e) = cleanup_legacy_project_antigravity_mcp(project, install_root) {
            warnings.push(format!("antigravity 清理旧项目 MCP: {}", e));
        }
        return warnings;
    }

    let is_avail = available
        .and_then(|a| a.get(agent).copied())
        .unwrap_or_else(|| gortex_agent_available(agent));
    if !is_avail {
        return warnings;
    }

    let res = match agent {
        "codex" => update_project_codex_mcp_config_owned(project, executable, install_root, false),
        "opencode" => update_project_opencode_mcp_config_owned(project, executable, install_root, false),
        _ => update_project_json_mcp_config_owned(agent, project, executable, install_root, false),
    };
    if let Err(e) = res {
        warnings.push(format!("{} 项目级 MCP: {}", agent, e));
    }
    warnings
}

pub fn gortex_remove_project_mcp_for_single_agent(
    executable: &str,
    project: &str,
    agent: &str,
    install_root: &Path,
) -> Vec<String> {
    let mut warnings = Vec::new();
    if agent == "antigravity" {
        if let Err(e) = cleanup_legacy_project_antigravity_mcp(project, install_root) {
            warnings.push(format!("antigravity 项目级 MCP: {}", e));
        }
        return warnings;
    }
    let res = match agent {
        "codex" => update_project_codex_mcp_config_owned(project, executable, install_root, true),
        "opencode" => update_project_opencode_mcp_config_owned(project, executable, install_root, true),
        _ => update_project_json_mcp_config_owned(agent, project, executable, install_root, true),
    };
    if let Err(e) = res {
        warnings.push(format!("{} 项目级 MCP: {}", agent, e));
    }
    warnings
}

pub fn gortex_register_project_mcp_for_project(
    project: &str,
    executable: &str,
    install_root: &Path,
    available: Option<&BTreeMap<String, bool>>,
) -> Vec<String> {
    let mut warnings = Vec::new();
    let p = Path::new(project);
    if !p.is_dir() {
        return vec![format!("项目级 MCP: 项目目录不存在，已跳过 {}", project)];
    }

    for agent in GORTEX_AGENTS {
        warnings.extend(gortex_register_project_mcp_for_single_agent(
            project,
            agent,
            executable,
            install_root,
            available,
        ));
    }
    warnings
}

pub fn gortex_remove_project_mcp(
    executable: &str,
    project: &str,
    install_root: &Path,
) -> Vec<String> {
    let mut warnings = Vec::new();
    for agent in GORTEX_AGENTS {
        warnings.extend(gortex_remove_project_mcp_for_single_agent(
            executable,
            project,
            agent,
            install_root,
        ));
    }
    warnings
}

pub fn gortex_project_mcp_projects_from_ownership(ownership: &super::types::GortexMCPOwnership) -> Vec<String> {
    let mut projects = Vec::new();
    for record in ownership.project_mcp.values() {
        let clean = clean_path_str(&record.project);
        if !clean.is_empty() {
            projects.push(clean);
        }
    }
    projects.sort();
    projects.dedup();
    projects
}
