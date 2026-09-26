use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufReader, copy};
use std::path::Path;
use tracing::info;
use zip::ZipArchive;

use super::assistant::{
    RTK_CLAUDE_AGENT_INSTRUCTIONS, RTK_CODEX_AGENT_INSTRUCTIONS, RTK_CODEX_COMMANDS,
};
use super::types::{RtkReleaseListResponse, RtkReleaseOption};
use crate::common::downloader::{build_http_client, fetch_github_releases, GithubRelease};

const RTK_REPO: &str = "rtk-ai/rtk";
const RTK_WINDOWS_ASSET_NAME: &str = "rtk-x86_64-pc-windows-msvc.zip";
const RTK_RELEASE_PAGE_SIZE: usize = 5;

pub fn rtk_windows_asset_name() -> &'static str {
    #[cfg(target_arch = "x86_64")]
    {
        RTK_WINDOWS_ASSET_NAME
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        ""
    }
}

pub fn parse_checksum(text: &str, asset_name: &str) -> Result<String, String> {
    for line in text.lines() {
        if line.contains(asset_name) {
            for word in line.split_whitespace() {
                let trimmed = word.trim();
                if trimmed.len() == 64 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Ok(trimmed.to_lowercase());
                }
            }
        }
    }
    Err(format!("checksums.txt 中没有找到 {} 的 SHA-256 摘要", asset_name))
}

pub async fn fetch_rtk_releases(page: usize, proxy_url: Option<&str>) -> Result<RtkReleaseListResponse, String> {
    let asset_name = rtk_windows_asset_name();
    if asset_name.is_empty() {
        return Ok(RtkReleaseListResponse {
            releases: Vec::new(),
            page,
            per_page: RTK_RELEASE_PAGE_SIZE,
            has_more: false,
        });
    }

    let releases = fetch_github_releases(RTK_REPO, page, RTK_RELEASE_PAGE_SIZE, proxy_url).await?;
    let has_more = releases.len() == RTK_RELEASE_PAGE_SIZE;

    let options = releases
        .into_iter()
        .filter(|rel| !rel.tag_name.trim().is_empty())
        .map(|rel| {
            let available = rel.assets.iter().any(|a| a.name.eq_ignore_ascii_case(asset_name));
            RtkReleaseOption {
                tag_name: rel.tag_name,
                name: rel.name.unwrap_or_default(),
                published_at: rel.published_at.unwrap_or_default(),
                prerelease: rel.prerelease.unwrap_or(false),
                available,
                asset_name: if available { asset_name.to_string() } else { String::new() },
            }
        })
        .collect();

    Ok(RtkReleaseListResponse {
        releases: options,
        page,
        per_page: RTK_RELEASE_PAGE_SIZE,
        has_more,
    })
}

