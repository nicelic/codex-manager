package main

import (
	"archive/zip"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"path"
	"path/filepath"
	"runtime"
	"strings"
)

const snipGitHubReleasesURL = "https://api.github.com/repos/edouard-claude/snip/releases"

type snipReleaseOption struct {
	TagName     string `json:"tag_name"`
	Name        string `json:"name"`
	PublishedAt string `json:"published_at"`
	Prerelease  bool   `json:"prerelease"`
	Available   bool   `json:"available"`
	AssetName   string `json:"asset_name,omitempty"`
}

type snipReleaseListResponse struct {
	Releases []snipReleaseOption `json:"releases"`
	Page     int                 `json:"page"`
	PerPage  int                 `json:"per_page"`
	HasMore  bool                `json:"has_more"`
}

func snipAssetCandidate(name string) bool {
	if runtime.GOOS != "windows" || runtime.GOARCH != "amd64" {
		return false
	}
	lower := strings.ToLower(name)
	return strings.HasSuffix(lower, "_windows_amd64.zip")
}

func pickSnipAsset(release githubRelease) *llmtrimReleaseAsset {
	for i := range release.Assets {
		asset := &release.Assets[i]
		if snipAssetCandidate(asset.Name) {
			return asset
		}
	}
	return nil
}

func (g *gateway) snipReleases(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	page := 1
	if raw := strings.TrimSpace(r.URL.Query().Get("page")); raw != "" {
		if _, err := fmt.Sscanf(raw, "%d", &page); err != nil || page < 1 || page > 100 {
			http.Error(w, "page must be between 1 and 100", http.StatusBadRequest)
			return
		}
	}
	var releases []githubRelease
	if err := g.githubJSON(r.Context(), fmt.Sprintf("%s?per_page=5&page=%d", snipGitHubReleasesURL, page), &releases); err != nil {
		http.Error(w, "读取 snip 上游版本失败: "+err.Error(), http.StatusBadGateway)
		return
	}
	items := make([]snipReleaseOption, 0, len(releases))
	for _, release := range releases {
		asset := pickSnipAsset(release)
		items = append(items, snipReleaseOption{TagName: release.TagName, Name: release.Name, PublishedAt: release.PublishedAt.Format("2006-01-02T15:04:05Z07:00"), Prerelease: release.Prerelease, Available: asset != nil, AssetName: func() string {
			if asset == nil {
				return ""
			}
			return asset.Name
		}()})
	}
	writeJSON(w, http.StatusOK, snipReleaseListResponse{Releases: items, Page: page, PerPage: 5, HasMore: len(items) == 5})
}

func (g *gateway) downloadAndInstallSnip(ctx context.Context, tagName, installDir string) error {
	requestURL := snipGitHubReleasesURL + "/tags/" + url.PathEscape(tagName)
	var release githubRelease
	if err := g.githubJSON(ctx, requestURL, &release); err != nil {
		return err
	}
	asset := pickSnipAsset(release)
	if asset == nil {
		return errors.New("该版本没有可用的 Windows snip.exe 安装包")
	}
	var checksums *llmtrimReleaseAsset
	for index := range release.Assets {
		if release.Assets[index].Name == "checksums.txt" {
			checksums = &release.Assets[index]
			break
		}
	}
	if checksums == nil {
		return errors.New("该版本缺少 checksums.txt，无法校验 snip 安装包")
	}
	temporary, err := g.downloadTempFile(ctx, asset.BrowserDownloadURL, "snip-*.zip")
	if err != nil {
		return err
	}
	defer os.Remove(temporary)
	checksumText, err := g.downloadText(ctx, checksums.BrowserDownloadURL)
	if err != nil {
		return fmt.Errorf("下载 snip 校验文件失败: %w", err)
	}
	expected := ""
	for _, line := range strings.Split(checksumText, "\n") {
		if strings.Contains(line, asset.Name) {
			fields := strings.Fields(line)
			if len(fields) > 0 {
				expected = fields[0]
			}
			break
		}
	}
	actual, err := sha256File(temporary)
	if err != nil {
		return err
	}
	if expected == "" || !strings.EqualFold(expected, actual) {
		return fmt.Errorf("snip 安装包 SHA-256 校验失败")
	}
	if err := installSnipZIP(temporary, installDir); err != nil {
		return err
	}
	return nil
}

func installSnipZIP(zipPath, installDir string) error {
	archive, err := zip.OpenReader(zipPath)
	if err != nil {
		return err
	}
	defer archive.Close()
	staging, err := os.MkdirTemp(filepath.Dir(installDir), ".snip-install-*")
	if err != nil {
		return err
	}
	defer os.RemoveAll(staging)
	found := false
	for _, entry := range archive.File {
		if entry.FileInfo().IsDir() || !strings.EqualFold(path.Base(entry.Name), "snip.exe") {
			continue
		}
		if found {
			return errors.New("snip ZIP 中存在多个 snip.exe")
		}
		reader, err := entry.Open()
		if err != nil {
			return err
		}
		target := filepath.Join(staging, "snip.exe")
		output, err := os.OpenFile(target, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, 0o755)
		if err == nil {
			_, err = io.Copy(output, reader)
			closeErr := output.Close()
			if err == nil {
				err = closeErr
			}
		}
		_ = reader.Close()
		if err != nil {
			return err
		}
		found = true
	}
	if !found {
		return errors.New("snip ZIP 中没有 snip.exe")
	}
	if err := removeOwnedSnipDirectory(installDir); err != nil {
		return err
	}
	return os.Rename(staging, installDir)
}

func sha256File(path string) (string, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return "", err
	}
	hash := sha256.Sum256(data)
	return hex.EncodeToString(hash[:]), nil
}
