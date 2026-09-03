package main

import (
	"archive/zip"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log"
	"net/http"
	"net/url"
	"os"
	"path"
	"path/filepath"
	"regexp"
	"runtime"
	"strconv"
	"strings"
	"time"
)

const (
	llmtrimGitHubReleasesURL = "https://api.github.com/repos/fkiene/llmtrim/releases"
	llmtrimReleasePageSize   = 5
)

var sha256Pattern = regexp.MustCompile(`(?i)[a-f0-9]{64}`)

type llmtrimReleaseAsset struct {
	Name               string `json:"name"`
	BrowserDownloadURL string `json:"browser_download_url"`
}

type githubRelease struct {
	TagName     string                `json:"tag_name"`
	Name        string                `json:"name"`
	HTMLURL     string                `json:"html_url"`
	PublishedAt time.Time             `json:"published_at"`
	Prerelease  bool                  `json:"prerelease"`
	Draft       bool                  `json:"draft"`
	Assets      []llmtrimReleaseAsset `json:"assets"`
}

type llmtrimReleaseOption struct {
	TagName     string `json:"tag_name"`
	Name        string `json:"name"`
	PublishedAt string `json:"published_at"`
	Prerelease  bool   `json:"prerelease"`
	Available   bool   `json:"available"`
	AssetName   string `json:"asset_name,omitempty"`
}

type llmtrimReleaseListResponse struct {
	Releases []llmtrimReleaseOption `json:"releases"`
	Page     int                    `json:"page"`
	PerPage  int                    `json:"per_page"`
	HasMore  bool                   `json:"has_more"`
}

type llmtrimInstallRequest struct {
	TagName string `json:"tag_name"`
}

type llmtrimInstallResponse struct {
	Path       string `json:"path"`
	Version    string `json:"version"`
	Running    bool   `json:"running"`
	Configured bool   `json:"configured"`
	ProcessID  int    `json:"process_id,omitempty"`
	Port       string `json:"port"`
	Message    string `json:"message"`
}

type llmtrimUninstallResponse struct {
	Path            string   `json:"path"`
	Running         bool     `json:"running"`
	Configured      bool     `json:"configured"`
	DirectoryExists bool     `json:"directory_exists"`
	StateDirExists  bool     `json:"state_dir_exists"`
	TrayRunning     bool     `json:"tray_running"`
	Residual        bool     `json:"residual"`
	ProcessID       int      `json:"process_id,omitempty"`
	TrayProcessID   int      `json:"tray_process_id,omitempty"`
	Port            string   `json:"port"`
	Message         string   `json:"message"`
	Warnings        []string `json:"warnings,omitempty"`
}

var githubDirectTransport = func() *http.Transport {
	transport := http.DefaultTransport.(*http.Transport).Clone()
	transport.Proxy = nil
	return transport
}()

func newGitHubDirectClient(timeout time.Duration) *http.Client {
	return &http.Client{Transport: githubDirectTransport, Timeout: timeout}
}

func llmtrimWindowsTarget() string {
	switch runtime.GOARCH {
	case "arm64":
		return "aarch64-pc-windows-msvc"
	default:
		return "x86_64-pc-windows-msvc"
	}
}

func (g *gateway) llmtrimReleases(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	page := 1
	if rawPage := strings.TrimSpace(r.URL.Query().Get("page")); rawPage != "" {
		parsed, err := strconv.Atoi(rawPage)
		if err != nil || parsed < 1 || parsed > 100 {
			http.Error(w, "page must be between 1 and 100", http.StatusBadRequest)
			return
		}
		page = parsed
	}
	releases, err := g.fetchLLMTrimReleases(r.Context(), page)
	if err != nil {
		log.Printf("fetch llmtrim releases page=%d failed: %v", page, err)
		http.Error(w, "读取 llmtrim 上游版本失败: "+err.Error(), http.StatusBadGateway)
		return
	}
	writeJSON(w, http.StatusOK, llmtrimReleaseListResponse{
		Releases: releases,
		Page:     page,
		PerPage:  llmtrimReleasePageSize,
		HasMore:  len(releases) == llmtrimReleasePageSize,
	})
}

