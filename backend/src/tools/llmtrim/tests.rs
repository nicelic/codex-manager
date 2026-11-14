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
    assert_eq!(
        LlmtrimService::calculate_activation_state(&snap),
        "not_installed"
    );

    // 2. installed and stopped
    let snap = LlmtrimActivationSnapshot {
        installed: true,
        configured_path: r"C:\tools\llmtrim.exe".to_string(),
        configured_path_available: true,
        ..Default::default()
    };
    assert_eq!(
        LlmtrimService::calculate_activation_state(&snap),
        "installed_stopped"
    );

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
    assert_eq!(
        LlmtrimService::calculate_activation_state(&snap),
        "attention"
    );

    // 5. requested daemon did not recover
    let snap = LlmtrimActivationSnapshot {
        installed: true,
        configured_path: r"C:\tools\llmtrim.exe".to_string(),
        configured_path_available: true,
        desired_running: true,
        ..Default::default()
    };
    assert_eq!(
        LlmtrimService::calculate_activation_state(&snap),
        "attention"
    );

    // 6. tray left behind
    let snap = LlmtrimActivationSnapshot {
        installed: true,
        configured_path: r"C:\tools\llmtrim.exe".to_string(),
        configured_path_available: true,
        tray_running: true,
        ..Default::default()
    };
    assert_eq!(
        LlmtrimService::calculate_activation_state(&snap),
        "attention"
    );

    // 7. configured path is unavailable
    let snap = LlmtrimActivationSnapshot {
        installed: true,
        configured_path: r"C:\tools\llmtrim.exe".to_string(),
        configured_path_available: false,
        ..Default::default()
    };
    assert_eq!(
        LlmtrimService::calculate_activation_state(&snap),
        "attention"
    );

    // 8. orphaned install directory
    let snap = LlmtrimActivationSnapshot {
        directory_exists: true,
        ..Default::default()
    };
    assert_eq!(
        LlmtrimService::calculate_activation_state(&snap),
        "attention"
    );

    // 9. unavailable state ledger
    let snap = LlmtrimActivationSnapshot {
        state_read_failed: true,
        ..Default::default()
    };
    assert_eq!(
        LlmtrimService::calculate_activation_state(&snap),
        "attention"
    );
}

#[cfg(target_os = "windows")]
#[test]
fn listener_must_belong_to_the_selected_executable() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let executable = std::env::current_exe().unwrap();
    assert_eq!(
        super::service::listener_pid_for_executable(&executable, port),
        Some(std::process::id())
    );
    assert_eq!(
        super::service::listener_pid_for_executable(&executable.with_file_name("other.exe"), port),
        None
    );
}

#[cfg(target_os = "windows")]
#[test]
fn daemon_pid_must_match_the_recorded_executable() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory =
        std::env::temp_dir().join(format!("llmtrim-pid-test-{}-{}", std::process::id(), nonce));
    std::fs::create_dir(&directory).unwrap();
    let pid_file = directory.join("serve.pid");
    let executable = std::env::current_exe().unwrap();
    assert_eq!(
        super::service::recorded_daemon_pid(&directory, &executable),
        None
    );
    std::fs::write(&pid_file, format!("{{\"pid\":{}}}", std::process::id())).unwrap();
    assert_eq!(
        super::service::recorded_daemon_pid(&directory, &executable),
        Some(std::process::id())
    );
    assert_eq!(
        super::service::recorded_daemon_pid(&directory, &executable.with_file_name("other.exe")),
        None
    );
    std::fs::remove_file(pid_file).unwrap();
    std::fs::remove_dir(directory).unwrap();
}

#[test]
fn log_viewer_runs_the_original_status_command_in_its_install_directory() {
    let command = super::service::build_llmtrim_status_command();
    assert!(command.contains("title llmtrim status"));
    assert!(command.contains(".\\llmtrim.exe status"));
    assert!(command.contains("chcp 65001"));
}

#[test]
fn start_failures_include_the_child_process_error() {
    assert_eq!(
        super::service::llmtrim_start_arguments(),
        &["start", "--port", "43117"]
    );
    let error = super::service::run_llmtrim_start(Path::new("missing-llmtrim.exe")).unwrap_err();
    assert!(error.contains("启动 llmtrim start 失败"), "{error}");
}

#[test]
fn managed_config_sync_preserves_unowned_settings() {
    use super::config::{
        sync_managed_llmtrim_config_at, LLMTRIM_MARKER_CONTENT, LLMTRIM_MARKER_FILE,
    };

    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "llmtrim-config-test-{}-{}",
        std::process::id(),
        nonce
    ));
    std::fs::create_dir(&directory).unwrap();
    let config_path = directory.join("config.toml");
    let marker_path = directory.join(LLMTRIM_MARKER_FILE);
    let custom = "extra_hosts = [\"custom.example\"]\nexclude_hosts = [\"other.example\"]\n";
    std::fs::write(&config_path, custom).unwrap();

    sync_managed_llmtrim_config_at("https://api.example/v1", None, &directory).unwrap();
    assert_eq!(std::fs::read_to_string(&config_path).unwrap(), custom);
    assert!(!marker_path.exists());

    std::fs::write(&marker_path, "unrelated owner").unwrap();
    sync_managed_llmtrim_config_at("https://api.example/v1", None, &directory).unwrap();
    assert_eq!(std::fs::read_to_string(&config_path).unwrap(), custom);

    std::fs::write(&marker_path, LLMTRIM_MARKER_CONTENT).unwrap();
    sync_managed_llmtrim_config_at("https://api.example/v1", None, &directory).unwrap();
    assert!(std::fs::read_to_string(&config_path)
        .unwrap()
        .contains("api.example"));

    std::fs::remove_file(config_path).unwrap();
    std::fs::remove_file(marker_path).unwrap();
    std::fs::remove_dir(directory).unwrap();
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
    assert_eq!(parse_sha256_hash(content), None);

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
