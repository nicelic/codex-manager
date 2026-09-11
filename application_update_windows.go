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

	"golang.org/x/net/proxy"
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
	Releases       []codeManagerReleaseOption `json:"releases"`
	Page           int                        `json:"page"`
	PerPage        int                        `json:"per_page"`
	HasMore        bool                       `json:"has_more"`
	CurrentVersion string                     `json:"current_version"`
	LatestVersion  string                     `json:"latest_version"`
	HasUpdate      bool                       `json:"has_update"`
	IsDevMode      bool                       `json:"is_dev_mode"`
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

func compareVersions(v1, v2 string) int {
	clean1 := strings.TrimPrefix(strings.TrimSpace(v1), "v")
	clean2 := strings.TrimPrefix(strings.TrimSpace(v2), "v")
	parts1 := strings.Split(clean1, ".")
	parts2 := strings.Split(clean2, ".")
	maxLen := len(parts1)
	if len(parts2) > maxLen {
		maxLen = len(parts2)
	}
	for i := 0; i < maxLen; i++ {
		var n1, n2 int
		if i < len(parts1) {
			n1, _ = strconv.Atoi(parts1[i])
		}
		if i < len(parts2) {
			n2, _ = strconv.Atoi(parts2[i])
		}
		if n1 != n2 {
			return n1 - n2
		}
	}
	return 0
}

func (g *gateway) codeManagerGitHubClient(timeout time.Duration) *http.Client {
	outbound := strings.TrimSpace(g.currentConfig().OutboundProxy)
	if outbound == "" {
		return newGitHubDirectClient(timeout)
	}
	proxyURL, err := url.Parse(outbound)
	if err != nil || proxyURL.Host == "" {
		return newGitHubDirectClient(timeout)
	}
	transport := http.DefaultTransport.(*http.Transport).Clone()
	if proxyURL.Scheme == "socks5" {
		dialer, err := proxy.SOCKS5("tcp", proxyURL.Host, nil, proxy.Direct)
		if err == nil {
			transport.Proxy = nil
			transport.Dial = dialer.Dial
			return &http.Client{Transport: transport, Timeout: timeout}
		}
	} else if proxyURL.Scheme == "http" || proxyURL.Scheme == "https" {
		transport.Proxy = http.ProxyURL(proxyURL)
		return &http.Client{Transport: transport, Timeout: timeout}
	}
	return newGitHubDirectClient(timeout)
}

func (g *gateway) codeManagerGitHubJSON(ctx context.Context, requestURL string, target any) error {
	client := g.codeManagerGitHubClient(30 * time.Second)
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, requestURL, nil)
	if err != nil {
		return err
	}
	request.Header.Set("Accept", "application/vnd.github+json")
	request.Header.Set("User-Agent", "code-Manager-updater")
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
	currentVersion, _ := embeddedApplicationVersion()
	isDevMode := false
	if _, _, targetErr := applicationUpdateTarget(); targetErr != nil && strings.Contains(targetErr.Error(), "开发环境文件") {
		isDevMode = true
	}
	var latestVersion string
	hasUpdate := false
	for _, opt := range releases {
		if opt.Available {
			if latestVersion == "" {
				latestVersion = opt.TagName
			}
			if compareVersions(opt.TagName, currentVersion) > 0 {
				hasUpdate = true
			}
		}
	}
	writeJSON(w, http.StatusOK, codeManagerReleaseListResponse{
		Releases:       releases,
		Page:           page,
		PerPage:        codeManagerReleasePageSize,
		HasMore:        len(releases) == codeManagerReleasePageSize,
		CurrentVersion: currentVersion,
		LatestVersion:  latestVersion,
		HasUpdate:      hasUpdate,
		IsDevMode:      isDevMode,
	})
}

