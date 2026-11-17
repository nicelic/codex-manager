use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Once};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::service::GortexService;
use crate::common::windows::{
    canonicalize_clean, canonicalize_or_clean, clean_path_str, get_user_profile_dir,
    strip_windows_verbatim_prefix,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeCliArgs {
    pub gortex_exe: String,
    pub agent: String,
    pub cwd: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterceptKind {
    WorkspaceRepos,
    WorkspaceInfo,
    ActiveProject,
    ResourceRepos,
    ResourceWorkspace,
    ResourceActiveProject,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct InventoryRepo {
    pub name: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct TrackedInventory {
    pub repos: Vec<InventoryRepo>,
    pub projects: BTreeMap<String, Vec<String>>,
    pub active_project: String,
    pub workspaces: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct BridgeRuntimePaths {
    pub install_root: PathBuf,
    pub socket_path: PathBuf,
    pub config_yaml_path: PathBuf,
    pub env_vars: Vec<(String, String)>,
}

#[derive(Debug, Deserialize)]
struct DaemonHandshakeAck {
    ok: bool,
    #[serde(default)]
    error_code: String,
    #[serde(default)]
    error_msg: String,
    #[serde(default)]
    session_id: String,
}

#[derive(Debug, Deserialize)]
struct DaemonControlResponse {
    ok: bool,
    #[serde(default)]
    error_code: String,
    #[serde(default)]
    error_msg: String,
    #[serde(default)]
    result: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
pub struct DaemonProbeRepo {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub workspace: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub state: String,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
pub struct DaemonProbeResult {
    #[serde(default)]
    pub tracked_repos: Vec<DaemonProbeRepo>,
}

#[derive(Debug)]
enum RelayEvent {
    FromClient(String),
    ClientEof,
    FromDaemon(u64, String),
    DaemonClosed(u64),
}

pub fn parse_bridge_args(args: &[String]) -> BridgeCliArgs {
    let mut parsed = BridgeCliArgs {
        gortex_exe: String::new(),
        agent: String::new(),
        cwd: String::new(),
    };
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--gortex" && i + 1 < args.len() {
            parsed.gortex_exe = args[i + 1].clone();
            i += 1;
        } else if args[i] == "--agent" && i + 1 < args.len() {
            parsed.agent = args[i + 1].clone();
            i += 1;
        } else if args[i] == "--cwd" && i + 1 < args.len() {
            parsed.cwd = args[i + 1].clone();
            i += 1;
        }
        i += 1;
    }
    parsed
}

fn normalize_cmp_path(p: &Path) -> String {
    let clean = clean_path_str(&canonicalize_or_clean(p).to_string_lossy());
    if cfg!(target_os = "windows") {
        clean.to_ascii_lowercase()
    } else {
        clean
    }
}

pub fn is_host_program_directory(p: &Path) -> bool {
    let normalized = clean_path_str(&p.to_string_lossy())
        .replace('/', "\\")
        .to_ascii_lowercase();
    let host_markers = [
        r"\programs\antigravity",
        r"\microsoft vs code",
        r"\appdata\local\programs\",
        r"\.antigravity",
        r"\.codex",
        r"\.cursor",
        r"\.gemini",
        r"\.claude",
        r"\.opencode",
        r"\.vscode",
        r"\windows\system32",
    ];
    if host_markers
        .iter()
        .any(|marker| normalized.contains(marker) || normalized.ends_with(marker.trim_end_matches('\\')))
    {
        return true;
    }
    normalized == r"\usr\bin" || normalized == r"\bin" || normalized == r"c:\windows"
}

pub fn is_ambiguous_or_host_directory(p: &Path) -> bool {
    let s = p.to_string_lossy().trim().to_string();
    if s.is_empty() || !p.is_dir() || p.parent().is_none() || is_host_program_directory(p) {
        return true;
    }

    let candidate_norm = normalize_cmp_path(p);
    let profile_norm = normalize_cmp_path(&get_user_profile_dir());
    if !profile_norm.is_empty() && candidate_norm == profile_norm {
        return true;
    }
    if let Ok(home) = std::env::var("HOME") {
        let home_path = PathBuf::from(home.trim());
        if !home.trim().is_empty() && normalize_cmp_path(&home_path) == candidate_norm {
            return true;
        }
    }
    false
}

pub fn path_is_same_or_inside(child: &Path, parent: &Path) -> bool {
    let c = normalize_cmp_path(child);
    let p = normalize_cmp_path(parent);
    if c.is_empty() || p.is_empty() {
        return false;
    }
    if c == p {
        return true;
    }
    let sep = if cfg!(target_os = "windows") { '\\' } else { '/' };
    let trimmed_p = p.trim_end_matches(['\\', '/']);
    let prefix = format!("{}{}", trimmed_p, sep);
    c.starts_with(&prefix)
}

pub fn is_cwd_covered_or_contained(cwd: &Path, tracked_paths: &[String]) -> bool {
    tracked_paths.iter().any(|raw| {
        let clean = clean_path_str(raw);
        if clean.is_empty() {
            return false;
        }
        let repo_path = Path::new(&clean);
        path_is_same_or_inside(cwd, repo_path) || path_is_same_or_inside(repo_path, cwd)
    })
}

pub fn collect_candidate_cwd(explicit_cwd: &str) -> Option<PathBuf> {
    let clean_valid_dir = |raw: &str| -> Option<PathBuf> {
        let cleaned = clean_path_str(raw);
        if cleaned.is_empty() {
            return None;
        }
        let p = PathBuf::from(&cleaned);
        if is_ambiguous_or_host_directory(&p) {
            return None;
        }
        Some(canonicalize_clean(&p).unwrap_or_else(|_| strip_windows_verbatim_prefix(&p)))
    };

    if let Some(found) = clean_valid_dir(explicit_cwd) {
        return Some(found);
    }

    for key in [
        "ANTIGRAVITY_WORKSPACE",
        "WORKSPACE_FOLDER",
        "WORKSPACE_DIR",
        "PROJECT_DIR",
        "WORKSPACE",
        "CODEX_PROJECT_ROOT",
    ] {
        if let Ok(val) = std::env::var(key) {
            if let Some(found) = clean_valid_dir(&val) {
                return Some(found);
            }
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        let raw = cwd.to_string_lossy().to_string();
        if let Some(found) = clean_valid_dir(&raw) {
            return Some(found);
        }
    }

    None
}

fn derive_repo_display_name(name: &str, prefix: &str, path: &str) -> String {
    if !prefix.trim().is_empty() {
        return prefix.trim().to_string();
    }
    if !name.trim().is_empty() {
        return name.trim().to_string();
    }
    Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "repo".to_string())
}

pub fn load_config_yaml_inventory(config_yaml_path: &Path) -> TrackedInventory {
    let mut inv = TrackedInventory::default();
    let content = match fs::read_to_string(config_yaml_path) {
        Ok(c) => c,
        Err(_) => return inv,
    };
    let doc: serde_yaml::Value = match serde_yaml::from_str(&content) {
        Ok(d) => d,
        Err(_) => return inv,
    };
    let root = match doc.as_mapping() {
        Some(m) => m,
        None => return inv,
    };

    let get_str = |m: &serde_yaml::Mapping, key: &str| -> String {
        m.get(serde_yaml::Value::String(key.to_string()))
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    };

    inv.active_project = get_str(root, "active_project");
    let mut workspaces = BTreeSet::new();
    let mut seen_paths = HashSet::new();

    if let Some(repos_seq) = root
        .get(serde_yaml::Value::String("repos".to_string()))
        .and_then(|v| v.as_sequence())
    {
        for item in repos_seq {
            if let Some(m) = item.as_mapping() {
                let raw_path = get_str(m, "path");
                let clean_path = clean_path_str(&raw_path);
                if clean_path.is_empty() {
                    continue;
                }
                let norm = clean_path.to_ascii_lowercase();
                let raw_name = get_str(m, "name");
                let ws = get_str(m, "workspace");
                let proj = get_str(m, "project");
                if !ws.is_empty() {
                    workspaces.insert(ws.clone());
                }
                if seen_paths.insert(norm) {
                    inv.repos.push(InventoryRepo {
                        name: derive_repo_display_name(&raw_name, "", &clean_path),
                        path: clean_path,
                        workspace: (!ws.is_empty()).then_some(ws),
                        project: (!proj.is_empty()).then_some(proj),
                        state: None,
                    });
                }
            }
        }
    }

    if let Some(projs_map) = root
        .get(serde_yaml::Value::String("projects".to_string()))
        .and_then(|v| v.as_mapping())
    {
        for (k, v) in projs_map {
            let proj_name = k.as_str().unwrap_or("").trim().to_string();
            if proj_name.is_empty() {
                continue;
            }
            let mut members = Vec::new();
            if let Some(proj_obj) = v.as_mapping() {
                if let Some(repos_seq) = proj_obj
                    .get(serde_yaml::Value::String("repos".to_string()))
                    .and_then(|r| r.as_sequence())
                {
                    for item in repos_seq {
                        if let Some(m) = item.as_mapping() {
                            let clean_path = clean_path_str(&get_str(m, "path"));
                            if clean_path.is_empty() {
                                continue;
                            }
                            let raw_name = get_str(m, "name");
                            let repo_name = derive_repo_display_name(&raw_name, "", &clean_path);
                            members.push(repo_name.clone());

                            let norm = clean_path.to_ascii_lowercase();
                            if seen_paths.insert(norm) {
                                let ws = get_str(m, "workspace");
                                let item_proj = get_str(m, "project");
                                let effective_proj = if !item_proj.is_empty() {
                                    Some(item_proj)
                                } else {
                                    Some(proj_name.clone())
                                };
                                if !ws.is_empty() {
                                    workspaces.insert(ws.clone());
                                }
                                inv.repos.push(InventoryRepo {
                                    name: repo_name,
                                    path: clean_path,
                                    workspace: (!ws.is_empty()).then_some(ws),
                                    project: effective_proj,
                                    state: None,
                                });
                            }
                        }
                    }
                }
            }
            inv.projects.insert(proj_name, members);
        }
    }

    inv.workspaces = workspaces.into_iter().collect();
    inv
}

fn resolve_bridge_runtime_paths(exe_path: &Path) -> BridgeRuntimePaths {
    let managed = GortexService::get_managed_env_vars(exe_path);
    let mut map: BTreeMap<String, String> = managed.into_iter().collect();

    for key in [
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "GORTEX_DAEMON_SOCKET",
        "GORTEX_DAEMON_PIDFILE",
        "GORTEX_DAEMON_LOGFILE",
        "GORTEX_DAEMON_STATEFILE",
        "GORTEX_INDEX_WORKERS",
        "GORTEX_RECONCILE_INTERVAL",
        "GORTEX_DAEMON_IDLE_TIMEOUT",
    ] {
        if let Ok(existing) = std::env::var(key) {
            if !existing.trim().is_empty() {
                map.insert(key.to_string(), existing.trim().to_string());
            }
        }
    }

    let socket_path = PathBuf::from(
        map.get("GORTEX_DAEMON_SOCKET")
            .cloned()
            .unwrap_or_default(),
    );
    let install_root = socket_path
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .unwrap_or_else(GortexService::get_install_dir);
    let xdg_config = map
        .get("XDG_CONFIG_HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| install_root.join("config"));
    let config_yaml_path = xdg_config.join("gortex").join("config.yaml");

    BridgeRuntimePaths {
        install_root,
        socket_path,
        config_yaml_path,
        env_vars: map.into_iter().collect(),
    }
}

fn load_tracked_inventory(
    socket_path: Option<&Path>,
    config_yaml_path: &Path,
    use_projects_json_fallback: bool,
) -> TrackedInventory {
    let mut cfg_inv = load_config_yaml_inventory(config_yaml_path);
    let probe_repos = socket_path
        .and_then(|sock| query_daemon_probe(sock).ok())
        .map(|p| p.tracked_repos)
        .unwrap_or_default();

    let mut by_norm: BTreeMap<String, InventoryRepo> = BTreeMap::new();
    let mut workspaces: BTreeSet<String> = cfg_inv.workspaces.iter().cloned().collect();

    for r in cfg_inv.repos.drain(..) {
        let norm = r.path.to_ascii_lowercase();
        by_norm.insert(norm, r);
    }

    for pr in probe_repos {
        let clean = clean_path_str(&pr.path);
        if clean.is_empty() {
            continue;
        }
        let norm = clean.to_ascii_lowercase();
        let name = derive_repo_display_name(&pr.name, &pr.prefix, &clean);
        let ws = (!pr.workspace.trim().is_empty()).then_some(pr.workspace.trim().to_string());
        if let Some(ref w) = ws {
            workspaces.insert(w.clone());
        }
        let proj = (!pr.project.trim().is_empty()).then_some(pr.project.trim().to_string());
        let st = (!pr.state.trim().is_empty()).then_some(pr.state.trim().to_string());

        if let Some(existing) = by_norm.get_mut(&norm) {
            if !pr.prefix.trim().is_empty() || !pr.name.trim().is_empty() {
                existing.name = name;
            }
            if existing.workspace.is_none() {
                existing.workspace = ws;
            }
            if existing.project.is_none() {
                existing.project = proj;
            }
            existing.state = st;
        } else {
            by_norm.insert(
                norm,
                InventoryRepo {
                    name,
                    path: clean,
                    workspace: ws,
                    project: proj,
                    state: st,
                },
            );
        }
    }

    if by_norm.is_empty() && use_projects_json_fallback {
        let local_projects_json = config_yaml_path
            .parent()
            .and_then(|p| p.parent())
            .map(|cfg| cfg.join("projects.json"));
        let mut fallback_paths: Vec<String> = local_projects_json
            .as_ref()
            .filter(|p| p.is_file())
            .and_then(|p| fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v.get("projects")?.as_array().cloned())
            .map(|arr| {
                arr.into_iter()
                    .filter_map(|x| x.as_str().map(clean_path_str))
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        if fallback_paths.is_empty() {
            fallback_paths = GortexService::get_tracked_projects();
        }
        for clean in fallback_paths {
            let clean = clean_path_str(&clean);
            if clean.is_empty() {
                continue;
            }
            let norm = clean.to_ascii_lowercase();
            let name = derive_repo_display_name("", "", &clean);
            by_norm.entry(norm).or_insert_with(|| InventoryRepo {
                name,
                path: clean,
                workspace: None,
                project: None,
                state: None,
            });
        }
    }

    let mut repos: Vec<InventoryRepo> = by_norm.into_values().collect();
    repos.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.path.cmp(&b.path)));

    TrackedInventory {
        repos,
        projects: cfg_inv.projects,
        active_project: cfg_inv.active_project,
        workspaces: workspaces.into_iter().collect(),
    }
}

pub fn json_rpc_id_key(id: &Value) -> Option<String> {
    if id.is_null() {
        return None;
    }
    serde_json::to_string(id).ok()
}

pub fn classify_intercept_request(req: &Value) -> Option<(String, InterceptKind)> {
    let id = req.get("id").and_then(json_rpc_id_key)?;
    let method = req.get("method").and_then(|v| v.as_str())?;
    let params = req.get("params")?;

    match method {
        "tools/call" => {
            let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let args = params.get("arguments");
            let kind = match tool_name {
                "workspace" => {
                    let op = args
                        .and_then(|a| a.get("operation"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("info")
                        .trim();
                    match op {
                        "" | "info" => InterceptKind::WorkspaceInfo,
                        "repos" => InterceptKind::WorkspaceRepos,
                        "active_project" => InterceptKind::ActiveProject,
                        _ => return None,
                    }
                }
                "list_repos" => InterceptKind::WorkspaceRepos,
                "workspace_info" => InterceptKind::WorkspaceInfo,
                "get_active_project" => InterceptKind::ActiveProject,
                _ => return None,
            };
            Some((id, kind))
        }
        "resources/read" => {
            let uri = params.get("uri").and_then(|v| v.as_str()).unwrap_or("");
            let kind = match uri {
                "gortex://repos" => InterceptKind::ResourceRepos,
                "gortex://workspace" => InterceptKind::ResourceWorkspace,
                "gortex://active-project" => InterceptKind::ResourceActiveProject,
                _ => return None,
            };
            Some((id, kind))
        }
        _ => None,
    }
}

fn payload_is_empty_unbound(text: &str, kind: InterceptKind) -> bool {
    let trimmed = text.trim();
    if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
        if let Some(obj) = val.as_object() {
            let mode = obj.get("mode").and_then(|v| v.as_str()).unwrap_or("");
            let repos_empty = obj
                .get("repos")
                .and_then(|v| v.as_array())
                .map(|a| a.is_empty())
                .unwrap_or(false);
            let members_empty = obj
                .get("members")
                .and_then(|v| v.as_array())
                .map(|a| a.is_empty())
                .unwrap_or(true);
            let ws = obj.get("workspace").and_then(|v| v.as_str()).unwrap_or("");

            if mode == "unbound" && repos_empty && members_empty {
                return true;
            }
            if mode == "workspace"
                && ws.contains("gortex-unresolved-workspace")
                && repos_empty
                && members_empty
            {
                return true;
            }
            if matches!(kind, InterceptKind::ActiveProject | InterceptKind::ResourceActiveProject)
                && repos_empty
                && obj.get("bound").and_then(|v| v.as_bool()) != Some(true)
            {
                return true;
            }
        }
    }

    trimmed.starts_with("mode: unbound") && (trimmed.contains("repos[0]:") || !trimmed.contains("repos"))
}

pub fn build_enriched_payload(
    kind: InterceptKind,
    inventory: &TrackedInventory,
    host_cwd_hint: Option<&str>,
) -> Value {
    let repo_names: Vec<String> = inventory.repos.iter().map(|r| r.name.clone()).collect();
    let workspace_label = if inventory.workspaces.len() == 1 {
        Value::String(inventory.workspaces[0].clone())
    } else if !inventory.workspaces.is_empty() {
        Value::String(inventory.workspaces[0].clone())
    } else {
        Value::Null
    };

    let host_cwd_val = host_cwd_hint
        .map(clean_path_str)
        .filter(|s| !s.is_empty())
        .map(Value::String)
        .unwrap_or(Value::Null);

    let note = if repo_names.is_empty() {
        "No repositories are currently tracked in Gortex. Track a repository in code-Manager (or call workspace_admin.track) before querying code."
    } else if host_cwd_val.is_string() {
        "Global cross-project session active; host_cwd is not tracked in Gortex. Match your target project against `members[].path` and pass its `repo` prefix (or track host_cwd explicitly)."
    } else {
        "Global cross-project session active across all tracked Gortex repositories. Match your conversation workspace path against `members[].path` and pass `repo` on tool calls."
    };

    let mut obj = serde_json::Map::new();
    match kind {
        InterceptKind::WorkspaceRepos | InterceptKind::ResourceRepos => {
            obj.insert("mode".to_string(), Value::String("unbound_global".to_string()));
            obj.insert("global_scope".to_string(), Value::Bool(true));
            obj.insert("repos".to_string(), json!(repo_names));
            obj.insert("members".to_string(), json!(inventory.repos));
            if !inventory.workspaces.is_empty() {
                obj.insert("workspaces".to_string(), json!(inventory.workspaces));
            }
            if !inventory.projects.is_empty() {
                obj.insert("projects".to_string(), json!(inventory.projects));
            }
            if !inventory.active_project.is_empty() {
                obj.insert("active_project".to_string(), json!(inventory.active_project));
            }
            obj.insert("host_cwd".to_string(), host_cwd_val);
            obj.insert("host_cwd_tracked".to_string(), Value::Bool(false));
            obj.insert("note".to_string(), Value::String(note.to_string()));
        }
        InterceptKind::WorkspaceInfo | InterceptKind::ResourceWorkspace => {
            obj.insert("mode".to_string(), Value::String("unbound_global".to_string()));
            obj.insert("global_scope".to_string(), Value::Bool(true));
            obj.insert("workspace".to_string(), workspace_label);
            obj.insert("workspaces".to_string(), json!(inventory.workspaces));
            obj.insert("project".to_string(), json!(inventory.active_project));
            obj.insert("repos".to_string(), json!(repo_names));
            obj.insert("members".to_string(), json!(inventory.repos));
            if !inventory.projects.is_empty() {
                obj.insert("projects".to_string(), json!(inventory.projects));
            }
            obj.insert("host_cwd".to_string(), host_cwd_val);
            obj.insert("host_cwd_tracked".to_string(), Value::Bool(false));
            obj.insert("note".to_string(), Value::String(note.to_string()));
        }
        InterceptKind::ActiveProject | InterceptKind::ResourceActiveProject => {
            obj.insert("project".to_string(), json!(inventory.active_project));
            obj.insert("global_scope".to_string(), Value::Bool(true));
            obj.insert("repos".to_string(), json!(inventory.repos));
            if !inventory.projects.is_empty() {
                obj.insert("projects".to_string(), json!(inventory.projects));
            }
            obj.insert("host_cwd".to_string(), host_cwd_val);
            obj.insert("host_cwd_tracked".to_string(), Value::Bool(false));
            obj.insert("note".to_string(), Value::String(note.to_string()));
        }
    }
    Value::Object(obj)
}

pub fn rewrite_unbound_jsonrpc_response<F>(
    raw_line: &str,
    kind: InterceptKind,
    host_cwd_hint: Option<&str>,
    inventory_loader: F,
) -> Option<String>
where
    F: FnOnce() -> TrackedInventory,
{
    let mut resp: Value = serde_json::from_str(raw_line).ok()?;
    let result = resp.get_mut("result")?.as_object_mut()?;

    if result.get("isError").and_then(|v| v.as_bool()) == Some(true) {
        return None;
    }

    match kind {
        InterceptKind::WorkspaceRepos
        | InterceptKind::WorkspaceInfo
        | InterceptKind::ActiveProject => {
            let content = result.get_mut("content")?.as_array_mut()?;
            let first_text = content
                .first()
                .and_then(|item| item.get("text"))
                .and_then(|v| v.as_str())?;
            if !payload_is_empty_unbound(first_text, kind) {
                return None;
            }
            let inventory = inventory_loader();
            if matches!(kind, InterceptKind::ActiveProject) && inventory.repos.is_empty() {
                return None;
            }
            let enriched = build_enriched_payload(kind, &inventory, host_cwd_hint);
            let enriched_text = serde_json::to_string(&enriched).ok()?;
            if let Some(first_obj) = content.first_mut().and_then(|v| v.as_object_mut()) {
                first_obj.insert("text".to_string(), Value::String(enriched_text));
            }
            if result.contains_key("structuredContent") {
                result.insert("structuredContent".to_string(), enriched);
            }
            serde_json::to_string(&resp).ok()
        }
        InterceptKind::ResourceRepos
        | InterceptKind::ResourceWorkspace
        | InterceptKind::ResourceActiveProject => {
            let contents = result.get_mut("contents")?.as_array_mut()?;
            let first_text = contents
                .first()
                .and_then(|item| item.get("text"))
                .and_then(|v| v.as_str())?;
            if !payload_is_empty_unbound(first_text, kind) {
                return None;
            }
            let inventory = inventory_loader();
            if matches!(kind, InterceptKind::ResourceActiveProject) && inventory.repos.is_empty() {
                return None;
            }
            let enriched = build_enriched_payload(kind, &inventory, host_cwd_hint);
            let enriched_text = serde_json::to_string(&enriched).ok()?;
            if let Some(first_obj) = contents.first_mut().and_then(|v| v.as_object_mut()) {
                first_obj.insert("text".to_string(), Value::String(enriched_text));
            }
            serde_json::to_string(&resp).ok()
        }
    }
}

fn normalize_daemon_client_name(agent: &str) -> String {
    match agent.trim().to_ascii_lowercase().as_str() {
        "" => "unknown".to_string(),
        "claude" => "claude-code".to_string(),
        other => other.to_string(),
    }
}

// ============================================================================
// Socket Transport (Windows WinSock AF_UNIX & Unix domain sockets)
// ============================================================================

#[cfg(target_os = "windows")]
mod sock_impl {
    use super::*;
    use std::io;
    use windows_sys::Win32::Networking::WinSock::{
        closesocket, connect, recv, send, setsockopt, shutdown, socket, WSAGetLastError,
        WSAStartup, AF_UNIX, INVALID_SOCKET, SD_BOTH, SOCKADDR_UN, SOCKET, SOCKET_ERROR,
        SOCK_STREAM, SOL_SOCKET, SO_RCVTIMEO, SO_SNDTIMEO, WSADATA,
    };

    static WSA_INIT: Once = Once::new();

    fn ensure_wsa() {
        WSA_INIT.call_once(|| unsafe {
            let mut data: WSADATA = std::mem::zeroed();
            let _ = WSAStartup(0x0202, &mut data);
        });
    }

    pub struct DaemonSocket {
        raw: SOCKET,
    }

    impl DaemonSocket {
        pub fn connect(path: &Path, io_timeout: Option<Duration>) -> Result<Self, String> {
            ensure_wsa();
            let path_str = clean_path_str(&path.to_string_lossy());
            let path_bytes = path_str.as_bytes();
            if path_bytes.is_empty() || path_bytes.len() >= 108 {
                return Err(format!("无效的 AF_UNIX socket 路径长度: {}", path_str));
            }

            let raw = unsafe { socket(AF_UNIX as i32, SOCK_STREAM as i32, 0) };
            if raw == INVALID_SOCKET {
                let err = unsafe { WSAGetLastError() };
                return Err(format!("创建 AF_UNIX socket 失败 (WSA {})", err));
            }

            let sock = Self { raw };
            if let Some(timeout) = io_timeout {
                let ms = timeout.as_millis().min(u32::MAX as u128) as u32;
                let bytes = ms.to_ne_bytes();
                unsafe {
                    let _ = setsockopt(
                        sock.raw,
                        SOL_SOCKET,
                        SO_RCVTIMEO,
                        bytes.as_ptr(),
                        bytes.len() as i32,
                    );
                    let _ = setsockopt(
                        sock.raw,
                        SOL_SOCKET,
                        SO_SNDTIMEO,
                        bytes.as_ptr(),
                        bytes.len() as i32,
                    );
                }
            }

            let mut addr: SOCKADDR_UN = unsafe { std::mem::zeroed() };
            addr.sun_family = AF_UNIX;
            for (idx, &b) in path_bytes.iter().enumerate() {
                addr.sun_path[idx] = b as _;
            }

            let rc = unsafe {
                connect(
                    sock.raw,
                    (&addr as *const SOCKADDR_UN).cast(),
                    std::mem::size_of::<SOCKADDR_UN>() as i32,
                )
            };
            if rc == SOCKET_ERROR {
                let err = unsafe { WSAGetLastError() };
                return Err(format!("连接 Gortex daemon socket 失败 (WSA {}): {}", err, path_str));
            }

            Ok(sock)
        }

        pub fn clear_timeouts(&self) {
            let zero = 0u32.to_ne_bytes();
            unsafe {
                let _ = setsockopt(
                    self.raw,
                    SOL_SOCKET,
                    SO_RCVTIMEO,
                    zero.as_ptr(),
                    zero.len() as i32,
                );
                let _ = setsockopt(
                    self.raw,
                    SOL_SOCKET,
                    SO_SNDTIMEO,
                    zero.as_ptr(),
                    zero.len() as i32,
                );
            }
        }

        pub fn write_all_bytes(&self, mut buf: &[u8]) -> Result<(), String> {
            while !buf.is_empty() {
                let chunk_len = buf.len().min(i32::MAX as usize) as i32;
                let sent = unsafe { send(self.raw, buf.as_ptr(), chunk_len, 0) };
                if sent <= 0 {
                    let err = unsafe { WSAGetLastError() };
                    return Err(format!("写入 daemon socket 失败 (WSA {})", err));
                }
                buf = &buf[sent as usize..];
            }
            Ok(())
        }

        pub fn recv_bytes(&self, buf: &mut [u8]) -> io::Result<usize> {
            let max_len = buf.len().min(i32::MAX as usize) as i32;
            let n = unsafe { recv(self.raw, buf.as_mut_ptr(), max_len, 0) };
            if n < 0 {
                let code = unsafe { WSAGetLastError() };
                return Err(io::Error::from_raw_os_error(code));
            }
            Ok(n as usize)
        }

        pub fn shutdown_both(&self) {
            unsafe {
                let _ = shutdown(self.raw, SD_BOTH);
            }
        }
    }

    impl Drop for DaemonSocket {
        fn drop(&mut self) {
            unsafe {
                let _ = closesocket(self.raw);
            }
        }
    }

    pub struct SocketReader {
        sock: Arc<DaemonSocket>,
    }

    impl SocketReader {
        pub fn new(sock: Arc<DaemonSocket>) -> Self {
            Self { sock }
        }
    }

    impl std::io::Read for SocketReader {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.sock.recv_bytes(buf)
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod sock_impl {
    use super::*;
    use std::io::{self, Read, Write};
    use std::net::Shutdown;
    use std::os::unix::net::UnixStream;

    pub struct DaemonSocket {
        stream: UnixStream,
    }

    impl DaemonSocket {
        pub fn connect(path: &Path, io_timeout: Option<Duration>) -> Result<Self, String> {
            let stream = UnixStream::connect(path)
                .map_err(|e| format!("连接 daemon socket 失败: {}", e))?;
            if let Some(t) = io_timeout {
                let _ = stream.set_read_timeout(Some(t));
                let _ = stream.set_write_timeout(Some(t));
            }
            Ok(Self { stream })
        }

        pub fn clear_timeouts(&self) {
            let _ = self.stream.set_read_timeout(None);
            let _ = self.stream.set_write_timeout(None);
        }

        pub fn write_all_bytes(&self, buf: &[u8]) -> Result<(), String> {
            let mut s = &self.stream;
            s.write_all(buf).map_err(|e| format!("写入 daemon socket 失败: {}", e))
        }

        pub fn shutdown_both(&self) {
            let _ = self.stream.shutdown(Shutdown::Both);
        }
    }

    pub struct SocketReader {
        stream: UnixStream,
    }

    impl SocketReader {
        pub fn new(sock: Arc<DaemonSocket>) -> Self {
            Self {
                stream: sock.stream.try_clone().expect("clone unix stream"),
            }
        }
    }

    impl Read for SocketReader {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.stream.read(buf)
        }
    }
}

use sock_impl::{DaemonSocket, SocketReader};

fn write_json_line(sock: &DaemonSocket, val: &Value) -> Result<(), String> {
    let mut line = serde_json::to_string(val).map_err(|e| format!("序列化 JSON 失败: {}", e))?;
    line.push('\n');
    sock.write_all_bytes(line.as_bytes())
}

fn read_json_line<R: BufRead>(reader: &mut R) -> Result<String, String> {
    let mut line = String::new();
    let n = reader
        .read_line(&mut line)
        .map_err(|e| format!("读取 socket 行失败: {}", e))?;
    if n == 0 {
        return Err("daemon socket unexpected EOF".to_string());
    }
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}

pub fn query_daemon_probe(socket_path: &Path) -> Result<DaemonProbeResult, String> {
    if socket_path.as_os_str().is_empty() {
        return Err("daemon socket path empty".to_string());
    }
    let sock = Arc::new(DaemonSocket::connect(socket_path, Some(Duration::from_secs(3)))?);
    let handshake = json!({
        "version": 1,
        "mode": "control",
        "client": "cli",
        "pid": std::process::id(),
    });
    write_json_line(&sock, &handshake)?;

    let mut reader = BufReader::new(SocketReader::new(Arc::clone(&sock)));
    let ack_line = read_json_line(&mut reader)?;
    let ack: DaemonHandshakeAck =
        serde_json::from_str(&ack_line).map_err(|e| format!("解析 probe ack 失败: {}", e))?;
    if !ack.ok {
        return Err(format!("daemon probe handshake rejected: {} {}", ack.error_code, ack.error_msg));
    }

    write_json_line(&sock, &json!({ "kind": "probe" }))?;
    let resp_line = read_json_line(&mut reader)?;
    let resp: DaemonControlResponse =
        serde_json::from_str(&resp_line).map_err(|e| format!("解析 probe response 失败: {}", e))?;
    if !resp.ok {
        return Err(format!("daemon probe error: {} {}", resp.error_code, resp.error_msg));
    }
    let result_val = resp.result.unwrap_or_else(|| json!({}));
    serde_json::from_value(result_val).map_err(|e| format!("解析 ProbeResponse 失败: {}", e))
}

fn dial_mcp_session(
    socket_path: &Path,
    client_name: &str,
    logical_session_id: &str,
) -> Result<(Arc<DaemonSocket>, BufReader<SocketReader>), String> {
    let sock = Arc::new(DaemonSocket::connect(socket_path, Some(Duration::from_secs(5)))?);
    let handshake = json!({
        "version": 1,
        "mode": "mcp",
        "cwd": "",
        "client": client_name,
        "pid": std::process::id(),
        "logical_session_id": logical_session_id,
    });
    write_json_line(&sock, &handshake)?;

    let mut reader = BufReader::new(SocketReader::new(Arc::clone(&sock)));
    let ack_line = read_json_line(&mut reader)?;
    let ack: DaemonHandshakeAck =
        serde_json::from_str(&ack_line).map_err(|e| format!("解析 mcp ack 失败: {}", e))?;
    if !ack.ok {
        return Err(format!(
            "daemon mcp handshake rejected ({}): {}",
            ack.error_code, ack.error_msg
        ));
    }

    // Clear initial handshake timeouts so idle streaming sessions never time out on blocking reads.
    sock.clear_timeouts();
    let _ = &ack.session_id;
    Ok((sock, reader))
}

fn spawn_gortex_daemon_detached(exe_path: &Path, env_vars: &[(String, String)]) -> Result<(), String> {
    let mut cmd = Command::new(exe_path);
    cmd.args(["daemon", "start", "--detach"]);
    for (k, v) in env_vars {
        cmd.env(k, v);
    }
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    cmd.spawn()
        .map_err(|e| format!("后台拉起 gortex daemon 失败: {}", e))?;
    Ok(())
}

fn ensure_daemon_and_dial_mcp(
    exe_path: &Path,
    runtime: &BridgeRuntimePaths,
    client_name: &str,
    logical_session_id: &str,
) -> Result<(Arc<DaemonSocket>, BufReader<SocketReader>), String> {
    if let Ok(conn) = dial_mcp_session(&runtime.socket_path, client_name, logical_session_id) {
        return Ok(conn);
    }

    if let Some(run_dir) = runtime.socket_path.parent() {
        let _ = fs::create_dir_all(run_dir);
    }
    spawn_gortex_daemon_detached(exe_path, &runtime.env_vars)?;

    let deadline = Instant::now() + Duration::from_secs(25);
    let mut last_err = String::new();
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(150));
        match dial_mcp_session(&runtime.socket_path, client_name, logical_session_id) {
            Ok(conn) => return Ok(conn),
            Err(e) => last_err = e,
        }
    }

    Err(format!("等待 Gortex daemon 就绪超时: {}", last_err))
}

fn spawn_socket_reader_thread(
    conn_id: u64,
    mut reader: BufReader<SocketReader>,
    tx: Sender<RelayEvent>,
) {
    std::thread::spawn(move || {
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    let _ = tx.send(RelayEvent::DaemonClosed(conn_id));
                    break;
                }
                Ok(_) => {
                    let trimmed = line.trim_end_matches(['\r', '\n']);
                    if !trimmed.is_empty() {
                        if tx.send(RelayEvent::FromDaemon(conn_id, trimmed.to_string())).is_err() {
                            break;
                        }
                    }
                }
                Err(_) => {
                    let _ = tx.send(RelayEvent::DaemonClosed(conn_id));
                    break;
                }
            }
        }
    });
}

fn emit_stdout_line(stdout: &mut std::io::StdoutLock<'_>, line: &str) -> Result<(), String> {
    stdout
        .write_all(line.as_bytes())
        .and_then(|_| stdout.write_all(b"\n"))
        .and_then(|_| stdout.flush())
        .map_err(|e| format!("写入 MCP stdout 失败: {}", e))
}

fn emit_rpc_error(stdout: &mut std::io::StdoutLock<'_>, id: Value, message: &str) -> Result<(), String> {
    let err_payload = json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": -32000,
            "message": message,
        }
    });
    let line = serde_json::to_string(&err_payload).map_err(|e| e.to_string())?;
    emit_stdout_line(stdout, &line)
}

