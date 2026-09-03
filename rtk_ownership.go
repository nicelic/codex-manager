package main

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

const (
	rtkOwnershipMetadataKey = "rtk_ownership_v1"
	rtkOwnershipVersion     = 1

	rtkArtifactCodexPrompt  = "codex-prompt"
	rtkArtifactClaudePrompt = "claude-prompt"
	rtkArtifactClaudeHook   = "claude-hook"
	rtkArtifactCopilotHook  = "copilot-hook"
	rtkArtifactCursorHook   = "cursor-hook"
)

// rtkArtifactOwnership records one concrete RTK mutation. Prompt artifacts are
// identified by the normalized body fingerprint; Hook artifacts by their exact
// command in one exact configuration file.
type rtkArtifactOwnership struct {
	Agent       string `json:"agent"`
	TargetPath  string `json:"target_path"`
	Fingerprint string `json:"fingerprint,omitempty"`
	Command     string `json:"command,omitempty"`
}

type rtkOwnershipLedger struct {
	Version   int                             `json:"version"`
	Artifacts map[string]rtkArtifactOwnership `json:"artifacts"`
}

func emptyRTKOwnershipLedger() rtkOwnershipLedger {
	return rtkOwnershipLedger{Version: rtkOwnershipVersion, Artifacts: map[string]rtkArtifactOwnership{}}
}

func readRTKOwnershipLedger(state managedToolState) (rtkOwnershipLedger, error) {
	if state.Metadata == nil || strings.TrimSpace(state.Metadata[rtkOwnershipMetadataKey]) == "" {
		return emptyRTKOwnershipLedger(), nil
	}
	var ledger rtkOwnershipLedger
	if err := json.Unmarshal([]byte(state.Metadata[rtkOwnershipMetadataKey]), &ledger); err != nil {
		return rtkOwnershipLedger{}, fmt.Errorf("解析 RTK 精确归属账本失败: %w", err)
	}
	if ledger.Version != rtkOwnershipVersion {
		return rtkOwnershipLedger{}, fmt.Errorf("不支持的 RTK 精确归属账本版本: %d", ledger.Version)
	}
	if ledger.Artifacts == nil {
		ledger.Artifacts = map[string]rtkArtifactOwnership{}
	}
	for name, artifact := range ledger.Artifacts {
		if !isKnownRTKArtifact(name) {
			return rtkOwnershipLedger{}, fmt.Errorf("RTK 精确归属账本包含未知项目: %s", name)
		}
		if strings.TrimSpace(artifact.Agent) == "" || strings.TrimSpace(artifact.TargetPath) == "" {
			return rtkOwnershipLedger{}, fmt.Errorf("RTK 精确归属账本项目不完整: %s", name)
		}
		if isRTKPromptArtifact(name) && strings.TrimSpace(artifact.Fingerprint) == "" {
			return rtkOwnershipLedger{}, fmt.Errorf("RTK 提示词归属缺少内容指纹: %s", name)
		}
		if isRTKHookArtifact(name) && strings.TrimSpace(artifact.Command) == "" {
			return rtkOwnershipLedger{}, fmt.Errorf("RTK Hook 归属缺少命令: %s", name)
		}
	}
	return ledger, nil
}

func writeRTKOwnershipLedger(state *managedToolState, ledger rtkOwnershipLedger) error {
	if state == nil {
		return errors.New("RTK 状态为空")
	}
	if ledger.Version == 0 {
		ledger.Version = rtkOwnershipVersion
	}
	if ledger.Version != rtkOwnershipVersion {
		return fmt.Errorf("不支持的 RTK 精确归属账本版本: %d", ledger.Version)
	}
	if ledger.Artifacts == nil {
		ledger.Artifacts = map[string]rtkArtifactOwnership{}
	}
	encoded, err := json.Marshal(ledger)
	if err != nil {
		return err
	}
	if state.Metadata == nil {
		state.Metadata = map[string]string{}
	}
	state.Metadata[rtkOwnershipMetadataKey] = string(encoded)
	state.OwnedAgents = rtkOwnedAgentsFromLedger(ledger)
	return nil
}

