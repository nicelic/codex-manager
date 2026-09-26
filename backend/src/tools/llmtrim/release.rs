use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::Path;
use tracing::{info, warn};
use zip::ZipArchive;

use super::types::{LlmtrimReleaseListResponse, LlmtrimReleaseOption};
use crate::common::downloader::{build_http_client, fetch_github_releases};

pub const LLMTRIM_REPO: &str = "fkiene/llmtrim";
pub const LLMTRIM_RELEASE_PAGE_SIZE: usize = 5;

pub fn target_windows_asset_name() -> &'static str {
    "llmtrim-x86_64-pc-windows-msvc.zip"
}

pub fn target_checksum_asset_name() -> &'static str {
    "llmtrim-x86_64-pc-windows-msvc.zip.sha256"
}

pub fn is_windows_zip_asset(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.ends_with(".zip") && (lower.contains("windows") || lower.contains("x86_64") || lower.contains("amd64"))
}

pub fn parse_sha256_hash(text: &str) -> Option<String> {
    for word in text.split_whitespace() {
        let clean = word.trim();
        if clean.len() == 64 && clean.chars().all(|c| c.is_ascii_hexdigit()) {
            return Some(clean.to_lowercase());
        }
    }
    None
}

pub fn compute_file_sha256(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| format!("打开文件计算 SHA256 失败: {}", e))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer).map_err(|e| format!("读取文件计算 SHA256 失败: {}", e))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hex::encode(hasher.finalize()))
}

pub async fn fetch_llmtrim_releases(
    page: usize,
    proxy_url: Option<&str>,
) -> Result<LlmtrimReleaseListResponse, String> {
    let releases = fetch_github_releases(LLMTRIM_REPO, page, LLMTRIM_RELEASE_PAGE_SIZE, proxy_url).await?;
    let count = releases.len();

    let options = releases
        .into_iter()
        .map(|rel| {
            let asset = rel.assets.iter().find(|a| is_windows_zip_asset(&a.name));
            let (available, asset_name) = match asset {
                Some(a) => (true, a.name.clone()),
                None => (false, String::new()),
            };

            LlmtrimReleaseOption {
                tag_name: rel.tag_name,
                name: rel.name.unwrap_or_default(),
                published_at: rel.published_at.unwrap_or_default(),
                prerelease: rel.prerelease.unwrap_or(false),
                available,
                asset_name,
            }
        })
        .collect();

    Ok(LlmtrimReleaseListResponse {
        releases: options,
        page,
        per_page: LLMTRIM_RELEASE_PAGE_SIZE,
        has_more: count == LLMTRIM_RELEASE_PAGE_SIZE,
    })
}

