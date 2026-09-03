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
	"path/filepath"
	"strings"
	"time"
)

type rtkStatusResponse struct {
	Path                   string   `json:"path"`
	Installed              bool     `json:"installed"`
	DirectoryExists        bool     `json:"directory_exists"`
	Version                string   `json:"version"`
	UserPath               bool     `json:"user_path"`
	SystemPath             bool     `json:"system_path"`
	CodexAvailable         bool     `json:"codex_available"`
	CodexConfigured        bool     `json:"codex_configured"`
	CodexResidual          bool     `json:"codex_residual"`
	ClaudeAvailable        bool     `json:"claude_available"`
	ClaudeHookConfigured   bool     `json:"claude_hook_configured"`
	ClaudePromptConfigured bool     `json:"claude_prompt_configured"`
	ClaudeConfigured       bool     `json:"claude_configured"`
	ClaudeResidual         bool     `json:"claude_residual"`
	CopilotAvailable       bool     `json:"copilot_available"`
	CopilotConfigured      bool     `json:"copilot_configured"`
	CursorAvailable        bool     `json:"cursor_available"`
	CursorConfigured       bool     `json:"cursor_configured"`
	Running                bool     `json:"running"`
	DesiredRunning         bool     `json:"desired_running"`
	ActivationState        string   `json:"activation_state"`
	BlockedBy              string   `json:"blocked_by,omitempty"`
	TrustNotice            string   `json:"trust_notice,omitempty"`
	ModifiedAgents         []string `json:"modified_agents,omitempty"`
	Message                string   `json:"message"`
}

type rtkInstallResponse struct {
	Path                   string   `json:"path"`
	Version                string   `json:"version"`
	UserPath               bool     `json:"user_path"`
	SystemPath             bool     `json:"system_path"`
	CodexAvailable         bool     `json:"codex_available"`
	CodexConfigured        bool     `json:"codex_configured"`
	ClaudeAvailable        bool     `json:"claude_available"`
	ClaudeHookConfigured   bool     `json:"claude_hook_configured"`
	ClaudePromptConfigured bool     `json:"claude_prompt_configured"`
	ClaudeConfigured       bool     `json:"claude_configured"`
	Running                bool     `json:"running"`
	DesiredRunning         bool     `json:"desired_running"`
	Message                string   `json:"message"`
	Warnings               []string `json:"warnings,omitempty"`
}

type rtkUninstallResponse struct {
	Path                   string   `json:"path"`
	Installed              bool     `json:"installed"`
	DirectoryExists        bool     `json:"directory_exists"`
	UserPath               bool     `json:"user_path"`
	SystemPath             bool     `json:"system_path"`
	CodexAvailable         bool     `json:"codex_available"`
	CodexConfigured        bool     `json:"codex_configured"`
	CodexResidual          bool     `json:"codex_residual"`
	ClaudeAvailable        bool     `json:"claude_available"`
	ClaudeHookConfigured   bool     `json:"claude_hook_configured"`
	ClaudePromptConfigured bool     `json:"claude_prompt_configured"`
	ClaudeConfigured       bool     `json:"claude_configured"`
	ClaudeResidual         bool     `json:"claude_residual"`
	Running                bool     `json:"running"`
	DesiredRunning         bool     `json:"desired_running"`
	Message                string   `json:"message"`
	Warnings               []string `json:"warnings,omitempty"`
}

func boolStatusLabel(value bool, yes, no string) string {
	if value {
		return yes
	}
	return no
}

func rtkInstallDirectory() (string, error) {
	executable, err := os.Executable()
	if err != nil {
		return "", fmt.Errorf("无法定位 code-Manager.exe: %w", err)
	}
	return filepath.Join(filepath.Dir(executable), "RTK-AI"), nil
}

func rtkExecutablePath() (string, error) {
	directory, err := rtkInstallDirectory()
	if err != nil {
		return "", err
	}
	return filepath.Join(directory, "rtk.exe"), nil
}

