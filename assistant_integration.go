package main

import (
	"bytes"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"unicode/utf8"
)

const (
	rtkCommandBlockStart = "---RTK命令_开始---"
	rtkCommandBlockEnd   = "---RTK命令_结束---"
)

type assistantKind string

const (
	assistantCodex      assistantKind = "Codex"
	assistantClaudeCode assistantKind = "Claude Code"
)

type assistantTarget struct {
	kind                 assistantKind
	agentName            string
	snapshotKey          string
	filePath             string
	createIfParentExists bool
}

type assistantFileState struct {
	exists          bool
	blockCount      int
	malformed       bool
	matchingCurrent bool
	legacyManaged   bool
	unmanaged       bool
}

type rtkPromptInstallResult struct {
	Artifacts map[string]rtkArtifactOwnership
	Unmanaged []string
}

func rtkAssistantCommandPayload() []byte {
	return rtkCodexAgentInstructions
}

func rtkClaudeAssistantCommandPayload() []byte {
	return rtkClaudeAgentInstructions
}

func discoverAssistantTargets() ([]assistantTarget, error) {
	codexDir, err := assistantConfigDirectory("CODEX_HOME", ".codex")
	if err != nil {
		return nil, err
	}
	claudeDir, err := assistantConfigDirectory("CLAUDE_CONFIG_DIR", ".claude")
	if err != nil {
		return nil, err
	}
	return []assistantTarget{
		{
			kind:        assistantCodex,
			agentName:   "codex",
			snapshotKey: "codex",
			filePath:    filepath.Join(codexDir, "AGENTS.md"),
		},
		{
			kind:                 assistantClaudeCode,
			agentName:            "claude-code",
			snapshotKey:          "claude-code-prompt",
			filePath:             filepath.Join(claudeDir, "CLAUDE.md"),
			createIfParentExists: true,
		},
	}, nil
}

func assistantTargetPayload(target assistantTarget) []byte {
	switch target.kind {
	case assistantCodex:
		return rtkAssistantCommandPayload()
	case assistantClaudeCode:
		return rtkClaudeAssistantCommandPayload()
	default:
		return nil
	}
}

