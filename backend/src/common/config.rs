use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

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

pub const UPSTREAM_API_KEY_PLACEHOLDER: &str = "PUT_YOUR_UPSTREAM_API_KEY_HERE";

fn default_listen_address() -> String {
    "127.0.0.1:7788".to_string()
}

fn default_upstream_base_url() -> String {
    "https://your-api.example.com".to_string()
}

fn default_upstream_api_key() -> String {
    UPSTREAM_API_KEY_PLACEHOLDER.to_string()
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

pub fn runtime_config_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            return parent.join("config");
        }
    }
    PathBuf::from("config")
}

pub fn default_config_path() -> PathBuf {
    runtime_config_dir().join("config.yaml")
}

pub fn is_upstream_placeholder(key: &str) -> bool {
    let trimmed = key.trim();
    trimmed.is_empty() || trimmed.eq_ignore_ascii_case(UPSTREAM_API_KEY_PLACEHOLDER)
}

/// 规范化并解析状态码规则（对齐 Go 版 normalizeRetryStatusCodes）
/// 支持 "100-199,300-399,401-407,409-499,500-503,505-523,525-599" 或单个 "429"
/// 自动去除空格、过滤非数字/超界(100~599)无效范围、升序排序并合并重叠区间
pub fn normalize_retry_status_codes(raw: &str) -> (String, Vec<(u16, u16)>) {
    let cleaned: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
    if cleaned.is_empty() {
        return (String::new(), Vec::new());
    }

    let mut ranges: Vec<(u16, u16)> = Vec::new();
    for token in cleaned.split(',') {
        if token.is_empty() {
            continue;
        }
        let parts: Vec<&str> = token.split('-').collect();
        if parts.is_empty() || parts.len() > 2 {
            continue;
        }
        let start = match parts[0].parse::<u16>() {
            Ok(s) if (100..=599).contains(&s) => s,
            _ => continue,
        };
        let end = if parts.len() == 2 {
            match parts[1].parse::<u16>() {
                Ok(e) if (100..=599).contains(&e) && start <= e => e,
                _ => continue,
            }
        } else {
            start
        };
        ranges.push((start, end));
    }

    if ranges.is_empty() {
        return (String::new(), Vec::new());
    }

    // 排序
    ranges.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

    // 合并重叠或相邻区间
    let mut merged: Vec<(u16, u16)> = Vec::with_capacity(ranges.len());
    for curr in ranges {
        if let Some(prev) = merged.last_mut() {
            if curr.0 <= prev.1 + 1 {
                if curr.1 > prev.1 {
                    prev.1 = curr.1;
                }
                continue;
            }
        }
        merged.push(curr);
    }

    // 格式化输出规范化字符串
    let formatted = merged
        .iter()
        .map(|&(s, e)| {
            if s == e {
                s.to_string()
            } else {
                format!("{}-{}", s, e)
            }
        })
        .collect::<Vec<_>>()
        .join(",");

    (formatted, merged)
}

/// 严格校验监听地址（对齐 Go 版 validateListenAddress）
/// 要求必须为合法的 IPv4:端口（如 127.0.0.1:7788、0.0.0.0:7788）或 [IPv6]:端口（如 [::]:7788、[::1]:7788）
/// 端口必须在 1..=65535，禁止使用主机名（如 localhost:7788）
pub fn validate_listen_address(raw: &str) -> Result<SocketAddr, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("监听地址不能为空".to_string());
    }

    let socket_addr: SocketAddr = trimmed.parse().map_err(|_| {
        if trimmed.contains("localhost") {
            "监听地址不能使用主机名 localhost，请使用 127.0.0.1 或 0.0.0.0".to_string()
        } else if !trimmed.contains(':') {
            "监听地址缺少端口号，格式应为 IP:端口（例如 127.0.0.1:7788）".to_string()
        } else if trimmed.contains(':') && !trimmed.starts_with('[') && trimmed.matches(':').count() > 1 {
            "IPv6 地址必须使用 [IPv6]:端口 格式（例如 [::]:7788 或 [::1]:7788）".to_string()
        } else {
            "监听地址格式无效，必须为 IPv4:端口（如 127.0.0.1:7788）或 [IPv6]:端口（如 [::]:7788），端口范围 1-65535".to_string()
        }
    })?;

    if socket_addr.port() == 0 {
        return Err("监听地址端口号不能为 0，有效范围为 1-65535".to_string());
    }

    Ok(socket_addr)
}

impl AppConfig {
    pub fn load_or_default<P: AsRef<Path>>(path: P) -> Self {
        let p = path.as_ref();
        if let Ok(content) = std::fs::read_to_string(p) {
            if let Ok(cfg) = serde_yaml::from_str::<AppConfig>(&content) {
                return cfg;
            }
        }
        let default_cfg = Self::default();
        let _ = default_cfg.save(p);
        default_cfg
    }

    pub fn save<P: AsRef<Path>>(&self, path: P) -> std::io::Result<()> {
        let p = path.as_ref();
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let serialized = serde_yaml::to_string(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        std::fs::write(p, serialized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_placeholder_check() {
        assert!(is_upstream_placeholder("PUT_YOUR_UPSTREAM_API_KEY_HERE"));
        assert!(is_upstream_placeholder("  PUT_YOUR_UPSTREAM_API_KEY_HERE  "));
        assert!(is_upstream_placeholder(""));
        assert!(is_upstream_placeholder("   "));
        assert!(!is_upstream_placeholder("sk-real-api-key-123456"));
    }

    #[test]
    fn test_default_config() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.listen_address, "127.0.0.1:7788");
        assert_eq!(cfg.upstream_base_url, "https://your-api.example.com");
        assert!(is_upstream_placeholder(&cfg.upstream_api_key));
        assert!(cfg.upstream_websocket_enabled);
        assert!(!cfg.startup_enabled);
        assert!(!cfg.retry_enabled);
    }

    #[test]
    fn test_normalize_retry_status_codes() {
        let (formatted, ranges) = normalize_retry_status_codes("500-503, 502, 525-599, 429, abc, 700-800");
        assert_eq!(formatted, "429,500-503,525-599");
        assert_eq!(ranges, vec![(429, 429), (500, 503), (525, 599)]);

        let (formatted_adjacent, ranges_adjacent) = normalize_retry_status_codes("100-199, 200-299");
        assert_eq!(formatted_adjacent, "100-299");
        assert_eq!(ranges_adjacent, vec![(100, 299)]);

        let (empty_f, empty_r) = normalize_retry_status_codes("   ,invalid, 600-700");
        assert_eq!(empty_f, "");
        assert!(empty_r.is_empty());
    }

    #[test]
    fn test_validate_listen_address() {
        assert!(validate_listen_address("127.0.0.1:7788").is_ok());
        assert!(validate_listen_address("0.0.0.0:7788").is_ok());
        assert!(validate_listen_address("[::]:7788").is_ok());
        assert!(validate_listen_address("[::1]:7788").is_ok());
        assert!(validate_listen_address("  127.0.0.1:8080  ").is_ok());

        assert!(validate_listen_address("").is_err());
        assert!(validate_listen_address("   ").is_err());
        assert!(validate_listen_address("localhost:7788").is_err());
        assert!(validate_listen_address("127.0.0.1").is_err());
        assert!(validate_listen_address("127.0.0.1:0").is_err());
        assert!(validate_listen_address("127.0.0.1:99999").is_err());
        assert!(validate_listen_address("::1:7788").is_err());
    }
}
