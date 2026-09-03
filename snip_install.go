package main

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
)

type snipAgentState struct {
	Name         string `json:"name"`
	Directory    string `json:"directory"`
	TargetFile   string `json:"target_file"`
	Available    bool   `json:"available"`
	HookExists   bool   `json:"hook_exists"`
	Configured   bool   `json:"configured"`
	Modified     bool   `json:"modified"`
	RepairNeeded bool   `json:"repair_needed"`
	Trust        string `json:"trust,omitempty"`
}

// snipAgentSpecs 是四个平台的官方 snip 接入约定。所有检测、初始化和清理都从这里取值，
// 避免某个平台只改了目录或目标文件，却遗漏生命周期中的其它步骤。
type snipAgentSpec struct {
	Name       string
	HomeDir    string
	TargetFile string
}

var snipAgentSpecs = []snipAgentSpec{
	{Name: "codex", HomeDir: ".codex", TargetFile: "hooks.json"},
	{Name: "claude-code", HomeDir: ".claude", TargetFile: "settings.json"},
	{Name: "cursor", HomeDir: ".cursor", TargetFile: "hooks.json"},
	{Name: "copilot", HomeDir: ".copilot", TargetFile: filepath.Join("hooks", "snip.json")},
}

func snipInitArgs(name string) []string {
	args := []string{"init"}
	if name != "claude-code" {
		args = append(args, "--agent", name)
	}
	return args
}

type snipStatusResponse struct {
	Path            string           `json:"path"`
	Installed       bool             `json:"installed"`
	DirectoryExists bool             `json:"directory_exists"`
	Version         string           `json:"version"`
	UserPath        bool             `json:"user_path"`
	SystemPath      bool             `json:"system_path"`
	Running         bool             `json:"running"`
	DesiredRunning  bool             `json:"desired_running"`
	ActivationState string           `json:"activation_state"`
	BlockedBy       string           `json:"blocked_by,omitempty"`
	TrustNotice     string           `json:"trust_notice,omitempty"`
	TrustStatus     string           `json:"trust_status,omitempty"`
	TrustCommand    string           `json:"trust_command,omitempty"`
	TrustRequired   bool             `json:"trust_required,omitempty"`
	TrustSteps      []string         `json:"trust_steps,omitempty"`
	TrustShellOpen  bool             `json:"trust_shell_open,omitempty"`
	CleanupRequired bool             `json:"cleanup_required,omitempty"`
	ModifiedAgents  []string         `json:"modified_agents,omitempty"`
	Agents          []snipAgentState `json:"agents"`
	Message         string           `json:"message"`
}

type snipAgentSnapshot struct {
	Path               string
	Exists             bool
	Data               []byte
	CreatedDirectories []string
}

func snapshotSnipAgentFiles() (map[string]snipAgentSnapshot, error) {
	result := make(map[string]snipAgentSnapshot, 4)
	for _, agent := range snipAgentStates() {
		snapshot := snipAgentSnapshot{Path: agent.TargetFile}
		if data, err := os.ReadFile(agent.TargetFile); err == nil {
			snapshot.Exists = true
			snapshot.Data = append([]byte(nil), data...)
		} else if errors.Is(err, os.ErrNotExist) {
			for directory := filepath.Dir(agent.TargetFile); directory != ""; directory = filepath.Dir(directory) {
				if _, statErr := os.Stat(directory); statErr == nil {
					break
				} else if !errors.Is(statErr, os.ErrNotExist) {
					break
				}
				snapshot.CreatedDirectories = append(snapshot.CreatedDirectories, directory)
				parent := filepath.Dir(directory)
				if parent == directory {
					break
				}
			}
		} else {
			return nil, fmt.Errorf("读取 %s Hook 快照失败: %w", agent.Name, err)
		}
		result[agent.Name] = snapshot
	}
	return result, nil
}

func changedSnipAgentFiles(before map[string]snipAgentSnapshot) []string {
	result := make([]string, 0, len(before))
	for name, snapshot := range before {
		after, err := os.ReadFile(snapshot.Path)
		if err == nil && (!snapshot.Exists || !bytes.Equal(snapshot.Data, after)) {
			result = append(result, name)
		}
	}
	sort.Strings(result)
	return result
}

func restoreSnipAgentSnapshots(before map[string]snipAgentSnapshot) error {
	for _, snapshot := range before {
		if !snapshot.Exists {
			if err := os.Remove(snapshot.Path); err != nil && !errors.Is(err, os.ErrNotExist) {
				return fmt.Errorf("删除新建的 Agent Hook 失败: %w", err)
			}
			for _, directory := range snapshot.CreatedDirectories {
				// 目录中若已出现用户内容，保留它而不是把回滚变成一次越界删除。
				_ = os.Remove(directory)
			}
			continue
		}
		current, err := os.ReadFile(snapshot.Path)
		if err != nil || !bytes.Equal(current, snapshot.Data) {
			if err := os.MkdirAll(filepath.Dir(snapshot.Path), 0o755); err != nil {
				return err
			}
			if err := replaceUTF8File(snapshot.Path, snapshot.Data); err != nil {
				return err
			}
		}
	}
	return nil
}

