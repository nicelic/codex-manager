package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"syscall"
	"time"

	"golang.org/x/sys/windows/registry"
)

const llmtrimProxyURL = "http://127.0.0.1:43117"
const llmtrimNoProxy = "localhost,127.0.0.1,::1,10.0.0.0/8,172.16.0.0/12,192.168.0.0/16,169.254.0.0/16,fd00::/8,*.local"
const llmtrimExecutableName = "llmtrim.exe"
const llmtrimTrayExecutableName = "llmtrim-tray.exe"
const llmtrimRunKey = `Software\Microsoft\Windows\CurrentVersion\Run`
const llmtrimStartupApprovedKey = `Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run`

var llmtrimVersionCache = struct {
	sync.Mutex
	values map[string]string
}{values: make(map[string]string)}

func llmtrimVersionMarkerPath(installDir string) string {
	return filepath.Join(installDir, ".llmtrim-version")
}

func readLLMTrimVersion(installDir string) string {
	data, err := os.ReadFile(llmtrimVersionMarkerPath(installDir))
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(data))
}

func writeLLMTrimVersion(installDir, version string) error {
	temporary, err := os.CreateTemp(installDir, ".llmtrim-version-*")
	if err != nil {
		return err
	}
	temporaryPath := temporary.Name()
	defer os.Remove(temporaryPath)
	if _, err := temporary.WriteString(strings.TrimSpace(version) + "\n"); err != nil {
		_ = temporary.Close()
		return err
	}
	if err := temporary.Close(); err != nil {
		return err
	}
	return os.Rename(temporaryPath, llmtrimVersionMarkerPath(installDir))
}

func currentLLMTrimVersion(installDir, configuredPath string) string {
	managedPath := filepath.Join(installDir, llmtrimExecutableName)
	candidate := managedPath
	if llmtrimExecutableExists(configuredPath) {
		candidate = configuredPath
	} else if !llmtrimExecutableExists(managedPath) {
		return ""
	}
	isManagedPath := strings.EqualFold(filepath.Clean(candidate), filepath.Clean(managedPath))
	if isManagedPath {
		if version := readLLMTrimVersion(installDir); version != "" {
			return version
		}
	}
	version := queryLLMTrimBinaryVersion(candidate)
	if version != "" && isManagedPath {
		if err := writeLLMTrimVersion(installDir, version); err != nil {
			log.Printf("保存 llmtrim 版本标记失败: %v", err)
		}
	}
	return version
}

func queryLLMTrimBinaryVersion(executable string) string {
	absolute, err := filepath.Abs(strings.TrimSpace(executable))
	if err != nil || absolute == "" {
		return ""
	}
	cacheKey := strings.ToLower(filepath.Clean(absolute))
	llmtrimVersionCache.Lock()
	if version := llmtrimVersionCache.values[cacheKey]; version != "" {
		llmtrimVersionCache.Unlock()
		return version
	}
	llmtrimVersionCache.Unlock()

	command := exec.Command(absolute, "--version")
	command.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	output, err := command.Output()
	if err != nil {
		return ""
	}
	version := ""
	for _, line := range strings.Split(string(output), "\n") {
		line = strings.TrimSpace(line)
		if line == "" {
			continue
		}
		if strings.HasPrefix(strings.ToLower(line), "llmtrim ") {
			line = strings.TrimSpace(line[len("llmtrim "):])
		}
		version = line
		break
	}
	if version == "" {
		return ""
	}
	llmtrimVersionCache.Lock()
	llmtrimVersionCache.values[cacheKey] = version
	llmtrimVersionCache.Unlock()
	return version
}

func initializeLLMTrimPath(configPath string, config *Config) error {
	if config == nil || strings.TrimSpace(config.LLMTrimPath) != "" {
		return nil
	}
	pathValue, processID := findRunningProcessByName("llmtrim.exe")
	if pathValue == "" {
		log.Print("llmtrim first-start discovery: no running llmtrim.exe found")
		return nil
	}
	validatedPath, err := validateLLMTrimPath(pathValue)
	if err != nil {
		return fmt.Errorf("发现运行中的 llmtrim.exe 但路径无效: %w", err)
	}
	if err := updateConfigValue(configPath, "llmtrim_path", validatedPath); err != nil {
		return fmt.Errorf("保存发现的 llmtrim_path 失败: %w", err)
	}
	config.LLMTrimPath = validatedPath
	log.Printf("llmtrim first-start discovery: found PID %d at %s", processID, validatedPath)
	return nil
}

