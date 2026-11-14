use std::fs::{self, File};
use std::io::{BufReader, copy};
use std::path::Path;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use zip::ZipArchive;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubRelease {
    pub tag_name: String,
    pub name: Option<String>,
    pub published_at: Option<String>,
    #[serde(default)]
    pub prerelease: Option<bool>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub assets: Vec<GithubReleaseAsset>,
}

pub fn parse_sha256_manifest(text: &str, asset_name: &str) -> Result<String, String> {
    let mut matched_hash = None;

    for line in text.lines() {
        let Some((hash, filename)) = line.trim().split_once(char::is_whitespace) else {
            continue;
        };
        let filename = filename.trim();
        let filename = filename.strip_prefix('*').unwrap_or(filename);
        let filename = filename.strip_prefix("./").unwrap_or(filename);
        if filename != asset_name {
            continue;
        }

        if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(format!("校验清单中 {} 的 SHA-256 摘要无效", asset_name));
        }
        if matched_hash.replace(hash.to_ascii_lowercase()).is_some() {
            return Err(format!("校验清单中 {} 存在重复条目", asset_name));
        }
    }

    matched_hash.ok_or_else(|| format!("校验清单中没有找到 {} 的 SHA-256 摘要", asset_name))
}

pub fn build_http_client(proxy_url: Option<&str>) -> Client {
    let mut builder = Client::builder()
        .user_agent(concat!("code-Manager/", env!("CARGO_PKG_VERSION"), " (Windows NT 10.0; Win64; x64)"))
        .timeout(std::time::Duration::from_secs(60));

    if let Some(proxy_str) = proxy_url {
        let trimmed = proxy_str.trim();
        if !trimmed.is_empty() {
            if let Ok(proxy) = reqwest::Proxy::all(trimmed) {
                builder = builder.proxy(proxy);
            } else {
                warn!("Failed to parse proxy URL: {}", trimmed);
            }
        }
    }

    builder.build().unwrap_or_else(|_| Client::new())
}

pub async fn fetch_github_releases(
    repo: &str,
    page: usize,
    per_page: usize,
    proxy_url: Option<&str>,
) -> Result<Vec<GithubRelease>, String> {
    let client = build_http_client(proxy_url);
    let url = format!(
        "https://api.github.com/repos/{}/releases?per_page={}&page={}",
        repo, per_page, page
    );

    let res = client
        .get(&url)
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .map_err(|e| format!("GitHub API 请求失败: {}", e))?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        return Err(format!("GitHub API 响应异常 ({}): {}", status, body));
    }

    let releases: Vec<GithubRelease> = res
        .json()
        .await
        .map_err(|e| format!("解析 GitHub releases JSON 失败: {}", e))?;

    Ok(releases)
}

pub async fn download_file(
    download_url: &str,
    dest_path: &Path,
    proxy_url: Option<&str>,
) -> Result<(), String> {
    let client = build_http_client(proxy_url);
    let res = client
        .get(download_url)
        .send()
        .await
        .map_err(|e| format!("下载请求失败 ({}): {}", download_url, e))?;

    if !res.status().is_success() {
        return Err(format!("下载响应错误: HTTP {}", res.status()));
    }

    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;
    }

    let bytes = res
        .bytes()
        .await
        .map_err(|e| format!("接收文件数据流失败: {}", e))?;

    fs::write(dest_path, &bytes).map_err(|e| format!("保存文件失败: {}", e))?;
    Ok(())
}