pub async fn download_and_install_rtk(
    tag_name: &str,
    install_dir: &Path,
    proxy_url: Option<&str>,
) -> Result<(), String> {
    let asset_name = rtk_windows_asset_name();
    if asset_name.is_empty() {
        return Err("当前架构没有可用的 RTK 安装包".to_string());
    }

    let client = build_http_client(proxy_url);

    // 1. 获取目标 Release 信息
    let target_url = if tag_name.trim().is_empty() {
        format!("https://api.github.com/repos/{}/releases/latest", RTK_REPO)
    } else {
        format!("https://api.github.com/repos/{}/releases/tags/{}", RTK_REPO, tag_name)
    };

    let res = client
        .get(&target_url)
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .map_err(|e| format!("读取版本 {} 失败: {}", tag_name, e))?;

    if !res.status().is_success() {
        return Err(format!("读取版本 {} 失败: HTTP {}", tag_name, res.status()));
    }

    let release: GithubRelease = res.json().await.map_err(|e| format!("解析版本 JSON 失败: {}", e))?;

    let zip_asset = release
        .assets
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case(asset_name))
        .ok_or_else(|| format!("版本 {} 没有 Windows 安装包 {}", tag_name, asset_name))?;

    let checksum_asset = release
        .assets
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case("checksums.txt"))
        .ok_or_else(|| format!("版本 {} 没有校验文件 checksums.txt", tag_name))?;

    // 2. 下载并校验 checksums.txt
    let checksum_text = client
        .get(&checksum_asset.browser_download_url)
        .send()
        .await
        .map_err(|e| format!("下载校验文件失败: {}", e))?
        .text()
        .await
        .map_err(|e| format!("读取校验文件内容失败: {}", e))?;

    let expected_hash = parse_checksum(&checksum_text, asset_name)?;

    // 3. 下载 ZIP 文件到临时目录
    let temp_zip_path = std::env::temp_dir().join(format!("rtk_download_{}.zip", chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)));
    let zip_bytes = client
        .get(&zip_asset.browser_download_url)
        .send()
        .await
        .map_err(|e| format!("下载 RTK 安装包失败: {}", e))?
        .bytes()
        .await
        .map_err(|e| format!("接收 RTK 安装包失败: {}", e))?;

    fs::write(&temp_zip_path, &zip_bytes).map_err(|e| format!("保存临时安装包失败: {}", e))?;

    // 4. 计算并核对 SHA-256
    let mut hasher = Sha256::new();
    hasher.update(&zip_bytes);
    let actual_hash = hex::encode(hasher.finalize()).to_lowercase();

    if actual_hash != expected_hash {
        let _ = fs::remove_file(&temp_zip_path);
        return Err(format!("RTK 安装包 SHA-256 校验失败，期望 {}，实际 {}", expected_hash, actual_hash));
    }

    // 5. 解压到临时 staging 目录
    let parent_dir = install_dir.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent_dir).map_err(|e| format!("创建安装目录失败: {}", e))?;

    let staging_dir = parent_dir.join(format!(".rtk-staging-{}", chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)));
    let _ = fs::remove_dir_all(&staging_dir);
    fs::create_dir_all(&staging_dir).map_err(|e| format!("创建临时解压目录失败: {}", e))?;

    let extract_res = (|| -> Result<(), String> {
        let file = File::open(&temp_zip_path).map_err(|e| format!("打开 ZIP 失败: {}", e))?;
        let mut archive = ZipArchive::new(BufReader::new(file)).map_err(|e| format!("解析 ZIP 失败: {}", e))?;

        let mut seen_rtk_exe = false;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(|e| format!("读取 ZIP 条目 {} 失败: {}", i, e))?;
            if entry.is_dir() {
                continue;
            }
            let filename = match entry.enclosed_name() {
                Some(name) => name.file_name().unwrap_or(name.as_os_str()).to_os_string(),
                None => continue,
            };
            let base_name = filename.to_string_lossy();
            if base_name == "." || base_name.is_empty() || base_name == ".." || base_name.starts_with('.') {
                continue;
            }
            if base_name.eq_ignore_ascii_case("rtk.exe") {
                seen_rtk_exe = true;
            }
            let outpath = staging_dir.join(&filename);
            let mut outfile = File::create(&outpath).map_err(|e| format!("创建文件 {:?} 失败: {}", outpath, e))?;
            copy(&mut entry, &mut outfile).map_err(|e| format!("写入文件 {:?} 失败: {}", outpath, e))?;
        }

        if !seen_rtk_exe {
            return Err("RTK ZIP 中未找到 rtk.exe".to_string());
        }

        Ok(())
    })();

    let _ = fs::remove_file(&temp_zip_path);

    if let Err(err) = extract_res {
        let _ = fs::remove_dir_all(&staging_dir);
        return Err(err);
    }

    // 6. 原子替换并写入文档
    if install_dir.exists() {
        let _ = fs::remove_dir_all(install_dir);
    }

    if let Err(e) = fs::rename(&staging_dir, install_dir) {
        let _ = fs::remove_dir_all(&staging_dir);
        return Err(format!("重命名 RTK-AI 目录失败: {}", e));
    }

    // 7. 写入内置文档与版本标记
    let _ = fs::write(install_dir.join(".rtk-version"), format!("{}\n", release.tag_name));
    let _ = fs::write(install_dir.join("RTK-Codex-commands.md"), RTK_CODEX_COMMANDS);
    let _ = fs::write(install_dir.join("RTK-Codex-agent-instructions.md"), RTK_CODEX_AGENT_INSTRUCTIONS);
    let _ = fs::write(install_dir.join("RTK-Claude-agent-instructions.md"), RTK_CLAUDE_AGENT_INSTRUCTIONS);

    info!("RTK {} installed to {:?}", release.tag_name, install_dir);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_checksum() {
        let sample = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  rtk-x86_64-pc-windows-msvc.zip\n1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef  other.zip";
        let hash = parse_checksum(sample, "rtk-x86_64-pc-windows-msvc.zip").unwrap();
        assert_eq!(hash, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }
}
