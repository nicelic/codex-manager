#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    struct TestTempDir {
        path: PathBuf,
    }

    impl TestTempDir {
        fn new(prefix: &str) -> Self {
            let p = std::env::temp_dir().join(format!(
                "cm_test_{}_{}",
                prefix,
                chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
            ));
            let _ = fs::create_dir_all(&p);
            Self { path: p }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestTempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    use crate::tools::snip::codex::{
        codex_snip_hook_hash, codex_snip_hook_identities, codex_snip_hooks_trusted,
        parse_codex_cli_version, CodexCLIVersion,
    };
    use crate::tools::snip::hook_config::{
        is_snip_executable_name, read_snip_hook_json, remove_snip_owned_hook,
        snip_hook_command_matches_executable, snip_hook_command_targets_agent,
        snip_hook_locations_at, snip_owned_hook_presence_at,
        split_snip_hook_command, write_snip_hook_json,
        SnipOwnedHookPresence,
    };
    use crate::tools::snip::types::SnipHookOwnership;
    use crate::tools::snip::windows::update_path_entries;
    use serde_json::json;

    #[test]
    fn test_snip_hook_command_matches_official_windows_escaped_quote() {
        let executable = r#"C:/Program Files/code-Manager/Snip/snip.exe"#;
        let command = r#"\"C:/Program Files/code-Manager/Snip/snip.exe\" hook codex"#;
        assert!(
            snip_hook_command_targets_agent(command, "codex"),
            "official Windows Codex command was not recognized: {}",
            command
        );
        assert!(
            snip_hook_command_matches_executable(command, "codex", executable),
            "official Windows Codex command did not match executable: {}",
            command
        );
    }

    #[test]
    fn test_split_snip_hook_command_variants() {
        let cmd1 = r#"& "C:\Tools\Snip\snip.exe" hook copilot"#;
        let (exe1, args1) = split_snip_hook_command(cmd1).expect("should parse");
        assert_eq!(exe1, r#"C:\Tools\Snip\snip.exe"#);
        assert_eq!(args1, vec!["hook", "copilot"]);

        let cmd2 = r#"snip.exe hook"#;
        let (exe2, args2) = split_snip_hook_command(cmd2).expect("should parse");
        assert_eq!(exe2, "snip.exe");
        assert_eq!(args2, vec!["hook"]);

        assert!(is_snip_executable_name(&exe1));
        assert!(is_snip_executable_name(&exe2));
    }

    #[test]
    fn test_parse_codex_cli_version_and_minimum() {
        let ver1 = parse_codex_cli_version("codex-cli 0.131.0").expect("should parse");
        let min = CodexCLIVersion {
            major: 0,
            minor: 131,
            patch: 0,
        };
        assert!(ver1.at_least(min));

        let ver2 = parse_codex_cli_version("codex 0.130.9").expect("should parse");
        assert!(!ver2.at_least(min));

        let ver3 = parse_codex_cli_version("codex 0.132.5").expect("should parse");
        assert!(ver3.at_least(min));

        assert!(parse_codex_cli_version("codex latest").is_err());
    }

    #[test]
    fn test_codex_snip_hook_hash_exact_match_with_go_reference() {
        let command = r#"C:\Snip\snip.exe hook codex"#;
        let raw_json = json!({
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [
                            {
                                "type": "command",
                                "command": command
                            }
                        ]
                    }
                ]
            }
        });
        let data = serde_json::to_vec(&raw_json).unwrap();
        let identities = codex_snip_hook_identities(&data).expect("should parse identities");
        assert_eq!(identities.len(), 1);

        let hash = codex_snip_hook_hash(&identities[0]).expect("should compute hash");
        // Known golden hash from Go test
        let want = "sha256:c2a0c9869fdc0b6138dee7886cb29499f7f7c441a84f35419d76dae7ef213aa7";
        assert_eq!(hash, want, "Computed hash must match Go golden hash");

        let hook_path = PathBuf::from(r#"C:\Users\tester\.codex\hooks.json"#);
        let config_toml = format!(
            "[hooks.state.'{}:pre_tool_use:0:0']\ntrusted_hash = \"{}\"\n",
            hook_path.to_string_lossy(),
            hash
        );

        let (trusted, modified) =
            codex_snip_hooks_trusted(&config_toml, &hook_path, &identities).expect("trust check");
        assert!(trusted);
        assert!(!modified);

        // Modify identity
        let modified_json = json!({
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [
                            {
                                "type": "command",
                                "command": "D:\\Snip\\snip.exe hook codex"
                            }
                        ]
                    }
                ]
            }
        });
        let mod_data = serde_json::to_vec(&modified_json).unwrap();
        let mod_identities = codex_snip_hook_identities(&mod_data).expect("mod identities");
        let (mod_trusted, mod_modified) =
            codex_snip_hooks_trusted(&config_toml, &hook_path, &mod_identities).expect("trust check");
        assert!(!mod_trusted);
        assert!(mod_modified);
    }

    #[test]
    fn test_remove_snip_owned_hook_preserves_manual_hooks() {
        let temp = TestTempDir::new("snip_test");
        let hooks_file = temp.path().join("hooks.json");

        // Create hooks.json containing both a manual hook and a managed snip hook in separate groups
        let initial_json = json!({
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [
                            {
                                "type": "command",
                                "command": "manual_tool.exe arg"
                            }
                        ]
                    },
                    {
                        "matcher": "Bash",
                        "hooks": [
                            {
                                "type": "command",
                                "command": "C:\\Snip\\snip.exe hook codex"
                            }
                        ]
                    }
                ]
            }
        });

        write_snip_hook_json(&hooks_file, &initial_json).expect("write initial");

        let (locs, exists) = snip_hook_locations_at("codex", &hooks_file, |cmd| {
            cmd.contains("snip.exe")
        })
        .expect("locs");
        assert!(exists);
        assert_eq!(locs.len(), 1);

        let artifact = SnipHookOwnership {
            agent: "codex".to_string(),
            target_path: hooks_file.to_string_lossy().to_string(),
            event: "PreToolUse".to_string(),
            group_index: locs[0].group_index,
            handler_index: locs[0].handler_index,
            group_fingerprint: locs[0].group_fingerprint.clone(),
            fingerprint: locs[0].fingerprint.clone(),
            command: locs[0].command.clone(),
        };

        let presence = snip_owned_hook_presence_at(&artifact).expect("presence");
        assert_eq!(presence, SnipOwnedHookPresence::Exact);

        let removed = remove_snip_owned_hook(&artifact).expect("remove");
        assert!(removed);

        // Verify remaining content only has the manual hook
        let after = read_snip_hook_json(&hooks_file).expect("read").expect("exists");
        let groups = after["hooks"]["PreToolUse"].as_array().expect("array");
        assert_eq!(groups.len(), 1);
        assert_eq!(
            groups[0]["hooks"][0]["command"].as_str().unwrap(),
            "manual_tool.exe arg"
        );
    }

    #[test]
    fn test_remove_ungrouped_copilot_hook() {
        let temp = TestTempDir::new("snip_test");
        let hooks_file = temp.path().join("snip.json");

        let initial_json = json!({
            "hooks": {
                "preToolUse": [
                    {
                        "type": "command",
                        "bash": "manual_copilot.exe"
                    },
                    {
                        "type": "command",
                        "bash": "C:\\Snip\\snip.exe hook copilot"
                    }
                ]
            }
        });

        write_snip_hook_json(&hooks_file, &initial_json).expect("write initial");

        let (locs, _) = snip_hook_locations_at("copilot", &hooks_file, |cmd| {
            cmd.contains("snip.exe")
        })
        .expect("locs");
        assert_eq!(locs.len(), 1);

        let artifact = SnipHookOwnership {
            agent: "copilot".to_string(),
            target_path: hooks_file.to_string_lossy().to_string(),
            event: "preToolUse".to_string(),
            group_index: -1,
            handler_index: locs[0].handler_index,
            group_fingerprint: None,
            fingerprint: locs[0].fingerprint.clone(),
            command: locs[0].command.clone(),
        };

        let removed = remove_snip_owned_hook(&artifact).expect("remove");
        assert!(removed);

        let after = read_snip_hook_json(&hooks_file).expect("read").expect("exists");
        let handlers = after["hooks"]["preToolUse"].as_array().expect("array");
        assert_eq!(handlers.len(), 1);
        assert_eq!(handlers[0]["bash"].as_str().unwrap(), "manual_copilot.exe");
    }

    #[test]
    fn test_update_path_entries() {
        let initial = r#"C:\Windows\System32;C:\Tools"#;
        let entry = r#"C:\Snip"#;

        let added = update_path_entries(initial, entry, true);
        assert_eq!(added, r#"C:\Windows\System32;C:\Tools;C:\Snip"#);

        // Add already existing
        let added_again = update_path_entries(&added, entry, true);
        assert_eq!(added_again, added);

        // Remove
        let removed = update_path_entries(&added, entry, false);
        assert_eq!(removed, initial);
    }
}
