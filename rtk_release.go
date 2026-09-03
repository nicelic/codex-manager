package main

import (
	"archive/zip"
	"context"
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
	"strings"
	"time"
)

const (
	rtkGitHubReleasesURL = "https://api.github.com/repos/rtk-ai/rtk/releases"
	rtkReleasePageSize   = 5
)

var rtkChecksumPattern = regexp.MustCompile(`(?i)[a-f0-9]{64}`)

type rtkReleaseAsset struct {
	Name               string `json:"name"`
	BrowserDownloadURL string `json:"browser_download_url"`
}

type rtkReleaseOption struct {
	TagName     string `json:"tag_name"`
	Name        string `json:"name"`
	PublishedAt string `json:"published_at"`
	Prerelease  bool   `json:"prerelease"`
	Available   bool   `json:"available"`
	AssetName   string `json:"asset_name,omitempty"`
}

type rtkReleaseListResponse struct {
	Releases []rtkReleaseOption `json:"releases"`
	Page     int                `json:"page"`
	PerPage  int                `json:"per_page"`
	HasMore  bool               `json:"has_more"`
}

type rtkInstallRequest struct {
	TagName string `json:"tag_name"`
}

func rtkWindowsAssetName() string {
	if runtime.GOARCH != "amd64" {
		return ""
	}
	return "rtk-x86_64-pc-windows-msvc.zip"
}

func (g *gateway) rtkReleases(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	page := 1
	if rawPage := strings.TrimSpace(r.URL.Query().Get("page")); rawPage != "" {
		parsed, err := parseReleasePage(rawPage)
		if err != nil {
			http.Error(w, err.Error(), http.StatusBadRequest)
			return
		}
		page = parsed
	}
	if rtkWindowsAssetName() == "" {
		writeJSON(w, http.StatusOK, rtkReleaseListResponse{Releases: []rtkReleaseOption{}, Page: page, PerPage: rtkReleasePageSize, HasMore: false})
		return
	}
	releases, err := g.fetchRTKReleases(r.Context(), page)
	if err != nil {
		log.Printf("fetch rtk releases page=%d failed: %v", page, err)
		http.Error(w, "读取 RTK 上游版本失败: "+err.Error(), http.StatusBadGateway)
		return
	}
	writeJSON(w, http.StatusOK, rtkReleaseListResponse{Releases: releases, Page: page, PerPage: rtkReleasePageSize, HasMore: len(releases) == rtkReleasePageSize})
}

func parseReleasePage(raw string) (int, error) {
	var page int
	if _, err := fmt.Sscanf(raw, "%d", &page); err != nil || page < 1 || page > 100 || fmt.Sprintf("%d", page) != raw {
		return 0, errors.New("page must be between 1 and 100")
	}
	return page, nil
}