func (g *gateway) llmtrimInstall(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	if err := g.llmtrimStatusViewer.stop(); err != nil {
		http.Error(w, "安装前关闭 llmtrim 状态窗口失败: "+err.Error(), http.StatusBadGateway)
		return
	}
	if g.isShuttingDown() {
		http.Error(w, "code-Manager 正在退出，不能安装 llmtrim", http.StatusServiceUnavailable)
		return
	}
	var input llmtrimInstallRequest
	decoder := json.NewDecoder(http.MaxBytesReader(w, r.Body, 8<<10))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&input); err != nil {
		http.Error(w, "invalid JSON request", http.StatusBadRequest)
		return
	}
	if err := decoder.Decode(&struct{}{}); err != io.EOF {
		http.Error(w, "invalid JSON request", http.StatusBadRequest)
		return
	}
	input.TagName = strings.TrimSpace(input.TagName)
	if input.TagName == "" {
		http.Error(w, "请选择要安装的 llmtrim 版本", http.StatusBadRequest)
		return
	}

	g.llmtrimMu.Lock()
	defer g.llmtrimMu.Unlock()
	if g.isShuttingDown() {
		http.Error(w, "code-Manager 正在退出，不能安装 llmtrim", http.StatusServiceUnavailable)
		return
	}
	installDir, err := llmtrimInstallDirectory()
	if err != nil {
		http.Error(w, err.Error(), http.StatusInternalServerError)
		return
	}
	installedPath := filepath.Join(installDir, "llmtrim.exe")
	configuredPath := configuredPathForCleanup(g.currentConfig().LLMTrimPath)
	if configuredPath == "" {
		configuredPath = discoverLLMTrimProcessPath()
	}
	if err := g.stopManagedLLMTrimForInstall(r.Context(), installDir, configuredPath); err != nil {
		log.Printf("stop existing llmtrim before install failed: %v", err)
		http.Error(w, "安装前停止现有 llmtrim 失败: "+err.Error(), http.StatusBadGateway)
		return
	}
	installContext, cancel := context.WithTimeout(r.Context(), 3*time.Minute)
	defer cancel()
	migrated, err := g.cleanupLLMTrimEnvironment(installContext, installDir, configuredPath)
	if err != nil {
		log.Printf("cleanup llmtrim before install failed: %v", err)
		http.Error(w, "安装前清理 llmtrim 环境失败: "+err.Error(), http.StatusBadGateway)
		return
	}
	if err := g.downloadAndInstallLLMTrim(installContext, input.TagName, installDir); err != nil {
		log.Printf("install llmtrim version=%s failed: %v", input.TagName, err)
		http.Error(w, "安装 llmtrim 失败: "+err.Error(), http.StatusBadGateway)
		return
	}
	if err := writeLLMTrimVersion(installDir, input.TagName); err != nil {
		g.rollbackLLMTrimInstallation(installDir, installedPath)
		http.Error(w, "保存 llmtrim 版本信息失败: "+err.Error(), http.StatusInternalServerError)
		return
	}
	if err := g.persistLLMTrimPath(installedPath); err != nil {
		g.rollbackLLMTrimInstallation(installDir, installedPath)
		http.Error(w, "保存 llmtrim_path 失败: "+err.Error(), http.StatusInternalServerError)
		return
	}
	if err := writeManagedToolState(installDir, managedToolState{DesiredRunning: false, Running: false}); err != nil {
		g.rollbackLLMTrimInstallation(installDir, installedPath)
		http.Error(w, "保存 llmtrim 状态失败: "+err.Error(), http.StatusInternalServerError)
		return
	}
	if err := recordManagedToolInstallation("llmtrim", installDir); err != nil {
		g.rollbackLLMTrimInstallation(installDir, installedPath)
		http.Error(w, "保存 llmtrim 安装记录失败: "+err.Error(), http.StatusInternalServerError)
		return
	}
	trackingDatabasePath, err := llmtrimTrackingDatabasePath(installDir)
	if err != nil {
		g.rollbackLLMTrimInstallation(installDir, installedPath)
		http.Error(w, "配置 llmtrim 统计数据库路径失败: "+err.Error(), http.StatusInternalServerError)
		return
	}
	configMutation, err := syncManagedLLMTrimConfigWithDatabase(g.currentConfig().UpstreamBaseURL, trackingDatabasePath)
	if err != nil {
		g.rollbackLLMTrimInstallation(installDir, installedPath)
		http.Error(w, "写入 llmtrim 受管配置失败: "+err.Error(), http.StatusInternalServerError)
		return
	}
	keepManagedConfig := false
	defer func() {
		if keepManagedConfig {
			return
		}
		if rollbackErr := configMutation.rollback(); rollbackErr != nil {
			log.Printf("回滚 llmtrim 受管配置失败: %v", rollbackErr)
		}
	}()
	if err := executeLLMTrimCommandWithTimeout(installContext, installedPath, 45*time.Second, "setup"); err != nil {
		log.Printf("llmtrim setup version=%s failed: %v", input.TagName, err)
		g.rollbackLLMTrimInstallation(installDir, installedPath)
		http.Error(w, "llmtrim setup 失败: "+err.Error(), http.StatusBadGateway)
		return
	}
	configured, configurationMessage := verifyLLMTrimWindowsSetup()
	if !configured {
		g.rollbackLLMTrimInstallation(installDir, installedPath)
		http.Error(w, "llmtrim setup 完成，但 Windows 配置校验失败: "+configurationMessage, http.StatusBadGateway)
		return
	}
	if !waitForLLMTrimState(installedPath, true, 3*time.Second) {
		if err := executeLLMTrimCommand(installContext, installedPath, "setup"); err != nil {
			log.Printf("llmtrim setup retry after setup version=%s failed: %v", input.TagName, err)
			g.rollbackLLMTrimInstallation(installDir, installedPath)
			http.Error(w, "llmtrim setup 已完成，但再次启动失败: "+err.Error(), http.StatusBadGateway)
			return
		}
	}
	if !waitForLLMTrimState(installedPath, true, 8*time.Second) {
		running, processID, message, _ := queryLLMTrimRunning(installedPath)
		g.rollbackLLMTrimInstallation(installDir, installedPath)
		http.Error(w, fmt.Sprintf("llmtrim 已完成 Windows 配置，但未能启动（running=%t, pid=%d, port=%s）：%s", running, processID, llmtrimDaemonAddress, message), http.StatusBadGateway)
		return
	}
	if err := writeManagedToolState(installDir, managedToolState{DesiredRunning: true, Running: true}); err != nil {
		log.Printf("保存 llmtrim 状态文件失败: %v", err)
	}
	_, processID, _, _ := queryLLMTrimRunning(installedPath)
	keepManagedConfig = true
	writeJSON(w, http.StatusOK, llmtrimInstallResponse{
		Path:       installedPath,
		Version:    input.TagName,
		Running:    true,
		Configured: true,
		ProcessID:  processID,
		Port:       llmtrimDaemonAddress,
		Message: func() string {
			message := "llmtrim 已安装到程序目录的 llmtrim 文件夹，统计库将写入该目录的 tracking.db，Windows 代理配置已验证，daemon 已启动。"
			if migrated {
				message += " 已自动清理本程序旧版本留下的受管 llmtrim 状态。"
			}
			return message
		}(),
	})
}