type snipInstallResponse struct {
	Path           string `json:"path"`
	Version        string `json:"version"`
	Running        bool   `json:"running"`
	DesiredRunning bool   `json:"desired_running"`
	Message        string `json:"message"`
}

func ensureSnipStoppedForInstall(state managedToolState) error {
	if state.Running || state.DesiredRunning {
		return &httpError{status: http.StatusConflict, message: "snip 正在运行，请先停止 snip"}
	}
	return nil
}

func snipInstallDirectory() (string, error) {
	executable, err := os.Executable()
	if err != nil {
		return "", fmt.Errorf("无法定位 code-Manager.exe: %w", err)
	}
	return filepath.Join(filepath.Dir(executable), "Snip"), nil
}

func snipExecutablePath() (string, error) {
	directory, err := snipInstallDirectory()
	if err != nil {
		return "", err
	}
	return filepath.Join(directory, "snip.exe"), nil
}

// snipAgentDirectory 必须和 snip v0.25.0 的 init 实现使用同一套目录规则。
// Codex 的官方 init 固定写入 os.UserHomeDir()/.codex，不读取 CODEX_HOME；Claude
// Code 则明确支持 CLAUDE_CONFIG_DIR。这里不能为了页面展示而另行推断目录，否则
// 快照、归属状态与 snip.exe 实际修改的位置会分叉。
func snipAgentDirectory(spec snipAgentSpec, home string) string {
	if spec.Name == "claude-code" {
		if configured := os.Getenv("CLAUDE_CONFIG_DIR"); configured != "" {
			return configured
		}
	}
	return filepath.Join(home, spec.HomeDir)
}

func snipAgentDirectories() map[string]string {
	home, err := os.UserHomeDir()
	if err != nil {
		return nil
	}
	result := make(map[string]string, len(snipAgentSpecs))
	for _, spec := range snipAgentSpecs {
		result[spec.Name] = snipAgentDirectory(spec, home)
	}
	return result
}

func snipAgentStates() []snipAgentState {
	result := make([]snipAgentState, 0, 4)
	directories := snipAgentDirectories()
	for _, spec := range snipAgentSpecs {
		directory := directories[spec.Name]
		if directory == "" {
			continue
		}
		if info, err := os.Stat(directory); err == nil && info.IsDir() {
			target := snipAgentHookFile(spec.Name, directory)
			_, targetErr := os.Stat(target)
			result = append(result, snipAgentState{Name: spec.Name, Directory: directory, TargetFile: target, Available: true, HookExists: targetErr == nil})
		}
	}
	return result
}

func snipAgentHookFile(name, directory string) string {
	for _, spec := range snipAgentSpecs {
		if spec.Name == name {
			return filepath.Join(directory, spec.TargetFile)
		}
	}
	return ""
}

func snipAgentCanModify(agent snipAgentState) bool {
	if agent.TargetFile == "" || !agent.Available {
		return false
	}
	return true
}

// snipExistingHookAgents 返回用户级 Agent 目录已经存在的 Agent。
// snip init 会按 Agent 的原生约定创建缺失的 Hook 文件。
func snipExistingHookAgents() []snipAgentState {
	result := make([]snipAgentState, 0, 4)
	for _, agent := range snipAgentStates() {
		if snipAgentCanModify(agent) {
			result = append(result, agent)
		}
	}
	return result
}

func snipAgentsNeedingInit(agents []snipAgentState, ledger snipOwnershipLedger, executable string, repairOwned bool) ([]snipAgentState, error) {
	result := make([]snipAgentState, 0, len(agents))
	for _, agent := range agents {
		_, artifact, owned := snipOwnershipArtifactForTarget(ledger, agent.Name, agent.TargetFile)
		if owned && repairOwned {
			if !snipHookCommandMatchesExecutable(artifact.Command, agent.Name, executable) {
				return nil, fmt.Errorf("受管 Snip %s Hook 仍指向另一发布目录；为避免覆盖该旧 Hook，请先停止并清理后再启动", agent.Name)
			}
			presence, err := snipOwnedHookPresenceAt(artifact)
			if err != nil {
				return nil, fmt.Errorf("检查受管 Snip %s Hook 失败: %w", agent.Name, err)
			}
			if presence == snipOwnedHookExact {
				continue
			}
			hasSnipHook, err := snipAgentHasAnySnipHookAt(agent.Name, agent.TargetFile)
			if err != nil {
				return nil, fmt.Errorf("检查 Snip %s Hook 失败: %w", agent.Name, err)
			}
			if hasSnipHook {
				return nil, fmt.Errorf("受管 Snip %s Hook 已被改写、移动或替换，且目标文件仍含 Snip Hook；为避免官方 init 覆盖手工配置，请先停止并处理该 Hook", agent.Name)
			}
			result = append(result, agent)
			continue
		}
		hasSnipHook, err := snipAgentHasAnySnipHookAt(agent.Name, agent.TargetFile)
		if err != nil {
			return nil, fmt.Errorf("检查 Snip %s Hook 失败: %w", agent.Name, err)
		}
		if !hasSnipHook {
			result = append(result, agent)
		}
	}
	return result, nil
}