func (g *gateway) fetchRTKReleases(ctx context.Context, page int) ([]rtkReleaseOption, error) {
	requestURL := fmt.Sprintf("%s?per_page=%d&page=%d", rtkGitHubReleasesURL, rtkReleasePageSize, page)
	var releases []githubRelease
	if err := g.githubJSON(ctx, requestURL, &releases); err != nil {
		return nil, err
	}
	assetName := rtkWindowsAssetName()
	options := make([]rtkReleaseOption, 0, len(releases))
	for _, release := range releases {
		if release.Draft || strings.TrimSpace(release.TagName) == "" {
			continue
		}
		available := false
		for _, asset := range release.Assets {
			if asset.Name == assetName {
				available = true
				break
			}
		}
		options = append(options, rtkReleaseOption{
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

func (g *gateway) downloadAndInstallRTK(ctx context.Context, tagName, installDir string) error {
	if strings.TrimSpace(tagName) == "" {
		return errors.New("tag_name is required")
	}
	assetName := rtkWindowsAssetName()
	if assetName == "" {
		return errors.New("当前 Windows 架构没有可用的 RTK 安装包")
	}
	requestURL := rtkGitHubReleasesURL + "/tags/" + url.PathEscape(tagName)
	var release githubRelease
	if err := g.githubJSON(ctx, requestURL, &release); err != nil {
		return fmt.Errorf("读取版本 %q 失败: %w", tagName, err)
	}
	var zipAsset *llmtrimReleaseAsset
	var checksumAsset *llmtrimReleaseAsset
	for index := range release.Assets {
		asset := &release.Assets[index]
		switch asset.Name {
		case assetName:
			zipAsset = asset
		case "checksums.txt":
			checksumAsset = asset
		}
	}
	if zipAsset == nil {
		return fmt.Errorf("版本 %q 没有 Windows 安装包 %s", tagName, assetName)
	}
	if checksumAsset == nil {
		return fmt.Errorf("版本 %q 没有校验文件 checksums.txt", tagName)
	}
	zipPath, err := g.downloadTempFile(ctx, zipAsset.BrowserDownloadURL, "rtk-*.zip")
	if err != nil {
		return fmt.Errorf("下载 RTK Windows 安装包失败: %w", err)
	}
	defer os.Remove(zipPath)
	checksumText, err := g.downloadText(ctx, checksumAsset.BrowserDownloadURL)
	if err != nil {
		return fmt.Errorf("下载 RTK 校验文件失败: %w", err)
	}
	expectedHash, err := parseRTKChecksum(checksumText, assetName)
	if err != nil {
		return err
	}
	actualHash, err := fileSHA256(zipPath)
	if err != nil {
		return fmt.Errorf("计算 RTK 安装包校验失败: %w", err)
	}
	if !strings.EqualFold(expectedHash, actualHash) {
		return fmt.Errorf("RTK 安装包 SHA-256 校验失败，期望 %s，实际 %s", expectedHash, actualHash)
	}
	if err := installRTKZip(zipPath, installDir); err != nil {
		return err
	}
	return nil
}

func parseRTKChecksum(text, assetName string) (string, error) {
	for _, line := range strings.Split(text, "\n") {
		if !strings.Contains(line, assetName) {
			continue
		}
		if hash := rtkChecksumPattern.FindString(line); hash != "" {
			return hash, nil
		}
	}
	return "", fmt.Errorf("checksums.txt 中没有找到 %s 的 SHA-256 摘要", assetName)
}

func installRTKZip(zipPath, installDir string) error {
	archive, err := zip.OpenReader(zipPath)
	if err != nil {
		return fmt.Errorf("打开 RTK ZIP 失败: %w", err)
	}
	defer archive.Close()
	stagingDir, err := os.MkdirTemp(filepath.Dir(installDir), ".rtk-install-*")
	if err != nil {
		return fmt.Errorf("创建 RTK 临时安装目录失败: %w", err)
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
			return fmt.Errorf("RTK ZIP 中存在重名文件 %q，无法安全解压", baseName)
		}
		seen[baseName] = true
		reader, err := entry.Open()
		if err != nil {
			return fmt.Errorf("读取 RTK ZIP 文件 %q 失败: %w", entry.Name, err)
		}
		output, err := os.OpenFile(filepath.Join(stagingDir, baseName), os.O_CREATE|os.O_WRONLY|os.O_TRUNC, 0755)
		if err == nil {
			_, err = io.Copy(output, reader)
			closeErr := output.Close()
			if err == nil {
				err = closeErr
			}
		}
		_ = reader.Close()
		if err != nil {
			return fmt.Errorf("解压 RTK ZIP 文件 %q 失败: %w", entry.Name, err)
		}
		fileCount++
	}
	if !seen["rtk.exe"] {
		return errors.New("RTK ZIP 中未找到 rtk.exe")
	}
	if fileCount == 0 {
		return errors.New("RTK ZIP 中没有可安装文件")
	}
	if err := removeOwnedRTKDirectory(installDir); err != nil {
		return fmt.Errorf("清理旧 RTK-AI 目录失败: %w", err)
	}
	if err := os.Rename(stagingDir, installDir); err != nil {
		return fmt.Errorf("创建 RTK-AI 目录失败: %w", err)
	}
	return nil
}