// repairInstalledRTKPath 在程序启动时补齐当前 EXE 同级 RTK-AI 目录的 PATH。
// Windows 注册表 PATH 不支持稳定的“相对 EXE 路径”，因此每次均从当前 EXE 动态计算实际目录。
func repairInstalledRTKPath() {
	installDir, err := rtkInstallDirectory()
	if err != nil {
		log.Printf("启动时定位 RTK-AI 目录失败: %v", err)
		return
	}
	executable := filepath.Join(installDir, "rtk.exe")
	info, err := os.Stat(executable)
	if errors.Is(err, os.ErrNotExist) {
		return
	}
	if err != nil {
		log.Printf("启动时检查 rtk.exe 失败: %v", err)
		return
	}
	if info.IsDir() {
		log.Printf("启动时检查 rtk.exe 失败: %s 不是文件", executable)
		return
	}
	userPath, systemPath, pathMessage := queryRTKPathStatus(installDir)
	if pathMessage != "" {
		log.Printf("启动时读取 RTK PATH 状态失败: %s", pathMessage)
		return
	}
	if userPath && systemPath {
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), 45*time.Second)
	defer cancel()
	userPath, systemPath, err = configureRTKPath(ctx, installDir)
	if err != nil {
		log.Printf("检测到已安装的 RTK 但 PATH 不完整，自动补齐失败: %v", err)
		return
	}
	log.Printf("检测到已安装的 RTK，已自动补齐 PATH：用户=%t，系统=%t，目录=%s", userPath, systemPath, installDir)
}

func rtkVersionMarkerPath(installDir string) string {
	return filepath.Join(installDir, ".rtk-version")
}

func readRTKVersion(installDir string) string {
	data, err := os.ReadFile(rtkVersionMarkerPath(installDir))
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(data))
}

func writeRTKVersion(installDir, version string) error {
	temporary, err := os.CreateTemp(installDir, ".rtk-version-*")
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
	return os.Rename(temporaryPath, rtkVersionMarkerPath(installDir))
}

// rtkActivationArtifactsPresent 只检查账本明确拥有的 RTK 接入残留。
// 手工写入的同内容提示词或同命令 Hook 没有账本归属，不能被误报为待清理残留。
func rtkActivationArtifactsPresent(state managedToolState, ledger rtkOwnershipLedger) (bool, error) {
	if state.UserPath || state.SystemPath {
		return true, nil
	}
	for name, artifact := range ledger.Artifacts {
		present, err := rtkArtifactPresent(name, artifact)
		if err != nil {
			return false, err
		}
		if present {
			return true, nil
		}
	}
	return false, nil
}