// Snip 0.25.0 的 Claude Code 与 Cursor 初始化命令没有可靠引用含空格的二进制路径。
// 在写入 PATH 或 Hook 前阻止这类部署，避免页面显示成功而对应 Hook 实际无法启动。
func ensureSnipHookCommandPathSupported(executable string, agents []snipAgentState) error {
	if !strings.ContainsAny(executable, " \t") {
		return nil
	}
	unsupported := make([]string, 0, 2)
	for _, agent := range agents {
		if agent.Name == "claude-code" || agent.Name == "cursor" {
			unsupported = append(unsupported, agent.Name)
		}
	}
	if len(unsupported) == 0 {
		return nil
	}
	return fmt.Errorf("Snip 0.25.0 的官方 %s Hook 初始化不能安全处理含空格的 snip.exe 路径；请将 code-Manager.exe 放到不含空格的目录后重新启动 Snip", strings.Join(unsupported, "、"))
}

func snipAgentsToClean(state managedToolState) []string {
	result := make([]string, 0, 4)
	for _, name := range state.OwnedAgents {
		if isKnownSnipAgent(name) && !containsString(result, name) {
			result = append(result, name)
		}
	}
	return result
}

func isKnownSnipAgent(name string) bool {
	for _, spec := range snipAgentSpecs {
		if spec.Name == name {
			return true
		}
	}
	return false
}

func executeSnipCommand(ctx context.Context, executable string, args ...string) error {
	command := exec.CommandContext(ctx, executable, args...)
	output, err := command.CombinedOutput()
	if err != nil {
		detail := strings.TrimSpace(string(bytes.ToValidUTF8(output, []byte("?"))))
		if len(detail) > 8<<10 {
			detail = detail[:8<<10] + "…"
		}
		if detail != "" {
			return fmt.Errorf("snip %s 失败: %w: %s", strings.Join(args, " "), err, detail)
		}
		return fmt.Errorf("snip %s 失败: %w", strings.Join(args, " "), err)
	}
	return nil
}

func snipCodexHookTrust(executable string) (string, string, *bool, error) {
	directory := snipAgentDirectories()["codex"]
	if directory == "" {
		return "", "", nil, nil
	}
	configPath := filepath.Join(directory, "config.toml")
	data, err := os.ReadFile(configPath)
	if errors.Is(err, os.ErrNotExist) {
		return "Codex Hook 使用默认设置，可能需要点击“添加信任”", "", nil, nil
	}
	if err != nil {
		return "无法读取 Codex Hook 设置，可能需要点击“添加信任”", "", nil, err
	}
	if key, _, found := findCodexFeatureHookToggle(string(data), false); found {
		// 明确关闭属于用户/管理员策略，不能由 code-Manager 擅自覆盖。
		return "Codex Hook 已明确关闭，请在 Codex 配置中手动开启；开启后再点击“添加信任”", key, nil, nil
	}
	if _, _, found := findCodexFeatureHookToggle(string(data), true); found {
		return "Codex Hook 已开启，可能需要点击“添加信任”", "", nil, nil
	}
	// 没有明确字段时按 Codex 默认行为处理，不创建或改写 config.toml。
	return "Codex Hook 使用默认开启，可能需要点击“添加信任”", "", nil, nil
}

// findCodexFeatureHookToggle 只识别 [features] 下明确的 hooks/codex_hooks 布尔开关。
func findCodexFeatureHookToggle(text string, wantValue bool) (key string, lineIndex int, found bool) {
	section := ""
	for index, line := range strings.Split(text, "\n") {
		trimmed := strings.TrimSpace(line)
		if strings.HasPrefix(trimmed, "[") && strings.HasSuffix(trimmed, "]") {
			section = strings.TrimSpace(strings.TrimSuffix(strings.TrimPrefix(trimmed, "["), "]"))
			continue
		}
		if section != "features" {
			continue
		}
		before, after, ok := strings.Cut(trimmed, "=")
		if !ok {
			continue
		}
		name := strings.TrimSpace(before)
		if name != "hooks" && name != "codex_hooks" {
			continue
		}
		value := strings.TrimSpace(strings.SplitN(after, "#", 2)[0])
		if (value == "true") == wantValue {
			return name, index, true
		}
	}
	return "", -1, false
}

