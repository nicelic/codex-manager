//go:build windows

package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log"
	"net/http"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"
	"sync/atomic"
	"syscall"
	"time"

	"golang.org/x/sys/windows"
)

const (
	codeManagerGitHubReleasesURL = "https://api.github.com/repos/nicelic/codex-manager/releases"
	codeManagerReleasePageSize   = 5
	codeManagerExecutableName    = "code-Manager.exe"
	codeManagerUpdateStateName   = ".code-manager-update.json"
	codeManagerUpdateMaxBytes    = 512 << 20
)

var (
	codeManagerAssetDigestPattern = regexp.MustCompile(`(?i)^sha256:([a-f0-9]{64})$`)
	codeManagerUpdateInProgress   atomic.Bool
)

type codeManagerReleaseOption struct {
	TagName           string `json:"tag_name"`
	Name              string `json:"name"`
	PublishedAt       string `json:"published_at"`
	Prerelease        bool   `json:"prerelease"`
	Available         bool   `json:"available"`
	AssetName         string `json:"asset_name,omitempty"`
	UnavailableReason string `json:"unavailable_reason,omitempty"`
}

type codeManagerReleaseListResponse struct {
	Releases []codeManagerReleaseOption `json:"releases"`
	Page     int                        `json:"page"`
	PerPage  int                        `json:"per_page"`
	HasMore  bool                       `json:"has_more"`
}

type codeManagerUpdateRequest struct {
	TagName string `json:"tag_name"`
}

type codeManagerUpdateResponse struct {
	Accepted      bool   `json:"accepted"`
	TargetVersion string `json:"target_version"`
	Message       string `json:"message"`
}

type codeManagerUpdateState struct {
	ParentPID     int    `json:"parent_pid"`
	TargetVersion string `json:"target_version"`
	Executable    string `json:"executable"`
	Staged        string `json:"staged"`
	Backup        string `json:"backup"`
	ResumeProxy   bool   `json:"resume_proxy"`
	CreatedAt     string `json:"created_at"`
}

func (g *gateway) applicationReleases(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	if codeManagerUpdateInProgress.Load() {
		http.Error(w, "code-Manager 正在更新，暂不能加载远端版本", http.StatusConflict)
		return
	}
	page := 1
	if raw := strings.TrimSpace(r.URL.Query().Get("page")); raw != "" {
		var err error
		page, err = strconv.Atoi(raw)
		if err != nil || page < 1 || page > 100 {
			http.Error(w, "page must be between 1 and 100", http.StatusBadRequest)
			return
		}
	}
	releases, err := g.fetchApplicationReleases(r.Context(), page)
	if err != nil {
		http.Error(w, "读取 code-Manager 远端版本失败: "+err.Error(), http.StatusBadGateway)
		return
	}
	writeJSON(w, http.StatusOK, codeManagerReleaseListResponse{
		Releases: releases,
		Page:     page,
		PerPage:  codeManagerReleasePageSize,
		HasMore:  len(releases) == codeManagerReleasePageSize,
	})
}

func (g *gateway) fetchApplicationReleases(ctx context.Context, page int) ([]codeManagerReleaseOption, error) {
	requestURL := fmt.Sprintf("%s?per_page=%d&page=%d", codeManagerGitHubReleasesURL, codeManagerReleasePageSize, page)
	var releases []githubRelease
	if err := g.githubJSON(ctx, requestURL, &releases); err != nil {
		return nil, err
	}
	options := make([]codeManagerReleaseOption, 0, len(releases))
	for _, release := range releases {
		if release.Draft || strings.TrimSpace(release.TagName) == "" {
			continue
		}
		option := codeManagerReleaseOption{
			TagName:     release.TagName,
			Name:        release.Name,
			PublishedAt: release.PublishedAt.Format(time.RFC3339),
			Prerelease:  release.Prerelease,
			AssetName:   codeManagerExecutableName,
		}
		asset, ok := codeManagerReleaseAsset(release)
		if !ok {
			option.UnavailableReason = "没有 code-Manager.exe 附件"
		} else if _, err := codeManagerAssetSHA256(asset); err != nil {
			option.UnavailableReason = "附件缺少有效 SHA-256 摘要"
		} else {
			option.Available = true
		}
		options = append(options, option)
	}
	return options, nil
}

