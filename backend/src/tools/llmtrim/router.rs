use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;

use super::ownership::set_managed_tool_desired_running;
use super::service::LlmtrimService;
use super::types::{
    LlmtrimInstallRequest, LlmtrimInstallResponse, LlmtrimStatusResponse, LlmtrimUninstallResponse,
};
use crate::common::process::{is_pid_alive, kill_process_by_pid, wait_for_process_exit};
use crate::state::AppState;

pub(crate) fn llmtrim_status_data(state: &AppState, path: &str) -> LlmtrimStatusResponse {
    let (route_selected, _) = crate::server::proxy::refresh_llmtrim_route(state, path);
    let mut status = LlmtrimService::get_status(path);
    status.route_selected = route_selected;
    status
}

async fn persist_llmtrim_path(state: &AppState, path: &str) -> Result<(), String> {
    let mut cfg = state.config.write().await;
    let mut updated = cfg.clone();
    updated.llmtrim_path = path.to_string();
    updated
        .save(&state.config_path)
        .map_err(|e| format!("保存 llmtrim 路径失败: {}", e))?;
    *cfg = updated;
    Ok(())
}

fn persist_llmtrim_route_intent(path: &str, desired: bool) -> Result<(), String> {
    let executable = LlmtrimService::get_executable_path(path);
    let directory = executable
        .parent()
        .ok_or_else(|| "无法确定 llmtrim 安装目录".to_string())?;
    set_managed_tool_desired_running(directory, desired)
}

async fn wait_for_llmtrim_port_to_close(path: &str) -> Result<(), String> {
    // The Go implementation gives the daemon up to eight seconds to tear
    // down its supervised tray/process tree before declaring the route stuck.
    for _ in 0..80 {
        if !LlmtrimService::is_port_open() && !LlmtrimService::owns_listener(path) {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    Err("llmtrim 代理端口仍在使用，路由保持锁定".to_string())
}

fn close_llmtrim_status_viewer(pid: u32) {
    #[cfg(target_os = "windows")]
    {
        let status = crate::common::process::new_silent_command("taskkill.exe")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status();
        if status.map(|s| s.success()).unwrap_or(false) {
            return;
        }
    }
    kill_process_by_pid(pid);
}

async fn stop_llmtrim_status_viewer(state: &AppState) {
    let pid = {
        let mut guard = state.llmtrim_log_viewer_pid.write().await;
        guard.take()
    };
    if let Some(pid) = pid {
        close_llmtrim_status_viewer(pid);
    }
}

fn route_was_selected(state: &AppState) -> bool {
    state
        .llmtrim_route
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .selected
}

#[derive(Debug, Deserialize)]
pub struct PageQuery {
    pub page: Option<usize>,
}

pub async fn get_status(State(state): State<AppState>) -> impl IntoResponse {
    let cfg = state.config.read().await;
    Json(llmtrim_status_data(&state, &cfg.llmtrim_path))
}

pub async fn get_releases(
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

    match LlmtrimService::fetch_releases(page, proxy.as_deref()).await {
        Ok(res) => (StatusCode::OK, Json(json!(res))),
        Err(err) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": err, "releases": [], "page": page, "has_more": false })),
        ),
    }
}

pub async fn install(
    State(state): State<AppState>,
    Json(payload): Json<LlmtrimInstallRequest>,
) -> impl IntoResponse {
    stop_llmtrim_status_viewer(&state).await;
    let _operation = state.llmtrim_operation_lock.lock().await;
    let installed_path = LlmtrimService::get_executable_path("")
        .to_string_lossy()
        .to_string();
    let (configured_path, upstream_url, proxy) = {
        let cfg = state.config.read().await;
        let configured_path = cfg.llmtrim_path.clone();
        let configured = crate::common::windows::canonicalize_or_clean(
            LlmtrimService::get_executable_path(&configured_path),
        );
        let installed = crate::common::windows::canonicalize_or_clean(&installed_path);
        let paths_differ = if cfg!(target_os = "windows") {
            !configured
                .to_string_lossy()
                .eq_ignore_ascii_case(&installed.to_string_lossy())
        } else {
            configured != installed
        };
        let (selected, _) = crate::server::proxy::refresh_llmtrim_route(&state, &configured_path);
        if paths_differ && selected {
            return (
                StatusCode::CONFLICT,
                Json(
                    json!({ "message": "请先停用当前 llmtrim 路由，再安装受管版本", "success": false }),
                ),
            );
        }
        if paths_differ {
            let status = LlmtrimService::get_status(&configured_path);
            if LlmtrimService::is_port_open() || status.process_id != 0 || status.tray_running {
                return (
                    StatusCode::CONFLICT,
                    Json(
                        json!({ "message": "其他路径的 llmtrim 或 43117 端口仍在使用，请先停止后再安装", "success": false }),
                    ),
                );
            }
        }
        let proxy = if cfg.outbound_proxy.is_empty() {
            None
        } else {
            Some(cfg.outbound_proxy.clone())
        };
        (configured_path, cfg.upstream_base_url.clone(), proxy)
    };

    let was_selected = route_was_selected(&state);
    crate::server::proxy::activate_llmtrim_route(&state);
    crate::server::proxy::clear_llmtrim_clients(&state).await;

    let result = LlmtrimService::install(payload.tag_name, &upstream_url, proxy.as_deref()).await;
    match result {
        Ok(()) => {
            let path = LlmtrimService::get_executable_path("")
                .to_string_lossy()
                .to_string();
            if let Err(err) = persist_llmtrim_path(&state, &path).await {
                crate::server::proxy::finish_llmtrim_route_transition(&state);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(
                        json!({ "message": format!("llmtrim 已启动，但{}", err), "success": false }),
                    ),
                );
            }
            crate::server::proxy::finish_llmtrim_route_transition(&state);
            crate::server::proxy::refresh_llmtrim_route(&state, &path);
            let cfg = state.config.read().await;
            let status = llmtrim_status_data(&state, &cfg.llmtrim_path);
            (
                StatusCode::OK,
                Json(json!(LlmtrimInstallResponse {
                    path: status.path,
                    version: status.version,
                    running: status.running,
                    configured: status.configured,
                    process_id: status.process_id,
                    port: status.port,
                    message: "llmtrim 已安装并配置，守护进程已启动。".to_string(),
                })),
            )
        }
        Err(err) => {
            crate::server::proxy::finish_llmtrim_route_transition(&state);
            if !was_selected {
                crate::server::proxy::deactivate_llmtrim_route(&state);
            } else {
                crate::server::proxy::refresh_llmtrim_route(&state, &configured_path);
            }
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "message": err, "success": false })),
            )
        }
    }
}

