use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_listen_address")]
    pub listen_address: String,
    #[serde(default = "default_upstream_base_url")]
    pub upstream_base_url: String,
    #[serde(default = "default_upstream_api_key")]
    pub upstream_api_key: String,
    #[serde(default)]
    pub outbound_proxy: String,
    #[serde(default)]
    pub llmtrim_path: String,
    #[serde(default)]
    pub local_api_key: String,
    #[serde(default = "default_true")]
    pub upstream_websocket_enabled: bool,
    #[serde(default)]
    pub startup_enabled: bool,
    #[serde(default)]
    pub background_start: bool,
    #[serde(default)]
    pub retry_enabled: bool,
    #[serde(default = "default_retry_count")]
    pub retry_count: String,
    #[serde(default = "default_retry_interval")]
    pub retry_interval_seconds: String,
    #[serde(default = "default_retry_status_codes")]
    pub retry_status_codes: String,
}

fn default_listen_address() -> String {
    "127.0.0.1:7780".to_string()
}

fn default_upstream_base_url() -> String {
    "https://your-api.example.com".to_string()
}

fn default_upstream_api_key() -> String {
    "PUT_YOUR_UPSTREAM_API_KEY_HERE".to_string()
}

fn default_true() -> bool {
    true
}

fn default_retry_count() -> String {
    "5".to_string()
}

fn default_retry_interval() -> String {
    "1".to_string()
}

fn default_retry_status_codes() -> String {
    "100-199,300-399,401-407,409-499,500-503,505-523,525-599".to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            listen_address: default_listen_address(),
            upstream_base_url: default_upstream_base_url(),
            upstream_api_key: default_upstream_api_key(),
            outbound_proxy: String::new(),
            llmtrim_path: String::new(),
            local_api_key: String::new(),
            upstream_websocket_enabled: true,
            startup_enabled: false,
            background_start: false,
            retry_enabled: false,
            retry_count: default_retry_count(),
            retry_interval_seconds: default_retry_interval(),
            retry_status_codes: default_retry_status_codes(),
        }
    }
}

impl AppConfig {
    pub fn load_or_default<P: AsRef<Path>>(path: P) -> Self {
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(cfg) = serde_yaml::from_str::<AppConfig>(&content) {
                return cfg;
            }
        }
        Self::default()
    }

    pub fn save<P: AsRef<Path>>(&self, path: P) -> std::io::Result<()> {
        let serialized = serde_yaml::to_string(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        std::fs::write(path, serialized)
    }
}
