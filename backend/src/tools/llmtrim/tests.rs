use super::config::{extract_upstream_host, format_managed_toml};
use super::release::parse_sha256_hash;
use super::service::LlmtrimService;
use super::types::LlmtrimActivationSnapshot;
use super::windows::is_llmtrim_proxy_value;
use std::path::Path;

#[test]
fn test_llmtrim_activation_state() {
    // 1. not installed
    let snap = LlmtrimActivationSnapshot::default();
    assert_eq!(LlmtrimService::calculate_activation_state(&snap), "not_installed");

    // 2. installed and stopped
    let snap = LlmtrimActivationSnapshot {
        installed: true,
        configured_path: r"C:\tools\llmtrim.exe".to_string(),
        configured_path_available: true,
        ..Default::default()
    };
    assert_eq!(LlmtrimService::calculate_activation_state(&snap), "installed_stopped");

    // 3. healthy daemon is running
    let snap = LlmtrimActivationSnapshot {
        installed: true,
        configured_path: r"C:\tools\llmtrim.exe".to_string(),
        configured_path_available: true,
        running: true,
        windows_configured: true,
        ..Default::default()
    };
    assert_eq!(LlmtrimService::calculate_activation_state(&snap), "running");

    // 4. running daemon without Windows setup needs attention
    let snap = LlmtrimActivationSnapshot {
        installed: true,
        configured_path: r"C:\tools\llmtrim.exe".to_string(),
        configured_path_available: true,
        running: true,
        windows_configured: false,
        ..Default::default()
    };
    assert_eq!(LlmtrimService::calculate_activation_state(&snap), "attention");

    // 5. requested daemon did not recover
    let snap = LlmtrimActivationSnapshot {
        installed: true,
        configured_path: r"C:\tools\llmtrim.exe".to_string(),
        configured_path_available: true,
        desired_running: true,
        ..Default::default()
    };
    assert_eq!(LlmtrimService::calculate_activation_state(&snap), "attention");

    // 6. tray left behind
    let snap = LlmtrimActivationSnapshot {
        installed: true,
        configured_path: r"C:\tools\llmtrim.exe".to_string(),
        configured_path_available: true,
        tray_running: true,
        ..Default::default()
    };
    assert_eq!(LlmtrimService::calculate_activation_state(&snap), "attention");

    // 7. configured path is unavailable
    let snap = LlmtrimActivationSnapshot {
        installed: true,
        configured_path: r"C:\tools\llmtrim.exe".to_string(),
        configured_path_available: false,
        ..Default::default()
    };
    assert_eq!(LlmtrimService::calculate_activation_state(&snap), "attention");

    // 8. orphaned install directory
    let snap = LlmtrimActivationSnapshot {
        directory_exists: true,
        ..Default::default()
    };
    assert_eq!(LlmtrimService::calculate_activation_state(&snap), "attention");

    // 9. unavailable state ledger
    let snap = LlmtrimActivationSnapshot {
        state_read_failed: true,
        ..Default::default()
    };
    assert_eq!(LlmtrimService::calculate_activation_state(&snap), "attention");
}

#[test]
fn test_extract_upstream_host() {
    assert_eq!(
        extract_upstream_host("https://Api.Example.test:32400/v1/").unwrap(),
        "api.example.test"
    );
    assert_eq!(
        extract_upstream_host("https://[2001:db8::1]:32400/v1").unwrap(),
        "2001:db8::1"
    );
    assert_eq!(
        extract_upstream_host("https://api.openai.com/v1").unwrap(),
        "api.openai.com"
    );
    assert!(extract_upstream_host("not-a-url").is_err());
    assert!(extract_upstream_host("").is_err());
}

#[test]
fn test_format_managed_toml() {
    let toml1 = format_managed_toml("api.example.com", None);
    assert_eq!(
        toml1,
        "# Managed by code-Manager.\nextra_hosts = [\"api.example.com\"]\n"
    );

    let toml2 = format_managed_toml(
        "api.example.com",
        Some(Path::new(r"C:\tools\llmtrim\tracking.db")),
    );
    assert_eq!(
        toml2,
        "# Managed by code-Manager.\nextra_hosts = [\"api.example.com\"]\ndb_path = \"C:/tools/llmtrim/tracking.db\"\n"
    );
}

#[test]
fn test_parse_sha256_hash() {
    let content = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  llmtrim-x86_64-pc-windows-msvc.zip\n";
    assert_eq!(
        parse_sha256_hash(content),
        Some("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string())
    );

    let upper = "E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855";
    assert_eq!(
        parse_sha256_hash(upper),
        Some("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string())
    );

    assert_eq!(parse_sha256_hash("invalid-hash"), None);
}

#[test]
fn test_is_llmtrim_proxy_value() {
    assert!(is_llmtrim_proxy_value("http://127.0.0.1:43117"));
    assert!(is_llmtrim_proxy_value("http://127.0.0.1:43117/"));
    assert!(is_llmtrim_proxy_value("HTTP://127.0.0.1:43117"));
    assert!(!is_llmtrim_proxy_value("http://127.0.0.1:7788"));
    assert!(!is_llmtrim_proxy_value("http://localhost:43117"));
}
