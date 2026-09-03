package main

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

const (
	rtkClaudePreToolUseEvent  = "PreToolUse"
	rtkCopilotPreToolUseEvent = "PreToolUse"
)

func rtkClaudeDirectory() string {
	directory, err := assistantConfigDirectory("CLAUDE_CONFIG_DIR", ".claude")
	if err != nil {
		return ""
	}
	return directory
}

func rtkClaudeSettingsFile() string {
	return filepath.Join(rtkClaudeDirectory(), "settings.json")
}

func rtkClaudeHookCommand(executable string) string {
	return quoteWindowsExecutable(executable) + " hook claude"
}

func queryRTKClaudeAvailable() bool {
	info, err := os.Stat(rtkClaudeDirectory())
	return err == nil && info.IsDir()
}

func rtkCopilotDirectory() string {
	if configured := strings.TrimSpace(os.Getenv("COPILOT_HOME")); configured != "" {
		return filepath.Clean(configured)
	}
	return userAgentDirectory(".copilot")
}

func rtkCopilotHookFile() string {
	return filepath.Join(rtkCopilotDirectory(), "hooks", "rtk-rewrite.json")
}

func rtkCopilotHookCommand(executable string) string {
	return quoteWindowsExecutable(executable) + " hook copilot"
}

func rtkCopilotAvailable() bool {
	info, err := os.Stat(rtkCopilotDirectory())
	return err == nil && info.IsDir()
}

func readRTKJSONObject(filePath string) (map[string]any, bool, error) {
	data, err := os.ReadFile(filePath)
	if errors.Is(err, os.ErrNotExist) {
		return map[string]any{}, false, nil
	}
	if err != nil {
		return nil, false, err
	}
	data = bytes.TrimPrefix(data, []byte{0xef, 0xbb, 0xbf})
	if len(bytes.TrimSpace(data)) == 0 {
		return map[string]any{}, true, nil
	}
	root := map[string]any{}
	if err := json.Unmarshal(data, &root); err != nil {
		return nil, true, err
	}
	return root, true, nil
}

func writeRTKJSONObject(filePath string, root map[string]any) error {
	encoded, err := json.MarshalIndent(root, "", "  ")
	if err != nil {
		return err
	}
	return replaceUTF8File(filePath, append(encoded, '\n'))
}

func isRTKHookCommand(command, agent string) bool {
	trimmed := strings.TrimSpace(command)
	suffix := " hook " + agent
	if len(trimmed) <= len(suffix) || !strings.EqualFold(trimmed[len(trimmed)-len(suffix):], suffix) {
		return false
	}
	executable := strings.Trim(strings.TrimSpace(trimmed[:len(trimmed)-len(suffix)]), `"'`)
	base := strings.ToLower(filepath.Base(filepath.Clean(executable)))
	return base == "rtk.exe" || base == "rtk"
}

func isRTKClaudeHookCommand(command string) bool {
	return isRTKHookCommand(command, "claude")
}

func isRTKCopilotHookCommand(command string) bool {
	return isRTKHookCommand(command, "copilot")
}

func isRTKCopilotHookEntry(item any) bool {
	obj, ok := item.(map[string]any)
	if !ok {
		return false
	}
	command, _ := obj["command"].(string)
	return isRTKCopilotHookCommand(command)
}

func isRTKClaudeHookEntry(item any) bool {
	obj, ok := item.(map[string]any)
	if !ok {
		return false
	}
	nested, ok := obj["hooks"].([]any)
	if !ok {
		return false
	}
	for _, nestedItem := range nested {
		hook, ok := nestedItem.(map[string]any)
		if !ok {
			continue
		}
		command, _ := hook["command"].(string)
		if isRTKClaudeHookCommand(command) {
			return true
		}
	}
	return false
}

func cleanRTKClaudeEntries(items []any, keepCommand string) ([]any, bool, bool, error) {
	cleaned := make([]any, 0, len(items))
	changed := false
	found := false
	for _, item := range items {
		obj, ok := item.(map[string]any)
		if !ok {
			cleaned = append(cleaned, item)
			continue
		}
		rawHooks, exists := obj["hooks"]
		if !exists {
			cleaned = append(cleaned, item)
			continue
		}
		nested, ok := rawHooks.([]any)
		if !ok {
			return nil, false, false, errors.New("Claude settings.hooks.PreToolUse 条目的 hooks 不是数组，拒绝覆盖用户配置")
		}
		keptHooks := make([]any, 0, len(nested))
		for _, nestedItem := range nested {
			hook, hookOK := nestedItem.(map[string]any)
			command, _ := hook["command"].(string)
			if hookOK && isRTKClaudeHookCommand(command) {
				if keepCommand != "" && strings.EqualFold(strings.TrimSpace(command), keepCommand) {
					found = true
					keptHooks = append(keptHooks, nestedItem)
				} else {
					changed = true
				}
				continue
			}
			keptHooks = append(keptHooks, nestedItem)
		}
		if len(keptHooks) == 0 {
			delete(obj, "hooks")
			changed = true
			continue
		}
		if len(keptHooks) != len(nested) {
			obj["hooks"] = keptHooks
		}
		cleaned = append(cleaned, item)
	}
	return cleaned, changed, found, nil
}

