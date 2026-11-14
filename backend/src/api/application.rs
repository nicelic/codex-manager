use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use crate::common::downloader::{download_file_with_hash, fetch_github_releases, GithubRelease, GithubReleaseAsset};
use crate::state::AppState;
use tracing::{info, warn};
use std::cmp::Ordering;

const APP_REPO: &str = "nicelic/codex-manager";
const CURRENT_VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));
const APP_RELEASE_PAGE_SIZE: usize = 5;

#[derive(Debug, Deserialize)]
pub struct PageQuery {
    pub page: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateRequest {
    pub tag_name: Option<String>,
}

/// 语义化版本对比：返回 Ordering (Greater / Equal / Less)
pub fn compare_versions(v1: &str, v2: &str) -> Ordering {
    fn parse_parts(v: &str) -> (Vec<u64>, Option<String>) {
        let clean = v.trim().trim_start_matches(|c| c == 'v' || c == 'V');
        let (num_part, pre_part) = match clean.split_once('-') {
            Some((num, pre)) => (num, Some(pre.to_string())),
            None => (clean, None),
        };
        let nums: Vec<u64> = num_part
            .split('.')
            .map(|s| s.parse::<u64>().unwrap_or(0))
            .collect();
        (nums, pre_part)
    }

    let (nums1, pre1) = parse_parts(v1);
    let (nums2, pre2) = parse_parts(v2);

    let max_len = nums1.len().max(nums2.len());
    for i in 0..max_len {
        let n1 = nums1.get(i).copied().unwrap_or(0);
        let n2 = nums2.get(i).copied().unwrap_or(0);
        if n1 != n2 {
            return n1.cmp(&n2);
        }
    }

    match (pre1, pre2) {
        (None, None) => Ordering::Equal,
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (Some(p1), Some(p2)) => p1.cmp(&p2),
    }
}

/// 检测当前是否运行在开发/源码工作区环境下
pub fn check_is_dev_mode() -> bool {
    if let Ok(val) = std::env::var("CODE_MANAGER_DEV") {
        return val == "1" || val.eq_ignore_ascii_case("true");
    }

    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(_) => return false,
    };

    let exe_str = exe.to_string_lossy().to_lowercase();
    if exe_str.contains(r"\target\debug\") || exe_str.contains(r"\target\release\") {
        return true;
    }

    if let Some(parent) = exe.parent() {
        for marker in &["Cargo.toml", "build.bat", "frontend"] {
            if parent.join(marker).exists() {
                return true;
            }
        }
    }

    false
}

/// 从 Release body 中提取指定资产或全局的 SHA-256 校验和
pub fn extract_sha256_for_asset(body: &str, asset_name: &str) -> Option<String> {
    let lines: Vec<&str> = body.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.to_lowercase().contains(&asset_name.to_lowercase()) {
            if i > 0 {
                if let Some(h) = parse_hash_line(lines[i - 1]) {
                    return Some(h);
                }
            }
            if let Some(h) = parse_hash_line(trimmed) {
                return Some(h);
            }
            if i + 1 < lines.len() {
                if let Some(h) = parse_hash_line(lines[i + 1]) {
                    return Some(h);
                }
            }
        }
    }

    // 全文兜底检索
    for line in lines {
        if let Some(h) = parse_hash_line(line) {
            return Some(h);
        }
    }
    None
}

fn parse_hash_line(s: &str) -> Option<String> {
    for part in s.split_whitespace() {
        let clean = part
            .trim()
            .trim_start_matches(|c: char| !c.is_ascii_hexdigit());
        if clean.len() == 64 && clean.chars().all(|c| c.is_ascii_hexdigit()) {
            return Some(clean.to_lowercase());
        }
    }
    None
}

/// 匹配 Release 中可用的 Windows EXE 资产
fn find_windows_exe_asset(release: &GithubRelease) -> Option<&GithubReleaseAsset> {
    // 优先匹配标准名称 code-Manager-rust.exe
    if let Some(a) = release.assets.iter().find(|a| a.name.eq_ignore_ascii_case("code-Manager-rust.exe")) {
        return Some(a);
    }
    // 其次匹配历史名称 code-Manager.exe
    if let Some(a) = release.assets.iter().find(|a| a.name.eq_ignore_ascii_case("code-Manager.exe")) {
        return Some(a);
    }
    // 再次匹配任意以 .exe 结尾的资产
    release.assets.iter().find(|a| a.name.ends_with(".exe"))
}

