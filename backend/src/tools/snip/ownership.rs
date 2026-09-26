use hex;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use crate::common::windows::{canonicalize_clean, clean_path_str, strip_windows_verbatim_prefix};

use super::hook_config::{
    remove_snip_owned_hook, replace_utf8_file,
    snip_agent_directories, snip_agent_hook_file, snip_hook_command_matches_executable,
    snip_hook_command_targets_agent, snip_hook_event_for_agent,
    snip_hook_locations_at, snip_hook_locations_from_config,
    snip_owned_hook_presence_at, SnipOwnedHookPresence,
};
use super::types::{
    ManagedToolState, SnipAgentSnapshot, SnipAgentState, SnipHookOwnership,
    SnipOwnershipLedger, SNIP_ARTIFACT_CLAUDE_HOOK, SNIP_ARTIFACT_CODEX_HOOK,
    SNIP_ARTIFACT_COPILOT_HOOK, SNIP_ARTIFACT_CURSOR_HOOK,
    SNIP_OWNERSHIP_METADATA_KEY, SNIP_OWNERSHIP_VERSION,
};

pub fn read_snip_ownership_ledger(state: &ManagedToolState) -> Result<SnipOwnershipLedger, String> {
    if let Some(val) = state.metadata.get(SNIP_OWNERSHIP_METADATA_KEY) {
        if !val.trim().is_empty() {
            let ledger: SnipOwnershipLedger = serde_json::from_str(val)
                .map_err(|e| format!("解析 snip 精确归属账本失败: {}", e))?;
            if ledger.version != SNIP_OWNERSHIP_VERSION {
                return Err(format!("不支持的 snip 精确归属账本版本: {}", ledger.version));
            }
            for (name, artifact) in &ledger.artifacts {
                validate_snip_hook_ownership(name, artifact)?;
            }
            validate_snip_ownership_targets(&ledger)?;
            return Ok(ledger);
        }
    }
    Ok(SnipOwnershipLedger::default())
}

pub fn write_snip_ownership_ledger(
    state: &mut ManagedToolState,
    ledger: &SnipOwnershipLedger,
) -> Result<(), String> {
    if ledger.version != SNIP_OWNERSHIP_VERSION {
        return Err(format!("不支持的 snip 精确归属账本版本: {}", ledger.version));
    }
    for (name, artifact) in &ledger.artifacts {
        validate_snip_hook_ownership(name, artifact)?;
    }
    validate_snip_ownership_targets(ledger)?;

    let encoded = serde_json::to_string(ledger)
        .map_err(|e| format!("序列化 snip 精确归属账本失败: {}", e))?;
    state.metadata.insert(SNIP_OWNERSHIP_METADATA_KEY.to_string(), encoded);
    state.owned_agents = snip_owned_agents_from_ledger(ledger);
    Ok(())
}

pub fn clear_snip_ownership_ledger(state: &mut ManagedToolState) {
    state.owned_agents.clear();
    state.metadata.remove(SNIP_OWNERSHIP_METADATA_KEY);
}

pub fn snip_artifact_for_agent(agent: &str) -> &'static str {
    match agent {
        "codex" => SNIP_ARTIFACT_CODEX_HOOK,
        "claude-code" => SNIP_ARTIFACT_CLAUDE_HOOK,
        "cursor" => SNIP_ARTIFACT_CURSOR_HOOK,
        "copilot" => SNIP_ARTIFACT_COPILOT_HOOK,
        _ => "",
    }
}

pub fn snip_agent_for_artifact(name: &str) -> &'static str {
    for agent in &["codex", "claude-code", "cursor", "copilot"] {
        let base = snip_artifact_for_agent(agent);
        if name == base || name.starts_with(&format!("{}:", base)) {
            return agent;
        }
    }
    ""
}

pub fn canonical_snip_target_path(path: &Path) -> String {
    let canonical = canonicalize_clean(path).unwrap_or_else(|_| strip_windows_verbatim_prefix(path));
    clean_path_str(&canonical.to_string_lossy())
}

pub fn snip_artifact_key_for_ownership(agent: &str, target_path: &str) -> String {
    let base = snip_artifact_for_agent(agent);
    if base.is_empty() {
        return String::new();
    }
    let canonical = canonical_snip_target_path(Path::new(target_path)).to_ascii_lowercase();
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    format!("{}:{}", base, hex::encode(hasher.finalize()))
}

