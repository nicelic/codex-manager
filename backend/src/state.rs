use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize};
use std::sync::Arc;
use tokio::sync::{broadcast, oneshot, watch, RwLock};
use crate::common::AppConfig;
use crate::tools::llmtrim::ownership::read_managed_tool_state;
use crate::tools::llmtrim::service::LlmtrimService;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LlmtrimRouteState {
    pub selected: bool,
    pub available: bool,
    pub persisted: bool,
    pub transitioning: bool,
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<AppConfig>>,
    pub config_path: PathBuf,
    pub proxy_running: Arc<RwLock<bool>>,
    pub proxy_state: Arc<RwLock<String>>,
    pub proxy_message: Arc<RwLock<String>>,
    pub proxy_address: Arc<RwLock<String>>,
    pub management_address: Arc<RwLock<String>>,
    pub web_port: Arc<RwLock<u16>>,
    pub browser_url: Arc<RwLock<String>>,
    pub proxy_dedicated_cancel: Arc<RwLock<Option<oneshot::Sender<()>>>>,
    pub log_viewer_pid: Arc<RwLock<Option<u32>>>,
    pub llmtrim_log_viewer_pid: Arc<RwLock<Option<u32>>>,
    pub llmtrim_client: Arc<RwLock<Option<(u64, reqwest::Client)>>>,
    pub llmtrim_http1_client: Arc<RwLock<Option<(u64, reqwest::Client)>>>,
    pub llmtrim_route: Arc<std::sync::Mutex<LlmtrimRouteState>>,
    pub llmtrim_operation_lock: Arc<tokio::sync::Mutex<()>>,
    pub proxy_route_changed: watch::Sender<u64>,
    pub proxy_route_monitor_started: Arc<AtomicBool>,
    pub event_sender: broadcast::Sender<serde_json::Value>,
    pub shutdown_sender: broadcast::Sender<()>,
    pub active_requests: Arc<AtomicI64>,
    pub active_ws_sessions: Arc<AtomicUsize>,
    pub upstream_h2_active: Arc<AtomicUsize>,
    pub upstream_ws_carriers: Arc<AtomicUsize>,
    pub http_client: reqwest::Client,
}

impl AppState {
    pub fn new(config: AppConfig, config_path: PathBuf) -> Self {
        let (event_sender, _) = broadcast::channel(100);
        let (shutdown_sender, _) = broadcast::channel(1);
        let (proxy_route_changed, _) = watch::channel(0);

        let http_client = reqwest::Client::builder()
            .no_proxy()
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .pool_max_idle_per_host(50)
            .build()
            .unwrap_or_default();

        let executable = LlmtrimService::get_executable_path(&config.llmtrim_path);
        let llmtrim_selected = executable.parent()
            .map(|directory| read_managed_tool_state(directory)
                .map(|state| state.desired_running)
                .unwrap_or(true))
            .unwrap_or(true);

        let initial_addr = config.listen_address.clone();
        let default_web_port = 7788u16;
        let default_browser_url = format!("http://127.0.0.1:{}", default_web_port);

        Self {
            config: Arc::new(RwLock::new(config)),
            config_path,
            proxy_running: Arc::new(RwLock::new(false)),
            proxy_state: Arc::new(RwLock::new("stopped".to_string())),
            proxy_message: Arc::new(RwLock::new("代理转发已停止，请在网页中点击“启动代理”。".to_string())),
            proxy_address: Arc::new(RwLock::new(initial_addr.clone())),
            management_address: Arc::new(RwLock::new(initial_addr)),
            web_port: Arc::new(RwLock::new(default_web_port)),
            browser_url: Arc::new(RwLock::new(default_browser_url)),
            proxy_dedicated_cancel: Arc::new(RwLock::new(None)),
            log_viewer_pid: Arc::new(RwLock::new(None)),
            llmtrim_log_viewer_pid: Arc::new(RwLock::new(None)),
            llmtrim_client: Arc::new(RwLock::new(None)),
            llmtrim_http1_client: Arc::new(RwLock::new(None)),
            llmtrim_route: Arc::new(std::sync::Mutex::new(LlmtrimRouteState {
                selected: llmtrim_selected,
                available: false,
                persisted: llmtrim_selected,
                transitioning: false,
            })),
            llmtrim_operation_lock: Arc::new(tokio::sync::Mutex::new(())),
            proxy_route_changed,
            proxy_route_monitor_started: Arc::new(AtomicBool::new(false)),
            event_sender,
            shutdown_sender,
            active_requests: Arc::new(AtomicI64::new(0)),
            active_ws_sessions: Arc::new(AtomicUsize::new(0)),
            upstream_h2_active: Arc::new(AtomicUsize::new(0)),
            upstream_ws_carriers: Arc::new(AtomicUsize::new(0)),
            http_client,
        }
    }
}