func (g *gateway) llmtrimUninstall(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	if err := g.llmtrimStatusViewer.stop(); err != nil {
		http.Error(w, "删除前关闭 llmtrim 状态窗口失败: "+err.Error(), http.StatusBadGateway)
		return
	}
	if g.isShuttingDown() {
		http.Error(w, "code-Manager 正在退出，不能删除 llmtrim", http.StatusServiceUnavailable)
		return
	}

	commandContext, cancel := context.WithTimeout(r.Context(), 30*time.Second)
	defer cancel()
	if err := g.uninstallLLMTrim(commandContext); err != nil {
		http.Error(w, "删除 llmtrim 失败: "+err.Error(), http.StatusBadGateway)
		return
	}
	writeJSON(w, http.StatusOK, llmtrimUninstallResponse{
		Path:            "",
		Running:         false,
		Configured:      false,
		DirectoryExists: false,
		StateDirExists:  false,
		TrayRunning:     false,
		Residual:        false,
		Port:            llmtrimDaemonAddress,
		Message:         "llmtrim 和 llmtrim-tray 已停止，code-Manager 受管的安装目录、统计数据库、环境、自启动、用户 CA、状态目录和 llmtrim 配置目录已清理。",
	})
}

func (g *gateway) uninstallLLMTrim(ctx context.Context) error {
	g.llmtrimMu.Lock()
	defer g.llmtrimMu.Unlock()
	if g.isShuttingDown() {
		return errors.New("code-Manager 正在退出，不能删除 llmtrim")
	}
	config := g.currentConfig()
	configuredPath := strings.TrimSpace(config.LLMTrimPath)
	if configuredPath == "" {
		configuredPath = discoverLLMTrimProcessPath()
	}
	installDir, err := llmtrimInstallDirectory()
	if err != nil {
		return err
	}
	if err := g.stopManagedLLMTrimForInstall(ctx, installDir, configuredPath); err != nil {
		log.Printf("stop llmtrim before uninstall failed: %v", err)
		return fmt.Errorf("删除前停止 llmtrim 和 llmtrim-tray 失败: %w", err)
	}
	if _, err := removeLLMTrimDefaultTrackingDatabase(); err != nil {
		return fmt.Errorf("删除历史 llmtrim 共享统计数据库失败: %w", err)
	}
	if _, err := g.cleanupLLMTrimEnvironment(ctx, installDir, configuredPath); err != nil {
		log.Printf("uninstall llmtrim failed: %v", err)
		return fmt.Errorf("删除 llmtrim 失败: %w", err)
	}
	if err := removeManagedLLMTrimConfigDirectory(); err != nil {
		return fmt.Errorf("删除受管 llmtrim 配置目录失败: %w", err)
	}
	if g.configPath != "" {
		if _, statErr := os.Stat(g.configPath); statErr == nil {
			if err := g.persistLLMTrimPath(""); err != nil {
				return fmt.Errorf("清理完成但无法清空 llmtrim_path: %w", err)
			}
		} else if !errors.Is(statErr, os.ErrNotExist) {
			return fmt.Errorf("检查 config.yaml 失败: %w", statErr)
		}
	}
	_ = removeManagedToolState(installDir)
	if err := forgetManagedToolInstallation("llmtrim", installDir); err != nil {
		log.Printf("移除 llmtrim 安装记录失败: %v", err)
	}
	return nil
}

// removeLLMTrimDefaultTrackingDatabase 在用户从管理页确认彻底删除时，清理
// llmtrim 0.13.x 未配置 db_path 时使用的共享账本。文件名和父目录均会被严格校验，
// 只会删除 tracking.db 及 SQLite 的 WAL/SHM 伴随文件。
func removeLLMTrimDefaultTrackingDatabase() (bool, error) {
	databasePath, err := llmtrimDefaultTrackingDatabasePath()
	if err != nil {
		return false, err
	}
	return removeLLMTrimTrackingDatabase(databasePath)
}

func configuredPathForCleanup(pathValue string) string {
	return strings.TrimSpace(pathValue)
}

