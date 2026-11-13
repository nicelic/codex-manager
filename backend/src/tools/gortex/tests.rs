#[cfg(test)]
mod tests {
    use crate::tools::gortex::ownership::*;
    use crate::tools::gortex::reconcile::*;
    use crate::tools::gortex::service::{
        UntrackCliOutcome, build_gortex_project_infos, classify_gortex_daemon_repo_status,
        classify_gortex_untrack_cli_output, has_gortex_database_files,
    };
    use crate::tools::gortex::types::*;
    use crate::tools::gortex::windows::*;
    use serde_json::json;

    #[test]
    fn test_ownership_ledger_roundtrip() {
        let mut ownership = GortexMCPOwnership::default();
        ownership.project_mcp_enabled = true;
        ownership.platforms.insert(
            "codex".to_string(),
            GortexOwnedMCP {
                fingerprint: "abc123hash".to_string(),
            },
        );
        ownership.artifacts.insert(
            "codex:prompt:default".to_string(),
            GortexOwnedArtifact {
                agent: "codex".to_string(),
                kind: "prompt".to_string(),
                path: "C:\\Users\\tester\\.codex\\AGENTS.md".to_string(),
                event: None,
                fingerprint: "hash456".to_string(),
                command: None,
                group_fingerprint: None,
                handler_fingerprint: None,
            },
        );

        let json_str = serde_json::to_string(&ownership).expect("serialize");
        let parsed: GortexMCPOwnership = serde_json::from_str(&json_str).expect("deserialize");
        assert_eq!(parsed.project_mcp_enabled, true);
        assert_eq!(parsed.platforms.len(), 1);
        assert_eq!(parsed.artifacts.len(), 1);
    }

    #[test]
    fn test_clean_gortex_project_slug() {
        assert_eq!(clean_gortex_project_slug("C:\\projects\\demo-project"), "demo-project");
        assert_eq!(clean_gortex_project_slug("C:/projects/my-project@v1"), "my-project");
        assert_eq!(clean_gortex_project_slug(""), "project");
    }

    #[test]
    fn test_path_normalization() {
        let entry = "C:\\tools\\gortex\\bin";
        let path_val = "C:\\Windows\\System32;C:\\tools\\gortex\\bin;D:\\Tools";
        assert!(path_contains_entry(path_val, entry));
        assert!(path_contains_entry(path_val, "c:/tools/gortex/bin/"));

        let updated = update_path_entries(path_val, entry, false);
        assert!(!path_contains_entry(&updated, entry));
    }