func assistantTargetMayBeCreated(target assistantTarget) (bool, error) {
	if !target.createIfParentExists {
		return false, nil
	}
	info, err := os.Stat(filepath.Dir(target.filePath))
	if errors.Is(err, os.ErrNotExist) {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	if !info.IsDir() {
		return false, fmt.Errorf("%s 配置路径不是目录: %s", target.kind, filepath.Dir(target.filePath))
	}
	return true, nil
}

func assistantConfigDirectory(envName, defaultDirectory string) (string, error) {
	if configured := strings.TrimSpace(os.Getenv(envName)); configured != "" {
		return filepath.Clean(configured), nil
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return "", fmt.Errorf("无法定位当前用户目录: %w", err)
	}
	return filepath.Join(home, defaultDirectory), nil
}

func installRTKAssistantIntegrations() error {
	_, err := installRTKAssistantIntegrationsWithOwnership()
	return err
}

func installRTKAssistantIntegrationsWithOwnership() (rtkPromptInstallResult, error) {
	if err := validateRTKCodexDocuments(); err != nil {
		return rtkPromptInstallResult{}, err
	}
	targets, err := discoverAssistantTargets()
	if err != nil {
		return rtkPromptInstallResult{}, err
	}
	// 先读取并校验目标，再开始写入，避免 Codex 文件异常时留下半套配置。
	type assistantUpdate struct {
		target  assistantTarget
		before  []byte
		updated []byte
	}
	updates := make([]assistantUpdate, 0, len(targets))
	result := rtkPromptInstallResult{Artifacts: map[string]rtkArtifactOwnership{}}
	for _, target := range targets {
		data, err := os.ReadFile(target.filePath)
		if errors.Is(err, os.ErrNotExist) {
			mayCreate, statErr := assistantTargetMayBeCreated(target)
			if statErr != nil {
				return rtkPromptInstallResult{}, statErr
			}
			if !mayCreate {
				continue
			}
			data = nil
			err = nil
		}
		if err != nil {
			return rtkPromptInstallResult{}, fmt.Errorf("读取 %s 文件失败: %w", target.kind, err)
		}
		updated, managed, unmanaged, err := upsertRTKAssistantPrompt(target, data)
		if err != nil {
			return rtkPromptInstallResult{}, fmt.Errorf("检查 %s 文件失败: %w", target.kind, err)
		}
		if unmanaged {
			result.Unmanaged = append(result.Unmanaged, string(target.kind))
			continue
		}
		// 只有本次确实写入或迁移的标记段才归属给 RTK。已经存在的同内容段
		// 可能是用户手工维护的内容，不能因为格式相同就被停止流程删除。
		if managed && !bytes.Equal(data, updated) {
			artifact := rtkPromptArtifactForAgent(target.agentName)
			if artifact != "" {
				result.Artifacts[artifact] = rtkPromptOwnership(target.agentName, target.filePath, assistantTargetPayload(target))
			}
		}
		if !bytes.Equal(data, updated) {
			updates = append(updates, assistantUpdate{target: target, before: data, updated: updated})
		}
	}
	for index, update := range updates {
		if err := replaceUTF8File(update.target.filePath, update.updated); err != nil {
			// 尽力恢复本次已经改过的文件；原始错误优先返回给调用方。
			for _, applied := range updates[:index] {
				if restoreErr := replaceUTF8File(applied.target.filePath, applied.before); restoreErr != nil {
					return rtkPromptInstallResult{}, fmt.Errorf("写入 %s 指令段失败: %w；回滚失败: %v", update.target.kind, err, restoreErr)
				}
			}
			return rtkPromptInstallResult{}, fmt.Errorf("写入 %s 指令段失败: %w", update.target.kind, err)
		}
	}
	return result, nil
}

// upsertRTKAssistantPrompt migrates a known RTK prompt body to the current
// embedded payload. A marker block with an unknown body is left untouched and
// reported to the lifecycle caller instead of being silently claimed.
func upsertRTKAssistantPrompt(target assistantTarget, data []byte) (updated []byte, managed, unmanaged bool, err error) {
	bodies, err := rtkCommandBlockBodies(data)
	if err != nil {
		return nil, false, false, err
	}
	if len(bodies) == 0 {
		return appendRTKCommandBlock(data, assistantTargetPayload(target)), true, false, nil
	}
	known := false
	for _, body := range bodies {
		if rtkAssistantPromptBlockMatches(target, body) {
			known = true
			continue
		}
		return data, false, true, nil
	}
	if !known {
		return data, false, true, nil
	}
	base, _, err := stripRTKCommandBlocksWhere(data, func(body string) bool {
		return rtkAssistantPromptBlockMatches(target, body)
	})
	if err != nil {
		return nil, false, false, err
	}
	return appendRTKCommandBlock(base, assistantTargetPayload(target)), true, false, nil
}

func rtkAssistantPromptBlockMatches(target assistantTarget, body string) bool {
	if normalizeRTKBlockBody(body) == normalizeRTKBlockBody(string(assistantTargetPayload(target))) {
		return true
	}
	return rtkAssistantPromptBlockLooksLegacy(target, body)
}

func rtkAssistantPromptBlockLooksLegacy(target assistantTarget, body string) bool {
	normalized := normalizeRTKBlockBody(body)
	switch target.kind {
	case assistantCodex:
		return strings.HasPrefix(normalized, "# RTK：Codex 常驻高密度命令规则") ||
			strings.HasPrefix(normalized, "# RTK: Codex 常驻高密度命令规则") ||
			strings.HasPrefix(normalized, "# RTK：Codex 执行规则、源码审计与完整命令参考") ||
			strings.HasPrefix(normalized, "# RTK: Codex 执行规则、源码审计与完整命令参考") ||
			strings.HasPrefix(normalized, "# RTK - Rust Token Killer (Codex CLI)")
	case assistantClaudeCode:
		return strings.Contains(normalized, "Claude Code 的 `PreToolUse` Hook") && strings.Contains(normalized, "`rtk`")
	default:
		return false
	}
}

func rtkCommandBlockBodies(data []byte) ([]string, error) {
	if !utf8.Valid(data) {
		return nil, errors.New("文件不是有效 UTF-8")
	}
	content := strings.TrimPrefix(string(data), "\ufeff")
	bodies := make([]string, 0, 1)
	inBlock := false
	var body strings.Builder
	for _, line := range strings.SplitAfter(content, "\n") {
		marker := strings.TrimSpace(strings.TrimPrefix(strings.TrimRight(line, "\r\n"), "\ufeff"))
		switch marker {
		case rtkCommandBlockStart:
			if inBlock {
				return nil, errors.New("RTK 命令标记段嵌套")
			}
			inBlock = true
			body.Reset()
		case rtkCommandBlockEnd:
			if !inBlock {
				return nil, errors.New("RTK 命令结束标记缺少开始标记")
			}
			bodies = append(bodies, body.String())
			inBlock = false
		default:
			if inBlock {
				body.WriteString(line)
			}
		}
	}
	if inBlock {
		return nil, errors.New("RTK 命令开始标记缺少结束标记")
	}
	return bodies, nil
}

func stripRTKCommandBlocksWhere(data []byte, shouldRemove func(string) bool) ([]byte, int, error) {
	if !utf8.Valid(data) {
		return nil, 0, errors.New("RTK 文件不是有效 UTF-8")
	}
	content := string(data)
	hasBOM := strings.HasPrefix(content, "\ufeff")
	if hasBOM {
		content = strings.TrimPrefix(content, "\ufeff")
	}
	var builder strings.Builder
	inBlock := false
	removed := 0
	startLine := ""
	var block strings.Builder
	for _, line := range strings.SplitAfter(content, "\n") {
		marker := strings.TrimSpace(strings.TrimPrefix(strings.TrimRight(line, "\r\n"), "\ufeff"))
		switch marker {
		case rtkCommandBlockStart:
			if inBlock {
				return nil, 0, errors.New("RTK 命令标记段嵌套")
			}
			inBlock = true
			startLine = line
			block.Reset()
		case rtkCommandBlockEnd:
			if !inBlock {
				return nil, 0, errors.New("RTK 命令结束标记缺少开始标记")
			}
			if shouldRemove(block.String()) {
				removed++
			} else {
				// 原样回写未受管标记段，避免仅因清理另一个 RTK 段就改变
				// 用户文件的缩进、BOM 或换行风格。
				builder.WriteString(startLine)
				builder.WriteString(block.String())
				builder.WriteString(line)
			}
			inBlock = false
		default:
			if inBlock {
				block.WriteString(line)
			} else {
				builder.WriteString(line)
			}
		}
	}
	if inBlock {
		return nil, 0, errors.New("RTK 命令开始标记缺少结束标记")
	}
	result := builder.String()
	if hasBOM {
		result = "\ufeff" + result
	}
	return []byte(result), removed, nil
}

func inspectAssistantFile(target assistantTarget) (assistantFileState, error) {
	state := assistantFileState{}
	data, err := os.ReadFile(target.filePath)
	if errors.Is(err, os.ErrNotExist) {
		return state, nil
	}
	if err != nil {
		return state, err
	}
	state.exists = true
	bodies, parseErr := rtkCommandBlockBodies(data)
	state.blockCount = len(bodies)
	err = parseErr
	if err != nil {
		state.malformed = true
		return state, err
	}
	for _, body := range bodies {
		if normalizeRTKBlockBody(body) == normalizeRTKBlockBody(string(assistantTargetPayload(target))) {
			state.matchingCurrent = true
			continue
		}
		if rtkAssistantPromptBlockLooksLegacy(target, body) {
			state.legacyManaged = true
			continue
		}
		state.unmanaged = true
	}
	return state, nil
}

func inspectAssistantState(kind assistantKind) (assistantFileState, error) {
	targets, err := discoverAssistantTargets()
	if err != nil {
		return assistantFileState{}, err
	}
	for _, target := range targets {
		if target.kind != kind {
			continue
		}
		state, inspectErr := inspectAssistantFile(target)
		if inspectErr != nil && !state.malformed {
			return state, inspectErr
		}
		return state, inspectErr
	}
	return assistantFileState{}, nil
}

func inspectRTKAssistantStates() (codex assistantFileState, err error) {
	return inspectAssistantState(assistantCodex)
}

func queryRTKCodexAvailable() bool {
	codex, err := inspectRTKAssistantStates()
	return err == nil && codex.exists
}

func queryRTKCodexConfigured() bool {
	targets, err := discoverAssistantTargets()
	if err != nil {
		return false
	}
	for _, target := range targets {
		if target.kind != assistantCodex {
			continue
		}
		data, readErr := os.ReadFile(target.filePath)
		if readErr != nil {
			return false
		}
		return hasMatchingRTKCommandBlock(data, assistantTargetPayload(target))
	}
	return false
}

func queryRTKClaudePromptConfigured() bool {
	targets, err := discoverAssistantTargets()
	if err != nil {
		return false
	}
	for _, target := range targets {
		if target.kind != assistantClaudeCode {
			continue
		}
		data, readErr := os.ReadFile(target.filePath)
		if readErr != nil {
			return false
		}
		return hasMatchingRTKCommandBlock(data, assistantTargetPayload(target))
	}
	return false
}

func rtkCodexResidual() (string, error) {
	codex, err := inspectRTKAssistantStates()
	if err != nil {
		return "", err
	}
	if codex.malformed {
		return "Codex AGENTS.md 的 RTK 标记段不完整", nil
	}
	if codex.unmanaged {
		return "Codex AGENTS.md 存在未受管的 RTK 标记段，已保留且不会自动删除", nil
	}
	if codex.legacyManaged {
		return "Codex AGENTS.md 存在旧版 RTK 提示词，请点击启动 RTK 迁移", nil
	}
	if codex.blockCount > 1 {
		return "Codex AGENTS.md 存在多个 RTK 命令标记段", nil
	}
	return "", nil
}

func rtkClaudePromptResidual() (string, error) {
	claude, err := inspectAssistantState(assistantClaudeCode)
	if err != nil {
		return "", err
	}
	if claude.malformed {
		return "Claude Code CLAUDE.md 的 RTK 标记段不完整", nil
	}
	if claude.unmanaged {
		return "Claude Code CLAUDE.md 存在未受管的 RTK 标记段，已保留且不会自动删除", nil
	}
	if claude.legacyManaged {
		return "Claude Code CLAUDE.md 存在旧版 RTK 提示词，请点击启动 RTK 迁移", nil
	}
	if claude.blockCount > 1 {
		return "Claude Code CLAUDE.md 存在多个 RTK 命令标记段", nil
	}
	return "", nil
}

func removeRTKCodexIntegration() error {
	return removeRTKAssistantIntegrationsForAgents([]string{"codex"})
}

// removeRTKAssistantIntegrationsForAgents 只清理指定 Agent 的现有提示词内容。
// 不存在的文件不会被创建；未被本次启用记录拥有的 Agent 不会被触碰。
func removeRTKAssistantIntegrationsForAgents(owned []string) error {
	targets, err := discoverAssistantTargets()
	if err != nil {
		return err
	}
	type assistantUpdate struct {
		target  assistantTarget
		before  []byte
		updated []byte
	}
	updates := make([]assistantUpdate, 0, len(targets))
	for _, target := range targets {
		if !containsString(owned, target.agentName) {
			continue
		}
		data, err := os.ReadFile(target.filePath)
		if errors.Is(err, os.ErrNotExist) {
			continue
		}
		if err != nil {
			return fmt.Errorf("读取 %s 文件失败: %w", target.kind, err)
		}
		updated, _, err := stripMatchingRTKCommandBlocks(data, assistantTargetPayload(target))
		if err != nil {
			return fmt.Errorf("检查 %s 文件失败: %w", target.kind, err)
		}
		if !bytes.Equal(data, updated) {
			updates = append(updates, assistantUpdate{target: target, before: data, updated: updated})
		}
	}
	for index, update := range updates {
		if err := replaceUTF8File(update.target.filePath, update.updated); err != nil {
			for _, applied := range updates[:index] {
				if restoreErr := replaceUTF8File(applied.target.filePath, applied.before); restoreErr != nil {
					return fmt.Errorf("清理 %s 指令段失败: %w；回滚失败: %v", update.target.kind, err, restoreErr)
				}
			}
			return fmt.Errorf("清理 %s 指令段失败: %w", update.target.kind, err)
		}
	}
	if containsString(owned, "codex") {
		if residual, err := rtkCodexResidual(); err != nil {
			return err
		} else if residual != "" {
			return errors.New(residual)
		}
	}
	if containsString(owned, "claude-code") {
		if residual, err := rtkClaudePromptResidual(); err != nil {
			return err
		} else if residual != "" {
			return errors.New(residual)
		}
	}
	return nil
}

// removeRTKPromptArtifact removes only the exact prompt body recorded at
// activation time. A manually edited or unrelated marker block is preserved.
func removeRTKPromptArtifact(artifact rtkArtifactOwnership) (bool, error) {
	if strings.TrimSpace(artifact.TargetPath) == "" || strings.TrimSpace(artifact.Fingerprint) == "" {
		return false, errors.New("RTK 提示词归属记录不完整")
	}
	data, err := os.ReadFile(artifact.TargetPath)
	if errors.Is(err, os.ErrNotExist) {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	updated, removed, err := stripRTKCommandBlocksWhere(data, func(body string) bool {
		return rtkPromptFingerprint([]byte(body)) == artifact.Fingerprint
	})
	if err != nil {
		return false, err
	}
	if removed == 0 {
		return false, nil
	}
	if err := replaceUTF8File(artifact.TargetPath, updated); err != nil {
		return false, err
	}
	return true, nil
}

func rtkPromptArtifactPresent(artifact rtkArtifactOwnership) (bool, error) {
	if strings.TrimSpace(artifact.TargetPath) == "" || strings.TrimSpace(artifact.Fingerprint) == "" {
		return false, errors.New("RTK 提示词归属记录不完整")
	}
	data, err := os.ReadFile(artifact.TargetPath)
	if errors.Is(err, os.ErrNotExist) {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	bodies, err := rtkCommandBlockBodies(data)
	if err != nil {
		return false, err
	}
	for _, body := range bodies {
		if rtkPromptFingerprint([]byte(body)) == artifact.Fingerprint {
			return true, nil
		}
	}
	return false, nil
}

func validateRTKAssistantIntegrations() error {
	targets, err := discoverAssistantTargets()
	if err != nil {
		return err
	}
	for _, target := range targets {
		data, readErr := os.ReadFile(target.filePath)
		if errors.Is(readErr, os.ErrNotExist) {
			continue
		}
		if readErr != nil {
			return fmt.Errorf("读取 %s 文件失败: %w", target.kind, readErr)
		}
		if _, _, parseErr := stripRTKCommandBlocks(data); parseErr != nil {
			return fmt.Errorf("检查 %s 文件失败: %w", target.kind, parseErr)
		}
	}
	return nil
}

func upsertExistingRTKCommandBlock(filePath string, payload []byte) error {
	data, err := os.ReadFile(filePath)
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	if err != nil {
		return err
	}
	base, _, err := stripRTKCommandBlocks(data)
	if err != nil {
		return err
	}
	updated := appendRTKCommandBlock(base, payload)
	if bytes.Equal(data, updated) {
		return nil
	}
	return replaceUTF8File(filePath, updated)
}

func removeRTKCommandBlocks(filePath string) error {
	data, err := os.ReadFile(filePath)
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	if err != nil {
		return err
	}
	base, count, err := stripRTKCommandBlocks(data)
	if err != nil {
		return err
	}
	if count == 0 || bytes.Equal(data, base) {
		return nil
	}
	return replaceUTF8File(filePath, base)
}

// stripMatchingRTKCommandBlocks removes only blocks whose body matches a known
// embedded RTK payload. Other marked blocks are preserved verbatim.
func stripMatchingRTKCommandBlocks(data []byte, payloads ...[]byte) ([]byte, int, error) {
	if !utf8.Valid(data) {
		return nil, 0, errors.New("RTK 文件不是有效 UTF-8")
	}
	if len(payloads) == 0 {
		return nil, 0, errors.New("缺少 RTK 命令标记段内容")
	}
	content := string(data)
	hasBOM := strings.HasPrefix(content, "\ufeff")
	if hasBOM {
		content = strings.TrimPrefix(content, "\ufeff")
	}
	normalize := func(value string) string {
		value = strings.ReplaceAll(value, "\r\n", "\n")
		value = strings.ReplaceAll(value, "\r", "\n")
		return strings.TrimSuffix(value, "\n")
	}
	wanted := make(map[string]struct{}, len(payloads))
	for _, payload := range payloads {
		if !utf8.Valid(payload) {
			return nil, 0, errors.New("RTK 文件不是有效 UTF-8")
		}
		wanted[normalize(string(payload))] = struct{}{}
	}
	var builder strings.Builder
	inBlock := false
	removed := 0
	var block strings.Builder
	for _, line := range strings.SplitAfter(content, "\n") {
		marker := strings.TrimSpace(strings.TrimPrefix(strings.TrimRight(line, "\r\n"), "\ufeff"))
		switch marker {
		case rtkCommandBlockStart:
			if inBlock {
				return nil, 0, errors.New("RTK 命令标记段嵌套")
			}
			inBlock = true
			block.Reset()
		case rtkCommandBlockEnd:
			if !inBlock {
				return nil, 0, errors.New("RTK 命令结束标记缺少开始标记")
			}
			if _, matches := wanted[normalize(block.String())]; matches {
				removed++
			} else {
				builder.WriteString(rtkCommandBlockStart)
				builder.WriteString("\n")
				builder.WriteString(block.String())
				builder.WriteString(rtkCommandBlockEnd)
				if strings.HasSuffix(line, "\r\n") {
					builder.WriteString("\r\n")
				} else {
					builder.WriteString("\n")
				}
			}
			inBlock = false
		default:
			if inBlock {
				block.WriteString(line)
			} else {
				builder.WriteString(line)
			}
		}
	}
	if inBlock {
		return nil, 0, errors.New("RTK 命令开始标记缺少结束标记")
	}
	result := builder.String()
	if hasBOM {
		result = "\ufeff" + result
	}
	return []byte(result), removed, nil
}

func hasMatchingRTKCommandBlock(data []byte, payloads ...[]byte) bool {
	_, count, err := stripMatchingRTKCommandBlocks(data, payloads...)
	return err == nil && count > 0
}

func stripRTKCommandBlocks(data []byte) ([]byte, int, error) {
	if !utf8.Valid(data) {
		return nil, 0, errors.New("文件不是有效 UTF-8")
	}
	content := string(data)
	hasBOM := strings.HasPrefix(content, "\ufeff")
	if hasBOM {
		content = strings.TrimPrefix(content, "\ufeff")
	}
	var builder strings.Builder
	inBlock := false
	count := 0
	for _, line := range strings.SplitAfter(content, "\n") {
		marker := strings.TrimSpace(strings.TrimPrefix(strings.TrimRight(line, "\r\n"), "\ufeff"))
		switch marker {
		case rtkCommandBlockStart:
			if inBlock {
				return nil, 0, errors.New("RTK 命令标记段嵌套")
			}
			inBlock = true
			count++
		case rtkCommandBlockEnd:
			if !inBlock {
				return nil, 0, errors.New("RTK 命令结束标记缺少开始标记")
			}
			inBlock = false
		default:
			if !inBlock {
				builder.WriteString(line)
			}
		}
	}
	if inBlock {
		return nil, 0, errors.New("RTK 命令开始标记缺少结束标记")
	}
	result := builder.String()
	if hasBOM {
		result = "\ufeff" + result
	}
	return []byte(result), count, nil
}

func appendRTKCommandBlock(base, payload []byte) []byte {
	baseText := string(base)
	newline := "\n"
	if strings.Contains(baseText, "\r\n") {
		newline = "\r\n"
	}
	if baseText != "" {
		if !strings.HasSuffix(baseText, "\n") {
			baseText += newline
		}
		if !strings.HasSuffix(baseText, newline+newline) {
			baseText += newline
		}
	}
	payloadText := normalizeRTKLineEndings(string(payload), newline)
	if !strings.HasSuffix(payloadText, newline) {
		payloadText += newline
	}
	return []byte(baseText + rtkCommandBlockStart + newline + payloadText + rtkCommandBlockEnd + newline)
}

func normalizeRTKLineEndings(value, newline string) string {
	value = strings.ReplaceAll(value, "\r\n", "\n")
	value = strings.ReplaceAll(value, "\r", "\n")
	if newline == "\r\n" {
		value = strings.ReplaceAll(value, "\n", "\r\n")
	}
	return value
}

func replaceUTF8File(filePath string, data []byte) error {
	if !utf8.Valid(data) {
		return errors.New("拒绝写入非 UTF-8 文件")
	}
	info, statErr := os.Stat(filePath)
	if statErr != nil && !errors.Is(statErr, os.ErrNotExist) {
		return statErr
	}
	mode := os.FileMode(0o644)
	if statErr == nil {
		if info.IsDir() {
			return fmt.Errorf("目标路径是目录: %s", filePath)
		}
		mode = info.Mode().Perm()
	}
	if err := os.MkdirAll(filepath.Dir(filePath), 0o755); err != nil {
		return err
	}
	temporary, err := os.CreateTemp(filepath.Dir(filePath), ".codex-rtk-*")
	if err != nil {
		return err
	}
	temporaryPath := temporary.Name()
	keep := false
	defer func() {
		_ = temporary.Close()
		if !keep {
			_ = os.Remove(temporaryPath)
		}
	}()
	if err := temporary.Chmod(mode); err != nil {
		return err
	}
	if _, err := temporary.Write(data); err != nil {
		return err
	}
	if err := temporary.Sync(); err != nil {
		return err
	}
	if err := temporary.Close(); err != nil {
		return err
	}
	hadExisting := statErr == nil
	backupPath := ""
	if hadExisting {
		backup, backupErr := os.CreateTemp(filepath.Dir(filePath), ".codex-rtk-backup-*")
		if backupErr != nil {
			return backupErr
		}
		backupPath = backup.Name()
		if backupErr := backup.Close(); backupErr != nil {
			_ = os.Remove(backupPath)
			return backupErr
		}
		if backupErr := os.Remove(backupPath); backupErr != nil {
			return backupErr
		}
		if err := os.Rename(filePath, backupPath); err != nil {
			return err
		}
	}
	if err := os.Rename(temporaryPath, filePath); err != nil {
		if hadExisting {
			_ = os.Rename(backupPath, filePath)
		}
		return err
	}
	if hadExisting {
		if err := os.Remove(backupPath); err != nil {
			return err
		}
	}
	keep = true
	return nil
}