func discoverLLMTrimProcessPath() string {
	for _, name := range []string{llmtrimExecutableName, llmtrimTrayExecutableName} {
		if pathValue, _ := findRunningProcessByName(name); pathValue != "" {
			return pathValue
		}
	}
	return ""
}

func (g *gateway) cleanupLLMTrimEnvironment(ctx context.Context, installDir, configuredPath string) (bool, error) {
	managedDirectories, err := managedLLMTrimInstallations(installDir, configuredPath)
	if err != nil {
		return false, err
	}
	managedGlobalState := len(managedDirectories) > 0
	if managedGlobalState {
		if err := removeLLMTrimAutostart(); err != nil {
			return false, fmt.Errorf("清理自启动注册表失败: %w", err)
		}
		if err := clearLLMTrimUserEnvironment(); err != nil {
			return false, fmt.Errorf("清理 Windows 用户环境失败: %w", err)
		}
		if _, err := removeLLMTrimUserTrust(); err != nil {
			return false, fmt.Errorf("移除 llmtrim CA 信任失败: %w", err)
		}
		if residual, err := queryLLMTrimAutostartResidual(); err != nil {
			return false, fmt.Errorf("校验自启动清理失败: %w", err)
		} else if residual {
			return false, errors.New("自启动注册表仍存在 llmtrim 条目")
		}
		if residual, err := queryLLMTrimEnvironmentResidual(); err != nil {
			return false, fmt.Errorf("校验 Windows 用户环境清理失败: %w", err)
		} else if residual {
			return false, errors.New("Windows 用户环境仍存在 llmtrim 配置")
		}
	}
	for _, directory := range managedDirectories {
		if err := removeOwnedLLMTrimDirectory(directory); err != nil {
			return false, fmt.Errorf("删除受管 llmtrim 文件夹失败（%s）: %w", directory, err)
		}
		if err := forgetManagedToolInstallation("llmtrim", directory); err != nil {
			log.Printf("移除旧 llmtrim 安装记录失败（%s）: %v", directory, err)
		}
	}
	if err := removeOwnedLLMTrimDirectory(installDir); err != nil {
		return false, fmt.Errorf("删除项目 llmtrim 文件夹失败: %w", err)
	}
	if err := removeConfiguredLLMTrimFiles(configuredPath, installDir); err != nil {
		return false, fmt.Errorf("清理已有 llmtrim 文件失败: %w", err)
	}
	if managedGlobalState {
		userProfile := strings.TrimSpace(os.Getenv("USERPROFILE"))
		if userProfile == "" {
			return false, errors.New("USERPROFILE 未设置，无法定位 llmtrim 状态目录")
		}
		stateDir := filepath.Join(userProfile, ".llmtrim")
		if err := removeOwnedLLMTrimDirectory(stateDir); err != nil {
			return false, fmt.Errorf("删除 llmtrim 状态目录失败: %w", err)
		}
		if _, err := os.Stat(stateDir); !errors.Is(err, os.ErrNotExist) {
			if err == nil {
				return false, errors.New("llmtrim 状态目录删除后仍然存在")
			}
			return false, fmt.Errorf("校验 llmtrim 状态目录删除失败: %w", err)
		}
	}
	if _, err := os.Stat(installDir); !errors.Is(err, os.ErrNotExist) {
		if err == nil {
			return false, errors.New("llmtrim 项目目录删除后仍然存在")
		}
		return false, fmt.Errorf("校验 llmtrim 项目目录删除失败: %w", err)
	}
	if isTCPPortOpen(llmtrimDaemonAddress) {
		return false, errors.New("43117 端口仍被占用")
	}
	return managedGlobalState, nil
}

func queryLLMTrimResidual(configPath, installDir string, running, trayRunning, directoryExists, stateDirExists bool) (bool, error) {
	managedDirectories, err := managedLLMTrimInstallations(installDir, strings.TrimSpace(configPath))
	if err != nil {
		return true, err
	}
	// .llmtrim、用户环境变量和 CA 是 llmtrim 的共享用户状态。没有受管目录证据时，
	// 它们可能来自用户独立安装，页面只展示状态目录，不将其误报为本程序的残留。
	managedGlobalState := len(managedDirectories) > 0
	residual := running || trayRunning || directoryExists || strings.TrimSpace(configPath) != ""
	if !managedGlobalState {
		return residual, nil
	}
	residual = residual || stateDirExists
	autostartResidual, err := queryLLMTrimAutostartResidual()
	if err != nil {
		return true, err
	}
	environmentResidual, err := queryLLMTrimEnvironmentResidual()
	if err != nil {
		return true, err
	}
	return residual || autostartResidual || environmentResidual, nil
}