    #[test]
    fn test_canonical_json_value_sorting() {
        let v1 = json!({ "b": 2, "a": 1, "nested": { "z": 9, "x": 8 } });
        let canon1 = canonical_json_value(&v1);
        let fp1 = gortex_json_artifact_fingerprint(&v1);

        let v2 = json!({ "nested": { "x": 8, "z": 9 }, "a": 1, "b": 2 });
        let canon2 = canonical_json_value(&v2);
        let fp2 = gortex_json_artifact_fingerprint(&v2);

        assert_eq!(canon1, canon2);
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn test_codex_toml_gortex_stripping() {
        let sample = r#"
model = "gpt-5.6-luna"

[plugins."browser@openai-bundled"]
enabled = true

[mcp_servers.gortex]
command = "C:\\path\\to\\gortex.exe"
args = ["mcp"]
startup_timeout_sec = 90
[mcp_servers.gortex.env]
XDG_CONFIG_HOME = "C:\\path\\config"
XDG_DATA_HOME = "C:\\path\\data"

[windows]
sandbox = "elevated"
"#;

        let mut lines = Vec::new();
        let mut skipping = false;
        for line in sample.lines() {
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
        let stripped = lines.join("\n");
        assert!(!stripped.contains("[mcp_servers.gortex]"));
        assert!(!stripped.contains("[mcp_servers.gortex.env]"));
        assert!(!stripped.contains("XDG_CONFIG_HOME"));
        assert!(stripped.contains("gpt-5.6-luna"));
        assert!(stripped.contains("sandbox = \"elevated\""));
    }

    #[test]
    fn test_global_codex_config_valid() {
        let path = std::path::Path::new(r"C:\Users\Administrator\.codex\config.toml");
        if path.is_file() {
            let content = std::fs::read_to_string(path).expect("read codex config.toml");
            let parsed: Result<toml::Value, _> = toml::from_str(&content);
            assert!(parsed.is_ok(), "Failed to parse config.toml: {:?}", parsed.err());
        }
    }

    #[test]
    fn test_codex_mcp_registration_lifecycle() {
        let mut content = "model = \"gpt-5.6-luna\"\n\n[desktop]\nsandbox = \"elevated\"\n".to_string();
        let toml_block = "\r\n[mcp_servers.gortex]\r\ncommand = \"C:\\\\path\\\\gortex.exe\"\r\nargs = [\"mcp\"]\r\nstartup_timeout_sec = 90\r\n[mcp_servers.gortex.env]\r\nXDG_CONFIG_HOME = \"C:\\\\path\\\\config\"\r\n";

        // 1. Initial register
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
        let mut res = lines.join("\r\n");
        res.push_str(toml_block);
        content = res;

        let parsed1: Result<toml::Value, _> = toml::from_str(&content);
        assert!(parsed1.is_ok(), "Failed after 1st register: {:?}", parsed1.err());

        // 2. Second register (update/overwrite)
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
        let mut res = lines.join("\r\n");
        res.push_str(toml_block);
        content = res;

        let parsed2: Result<toml::Value, _> = toml::from_str(&content);
        assert!(parsed2.is_ok(), "Failed after 2nd register: {:?}", parsed2.err());
        assert_eq!(content.matches("[mcp_servers.gortex.env]").count(), 1);

        // 3. Unregister
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
        content = lines.join("\r\n");

        let parsed3: Result<toml::Value, _> = toml::from_str(&content);
        assert!(parsed3.is_ok(), "Failed after unregister: {:?}", parsed3.err());
        assert!(!content.contains("[mcp_servers.gortex]"));
        assert!(!content.contains("[mcp_servers.gortex.env]"));
    }

    #[test]
    fn test_gortex_runtime_state_deserialization() {
        let sample = r#"{
            "pid": 11360,
            "backend_path": "C:\\test\\store.sqlite",
            "startup_phase": "migrating",
            "startup_started_at": 1793187091083,
            "startup_updated_at": 1793187578145
        }"#;
        let st: GortexRuntimeState = serde_json::from_str(sample).expect("parse runtime state");
        assert_eq!(st.pid, Some(11360));
        assert_eq!(st.startup_phase.as_deref(), Some("migrating"));
        assert_eq!(st.startup_started_at, Some(1793187091083));
    }

    #[test]
    fn test_calculate_prompt_slices() {
        use crate::tools::gortex::integrations::calculate_prompt_slices;

        assert_eq!(calculate_prompt_slices(0, 300), vec!["L1-1"]);
        assert_eq!(calculate_prompt_slices(300, 300), vec!["L1-300"]);
        assert_eq!(calculate_prompt_slices(500, 300), vec!["L1-300", "L301-500"]);
        assert_eq!(
            calculate_prompt_slices(1000, 300),
            vec!["L1-300", "L301-600", "L601-900", "L901-1000"]
        );
        assert_eq!(
            calculate_prompt_slices(2429, 300),
            vec![
                "L1-300",
                "L301-600",
                "L601-900",
                "L901-1200",
                "L1201-1500",
                "L1501-1800",
                "L1801-2100",
                "L2101-2400",
                "L2401-2429"
            ]
        );
        assert_eq!(
            calculate_prompt_slices(1000, 0),
            vec!["L1-300", "L301-600", "L601-900", "L901-1000"]
        );
    }

    #[test]
    fn test_gortex_delegated_prompt_body_generation() {
        use crate::tools::gortex::integrations::gortex_delegated_prompt_body;
        use std::path::Path;

        let dummy_path = Path::new("C:/test/gortex提示词.md");
        let prompt = gortex_delegated_prompt_body(dummy_path);
        let (expected_lines, _) = crate::tools::gortex::integrations::gortex_prompt_file_stats(dummy_path);
        let expected_line_str = format!("共计 {} 行", expected_lines);

        assert!(prompt.contains("为方便接下来工作的展开，你必须先全量读取"));
        assert!(prompt.contains("C:\\test\\gortex提示词.md"));
        assert!(prompt.contains(&expected_line_str));
        let expected_slices = crate::tools::gortex::integrations::calculate_prompt_slices(expected_lines, 300);
        let expected_slice_count_str = format!("共 {} 个切片：", expected_slices.len());
        let expected_last_slice = expected_slices.last().unwrap();
        assert!(prompt.contains(&expected_slice_count_str));
        assert!(prompt.contains(expected_last_slice));
        assert!(prompt.contains(&expected_slices.join("、")));
        assert!(prompt.contains("外部文档只读豁免"));
        assert!(prompt.contains("宿主原生只读工具"));
        assert!(prompt.contains("严禁调用 Gortex MCP 读取“gortex提示词.md”"));
        assert!(prompt.contains("顺序分片读取与完整性验收"));
        assert!(prompt.contains("必须等待当前调用返回、验收并补全后"));
        assert!(prompt.contains("禁止并发、批量下发"));
        assert!(prompt.contains("调用尚未返回时等待，不得重发"));
        assert!(prompt.contains("仅补读未验收范围"));
        assert!(prompt.contains("若文档在读取过程中变化"));
        assert!(!prompt.contains("可在首轮同时发起各切片的独立读取"));
        assert!(prompt.contains(&format!("L1 至 L{}", expected_lines)));
        assert!(prompt.contains("后续代码闭环契约"));
        assert!(prompt.contains("使用原生工具"));
        assert!(prompt.contains("全程编码规范"));
        assert!(prompt.contains("UTF-8"));
        assert!(prompt.contains("从开始读取“gortex提示词.md”就必须使用 UTF-8"));
        assert!(prompt.contains("Git 与 GitHub 操作豁免"));
        assert!(!prompt.contains("禁绝补偿读取"));
        assert!(!prompt.contains("可免除预读此文档"));
        assert!(prompt.len() <= 24 * 1024, "delegated prompt exceeds 24KB: {} bytes", prompt.len());

        // 验证单切片分支同样包含完整性验收与 24KB 以内预算控制
        let temp_small = std::env::temp_dir().join(format!(
            "gortex_small_prompt_{}.md",
            std::process::id()
        ));
        let small_content = (1..=50).map(|i| format!("line {}", i)).collect::<Vec<_>>().join("\n");
        std::fs::write(&temp_small, small_content).expect("write temp small prompt");
        let small_prompt = gortex_delegated_prompt_body(&temp_small);
        let _ = std::fs::remove_file(&temp_small);

        assert!(small_prompt.contains("共 50 行"));
        assert!(small_prompt.contains("L1-50"));
        assert!(small_prompt.contains("完整覆盖 1-50"));
        assert!(small_prompt.contains("未达第 50 行"));
        assert!(small_prompt.contains("核验完整载入后"));
        assert!(small_prompt.len() <= 24 * 1024, "single-slice prompt exceeds 24KB: {} bytes", small_prompt.len());
    }

    #[test]
    fn test_gortex_daemon_status_json_deserialization() {
        let sample = r#"{
            "version": "v0.64.11+c7abb9b",
            "pid": 9796,
            "uptime_seconds": 16,
            "ready": true,
            "tracked_repos": [
                {
                    "prefix": "edit-rust",
                    "path": "C:\\EXEXX\\edit-rust",
                    "files": 92,
                    "nodes": 3186,
                    "edges": 19331,
                    "loading": false,
                    "missing": false
                },
                {
                    "prefix": "gortex",
                    "path": "E:\\aex\\Downloads\\gortex-0.64.11\\gortex-0.64.11",
                    "files": 2736,
                    "nodes": 247533,
                    "edges": 1482282,
                    "loading": true,
                    "missing": false
                }
            ]
        }"#;

        let st: GortexDaemonStatusJsonResponse = serde_json::from_str(sample).expect("parse daemon status json");
        assert_eq!(st.pid, Some(9796));
        assert_eq!(st.ready, Some(true));
        assert_eq!(st.tracked_repos.len(), 2);
        assert_eq!(st.tracked_repos[0].path, "C:\\EXEXX\\edit-rust");
        assert_eq!(st.tracked_repos[0].files, 92);
        assert_eq!(st.tracked_repos[0].nodes, 3186);
        assert_eq!(st.tracked_repos[0].loading, false);
        assert_eq!(st.tracked_repos[1].loading, true);
    }