func codeManagerReleaseAsset(release githubRelease) (llmtrimReleaseAsset, bool) {
	for _, asset := range release.Assets {
		if asset.Name == codeManagerExecutableName {
			return asset, true
		}
	}
	return llmtrimReleaseAsset{}, false
}

func codeManagerAssetSHA256(asset llmtrimReleaseAsset) (string, error) {
	match := codeManagerAssetDigestPattern.FindStringSubmatch(strings.TrimSpace(asset.Digest))
	if len(match) != 2 {
		return "", errors.New("GitHub Release 附件未提供 sha256 digest")
	}
	return strings.ToLower(match[1]), nil
}

func (g *gateway) applicationUpdate(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	if g.application == nil {
		http.Error(w, "code-Manager 更新控制不可用", http.StatusServiceUnavailable)
		return
	}
	if !codeManagerUpdateInProgress.CompareAndSwap(false, true) {
		http.Error(w, "code-Manager 正在更新，请等待当前操作完成", http.StatusConflict)
		return
	}
	keepUpdating := false
	defer func() {
		if !keepUpdating {
			codeManagerUpdateInProgress.Store(false)
		}
	}()

	input := codeManagerUpdateRequest{}
	decoder := json.NewDecoder(http.MaxBytesReader(w, r.Body, 16<<10))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&input); err != nil {
		http.Error(w, "invalid JSON request", http.StatusBadRequest)
		return
	}
	input.TagName = strings.TrimSpace(input.TagName)
	if input.TagName == "" || len(input.TagName) > 128 || strings.ContainsAny(input.TagName, "\r\n") {
		http.Error(w, "版本标识无效", http.StatusBadRequest)
		return
	}

	executable, releaseDir, err := applicationUpdateTarget()
	if err != nil {
		http.Error(w, err.Error(), http.StatusConflict)
		return
	}
	ctx, cancel := context.WithTimeout(r.Context(), 5*time.Minute)
	defer cancel()
	var release githubRelease
	requestURL := fmt.Sprintf("%s/tags/%s", codeManagerGitHubReleasesURL, url.PathEscape(input.TagName))
	if err := g.githubJSON(ctx, requestURL, &release); err != nil {
		http.Error(w, "读取所选 code-Manager Release 失败: "+err.Error(), http.StatusBadGateway)
		return
	}
	if release.Draft || strings.TrimSpace(release.TagName) != input.TagName {
		http.Error(w, "所选版本不可安装", http.StatusBadRequest)
		return
	}
	asset, ok := codeManagerReleaseAsset(release)
	if !ok {
		http.Error(w, "所选版本没有 code-Manager.exe 附件", http.StatusBadRequest)
		return
	}
	expectedSHA256, err := codeManagerAssetSHA256(asset)
	if err != nil {
		http.Error(w, "所选附件无法验证: "+err.Error(), http.StatusBadGateway)
		return
	}
	stagedPath, err := g.downloadApplicationUpdate(ctx, asset.BrowserDownloadURL, releaseDir)
	if err != nil {
		http.Error(w, "下载 code-Manager 更新失败: "+err.Error(), http.StatusBadGateway)
		return
	}
	staged := true
	defer func() {
		if staged {
			_ = os.Remove(stagedPath)
		}
	}()
	actualSHA256, err := fileSHA256(stagedPath)
	if err != nil {
		http.Error(w, "计算更新文件 SHA-256 失败: "+err.Error(), http.StatusInternalServerError)
		return
	}
	if !strings.EqualFold(actualSHA256, expectedSHA256) {
		http.Error(w, fmt.Sprintf("更新文件 SHA-256 校验失败，期望 %s，实际 %s", expectedSHA256, actualSHA256), http.StatusBadGateway)
		return
	}
	if err := validateCodeManagerExecutable(stagedPath); err != nil {
		http.Error(w, "更新文件无效: "+err.Error(), http.StatusBadGateway)
		return
	}

	_, proxyState, _, _ := g.proxyStatusSnapshot()
	resumeProxy := proxyState == proxyStateRunning || proxyState == proxyStateConnecting
	if resumeProxy {
		stopContext, stopCancel := context.WithTimeout(context.Background(), 60*time.Second)
		stopErr := g.stopProxy(stopContext)
		stopCancel()
		if stopErr != nil {
			http.Error(w, "更新前停止代理失败: "+stopErr.Error(), http.StatusConflict)
			return
		}
		if running, state, _, _ := g.proxyStatusSnapshot(); running || state != proxyStateStopped {
			http.Error(w, "更新前代理未能完全停止", http.StatusConflict)
			return
		}
	}

	state := codeManagerUpdateState{
		ParentPID:     os.Getpid(),
		TargetVersion: release.TagName,
		Executable:    executable,
		Staged:        stagedPath,
		Backup:        filepath.Join(releaseDir, fmt.Sprintf(".code-manager-backup-%d.exe", time.Now().UnixNano())),
		ResumeProxy:   resumeProxy,
		CreatedAt:     time.Now().UTC().Format(time.RFC3339Nano),
	}
	statePath, err := writeCodeManagerUpdateState(state)
	if err != nil {
		if resumeProxy {
			if restartErr := g.startProxy(); restartErr != nil {
				log.Printf("更新准备失败后恢复代理失败: %v", restartErr)
			}
		}
		http.Error(w, "保存更新事务失败: "+err.Error(), http.StatusInternalServerError)
		return
	}
	if err := launchCodeManagerUpdatePowerShell(state, statePath); err != nil {
		_ = os.Remove(statePath)
		if resumeProxy {
			if restartErr := g.startProxy(); restartErr != nil {
				log.Printf("更新启动失败后恢复代理失败: %v", restartErr)
			}
		}
		http.Error(w, "启动更新程序失败: "+err.Error(), http.StatusInternalServerError)
		return
	}

	staged = false
	keepUpdating = true
	writeJSON(w, http.StatusAccepted, codeManagerUpdateResponse{
		Accepted:      true,
		TargetVersion: release.TagName,
		Message:       fmt.Sprintf("code-Manager %s 已下载并校验，正在停止管理页面、替换文件并重启。", release.TagName),
	})
	if flusher, ok := w.(http.Flusher); ok {
		flusher.Flush()
	}
	go func(app *application) {
		time.Sleep(250 * time.Millisecond)
		app.beginUpdateExit()
	}(g.application)
}