func clearRTKOwnershipLedger(state *managedToolState) {
	if state == nil {
		return
	}
	state.OwnedAgents = nil
	if state.Metadata == nil {
		return
	}
	delete(state.Metadata, rtkOwnershipMetadataKey)
	if len(state.Metadata) == 0 {
		state.Metadata = nil
	}
}

func isKnownRTKArtifact(name string) bool {
	switch name {
	case rtkArtifactCodexPrompt, rtkArtifactClaudePrompt, rtkArtifactClaudeHook, rtkArtifactCopilotHook, rtkArtifactCursorHook:
		return true
	default:
		return false
	}
}

func isRTKPromptArtifact(name string) bool {
	return name == rtkArtifactCodexPrompt || name == rtkArtifactClaudePrompt
}

func isRTKHookArtifact(name string) bool {
	return name == rtkArtifactClaudeHook || name == rtkArtifactCopilotHook || name == rtkArtifactCursorHook
}

func rtkOwnedAgentsFromLedger(ledger rtkOwnershipLedger) []string {
	seen := map[string]struct{}{}
	for _, artifact := range ledger.Artifacts {
		if artifact.Agent != "" {
			seen[artifact.Agent] = struct{}{}
		}
	}
	result := make([]string, 0, len(seen))
	for agent := range seen {
		result = append(result, agent)
	}
	sort.Strings(result)
	return result
}

func rtkPromptArtifactForAgent(agent string) string {
	switch agent {
	case "codex":
		return rtkArtifactCodexPrompt
	case "claude-code":
		return rtkArtifactClaudePrompt
	default:
		return ""
	}
}

func rtkPromptFingerprint(payload []byte) string {
	normalized := normalizeRTKBlockBody(string(payload))
	sum := sha256.Sum256([]byte(normalized))
	return hex.EncodeToString(sum[:])
}

func normalizeRTKBlockBody(value string) string {
	value = strings.ReplaceAll(value, "\r\n", "\n")
	value = strings.ReplaceAll(value, "\r", "\n")
	return strings.TrimSuffix(value, "\n")
}

func canonicalRTKTargetPath(path string) string {
	if absolute, err := filepath.Abs(path); err == nil {
		path = absolute
	}
	return filepath.Clean(path)
}

func rtkPromptOwnership(agent, targetPath string, payload []byte) rtkArtifactOwnership {
	return rtkArtifactOwnership{
		Agent:       agent,
		TargetPath:  canonicalRTKTargetPath(targetPath),
		Fingerprint: rtkPromptFingerprint(payload),
	}
}

func rtkHookOwnership(agent, targetPath, command string) rtkArtifactOwnership {
	return rtkArtifactOwnership{
		Agent:      agent,
		TargetPath: canonicalRTKTargetPath(targetPath),
		Command:    strings.TrimSpace(command),
	}
}

func rtkArtifactPresent(name string, artifact rtkArtifactOwnership) (bool, error) {
	switch name {
	case rtkArtifactCodexPrompt, rtkArtifactClaudePrompt:
		return rtkPromptArtifactPresent(artifact)
	case rtkArtifactClaudeHook:
		return rtkClaudeHookConfiguredForCommand(artifact.TargetPath, artifact.Command), nil
	case rtkArtifactCopilotHook:
		return rtkCopilotConfiguredForCommand(artifact.TargetPath, artifact.Command), nil
	case rtkArtifactCursorHook:
		return rtkCursorConfiguredForCommand(artifact.TargetPath, artifact.Command), nil
	default:
		return false, fmt.Errorf("未知 RTK 归属项目: %s", name)
	}
}

func removeRTKArtifact(name string, artifact rtkArtifactOwnership) error {
	switch name {
	case rtkArtifactCodexPrompt, rtkArtifactClaudePrompt:
		_, err := removeRTKPromptArtifact(artifact)
		return err
	case rtkArtifactClaudeHook:
		return removeRTKClaudeIntegrationCommand(artifact.TargetPath, artifact.Command)
	case rtkArtifactCopilotHook:
		return removeRTKCopilotIntegrationCommand(artifact.TargetPath, artifact.Command)
	case rtkArtifactCursorHook:
		return removeRTKCursorHookCommand(artifact.TargetPath, artifact.Command)
	default:
		return fmt.Errorf("未知 RTK 归属项目: %s", name)
	}
}

