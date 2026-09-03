package main

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"path/filepath"
	"sort"
	"strings"
)

const (
	snipOwnershipMetadataKey = "snip_ownership_v1"
	snipOwnershipVersion     = 1

	snipArtifactCodexHook   = "codex-hook"
	snipArtifactClaudeHook  = "claude-code-hook"
	snipArtifactCursorHook  = "cursor-hook"
	snipArtifactCopilotHook = "copilot-hook"
)

// snipHookOwnership records the one native Hook handler that code-Manager
// actually added. Group and handler indexes make duplicate handlers
// distinguishable; group context and handler fingerprints prevent a later
// manual edit from being removed as though it were still the original Hook.
type snipHookOwnership struct {
	Agent            string `json:"agent"`
	TargetPath       string `json:"target_path"`
	Event            string `json:"event"`
	GroupIndex       int    `json:"group_index"`
	HandlerIndex     int    `json:"handler_index"`
	GroupFingerprint string `json:"group_fingerprint,omitempty"`
	Fingerprint      string `json:"fingerprint"`
	Command          string `json:"command"`
}

type snipOwnershipLedger struct {
	Version   int                          `json:"version"`
	Artifacts map[string]snipHookOwnership `json:"artifacts"`
}

func emptySnipOwnershipLedger() snipOwnershipLedger {
	return snipOwnershipLedger{Version: snipOwnershipVersion, Artifacts: map[string]snipHookOwnership{}}
}

func readSnipOwnershipLedger(state managedToolState) (snipOwnershipLedger, error) {
	if state.Metadata == nil || strings.TrimSpace(state.Metadata[snipOwnershipMetadataKey]) == "" {
		return emptySnipOwnershipLedger(), nil
	}
	var ledger snipOwnershipLedger
	if err := json.Unmarshal([]byte(state.Metadata[snipOwnershipMetadataKey]), &ledger); err != nil {
		return snipOwnershipLedger{}, fmt.Errorf("解析 snip 精确归属账本失败: %w", err)
	}
	if ledger.Version != snipOwnershipVersion {
		return snipOwnershipLedger{}, fmt.Errorf("不支持的 snip 精确归属账本版本: %d", ledger.Version)
	}
	if ledger.Artifacts == nil {
		ledger.Artifacts = map[string]snipHookOwnership{}
	}
	for name, artifact := range ledger.Artifacts {
		if err := validateSnipHookOwnership(name, artifact); err != nil {
			return snipOwnershipLedger{}, err
		}
	}
	if err := validateSnipOwnershipTargets(ledger); err != nil {
		return snipOwnershipLedger{}, err
	}
	return ledger, nil
}

func writeSnipOwnershipLedger(state *managedToolState, ledger snipOwnershipLedger) error {
	if state == nil {
		return errors.New("snip 状态为空")
	}
	if ledger.Version == 0 {
		ledger.Version = snipOwnershipVersion
	}
	if ledger.Version != snipOwnershipVersion {
		return fmt.Errorf("不支持的 snip 精确归属账本版本: %d", ledger.Version)
	}
	if ledger.Artifacts == nil {
		ledger.Artifacts = map[string]snipHookOwnership{}
	}
	for name, artifact := range ledger.Artifacts {
		if err := validateSnipHookOwnership(name, artifact); err != nil {
			return err
		}
	}
	if err := validateSnipOwnershipTargets(ledger); err != nil {
		return err
	}
	encoded, err := json.Marshal(ledger)
	if err != nil {
		return err
	}
	if state.Metadata == nil {
		state.Metadata = map[string]string{}
	}
	state.Metadata[snipOwnershipMetadataKey] = string(encoded)
	state.OwnedAgents = snipOwnedAgentsFromLedger(ledger)
	return nil
}

func clearSnipOwnershipLedger(state *managedToolState) {
	if state == nil {
		return
	}
	state.OwnedAgents = nil
	if state.Metadata == nil {
		return
	}
	delete(state.Metadata, snipOwnershipMetadataKey)
	if len(state.Metadata) == 0 {
		state.Metadata = nil
	}
}

func validateSnipHookOwnership(name string, artifact snipHookOwnership) error {
	wantAgent := snipAgentForArtifact(name)
	if wantAgent == "" {
		return fmt.Errorf("snip 精确归属账本包含未知项目: %s", name)
	}
	if artifact.Agent != wantAgent {
		return fmt.Errorf("snip 精确归属项目的 Agent 不匹配: %s", name)
	}
	if strings.TrimSpace(artifact.TargetPath) == "" || strings.TrimSpace(artifact.Fingerprint) == "" || strings.TrimSpace(artifact.Command) == "" {
		return fmt.Errorf("snip 精确归属项目不完整: %s", name)
	}
	wantEvent, grouped := snipHookEventForAgent(artifact.Agent)
	if artifact.Event != wantEvent || artifact.HandlerIndex < 0 || (grouped && artifact.GroupIndex < 0) || (!grouped && artifact.GroupIndex != -1) {
		return fmt.Errorf("snip 精确归属项目位置无效: %s", name)
	}
	if grouped && strings.TrimSpace(artifact.GroupFingerprint) == "" {
		return fmt.Errorf("snip 精确归属项目缺少 Hook 组指纹: %s", name)
	}
	if !grouped && artifact.GroupFingerprint != "" {
		return fmt.Errorf("snip 精确归属项目包含无效 Hook 组指纹: %s", name)
	}
	if !snipHookCommandTargetsAgent(artifact.Command, artifact.Agent) {
		return fmt.Errorf("snip 精确归属项目命令无效: %s", name)
	}
	if name != snipArtifactForAgent(artifact.Agent) && name != snipArtifactKeyForOwnership(artifact.Agent, artifact.TargetPath) {
		return fmt.Errorf("snip 精确归属项目键与目标文件不匹配: %s", name)
	}
	return nil
}