func (g *gateway) rtkStatus(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	installDir, err := rtkInstallDirectory()
	if err != nil {
		http.Error(w, err.Error(), http.StatusInternalServerError)
		return
	}
	executable, err := rtkExecutablePath()
	if err != nil {
		http.Error(w, err.Error(), http.StatusInternalServerError)
		return
	}
	installed := false
	if info, statErr := os.Stat(executable); statErr == nil {
		installed = !info.IsDir()
	}
	directoryExists := false
	if info, statErr := os.Stat(installDir); statErr == nil {
		directoryExists = info.IsDir()
	}
	userPath, systemPath, pathMessage := queryRTKPathStatus(installDir)
	toolState, stateErr := readManagedToolState(installDir)
	ownershipLedger := emptyRTKOwnershipLedger()
	var ownershipErr error
	if stateErr == nil {
		ownershipLedger, ownershipErr = recoverLegacyRTKOwnership(toolState, executable)
	}
	codexAvailable := queryRTKCodexAvailable()
	codexConfigured := queryRTKCodexConfigured()
	codexResidual, residualErr := rtkCodexResidual()
	codexResidualFlag := residualErr != nil || codexResidual != ""
	claudeAvailable := queryRTKClaudeAvailable()
	claudeHookConfigured := rtkClaudeHookConfiguredForExecutable(executable)
	claudePromptConfigured := queryRTKClaudePromptConfigured()
	claudeConfigured := claudeHookConfigured && claudePromptConfigured
	claudeResidual, claudeResidualErr := rtkClaudeResidual()
	claudePromptResidual, claudePromptResidualErr := rtkClaudePromptResidual()
	claudeResidualFlag := claudeResidualErr != nil || claudePromptResidualErr != nil || claudeResidual != "" || claudePromptResidual != ""
	if residualErr != nil {
		pathMessage = strings.TrimSpace(strings.Trim(pathMessage+"；无法读取 Codex 状态: "+residualErr.Error(), "；"))
	}
	if claudeResidualErr != nil {
		pathMessage = strings.TrimSpace(strings.Trim(pathMessage+"；无法读取 Claude 状态: "+claudeResidualErr.Error(), "；"))
	}
	if claudePromptResidualErr != nil {
		pathMessage = strings.TrimSpace(strings.Trim(pathMessage+"；无法读取 Claude 提示词状态: "+claudePromptResidualErr.Error(), "；"))
	}
	if stateErr != nil {
		pathMessage = strings.TrimSpace(strings.Trim(pathMessage+"；无法读取 RTK 状态: "+stateErr.Error(), "；"))
	}
	if ownershipErr != nil {
		pathMessage = strings.TrimSpace(strings.Trim(pathMessage+"；无法读取 RTK 精确归属: "+ownershipErr.Error(), "；"))
	}
	unexpectedResidual := false
	var artifactErr error
	if stateErr == nil && ownershipErr == nil && !toolState.Running && !toolState.DesiredRunning {
		unexpectedResidual, artifactErr = rtkActivationArtifactsPresent(toolState, ownershipLedger)
		if artifactErr != nil {
			pathMessage = strings.TrimSpace(strings.Trim(pathMessage+"；无法检查 RTK 受管残留: "+artifactErr.Error(), "；"))
		}
	}
	modifiedAgents := rtkModifiedAgentNames(ownershipLedger)
	activationState := "not_installed"
	if installed {
		activationState = "installed_stopped"
		if toolState.Running {
			activationState = "running"
		}
		if stateErr != nil || ownershipErr != nil || artifactErr != nil || unexpectedResidual || managedToolStateHasAttention(toolState) {
			activationState = "attention"
		}
	}
	blockedBy := ""
	if toolState.Running || toolState.DesiredRunning {
		if snipRunning, _ := managedToolActive("snip"); snipRunning {
			activationState, blockedBy = "conflict", "snip"
		}
	}
	message := "RTK 尚未安装。"
	if installed {
		message = "RTK 已安装，但 Windows PATH 尚未完整配置。"
		if userPath && systemPath {
			message = "RTK 已安装，用户和系统 PATH 已配置。"
			if toolState.Running || toolState.DesiredRunning {
				message = "RTK 已激活；这只表示激活流程已完成，不代表四个平台都已接入。仅检测到且可安全写入的平台会尝试接入，请以各平台状态为准。"
			}
			if codexAvailable {
				if codexConfigured {
					message += " Codex AGENTS.md 已集成。"
				} else {
					message += " Codex AGENTS.md 集成未完成。"
				}
			}
			if claudeAvailable {
				message += fmt.Sprintf(" Claude Hook%s，提示词%s。", boolStatusLabel(claudeHookConfigured, "已配置", "未配置"), boolStatusLabel(claudePromptConfigured, "已集成", "未集成"))
			}
		}
	} else if directoryExists || userPath || systemPath || codexResidualFlag || claudeResidualFlag {
		message = "检测到 RTK 残留环境，请先删除后再安装。"
	}
	if pathMessage != "" {
		message += " " + pathMessage
	}
	writeJSON(w, http.StatusOK, rtkStatusResponse{
		Path:                   executable,
		Installed:              installed,
		DirectoryExists:        directoryExists,
		Version:                readRTKVersion(installDir),
		UserPath:               userPath,
		SystemPath:             systemPath,
		CodexAvailable:         codexAvailable,
		CodexConfigured:        codexConfigured,
		CodexResidual:          codexResidualFlag,
		ClaudeAvailable:        claudeAvailable,
		ClaudeHookConfigured:   claudeHookConfigured,
		ClaudePromptConfigured: claudePromptConfigured,
		ClaudeConfigured:       claudeConfigured,
		ClaudeResidual:         claudeResidualFlag,
		CopilotAvailable:       rtkCopilotAvailable(),
		CopilotConfigured:      rtkCopilotConfiguredForExecutable(executable),
		CursorAvailable:        rtkCursorAvailable(),
		CursorConfigured:       rtkCursorConfiguredForExecutable(executable),
		Running:                toolState.Running,
		DesiredRunning:         toolState.DesiredRunning,
		ActivationState:        activationState,
		BlockedBy:              blockedBy,
		ModifiedAgents:         modifiedAgents,
		Message:                message,
	})
}