pub async fn identity() -> impl IntoResponse {
    let current_exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    Json(json!({
        "version": CURRENT_VERSION,
        "is_dev_mode": check_is_dev_mode(),
        "name": "code-Manager-rust",
        "executable_path": current_exe
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

    let gh_releases = match fetch_github_releases(APP_REPO, page, APP_RELEASE_PAGE_SIZE, proxy.as_deref()).await {
        Ok(r) => r,
        Err(e) => {
            warn!("获取 GitHub releases 失败: {}", e);
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({
                    "releases": [],
                    "page": page,
                    "per_page": APP_RELEASE_PAGE_SIZE,
                    "has_more": false,
                    "current_version": CURRENT_VERSION,
                    "latest_version": CURRENT_VERSION,
                    "has_update": false,
                    "is_dev_mode": check_is_dev_mode(),
                    "message": format!("无法连接 GitHub 检查更新: {}", e)
                })),
            );
        }
    };

    let has_more = gh_releases.len() == APP_RELEASE_PAGE_SIZE;
    let mut release_options = Vec::new();
    let mut latest_version = CURRENT_VERSION.to_string();

    for (idx, r) in gh_releases.iter().enumerate() {
        if idx == 0 && page == 1 {
            latest_version = r.tag_name.clone();
        }
        let (available, asset_name, unavailable_reason) = match find_windows_exe_asset(r) {
            Some(a) => (true, a.name.clone(), None),
            None => (false, String::new(), Some("无可用的 Windows 可执行程序 (.exe)".to_string())),
        };

        release_options.push(json!({
            "tag_name": r.tag_name,
            "name": r.name.as_deref().unwrap_or(&r.tag_name),
            "published_at": r.published_at.as_deref().unwrap_or(""),
            "prerelease": r.prerelease.unwrap_or(false),
            "available": available,
            "asset_name": asset_name,
            "unavailable_reason": unavailable_reason
        }));
    }

    if release_options.is_empty() && page == 1 {
        release_options.push(json!({
            "tag_name": CURRENT_VERSION,
            "name": format!("{} 当前版本", CURRENT_VERSION),
            "published_at": "",
            "prerelease": false,
            "available": true,
            "asset_name": "code-Manager-rust.exe"
        }));
    }

    let has_update = compare_versions(&latest_version, CURRENT_VERSION) == Ordering::Greater;

    (
        StatusCode::OK,
        Json(json!({
            "releases": release_options,
            "page": page,
            "per_page": APP_RELEASE_PAGE_SIZE,
            "has_more": has_more,
            "current_version": CURRENT_VERSION,
            "latest_version": latest_version,
            "has_update": has_update,
            "is_dev_mode": check_is_dev_mode()
        })),
    )
}

pub async fn update(
    State(state): State<AppState>,
    Json(payload): Json<UpdateRequest>,
) -> impl IntoResponse {
    if check_is_dev_mode() {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "accepted": false,
                "success": false,
                "message": "当前处于源码开发环境，已锁定更新，避免覆盖正在开发的文件"
            })),
        );
    }

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
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({
                    "accepted": false,
                    "success": false,
                    "message": format!("获取 GitHub Releases 失败: {}", e)
                })),
            );
        }
    };

    let target_release = if let Some(tag) = payload.tag_name.as_deref().filter(|t| !t.trim().is_empty()) {
        match gh_releases.into_iter().find(|r| r.tag_name.eq_ignore_ascii_case(tag.trim())) {
            Some(r) => r,
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(json!({
                        "accepted": false,
                        "success": false,
                        "message": format!("未在近期的远端 Releases 中找到版本 {}", tag)
                    })),
                );
            }
        }
    } else {
        match gh_releases.into_iter().next() {
            Some(r) => r,
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(json!({
                        "accepted": false,
                        "success": false,
                        "message": "GitHub 仓库中未找到任何可用的 Release 版本"
                    })),
                );
            }
        }
    };

    let target_tag = target_release.tag_name.clone();
    let asset = match find_windows_exe_asset(&target_release) {
        Some(a) => a.clone(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "accepted": false,
                    "success": false,
                    "message": format!("版本 {} 未提供 Windows 可执行文件资产 (.exe)", target_tag)
                })),
            );
        }
    };

    let current_exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "accepted": false,
                    "success": false,
                    "message": format!("无法定位当前程序路径: {}", e)
                })),
            );
        }
    };

    let release_dir = match current_exe.parent() {
        Some(d) => d.to_path_buf(),
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "accepted": false,
                    "success": false,
                    "message": "无法获取当前程序所在目录"
                })),
            );
        }
    };

    let timestamp = chrono::Utc::now().timestamp_millis();
    let staged_exe = release_dir.join(format!(".code-manager-staged-{}.exe", timestamp));

    // 提取可选的 SHA-256 校验和
    let expected_sha256 = target_release
        .body
        .as_deref()
        .and_then(|b| extract_sha256_for_asset(b, &asset.name));

    info!(
        "开始下载更新: 版本={}, 资产={}, URL={}, 预期SHA256={:?}",
        target_tag, asset.name, asset.browser_download_url, expected_sha256
    );

    if let Err(e) = download_file_with_hash(
        &asset.browser_download_url,
        &staged_exe,
        expected_sha256.as_deref(),
        proxy.as_deref(),
    ).await {
        let _ = std::fs::remove_file(&staged_exe);
        return (
            StatusCode::BAD_GATEWAY,
            Json(json!({
                "accepted": false,
                "success": false,
                "message": format!("下载并校验更新包失败: {}", e)
            })),
        );
    }

    // 准备 Windows 原子替换批处理脚本
    let bat_path = release_dir.join(format!(".code-manager-update-{}.bat", timestamp));
    let backup_exe = release_dir.join(".code-manager-backup.exe");
    let config_dir = release_dir.join("config");
    let log_file = config_dir.join("code-Manager-update.log");

    let bat_content = format!(
        r#"@echo off
setlocal enabledelayedexpansion

set "TARGET={target}"
set "STAGED={staged}"
set "PARENT_PID={pid}"
set "RELEASE_DIR={release_dir}"
set "CONFIG_DIR={config_dir}"
set "LOG_FILE={log_file}"
set "BACKUP={backup}"

if not exist "%CONFIG_DIR%" mkdir "%CONFIG_DIR%" >nul 2>&1
echo [%date% %time%] Update batch started. Target="%TARGET%" Staged="%STAGED%" ParentPID=%PARENT_PID% >> "%LOG_FILE%"

rem 1. 等待主进程退出
ping -n 3 127.0.0.1 >nul
echo [%date% %time%] Initial wait finished. >> "%LOG_FILE%"

rem 2. 清理残留工具进程（独立守护模式：保持 Gortex daemon 常驻）
taskkill /F /IM rtk.exe >nul 2>&1
taskkill /F /IM snip.exe >nul 2>&1

rem 3. 确保旧主进程已退出
if not "%PARENT_PID%"=="" (
    taskkill /F /PID %PARENT_PID% >nul 2>&1
)
taskkill /F /IM code-Manager-rust.exe >nul 2>&1
taskkill /F /IM code-Manager.exe >nul 2>&1
ping -n 2 127.0.0.1 >nul

rem 4. 备份现有程序（带 30 次重试）
echo [%date% %time%] Backing up current target executable... >> "%LOG_FILE%"
for /l %%i in (1,1,30) do (
    if not exist "%TARGET%" goto :do_copy
    move /y "%TARGET%" "%BACKUP%" >nul 2>&1
    if not errorlevel 1 (
        echo [%date% %time%] Original moved to backup successfully. >> "%LOG_FILE%"
        goto :do_copy
    )
    ping -n 2 127.0.0.1 >nul
)

:do_copy
echo [%date% %time%] Copying staged executable to target... >> "%LOG_FILE%"
for /l %%i in (1,1,30) do (
    copy /y "%STAGED%" "%TARGET%" >nul 2>&1
    if not errorlevel 1 (
        echo [%date% %time%] Copy succeeded. >> "%LOG_FILE%"
        goto :replace_ok
    )
    ping -n 2 127.0.0.1 >nul
)

echo [%date% %time%] ERROR: Copy failed! Restoring backup... >> "%LOG_FILE%"
if exist "%BACKUP%" move /y "%BACKUP%" "%TARGET%" >nul 2>&1
goto :start_and_cleanup

:replace_ok
if exist "%BACKUP%" del /f /q "%BACKUP%" >nul 2>&1
if exist "%STAGED%" del /f /q "%STAGED%" >nul 2>&1
echo [%date% %time%] Replacement and cleanup completed. >> "%LOG_FILE%"

:start_and_cleanup
rem 5. 启动新版本程序
echo [%date% %time%] Launching new target executable... >> "%LOG_FILE%"
cd /d "%RELEASE_DIR%"
start "" "%TARGET%"
echo [%date% %time%] New target started. >> "%LOG_FILE%"

rem 6. 自删除批处理脚本
ping -n 2 127.0.0.1 >nul
(goto) 2>nul & del "%~f0"
"#,
        target = current_exe.to_string_lossy(),
        staged = staged_exe.to_string_lossy(),
        pid = std::process::id(),
        release_dir = release_dir.to_string_lossy(),
        config_dir = config_dir.to_string_lossy(),
        log_file = log_file.to_string_lossy(),
        backup = backup_exe.to_string_lossy(),
    );

    let crlf_content = bat_content.replace("\r\n", "\n").replace('\n', "\r\n");
    if let Err(e) = std::fs::write(&bat_path, crlf_content) {
        let _ = std::fs::remove_file(&staged_exe);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "accepted": false,
                "success": false,
                "message": format!("写入更新批处理脚本失败: {}", e)
            })),
        );
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x00000008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;

        if let Err(e) = std::process::Command::new("cmd.exe")
            .args(["/c", "start", "", "/min", &bat_path.to_string_lossy()])
            .current_dir(&release_dir)
            .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
            .spawn()
        {
            let _ = std::fs::remove_file(&staged_exe);
            let _ = std::fs::remove_file(&bat_path);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "accepted": false,
                    "success": false,
                    "message": format!("启动更新批处理失败: {}", e)
                })),
            );
        }
    }

    // 触发安全停机
    let sender = state.shutdown_sender.clone();
    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
        let _ = sender.send(());
    });

    (
        StatusCode::OK,
        Json(json!({
            "accepted": true,
            "success": true,
            "target_version": target_tag,
            "message": format!("code-Manager {} 已成功下载并通过校验，正在重启...", target_tag)
        })),
    )
}