pub fn validate_snip_hook_ownership(name: &str, artifact: &SnipHookOwnership) -> Result<(), String> {
    let want_agent = snip_agent_for_artifact(name);
    if want_agent.is_empty() {
        return Err(format!("snip 精确归属账本包含未知项目: {}", name));
    }
    if artifact.agent != want_agent {
        return Err(format!("snip 精确归属项目的 Agent 不匹配: {}", name));
    }
    if artifact.target_path.trim().is_empty()
        || artifact.fingerprint.trim().is_empty()
        || artifact.command.trim().is_empty()
    {
        return Err(format!("snip 精确归属项目不完整: {}", name));
    }

    let (want_event, grouped) = match snip_hook_event_for_agent(&artifact.agent) {
        Some(p) => p,
        None => return Err(format!("未知 Snip Agent: {}", artifact.agent)),
    };

    if artifact.event != want_event
        || (grouped && artifact.group_index < 0)
        || (!grouped && artifact.group_index != -1)
    {
        return Err(format!("snip 精确归属项目位置无效: {}", name));
    }

    if grouped && artifact.group_fingerprint.as_deref().unwrap_or("").trim().is_empty() {
        return Err(format!("snip 精确归属项目缺少 Hook 组指纹: {}", name));
    }
    if !grouped && artifact.group_fingerprint.is_some() {
        return Err(format!("snip 精确归属项目包含无效 Hook 组指纹: {}", name));
    }

    if !snip_hook_command_targets_agent(&artifact.command, &artifact.agent) {
        return Err(format!("snip 精确归属项目命令无效: {}", name));
    }

    let expected_key = snip_artifact_key_for_ownership(&artifact.agent, &artifact.target_path);
    let base_key = snip_artifact_for_agent(&artifact.agent);
    if name != base_key && name != expected_key {
        return Err(format!("snip 精确归属项目键与目标文件不匹配: {}", name));
    }

    Ok(())
}

pub fn validate_snip_ownership_targets(ledger: &SnipOwnershipLedger) -> Result<(), String> {
    let mut seen = HashMap::new();
    for (name, artifact) in &ledger.artifacts {
        let key = format!(
            "{}\x00{}",
            artifact.agent,
            canonical_snip_target_path(Path::new(&artifact.target_path)).to_ascii_lowercase()
        );
        if let Some(previous) = seen.get(&key) {
            return Err(format!(
                "snip 精确归属账本为同一目标文件记录了多个 Hook: {}、{}",
                previous, name
            ));
        }
        seen.insert(key, name.clone());
    }
    Ok(())
}

pub fn snip_ownership_artifact_for_target(
    ledger: &SnipOwnershipLedger,
    agent: &str,
    target_path: &Path,
) -> Option<(String, SnipHookOwnership)> {
    let canonical_target = canonical_snip_target_path(target_path);
    for (name, artifact) in &ledger.artifacts {
        if artifact.agent == agent
            && canonical_snip_target_path(Path::new(&artifact.target_path))
                .eq_ignore_ascii_case(&canonical_target)
        {
            return Some((name.clone(), artifact.clone()));
        }
    }
    None
}

pub fn snip_owned_agents_from_ledger(ledger: &SnipOwnershipLedger) -> Vec<String> {
    let mut set = HashSet::new();
    for artifact in ledger.artifacts.values() {
        if !artifact.agent.is_empty() {
            set.insert(artifact.agent.clone());
        }
    }
    let mut result: Vec<String> = set.into_iter().collect();
    result.sort();
    result
}

pub fn snip_owned_agents_needing_repair(
    ledger: &SnipOwnershipLedger,
    executable: &Path,
) -> Result<Vec<String>, String> {
    let mut seen = HashSet::new();
    let exe_str = executable.to_string_lossy().to_string();

    for artifact in ledger.artifacts.values() {
        if !snip_hook_command_matches_executable(&artifact.command, &artifact.agent, &exe_str) {
            seen.insert(artifact.agent.clone());
            continue;
        }
        let presence = snip_owned_hook_presence_at(artifact)?;
        if presence != SnipOwnedHookPresence::Exact {
            seen.insert(artifact.agent.clone());
        }
    }
    let mut result: Vec<String> = seen.into_iter().collect();
    result.sort();
    Ok(result)
}