func (g *gateway) rtkInstall(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	if g.isShuttingDown() {
		http.Error(w, "code-Manager 正在退出，不能安装 RTK", http.StatusServiceUnavailable)
		return
	}
	var input rtkInstallRequest
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
		http.Error(w, "请选择要安装的 RTK 版本", http.StatusBadRequest)
		return
	}

	g.rtkMu.Lock()
	defer g.rtkMu.Unlock()
	managedToolMu.Lock()
	defer managedToolMu.Unlock()
	if running, _ := managedToolActive("snip"); running {
		http.Error(w, "snip 正在运行，请先停止 snip", http.StatusConflict)
		return
	}
	installDir, err := rtkInstallDirectory()
	if err != nil {
		http.Error(w, err.Error(), http.StatusInternalServerError)
		return
	}
	executable := filepath.Join(installDir, "rtk.exe")
	if state, stateErr := readManagedToolState(installDir); stateErr != nil {
		http.Error(w, "读取 RTK 状态失败: "+stateErr.Error(), http.StatusInternalServerError)
		return
	} else if state.Running || state.DesiredRunning {
		http.Error(w, "RTK 正在运行，请先停止 RTK", http.StatusConflict)
		return
	}
	if processID := findProcessIDByExecutable(executable); processID != 0 {
		http.Error(w, fmt.Sprintf("RTK 正在运行（PID %d），请先结束该命令后再安装", processID), http.StatusConflict)
		return
	}
	installContext, cancel := context.WithTimeout(r.Context(), 3*time.Minute)
	defer cancel()
	if err := g.downloadAndInstallRTK(installContext, input.TagName, installDir); err != nil {
		log.Printf("install rtk version=%s failed: %v", input.TagName, err)
		http.Error(w, "安装 RTK 失败: "+err.Error(), http.StatusBadGateway)
		return
	}
	if err := writeRTKVersion(installDir, input.TagName); err != nil {
		rollbackRTKInstallation(installDir)
		http.Error(w, "保存 RTK 版本信息失败: "+err.Error(), http.StatusInternalServerError)
		return
	}
	if err := writeRTKCodexCommands(installDir); err != nil {
		rollbackRTKInstallation(installDir)
		http.Error(w, "保存 RTK 内置命令文档失败: "+err.Error(), http.StatusInternalServerError)
		return
	}
	if err := writeManagedToolState(installDir, managedToolState{DesiredRunning: false, Running: false}); err != nil {
		rollbackRTKInstallation(installDir)
		http.Error(w, "保存 RTK 状态失败: "+err.Error(), http.StatusInternalServerError)
		return
	}
	message := fmt.Sprintf("RTK %s 已安装到 RTK-AI，当前为已安装/已停止；启动后才配置 PATH 和助手集成。", input.TagName)
	codexAvailable := queryRTKCodexAvailable()
	codexConfigured := queryRTKCodexConfigured()
	claudeAvailable := queryRTKClaudeAvailable()
	claudeHookConfigured := rtkClaudeHookConfiguredForExecutable(executable)
	claudePromptConfigured := queryRTKClaudePromptConfigured()
	claudeConfigured := claudeHookConfigured && claudePromptConfigured
	writeJSON(w, http.StatusOK, rtkInstallResponse{
		Path:                   executable,
		Version:                input.TagName,
		UserPath:               false,
		SystemPath:             false,
		CodexAvailable:         codexAvailable,
		CodexConfigured:        codexConfigured,
		ClaudeAvailable:        claudeAvailable,
		ClaudeHookConfigured:   claudeHookConfigured,
		ClaudePromptConfigured: claudePromptConfigured,
		ClaudeConfigured:       claudeConfigured,
		Message:                message,
	})
}