func removeRTKOwnedArtifacts(ledger rtkOwnershipLedger) error {
	order := []string{
		rtkArtifactCodexPrompt,
		rtkArtifactClaudePrompt,
		rtkArtifactClaudeHook,
		rtkArtifactCopilotHook,
		rtkArtifactCursorHook,
	}
	for _, name := range order {
		artifact, exists := ledger.Artifacts[name]
		if !exists {
			continue
		}
		if err := removeRTKArtifact(name, artifact); err != nil {
			return fmt.Errorf("清理 %s 失败: %w", name, err)
		}
	}
	return nil
}

func rtkExecutablePathOrEmpty() string {
	executable, err := rtkExecutablePath()
	if err != nil {
		return ""
	}
	return executable
}

// recoverLegacyRTKOwnership converts the former platform-level ledger only
// when the concrete artifact can still be identified exactly. It never claims
// an unknown marker block or a Hook from another executable path.
func recoverLegacyRTKOwnership(state managedToolState, executable string) (rtkOwnershipLedger, error) {
	ledger, err := readRTKOwnershipLedger(state)
	if err != nil {
		return rtkOwnershipLedger{}, err
	}
	if len(ledger.Artifacts) > 0 || len(state.OwnedAgents) == 0 {
		return ledger, nil
	}
	targets, err := discoverAssistantTargets()
	if err != nil {
		return rtkOwnershipLedger{}, err
	}
	for _, target := range targets {
		if !containsString(state.OwnedAgents, target.agentName) {
			continue
		}
		artifactName := rtkPromptArtifactForAgent(target.agentName)
		if artifactName == "" {
			continue
		}
		artifact, found, findErr := recoverLegacyRTKPromptOwnership(target)
		if findErr != nil {
			return rtkOwnershipLedger{}, findErr
		}
		if found {
			ledger.Artifacts[artifactName] = artifact
		}
	}
	if executable == "" {
		return ledger, nil
	}
	if containsString(state.OwnedAgents, "claude-code") && rtkClaudeHookConfiguredForExecutable(executable) {
		ledger.Artifacts[rtkArtifactClaudeHook] = rtkHookOwnership("claude-code", rtkClaudeSettingsFile(), rtkClaudeHookCommand(executable))
	}
	if containsString(state.OwnedAgents, "copilot") && rtkCopilotConfiguredForExecutable(executable) {
		ledger.Artifacts[rtkArtifactCopilotHook] = rtkHookOwnership("copilot", rtkCopilotHookFile(), rtkCopilotHookCommand(executable))
	}
	if containsString(state.OwnedAgents, "cursor") && rtkCursorConfiguredForExecutable(executable) {
		ledger.Artifacts[rtkArtifactCursorHook] = rtkHookOwnership("cursor", rtkCursorHooksFile(), rtkCursorHookCommand(executable))
	}
	return ledger, nil
}

func recoverLegacyRTKPromptOwnership(target assistantTarget) (rtkArtifactOwnership, bool, error) {
	data, err := os.ReadFile(target.filePath)
	if errors.Is(err, os.ErrNotExist) {
		return rtkArtifactOwnership{}, false, nil
	}
	if err != nil {
		return rtkArtifactOwnership{}, false, err
	}
	bodies, err := rtkCommandBlockBodies(data)
	if err != nil {
		return rtkArtifactOwnership{}, false, err
	}
	var recognized string
	for _, body := range bodies {
		if !rtkAssistantPromptBlockMatches(target, body) {
			continue
		}
		if recognized != "" {
			// 多段相同的已知内容无法区分哪一段来自旧版 RTK，不能猜测归属。
			return rtkArtifactOwnership{}, false, nil
		}
		recognized = body
	}
	if recognized == "" {
		return rtkArtifactOwnership{}, false, nil
	}
	return rtkPromptOwnership(target.agentName, target.filePath, []byte(recognized)), true, nil
}