func (g *gateway) fetchLLMTrimReleases(ctx context.Context, page int) ([]llmtrimReleaseOption, error) {
	requestURL := fmt.Sprintf("%s?per_page=%d&page=%d", llmtrimGitHubReleasesURL, llmtrimReleasePageSize, page)
	var releases []githubRelease
	if err := g.githubJSON(ctx, requestURL, &releases); err != nil {
		return nil, err
	}
	options := make([]llmtrimReleaseOption, 0, len(releases))
	for _, release := range releases {
		if release.Draft || strings.TrimSpace(release.TagName) == "" {
			continue
		}
		assetName := llmtrimWindowsAssetName()
		available := false
		for _, asset := range release.Assets {
			if asset.Name == assetName {
				available = true
				break
			}
		}
		options = append(options, llmtrimReleaseOption{
			TagName:     release.TagName,
			Name:        release.Name,
			PublishedAt: release.PublishedAt.Format(time.RFC3339),
			Prerelease:  release.Prerelease,
			Available:   available,
			AssetName:   assetName,
		})
	}
	return options, nil
}

func (g *gateway) githubJSON(ctx context.Context, requestURL string, target any) error {
	client := newGitHubDirectClient(30 * time.Second)
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, requestURL, nil)
	if err != nil {
		return err
	}
	request.Header.Set("Accept", "application/vnd.github+json")
	request.Header.Set("User-Agent", "code-Manager-llmtrim-manager")
	response, err := client.Do(request)
	if err != nil {
		return err
	}
	defer response.Body.Close()
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		body, _ := io.ReadAll(io.LimitReader(response.Body, 4<<10))
		return fmt.Errorf("GitHub returned HTTP %d: %s", response.StatusCode, strings.TrimSpace(string(body)))
	}
	if err := json.NewDecoder(response.Body).Decode(target); err != nil {
		return fmt.Errorf("decode GitHub response: %w", err)
	}
	return nil
}

func llmtrimWindowsAssetName() string {
	return "llmtrim-" + llmtrimWindowsTarget() + ".zip"
}

func (g *gateway) downloadAndInstallLLMTrim(ctx context.Context, tagName string, installDir string) error {
	if strings.TrimSpace(tagName) == "" {
		return errors.New("tag_name is required")
	}
	requestURL := llmtrimGitHubReleasesURL + "/tags/" + url.PathEscape(tagName)
	var release githubRelease
	if err := g.githubJSON(ctx, requestURL, &release); err != nil {
		return fmt.Errorf("读取版本 %q 失败: %w", tagName, err)
	}
	zipName := llmtrimWindowsAssetName()
	var zipAsset, checksumAsset *llmtrimReleaseAsset
	for index := range release.Assets {
		asset := &release.Assets[index]
		switch asset.Name {
		case zipName:
			zipAsset = asset
		case strings.TrimSuffix(zipName, ".zip") + ".sha256", zipName + ".sha256":
			checksumAsset = asset
		}
	}
	if zipAsset == nil {
		return fmt.Errorf("版本 %q 没有 Windows 安装包 %s", tagName, zipName)
	}
	if checksumAsset == nil {
		return fmt.Errorf("版本 %q 没有校验文件 %s.sha256", tagName, zipName)
	}
	zipPath, err := g.downloadTempFile(ctx, zipAsset.BrowserDownloadURL, "llmtrim-*.zip")
	if err != nil {
		return fmt.Errorf("下载 Windows 安装包失败: %w", err)
	}
	defer os.Remove(zipPath)
	checksumText, err := g.downloadText(ctx, checksumAsset.BrowserDownloadURL)
	if err != nil {
		return fmt.Errorf("下载校验文件失败: %w", err)
	}
	expectedHash := sha256Pattern.FindString(checksumText)
	if expectedHash == "" {
		return errors.New("校验文件中没有找到 SHA-256 摘要")
	}
	actualHash, err := fileSHA256(zipPath)
	if err != nil {
		return fmt.Errorf("计算安装包校验失败: %w", err)
	}
	if !strings.EqualFold(expectedHash, actualHash) {
		return fmt.Errorf("安装包 SHA-256 校验失败，期望 %s，实际 %s", expectedHash, actualHash)
	}
	if err := installLLMTrimZip(zipPath, installDir); err != nil {
		return err
	}
	return nil
}