func (g *gateway) rtkUninstall(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	if g.isShuttingDown() {
		http.Error(w, "code-Manager 正在退出，不能删除 RTK", http.StatusServiceUnavailable)
		return
	}
	commandContext, cancel := context.WithTimeout(r.Context(), 45*time.Second)
	defer cancel()
	if err := g.uninstallRTK(commandContext); err != nil {
		http.Error(w, "删除 RTK 失败: "+err.Error(), http.StatusBadGateway)
		return
	}
	installDir, err := rtkInstallDirectory()
	if err != nil {
		http.Error(w, err.Error(), http.StatusInternalServerError)
		return
	}
	executable := filepath.Join(installDir, "rtk.exe")
	userPath, systemPath, pathMessage := queryRTKPathStatus(installDir)
	if pathMessage != "" {
		http.Error(w, "删除 RTK 后校验失败: "+pathMessage, http.StatusInternalServerError)
		return
	}
	claudeHookConfigured := rtkClaudeHookConfiguredForExecutable(executable)
	claudePromptConfigured := queryRTKClaudePromptConfigured()
	claudeResidual := func() bool {
		hookResidual, hookErr := rtkClaudeResidual()
		promptResidual, promptErr := rtkClaudePromptResidual()
		return hookErr != nil || promptErr != nil || hookResidual != "" || promptResidual != ""
	}()
	writeJSON(w, http.StatusOK, rtkUninstallResponse{
		Path:                   executable,
		Installed:              false,
		DirectoryExists:        false,
		UserPath:               userPath,
		SystemPath:             systemPath,
		CodexAvailable:         queryRTKCodexAvailable(),
		CodexConfigured:        queryRTKCodexConfigured(),
		CodexResidual:          func() bool { value, err := rtkCodexResidual(); return err != nil || value != "" }(),
		ClaudeAvailable:        queryRTKClaudeAvailable(),
		ClaudeHookConfigured:   claudeHookConfigured,
		ClaudePromptConfigured: claudePromptConfigured,
		ClaudeConfigured:       claudeHookConfigured && claudePromptConfigured,
		ClaudeResidual:         claudeResidual,
		Message:                "RTK 已删除；已清理 RTK-AI 目录、受管 PATH 及账本明确归属的提示词和 Hook。未受管或手工内容会保留。",
	})
}

func (g *gateway) uninstallRTK(ctx context.Context) error {
	g.rtkMu.Lock()
	defer g.rtkMu.Unlock()
	managedToolMu.Lock()
	defer managedToolMu.Unlock()
	if running, _ := managedToolActive("snip"); running {
		return errors.New("snip 正在运行，请先停止 snip")
	}
	installDir, err := rtkInstallDirectory()
	if err != nil {
		return err
	}
	executable := filepath.Join(installDir, "rtk.exe")
	state, stateErr := readManagedToolState(installDir)
	if stateErr != nil {
		return fmt.Errorf("读取 RTK 状态失败: %w", stateErr)
	}
	if state.Running || state.DesiredRunning {
		return errors.New("请先停止 RTK，再删除")
	}
	if processID := findProcessIDByExecutable(executable); processID != 0 {
		return fmt.Errorf("RTK 正在运行（PID %d），请先结束该命令后再删除", processID)
	}
	_, _, pathMessage := queryRTKPathStatus(installDir)
	if pathMessage != "" {
		return errors.New(pathMessage)
	}
	ledger, err := recoverLegacyRTKOwnership(state, executable)
	if err != nil {
		return err
	}
	if err := removeRTKOwnedArtifacts(ledger); err != nil {
		return fmt.Errorf("清理受管 Agent 接入失败: %w", err)
	}
	if err := removeOwnedRTKPath(ctx, installDir, state.UserPath, state.SystemPath); err != nil {
		return fmt.Errorf("清理 RTK PATH 失败: %w", err)
	}
	if err := removeOwnedRTKDirectory(installDir); err != nil {
		log.Printf("uninstall rtk failed: %v", err)
		return fmt.Errorf("删除 RTK 失败: %w", err)
	}
	_ = removeManagedToolState(installDir)
	userPath, systemPath, pathMessage := queryRTKPathStatus(installDir)
	if pathMessage != "" {
		return fmt.Errorf("删除 RTK 后校验失败: %s", pathMessage)
	}
	if (state.UserPath && userPath) || (state.SystemPath && systemPath) {
		return errors.New("删除 RTK 后校验失败：code-Manager 写入的 PATH 仍存在")
	}
	if _, statErr := os.Stat(executable); statErr == nil {
		return errors.New("删除 RTK 后校验失败：RTK-AI 目录仍存在")
	} else if !errors.Is(statErr, os.ErrNotExist) {
		return fmt.Errorf("删除 RTK 后检查 rtk.exe 失败: %w", statErr)
	}
	return nil
}