func validateSnipOwnershipTargets(ledger snipOwnershipLedger) error {
	seen := map[string]string{}
	for name, artifact := range ledger.Artifacts {
		key := artifact.Agent + "\x00" + strings.ToLower(canonicalSnipTargetPath(artifact.TargetPath))
		if previous, exists := seen[key]; exists {
			return fmt.Errorf("snip 精确归属账本为同一目标文件记录了多个 Hook: %s、%s", previous, name)
		}
		seen[key] = name
	}
	return nil
}

func snipArtifactForAgent(agent string) string {
	switch agent {
	case "codex":
		return snipArtifactCodexHook
	case "claude-code":
		return snipArtifactClaudeHook
	case "cursor":
		return snipArtifactCursorHook
	case "copilot":
		return snipArtifactCopilotHook
	default:
		return ""
	}
}

func snipAgentForArtifact(name string) string {
	for _, agent := range []string{"codex", "claude-code", "cursor", "copilot"} {
		base := snipArtifactForAgent(agent)
		if name == base || strings.HasPrefix(name, base+":") {
			return agent
		}
	}
	return ""
}

func snipArtifactKeyForOwnership(agent, targetPath string) string {
	base := snipArtifactForAgent(agent)
	if base == "" {
		return ""
	}
	sum := sha256.Sum256([]byte(strings.ToLower(canonicalSnipTargetPath(targetPath))))
	return base + ":" + hex.EncodeToString(sum[:])
}

func snipOwnershipArtifactForTarget(ledger snipOwnershipLedger, agent, targetPath string) (string, snipHookOwnership, bool) {
	canonicalTarget := canonicalSnipTargetPath(targetPath)
	for name, artifact := range ledger.Artifacts {
		if artifact.Agent == agent && strings.EqualFold(canonicalSnipTargetPath(artifact.TargetPath), canonicalTarget) {
			return name, artifact, true
		}
	}
	return "", snipHookOwnership{}, false
}

func snipOwnershipHasAgent(ledger snipOwnershipLedger, agent string) bool {
	for _, artifact := range ledger.Artifacts {
		if artifact.Agent == agent {
			return true
		}
	}
	return false
}

func snipNamedOwnershipArtifacts(ledger snipOwnershipLedger) []snipNamedHookOwnership {
	result := make([]snipNamedHookOwnership, 0, len(ledger.Artifacts))
	for name, artifact := range ledger.Artifacts {
		result = append(result, snipNamedHookOwnership{Name: name, Artifact: artifact})
	}
	sort.Slice(result, func(i, j int) bool {
		if result[i].Artifact.Agent != result[j].Artifact.Agent {
			return result[i].Artifact.Agent < result[j].Artifact.Agent
		}
		left := strings.ToLower(canonicalSnipTargetPath(result[i].Artifact.TargetPath))
		right := strings.ToLower(canonicalSnipTargetPath(result[j].Artifact.TargetPath))
		if left != right {
			return left < right
		}
		return result[i].Name < result[j].Name
	})
	return result
}

type snipNamedHookOwnership struct {
	Name     string
	Artifact snipHookOwnership
}

func canonicalSnipTargetPath(path string) string {
	if absolute, err := filepath.Abs(path); err == nil {
		path = absolute
	}
	return filepath.Clean(path)
}

func snipHookOwnershipFromLocation(location snipHookLocation) snipHookOwnership {
	return snipHookOwnership{
		Agent:            location.Agent,
		TargetPath:       canonicalSnipTargetPath(location.TargetPath),
		Event:            location.Event,
		GroupIndex:       location.GroupIndex,
		HandlerIndex:     location.HandlerIndex,
		GroupFingerprint: location.GroupFingerprint,
		Fingerprint:      location.Fingerprint,
		Command:          strings.TrimSpace(location.Command),
	}
}