func installRTKClaudeIntegration(executable string) error {
	directory := rtkClaudeDirectory()
	if directory == "" {
		return nil
	}
	info, err := os.Stat(directory)
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	if err != nil {
		return err
	}
	if !info.IsDir() {
		return fmt.Errorf("Claude 配置路径不是目录: %s", directory)
	}

	filePath := rtkClaudeSettingsFile()
	root, _, err := readRTKJSONObject(filePath)
	if err != nil {
		return fmt.Errorf("解析 Claude settings.json 失败: %w", err)
	}
	hooks, hooksPresent := root["hooks"].(map[string]any)
	if _, exists := root["hooks"]; exists && !hooksPresent {
		return errors.New("Claude settings.hooks 不是对象，拒绝覆盖用户配置")
	}
	if hooks == nil {
		hooks = map[string]any{}
		root["hooks"] = hooks
	}
	items := []any{}
	if raw, exists := hooks[rtkClaudePreToolUseEvent]; exists {
		var ok bool
		items, ok = raw.([]any)
		if !ok {
			return errors.New("Claude settings.hooks.PreToolUse 不是数组，拒绝覆盖用户配置")
		}
	}
	command := rtkClaudeHookCommand(executable)
	found := rtkClaudeHookEntriesContainCommand(items, command)
	changed := false
	if !found {
		items = append(items, map[string]any{
			"matcher": "Bash",
			"hooks": []any{map[string]any{
				"type":    "command",
				"command": command,
			}},
		})
		changed = true
	}
	if !changed {
		return nil
	}
	hooks[rtkClaudePreToolUseEvent] = items
	return writeRTKJSONObject(filePath, root)
}

func rtkClaudeHookEntriesContainCommand(items []any, wantCommand string) bool {
	for _, item := range items {
		obj, ok := item.(map[string]any)
		if !ok {
			continue
		}
		nested, ok := obj["hooks"].([]any)
		if !ok {
			continue
		}
		for _, nestedItem := range nested {
			hook, ok := nestedItem.(map[string]any)
			if !ok {
				continue
			}
			command, _ := hook["command"].(string)
			if strings.EqualFold(strings.TrimSpace(command), strings.TrimSpace(wantCommand)) {
				return true
			}
		}
	}
	return false
}

func rtkClaudeHookConfiguredForCommand(filePath, command string) bool {
	root, exists, err := readRTKJSONObject(filePath)
	if err != nil || !exists {
		return false
	}
	hooks, ok := root["hooks"].(map[string]any)
	if !ok {
		return false
	}
	items, ok := hooks[rtkClaudePreToolUseEvent].([]any)
	return ok && rtkClaudeHookEntriesContainCommand(items, command)
}

func rtkClaudeHookConfiguredForExecutable(executable string) bool {
	return rtkClaudeHookConfiguredForCommand(rtkClaudeSettingsFile(), rtkClaudeHookCommand(executable))
}

func queryRTKClaudeHookConfigured() bool {
	root, exists, err := readRTKJSONObject(rtkClaudeSettingsFile())
	if err != nil || !exists {
		return false
	}
	hooks, ok := root["hooks"].(map[string]any)
	if !ok {
		return false
	}
	items, ok := hooks[rtkClaudePreToolUseEvent].([]any)
	if !ok {
		return false
	}
	for _, item := range items {
		if isRTKClaudeHookEntry(item) {
			return true
		}
	}
	return false
}

func queryRTKClaudeConfigured() bool {
	return queryRTKClaudeHookConfigured() && queryRTKClaudePromptConfigured()
}

func rtkClaudeConfiguredForExecutable(executable string) bool {
	return rtkClaudeHookConfiguredForExecutable(executable) && queryRTKClaudePromptConfigured()
}

func removeRTKClaudeIntegration() error {
	executable, err := rtkExecutablePath()
	if err != nil {
		return err
	}
	return removeRTKClaudeIntegrationCommand(rtkClaudeSettingsFile(), rtkClaudeHookCommand(executable))
}