func applicationUpdateTarget() (string, string, error) {
	executable, err := currentCodeManagerExecutable()
	if err != nil {
		return "", "", err
	}
	if !strings.EqualFold(filepath.Base(executable), codeManagerExecutableName) {
		return "", "", fmt.Errorf("拒绝更新非 %s 文件: %s", codeManagerExecutableName, executable)
	}
	releaseDir := filepath.Dir(executable)
	developmentEntry, err := developmentDirectoryEntry(releaseDir)
	if err != nil {
		return "", "", err
	}
	if developmentEntry != "" {
		return "", "", fmt.Errorf("检测到开发环境文件 %s，已拒绝覆盖开发目录中的 EXE", developmentEntry)
	}
	return executable, releaseDir, nil
}

func (g *gateway) downloadApplicationUpdate(ctx context.Context, downloadURL, releaseDir string) (string, error) {
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, downloadURL, nil)
	if err != nil {
		return "", err
	}
	request.Header.Set("Accept", "application/octet-stream")
	request.Header.Set("User-Agent", "code-Manager-updater")
	response, err := newGitHubDirectClient(5 * time.Minute).Do(request)
	if err != nil {
		return "", err
	}
	defer response.Body.Close()
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		return "", fmt.Errorf("download returned HTTP %d", response.StatusCode)
	}
	temporary, err := os.CreateTemp(releaseDir, ".code-manager-update-*.new")
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
	written, err := io.Copy(temporary, io.LimitReader(response.Body, codeManagerUpdateMaxBytes+1))
	if err != nil {
		return "", err
	}
	if written > codeManagerUpdateMaxBytes {
		return "", fmt.Errorf("更新文件超过 %d MiB 限制", codeManagerUpdateMaxBytes>>20)
	}
	if err := temporary.Close(); err != nil {
		return "", err
	}
	keep = true
	return temporaryPath, nil
}