func (g *gateway) snipStatus(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	directory, err := snipInstallDirectory()
	if err != nil {
		http.Error(w, err.Error(), http.StatusInternalServerError)
		return
	}
	executable := filepath.Join(directory, "snip.exe")
	info, statErr := os.Stat(executable)
	installed := statErr == nil && !info.IsDir()
	directoryExists := false
	if dirInfo, err := os.Stat(directory); err == nil {
		directoryExists = dirInfo.IsDir()
	}
	state, stateErr := readManagedToolState(directory)
	userPath, systemPath, pathMessage := querySnipPathStatus(directory)
	appendStatusIssue := func(issue string) {
		issue = strings.TrimSpace(issue)
		if issue == "" {
			return
		}
		if pathMessage != "" {
			pathMessage += "；"
		}
		pathMessage += issue
	}
	ledger := emptySnipOwnershipLedger()
	var ownershipErr error
	if stateErr == nil {
		ledger, ownershipErr = recoverLegacySnipOwnership(state, executable)
	}
	if stateErr != nil {
		appendStatusIssue("无法读取 snip 状态: " + stateErr.Error())
	}
	if ownershipErr != nil {
		appendStatusIssue("无法读取 snip 精确归属: " + ownershipErr.Error())
	}
	agents := snipAgentStates()
	codexTrust, codexTrustErr := detectSnipCodexTrust()
	modifiedAgents := make([]string, 0, len(agents))
	repairAgents := make([]string, 0, len(agents))
	for i := range agents {
		configured, configuredErr := snipAgentHookPresentAt(agents[i].Name, agents[i].TargetFile)
		if configuredErr != nil {
			appendStatusIssue(fmt.Sprintf("无法读取 %s Hook: %v", agents[i].Name, configuredErr))
		} else {
			hasSnipHook, anyHookErr := snipAgentHasAnySnipHookAt(agents[i].Name, agents[i].TargetFile)
			if anyHookErr != nil {
				appendStatusIssue(fmt.Sprintf("无法读取 %s Snip Hook: %v", agents[i].Name, anyHookErr))
			} else {
				configured = configured || hasSnipHook
			}
		}
		agents[i].Configured = configured
		if ownershipErr == nil {
			if _, artifact, owned := snipOwnershipArtifactForTarget(ledger, agents[i].Name, agents[i].TargetFile); owned {
				if !snipHookCommandMatchesExecutable(artifact.Command, agents[i].Name, executable) {
					agents[i].RepairNeeded = true
				} else {
					presence, presenceErr := snipOwnedHookPresenceAt(artifact)
					if presenceErr != nil {
						appendStatusIssue(fmt.Sprintf("无法检查受管 %s Hook: %v", agents[i].Name, presenceErr))
						agents[i].RepairNeeded = true
					} else {
						agents[i].Modified = presence == snipOwnedHookExact
						agents[i].RepairNeeded = presence != snipOwnedHookExact
					}
				}
			}
		}
		if agents[i].Modified {
			modifiedAgents = append(modifiedAgents, agents[i].Name)
		}
		if agents[i].RepairNeeded && !containsString(repairAgents, agents[i].Name) {
			repairAgents = append(repairAgents, agents[i].Name)
		}
		if agents[i].Name == "codex" && agents[i].Configured {
			agents[i].Trust = codexTrust.Status
		}
	}
	if ownershipErr == nil {
		ledgerRepairAgents, ownershipStatusErr := snipOwnedAgentsNeedingRepair(ledger, executable)
		if ownershipStatusErr != nil {
			appendStatusIssue(ownershipStatusErr.Error())
		} else {
			for _, agent := range ledgerRepairAgents {
				if !containsString(repairAgents, agent) {
					repairAgents = append(repairAgents, agent)
				}
			}
		}
		if state.Running || state.DesiredRunning {
			currentTargetsNeedingInit, inspectErr := snipAgentsNeedingInit(agents, ledger, executable, true)
			if inspectErr != nil {
				appendStatusIssue(inspectErr.Error())
			} else {
				for _, agent := range currentTargetsNeedingInit {
					if !containsString(repairAgents, agent.Name) {
						repairAgents = append(repairAgents, agent.Name)
					}
				}
			}
		}
		for i := range agents {
			if containsString(repairAgents, agents[i].Name) {
				agents[i].RepairNeeded = true
			}
		}
	}
	legacyOwnershipPending := stateErr == nil && ownershipErr == nil && len(state.OwnedAgents) > 0 && len(ledger.Artifacts) == 0
	cleanupRequired := false
	unexpectedResidual := false
	var residualErr error
	if stateErr == nil && ownershipErr == nil && !state.Running && !state.DesiredRunning {
		cleanupRequired = snipOwnershipNeedsCleanup(state, ledger) || legacyOwnershipPending
		unexpectedResidual, residualErr = snipActivationArtifactsPresent(state, ledger)
		if residualErr != nil {
			appendStatusIssue("无法检查 snip 受管残留: " + residualErr.Error())
		}
	}
	activation := "not_installed"
	if installed {
		activation = "installed_stopped"
		if state.Running || state.DesiredRunning {
			activation = "running"
		}
		if stateErr != nil || ownershipErr != nil || residualErr != nil || pathMessage != "" || managedToolStateHasAttention(state) || ((state.Running || state.DesiredRunning) && len(repairAgents) > 0) || (!state.Running && !state.DesiredRunning && (unexpectedResidual || cleanupRequired)) {
			activation = "attention"
		}
	}
	blockedBy := ""
	if running, _ := managedToolActive("rtk"); running && (state.Running || state.DesiredRunning) {
		activation = "conflict"
		blockedBy = "rtk"
	}
	trustNotice := codexTrust.Notice
	if codexTrustErr != nil && trustNotice == "" {
		trustNotice = "无法完整读取 Codex 信任状态，请点击“添加信任”，并在 PowerShell 审核界面选择第 2 项后按键盘 Enter（回车）。"
	}
	message := "snip 尚未安装。"
	if installed {
		if state.Running || state.DesiredRunning {
			message = "snip 已启动，原生 Hook 已配置。"
		} else {
			message = "snip 已安装/已停止。"
		}
	}
	if state.Metadata != nil {
		if trustNotice == "" {
			trustNotice = state.Metadata["trust_notice"]
		}
	}
	if pathMessage != "" {
		message += " " + pathMessage
	}
	if (state.Running || state.DesiredRunning) && len(repairAgents) > 0 {
		message += " 受管 Hook 已缺失、改写或移动：" + strings.Join(repairAgents, "、") + "。为避免覆盖手工配置，先停止并处理该 Hook；不存在其它 Snip Hook 时可再次启动接入。"
	}
	if cleanupRequired {
		message += " 检测到已停止 snip 的受管状态或 Hook 残留，请点击“清理”；无法精确确认归属的手工 Hook 会保留。"
	}
	trustRequired := codexTrust.Status == snipTrustUntrusted || codexTrust.Status == snipTrustUnknown || codexTrust.Status == snipTrustDisabled
	writeJSON(w, http.StatusOK, snipStatusResponse{Path: executable, Installed: installed, DirectoryExists: directoryExists, Version: readSnipVersion(directory), UserPath: userPath, SystemPath: systemPath, Running: state.Running, DesiredRunning: state.DesiredRunning, ActivationState: activation, BlockedBy: blockedBy, TrustNotice: trustNotice, TrustStatus: codexTrust.Status, TrustCommand: codexTrust.Command, TrustRequired: trustRequired, TrustSteps: codexTrust.Steps, CleanupRequired: cleanupRequired, ModifiedAgents: modifiedAgents, Agents: agents, Message: message})
}