func (g *gateway) rtkStart(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	if running, _ := managedToolActive("snip"); running {
		http.Error(w, "snip 正在运行，请先停止 snip", http.StatusConflict)
		return
	}
	if err := g.startRTK(r.Context()); err != nil {
		if installDir, dirErr := rtkInstallDirectory(); dirErr == nil {
			state, _ := readManagedToolState(installDir)
			markManagedToolAttention(installDir, state, err)
		}
		http.Error(w, "启动 RTK 失败: "+err.Error(), http.StatusBadGateway)
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"running": true, "desired_running": true, "activation_state": "running", "message": "RTK 已激活。PATH 已配置；仅检测到且可安全写入的平台会尝试接入，请查看各平台状态。"})
}

func (g *gateway) rtkStop(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	if err := g.stopRTK(r.Context()); err != nil {
		if installDir, dirErr := rtkInstallDirectory(); dirErr == nil {
			state, _ := readManagedToolState(installDir)
			markManagedToolAttention(installDir, state, err)
		}
		http.Error(w, "停止 RTK 失败: "+err.Error(), http.StatusBadGateway)
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"running": false, "desired_running": false, "activation_state": "installed_stopped", "message": "RTK 已停止，当前受管 PATH、提示词和 Hook 已清理；未受管内容会保留。"})
}

func (g *gateway) startRTK(ctx context.Context) error {
	return withManagedToolLock(func() error { return g.startRTKLocked(ctx) })
}