func validateCodeManagerExecutable(filePath string) error {
	info, err := os.Stat(filePath)
	if err != nil {
		return err
	}
	if info.IsDir() || info.Size() < 2 {
		return errors.New("文件不是有效的 Windows 可执行文件")
	}
	file, err := os.Open(filePath)
	if err != nil {
		return err
	}
	defer file.Close()
	magic := make([]byte, 2)
	if _, err := io.ReadFull(file, magic); err != nil {
		return err
	}
	if string(magic) != "MZ" {
		return errors.New("文件缺少 Windows PE MZ 标识")
	}
	return nil
}

func codeManagerUpdateStatePath(executable string) string {
	return filepath.Join(filepath.Dir(executable), runtimeConfigDirectoryName, codeManagerUpdateStateName)
}

func writeCodeManagerUpdateState(state codeManagerUpdateState) (string, error) {
	statePath := codeManagerUpdateStatePath(state.Executable)
	if err := os.MkdirAll(filepath.Dir(statePath), 0o700); err != nil {
		return "", err
	}
	content, err := json.Marshal(state)
	if err != nil {
		return "", err
	}
	temporary, err := os.CreateTemp(filepath.Dir(statePath), ".code-manager-update-state-*.tmp")
	if err != nil {
		return "", err
	}
	temporaryPath := temporary.Name()
	defer func() { _ = os.Remove(temporaryPath) }()
	if err := temporary.Chmod(0o600); err != nil {
		_ = temporary.Close()
		return "", err
	}
	if _, err := temporary.Write(append(content, '\n')); err != nil {
		_ = temporary.Close()
		return "", err
	}
	if err := temporary.Close(); err != nil {
		return "", err
	}
	if err := os.Remove(statePath); err != nil && !errors.Is(err, os.ErrNotExist) {
		return "", err
	}
	if err := os.Rename(temporaryPath, statePath); err != nil {
		return "", err
	}
	return statePath, nil
}

func applicationUpdatePendingForParent(parentPID uint32) bool {
	executable, err := currentCodeManagerExecutable()
	if err != nil {
		return false
	}
	content, err := os.ReadFile(codeManagerUpdateStatePath(executable))
	if err != nil {
		return false
	}
	var state codeManagerUpdateState
	if json.Unmarshal(content, &state) != nil {
		return false
	}
	return state.ParentPID == int(parentPID) && strings.EqualFold(filepath.Clean(state.Executable), filepath.Clean(executable))
}