pub async fn download_and_install_llmtrim(
    tag_name: Option<&str>,
    install_dir: &Path,
    proxy_url: Option<&str>,
) -> Result<String, String> {
    let releases = fetch_github_releases(LLMTRIM_REPO, 1, 10, proxy_url).await?;
    let target_release = if let Some(tag) = tag_name {
        releases.into_iter().find(|r| r.tag_name == tag)
    } else {
        releases.into_iter().next()
    }
    .ok_or_else(|| "未找到匹配的 llmtrim 发布版本".to_string())?;

    let version_tag = target_release.tag_name.clone();

    let zip_asset = target_release
        .assets
        .iter()
        .find(|a| is_windows_zip_asset(&a.name))
        .ok_or_else(|| format!("版本 {} 没有适用于 Windows 的 llmtrim 安装包", version_tag))?;

    let checksum_asset = target_release
        .assets
        .iter()
        .find(|a| a.name.ends_with(".sha256") || a.name == format!("{}.sha256", zip_asset.name))
        .ok_or_else(|| format!("版本 {} 没有校验文件 {}.sha256", version_tag, zip_asset.name))?;

    let client = build_http_client(proxy_url);

    // 1. 下载 SHA-256 校验文件
    let checksum_res = client
        .get(&checksum_asset.browser_download_url)
        .send()
        .await
        .map_err(|e| format!("下载校验文件失败: {}", e))?;
    if !checksum_res.status().is_success() {
        return Err(format!("下载校验文件返回 HTTP {}", checksum_res.status()));
    }
    let checksum_text = checksum_res
        .text()
        .await
        .map_err(|e| format!("读取校验文件内容失败: {}", e))?;
    let expected_hash = parse_sha256_hash(&checksum_text)
        .ok_or_else(|| "校验文件中没有找到有效的 64 位 SHA-256 摘要".to_string())?;

    // 2. 下载 ZIP 安装包到临时文件
    let temp_dir = std::env::temp_dir();
    let temp_zip = temp_dir.join(format!(
        "llmtrim_install_{}_{}.zip",
        version_tag,
        chrono::Utc::now().timestamp_millis()
    ));

    let zip_res = client
        .get(&zip_asset.browser_download_url)
        .send()
        .await
        .map_err(|e| format!("下载 Windows 安装包失败: {}", e))?;
    if !zip_res.status().is_success() {
        return Err(format!("下载安装包返回 HTTP {}", zip_res.status()));
    }
    let zip_bytes = zip_res
        .bytes()
        .await
        .map_err(|e| format!("接收安装包数据流失败: {}", e))?;
    fs::write(&temp_zip, &zip_bytes).map_err(|e| format!("保存临时安装包失败: {}", e))?;

    // 3. 计算并校验 SHA-256
    let actual_hash = compute_file_sha256(&temp_zip)?;
    if !actual_hash.eq_ignore_ascii_case(&expected_hash) {
        let _ = fs::remove_file(&temp_zip);
        return Err(format!(
            "安装包 SHA-256 校验失败，期望 {}，实际 {}",
            expected_hash, actual_hash
        ));
    }
    info!("LLMTrim 安装包 SHA-256 校验通过: {}", actual_hash);

    // 4. 安全解压至 staging 临时目录
    let staging_dir = temp_dir.join(format!(
        "llmtrim_staging_{}_{}",
        version_tag,
        chrono::Utc::now().timestamp_millis()
    ));
    let _ = fs::create_dir_all(&staging_dir);

    let extract_res = (|| -> Result<(), String> {
        let file = File::open(&temp_zip).map_err(|e| format!("打开 ZIP 失败: {}", e))?;
        let mut archive = ZipArchive::new(BufReader::new(file)).map_err(|e| format!("解析 ZIP 失败: {}", e))?;

        let mut found_exe = false;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(|e| format!("读取条目 {} 失败: {}", i, e))?;
            if entry.is_dir() {
                continue;
            }
            let name = entry.name().replace('\\', "/");
            let file_name = Path::new(&name)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");

            if file_name.is_empty() || file_name == "." || file_name == ".." || file_name.starts_with('.') {
                continue;
            }

            if file_name.eq_ignore_ascii_case("llmtrim.exe") {
                found_exe = true;
            }

            let dest_file = staging_dir.join(file_name);
            let mut out = File::create(&dest_file).map_err(|e| format!("创建解压文件 {:?} 失败: {}", dest_file, e))?;
            std::io::copy(&mut entry, &mut out).map_err(|e| format!("写入解压文件 {:?} 失败: {}", dest_file, e))?;
        }

        if !found_exe {
            return Err("ZIP 中未找到 llmtrim.exe 可执行文件".to_string());
        }
        Ok(())
    })();

    let _ = fs::remove_file(&temp_zip);
    if let Err(err) = extract_res {
        let _ = fs::remove_dir_all(&staging_dir);
        return Err(err);
    }

    // 5. 原子替换安装目录
    if install_dir.exists() {
        let _ = fs::remove_dir_all(install_dir);
    }
    if let Some(parent) = install_dir.parent() {
        let _ = fs::create_dir_all(parent);
    }

    if let Err(e) = fs::rename(&staging_dir, install_dir) {
        // Fallback copy if rename across drives/mount points fails
        let _ = fs::create_dir_all(install_dir);
        if let Ok(entries) = fs::read_dir(&staging_dir) {
            for entry in entries.flatten() {
                let dest = install_dir.join(entry.file_name());
                let _ = fs::copy(entry.path(), dest);
            }
        }
        let _ = fs::remove_dir_all(&staging_dir);
        warn!("重命名 staging 目录失败，已执行文件复制: {}", e);
    }

    // 写入版本标记
    let _ = fs::write(install_dir.join(".llmtrim-version"), format!("{}\n", version_tag));
    info!("LLMTrim {} 已成功安装至 {:?}", version_tag, install_dir);

    Ok(version_tag)
}