func (g *gateway) startRTKLocked(ctx context.Context) error {
	if running, _ := managedToolActive("snip"); running {
		return errors.New("snip 正在运行")
	}
	installDir, err := rtkInstallDirectory()
	if err != nil {
		return err
	}
	executable := filepath.Join(installDir, "rtk.exe")
	if _, err := os.Stat(executable); err != nil {
		return errors.New("RTK 尚未安装")
	}
	state, err := readManagedToolState(installDir)
	if err != nil {
		return fmt.Errorf("读取 RTK 状态失败: %w", err)
	}
	// 旧状态只有平台级 owned_agents。启动时先把仍可精确确认的旧内容
	// 恢复到新账本，避免重启激活后遗失此前由 code-Manager 写入的归属。
	ledger, err := recoverLegacyRTKOwnership(state, executable)
	if err != nil {
		return err
	}
	integrationBefore := snapshotRTKIntegrations()
	claudeHookBefore := rtkClaudeHookConfiguredForExecutable(executable)
	copilotHookBefore := rtkCopilotConfiguredForExecutable(executable)
	cursorHookBefore := rtkCursorConfiguredForExecutable(executable)
	userPathBefore, systemPathBefore, pathMessage := queryRTKPathStatus(installDir)
	if pathMessage != "" {
		return errors.New(pathMessage)
	}
	if _, _, err := configureRTKPath(ctx, installDir); err != nil {
		return err
	}
	if err := writeRTKCodexCommands(installDir); err != nil {
		_ = removeRTKPath(ctx, installDir)
		return err
	}
	promptResult, err := installRTKAssistantIntegrationsWithOwnership()
	if err != nil {
		_ = restoreRTKIntegrationSnapshots(integrationBefore)
		_ = removeRTKPath(ctx, installDir)
		return err
	}
	if len(promptResult.Unmanaged) > 0 {
		_ = restoreRTKIntegrationSnapshots(integrationBefore)
		_ = removeRTKPath(ctx, installDir)
		return fmt.Errorf("检测到未受管的 RTK 提示词标记段（%s）；为避免覆盖或误删，请先手工处理后再启动", strings.Join(promptResult.Unmanaged, "、"))
	}
	if err := installRTKClaudeIntegration(executable); err != nil {
		_ = restoreRTKIntegrationSnapshots(integrationBefore)
		_ = removeRTKPath(ctx, installDir)
		return fmt.Errorf("配置 Claude Hook 失败: %w", err)
	}
	if err := installRTKCopilotIntegration(executable); err != nil {
		_ = restoreRTKIntegrationSnapshots(integrationBefore)
		_ = removeRTKPath(ctx, installDir)
		return fmt.Errorf("配置 Copilot Hook 失败: %w", err)
	}
	if err := installRTKCursorHook(executable); err != nil {
		_ = restoreRTKIntegrationSnapshots(integrationBefore)
		_ = removeRTKPath(ctx, installDir)
		return fmt.Errorf("配置 Cursor Hook 失败: %w", err)
	}
	for name, artifact := range promptResult.Artifacts {
		ledger.Artifacts[name] = artifact
	}
	// 仅把本次新写入的 Hook 纳入账本。已有相同命令可能是用户手工配置，
	// 没有旧账本归属时停止 RTK 不应删除它。
	if !claudeHookBefore && rtkClaudeHookConfiguredForExecutable(executable) {
		ledger.Artifacts[rtkArtifactClaudeHook] = rtkHookOwnership("claude-code", rtkClaudeSettingsFile(), rtkClaudeHookCommand(executable))
	}
	if !copilotHookBefore && rtkCopilotConfiguredForExecutable(executable) {
		ledger.Artifacts[rtkArtifactCopilotHook] = rtkHookOwnership("copilot", rtkCopilotHookFile(), rtkCopilotHookCommand(executable))
	}
	if !cursorHookBefore && rtkCursorConfiguredForExecutable(executable) {
		ledger.Artifacts[rtkArtifactCursorHook] = rtkHookOwnership("cursor", rtkCursorHooksFile(), rtkCursorHookCommand(executable))
	}
	state.DesiredRunning, state.Running = true, true
	state.UserPath = state.UserPath || !userPathBefore
	state.SystemPath = state.SystemPath || !systemPathBefore
	if err := writeRTKOwnershipLedger(&state, ledger); err != nil {
		_ = restoreRTKIntegrationSnapshots(integrationBefore)
		_ = removeOwnedRTKPath(ctx, installDir, state.UserPath, state.SystemPath)
		return fmt.Errorf("保存 RTK 精确归属状态失败: %w", err)
	}
	if state.Metadata != nil {
		delete(state.Metadata, "last_error")
	}
	if err := writeManagedToolState(installDir, state); err != nil {
		// 状态无法落盘时不能留下已生效的 PATH/Hook，否则停止流程无法知道归属。
		var cleanupErrs []string
		if cleanupErr := restoreRTKIntegrationSnapshots(integrationBefore); cleanupErr != nil {
			cleanupErrs = append(cleanupErrs, cleanupErr.Error())
		}
		if cleanupErr := removeOwnedRTKPath(ctx, installDir, state.UserPath, state.SystemPath); cleanupErr != nil {
			cleanupErrs = append(cleanupErrs, cleanupErr.Error())
		}
		if len(cleanupErrs) > 0 {
			return fmt.Errorf("保存 RTK 状态失败: %w；回滚激活配置失败: %s", err, strings.Join(cleanupErrs, "；"))
		}
		return fmt.Errorf("保存 RTK 状态失败: %w", err)
	}
	return nil
}

func (g *gateway) stopRTK(ctx context.Context) error {
	return withManagedToolLock(func() error { return g.stopRTKLocked(ctx) })
}

