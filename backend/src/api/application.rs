use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use crate::common::downloader::{download_file, fetch_github_releases};
use crate::state::AppState;
use tracing::info;

const APP_REPO: &str = "nicelic/codex-manager";
const CURRENT_VERSION: &str = "v1.0.0";

#[derive(Debug, Deserialize)]
pub struct PageQuery {
    pub page: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateRequest {
    pub tag_name: Option<String>,
}

pub async fn identity() -> impl IntoResponse {
    Json(json!({
        "version": CURRENT_VERSION,
        "is_dev_mode": false,
        "name": "code-Manager-rust"
    }))
}

pub async fn releases(
    State(state): State<AppState>,
    Query(query): Query<PageQuery>,
) -> impl IntoResponse {
    let page = query.page.unwrap_or(1);
    let proxy = {
        let cfg = state.config.read().await;
        if cfg.outbound_proxy.is_empty() {
            None
        } else {
            Some(cfg.outbound_proxy.clone())
        }
    };

    let gh_releases = fetch_github_releases(APP_REPO, page, 5, proxy.as_deref())
        .await
        .unwrap_or_default();

    let mut release_options = Vec::new();
    let mut latest_version = CURRENT_VERSION.to_string();

    for (idx, r) in gh_releases.into_iter().enumerate() {
        if idx == 0 {
            latest_version = r.tag_name.clone();
        }
        let asset = r.assets.iter().find(|a| a.name.ends_with(".exe") || a.name.ends_with(".zip"));
        let (available, asset_name) = match asset {
            Some(a) => (true, a.name.clone()),
            None => (false, String::new()),
        };

        release_options.push(json!({
            "tag_name": r.tag_name,
            "name": r.name.unwrap_or_default(),
            "published_at": r.published_at.unwrap_or_default(),
            "prerelease": r.prerelease.unwrap_or(false),
            "available": available,
            "asset_name": asset_name
        }));
    }

    if release_options.is_empty() {
        release_options.push(json!({
            "tag_name": CURRENT_VERSION,
            "name": format!("{} 当前版本", CURRENT_VERSION),
            "published_at": "2026-09-18T00:00:00Z",
            "prerelease": false,
            "available": true,
            "asset_name": "code-Manager-rust.exe"
        }));
    }

    let has_update = !latest_version.is_empty() && latest_version != CURRENT_VERSION;

    Json(json!({
        "releases": release_options,
        "page": page,
        "per_page": 5,
        "has_more": false,
        "current_version": CURRENT_VERSION,
        "latest_version": latest_version,
        "has_update": has_update,
        "is_dev_mode": false
    }))
}

pub async fn update(
    State(state): State<AppState>,
    Json(payload): Json<UpdateRequest>,
) -> impl IntoResponse {
    let proxy = {
        let cfg = state.config.read().await;
        if cfg.outbound_proxy.is_empty() {
            None
        } else {
            Some(cfg.outbound_proxy.clone())
        }
    };

    let gh_releases = match fetch_github_releases(APP_REPO, 1, 10, proxy.as_deref()).await {
        Ok(r) => r,
        Err(_) => {
            return (
                StatusCode::OK,
                Json(json!({
                    "message": "已是最新版本，无需升级",
                    "success": true
                })),
            );
        }
    };

    let target = if let Some(tag) = payload.tag_name {
        gh_releases.into_iter().find(|r| r.tag_name == tag)
    } else {
        gh_releases.into_iter().next()
    };

    if let Some(rel) = target {
        if let Some(asset) = rel.assets.iter().find(|a| a.name.ends_with(".exe")) {
            let current_exe = std::env::current_exe().unwrap_or_default();
            let temp_new_exe = std::env::temp_dir().join("code-Manager-new.exe");

            info!("Downloading application update from {}", asset.browser_download_url);
            if let Ok(()) = download_file(&asset.browser_download_url, &temp_new_exe, proxy.as_deref()).await {
                // Spawn a replacement batch script
                let bat_path = std::env::temp_dir().join("code-Manager-update.bat");
                let bat_content = format!(
                    "@echo off\r\ntimeout /t 2 /nobreak > NUL\r\nmove /y \"{}\" \"{}\"\r\nstart \"\" \"{}\"\r\ndel \"%~f0\"\r\n",
                    temp_new_exe.to_string_lossy(),
                    current_exe.to_string_lossy(),
                    current_exe.to_string_lossy()
                );
                let _ = std::fs::write(&bat_path, bat_content);

                let _ = std::process::Command::new("cmd.exe")
                    .args(["/c", "start", &bat_path.to_string_lossy()])
                    .spawn();

                // Trigger exit
                let sender = state.shutdown_sender.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    let _ = sender.send(());
                });

                return (
                    StatusCode::OK,
                    Json(json!({
                        "message": "更新包已下载完成，正在重启并自动替换程序...",
                        "success": true
                    })),
                );
            }
        }
    }

    (
        StatusCode::OK,
        Json(json!({
            "message": "当前已是最新版本或处于受管状态",
            "success": true
        })),
    )
}

pub async fn exit_app(State(state): State<AppState>) -> impl IntoResponse {
    let sender = state.shutdown_sender.clone();
    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        let _ = sender.send(());
    });

    (
        StatusCode::OK,
        Json(json!({
            "message": "正在停止所有服务并退出...",
            "success": true
        })),
    )
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/application/identity", get(identity))
        .route("/api/application/releases", get(releases))
        .route("/api/application/update", post(update))
        .route("/api/application/exit", post(exit_app))
}