func queryLLMTrimEnvironmentResidual() (bool, error) {
	key, err := registry.OpenKey(registry.CURRENT_USER, `Environment`, registry.QUERY_VALUE)
	if errors.Is(err, syscall.ERROR_FILE_NOT_FOUND) {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	defer key.Close()
	for _, name := range []string{"HTTPS_PROXY", "HTTP_PROXY"} {
		value, _, valueErr := key.GetStringValue(name)
		if valueErr == nil && isLLMTrimProxyValue(value) {
			return true, nil
		}
	}
	if value, _, valueErr := key.GetStringValue("NO_PROXY"); valueErr == nil && strings.TrimSpace(value) == llmtrimNoProxy {
		return true, nil
	}
	caPath := filepath.Join(os.Getenv("USERPROFILE"), ".llmtrim", "ca.pem")
	if value, _, valueErr := key.GetStringValue("NODE_EXTRA_CA_CERTS"); valueErr == nil && sameLLMTrimCAPath(value, caPath) {
		return true, nil
	}
	if value, _, valueErr := key.GetStringValue("NODE_USE_ENV_PROXY"); valueErr == nil && strings.TrimSpace(value) == "1" {
		return true, nil
	}
	return false, nil
}

func (g *gateway) rollbackLLMTrimInstallation(installDir, installedPath string) {
	ctx, cancel := context.WithTimeout(context.Background(), 45*time.Second)
	defer cancel()
	stopErr := g.stopLLMTrimProcesses(ctx, installedPath)
	if stopErr != nil {
		log.Printf("rollback llmtrim stop failed: %v", stopErr)
	}
	_, cleanupErr := g.cleanupLLMTrimEnvironment(ctx, installDir, installedPath)
	if cleanupErr != nil {
		log.Printf("rollback llmtrim cleanup failed: %v", cleanupErr)
	}
	if err := g.persistLLMTrimPath(""); err != nil {
		log.Printf("rollback llmtrim path reset failed: %v", err)
	}
}

func removeLLMTrimAutostart() error {
	var failures []string
	for _, subkey := range []string{llmtrimRunKey, llmtrimStartupApprovedKey} {
		for _, valueName := range []string{"llmtrim", "llmtrim-tray"} {
			exists, err := registryValueExists(registry.CURRENT_USER, subkey, valueName)
			if err != nil {
				failures = append(failures, fmt.Sprintf("HKCU\\%s\\%s: %v", subkey, valueName, err))
				continue
			}
			if !exists {
				continue
			}
			if err := deleteRegistryValue(registry.CURRENT_USER, subkey, valueName); err != nil {
				failures = append(failures, fmt.Sprintf("HKCU\\%s\\%s: %v", subkey, valueName, err))
			}
		}
	}
	needsElevation := false
	for _, subkey := range []string{llmtrimRunKey, llmtrimStartupApprovedKey} {
		for _, valueName := range []string{"llmtrim", "llmtrim-tray"} {
			exists, err := registryValueExists(registry.LOCAL_MACHINE, subkey, valueName)
			if err != nil {
				failures = append(failures, fmt.Sprintf("HKLM\\%s\\%s: %v", subkey, valueName, err))
				continue
			}
			if !exists {
				continue
			}
			if err := deleteRegistryValue(registry.LOCAL_MACHINE, subkey, valueName); err != nil {
				if isWindowsAccessDenied(err) {
					needsElevation = true
				} else {
					failures = append(failures, fmt.Sprintf("HKLM\\%s\\%s: %v", subkey, valueName, err))
				}
			}
		}
	}
	if needsElevation {
		if err := runElevatedLLMTrimAutostartCleanup(); err != nil {
			failures = append(failures, "HKLM 自启动: "+err.Error())
		}
	}
	if len(failures) > 0 {
		return errors.New(strings.Join(failures, "；"))
	}
	return nil
}

func runElevatedLLMTrimAutostartCleanup() error {
	const script = `$keys=@('Software\Microsoft\Windows\CurrentVersion\Run','Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run');$names=@('llmtrim','llmtrim-tray');foreach($key in $keys){foreach($name in $names){Remove-ItemProperty -LiteralPath ('HKLM:\'+$key) -Name $name -ErrorAction SilentlyContinue}}`
	return runElevatedPowerShell(context.Background(), script)
}

func queryLLMTrimAutostartResidual() (bool, error) {
	for _, hive := range []registry.Key{registry.CURRENT_USER, registry.LOCAL_MACHINE} {
		for _, subkey := range []string{llmtrimRunKey, llmtrimStartupApprovedKey} {
			for _, valueName := range []string{"llmtrim", "llmtrim-tray"} {
				exists, err := registryValueExists(hive, subkey, valueName)
				if err != nil {
					return false, err
				}
				if exists {
					return true, nil
				}
			}
		}
	}
	return false, nil
}

func registryValueExists(hive registry.Key, subkey, valueName string) (bool, error) {
	key, err := registry.OpenKey(hive, subkey, registry.QUERY_VALUE)
	if errors.Is(err, syscall.ERROR_FILE_NOT_FOUND) {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	defer key.Close()
	_, _, err = key.GetValue(valueName, nil)
	if errors.Is(err, syscall.ERROR_FILE_NOT_FOUND) {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	return true, nil
}

func deleteRegistryValue(hive registry.Key, subkey string, valueName string) error {
	key, err := registry.OpenKey(hive, subkey, registry.SET_VALUE)
	if err != nil {
		if errors.Is(err, syscall.ERROR_FILE_NOT_FOUND) {
			return nil
		}
		return err
	}
	defer key.Close()
	if err := key.DeleteValue(valueName); err != nil && !errors.Is(err, syscall.ERROR_FILE_NOT_FOUND) {
		return err
	}
	return nil
}

func clearLLMTrimUserEnvironment() error {
	key, err := registry.OpenKey(registry.CURRENT_USER, `Environment`, registry.QUERY_VALUE|registry.SET_VALUE)
	if err != nil {
		if errors.Is(err, syscall.ERROR_FILE_NOT_FOUND) {
			return nil
		}
		return err
	}
	defer key.Close()
	managedProxy := false
	for _, name := range []string{"HTTPS_PROXY", "HTTP_PROXY"} {
		value, _, valueErr := key.GetStringValue(name)
		if valueErr == nil && isLLMTrimProxyValue(value) {
			if deleteErr := key.DeleteValue(name); deleteErr != nil && !errors.Is(deleteErr, syscall.ERROR_FILE_NOT_FOUND) {
				return fmt.Errorf("delete %s: %w", name, deleteErr)
			}
			managedProxy = true
		}
	}
	if value, _, valueErr := key.GetStringValue("NO_PROXY"); valueErr == nil && strings.TrimSpace(value) == llmtrimNoProxy {
		if deleteErr := key.DeleteValue("NO_PROXY"); deleteErr != nil && !errors.Is(deleteErr, syscall.ERROR_FILE_NOT_FOUND) {
			return fmt.Errorf("delete NO_PROXY: %w", deleteErr)
		}
	}
	caPath := filepath.Join(os.Getenv("USERPROFILE"), ".llmtrim", "ca.pem")
	if value, _, valueErr := key.GetStringValue("NODE_EXTRA_CA_CERTS"); valueErr == nil && sameLLMTrimCAPath(value, caPath) {
		if deleteErr := key.DeleteValue("NODE_EXTRA_CA_CERTS"); deleteErr != nil && !errors.Is(deleteErr, syscall.ERROR_FILE_NOT_FOUND) {
			return fmt.Errorf("delete NODE_EXTRA_CA_CERTS: %w", deleteErr)
		}
	}
	if managedProxy {
		if value, _, valueErr := key.GetStringValue("NODE_USE_ENV_PROXY"); valueErr == nil && strings.TrimSpace(value) == "1" {
			if deleteErr := key.DeleteValue("NODE_USE_ENV_PROXY"); deleteErr != nil && !errors.Is(deleteErr, syscall.ERROR_FILE_NOT_FOUND) {
				return fmt.Errorf("delete NODE_USE_ENV_PROXY: %w", deleteErr)
			}
		}
	}
	broadcastWindowsEnvironmentChange()
	return nil
}

func isLLMTrimProxyValue(value string) bool {
	return strings.EqualFold(strings.TrimRight(strings.TrimSpace(value), "/"), llmtrimProxyURL)
}

func sameLLMTrimCAPath(value string, expected string) bool {
	cleanValue := strings.TrimSpace(value)
	if strings.EqualFold(cleanValue, expected) {
		return true
	}
	return strings.EqualFold(cleanValue, `%USERPROFILE%\.llmtrim\ca.pem`)
}

func removeLLMTrimUserTrust() (bool, error) {
	result, err := exec.Command("certutil.exe", "-user", "-delstore", "Root", "llmtrim local CA").CombinedOutput()
	if err != nil {
		var exitErr *exec.ExitError
		if !errors.As(err, &exitErr) {
			return false, err
		}
	}
	exists, verifyErr := llmtrimUserTrustExists()
	if verifyErr != nil {
		return false, fmt.Errorf("校验 llmtrim CA 信任状态失败: %w", verifyErr)
	}
	if exists {
		message := strings.TrimSpace(string(result))
		if message == "" && err != nil {
			message = err.Error()
		}
		return false, fmt.Errorf("llmtrim local CA 仍存在于当前用户 Root 证书库: %s", message)
	}
	return err == nil, nil
}

func llmtrimUserTrustExists() (bool, error) {
	_, err := exec.Command("certutil.exe", "-user", "-store", "Root", "llmtrim local CA").CombinedOutput()
	if err == nil {
		return true, nil
	}
	var exitErr *exec.ExitError
	if errors.As(err, &exitErr) {
		// certutil 用非零退出码表示按名称未找到证书；删除操作是幂等的。
		return false, nil
	}
	return false, err
}

func broadcastWindowsEnvironmentChange() {
	const script = `$sig='[DllImport("user32.dll", SetLastError=true, CharSet=CharSet.Auto)] public static extern IntPtr SendMessageTimeout(IntPtr hWnd,uint Msg,UIntPtr wParam,string lParam,uint fuFlags,uint uTimeout,out UIntPtr lpdwResult);';$t=Add-Type -MemberDefinition $sig -Name LLMTrimEnvBroadcast -Namespace CodeManager -PassThru;$r=[UIntPtr]::Zero;[void]$t::SendMessageTimeout([IntPtr]0xffff,0x1A,[UIntPtr]::Zero,'Environment',0x2,5000,[ref]$r)`
	command := exec.Command("powershell.exe", "-NoProfile", "-NonInteractive", "-Command", script)
	command.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	command.Stdout = io.Discard
	command.Stderr = io.Discard
	_ = command.Run()
}

func removeOwnedLLMTrimDirectory(directory string) error {
	absolute, err := filepath.Abs(directory)
	if err != nil {
		return err
	}
	base := strings.ToLower(filepath.Base(filepath.Clean(absolute)))
	if base != "llmtrim" && base != ".llmtrim" {
		return fmt.Errorf("拒绝删除非 llmtrim 专属目录: %s", absolute)
	}
	info, err := os.Stat(absolute)
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	if err != nil {
		return err
	}
	if !info.IsDir() {
		return fmt.Errorf("目标不是目录: %s", absolute)
	}
	return os.RemoveAll(absolute)
}

func removeConfiguredLLMTrimFiles(configuredPath string, projectInstallDir string) error {
	if strings.TrimSpace(configuredPath) == "" {
		return nil
	}
	configuredAbsolute, err := filepath.Abs(configuredPath)
	if err != nil {
		return err
	}
	projectAbsolute, err := filepath.Abs(projectInstallDir)
	if err != nil {
		return err
	}
	configuredParent := filepath.Clean(filepath.Dir(configuredAbsolute))
	if strings.EqualFold(configuredParent, filepath.Clean(projectAbsolute)) || !strings.EqualFold(filepath.Base(configuredParent), "llmtrim") {
		return nil
	}
	// 外部配置路径可能属于用户手工安装；只有该目录带有 code-Manager 状态文件时才整体移除。
	if !isManagedToolInstallationDirectory("llmtrim", configuredParent) {
		return nil
	}
	// 配置路径位于已验证的独立 llmtrim 目录时整体移除，确保旧版本新增文件不会残留。
	return removeOwnedLLMTrimDirectory(configuredParent)
}

func llmtrimInstallDirectory() (string, error) {
	executable, err := os.Executable()
	if err != nil {
		return "", fmt.Errorf("无法定位 code-Manager.exe: %w", err)
	}
	return filepath.Join(filepath.Dir(executable), "llmtrim"), nil
}

func (g *gateway) stopManagedLLMTrimForInstall(ctx context.Context, installDir, configuredPath string) error {
	directories, err := managedLLMTrimInstallations(installDir, configuredPath)
	if err != nil {
		return err
	}
	if len(directories) == 0 {
		if isTCPPortOpen(llmtrimDaemonAddress) {
			return errors.New("43117 端口正被未受管的 llmtrim 或其它程序占用，未自动结束该进程")
		}
		g.closeLLMTrimProxyClientIdleConnections()
		return nil
	}
	if err := stopLLMTrimProcessesInDirectories(ctx, directories); err != nil {
		return err
	}
	// 已确认旧 daemon 停止。保持代理监听，但关闭旧 /v1 连接，防止
	// 客户端继续停留在已失效的 llmtrim 路由上。
	g.closeGatewayProxyConnections()
	// 安装替换和删除都会走这里。daemon 已停止后必须废弃旧连接池，
	// 以便下次启动时重新加载 llmtrim 可能重建过的 CA。
	g.closeLLMTrimProxyClientIdleConnections()
	return nil
}

// stopLLMTrimAndCleanup 停止 daemon/tray，并撤销 setup 写入的自启动和用户环境变量。
// 它不会删除 llmtrim 安装目录、CA 或用户状态目录；彻底删除仍由卸载流程负责。
func (g *gateway) stopLLMTrimAndCleanup(ctx context.Context, configuredPath string) error {
	var failures []string
	if err := g.stopLLMTrimProcesses(ctx, configuredPath); err != nil {
		failures = append(failures, "停止 llmtrim 进程: "+err.Error())
	} else {
		// Daemon has stopped even if later registry/environment cleanup fails.
		g.closeLLMTrimProxyClientIdleConnections()
		// 路由已确认恢复基线。监听继续运行，已有 /v1 连接主动断开后由
		// 客户端重新建立，新的请求才会按停止状态选择基线路由。
		g.closeGatewayProxyConnections()
	}
	if err := removeLLMTrimCurrentUserAutostart(); err != nil {
		failures = append(failures, "清理 llmtrim 当前用户自启动注册表: "+err.Error())
	}
	if err := clearLLMTrimUserEnvironment(); err != nil {
		failures = append(failures, "清理 llmtrim 用户环境变量: "+err.Error())
	}
	if residual, err := queryLLMTrimEnvironmentResidual(); err != nil {
		failures = append(failures, "校验 llmtrim 用户环境清理: "+err.Error())
	} else if residual {
		failures = append(failures, "llmtrim 用户环境变量仍存在配置")
	}
	if len(failures) > 0 {
		return errors.New(strings.Join(failures, "；"))
	}
	if installDir, err := llmtrimInstallDirectory(); err == nil {
		if info, statErr := os.Stat(installDir); statErr == nil && info.IsDir() {
			if stateErr := writeManagedToolState(installDir, managedToolState{DesiredRunning: false, Running: false}); stateErr != nil {
				log.Printf("保存 llmtrim 停止状态失败: %v", stateErr)
			}
		}
	}
	return nil
}

func removeLLMTrimCurrentUserAutostart() error {
	var failures []string
	for _, subkey := range []string{llmtrimRunKey, llmtrimStartupApprovedKey} {
		for _, valueName := range []string{"llmtrim", "llmtrim-tray"} {
			if err := deleteRegistryValue(registry.CURRENT_USER, subkey, valueName); err != nil {
				failures = append(failures, fmt.Sprintf("HKCU\\%s\\%s: %v", subkey, valueName, err))
			}
		}
	}
	if len(failures) > 0 {
		return errors.New(strings.Join(failures, "；"))
	}
	return nil
}

func (g *gateway) stopLLMTrimProcesses(ctx context.Context, configuredPath string) error {
	pathValue := strings.TrimSpace(configuredPath)
	if pathValue == "" {
		if isTCPPortOpen(llmtrimDaemonAddress) {
			return errors.New("43117 端口正被未指定路径的 llmtrim 或其它程序占用，未自动结束该进程")
		}
		return nil
	}
	absolute, err := filepath.Abs(pathValue)
	if err != nil {
		return err
	}
	if !strings.EqualFold(filepath.Base(absolute), llmtrimExecutableName) {
		return errors.New("llmtrim 路径文件名必须是 llmtrim.exe")
	}
	if validatedPath, validateErr := validateLLMTrimPath(absolute); validateErr == nil {
		if err := executeLLMTrimCommand(ctx, validatedPath, "autostart", "--off"); err != nil {
			log.Printf("关闭指定 llmtrim 自启动失败（%s）: %v", validatedPath, err)
		}
		if err := executeLLMTrimCommand(ctx, validatedPath, "stop"); err != nil {
			log.Printf("停止指定 llmtrim 返回错误（%s）: %v", validatedPath, err)
		}
	} else if findProcessIDByExecutable(absolute) != 0 {
		log.Printf("指定 llmtrim 路径对应的进程仍在运行，但文件不可用: %s", absolute)
	}
	return stopLLMTrimProcessesInDirectories(ctx, []string{filepath.Dir(absolute)})
}

// stopLLMTrimProcessesInDirectories 只处理已验证为 code-Manager 所有的目录。
// 发布目录迁移不能按文件名终止所有 llmtrim，否则会影响用户的独立安装。
func stopLLMTrimProcessesInDirectories(ctx context.Context, directories []string) error {
	targets := make(map[string]string, len(directories)*2)
	for _, directory := range directories {
		for _, name := range []string{llmtrimExecutableName, llmtrimTrayExecutableName} {
			pathValue := filepath.Join(directory, name)
			absolute, err := filepath.Abs(pathValue)
			if err == nil {
				targets[strings.ToLower(filepath.Clean(absolute))] = absolute
			}
		}
	}
	for _, process := range findRunningProcessesByNames(llmtrimExecutableName, llmtrimTrayExecutableName) {
		pathValue := strings.ToLower(filepath.Clean(process.Path))
		if _, owned := targets[pathValue]; !owned {
			continue
		}
		if strings.EqualFold(filepath.Base(process.Path), llmtrimExecutableName) {
			if err := executeLLMTrimCommand(ctx, process.Path, "autostart", "--off"); err != nil {
				log.Printf("关闭受管 llmtrim 自启动失败（%s）: %v", process.Path, err)
			}
			if err := executeLLMTrimCommand(ctx, process.Path, "stop"); err != nil {
				log.Printf("停止受管 llmtrim 返回错误（%s）: %v", process.Path, err)
			}
		}
	}
	for _, target := range targets {
		if err := terminateProcessesByPath(target); err != nil {
			return fmt.Errorf("结束受管 llmtrim 进程失败（%s）: %w", target, err)
		}
	}
	deadline := time.Now().Add(8 * time.Second)
	for time.Now().Before(deadline) {
		stillRunning := false
		for _, target := range targets {
			if findProcessIDByExecutable(target) != 0 {
				stillRunning = true
				break
			}
		}
		if !stillRunning {
			break
		}
		time.Sleep(200 * time.Millisecond)
	}
	for _, target := range targets {
		if processID := findProcessIDByExecutable(target); processID != 0 {
			return fmt.Errorf("受管 llmtrim 仍未停止（PID %d，%s）", processID, target)
		}
	}
	if isTCPPortOpen(llmtrimDaemonAddress) {
		return errors.New("43117 端口仍被未受管的 llmtrim 或其它程序占用，未自动结束该进程")
	}
	return nil
}

func verifyLLMTrimWindowsSetup() (bool, string) {
	key, err := registry.OpenKey(registry.CURRENT_USER, `Environment`, registry.QUERY_VALUE)
	if err != nil {
		return false, fmt.Sprintf("无法读取 HKCU\\Environment: %v", err)
	}
	defer key.Close()
	missing := make([]string, 0, 3)
	for _, name := range []string{"HTTPS_PROXY", "HTTP_PROXY"} {
		value, _, valueErr := key.GetStringValue(name)
		if valueErr != nil || !strings.EqualFold(strings.TrimSpace(value), llmtrimProxyURL) {
			missing = append(missing, name+"="+llmtrimProxyURL)
		}
	}
	caPath := filepath.Join(os.Getenv("USERPROFILE"), ".llmtrim", "ca.pem")
	if _, err := os.Stat(caPath); err != nil {
		missing = append(missing, "CA 文件 "+caPath)
	}
	nodeCA, _, nodeCAErr := key.GetStringValue("NODE_EXTRA_CA_CERTS")
	if nodeCAErr != nil || !strings.EqualFold(strings.TrimSpace(nodeCA), caPath) {
		missing = append(missing, "NODE_EXTRA_CA_CERTS="+caPath)
	}
	nodeProxy, _, nodeProxyErr := key.GetStringValue("NODE_USE_ENV_PROXY")
	if nodeProxyErr != nil || strings.TrimSpace(nodeProxy) != "1" {
		missing = append(missing, "NODE_USE_ENV_PROXY=1")
	}
	if len(missing) > 0 {
		return false, "缺少或不匹配：" + strings.Join(missing, "；")
	}
	return true, "Windows 用户代理环境和本地 CA 已配置。"
}
