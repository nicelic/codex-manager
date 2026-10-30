#[cfg(test)]
mod tests {
    use crate::tools::gortex::ownership::*;
    use crate::tools::gortex::reconcile::*;
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

        assert_eq!(calculate_prompt_slices(0, 700), vec!["L1-1"]);
        assert_eq!(calculate_prompt_slices(500, 700), vec!["L1-500"]);
        assert_eq!(calculate_prompt_slices(700, 700), vec!["L1-700"]);
        assert_eq!(
            calculate_prompt_slices(2429, 700),
            vec!["L1-700", "L701-1400", "L1401-2100", "L2101-2429"]
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
        assert!(prompt.contains("并发发起 4 个切片读取"));
        assert!(prompt.contains(&format!("L2101-{}", expected_lines)));
        assert!(prompt.contains("外部文档只读豁免"));
        assert!(prompt.contains("宿主原生只读工具"));
        assert!(prompt.contains("严禁调用 Gortex MCP 读取该外部文件"));
        assert!(prompt.contains("后续代码闭环契约"));
        assert!(prompt.contains("使用原生工具"));
        assert!(!prompt.contains("可免除预读此文档"));
    }
}

