use std::sync::atomic::AtomicI64;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use crate::common::AppConfig;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<AppConfig>>,
    pub config_path: String,
    pub proxy_running: Arc<RwLock<bool>>,
    pub log_showing: Arc<RwLock<bool>>,
    pub llmtrim_log_showing: Arc<RwLock<bool>>,
    pub event_sender: broadcast::Sender<serde_json::Value>,
    pub shutdown_sender: broadcast::Sender<()>,
    pub active_requests: Arc<AtomicI64>,
    pub http_client: reqwest::Client,
}

impl AppState {
    pub fn new(config: AppConfig, config_path: String) -> Self {
        let (event_sender, _) = broadcast::channel(100);
        let (shutdown_sender, _) = broadcast::channel(1);

        let http_client = reqwest::Client::builder()
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .pool_max_idle_per_host(50)
            .build()
            .unwrap_or_default();

        Self {
            config: Arc::new(RwLock::new(config)),
            config_path,
            proxy_running: Arc::new(RwLock::new(false)),
            log_showing: Arc::new(RwLock::new(false)),
            llmtrim_log_showing: Arc::new(RwLock::new(false)),
            event_sender,
            shutdown_sender,
            active_requests: Arc::new(AtomicI64::new(0)),
            http_client,
        }
    }
}