fn run_unbound_global_relay(
    exe_path: &Path,
    agent: &str,
    host_cwd_hint: Option<&Path>,
    runtime: &BridgeRuntimePaths,
) -> ! {
    let client_name = normalize_daemon_client_name(agent);
    let logical_session_id = format!("bridge-{}-{}", std::process::id(), client_name);
    let host_cwd_str = host_cwd_hint
        .map(|p| clean_path_str(&p.to_string_lossy()))
        .filter(|s| !s.is_empty());

    let (initial_sock, initial_reader) = match ensure_daemon_and_dial_mcp(
        exe_path,
        runtime,
        &client_name,
        &logical_session_id,
    ) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[gortex-bridge] 全局未绑定中继连接失败: {}", e);
            std::process::exit(1);
        }
    };

    let (tx, rx) = mpsc::channel::<RelayEvent>();
    let mut current_conn_id: u64 = 1;
    let mut current_sock: Option<Arc<DaemonSocket>> = Some(initial_sock);
    spawn_socket_reader_thread(current_conn_id, initial_reader, tx.clone());

    let stdin_tx = tx.clone();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut reader = BufReader::new(stdin.lock());
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    let _ = stdin_tx.send(RelayEvent::ClientEof);
                    break;
                }
                Ok(_) => {
                    let trimmed = line.trim_end_matches(['\r', '\n']);
                    if !trimmed.is_empty() {
                        if stdin_tx.send(RelayEvent::FromClient(trimmed.to_string())).is_err() {
                            break;
                        }
                    }
                }
                Err(_) => {
                    let _ = stdin_tx.send(RelayEvent::ClientEof);
                    break;
                }
            }
        }
    });

    let mut pending_intercepts: HashMap<String, InterceptKind> = HashMap::new();
    let mut pending_raw_ids: HashMap<String, Value> = HashMap::new();
    let mut suppressed_init_ids: HashSet<String> = HashSet::new();
    let mut init_frame: Option<String> = None;
    let mut initialized_frame: Option<String> = None;

    let stdout = std::io::stdout();
    let mut stdout_lock = stdout.lock();

    while let Ok(event) = rx.recv() {
        match event {
            RelayEvent::ClientEof => {
                if let Some(ref sock) = current_sock {
                    sock.shutdown_both();
                }
                break;
            }
            RelayEvent::DaemonClosed(closed_id) => {
                if closed_id == current_conn_id {
                    if let Some(sock) = current_sock.take() {
                        sock.shutdown_both();
                    }
                    for (_, raw_id) in pending_raw_ids.drain() {
                        let _ = emit_rpc_error(
                            &mut stdout_lock,
                            raw_id,
                            "Gortex daemon connection reset; please retry the tool call.",
                        );
                    }
                    pending_intercepts.clear();
                }
            }
            RelayEvent::FromDaemon(conn_id, line) => {
                if conn_id != current_conn_id {
                    continue;
                }
                let out_line = if let Ok(val) = serde_json::from_str::<Value>(&line) {
                    if let Some(id_key) = val.get("id").and_then(json_rpc_id_key) {
                        if suppressed_init_ids.remove(&id_key) {
                            continue;
                        }
                        pending_raw_ids.remove(&id_key);
                        if let Some(kind) = pending_intercepts.remove(&id_key) {
                            rewrite_unbound_jsonrpc_response(
                                &line,
                                kind,
                                host_cwd_str.as_deref(),
                                || {
                                    load_tracked_inventory(
                                        Some(&runtime.socket_path),
                                        &runtime.config_yaml_path,
                                        true,
                                    )
                                },
                            )
                            .unwrap_or(line)
                        } else {
                            line
                        }
                    } else {
                        line
                    }
                } else {
                    line
                };
                if emit_stdout_line(&mut stdout_lock, &out_line).is_err() {
                    break;
                }
            }
            RelayEvent::FromClient(line) => {
                let parsed = serde_json::from_str::<Value>(&line).ok();
                let req_id_opt = parsed.as_ref().and_then(|v| v.get("id")).cloned().filter(|v| !v.is_null());
                let req_method = parsed
                    .as_ref()
                    .and_then(|v| v.get("method"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                if req_method == "initialize" {
                    init_frame = Some(line.clone());
                } else if req_method == "notifications/initialized" {
                    initialized_frame = Some(line.clone());
                }

                if current_sock.is_none() {
                    match ensure_daemon_and_dial_mcp(
                        exe_path,
                        runtime,
                        &client_name,
                        &logical_session_id,
                    ) {
                        Ok((new_sock, new_reader)) => {
                            current_conn_id += 1;
                            spawn_socket_reader_thread(current_conn_id, new_reader, tx.clone());
                            if req_method != "initialize" {
                                if let Some(ref init) = init_frame {
                                    if let Ok(iv) = serde_json::from_str::<Value>(init) {
                                        if let Some(id_k) = iv.get("id").and_then(json_rpc_id_key) {
                                            suppressed_init_ids.insert(id_k);
                                        }
                                    }
                                    let _ = new_sock.write_all_bytes(format!("{}\n", init).as_bytes());
                                    if let Some(ref inited) = initialized_frame {
                                        let _ = new_sock.write_all_bytes(format!("{}\n", inited).as_bytes());
                                    }
                                }
                            }
                            current_sock = Some(new_sock);
                        }
                        Err(e) => {
                            if let Some(raw_id) = req_id_opt {
                                let _ = emit_rpc_error(
                                    &mut stdout_lock,
                                    raw_id,
                                    &format!("Gortex daemon unavailable: {}", e),
                                );
                            }
                            continue;
                        }
                    }
                }

                let mut recorded_id_key = None;
                if let Some(ref val) = parsed {
                    if let Some((id_k, kind)) = classify_intercept_request(val) {
                        pending_intercepts.insert(id_k.clone(), kind);
                    }
                    if let Some(raw_id) = req_id_opt.clone() {
                        if let Some(id_k) = json_rpc_id_key(&raw_id) {
                            pending_raw_ids.insert(id_k.clone(), raw_id);
                            recorded_id_key = Some(id_k);
                        }
                    }
                }

                let frame = format!("{}\n", line);
                let send_ok = current_sock
                    .as_ref()
                    .map(|s| s.write_all_bytes(frame.as_bytes()).is_ok())
                    .unwrap_or(false);
                if !send_ok {
                    if let Some(sock) = current_sock.take() {
                        sock.shutdown_both();
                    }
                    if let Some(id_k) = recorded_id_key {
                        pending_intercepts.remove(&id_k);
                        pending_raw_ids.remove(&id_k);
                    }
                    if let Some(raw_id) = req_id_opt {
                        let _ = emit_rpc_error(
                            &mut stdout_lock,
                            raw_id,
                            "Failed to deliver MCP request to Gortex daemon.",
                        );
                    }
                }
            }
        }
    }

    std::process::exit(0);
}

fn spawn_bound_direct_mcp(
    exe_path: &Path,
    agent: &str,
    target_cwd: &Path,
    runtime: &BridgeRuntimePaths,
) -> ! {
    let mut cmd = Command::new(exe_path);
    cmd.arg("mcp");
    cmd.current_dir(target_cwd);
    cmd.stdin(Stdio::inherit());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());

    for (k, v) in &runtime.env_vars {
        cmd.env(k, v);
    }

    let cwd_str = clean_path_str(&target_cwd.to_string_lossy());
    cmd.env("ANTIGRAVITY_WORKSPACE", &cwd_str);
    cmd.env("WORKSPACE", &cwd_str);
    cmd.env("PROJECT_DIR", &cwd_str);
    if !agent.is_empty() {
        let env_key = format!("{}_WORKSPACE", agent.to_uppercase().replace('-', "_"));
        cmd.env(env_key, &cwd_str);
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    match cmd.status() {
        Ok(status) => std::process::exit(status.code().unwrap_or(0)),
        Err(e) => {
            eprintln!("[gortex-bridge] Gortex 进程启动失败: {}", e);
            std::process::exit(1);
        }
    }
}

pub fn run_gortex_bridge(args: &[String]) -> ! {
    let parsed = parse_bridge_args(args);
    let mut gortex_exe = parsed.gortex_exe;

    if gortex_exe.is_empty() {
        if let Ok(val) = std::env::var("GORTEX_EXECUTABLE") {
            gortex_exe = val;
        }
    }
    if gortex_exe.is_empty() {
        if let Some(m) = GortexService::get_managed_executable_path() {
            gortex_exe = m.to_string_lossy().to_string();
        }
    }
    if gortex_exe.is_empty() {
        gortex_exe = GortexService::get_executable_path().to_string_lossy().to_string();
    }

    let exe_path = PathBuf::from(&gortex_exe);
    if !exe_path.is_file() {
        eprintln!("[gortex-bridge] 未找到有效的 Gortex 可执行文件: {}", gortex_exe);
        std::process::exit(1);
    }

    let runtime = resolve_bridge_runtime_paths(&exe_path);
    let candidate_cwd = collect_candidate_cwd(&parsed.cwd);

    if let Some(ref cwd) = candidate_cwd {
        let inv = load_tracked_inventory(
            Some(&runtime.socket_path),
            &runtime.config_yaml_path,
            true,
        );
        let tracked_paths: Vec<String> = inv.repos.iter().map(|r| r.path.clone()).collect();
        if is_cwd_covered_or_contained(cwd, &tracked_paths) {
            spawn_bound_direct_mcp(&exe_path, &parsed.agent, cwd, &runtime);
        }
    }

    // No valid tracked project CWD (Antigravity Desktop, global Codex, 0 repos, or untracked dir):
    // connect with CWD="" via AF_UNIX global unbound relay instead of falling back to "1st repo".
    run_unbound_global_relay(&exe_path, &parsed.agent, candidate_cwd.as_deref(), &runtime);
}