pub async fn start(State(state): State<AppState>) -> impl IntoResponse {
    stop_llmtrim_status_viewer(&state).await;
    let _operation = state.llmtrim_operation_lock.lock().await;
    let was_selected = route_was_selected(&state);
    crate::server::proxy::activate_llmtrim_route(&state);
    let (path, upstream_url) = {
        let cfg = state.config.read().await;
        (cfg.llmtrim_path.clone(), cfg.upstream_base_url.clone())
    };
    crate::server::proxy::clear_llmtrim_clients(&state).await;

    let start_path = path.clone();
    let start_result =
        tokio::task::spawn_blocking(move || LlmtrimService::start(&start_path, &upstream_url))
            .await;
    match start_result.unwrap_or_else(|e| Err(format!("llmtrim 启动任务失败: {}", e))) {
        Ok(()) => {
            let resolved_path = LlmtrimService::get_executable_path(&path)
                .to_string_lossy()
                .to_string();
            if let Err(err) = persist_llmtrim_path(&state, &resolved_path).await {
                crate::server::proxy::finish_llmtrim_route_transition(&state);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(
                        json!({ "message": format!("llmtrim 已启动，但{}", err), "success": false }),
                    ),
                );
            }
            crate::server::proxy::finish_llmtrim_route_transition(&state);
            crate::server::proxy::refresh_llmtrim_route(&state, &resolved_path);
            let status = llmtrim_status_data(&state, &resolved_path);
            (
                StatusCode::OK,
                Json(json!({
                    "message": "llmtrim 守护进程已启动并确认正在运行。",
                    "success": true,
                    "running": true,
                    "status": status,
                })),
            )
        }
        Err(err) => {
            crate::server::proxy::finish_llmtrim_route_transition(&state);
            if !was_selected {
                crate::server::proxy::deactivate_llmtrim_route(&state);
            } else {
                crate::server::proxy::refresh_llmtrim_route(&state, &path);
            }
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "message": err, "success": false })),
            )
        }
    }
}