func (g *gateway) fetchApplicationReleases(ctx context.Context, page int) ([]codeManagerReleaseOption, error) {
	requestURL := fmt.Sprintf("%s?per_page=%d&page=%d", codeManagerGitHubReleasesURL, codeManagerReleasePageSize, page)
	var releases []githubRelease
	if err := g.codeManagerGitHubJSON(ctx, requestURL, &releases); err != nil {
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
		} else if _, err := codeManagerAssetSHA256(release, asset); err != nil {
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

func codeManagerAssetSHA256(release githubRelease, asset llmtrimReleaseAsset) (string, error) {
	if match := codeManagerAssetDigestPattern.FindStringSubmatch(strings.TrimSpace(asset.Digest)); len(match) == 2 {
		return strings.ToLower(match[1]), nil
	}
	if release.Body != "" {
		re := regexp.MustCompile(`(?i)(?:sha256|sha-256)[\s:=]+([a-f0-9]{64})`)
		if match := re.FindStringSubmatch(release.Body); len(match) == 2 {
			return strings.ToLower(match[1]), nil
		}
	}
	return "", errors.New("GitHub Release 附件未提供 sha256 digest")
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
	if err := g.codeManagerGitHubJSON(ctx, requestURL, &release); err != nil {
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
	expectedSHA256, err := codeManagerAssetSHA256(release, asset)
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

	stopGortexContext, stopGortexCancel := context.WithTimeout(context.Background(), 15*time.Second)
	_ = g.stopGortex(stopGortexContext)
	stopGortexCancel()

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
	if err := launchCodeManagerUpdateBat(state, statePath); err != nil {
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
	response, err := g.codeManagerGitHubClient(5 * time.Minute).Do(request)
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

const codeManagerUpdateBatTemplate = `@echo off
setlocal enabledelayedexpansion
chcp 65001 >nul

set "TARGET=%~1"
set "STAGED=%~2"
set "PARENT_PID=%~3"
set "STATE_PATH=%~4"
set "RESUME_PROXY=%~5"
set "RELEASE_DIR=%~dp1"
set "LOG_FILE=%RELEASE_DIR%config\code-Manager-update.log"
set "BACKUP=%RELEASE_DIR%.code-manager-backup.exe"

if not exist "%RELEASE_DIR%config" mkdir "%RELEASE_DIR%config" >nul 2>&1
echo [%date% %time%] Update batch started. Target="%TARGET%" Staged="%STAGED%" ParentPID=%PARENT_PID% >> "%LOG_FILE%"

:: 1. 等待主进程退出（最多 15 秒）
if not "%PARENT_PID%"=="" if not "%PARENT_PID%"=="0" (
    for /l %%i in (1,1,15) do (
        tasklist /fi "PID eq %PARENT_PID%" 2>nul | findstr /i "%PARENT_PID%" >nul
        if errorlevel 1 goto :parent_done
        timeout /t 1 /nobreak >nul
    )
)
:parent_done
echo [%date% %time%] Parent process confirmed stopped. >> "%LOG_FILE%"

:: 2. 彻底终止四大工具的所有进程
taskkill /F /IM gortex.exe /T 2>nul
taskkill /F /IM rtk.exe /T 2>nul
taskkill /F /IM snip.exe /T 2>nul
taskkill /F /IM llmtrim.exe /T 2>nul
taskkill /F /IM llmtrim-tray.exe /T 2>nul

:: 3. 彻底终止 code-Manager.exe 所有同名进程（释放文件锁）
taskkill /F /IM code-Manager.exe /T 2>nul
timeout /t 1 /nobreak >nul

:: 4. 覆盖替换程序（保留备份，循环重试最多 30 秒）
if not exist "%TARGET%" (
    echo [%date% %time%] Target does not exist, proceeding to copy. >> "%LOG_FILE%"
    goto :do_copy
)

set ORIGINAL_MOVED=0
for /l %%i in (1,1,30) do (
    move /y "%TARGET%" "%BACKUP%" >nul 2>&1
    if not errorlevel 1 (
        set ORIGINAL_MOVED=1
        echo [%date% %time%] Original EXE moved to backup. >> "%LOG_FILE%"
        goto :do_copy
    )
    timeout /t 1 /nobreak >nul
)

:do_copy
for /l %%i in (1,1,30) do (
    copy /y "%STAGED%" "%TARGET%" >nul 2>&1
    if not errorlevel 1 (
        echo [%date% %time%] Staged EXE copied to target successfully. >> "%LOG_FILE%"
        goto :replace_ok
    )
    timeout /t 1 /nobreak >nul
)

echo [%date% %time%] ERROR: Replace failed! Restoring backup... >> "%LOG_FILE%"
if "!ORIGINAL_MOVED!"=="1" if exist "%BACKUP%" (
    move /y "%BACKUP%" "%TARGET%" >nul 2>&1
)
goto :start_and_cleanup

:replace_ok
del /f /q "%BACKUP%" 2>nul
del /f /q "%STAGED%" 2>nul
if not "%STATE_PATH%"=="" del /f /q "%STATE_PATH%" 2>nul
echo [%date% %time%] Replace succeeded and temp files cleaned. >> "%LOG_FILE%"

:start_and_cleanup
:: 5. 按照名称启动目标 EXE
echo [%date% %time%] Launching new target EXE... >> "%LOG_FILE%"
cd /d "%RELEASE_DIR%"
start "" "%TARGET%"
echo [%date% %time%] New target EXE started. >> "%LOG_FILE%"

:: 6. 若需要恢复代理，等待服务就绪后发起调用
if "%RESUME_PROXY%"=="1" (
    timeout /t 3 /nobreak >nul
    curl -s -X POST http://127.0.0.1:7780/api/proxy/start >nul 2>&1
)

echo [%date% %time%] Update batch finished, self-deleting... >> "%LOG_FILE%"
(goto) 2>nul & del "%~f0"
`

func launchCodeManagerUpdateBat(state codeManagerUpdateState, statePath string) error {
	releaseDir := filepath.Dir(state.Executable)
	batPath := filepath.Join(releaseDir, fmt.Sprintf(".code-manager-update-%d.bat", time.Now().UnixNano()))
	if err := os.WriteFile(batPath, []byte(codeManagerUpdateBatTemplate), 0o700); err != nil {
		return fmt.Errorf("写入更新批处理脚本失败: %w", err)
	}

	resumeProxyArg := "0"
	if state.ResumeProxy {
		resumeProxyArg = "1"
	}

	command := exec.Command("cmd.exe", "/c", "start", "", "/min", batPath, state.Executable, state.Staged, strconv.Itoa(state.ParentPID), statePath, resumeProxyArg)
	command.Dir = releaseDir
	command.SysProcAttr = &syscall.SysProcAttr{CreationFlags: windows.CREATE_NEW_PROCESS_GROUP | windows.DETACHED_PROCESS}
	if err := command.Start(); err != nil {
		_ = os.Remove(batPath)
		return fmt.Errorf("启动更新批处理失败: %w", err)
	}
	return command.Process.Release()
}