func snipOwnedAgentsFromLedger(ledger snipOwnershipLedger) []string {
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

func snipOwnedHookPresent(artifact snipHookOwnership) (bool, error) {
	presence, err := snipOwnedHookPresenceAt(artifact)
	return presence == snipOwnedHookExact, err
}

func snipOwnedAgentsNeedingRepair(ledger snipOwnershipLedger, executable string) ([]string, error) {
	seen := map[string]struct{}{}
	for _, item := range snipNamedOwnershipArtifacts(ledger) {
		if !snipHookCommandMatchesExecutable(item.Artifact.Command, item.Artifact.Agent, executable) {
			seen[item.Artifact.Agent] = struct{}{}
			continue
		}
		presence, err := snipOwnedHookPresenceAt(item.Artifact)
		if err != nil {
			return nil, fmt.Errorf("检查受管 Snip %s Hook 失败: %w", item.Artifact.Agent, err)
		}
		if presence != snipOwnedHookExact {
			seen[item.Artifact.Agent] = struct{}{}
		}
	}
	result := make([]string, 0, len(seen))
	for agent := range seen {
		result = append(result, agent)
	}
	sort.Strings(result)
	return result, nil
}

// snipActivationArtifactsPresent only treats artifacts proven by the exact
// ledger as residual. A manually configured Hook, even one that points to the
// same snip.exe, is not a code-Manager residual without this evidence.
func snipActivationArtifactsPresent(state managedToolState, ledger snipOwnershipLedger) (bool, error) {
	if state.UserPath || state.SystemPath {
		return true, nil
	}
	for _, artifact := range ledger.Artifacts {
		presence, err := snipOwnedHookPresenceAt(artifact)
		if err != nil {
			return false, err
		}
		if presence != snipOwnedHookAbsent {
			return true, nil
		}
	}
	return false, nil
}

func snipOwnershipNeedsCleanup(state managedToolState, ledger snipOwnershipLedger) bool {
	return state.UserPath || state.SystemPath || len(ledger.Artifacts) > 0
}

func removeSnipOwnedArtifacts(ledger snipOwnershipLedger) error {
	for _, item := range snipNamedOwnershipArtifacts(ledger) {
		if _, err := removeSnipOwnedHook(item.Artifact); err != nil {
			return fmt.Errorf("清理受管 Snip %s Hook 失败: %w", item.Artifact.Agent, err)
		}
	}
	return nil
}

// recoverLegacySnipOwnership converts the old platform-level owned_agents
// record only when exactly one current-release Hook can be located. Multiple
// matching handlers are intentionally left unclaimed because old state cannot
// tell which one code-Manager wrote.
func recoverLegacySnipOwnership(state managedToolState, executable string) (snipOwnershipLedger, error) {
	ledger, err := readSnipOwnershipLedger(state)
	if err != nil {
		return snipOwnershipLedger{}, err
	}
	if len(state.OwnedAgents) == 0 || strings.TrimSpace(executable) == "" {
		return ledger, nil
	}
	directories := snipAgentDirectories()
	for _, agent := range snipAgentsToClean(state) {
		if snipOwnershipHasAgent(ledger, agent) {
			continue
		}
		directory := directories[agent]
		if directory == "" {
			continue
		}
		locations, _, err := snipHookLocationsMatchingExecutableAt(agent, snipAgentHookFile(agent, directory), executable)
		if err != nil {
			return snipOwnershipLedger{}, fmt.Errorf("检查旧版 Snip %s Hook 失败: %w", agent, err)
		}
		if len(locations) == 1 {
			artifact := snipHookOwnershipFromLocation(locations[0])
			ledger.Artifacts[snipArtifactKeyForOwnership(agent, artifact.TargetPath)] = artifact
		}
	}
	return ledger, nil
}

// snipHookOwnershipAddedByInit identifies the one current-release Hook that
// appeared after a successful official init. It compares handler fingerprints
// rather than whole files so unrelated hooks can coexist and be preserved.
func snipHookOwnershipAddedByInit(agent string, before snipAgentSnapshot, executable string) (snipHookOwnership, error) {
	after, exists, err := snipHookLocationsMatchingExecutableAt(agent, before.Path, executable)
	if err != nil {
		return snipHookOwnership{}, err
	}
	if !exists {
		return snipHookOwnership{}, errors.New("初始化后目标 Hook 文件不存在")
	}
	previous := []snipHookLocation(nil)
	if before.Exists {
		previous, err = snipHookLocationsMatchingExecutableInData(agent, before.Path, before.Data, executable)
		if err != nil {
			return snipHookOwnership{}, fmt.Errorf("读取初始化前 Hook 快照失败: %w", err)
		}
	}
	known := make(map[string]int, len(previous))
	for _, location := range previous {
		known[location.Fingerprint]++
	}
	added := make([]snipHookLocation, 0, 1)
	for _, location := range after {
		if known[location.Fingerprint] > 0 {
			known[location.Fingerprint]--
			continue
		}
		added = append(added, location)
	}
	if len(added) != 1 {
		return snipHookOwnership{}, fmt.Errorf("官方初始化后无法唯一确认新增的 Snip %s Hook（检测到 %d 条）", agent, len(added))
	}
	return snipHookOwnershipFromLocation(added[0]), nil
}