func codeManagerUpdatePowerShellScript(state codeManagerUpdateState, statePath string) string {
	resumeProxy := "$false"
	if state.ResumeProxy {
		resumeProxy = "$true"
	}
	return fmt.Sprintf(`$ErrorActionPreference = 'Stop'
$parentPID = %d
$target = %s
$staged = %s
$backup = %s
$statePath = %s
$targetVersion = %s
$resumeProxy = %s
$originalMoved = $false
$replacementProcess = $null

function Start-CodeManager {
  param([string]$filePath)
  return Start-Process -FilePath $filePath -WorkingDirectory (Split-Path -LiteralPath $filePath -Parent) -PassThru
}

function Wait-CodeManagerReady {
  param([string]$expectedVersion, [bool]$allowLegacy = $false)
  $legacyDeadline = [DateTime]::UtcNow.AddSeconds(5)
  for ($attempt = 0; $attempt -lt 40; $attempt++) {
    try {
      $response = Invoke-WebRequest -UseBasicParsing -Uri 'http://127.0.0.1:7780/api/application/identity' -TimeoutSec 3
      $identity = $response.Content | ConvertFrom-Json
      $matchesExecutable = [string]::Equals([string]$identity.executable_path, $target, [StringComparison]::OrdinalIgnoreCase)
      if (-not $matchesExecutable) { return $false }
      $matchesVersion = $expectedVersion -eq '' -or [string]$identity.version -eq $expectedVersion
      return $matchesVersion
    } catch {}
    if ($replacementProcess -and $replacementProcess.HasExited) { return $false }
    if ($allowLegacy -and [DateTime]::UtcNow -ge $legacyDeadline) {
      try {
        Invoke-WebRequest -UseBasicParsing -Uri 'http://127.0.0.1:7780/healthz' -TimeoutSec 3 | Out-Null
        return $true
      } catch {}
    }
    Start-Sleep -Milliseconds 500
  }
  return $false
}

function Resume-Proxy {
  if (-not $resumeProxy) { return }
  $deadline = [DateTime]::UtcNow.AddSeconds(60)
  while ([DateTime]::UtcNow -lt $deadline) {
    try {
      Invoke-WebRequest -UseBasicParsing -Method Post -Uri 'http://127.0.0.1:7780/api/proxy/start' -TimeoutSec 60 | Out-Null
      return
    } catch {
      Start-Sleep -Seconds 1
    }
  }
}

function Restore-PreviousVersion {
  try {
    if ($replacementProcess -and -not $replacementProcess.HasExited) {
      Stop-Process -Id $replacementProcess.Id -Force -ErrorAction SilentlyContinue
      $replacementProcess.WaitForExit(10000) | Out-Null
    }
    $deadline = [DateTime]::UtcNow.AddSeconds(60)
    while ($true) {
      try {
        if (Test-Path -LiteralPath $target) { Remove-Item -LiteralPath $target -Force -ErrorAction Stop }
        if ($originalMoved) { Move-Item -LiteralPath $backup -Destination $target -Force -ErrorAction Stop }
        break
      } catch {
        if ([DateTime]::UtcNow -ge $deadline) { return $false }
        Start-Sleep -Milliseconds 250
      }
    }
    if (-not (Test-Path -LiteralPath $target)) { return $false }
    Start-CodeManager $target | Out-Null
    if (-not (Wait-CodeManagerReady '')) { return $false }
    Resume-Proxy
    return $true
  } catch {
    return $false
  }
}

try {
  try { Wait-Process -Id $parentPID -ErrorAction SilentlyContinue } catch {}
  $deadline = [DateTime]::UtcNow.AddSeconds(60)
  while ($true) {
    try {
      Move-Item -LiteralPath $target -Destination $backup -Force -ErrorAction Stop
      $originalMoved = $true
      break
    } catch {
      if ([DateTime]::UtcNow -ge $deadline) { throw '等待旧版 code-Manager.exe 释放文件锁超时。' }
      Start-Sleep -Milliseconds 250
    }
  }
  Move-Item -LiteralPath $staged -Destination $target -Force -ErrorAction Stop
  $replacementProcess = Start-CodeManager $target
  if (-not (Wait-CodeManagerReady $targetVersion $true)) { throw '新版 code-Manager 未通过启动或版本身份检查。' }
  Resume-Proxy
  Remove-Item -LiteralPath $backup -Force -ErrorAction SilentlyContinue
  Remove-Item -LiteralPath $statePath -Force -ErrorAction SilentlyContinue
} catch {
  if (Restore-PreviousVersion) {
    Remove-Item -LiteralPath $staged -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $statePath -Force -ErrorAction SilentlyContinue
  }
}
`, state.ParentPID, quotePowerShellString(state.Executable), quotePowerShellString(state.Staged), quotePowerShellString(state.Backup), quotePowerShellString(statePath), quotePowerShellString(state.TargetVersion), resumeProxy)
}

func launchCodeManagerUpdatePowerShell(state codeManagerUpdateState, statePath string) error {
	script := codeManagerUpdatePowerShellScript(state, statePath)
	command := exec.Command("cmd.exe", "/d", "/c", "start", "", "/b", "powershell.exe", "-NoLogo", "-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-EncodedCommand", encodePowerShellCommand(script))
	command.SysProcAttr = &syscall.SysProcAttr{CreationFlags: windows.CREATE_NO_WINDOW}
	if err := command.Start(); err != nil {
		return err
	}
	return command.Process.Release()
}
