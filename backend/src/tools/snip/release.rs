use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{copy, BufReader};
use std::path::Path;
use tracing::info;
use zip::ZipArchive;

use crate::common::windows::{canonicalize_clean, strip_windows_verbatim_prefix};

use super::types::{SnipReleaseListResponse, SnipReleaseOption};
use crate::common::downloader::{
    download_file, fetch_github_releases, parse_sha256_manifest, GithubRelease, GithubReleaseAsset,
};

const SNIP_REPO: &str = "edouard-claude/snip";
const SNIP_RELEASE_PAGE_SIZE: usize = 5;

pub fn snip_asset_candidate(name: &str) -> bool {
    let suffix = match std::env::consts::ARCH {
        "x86_64" => "_windows_amd64.zip",
        "aarch64" => "_windows_arm64.zip",
        _ => return false,
    };
    name.starts_with("snip_") && name.ends_with(suffix)
}

fn pick_snip_asset_for_arch<'a>(
    release: &'a GithubRelease,
    arch: &str,
) -> Option<&'a GithubReleaseAsset> {
    let arch = match arch {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        _ => return None,
    };
    let version = release.tag_name.strip_prefix('v')
        .or_else(|| release.tag_name.strip_prefix('V'))
        .unwrap_or(release.tag_name.as_str());
    if version.is_empty() {
        return None;
    }
    let expected = format!("snip_{}_windows_{}.zip", version, arch);
    release.assets.iter().find(|asset| asset.name == expected)
}

pub fn pick_snip_asset(release: &GithubRelease) -> Option<&GithubReleaseAsset> {
    pick_snip_asset_for_arch(release, std::env::consts::ARCH)
        .filter(|asset| snip_asset_candidate(&asset.name))
}

pub fn parse_checksum(text: &str, asset_name: &str) -> Result<String, String> {
    parse_sha256_manifest(text, asset_name)
}

pub async fn fetch_snip_releases(
    page: usize,
    proxy_url: Option<&str>,
) -> Result<SnipReleaseListResponse, String> {
    let releases = fetch_github_releases(SNIP_REPO, page, SNIP_RELEASE_PAGE_SIZE, proxy_url).await?;
    let has_more = releases.len() == SNIP_RELEASE_PAGE_SIZE;

    let options = releases
        .into_iter()
        .filter(|rel| !rel.tag_name.trim().is_empty())
        .map(|rel| {
            let asset = pick_snip_asset(&rel)
                .filter(|_| rel.assets.iter().any(|a| a.name == "checksums.txt"));
            let available = asset.is_some();
            let asset_name = asset.map(|a| a.name.clone()).unwrap_or_default();
            SnipReleaseOption {
                tag_name: rel.tag_name,
                name: rel.name.unwrap_or_default(),
                published_at: rel.published_at.unwrap_or_default(),
                prerelease: rel.prerelease.unwrap_or(false),
                available,
                asset_name,
            }
        })
        .collect();

    Ok(SnipReleaseListResponse {
        releases: options,
        page,
        per_page: SNIP_RELEASE_PAGE_SIZE,
        has_more,
    })
}

pub async fn download_and_install_snip(
    tag_name: &str,
    install_dir: &Path,
    proxy_url: Option<&str>,
) -> Result<(), String> {
    let releases = fetch_github_releases(SNIP_REPO, 1, 30, proxy_url).await?;
    let target_release = releases
        .into_iter()
        .find(|r| r.tag_name.eq_ignore_ascii_case(tag_name))
        .ok_or_else(|| format!("未找到指定的 Snip 发布版本: {}", tag_name))?;

    let asset = pick_snip_asset(&target_release)
        .ok_or_else(|| "该版本没有适用于当前架构的 Windows snip.exe 安装包".to_string())?;

    let checksum_asset = target_release
        .assets
        .iter()
        .find(|a| a.name == "checksums.txt")
        .ok_or_else(|| "该版本缺少 checksums.txt，无法校验 snip 安装包".to_string())?;

    let temp_dir = std::env::temp_dir();
    let temp_zip = temp_dir.join(format!(
        "snip_download_{}.zip",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    ));
    let temp_checksum = temp_dir.join(format!(
        "snip_checksum_{}.txt",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    ));

    info!("Downloading Snip checksums from {}", checksum_asset.browser_download_url);
    download_file(&checksum_asset.browser_download_url, &temp_checksum, proxy_url).await?;

    let checksum_text = fs::read_to_string(&temp_checksum);
    let _ = fs::remove_file(&temp_checksum);
    let checksum_text = checksum_text.map_err(|e| format!("读取 snip 校验文件失败: {}", e))?;
    let expected_hash = parse_checksum(&checksum_text, &asset.name)?;

    info!("Downloading Snip ZIP from {}", asset.browser_download_url);
    download_file(&asset.browser_download_url, &temp_zip, proxy_url).await?;

    let zip_bytes = fs::read(&temp_zip)
        .map_err(|e| format!("读取下载的 snip ZIP 文件失败: {}", e))?;
    let mut hasher = Sha256::new();
    hasher.update(&zip_bytes);
    let actual_hash = hex::encode(hasher.finalize()).to_ascii_lowercase();

    if expected_hash != actual_hash {
        let _ = fs::remove_file(&temp_zip);
        return Err("snip 安装包 SHA-256 校验失败".to_string());
    }

    let install_result = install_snip_zip(&temp_zip, install_dir);
    let _ = fs::remove_file(&temp_zip);
    install_result
}