pub(crate) async fn stop_managed_llmtrim(state: &AppState) -> Result<String, String> {
    stop_llmtrim_status_viewer(state).await;
    let _operation = state.llmtrim_operation_lock.lock().await;
    let was_selected = route_was_selected(state);
    crate::server::proxy::activate_llmtrim_route(&state);
    let path = {
        let cfg = state.config.read().await;
        cfg.llmtrim_path.clone()
    };

    crate::server::proxy::clear_llmtrim_clients(&state).await;

    let stop_path = path.clone();
    let mut result = tokio::task::spawn_blocking(move || LlmtrimService::stop(&stop_path))
        .await
        .unwrap_or_else(|err| Err(format!("llmtrim 停止任务失败: {}", err)));
    if result.is_ok() {
        result = wait_for_llmtrim_port_to_close(&path).await;
    }
    if result.is_ok() {
        result = persist_llmtrim_route_intent(&path, false);
    }
    if result.is_ok() {
        crate::server::proxy::deactivate_llmtrim_route(&state);
    } else {
        crate::server::proxy::finish_llmtrim_route_transition(&state);
        if !was_selected {
            crate::server::proxy::deactivate_llmtrim_route(&state);
        } else {
            crate::server::proxy::refresh_llmtrim_route(&state, &path);
        }
    }
    result.map(|()| path)
}

pub async fn stop(State(state): State<AppState>) -> impl IntoResponse {
    match stop_managed_llmtrim(&state).await {
        Ok(path) => {
            let status = llmtrim_status_data(&state, &path);
            (
                StatusCode::OK,
                Json(json!({
                    "message": "llmtrim 和 llmtrim-tray 已停止，相关自启动和用户环境变量已清理。",
                    "success": true,
                    "running": false,
                    "status": status,
                })),
            )
        }
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "message": err, "success": false })),
        ),
    }
}

pub async fn uninstall(State(state): State<AppState>) -> impl IntoResponse {
    stop_llmtrim_status_viewer(&state).await;
    let _operation = state.llmtrim_operation_lock.lock().await;
    let was_selected = route_was_selected(&state);
    crate::server::proxy::activate_llmtrim_route(&state);
    let path = {
        let cfg = state.config.read().await;
        cfg.llmtrim_path.clone()
    };

    crate::server::proxy::clear_llmtrim_clients(&state).await;

    let mut result = LlmtrimService::uninstall(&path);
    if result.is_ok() {
        if let Err(err) = wait_for_llmtrim_port_to_close(&path).await {
            result = Err(err);
        }
    }
    if result.is_ok() {
        crate::server::proxy::deactivate_llmtrim_route(&state);
    } else {
        crate::server::proxy::finish_llmtrim_route_transition(&state);
        if !was_selected {
            crate::server::proxy::deactivate_llmtrim_route(&state);
        } else {
            crate::server::proxy::refresh_llmtrim_route(&state, &path);
        }
    }
    match result {
        Ok(warnings) => {
            // 清空 config.yaml 中的 llmtrim_path 并保存
            {
                let mut cfg = state.config.write().await;
                cfg.llmtrim_path = String::new();
                let _ = cfg.save(&state.config_path);
            }

            (
                StatusCode::OK,
                Json(json!(LlmtrimUninstallResponse {
                    path: String::new(),
                    running: false,
                    configured: false,
                    directory_exists: false,
                    state_dir_exists: false,
                    tray_running: false,
                    residual: false,
                    process_id: 0,
                    tray_process_id: 0,
                    port: "127.0.0.1:43117".to_string(),
                    message: "llmtrim 和 llmtrim-tray 已停止，受管安装目录、统计数据库、用户 CA 与环境配置已彻底清理。".to_string(),
                    warnings,
                })),
            )
        }
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "message": err, "success": false })),
        ),
    }
}