func (g *gateway) stopRTKLocked(ctx context.Context) error {
	installDir, err := rtkInstallDirectory()
	if err != nil {
		return err
	}
	state, err := readManagedToolState(installDir)
	if err != nil {
		return err
	}
	ledger, err := recoverLegacyRTKOwnership(state, rtkExecutablePathOrEmpty())
	if err != nil {
		return err
	}
	if !state.Running && !state.DesiredRunning && len(ledger.Artifacts) == 0 && !state.UserPath && !state.SystemPath {
		return nil
	}
	if err := removeOwnedRTKPath(ctx, installDir, state.UserPath, state.SystemPath); err != nil {
		return err
	}
	if err := removeRTKOwnedArtifacts(ledger); err != nil {
		return err
	}
	state.DesiredRunning, state.Running, state.UserPath, state.SystemPath = false, false, false, false
	clearRTKOwnershipLedger(&state)
	if state.Metadata != nil {
		delete(state.Metadata, "last_error")
	}
	return writeManagedToolState(installDir, state)
}

// cleanupRTKEnvironment 严格清理账本明确归属的 RTK 接入、PATH 和专属目录。
func cleanupRTKEnvironment(ctx context.Context, installDir string) error {
	state, err := readManagedToolState(installDir)
	if err != nil {
		return fmt.Errorf("读取 RTK 状态失败: %w", err)
	}
	ledger, err := recoverLegacyRTKOwnership(state, rtkExecutablePathOrEmpty())
	if err != nil {
		return err
	}
	if err := removeRTKOwnedArtifacts(ledger); err != nil {
		return err
	}
	if err := cleanupRTKInstallArtifacts(ctx, installDir); err != nil {
		return err
	}
	return nil
}

func cleanupRTKInstallArtifacts(ctx context.Context, installDir string) error {
	if err := removeRTKPath(ctx, installDir); err != nil {
		return fmt.Errorf("清理用户/系统 PATH 失败: %w", err)
	}
	userPath, systemPath, pathMessage := queryRTKPathStatus(installDir)
	if pathMessage != "" {
		return fmt.Errorf("校验用户/系统 PATH 失败: %s", pathMessage)
	}
	if userPath || systemPath {
		return errors.New("用户或系统 PATH 中仍存在 RTK-AI 目录")
	}
	if err := removeOwnedRTKDirectory(installDir); err != nil {
		return fmt.Errorf("删除 RTK-AI 目录失败: %w", err)
	}
	if _, err := os.Stat(installDir); !errors.Is(err, os.ErrNotExist) {
		if err == nil {
			return errors.New("RTK-AI 目录删除后仍然存在")
		}
		return fmt.Errorf("校验 RTK-AI 目录删除失败: %w", err)
	}
	return nil
}

func rollbackRTKInstallation(installDir string) {
	ctx, cancel := context.WithTimeout(context.Background(), 45*time.Second)
	defer cancel()
	if err := cleanupRTKInstallArtifacts(ctx, installDir); err != nil {
		log.Printf("rollback rtk installation failed: %v", err)
	}
}

func verifyRTKInstallation(installDir string) error {
	executable := filepath.Join(installDir, "rtk.exe")
	if info, err := os.Stat(executable); err != nil || info.IsDir() {
		if err == nil {
			return errors.New("rtk.exe 不存在或不是文件")
		}
		return fmt.Errorf("rtk.exe 不存在: %w", err)
	}
	userPath, systemPath, pathMessage := queryRTKPathStatus(installDir)
	if pathMessage != "" {
		return errors.New(pathMessage)
	}
	if !userPath || !systemPath {
		return errors.New("用户或系统 PATH 未完整配置")
	}
	return nil
}

func removeOwnedRTKDirectory(directory string) error {
	absolute, err := filepath.Abs(directory)
	if err != nil {
		return err
	}
	if strings.ToLower(filepath.Base(filepath.Clean(absolute))) != "rtk-ai" {
		return fmt.Errorf("拒绝删除非 RTK-AI 专属目录: %s", absolute)
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