pub fn snip_activation_artifacts_present(
    state: &ManagedToolState,
    ledger: &SnipOwnershipLedger,
) -> Result<bool, String> {
    if state.user_path || state.system_path {
        return Ok(true);
    }
    for artifact in ledger.artifacts.values() {
        let presence = snip_owned_hook_presence_at(artifact)?;
        if presence != SnipOwnedHookPresence::Absent {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn snip_ownership_needs_cleanup(
    state: &ManagedToolState,
    ledger: &SnipOwnershipLedger,
) -> bool {
    state.user_path || state.system_path || !ledger.artifacts.is_empty()
}

pub fn remove_snip_owned_artifacts(ledger: &SnipOwnershipLedger) -> Result<(), String> {
    let mut items: Vec<(&String, &SnipHookOwnership)> = ledger.artifacts.iter().collect();
    items.sort_by(|a, b| {
        if a.1.agent != b.1.agent {
            a.1.agent.cmp(&b.1.agent)
        } else {
            let left = canonical_snip_target_path(Path::new(&a.1.target_path)).to_ascii_lowercase();
            let right = canonical_snip_target_path(Path::new(&b.1.target_path)).to_ascii_lowercase();
            if left != right {
                left.cmp(&right)
            } else {
                a.0.cmp(b.0)
            }
        }
    });

    for (_, artifact) in items {
        remove_snip_owned_hook(artifact)?;
    }
    Ok(())
}

pub fn recover_legacy_snip_ownership(
    state: &ManagedToolState,
    executable: &Path,
) -> Result<SnipOwnershipLedger, String> {
    let mut ledger = read_snip_ownership_ledger(state)?;
    let exe_str = executable.to_string_lossy().to_string();
    if state.owned_agents.is_empty() || exe_str.trim().is_empty() {
        return Ok(ledger);
    }

    let directories = snip_agent_directories();
    for agent in &state.owned_agents {
        if ledger.artifacts.values().any(|a| a.agent == *agent) {
            continue;
        }
        let directory = match directories.get(agent.as_str()) {
            Some(d) => d,
            None => continue,
        };
        let hook_file = snip_agent_hook_file(agent, directory);
        let (locations, exists) = snip_hook_locations_at(agent, &hook_file, |command| {
            snip_hook_command_matches_executable(command, agent, &exe_str)
        })?;

        if exists && locations.len() == 1 {
            let loc = &locations[0];
            let artifact = SnipHookOwnership {
                agent: loc.agent.clone(),
                target_path: canonical_snip_target_path(Path::new(&loc.target_path)),
                event: loc.event.clone(),
                group_index: loc.group_index,
                handler_index: loc.handler_index,
                group_fingerprint: loc.group_fingerprint.clone(),
                fingerprint: loc.fingerprint.clone(),
                command: loc.command.trim().to_string(),
            };
            let key = snip_artifact_key_for_ownership(agent, &artifact.target_path);
            ledger.artifacts.insert(key, artifact);
        }
    }
    Ok(ledger)
}

pub fn snapshot_snip_agent_files(agents: &[SnipAgentState]) -> Result<HashMap<String, SnipAgentSnapshot>, String> {
    let mut result = HashMap::new();
    for agent in agents {
        let target = Path::new(&agent.target_file);
        let mut snapshot = SnipAgentSnapshot {
            path: target.to_path_buf(),
            exists: false,
            data: Vec::new(),
            created_directories: Vec::new(),
        };

        if target.exists() {
            let data = fs::read(target).map_err(|e| format!("读取 {} Hook 快照失败: {}", agent.name, e))?;
            snapshot.exists = true;
            snapshot.data = data;
        } else {
            let mut curr = target.parent();
            let mut missing_dirs = Vec::new();
            while let Some(dir) = curr {
                if dir.exists() {
                    break;
                }
                missing_dirs.push(dir.to_path_buf());
                curr = dir.parent();
            }
            snapshot.created_directories = missing_dirs;
        }
        result.insert(agent.name.clone(), snapshot);
    }
    Ok(result)
}

pub fn restore_snip_agent_snapshots(before: &HashMap<String, SnipAgentSnapshot>) -> Result<(), String> {
    for snapshot in before.values() {
        if !snapshot.exists {
            if snapshot.path.exists() {
                let _ = fs::remove_file(&snapshot.path);
            }
            for dir in &snapshot.created_directories {
                let _ = fs::remove_dir(dir);
            }
            continue;
        }
        let current = fs::read(&snapshot.path).unwrap_or_default();
        if current != snapshot.data {
            replace_utf8_file(&snapshot.path, &snapshot.data)?;
        }
    }
    Ok(())
}

pub fn snip_hook_ownership_added_by_init(
    agent: &str,
    before: &SnipAgentSnapshot,
    executable: &Path,
) -> Result<SnipHookOwnership, String> {
    let exe_str = executable.to_string_lossy().to_string();
    let (after_locations, exists) = snip_hook_locations_at(agent, &before.path, |command| {
        snip_hook_command_matches_executable(command, agent, &exe_str)
    })?;

    if !exists {
        return Err("初始化后目标 Hook 文件不存在".to_string());
    }

    let mut previous_locations = Vec::new();
    if before.exists {
        let val: Value = serde_json::from_slice(&before.data)
            .map_err(|e| format!("读取初始化前 Hook 快照失败: {}", e))?;
        previous_locations = snip_hook_locations_from_config(agent, &before.path, &val, |command| {
            snip_hook_command_matches_executable(command, agent, &exe_str)
        })?;
    }

    let mut known_counts: HashMap<String, usize> = HashMap::new();
    for loc in &previous_locations {
        *known_counts.entry(loc.fingerprint.clone()).or_insert(0) += 1;
    }

    let mut added = Vec::new();
    for loc in after_locations {
        if let Some(count) = known_counts.get_mut(&loc.fingerprint) {
            if *count > 0 {
                *count -= 1;
                continue;
            }
        }
        added.push(loc);
    }

    if added.len() != 1 {
        return Err(format!(
            "官方初始化后无法唯一确认新增的 Snip {} Hook（检测到 {} 条）",
            agent,
            added.len()
        ));
    }

    let loc = &added[0];
    Ok(SnipHookOwnership {
        agent: loc.agent.clone(),
        target_path: canonical_snip_target_path(Path::new(&loc.target_path)),
        event: loc.event.clone(),
        group_index: loc.group_index,
        handler_index: loc.handler_index,
        group_fingerprint: loc.group_fingerprint.clone(),
        fingerprint: loc.fingerprint.clone(),
        command: loc.command.trim().to_string(),
    })
}