pub async fn get_logs(State(state): State<AppState>) -> impl IntoResponse {
    let mut pid_guard = state.llmtrim_log_viewer_pid.write().await;
    if let Some(pid) = *pid_guard {
        if !is_pid_alive(pid) {
            *pid_guard = None;
        }
    }
    let showing = pid_guard.is_some();
    Json(json!({
        "showing": showing,
        "path": LlmtrimService::get_log_path().to_string_lossy().to_string()
    }))
}

pub async fn show_logs(State(state): State<AppState>) -> impl IntoResponse {
    stop_llmtrim_status_viewer(&state).await;
    let configured_path = state.config.read().await.llmtrim_path.clone();
    let mut pid_guard = state.llmtrim_log_viewer_pid.write().await;
    match LlmtrimService::show_logs(&configured_path) {
        Ok(pid) => {
            *pid_guard = Some(pid);
            drop(pid_guard);

            let state_clone = state.clone();
            tokio::spawn(async move {
                let _ = tokio::task::spawn_blocking(move || {
                    wait_for_process_exit(pid);
                })
                .await;
                let mut guard = state_clone.llmtrim_log_viewer_pid.write().await;
                if *guard == Some(pid) {
                    *guard = None;
                }
            });

            (
                StatusCode::OK,
                Json(
                    json!({ "showing": true, "message": "已打开 llmtrim 状态窗口，正在运行 .\\llmtrim.exe status。" }),
                ),
            )
        }
        Err(e) => {
            *pid_guard = None;
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "showing": false, "message": format!("启动日志窗口失败: {}", e) })),
            )
        }
    }
}

pub async fn hide_logs(State(state): State<AppState>) -> impl IntoResponse {
    let mut pid_guard = state.llmtrim_log_viewer_pid.write().await;
    if let Some(pid) = *pid_guard {
        close_llmtrim_status_viewer(pid);
    }
    *pid_guard = None;
    (
        StatusCode::OK,
        Json(json!({ "showing": false, "message": "已隐藏 LLMTrim 日志" })),
    )
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/llmtrim", get(get_status))
        .route("/api/llmtrim/releases", get(get_releases))
        .route("/api/llmtrim/install", post(install))
        .route("/api/llmtrim/start", post(start))
        .route("/api/llmtrim/stop", post(stop))
        .route("/api/llmtrim/uninstall", post(uninstall))
        .route("/api/llmtrim/logs", get(get_logs))
        .route("/api/llmtrim/logs/show", post(show_logs))
        .route("/api/llmtrim/logs/hide", post(hide_logs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::AppConfig;

    #[tokio::test]
    async fn install_keeps_a_selected_custom_route_unchanged() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "llmtrim-install-route-test-{}-{}",
            std::process::id(),
            nonce
        ));
        let custom_path = directory.join("llmtrim.exe").to_string_lossy().to_string();
        let state = AppState::new(
            AppConfig {
                llmtrim_path: custom_path.clone(),
                ..AppConfig::default()
            },
            directory.join("config.yaml"),
        );
        crate::server::proxy::activate_llmtrim_route(&state);

        let response = install(
            State(state.clone()),
            Json(LlmtrimInstallRequest { tag_name: None }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(state.config.read().await.llmtrim_path, custom_path);
        assert!(state.llmtrim_route.lock().unwrap().selected);
        assert!(!directory.exists());
    }

    #[tokio::test]
    async fn saved_path_is_persisted_and_a_failed_save_keeps_the_old_value() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "llmtrim-path-test-{}-{}",
            std::process::id(),
            nonce
        ));
        std::fs::create_dir(&directory).unwrap();
        let config_path = directory.join("config.yaml");
        let state = AppState::new(AppConfig::default(), config_path.clone());
        let path = r"C:\tools\llmtrim\llmtrim.exe";

        persist_llmtrim_path(&state, path).await.unwrap();
        let saved: AppConfig =
            serde_yaml::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
        assert_eq!(saved.llmtrim_path, path);
        assert_eq!(state.config.read().await.llmtrim_path, path);

        let invalid_state = AppState::new(saved, directory.clone());
        assert!(persist_llmtrim_path(&invalid_state, "other.exe")
            .await
            .is_err());
        assert_eq!(invalid_state.config.read().await.llmtrim_path, path);

        std::fs::remove_file(config_path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}