    #[test]
    fn test_gortex_status_response_takeover_serialization() {
        let resp = GortexStatusResponse {
            path: "C:\\Gortex\\bin\\gortex.exe".to_string(),
            managed_root: "C:\\Gortex".to_string(),
            managed_root_exists: true,
            version: "v0.64.11".to_string(),
            installed: true,
            managed_installed: true,
            installing: false,
            running: true,
            starting: false,
            startup_phase: None,
            any_process_running: true,
            unmanaged_process_running: false,
            process_id: Some(24152),
            activation_state: "running".to_string(),
            codex_available: true,
            codex_configured: true,
            codex_complete: true,
            claude_available: false,
            claude_configured: false,
            claude_complete: false,
            cursor_available: false,
            cursor_configured: false,
            cursor_complete: false,
            copilot_available: false,
            copilot_configured: false,
            copilot_complete: false,
            opencode_available: false,
            opencode_configured: false,
            opencode_complete: false,
            antigravity_available: true,
            antigravity_configured: true,
            antigravity_complete: true,
            gemini_available: false,
            gemini_configured: false,
            gemini_complete: false,
            codex_prompt: true,
            codex_prompt_complete: true,
            claude_prompt: false,
            claude_prompt_complete: false,
            cursor_prompt: false,
            cursor_prompt_complete: false,
            copilot_prompt: false,
            copilot_prompt_complete: false,
            opencode_prompt: false,
            opencode_prompt_complete: false,
            antigravity_prompt: true,
            antigravity_prompt_complete: true,
            gemini_prompt: false,
            gemini_prompt_complete: false,
            codex_hook: false,
            codex_hook_complete: false,
            claude_hook: false,
            claude_hook_complete: false,
            copilot_hook: false,
            copilot_hook_complete: false,
            opencode_hook: false,
            opencode_hook_complete: false,
            antigravity_hook: false,
            antigravity_hook_complete: false,
            gemini_hook: false,
            gemini_hook_complete: false,
            user_path: true,
            system_path: true,
            codex_trust_status: None,
            codex_trust_required: false,
            codex_trust_notice: None,
            codex_trust_steps: None,
            tracked_projects: vec!["C:\\EXEXX\\edit-rust".to_string()],
            tracked_details: None,
            daemon_health: Some("serving".to_string()),
            project_mcp_enabled: true,
            project_mcp_projects: None,
            default_project: "C:\\EXEXX\\edit-rust".to_string(),
            integration_present: true,
            active_task: None,
            message: "Gortex daemon 正在运行，已自动受管接管 (PID: Some(24152))。".to_string(),
            takeover_state: Some("managed_adopted".to_string()),
            mcp_client_count: 1,
            gortex_process_count: 2,
        };
        let s = serde_json::to_string(&resp).expect("serialize");
        assert!(s.contains("\"takeover_state\":\"managed_adopted\""));
        assert!(s.contains("\"mcp_client_count\":1"));
        assert!(s.contains("\"gortex_process_count\":2"));
    }

