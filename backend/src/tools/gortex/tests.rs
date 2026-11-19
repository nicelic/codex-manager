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
    use std::path::{Path, PathBuf};

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

        // 显式指定分片大小与默认分片回退（chunk_size == 0 时回退至 256 行）
        assert_eq!(calculate_prompt_slices(0, 256), vec!["L1-1"]);
        assert_eq!(calculate_prompt_slices(256, 256), vec!["L1-256"]);
        assert_eq!(calculate_prompt_slices(356, 256), vec!["L1-256", "L257-356"]);
        assert_eq!(calculate_prompt_slices(500, 400), vec!["L1-400", "L401-500"]);
        assert_eq!(
            calculate_prompt_slices(1000, 400),
            vec!["L1-400", "L401-800", "L801-1000"]
        );
        assert_eq!(
            calculate_prompt_slices(2429, 400),
            vec![
                "L1-400",
                "L401-800",
                "L801-1200",
                "L1201-1600",
                "L1601-2000",
                "L2001-2400",
                "L2401-2429"
            ]
        );
        assert_eq!(
            calculate_prompt_slices(1000, 0),
            vec!["L1-256", "L257-512", "L513-768", "L769-1000"]
        );
        assert_eq!(calculate_prompt_slices(500, 300), vec!["L1-300", "L301-500"]);
    }

    #[test]
    fn test_gortex_delegated_prompt_body_generation() {
        use crate::tools::gortex::integrations::gortex_delegated_prompt_body;
        use std::path::Path;

        let dummy_path = Path::new("C:/test/gortex提示词.md");
        let prompt = gortex_delegated_prompt_body(dummy_path);
        let (expected_lines, _) = crate::tools::gortex::integrations::gortex_prompt_file_stats(dummy_path);
        let expected_slices = crate::tools::gortex::integrations::calculate_prompt_slices(
            expected_lines,
            crate::tools::gortex::integrations::DEFAULT_PROMPT_CHUNK_LINES,
        );
        let expected_line_str = if expected_slices.len() <= 1 {
            format!("共 {} 行", expected_lines)
        } else {
            format!("共计 {} 行", expected_lines)
        };

        assert!(prompt.contains("为方便接下来工作的展开，你必须先全量读取"));
        assert!(prompt.contains("- 文档路径：`C:/test/gortex提示词.md`"));
        assert!(!prompt.contains("C:\\test\\gortex提示词.md"));
        assert!(prompt.contains(&expected_line_str));
        assert!(prompt.contains(&format!("- 文档规模：{}", expected_line_str)));

        let escape_prone_path = Path::new(r"C:\tools\new\release\users\gortex提示词.md");
        let escape_prone_prompt = gortex_delegated_prompt_body(escape_prone_path);
        assert!(escape_prone_prompt.contains("- 文档路径：`C:/tools/new/release/users/gortex提示词.md`"));
        assert!(!escape_prone_prompt.contains(r"C:\tools"));
        let expected_last_slice = expected_slices.last().unwrap();
        assert!(prompt.contains("初始计划为"));
        assert!(prompt.contains(expected_last_slice));
        assert!(prompt.contains(&expected_slices.join("、")));
        assert!(prompt.contains("外部文档只读豁免"));
        assert!(prompt.contains("宿主原生只读工具"));
        assert!(prompt.contains("严禁调用 Gortex MCP 读取该文件"));
        assert!(prompt.contains("顺序分片读取与完整性验收"));
        assert!(prompt.contains("默认按 256 行分片"));
        assert!(!prompt.contains("默认按 400 行分片"));
        assert!(!prompt.contains("默认按 300 行分片"));
        assert!(!prompt.contains("默认按 236 行分片"));
        assert!(prompt.contains("必须等待当前调用返回、验收并补全后"));
        assert!(prompt.contains("禁止并发、批量下发"));
        assert!(prompt.contains("调用尚未返回时必须等待，不得重发"));
        assert!(prompt.contains("仅补读未验收范围"));
        assert!(prompt.contains("若文档在读取过程中变化"));
        assert!(!prompt.contains("可在首轮同时发起各切片的独立读取"));
        assert!(prompt.contains(&format!("L1 至 L{}", expected_lines)));
        assert!(prompt.contains("后续代码闭环契约"));
        assert!(prompt.contains("使用原生工具"));
        assert!(prompt.contains("全程编码规范"));
        assert!(prompt.contains("UTF-8"));
        assert!(prompt.contains("从开始读取“gortex提示词.md”就必须使用 UTF-8"));
        assert!(prompt.contains("Git、GitHub 与测试执行调试豁免"));
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

        // 验证多切片分支按 256 行分片
        let temp_multi = std::env::temp_dir().join(format!(
            "gortex_multi_prompt_{}.md",
            std::process::id()
        ));
        let multi_content = (1..=450).map(|i| format!("line {}", i)).collect::<Vec<_>>().join("\n");
        std::fs::write(&temp_multi, multi_content).expect("write temp multi prompt");
        let multi_prompt = gortex_delegated_prompt_body(&temp_multi);
        let _ = std::fs::remove_file(&temp_multi);

        assert!(multi_prompt.contains("共计 450 行"));
        assert!(multi_prompt.contains("默认按 256 行分片，初始计划为 L1-256、L257-450。"));

        for generated in [&prompt, &small_prompt] {
            for required in [
                "（如环境排查、外部构建与测试等）",
                "全程从开始到结束必须使用 UTF-8",
                "从开始读取“gortex提示词.md”就必须使用 UTF-8",
                "所有操作仍须遵守更高优先级的系统指令和平台权限",
                "测试进程的标准输出",
                "无论测试与业务代码写在同一文件中",
                "Rust 源文件内的 #[cfg(test)] 模块",
                "pytest、npm test 等原生命令",
                "Gortex 的 change.tests 等功能",
                "不得仅因识别不完整而自动改用原生工具读写工程代码",
                "若任务仅运行或调试已知测试",
            ] {
                assert!(generated.contains(required), "prompt missing: {}", required);
            }
        }
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

    #[test]
    fn test_gortex_bridge_mcp_entry_rewired() {
        use crate::tools::gortex::integrations::{
            gortex_bridge_mcp_entry, gortex_codex_mcp_entry, gortex_opencode_mcp_entry,
            gortex_platform_mcp_entry, update_codex_mcp_config_owned,
        };
        use crate::tools::gortex::ownership::{
            gortex_mcp_entry_looks_managed, gortex_opencode_entry_looks_managed,
        };
        use crate::tools::gortex::project_mcp::gortex_project_mcp_entry;

        let install_root = PathBuf::from(r"C:\TestGortex");
        let exe = r"C:\TestGortex\bin\gortex.exe";

        let ag = gortex_platform_mcp_entry("antigravity", exe, &install_root);
        let ag_args = ag["args"].as_array().unwrap();
        assert_eq!(ag_args[0], "gortex-bridge");
        assert!(ag_args.contains(&serde_json::json!("--gortex")));
        assert!(ag_args.contains(&serde_json::json!("antigravity")));
        assert!(gortex_mcp_entry_looks_managed(&ag));

        let codex = gortex_codex_mcp_entry(exe, &install_root);
        assert_eq!(codex["args"].as_array().unwrap()[0], "gortex-bridge");
        assert_eq!(codex["startup_timeout_sec"], 90);
        assert!(gortex_mcp_entry_looks_managed(&codex));

        let opencode = gortex_opencode_mcp_entry(exe, &install_root);
        let oc_cmd = opencode["command"].as_array().unwrap();
        assert_eq!(oc_cmd[1], "gortex-bridge");
        assert!(gortex_opencode_entry_looks_managed(&opencode));

        let proj_codex = gortex_project_mcp_entry("codex", exe, r"D:\repo\app", &install_root);
        let proj_args = proj_codex["args"].as_array().unwrap();
        assert_eq!(proj_args[0], "gortex-bridge");
        assert!(proj_args.contains(&serde_json::json!("--cwd")));
        assert!(proj_args.contains(&serde_json::json!(r"D:\repo\app")));
        assert_eq!(proj_codex["cwd"], r"D:\repo\app");
        assert!(gortex_mcp_entry_looks_managed(&proj_codex));

        // Check legacy direct "mcp" entries still count as managed during migration
        let legacy = serde_json::json!({
            "command": r"C:\TestGortex\bin\gortex.exe",
            "args": ["mcp"]
        });
        assert!(gortex_mcp_entry_looks_managed(&legacy));

        let _ = gortex_bridge_mcp_entry("cursor", exe, &install_root);
        let _ = update_codex_mcp_config_owned as fn(&str, &Path, bool) -> Result<bool, String>;
    }

    #[test]
    fn test_antigravity_project_mcp_disabled_and_legacy_cleanup() {
        use crate::tools::gortex::project_mcp::{
            cleanup_legacy_project_antigravity_mcp, gortex_project_mcp_config_path,
            gortex_register_project_mcp_for_single_agent,
        };
        use std::collections::BTreeMap;

        let base = std::env::temp_dir().join(format!(
            "gortex_ag_proj_cleanup_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let project_dir = base.join("sample_project");
        let install_root = base.join("install");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::create_dir_all(&install_root).unwrap();

        assert_eq!(
            gortex_project_mcp_config_path("antigravity", &project_dir.to_string_lossy()),
            None
        );

        // Seed a legacy `.agents/mcp_config.json` containing gortex + another user server
        let legacy_dir = project_dir.join(".agents");
        std::fs::create_dir_all(&legacy_dir).unwrap();
        let legacy_file = legacy_dir.join("mcp_config.json");
        std::fs::write(
            &legacy_file,
            serde_json::to_string_pretty(&serde_json::json!({
                "mcpServers": {
                    "gortex": {
                        "command": r"C:\Old\gortex.exe",
                        "args": ["mcp"]
                    },
                    "custom-other": {
                        "command": "node",
                        "args": ["server.js"]
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let mut available = BTreeMap::new();
        available.insert("antigravity".to_string(), true);

        let warnings = gortex_register_project_mcp_for_single_agent(
            &project_dir.to_string_lossy(),
            "antigravity",
            r"C:\Old\gortex.exe",
            &install_root,
            Some(&available),
        );
        assert!(warnings.is_empty(), "unexpected warnings: {:?}", warnings);

        let after: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&legacy_file).unwrap()).unwrap();
        assert!(after["mcpServers"].get("gortex").is_none());
        assert!(after["mcpServers"].get("custom-other").is_some());

        // Now test complete removal when gortex was the only entry
        std::fs::write(
            &legacy_file,
            serde_json::to_string_pretty(&serde_json::json!({
                "mcpServers": {
                    "gortex": {
                        "command": r"C:\Old\gortex.exe",
                        "args": ["mcp"]
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();
        assert!(cleanup_legacy_project_antigravity_mcp(
            &project_dir.to_string_lossy(),
            &install_root
        )
        .unwrap());
        assert!(!legacy_file.exists());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn test_ensure_prompt_document_updates_stale_file() {
        use crate::tools::gortex::integrations::{normalize_gortex_line_endings, GORTEX_PROMPT_DOC};
        use crate::tools::gortex::service::GortexService;

        let base = std::env::temp_dir().join(format!(
            "gortex_prompt_sync_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        let prompt_path = base.join("gortex提示词.md");
        std::fs::write(&prompt_path, "stale v300 prompt content\n").unwrap();

        let out_path = GortexService::ensure_prompt_document(&base);
        let updated = std::fs::read_to_string(&out_path).unwrap();
        assert_eq!(
            normalize_gortex_line_endings(&updated),
            normalize_gortex_line_endings(GORTEX_PROMPT_DOC)
        );
        assert!(updated.contains("v703"));
        assert!(updated.contains("unbound_global"));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn test_bridge_cwd_classification_no_first_repo_fallback() {
        use crate::tools::gortex::bridge::{
            collect_candidate_cwd, is_ambiguous_or_host_directory, is_cwd_covered_or_contained,
            is_host_program_directory, parse_bridge_args, path_is_same_or_inside,
        };

        let parsed = parse_bridge_args(&[
            "--gortex".to_string(),
            r"C:\Gortex\bin\gortex.exe".to_string(),
            "--agent".to_string(),
            "antigravity".to_string(),
        ]);
        assert_eq!(parsed.agent, "antigravity");
        assert_eq!(parsed.cwd, "");

        assert!(is_host_program_directory(Path::new(
            r"C:\Users\Admin\AppData\Local\Programs\antigravity"
        )));
        assert!(is_host_program_directory(Path::new(
            r"C:\Users\Admin\.codex"
        )));
        assert!(is_ambiguous_or_host_directory(Path::new(r"C:\")));

        let root = std::env::temp_dir().join(format!(
            "gortex_bridge_cwd_test_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let repo = root.join("kwor-rust");
        let repo_sub = repo.join("src");
        let sibling = root.join("kwor-rust2");
        std::fs::create_dir_all(&repo_sub).unwrap();
        std::fs::create_dir_all(&sibling).unwrap();

        assert!(path_is_same_or_inside(&repo_sub, &repo));
        assert!(!path_is_same_or_inside(&sibling, &repo));

        let tracked = vec![repo.to_string_lossy().to_string()];
        assert!(is_cwd_covered_or_contained(&repo, &tracked));
        assert!(is_cwd_covered_or_contained(&repo_sub, &tracked));
        // Parent folder containing tracked repos (ContainedReposScope)
        assert!(is_cwd_covered_or_contained(&root, &tracked));
        // Unrelated sibling must NOT match or fall back to tracked[0]
        assert!(!is_cwd_covered_or_contained(&sibling, &tracked));

        // Explicit missing path must NOT fall back to tracked repos
        let missing_dir = root.join("nonexistent-dir");
        assert!(!is_ambiguous_or_host_directory(&repo));
        assert!(is_ambiguous_or_host_directory(&missing_dir));

        let _ = collect_candidate_cwd as fn(&str) -> Option<PathBuf>;
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_bridge_unbound_enrichment_and_config_yaml() {
        use crate::tools::gortex::bridge::{
            classify_intercept_request, load_config_yaml_inventory,
            rewrite_unbound_jsonrpc_response, InterceptKind, InventoryRepo, TrackedInventory,
        };

        let base = std::env::temp_dir().join(format!(
            "gortex_bridge_cfg_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        let cfg = base.join("config.yaml");
        std::fs::write(
            &cfg,
            r#"
active_project: stack
repos:
  - path: C:\EXEXX\edit-rust
    name: edit-rust
    workspace: default
    project: edit-rust
  - path: D:\kwor-rust\kwor-rust
    name: kwor-rust
    workspace: default
    project: kwor-rust
projects:
  stack:
    repos:
      - path: C:\EXEXX\edit-rust
        name: edit-rust
"#,
        )
        .unwrap();

        let inv = load_config_yaml_inventory(&cfg);
        assert_eq!(inv.active_project, "stack");
        assert_eq!(inv.repos.len(), 2);
        assert_eq!(inv.workspaces, vec!["default".to_string()]);
        assert_eq!(
            inv.projects.get("stack"),
            Some(&vec!["edit-rust".to_string()])
        );

        // Request classification checks
        let ws_repos_req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "tools/call",
            "params": { "name": "workspace", "arguments": { "operation": "repos", "arguments": {} } }
        });
        assert_eq!(
            classify_intercept_request(&ws_repos_req),
            Some(("7".to_string(), InterceptKind::WorkspaceRepos))
        );

        let res_ws_req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "res-1",
            "method": "resources/read",
            "params": { "uri": "gortex://workspace" }
        });
        assert_eq!(
            classify_intercept_request(&res_ws_req),
            Some(("\"res-1\"".to_string(), InterceptKind::ResourceWorkspace))
        );

        // 1. JSON unbound response gets enriched
        let unbound_tool_resp = serde_json::to_string(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 7,
            "result": {
                "content": [{ "type": "text", "text": r#"{"mode":"unbound","repos":[]}"# }]
            }
        }))
        .unwrap();
        let rewritten = rewrite_unbound_jsonrpc_response(
            &unbound_tool_resp,
            InterceptKind::WorkspaceRepos,
            None,
            || inv.clone(),
        )
        .expect("should rewrite unbound response");
        let parsed: serde_json::Value = serde_json::from_str(&rewritten).unwrap();
        let inner: serde_json::Value =
            serde_json::from_str(parsed["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(inner["mode"], "unbound_global");
        assert_eq!(inner["global_scope"], true);
        assert_eq!(inner["repos"].as_array().unwrap().len(), 2);
        assert_eq!(inner["members"].as_array().unwrap().len(), 2);

        // 2. TOON unbound response gets enriched cleanly too
        let toon_resp = serde_json::to_string(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 8,
            "result": {
                "content": [{ "type": "text", "text": "mode: unbound\nrepos[0]:\n" }]
            }
        }))
        .unwrap();
        let rewritten_toon = rewrite_unbound_jsonrpc_response(
            &toon_resp,
            InterceptKind::WorkspaceInfo,
            Some(r"D:\untracked\scratch"),
            || TrackedInventory {
                repos: vec![InventoryRepo {
                    name: "kwor-rust".to_string(),
                    path: r"D:\kwor-rust\kwor-rust".to_string(),
                    workspace: Some("default".to_string()),
                    project: Some("kwor-rust".to_string()),
                    state: Some("ready".to_string()),
                }],
                projects: Default::default(),
                active_project: String::new(),
                workspaces: vec!["default".to_string()],
            },
        )
        .expect("should rewrite TOON unbound");
        let parsed_toon: serde_json::Value = serde_json::from_str(&rewritten_toon).unwrap();
        let inner_toon: serde_json::Value =
            serde_json::from_str(parsed_toon["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap();
        assert_eq!(inner_toon["mode"], "unbound_global");
        assert_eq!(inner_toon["host_cwd"], r"D:\untracked\scratch");
        assert_eq!(inner_toon["members"][0]["name"], "kwor-rust");

        // 3. Unresolved workspace resource gets enriched
        let res_ws_resp = serde_json::to_string(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": "res-1",
            "result": {
                "contents": [{
                    "uri": "gortex://workspace",
                    "mimeType": "application/json",
                    "text": r#"{"mode":"workspace","repos":[],"workspace":"\u0000gortex-unresolved-workspace:C:\\Programs\\antigravity"}"#
                }]
            }
        }))
        .unwrap();
        let rewritten_res = rewrite_unbound_jsonrpc_response(
            &res_ws_resp,
            InterceptKind::ResourceWorkspace,
            None,
            || inv.clone(),
        )
        .expect("should rewrite unresolved resource");
        let parsed_res: serde_json::Value = serde_json::from_str(&rewritten_res).unwrap();
        let inner_res: serde_json::Value =
            serde_json::from_str(parsed_res["result"]["contents"][0]["text"].as_str().unwrap())
                .unwrap();
        assert_eq!(inner_res["mode"], "unbound_global");
        assert_eq!(inner_res["repos"].as_array().unwrap().len(), 2);

        // 4. Real bound workspace with repos MUST pass through untouched
        let bound_resp = serde_json::to_string(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 9,
            "result": {
                "content": [{ "type": "text", "text": r#"{"mode":"workspace","workspace":"default","repos":["kwor-rust"]}"# }]
            }
        }))
        .unwrap();
        assert!(rewrite_unbound_jsonrpc_response(
            &bound_resp,
            InterceptKind::WorkspaceRepos,
            None,
            || inv.clone()
        )
        .is_none());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_bridge_win_af_unix_probe_loopback() {
        use crate::tools::gortex::bridge::query_daemon_probe;
        use windows_sys::Win32::Networking::WinSock::{
            accept, bind, closesocket, listen, recv, send, socket, WSAStartup, AF_UNIX,
            INVALID_SOCKET, SOCKADDR_UN, SOCKET_ERROR, SOCK_STREAM, WSADATA,
        };

        let sock_path = std::env::temp_dir().join(format!(
            "gortex_bridge_afunix_{}.sock",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&sock_path);
        let path_str = sock_path.to_string_lossy().to_string();
        let path_bytes = path_str.as_bytes();
        assert!(path_bytes.len() < 108);

        unsafe {
            let mut wsa: WSADATA = std::mem::zeroed();
            assert_eq!(WSAStartup(0x0202, &mut wsa), 0);

            let listener = socket(AF_UNIX as i32, SOCK_STREAM as i32, 0);
            assert_ne!(listener, INVALID_SOCKET);

            let mut addr: SOCKADDR_UN = std::mem::zeroed();
            addr.sun_family = AF_UNIX;
            for (i, &b) in path_bytes.iter().enumerate() {
                addr.sun_path[i] = b as _;
            }
            assert_ne!(
                bind(
                    listener,
                    (&addr as *const SOCKADDR_UN).cast(),
                    std::mem::size_of::<SOCKADDR_UN>() as i32,
                ),
                SOCKET_ERROR
            );
            assert_ne!(listen(listener, 1), SOCKET_ERROR);

            let server_handle = std::thread::spawn(move || {
                let client = accept(listener, std::ptr::null_mut(), std::ptr::null_mut());
                assert_ne!(client, INVALID_SOCKET);

                let mut buf = [0u8; 4096];
                let mut acc = Vec::new();
                loop {
                    let n = recv(client, buf.as_mut_ptr(), buf.len() as i32, 0);
                    assert!(n > 0);
                    acc.extend_from_slice(&buf[..n as usize]);
                    if acc.contains(&b'\n') {
                        break;
                    }
                }
                let ack = b"{\"ok\":true,\"session_id\":\"s-test\"}\n";
                assert!(send(client, ack.as_ptr(), ack.len() as i32, 0) > 0);

                acc.clear();
                loop {
                    let n = recv(client, buf.as_mut_ptr(), buf.len() as i32, 0);
                    assert!(n > 0);
                    acc.extend_from_slice(&buf[..n as usize]);
                    if acc.contains(&b'\n') {
                        break;
                    }
                }
                let probe_resp = b"{\"ok\":true,\"result\":{\"version\":\"v0.64.11\",\"tracked_repos\":[{\"path\":\"D:\\\\kwor-rust\\\\kwor-rust\",\"prefix\":\"kwor-rust\",\"workspace\":\"default\",\"state\":\"ready\"}]}}\n";
                assert!(send(client, probe_resp.as_ptr(), probe_resp.len() as i32, 0) > 0);

                let _ = closesocket(client);
                let _ = closesocket(listener);
            });

            let probe = query_daemon_probe(&sock_path).expect("AF_UNIX probe must succeed");
            assert_eq!(probe.tracked_repos.len(), 1);
            assert_eq!(probe.tracked_repos[0].prefix, "kwor-rust");
            assert_eq!(probe.tracked_repos[0].workspace, "default");
            assert_eq!(probe.tracked_repos[0].state, "ready");

            server_handle.join().unwrap();
        }

        let _ = std::fs::remove_file(&sock_path);
    }
}