func (g *gateway) snipInstall(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	if g.isShuttingDown() {
		http.Error(w, "code-Manager 正在退出，不能安装 snip", http.StatusServiceUnavailable)
		return
	}
	var input struct {
		TagName string `json:"tag_name"`
	}
	decoder := json.NewDecoder(http.MaxBytesReader(w, r.Body, 8<<10))
	if err := decoder.Decode(&input); err != nil || strings.TrimSpace(input.TagName) == "" {
		http.Error(w, "请选择要安装的 snip 版本", http.StatusBadRequest)
		return
	}
	var response snipInstallResponse
	err := withManagedToolLock(func() error {
		if running, _ := managedToolActive("rtk"); running {
			return &httpError{status: http.StatusConflict, message: "RTK 正在运行，请先停止 RTK"}
		}
		directory, err := snipInstallDirectory()
		if err != nil {
			return err
		}
		state, err := readManagedToolState(directory)
		if err != nil {
			return fmt.Errorf("读取 snip 状态失败: %w", err)
		}
		if err := ensureSnipStoppedForInstall(state); err != nil {
			return err
		}
		migrated, err := migrateSnipInstallationsBeforeInstall(r.Context(), directory)
		if err != nil {
			return fmt.Errorf("安装前迁移旧 snip 环境失败: %w", err)
		}
		if err := g.downloadAndInstallSnip(r.Context(), strings.TrimSpace(input.TagName), directory); err != nil {
			return err
		}
		if err := writeSnipVersion(directory, input.TagName); err != nil {
			_ = removeOwnedSnipDirectory(directory)
			return err
		}
		if err := writeManagedToolState(directory, managedToolState{DesiredRunning: false, Running: false}); err != nil {
			_ = removeOwnedSnipDirectory(directory)
			return err
		}
		if err := recordManagedToolInstallation("snip", directory); err != nil {
			_ = removeOwnedSnipDirectory(directory)
			return err
		}
		message := fmt.Sprintf("snip %s 已安装，当前为已安装/已停止。", input.TagName)
		if len(migrated) > 0 {
			message += " 已自动清理本程序旧版本留下的受管 PATH 和已记录 Hook。"
		}
		response = snipInstallResponse{Path: filepath.Join(directory, "snip.exe"), Version: input.TagName, Message: message}
		return nil
	})
	if err != nil {
		if httpErr, ok := err.(*httpError); ok {
			http.Error(w, httpErr.message, httpErr.status)
			return
		}
		http.Error(w, "安装 snip 失败: "+err.Error(), http.StatusBadGateway)
		return
	}
	writeJSON(w, http.StatusOK, response)
}

func (g *gateway) snipStart(w http.ResponseWriter, r *http.Request) { g.runSnipLifecycle(w, r, true) }
func (g *gateway) snipStop(w http.ResponseWriter, r *http.Request)  { g.runSnipLifecycle(w, r, false) }