func removeRTKClaudeIntegrationCommand(filePath, command string) error {
	root, exists, err := readRTKJSONObject(filePath)
	if errors.Is(err, os.ErrNotExist) || !exists {
		return nil
	}
	if err != nil {
		return fmt.Errorf("解析 Claude settings.json 失败: %w", err)
	}
	hooks, hooksPresent := root["hooks"].(map[string]any)
	if _, exists := root["hooks"]; exists && !hooksPresent {
		return errors.New("Claude settings.hooks 不是对象，拒绝覆盖用户配置")
	}
	if hooks == nil {
		return nil
	}
	items, exists := hooks[rtkClaudePreToolUseEvent]
	if !exists {
		return nil
	}
	preToolUse, ok := items.([]any)
	if !ok {
		return errors.New("Claude settings.hooks.PreToolUse 不是数组，拒绝覆盖用户配置")
	}
	cleaned, changed := removeRTKClaudeEntriesByCommand(preToolUse, command)
	if !changed {
		return nil
	}
	if len(cleaned) == 0 {
		delete(hooks, rtkClaudePreToolUseEvent)
	} else {
		hooks[rtkClaudePreToolUseEvent] = cleaned
	}
	if len(hooks) == 0 {
		delete(root, "hooks")
	}
	return writeRTKJSONObject(filePath, root)
}

func removeRTKClaudeEntriesByCommand(items []any, command string) ([]any, bool) {
	cleaned := make([]any, 0, len(items))
	changed := false
	for _, item := range items {
		obj, ok := item.(map[string]any)
		if !ok {
			cleaned = append(cleaned, item)
			continue
		}
		rawHooks, exists := obj["hooks"]
		if !exists {
			cleaned = append(cleaned, item)
			continue
		}
		nested, ok := rawHooks.([]any)
		if !ok {
			cleaned = append(cleaned, item)
			continue
		}
		keptHooks := make([]any, 0, len(nested))
		groupChanged := false
		for _, nestedItem := range nested {
			hook, hookOK := nestedItem.(map[string]any)
			itemCommand, _ := hook["command"].(string)
			if hookOK && strings.EqualFold(strings.TrimSpace(itemCommand), strings.TrimSpace(command)) {
				changed = true
				groupChanged = true
				continue
			}
			keptHooks = append(keptHooks, nestedItem)
		}
		if groupChanged && len(keptHooks) == 0 {
			continue
		}
		if groupChanged {
			obj["hooks"] = keptHooks
		}
		cleaned = append(cleaned, item)
	}
	return cleaned, changed
}

func rtkClaudeResidual() (string, error) {
	root, exists, err := readRTKJSONObject(rtkClaudeSettingsFile())
	if errors.Is(err, os.ErrNotExist) || !exists {
		return "", nil
	}
	if err != nil {
		return "Claude settings.json 无法解析", nil
	}
	hooks, ok := root["hooks"].(map[string]any)
	if !ok {
		return "", nil
	}
	items, ok := hooks[rtkClaudePreToolUseEvent].([]any)
	if !ok {
		return "Claude settings.json 的 PreToolUse 不是数组", nil
	}
	count := 0
	for _, item := range items {
		if isRTKClaudeHookEntry(item) {
			count++
		}
	}
	if count > 1 {
		return "Claude settings.json 存在多个 RTK Hook", nil
	}
	return "", nil
}

func cleanRTKCopilotEntries(items []any, keepCommand string) ([]any, bool, bool) {
	cleaned := make([]any, 0, len(items))
	changed := false
	found := false
	for _, item := range items {
		if isRTKCopilotHookEntry(item) {
			obj := item.(map[string]any)
			command, _ := obj["command"].(string)
			if keepCommand != "" && strings.EqualFold(strings.TrimSpace(command), keepCommand) {
				found = true
				cleaned = append(cleaned, item)
			} else {
				changed = true
			}
			continue
		}
		cleaned = append(cleaned, item)
	}
	return cleaned, changed, found
}