#[cfg(test)]
mod release_tests {
    use super::*;

    fn asset(name: &str) -> GithubReleaseAsset {
        GithubReleaseAsset {
            name: name.to_string(),
            browser_download_url: String::new(),
            size: None,
        }
    }

    #[test]
    fn chooses_only_the_requested_version_and_architecture() {
        let release = GithubRelease {
            tag_name: "v0.26.0".to_string(),
            name: None,
            published_at: None,
            prerelease: None,
            body: None,
            assets: vec![
                asset("snip_other_0.26.0_windows_amd64.zip"),
                asset("snip_0.26.0_windows_arm64.zip"),
                asset("snip_0.26.0_windows_amd64.zip"),
            ],
        };

        assert_eq!(
            pick_snip_asset_for_arch(&release, "x86_64").unwrap().name,
            "snip_0.26.0_windows_amd64.zip"
        );
        assert_eq!(
            pick_snip_asset_for_arch(&release, "aarch64").unwrap().name,
            "snip_0.26.0_windows_arm64.zip"
        );
        assert!(pick_snip_asset_for_arch(&release, "x86").is_none());

        let wrong_version = GithubRelease {
            tag_name: "v0.25.0".to_string(),
            ..release
        };
        assert!(pick_snip_asset_for_arch(&wrong_version, "x86_64").is_none());
    }
}

fn install_snip_zip(zip_path: &Path, install_dir: &Path) -> Result<(), String> {
    let file = File::open(zip_path).map_err(|e| format!("打开 snip ZIP 失败: {}", e))?;
    let mut archive = ZipArchive::new(BufReader::new(file))
        .map_err(|e| format!("解析 snip ZIP 失败: {}", e))?;

    let parent_dir = install_dir.parent().unwrap_or_else(|| Path::new("."));
    let staging_dir = parent_dir.join(format!(
        ".snip-install-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    ));
    fs::create_dir_all(&staging_dir)
        .map_err(|e| format!("创建临时安装目录失败: {}", e))?;

    let mut found = false;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("读取 ZIP 条目失败: {}", e))?;

        if entry.is_dir() {
            continue;
        }

        let name = entry.name().to_string();
        let file_name = Path::new(&name).file_name().and_then(|f| f.to_str()).unwrap_or("");
        if file_name.eq_ignore_ascii_case("snip.exe") {
            if found {
                let _ = fs::remove_dir_all(&staging_dir);
                return Err("snip ZIP 中存在多个 snip.exe".to_string());
            }

            let target_exe = staging_dir.join("snip.exe");
            let mut out = File::create(&target_exe)
                .map_err(|e| format!("创建 snip.exe 失败: {}", e))?;
            copy(&mut entry, &mut out)
                .map_err(|e| format!("提取 snip.exe 失败: {}", e))?;
            found = true;
        }
    }

    if !found {
        let _ = fs::remove_dir_all(&staging_dir);
        return Err("snip ZIP 中没有 snip.exe".to_string());
    }

    if let Err(e) = remove_owned_snip_directory(install_dir) {
        let _ = fs::remove_dir_all(&staging_dir);
        return Err(e);
    }

    if let Err(e) = fs::rename(&staging_dir, install_dir) {
        let _ = fs::remove_dir_all(&staging_dir);
        return Err(format!("重命名 Snip 目录失败: {}", e));
    }

    Ok(())
}

pub fn remove_owned_snip_directory(directory: &Path) -> Result<(), String> {
    if !directory.exists() {
        return Ok(());
    }
    let canonical = canonicalize_clean(directory).unwrap_or_else(|_| strip_windows_verbatim_prefix(directory));
    let name = canonical.file_name().and_then(|f| f.to_str()).unwrap_or("");
    if !name.eq_ignore_ascii_case("snip") {
        return Err(format!("拒绝删除非 Snip 专属目录: {}", canonical.display()));
    }
    fs::remove_dir_all(&canonical).map_err(|e| format!("删除 Snip 目录失败: {}", e))
}