// stopSnip 只清理 code-Manager 有明确归属记录的 Snip PATH 和 Hook。Snip 官方
// --uninstall 会删除同一文件中的所有 Snip Hook，不能用于受管与手工接入共存的场景。
func (g *gateway) stopSnip(ctx context.Context) error {
	return withManagedToolLock(func() error {
		directory, err := snipInstallDirectory()
		if err != nil {
			return err
		}
		state, err := readManagedToolState(directory)
		if err != nil {
			return fmt.Errorf("Snip 状态账本不可读，无法安全判断 Hook/PATH 归属: %w", err)
		}
		ledger, err := recoverLegacySnipOwnership(state, filepath.Join(directory, "snip.exe"))
		if err != nil {
			return err
		}
		if !state.Running && !state.DesiredRunning && !snipOwnershipNeedsCleanup(state, ledger) && len(state.OwnedAgents) == 0 {
			return nil
		}
		if err := removeSnipOwnedArtifacts(ledger); err != nil {
			return err
		}
		if err := removeOwnedIndependentPath(ctx, directory, state.UserPath, state.SystemPath); err != nil {
			return err
		}
		state.DesiredRunning, state.Running = false, false
		state.UserPath, state.SystemPath = false, false
		clearSnipOwnershipLedger(&state)
		if state.Metadata != nil {
			delete(state.Metadata, "last_error")
		}
		return writeManagedToolState(directory, state)
	})
}

// snipTrust opens the Codex CLI review screen independently from the Snip
// lifecycle. Starting Snip only configures platform hooks; this action is the
// explicit user request to launch the Codex trust review.
func (g *gateway) snipTrust(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	if g.isShuttingDown() {
		http.Error(w, "code-Manager 正在退出，不能打开 Codex 信任窗口", http.StatusServiceUnavailable)
		return
	}
	directory, err := snipInstallDirectory()
	if err != nil {
		http.Error(w, err.Error(), http.StatusInternalServerError)
		return
	}
	executable := filepath.Join(directory, "snip.exe")
	if _, err := os.Stat(executable); err != nil {
		http.Error(w, "snip 尚未安装", http.StatusConflict)
		return
	}
	info, detectErr := detectSnipCodexTrust()
	if detectErr != nil && info.Status != snipTrustUnknown {
		http.Error(w, "读取 Codex 信任状态失败: "+detectErr.Error(), http.StatusBadGateway)
		return
	}
	if info.Status == snipTrustNotApplicable {
		http.Error(w, "未检测到 Codex Snip Hook，请先点击 snip“启动”完成 Hook 接入", http.StatusConflict)
		return
	}
	if info.Status == snipTrustTrusted {
		writeJSON(w, http.StatusOK, snipStatusResponse{
			Path:         executable,
			Installed:    true,
			TrustStatus:  info.Status,
			TrustNotice:  info.Notice,
			TrustCommand: info.Command,
			TrustSteps:   info.Steps,
			Message:      "Codex Hook 已信任，无需重复添加。",
		})
		return
	}
	if info.Status == snipTrustDisabled {
		http.Error(w, info.Notice, http.StatusConflict)
		return
	}
	if err := openCodexTrustShell(&info); err != nil {
		http.Error(w, "打开 Codex 信任 PowerShell 失败: "+err.Error(), http.StatusBadGateway)
		return
	}
	writeJSON(w, http.StatusOK, snipStatusResponse{
		Path:            executable,
		Installed:       true,
		DirectoryExists: true,
		TrustStatus:     info.Status,
		TrustNotice:     info.Notice,
		TrustCommand:    info.Command,
		TrustRequired:   true,
		TrustSteps:      info.Steps,
		TrustShellOpen:  true,
		Message:         "已打开 PowerShell，请在 Hooks need review 界面选择第 2 项，然后按键盘 Enter（回车）。",
	})
}

