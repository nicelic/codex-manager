pub mod api;
pub mod common;
pub mod router;
pub mod server;
pub mod state;
pub mod tools;
pub mod web;

use common::AppConfig;
use state::AppState;
use std::net::SocketAddr;
use std::path::Path;
use tokio::net::TcpListener;
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,edit_rust=debug".into()),
        )
        .init();

    let config_path = "config.yaml";
    let config = if Path::new(config_path).exists() {
        AppConfig::load_or_default(config_path)
    } else {
        let default_cfg = AppConfig::default();
        let _ = default_cfg.save(config_path);
        default_cfg
    };

    let state = AppState::new(config, config_path.to_string());
    let mut shutdown_rx = state.shutdown_sender.subscribe();

    // 优先尝试 3000 端口，如果被占用则自动探测并分配空闲端口
    let preferred_ports = [3000, 7780, 8080];
    let mut listener = None;
    let mut actual_addr = None;

    for port in preferred_ports {
        let addr: SocketAddr = format!("127.0.0.1:{}", port).parse()?;
        if let Ok(l) = TcpListener::bind(addr).await {
            actual_addr = Some(addr);
            listener = Some(l);
            break;
        }
    }

    let (listener, addr) = match listener {
        Some(l) => (l, actual_addr.unwrap()),
        None => {
            let l = TcpListener::bind("127.0.0.1:0").await?;
            let local_addr = l.local_addr()?;
            (l, local_addr)
        }
    };

    let url = format!("http://{}", addr);
    info!("==================================================");
    info!("服务已启动，监听地址: {}", url);
    info!("前端网页已内嵌打包在可执行文件中");
    info!("==================================================");

    // 自动唤起默认系统浏览器
    let launch_url = url.clone();
    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
        if let Err(e) = open::that(&launch_url) {
            error!("无法自动唤起系统浏览器: {}, 请手动访问: {}", e, launch_url);
        }
    });

    let app = router::create_router(state);

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    info!("收到安全停机指令，正在优雅退出...");
                }
                _ = tokio::signal::ctrl_c() => {
                    info!("收到 Ctrl+C 中断信号，正在优雅退出...");
                }
            }
        })
        .await?;

    info!("code-Manager 已安全退出。");
    Ok(())
}