func (g *gateway) downloadTempFile(ctx context.Context, downloadURL string, pattern string) (string, error) {
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, downloadURL, nil)
	if err != nil {
		return "", err
	}
	request.Header.Set("Accept", "application/octet-stream")
	request.Header.Set("User-Agent", "code-Manager-llmtrim-manager")
	client := newGitHubDirectClient(2 * time.Minute)
	response, err := client.Do(request)
	if err != nil {
		return "", err
	}
	defer response.Body.Close()
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		return "", fmt.Errorf("download returned HTTP %d", response.StatusCode)
	}
	temporary, err := os.CreateTemp("", pattern)
	if err != nil {
		return "", err
	}
	temporaryPath := temporary.Name()
	keep := false
	defer func() {
		_ = temporary.Close()
		if !keep {
			_ = os.Remove(temporaryPath)
		}
	}()
	if _, err := io.Copy(temporary, io.LimitReader(response.Body, 256<<20)); err != nil {
		return "", err
	}
	if err := temporary.Close(); err != nil {
		return "", err
	}
	keep = true
	return temporaryPath, nil
}

func (g *gateway) downloadText(ctx context.Context, downloadURL string) (string, error) {
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, downloadURL, nil)
	if err != nil {
		return "", err
	}
	request.Header.Set("Accept", "text/plain")
	request.Header.Set("User-Agent", "code-Manager-llmtrim-manager")
	client := newGitHubDirectClient(30 * time.Second)
	response, err := client.Do(request)
	if err != nil {
		return "", err
	}
	defer response.Body.Close()
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		return "", fmt.Errorf("download returned HTTP %d", response.StatusCode)
	}
	body, err := io.ReadAll(io.LimitReader(response.Body, 16<<10))
	if err != nil {
		return "", err
	}
	return string(body), nil
}

func fileSHA256(filePath string) (string, error) {
	file, err := os.Open(filePath)
	if err != nil {
		return "", err
	}
	defer file.Close()
	hash := sha256.New()
	if _, err := io.Copy(hash, file); err != nil {
		return "", err
	}
	return hex.EncodeToString(hash.Sum(nil)), nil
}

func installLLMTrimZip(zipPath string, installDir string) error {
	archive, err := zip.OpenReader(zipPath)
	if err != nil {
		return fmt.Errorf("打开 llmtrim ZIP 失败: %w", err)
	}
	defer archive.Close()
	stagingDir, err := os.MkdirTemp(filepath.Dir(installDir), ".llmtrim-install-*")
	if err != nil {
		return fmt.Errorf("创建临时安装目录失败: %w", err)
	}
	defer os.RemoveAll(stagingDir)
	seen := map[string]bool{}
	fileCount := 0
	for _, entry := range archive.File {
		if entry.FileInfo().IsDir() {
			continue
		}
		name := strings.ReplaceAll(entry.Name, "\\", "/")
		baseName := path.Base(path.Clean(name))
		if baseName == "." || baseName == "" || baseName == ".." || strings.HasPrefix(baseName, ".") {
			continue
		}
		if seen[baseName] {
			return fmt.Errorf("ZIP 中存在重名文件 %q，无法安全解压", baseName)
		}
		seen[baseName] = true
		reader, err := entry.Open()
		if err != nil {
			return fmt.Errorf("读取 ZIP 文件 %q 失败: %w", entry.Name, err)
		}
		targetPath := filepath.Join(stagingDir, baseName)
		output, err := os.OpenFile(targetPath, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, 0755)
		if err == nil {
			_, err = io.Copy(output, reader)
			closeErr := output.Close()
			if err == nil {
				err = closeErr
			}
		}
		_ = reader.Close()
		if err != nil {
			return fmt.Errorf("解压 ZIP 文件 %q 失败: %w", entry.Name, err)
		}
		fileCount++
	}
	if !seen["llmtrim.exe"] {
		return errors.New("ZIP 中未找到 llmtrim.exe")
	}
	if fileCount == 0 {
		return errors.New("ZIP 中没有可安装文件")
	}
	if err := removeOwnedLLMTrimDirectory(installDir); err != nil {
		return fmt.Errorf("清理旧 llmtrim 目录失败: %w", err)
	}
	if err := os.Rename(stagingDir, installDir); err != nil {
		return fmt.Errorf("创建 llmtrim 目录失败: %w", err)
	}
	return nil
}

func copyFile(sourcePath string, targetPath string) error {
	source, err := os.Open(sourcePath)
	if err != nil {
		return err
	}
	defer source.Close()
	info, err := source.Stat()
	if err != nil {
		return err
	}
	target, err := os.OpenFile(targetPath, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, info.Mode().Perm())
	if err != nil {
		return err
	}
	if _, err := io.Copy(target, source); err != nil {
		_ = target.Close()
		return err
	}
	return target.Close()
}