func (g *gateway) startSnip(ctx context.Context) error {
	return withManagedToolLock(func() error {
		directory, err := snipInstallDirectory()
		if err != nil {
			return err
		}
		state, err := readManagedToolState(directory)
		if err != nil {
			return err
		}
		if running, _ := managedToolActive("rtk"); running {
			return errors.New("RTK 正在运行")
		}
		executable := filepath.Join(directory, "snip.exe")
		if _, err := os.Stat(executable); err != nil {
			return errors.New("snip 尚未安装")
		}
		ledger, err := recoverLegacySnipOwnership(state, executable)
		if err != nil {
			return err
		}
		active := state.Running || state.DesiredRunning
		legacyOwnershipPending := len(state.OwnedAgents) > 0 && len(ledger.Artifacts) == 0
		if !active && (snipOwnershipNeedsCleanup(state, ledger) || legacyOwnershipPending) {
			return errors.New("snip 已停止但仍保留本程序受管的 Hook、PATH 或旧账本记录，请先点击“清理”；无法精确确认归属的 Hook 会保留")
		}
		availableAgents := snipExistingHookAgents()
		agents, err := snipAgentsNeedingInit(availableAgents, ledger, executable, active)
		if err != nil {
			return err
		}
		if err := ensureSnipHookCommandPathSupported(executable, agents); err != nil {
			return err
		}
		if err := ensureSnipCodexRuntimeHookSupported(ctx, agents); err != nil {
			return err
		}
		if active {
			userPath, systemPath, pathMessage := querySnipPathStatus(directory)
			repairAgents, repairErr := snipOwnedAgentsNeedingRepair(ledger, executable)
			if repairErr != nil {
				return repairErr
			}
			if len(agents) == 0 && len(repairAgents) == 0 && pathMessage == "" && userPath && systemPath {
				if err := writeSnipOwnershipLedger(&state, ledger); err != nil {
					return fmt.Errorf("迁移 snip 精确归属状态失败: %w", err)
				}
				if state.Metadata != nil {
					delete(state.Metadata, "last_error")
				}
				return writeManagedToolState(directory, state)
			}
		}
		agentBefore, err := snapshotSnipAgentFiles()
		if err != nil {
			return err
		}
		userPathBefore, systemPathBefore, pathMessage := querySnipPathStatus(directory)
		if pathMessage != "" {
			return errors.New(pathMessage)
		}
		userPathAdded, systemPathAdded := !userPathBefore, !systemPathBefore
		if _, _, err := configureSnipPath(ctx, directory); err != nil {
			return err
		}
		for _, agent := range agents {
			// 仅当目标事件中没有任何 Snip 命令时才会调用官方 init；这样不会让
			// 上游的宽匹配逻辑覆盖已有的手工 Snip Hook。
			if err := executeSnipCommand(ctx, executable, snipInitArgs(agent.Name)...); err != nil {
				_ = restoreSnipAgentSnapshots(agentBefore)
				_ = removeOwnedIndependentPath(context.Background(), directory, userPathAdded, systemPathAdded)
				return err
			}
			artifact, ownershipErr := snipHookOwnershipAddedByInit(agent.Name, agentBefore[agent.Name], executable)
			if ownershipErr != nil {
				_ = restoreSnipAgentSnapshots(agentBefore)
				_ = removeOwnedIndependentPath(context.Background(), directory, userPathAdded, systemPathAdded)
				return fmt.Errorf("snip %s 初始化后无法确认受管 Hook: %w", agent.Name, ownershipErr)
			}
			ledger.Artifacts[snipArtifactKeyForOwnership(agent.Name, artifact.TargetPath)] = artifact
		}
		if len(ledger.Artifacts) == 0 {
			_ = restoreSnipAgentSnapshots(agentBefore)
			_ = removeOwnedIndependentPath(context.Background(), directory, userPathAdded, systemPathAdded)
			if len(agents) == 0 {
				return errors.New("未发现可由 code-Manager 安全接入的用户级 Agent Hook；已有的非受管 Snip Hook 不会被覆盖")
			}
			return errors.New("snip 初始化后未发现可精确记录的受管 Hook")
		}
		state.DesiredRunning, state.Running = true, true
		state.UserPath = state.UserPath || userPathAdded
		state.SystemPath = state.SystemPath || systemPathAdded
		if err := writeSnipOwnershipLedger(&state, ledger); err != nil {
			_ = restoreSnipAgentSnapshots(agentBefore)
			_ = removeOwnedIndependentPath(context.Background(), directory, userPathAdded, systemPathAdded)
			return fmt.Errorf("保存 snip 精确归属状态失败: %w", err)
		}
		if state.Metadata != nil {
			delete(state.Metadata, "last_error")
		}
		if state.Metadata == nil {
			state.Metadata = map[string]string{}
		}
		state.Metadata["activation_recorded"] = "true"
		if err := writeManagedToolState(directory, state); err != nil {
			// 状态无法落盘时回滚已执行的 init/PATH，避免留下无法停止的 Hook。
			var cleanupErrs []string
			if cleanupErr := restoreSnipAgentSnapshots(agentBefore); cleanupErr != nil {
				cleanupErrs = append(cleanupErrs, cleanupErr.Error())
			}
			if cleanupErr := removeOwnedIndependentPath(context.Background(), directory, userPathAdded, systemPathAdded); cleanupErr != nil {
				cleanupErrs = append(cleanupErrs, cleanupErr.Error())
			}
			if len(cleanupErrs) > 0 {
				return fmt.Errorf("保存 snip 状态失败: %w；回滚激活配置失败: %s", err, strings.Join(cleanupErrs, "；"))
			}
			return fmt.Errorf("保存 snip 状态失败: %w", err)
		}
		return nil
	})
}