pub async fn exit_app(State(state): State<AppState>) -> impl IntoResponse {
    let path = state.config.read().await.llmtrim_path.clone();
    let status = crate::tools::llmtrim::router::llmtrim_status_data(&state, &path);
    let mut warnings = Vec::new();
    if status.installed || status.running || status.tray_running || status.desired_running || status.route_selected {
        if let Err(err) = crate::tools::llmtrim::router::stop_managed_llmtrim(&state).await {
            warnings.push(format!("停止 llmtrim 失败: {}", err));
        }
    }
    let sender = state.shutdown_sender.clone();
    tokio::spawn(async move {
        // 独立守护模式：主程序退出时不停止或杀死 Gortex daemon，保持后台常驻
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        let _ = sender.send(());
    });

    (
        StatusCode::OK,
        Json(json!({
            "completed": true,
            "clean": warnings.is_empty(),
            "message": "正在退出程序...",
            "success": true,
            "warnings": warnings
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compare_versions() {
        assert_eq!(compare_versions("v1.0.16", "v1.0.15"), Ordering::Greater);
        assert_eq!(compare_versions("v1.0.15", "v1.0.16"), Ordering::Less);
        assert_eq!(compare_versions("v1.0.16", "v1.0.16"), Ordering::Equal);
        assert_eq!(compare_versions("v1.0.16", "1.0.16"), Ordering::Equal);
        assert_eq!(compare_versions("v1.0.4", "v1.0.16"), Ordering::Less);
        assert_eq!(compare_versions("v1.0.16", "v1.0.4"), Ordering::Greater);
        assert_eq!(compare_versions("v1.0.16-beta", "v1.0.16"), Ordering::Less);
        assert_eq!(compare_versions("v1.0.16", "v1.0.16-beta"), Ordering::Greater);
    }

    #[test]
    fn test_extract_sha256() {
        let body = r#"
v1.0.16 release. Windows executable included.
SHA-256: 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
code-Manager-rust.exe
        "#;
        let hash = extract_sha256_for_asset(body, "code-Manager-rust.exe");
        assert_eq!(hash, Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string()));
    }
}