func installRTKCopilotIntegration(executable string) error {
	directory := rtkCopilotDirectory()
	if directory == "" {
		return nil
	}
	info, err := os.Stat(directory)
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	if err != nil {
		return err
	}
	if !info.IsDir() {
		return fmt.Errorf("Copilot 配置路径不是目录: %s", directory)
	}
	if err := os.MkdirAll(filepath.Dir(rtkCopilotHookFile()), 0o755); err != nil {
		return err
	}

	root, _, err := readRTKJSONObject(rtkCopilotHookFile())
	if err != nil {
		return fmt.Errorf("解析 Copilot Hook 配置失败: %w", err)
	}
	hooks, hooksPresent := root["hooks"].(map[string]any)
	if _, exists := root["hooks"]; exists && !hooksPresent {
		return errors.New("Copilot hooks 不是对象，拒绝覆盖用户配置")
	}
	if hooks == nil {
		hooks = map[string]any{}
		root["hooks"] = hooks
	}
	command := rtkCopilotHookCommand(executable)
	items := []any{}
	if raw, exists := hooks[rtkCopilotPreToolUseEvent]; exists {
		var ok bool
		items, ok = raw.([]any)
		if !ok {
			return errors.New("Copilot hooks.PreToolUse 不是数组，拒绝覆盖用户配置")
		}
	}
	found := rtkCopilotHookEntriesContainCommand(items, command)
	changed := false
	if !found {
		items = append(items, map[string]any{
			"type":    "command",
			"command": command,
			"cwd":     ".",
			"timeout": 5,
		})
		changed = true
	}
	hooks[rtkCopilotPreToolUseEvent] = items
	if !changed {
		return nil
	}
	root["version"] = 1
	return writeRTKJSONObject(rtkCopilotHookFile(), root)
}

func rtkCopilotHookEntriesContainCommand(items []any, wantCommand string) bool {
	for _, item := range items {
		obj, ok := item.(map[string]any)
		if !ok {
			continue
		}
		command, _ := obj["command"].(string)
		if strings.EqualFold(strings.TrimSpace(command), strings.TrimSpace(wantCommand)) {
			return true
		}
	}
	return false
}

func rtkCopilotConfiguredForCommand(filePath, command string) bool {
	root, exists, err := readRTKJSONObject(filePath)
	if err != nil || !exists {
		return false
	}
	hooks, ok := root["hooks"].(map[string]any)
	if !ok {
		return false
	}
	items, ok := hooks[rtkCopilotPreToolUseEvent].([]any)
	return ok && rtkCopilotHookEntriesContainCommand(items, command)
}

func rtkCopilotConfiguredForExecutable(executable string) bool {
	return rtkCopilotConfiguredForCommand(rtkCopilotHookFile(), rtkCopilotHookCommand(executable))
}

func rtkCopilotConfigured() bool {
	root, exists, err := readRTKJSONObject(rtkCopilotHookFile())
	if err != nil || !exists {
		return false
	}
	hooks, ok := root["hooks"].(map[string]any)
	if !ok {
		return false
	}
	items, ok := hooks[rtkCopilotPreToolUseEvent].([]any)
	if !ok {
		return false
	}
	for _, item := range items {
		if isRTKCopilotHookEntry(item) {
			return true
		}
	}
	return false
}

func removeRTKCopilotIntegration() error {
	executable, err := rtkExecutablePath()
	if err != nil {
		return err
	}
	return removeRTKCopilotIntegrationCommand(rtkCopilotHookFile(), rtkCopilotHookCommand(executable))
}

func removeRTKCopilotIntegrationCommand(filePath, command string) error {
	root, exists, err := readRTKJSONObject(filePath)
	if errors.Is(err, os.ErrNotExist) || !exists {
		return nil
	}
	if err != nil {
		return fmt.Errorf("解析 Copilot Hook 配置失败: %w", err)
	}
	hooks, hooksPresent := root["hooks"].(map[string]any)
	if _, exists := root["hooks"]; exists && !hooksPresent {
		return errors.New("Copilot hooks 不是对象，拒绝覆盖用户配置")
	}
	if hooks == nil {
		return nil
	}
	changed := false
	for _, key := range []string{rtkCopilotPreToolUseEvent} {
		raw, exists := hooks[key]
		if !exists {
			continue
		}
		items, ok := raw.([]any)
		if !ok {
			return fmt.Errorf("Copilot hooks.%s 不是数组，拒绝覆盖用户配置", key)
		}
		cleaned, itemChanged := removeRTKCopilotEntriesByCommand(items, command)
		if !itemChanged {
			continue
		}
		changed = true
		if len(cleaned) == 0 {
			delete(hooks, key)
		} else {
			hooks[key] = cleaned
		}
	}
	if !changed {
		return nil
	}
	if len(hooks) == 0 {
		delete(root, "hooks")
	}
	return writeRTKJSONObject(filePath, root)
}

func removeRTKCopilotEntriesByCommand(items []any, command string) ([]any, bool) {
	cleaned := make([]any, 0, len(items))
	changed := false
	for _, item := range items {
		obj, ok := item.(map[string]any)
		itemCommand, _ := obj["command"].(string)
		if ok && strings.EqualFold(strings.TrimSpace(itemCommand), strings.TrimSpace(command)) {
			changed = true
			continue
		}
		cleaned = append(cleaned, item)
	}
	return cleaned, changed
}