func (g *gateway) runSnipLifecycle(w http.ResponseWriter, r *http.Request, start bool) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	if start {
		if running, _ := managedToolActive("rtk"); running {
			http.Error(w, "RTK 正在运行，请先停止 RTK", http.StatusConflict)
			return
		}
		if err := g.startSnip(r.Context()); err != nil {
			if directory, dirErr := snipInstallDirectory(); dirErr == nil {
				state, _ := readManagedToolState(directory)
				markManagedToolAttention(directory, state, err)
			}
			http.Error(w, "启动 snip 失败: "+err.Error(), http.StatusBadGateway)
			return
		}
		directory, _ := snipInstallDirectory()
		state, _ := readManagedToolState(directory)
		writeJSON(w, http.StatusOK, snipStatusResponse{Path: filepath.Join(directory, "snip.exe"), Installed: true, DirectoryExists: true, Version: readSnipVersion(directory), Running: state.Running, DesiredRunning: state.DesiredRunning, ActivationState: "running", Message: "snip 已启动，原生 Hook 已配置。"})
		return
	}
	err := g.stopSnip(r.Context())
	if err != nil {
		if directory, dirErr := snipInstallDirectory(); dirErr == nil {
			state, _ := readManagedToolState(directory)
			markManagedToolAttention(directory, state, err)
		}
		if httpErr, ok := err.(*httpError); ok {
			http.Error(w, httpErr.message, httpErr.status)
			return
		}
		http.Error(w, "snip 操作失败: "+err.Error(), http.StatusBadGateway)
		return
	}
	directory, _ := snipInstallDirectory()
	state, _ := readManagedToolState(directory)
	writeJSON(w, http.StatusOK, snipStatusResponse{Path: filepath.Join(directory, "snip.exe"), Installed: true, DirectoryExists: true, Version: readSnipVersion(directory), Running: state.Running, DesiredRunning: state.DesiredRunning, ActivationState: func() string {
		if state.Running {
			return "running"
		}
		return "installed_stopped"
	}(), Message: func() string {
		if start {
			return "snip 已启动，原生 Hook 已配置。"
		}
		return "snip 已停止，原生 Hook 已清理。"
	}()})
}

func (g *gateway) snipUninstall(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	if err := g.uninstallSnip(r.Context()); err != nil {
		if httpErr, ok := err.(*httpError); ok {
			http.Error(w, httpErr.message, httpErr.status)
			return
		}
		http.Error(w, "删除 snip 失败: "+err.Error(), http.StatusBadGateway)
		return
	}
	writeJSON(w, http.StatusOK, snipStatusResponse{Path: "", Installed: false, DirectoryExists: false, ActivationState: "not_installed", Message: "snip 已删除。"})
}

func (g *gateway) uninstallSnip(ctx context.Context) error {
	return withManagedToolLock(func() error {
		directory, err := snipInstallDirectory()
		if err != nil {
			return err
		}
		state, err := readManagedToolState(directory)
		if err != nil {
			return err
		}
		if state.Running || state.DesiredRunning {
			return &httpError{status: http.StatusConflict, message: "请先停止 snip，再删除"}
		}
		if running, _ := managedToolActive("rtk"); running {
			return &httpError{status: http.StatusConflict, message: "RTK 正在运行，请先停止 RTK"}
		}
		ledger, err := recoverLegacySnipOwnership(state, filepath.Join(directory, "snip.exe"))
		if err != nil {
			return err
		}
		if err := removeSnipOwnedArtifacts(ledger); err != nil {
			return err
		}
		if err := removeOwnedIndependentPath(ctx, directory, state.UserPath, state.SystemPath); err != nil {
			return err
		}
		if err := removeOwnedSnipDirectory(directory); err != nil {
			return err
		}
		if err := forgetManagedToolInstallation("snip", directory); err != nil {
			log.Printf("移除 snip 安装记录失败: %v", err)
		}
		return nil
	})
}

type httpError struct {
	status  int
	message string
}

func (e *httpError) Error() string { return e.message }

func readSnipVersion(directory string) string {
	data, err := os.ReadFile(filepath.Join(directory, ".snip-version"))
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(data))
}
func writeSnipVersion(directory, version string) error {
	return os.WriteFile(filepath.Join(directory, ".snip-version"), []byte(strings.TrimSpace(version)+"\n"), 0o644)
}
func removeOwnedSnipDirectory(directory string) error {
	absolute, err := filepath.Abs(directory)
	if err != nil {
		return err
	}
	if !strings.EqualFold(filepath.Base(filepath.Clean(absolute)), "snip") {
		return fmt.Errorf("拒绝删除非 Snip 专属目录: %s", absolute)
	}
	return os.RemoveAll(absolute)
}

func managedToolRunning(name string) (bool, error) {
	var directory string
	var err error
	if name == "rtk" {
		directory, err = rtkInstallDirectory()
	} else {
		directory, err = snipInstallDirectory()
	}
	if err != nil {
		return false, err
	}
	state, err := readManagedToolState(directory)
	return state.Running, err
}

func managedToolActive(name string) (bool, error) {
	var directory string
	var err error
	if name == "rtk" {
		directory, err = rtkInstallDirectory()
	} else {
		directory, err = snipInstallDirectory()
	}
	if err != nil {
		return false, err
	}
	state, err := readManagedToolState(directory)
	if err != nil {
		// 状态文件损坏时必须 fail-closed，避免另一工具趁机启动。
		return true, err
	}
	return state.Running || state.DesiredRunning, nil
}

func querySnipPathStatus(directory string) (bool, bool, string) {
	return queryIndependentPathStatus(directory)
}
func configureSnipPath(ctx context.Context, directory string) (bool, bool, error) {
	return configureIndependentPath(ctx, directory)
}
func removeSnipPath(ctx context.Context, directory string) error {
	return removeIndependentPath(ctx, directory)
}