pub async fn download_file_with_hash(
    download_url: &str,
    dest_path: &Path,
    expected_sha256: Option<&str>,
    proxy_url: Option<&str>,
) -> Result<(), String> {
    let client = build_http_client(proxy_url);
    let res = client
        .get(download_url)
        .send()
        .await
        .map_err(|e| format!("下载请求失败 ({}): {}", download_url, e))?;

    if !res.status().is_success() {
        return Err(format!("下载响应错误: HTTP {}", res.status()));
    }

    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;
    }

    let bytes = res
        .bytes()
        .await
        .map_err(|e| format!("接收文件数据流失败: {}", e))?;

    // 检查 Windows EXE 大小与文件头特征
    let is_exe = dest_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|s| s.eq_ignore_ascii_case("exe"))
        .unwrap_or(false);

    if is_exe {
        if bytes.len() < 1024 * 1024 {
            return Err("下载的可执行文件大小异常（小于 1MB），可能下载不完整或返回了错误页".to_string());
        }
        if bytes.len() >= 2 && &bytes[0..2] != b"MZ" {
            return Err("下载的文件不是有效的 Windows 可执行文件（缺少 MZ 标识头）".to_string());
        }
    }

    if let Some(expected) = expected_sha256 {
        let expected_trimmed = expected.trim();
        if !expected_trimmed.is_empty() {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            let actual_hash = hex::encode(hasher.finalize());
            if !actual_hash.eq_ignore_ascii_case(expected_trimmed) {
                return Err(format!(
                    "文件 SHA-256 校验失败！预期: {}, 实际: {}",
                    expected_trimmed, actual_hash
                ));
            }
        }
    }

    fs::write(dest_path, &bytes).map_err(|e| format!("保存文件失败: {}", e))?;
    Ok(())
}

pub fn extract_zip<P: AsRef<Path>, D: AsRef<Path>>(zip_path: P, dest_dir: D) -> Result<(), String> {
    let file = File::open(zip_path.as_ref())
        .map_err(|e| format!("打开 ZIP 文件失败: {}", e))?;
    let mut archive = ZipArchive::new(BufReader::new(file))
        .map_err(|e| format!("读取 ZIP 格式失败: {}", e))?;

    fs::create_dir_all(dest_dir.as_ref()).map_err(|e| format!("创建目标解压目录失败: {}", e))?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("解压条目 {} 错误: {}", i, e))?;
        
        let outpath = match file.enclosed_name() {
            Some(path) => dest_dir.as_ref().join(path),
            None => continue,
        };

        if file.is_dir() {
            fs::create_dir_all(&outpath).map_err(|e| format!("创建子目录失败: {}", e))?;
        } else {
            if let Some(p) = outpath.parent() {
                if !p.exists() {
                    fs::create_dir_all(p).map_err(|e| format!("创建父目录失败: {}", e))?;
                }
            }
            let mut outfile = File::create(&outpath).map_err(|e| format!("创建解压文件失败: {}", e))?;
            copy(&mut file, &mut outfile).map_err(|e| format!("写入解压文件失败: {}", e))?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod checksum_tests {
    use super::parse_sha256_manifest;

    #[test]
    fn matches_only_the_complete_asset_name() {
        let expected = "a".repeat(64);
        let unrelated = "b".repeat(64);
        let asset = "tool_windows_amd64.zip";
        let manifest = format!(
            "{}  {}.sig\n{}  *./{}\n",
            unrelated, asset, expected, asset
        );

        assert_eq!(parse_sha256_manifest(&manifest, asset).unwrap(), expected);
        assert!(parse_sha256_manifest(&format!("{}  {}.sig", unrelated, asset), asset).is_err());
        assert!(parse_sha256_manifest(&expected, asset).is_err());
    }

    #[test]
    fn rejects_invalid_and_duplicate_entries() {
        let hash = "A".repeat(64);
        let asset = "tool_windows_amd64.zip";

        assert_eq!(
            parse_sha256_manifest(&format!("{}  {}", hash, asset), asset).unwrap(),
            hash.to_ascii_lowercase()
        );
        assert!(parse_sha256_manifest(&format!("invalid  {}", asset), asset).is_err());
        assert!(parse_sha256_manifest(
            &format!("{}  {}\n{}  {}", hash, asset, hash, asset),
            asset,
        ).is_err());
    }
}

pub async fn download_and_extract_zip(
    download_url: &str,
    dest_dir: &Path,
    proxy_url: Option<&str>,
) -> Result<(), String> {
    let temp_dir = std::env::temp_dir();
    let temp_zip = temp_dir.join(format!("cm_download_{}.zip", chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)));

    info!("Downloading {} to {:?}", download_url, temp_zip);
    download_file(download_url, &temp_zip, proxy_url).await?;

    info!("Extracting {:?} to {:?}", temp_zip, dest_dir);
    let result = extract_zip(&temp_zip, dest_dir);

    // Clean up temporary zip file
    let _ = fs::remove_file(&temp_zip);
    result
}
