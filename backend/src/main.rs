#![windows_subsystem = "windows"]

pub mod api;
pub mod common;
pub mod router;
pub mod server;
pub mod state;
pub mod tools;
pub mod tray;
pub mod web;

use common::AppConfig;
use state::AppState;
use std::net::SocketAddr;
use std::path::Path;
use tokio::net::TcpListener;
use tracing::{error, info, warn};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let log_path = common::config::runtime_config_dir().join("code-Manager-rust.log");
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    if let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(true)
        .open(&log_path)
    {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "info,edit_rust=debug".into()),
            )
            .with_writer(std::sync::Mutex::new(file))
            .with_ansi(false)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "info,edit_rust=debug".into()),
            )
            .with_ansi(false)
            .init();
    }

    let config_path = common::config::default_config_path();
    if let Some(parent) = config_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let config = if Path::new(&config_path).exists() {
        AppConfig::load_or_default(&config_path)
    } else {
        let default_cfg = AppConfig::default();
        let _ = default_cfg.save(&config_path);
        default_cfg
    };

    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "gortex-bridge") {
        crate::tools::gortex::bridge::run_gortex_bridge(&args);
    }
    let is_background = args.iter().any(|a| a == "--background" || a == "--startup");

    let _single_instance = match common::windows::acquire_single_instance(common::windows::SINGLE_INSTANCE_MUTEX) {
        Ok(guard) => guard,
        Err(err) => {
            warn!("{}", err);
            eprintln!("{}", err);
            // 遵循单实例语义：如果已经在运行，手动双击启动时自动唤起已有实例的网页
            if !is_background {
                let launch_url = "http://127.0.0.1:7788".to_string();
                let _ = open::that(&launch_url);
            }
            // 即使旧版进程仍在运行，也让新 EXE 修复已有快捷方式的图标资源
            common::windows::refresh_shortcut_icons();
            return Ok(());
        }
    };

    let state = AppState::new(config.clone(), config_path.clone());
    let mut shutdown_rx = state.shutdown_sender.subscribe();

    // 网页服务默认以 7788 端口启动；若被占用则按 7788 -> 7789 -> 7790 ... 逐个 +1 尝试绑定
    let host_ip = config
        .listen_address
        .parse::<SocketAddr>()
        .map(|a| a.ip())
        .unwrap_or_else(|_| "0.0.0.0".parse().unwrap());

    let base_web_port: u16 = 7788;
    let mut listener = None;
    let mut actual_addr = None;
    let mut actual_port = base_web_port;

    for offset in 0..100 {
        let test_port = base_web_port + offset;
        let addr = SocketAddr::new(host_ip, test_port);
        match TcpListener::bind(addr).await {
            Ok(l) => {
                let local_addr = l.local_addr().unwrap_or(addr);
                actual_port = local_addr.port();
                actual_addr = Some(local_addr);
                listener = Some(l);
                if offset > 0 {
                    warn!("网页服务默认端口 {} 被占用，已自动递增至端口 {} 启动", base_web_port, actual_port);
                }
                break;
            }
            Err(_) => {
                // 端口被占用，继续尝试下一个端口
            }
        }
    }

    let (listener, addr) = match listener {
        Some(l) => (l, actual_addr.unwrap()),
        None => {
            let l = TcpListener::bind(SocketAddr::new(host_ip, 0)).await?;
            let local_addr = l.local_addr()?;
            actual_port = local_addr.port();
            warn!("连续 100 个端口均被占用，网页服务已由系统分配临时端口 {} 启动", actual_port);
            (l, local_addr)
        }
    };

    *state.management_address.write().await = addr.to_string();
    *state.web_port.write().await = actual_port;
    let browser_url = format!("http://127.0.0.1:{}", actual_port);
    *state.browser_url.write().await = browser_url.clone();

    info!("==================================================");
    info!("code-Manager-rust 服务已启动，本地监听: {}", addr);
    info!("本机管理控制台地址: {}", browser_url);
    info!("配置文件位于: {:?}", config_path);
    info!("前端网页已内嵌打包在可执行文件中");
    info!("==================================================");

    // 自动唤起默认系统浏览器（若非后台静默启动），永远使用 127.0.0.1
    if !is_background && !config.background_start {
        let launch_url = browser_url.clone();
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
            if let Err(e) = open::that(&launch_url) {
                error!("无法自动唤起系统浏览器: {}, 请手动访问: {}", e, launch_url);
            }
        });
    } else {
        info!("检测到后台静默运行模式，已跳过自动唤起浏览器。");
    }

    let app = router::create_router(state.clone());

    info!("正在启动 Windows 系统托盘与任务栏服务...");
    let tray_handle = tray::start_tray(state.shutdown_sender.clone(), browser_url.clone());
    info!("Windows 系统托盘与任务栏服务启动完成: is_some={}", tray_handle.is_some());

    // 后台异步刷新相关快捷方式图标指向当前可执行文件，并修复已激活工具的环境变量
    tokio::task::spawn_blocking(|| {
        common::windows::refresh_shortcut_icons();
        tools::rtk::RtkService::repair_installed_rtk_path();
    });

    // 启动 Gortex 双轨配置守护与 ModTime 协调器
    crate::tools::gortex::reconcile::start_gortex_watch_enforcer();

    info!("正在启动 HTTP 服务器并开始监听请求...");
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            tokio::select! {
                res = shutdown_rx.recv() => {
                    match res {
                        Ok(()) => info!("收到安全停机指令，正在清理资源并优雅退出..."),
                        Err(e) => warn!("shutdown_rx 异常: {:?}", e),
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    info!("收到 Ctrl+C 中断信号，正在清理资源并优雅退出...");
                }
            }

            // 清理系统托盘与任务栏图标
            if let Some(tray) = tray_handle {
                tray.close();
            }

            // 清理已打开的日志查看器控制台
            if let Some(pid) = *state.log_viewer_pid.read().await {
                crate::common::process::kill_process_by_pid(pid);
            }
            if let Some(pid) = *state.llmtrim_log_viewer_pid.read().await {
                crate::common::process::kill_process_by_pid(pid);
            }

            // 确保彻底停止并杀死所有 Gortex 进程（包括其他方式启动的）
            let _ = tokio::task::spawn_blocking(|| {
                info!("优雅退出：正在停止并杀死所有 Gortex 进程（包括其他方式启动的）...");
                let _ = crate::tools::gortex::service::GortexService::stop_daemon();
                let _ = crate::tools::gortex::service::GortexService::stop_all_processes(
                    std::time::Duration::from_secs(3),
                );
            }).await;
        })
        .await?;

    info!("code-Manager-rust 已安全退出。");
    Ok(())
}