    #[test]
    fn test_reconcile_preserves_external_repos_and_config_helpers() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "edit_rust_reconcile_test_{}_{}",
            std::process::id(),
            nonce
        ));
        std::fs::create_dir_all(&dir).expect("create temp test dir");
        let config_path = dir.join("config.yaml");

        // When seed_missing_ui_projects == false and config.yaml doesn't exist, it is not created.
        let ui_only = vec!["C:\\work\\app-a".to_string()];
        let created = ensure_gortex_global_config_workspaces_at_path(
            &config_path,
            "default",
            &ui_only,
            false,
        )
        .expect("reconcile absent config");
        assert!(!created);
        assert!(!config_path.exists());

        // Seed existing config with external repo, managed duplicate, and projects.* entry.
        let initial_yaml = r#"repos:
  - path: C:\work\external-repo
    name: ext-custom
    workspace: custom-ws
    project: ext-proj
  - path: C:\work\app-a
    workspace: other-ws
    project: wrong-slug
  - path: c:/work/app-a/
    workspace: dup-ws
projects:
  myproj:
    repos:
      - path: C:\work\in-project-repo
"#;
        std::fs::write(&config_path, initial_yaml).expect("write initial config");

        let changed = ensure_gortex_global_config_workspaces_at_path(
            &config_path,
            "default",
            &[
                "C:\\work\\app-a".to_string(),
                "C:\\work\\unseeded".to_string(),
            ],
            false,
        )
        .expect("reconcile existing config");
        assert!(changed);

        let paths = read_gortex_config_repo_paths(&config_path);
        assert!(paths.iter().any(|p| p.eq_ignore_ascii_case("C:\\work\\external-repo")));
        assert!(paths.iter().any(|p| p.eq_ignore_ascii_case("C:\\work\\in-project-repo")));
        assert_eq!(
            paths
                .iter()
                .filter(|p| p.eq_ignore_ascii_case("C:\\work\\app-a"))
                .count(),
            1
        );
        assert!(!paths.iter().any(|p| p.eq_ignore_ascii_case("C:\\work\\unseeded")));

        let raw_after = std::fs::read_to_string(&config_path).expect("read reconciled config");
        assert!(raw_after.contains("ext-custom"));
        assert!(raw_after.contains("custom-ws"));

        // seed_missing_ui_projects == true adds missing managed repos without dropping external ones.
        let seeded = ensure_gortex_global_config_workspaces_at_path(
            &config_path,
            "default",
            &[
                "C:\\work\\app-a".to_string(),
                "C:\\work\\seeded".to_string(),
            ],
            true,
        )
        .expect("seed missing config");
        assert!(seeded);
        assert!(read_gortex_config_repo_paths(&config_path)
            .iter()
            .any(|p| p.eq_ignore_ascii_case("C:\\work\\seeded")));

        // Upsert helper is idempotent and remove only removes top-level repos.
        assert_eq!(
            upsert_project_in_gortex_config(&config_path, "default", "C:\\work\\upserted")
                .expect("first upsert"),
            true
        );
        assert_eq!(
            upsert_project_in_gortex_config(&config_path, "default", "C:\\work\\upserted")
                .expect("second upsert"),
            false
        );
        assert_eq!(
            remove_project_from_gortex_config(&config_path, "c:/work/app-a")
                .expect("remove top-level"),
            true
        );
        assert_eq!(
            remove_project_from_gortex_config(&config_path, "C:\\work\\in-project-repo")
                .expect("remove projects.* should not alter projects section"),
            false
        );

        let final_paths = read_gortex_config_repo_paths(&config_path);
        assert!(!final_paths.iter().any(|p| p.eq_ignore_ascii_case("C:\\work\\app-a")));
        assert!(final_paths.iter().any(|p| p.eq_ignore_ascii_case("C:\\work\\external-repo")));
        assert!(final_paths.iter().any(|p| p.eq_ignore_ascii_case("C:\\work\\in-project-repo")));
        assert!(final_paths.iter().any(|p| p.eq_ignore_ascii_case("C:\\work\\upserted")));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_classify_gortex_untrack_cli_output() {
        assert_eq!(
            classify_gortex_untrack_cli_output(true, r#"{"status":"forgotten"}"#, ""),
            UntrackCliOutcome::Forgotten
        );
        assert_eq!(
            classify_gortex_untrack_cli_output(true, r#"{"status":"untracked"}"#, ""),
            UntrackCliOutcome::Untracked
        );
        assert_eq!(
            classify_gortex_untrack_cli_output(true, r#"{"status":"evicted"}"#, ""),
            UntrackCliOutcome::Untracked
        );
        assert_eq!(
            classify_gortex_untrack_cli_output(false, r#"{"condition":"repo_not_tracked"}"#, ""),
            UntrackCliOutcome::NotTracked
        );
        assert_eq!(
            classify_gortex_untrack_cli_output(true, r#"{"demoted":true,"status":"demoted"}"#, ""),
            UntrackCliOutcome::Demoted
        );
        assert_eq!(
            classify_gortex_untrack_cli_output(true, r#"{"status":"preview","plan":"ready"}"#, ""),
            UntrackCliOutcome::PreviewRequired
        );
        assert_eq!(
            classify_gortex_untrack_cli_output(
                true,
                r#"{"status":"preview","plan":"blocked","blockers":["still wanted by project foo"]}"#,
                ""
            ),
            UntrackCliOutcome::Blocked("still wanted by project foo".to_string())
        );
        assert_eq!(
            classify_gortex_untrack_cli_output(
                true,
                "",
                "[gortex] demoted D:\\repo to automatic lane"
            ),
            UntrackCliOutcome::Demoted
        );
        assert_eq!(
            classify_gortex_untrack_cli_output(
                true,
                "",
                "[gortex] untracked D:\\repo (config only)"
            ),
            UntrackCliOutcome::ConfigOnly
        );
        assert!(matches!(
            classify_gortex_untrack_cli_output(false, "", "Error: unknown flag: --confirm"),
            UntrackCliOutcome::UnsupportedFlagOrCmd(_)
        ));
        assert!(matches!(
            classify_gortex_untrack_cli_output(
                false,
                "",
                r#"Error: daemon does not track "D:\repo""#
            ),
            UntrackCliOutcome::Failed(_)
        ));
        assert_eq!(
            classify_gortex_untrack_cli_output(true, "", "repo is not tracked"),
            UntrackCliOutcome::NotTracked
        );
    }

    #[test]
    fn test_build_gortex_project_infos_and_db_detection() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let install_dir = std::env::temp_dir().join(format!(
            "edit_rust_db_detect_{}_{}",
            std::process::id(),
            nonce
        ));
        let sub_data = install_dir.join("data").join("nested");
        std::fs::create_dir_all(&sub_data).expect("create nested data dir");
        std::fs::write(sub_data.join("log.txt"), b"ok").expect("write non-db file");
        assert!(!has_gortex_database_files(&install_dir));
        std::fs::write(sub_data.join("graph.sqlite"), b"sqlite").expect("write sqlite file");
        assert!(has_gortex_database_files(&install_dir));
        let _ = std::fs::remove_dir_all(&install_dir);

        let daemon_projs = vec![
            GortexProjectInfo {
                path: "C:\\repo\\managed".to_string(),
                name: "managed".to_string(),
                status: "ready".to_string(),
                files: Some(10),
                nodes: Some(500),
                is_external: false,
                tracked: true,
                in_config: false,
                in_daemon: true,
            },
            GortexProjectInfo {
                path: "C:\\repo\\ext".to_string(),
                name: "ext".to_string(),
                status: "ready".to_string(),
                files: Some(5),
                nodes: Some(200),
                is_external: false,
                tracked: true,
                in_config: false,
                in_daemon: true,
            },
            GortexProjectInfo {
                path: "C:\\repo\\residual".to_string(),
                name: "residual".to_string(),
                status: "ready".to_string(),
                files: Some(3),
                nodes: Some(100),
                is_external: false,
                tracked: true,
                in_config: false,
                in_daemon: true,
            },
        ];
        let config_paths = vec![
            "C:\\repo\\managed".to_string(),
            "C:\\repo\\ext".to_string(),
            "C:\\repo\\only-config".to_string(),
        ];
        let local_tracked = vec![
            "C:\\repo\\managed".to_string(),
            "C:\\repo\\stale-local".to_string(),
        ];

        let running_infos = build_gortex_project_infos(
            daemon_projs,
            &config_paths,
            &local_tracked,
            true,
            false,
            None,
        );
        let find_info = |path: &str| {
            running_infos
                .iter()
                .find(|i| i.path.eq_ignore_ascii_case(path))
                .expect(path)
        };

        let managed = find_info("C:\\repo\\managed");
        assert!(managed.tracked && managed.in_daemon && managed.in_config && !managed.is_external);
        assert_eq!(managed.status, "ready");

        let ext = find_info("C:\\repo\\ext");
        assert!(ext.tracked && ext.in_daemon && ext.in_config && ext.is_external);

        let only_config = find_info("C:\\repo\\only-config");
        assert!(only_config.tracked && !only_config.in_daemon && only_config.in_config && only_config.is_external);
        assert_eq!(only_config.status, "indexing");

        let residual = find_info("C:\\repo\\residual");
        assert!(residual.tracked && residual.in_daemon && !residual.in_config && residual.is_external);

        let stale = find_info("C:\\repo\\stale-local");
        assert!(!stale.tracked && !stale.in_daemon && !stale.in_config && !stale.is_external);
        assert_eq!(stale.status, "unindexed");

        // When stopped, config entries stay tracked with stopped status, and local-only stays untracked.
        let stopped_infos = build_gortex_project_infos(
            Vec::new(),
            &["C:\\repo\\managed".to_string()],
            &local_tracked,
            false,
            false,
            None,
        );
        let stopped_managed = stopped_infos
            .iter()
            .find(|i| i.path.eq_ignore_ascii_case("C:\\repo\\managed"))
            .unwrap();
        assert!(stopped_managed.tracked && stopped_managed.in_config && !stopped_managed.in_daemon);
        assert_eq!(stopped_managed.status, "stopped");

        let stopped_stale = stopped_infos
            .iter()
            .find(|i| i.path.eq_ignore_ascii_case("C:\\repo\\stale-local"))
            .unwrap();
        assert!(!stopped_stale.tracked && !stopped_stale.in_config && !stopped_stale.in_daemon);
        assert_eq!(stopped_stale.status, "unindexed");

        // Active track task promotes local-only entry to tracked + indexing.
        let active_task = GortexActiveTask {
            path: "C:\\repo\\stale-local".to_string(),
            action: "track".to_string(),
        };
        let tracking_infos = build_gortex_project_infos(
            Vec::new(),
            &[],
            &local_tracked,
            false,
            false,
            Some(&active_task),
        );
        let tracking_local = tracking_infos
            .iter()
            .find(|i| i.path.eq_ignore_ascii_case("C:\\repo\\stale-local"))
            .unwrap();
        assert!(tracking_local.tracked);
        assert_eq!(tracking_local.status, "indexing");

        // Startup phase alone without an active track task must not mark local-only untracked items as indexing.
        let starting_infos = build_gortex_project_infos(
            Vec::new(),
            &["C:\\repo\\managed".to_string()],
            &local_tracked,
            false,
            true,
            None,
        );
        let starting_managed = starting_infos
            .iter()
            .find(|i| i.path.eq_ignore_ascii_case("C:\\repo\\managed"))
            .unwrap();
        assert!(starting_managed.tracked && starting_managed.status == "indexing");
        let starting_stale = starting_infos
            .iter()
            .find(|i| i.path.eq_ignore_ascii_case("C:\\repo\\stale-local"))
            .unwrap();
        assert!(!starting_stale.tracked && starting_stale.status == "unindexed");
    }

    #[test]
    fn test_classify_gortex_daemon_repo_status() {
        let missing = GortexDaemonTrackedRepo {
            missing: true,
            loading: true,
            ..Default::default()
        };
        assert_eq!(classify_gortex_daemon_repo_status(&missing), "missing");

        let loading = GortexDaemonTrackedRepo {
            loading: true,
            unloaded: true,
            ..Default::default()
        };
        assert_eq!(classify_gortex_daemon_repo_status(&loading), "indexing");

        let unloaded = GortexDaemonTrackedRepo {
            unloaded: true,
            index_degraded: true,
            ..Default::default()
        };
        assert_eq!(classify_gortex_daemon_repo_status(&unloaded), "unindexed");

        let degraded_flag = GortexDaemonTrackedRepo {
            index_degraded: true,
            files: 12,
            ..Default::default()
        };
        assert_eq!(classify_gortex_daemon_repo_status(&degraded_flag), "degraded");

        let degraded_err = GortexDaemonTrackedRepo {
            index_health_error: "parse error".to_string(),
            failed_files: 1,
            files: 10,
            ..Default::default()
        };
        assert_eq!(classify_gortex_daemon_repo_status(&degraded_err), "degraded");

        let empty = GortexDaemonTrackedRepo {
            files: 0,
            last_index_unix: 1700000000,
            ..Default::default()
        };
        assert_eq!(classify_gortex_daemon_repo_status(&empty), "empty");

        let ready = GortexDaemonTrackedRepo {
            files: 42,
            last_index_unix: 1700000000,
            ..Default::default()
        };
        assert_eq!(classify_gortex_daemon_repo_status(&ready), "ready");
    }
}

