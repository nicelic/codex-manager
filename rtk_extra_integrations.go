package main

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

const rtkCursorPreToolUseEvent = "preToolUse"

func quoteWindowsExecutable(path string) string {
	clean := filepath.Clean(path)
	return `"` + strings.ReplaceAll(clean, `"`, `\"`) + `"`
}

func userAgentDirectory(name string) string {
	home, err := os.UserHomeDir()
	if err != nil {
		return ""
	}
	return filepath.Join(home, name)
}

func rtkCursorHooksFile() string { return filepath.Join(userAgentDirectory(".cursor"), "hooks.json") }

func rtkCursorHookCommand(executable string) string {
	return quoteWindowsExecutable(executable) + " hook cursor"
}

func rtkCursorAvailable() bool {
	info, err := os.Stat(userAgentDirectory(".cursor"))
	return err == nil && info.IsDir()
}

func rtkCursorConfigured() bool {
	root, exists, err := readRTKJSONObject(rtkCursorHooksFile())
	if err != nil || !exists {
		return false
	}
	hooks, ok := root["hooks"].(map[string]any)
	if !ok {
		return false
	}
	items, ok := hooks[rtkCursorPreToolUseEvent].([]any)
	if !ok {
		return false
	}
	for _, item := range items {
		if isRTKCursorHookEntry(item) {
			return true
		}
	}
	return false
}

func rtkCursorConfiguredForCommand(filePath, command string) bool {
	root, exists, err := readRTKJSONObject(filePath)
	if err != nil || !exists {
		return false
	}
	hooks, ok := root["hooks"].(map[string]any)
	if !ok {
		return false
	}
	items, ok := hooks[rtkCursorPreToolUseEvent].([]any)
	if !ok {
		return false
	}
	for _, item := range items {
		obj, ok := item.(map[string]any)
		itemCommand, _ := obj["command"].(string)
		if ok && strings.EqualFold(strings.TrimSpace(itemCommand), strings.TrimSpace(command)) {
			return true
		}
	}
	return false
}

func rtkCursorConfiguredForExecutable(executable string) bool {
	return rtkCursorConfiguredForCommand(rtkCursorHooksFile(), rtkCursorHookCommand(executable))
}

func isRTKCursorCommand(command string) bool {
	trimmed := strings.TrimSpace(command)
	const suffix = " hook cursor"
	if len(trimmed) <= len(suffix) || !strings.EqualFold(trimmed[len(trimmed)-len(suffix):], suffix) {
		return false
	}
	executable := strings.Trim(strings.TrimSpace(trimmed[:len(trimmed)-len(suffix)]), `"'`)
	base := strings.ToLower(filepath.Base(filepath.Clean(executable)))
	return base == "rtk.exe" || base == "rtk"
}

func isRTKCursorHookEntry(item any) bool {
	obj, ok := item.(map[string]any)
	if !ok {
		return false
	}
	command, ok := obj["command"].(string)
	if !ok || !isRTKCursorCommand(command) {
		return false
	}
	matcher, _ := obj["matcher"].(string)
	return strings.TrimSpace(matcher) == "" || strings.EqualFold(strings.TrimSpace(matcher), "Shell")
}

func removeRTKCursorEntries(hooks map[string]any, keys []string, command string) (bool, error) {
	changed := false
	for _, key := range keys {
		raw, exists := hooks[key]
		if !exists {
			continue
		}
		items, ok := raw.([]any)
		if !ok {
			return false, fmt.Errorf("Cursor hooks.%s 不是数组，拒绝覆盖用户配置", key)
		}
		filtered := make([]any, 0, len(items))
		for _, item := range items {
			obj, _ := item.(map[string]any)
			itemCommand, _ := obj["command"].(string)
			if obj != nil && strings.EqualFold(strings.TrimSpace(itemCommand), strings.TrimSpace(command)) {
				changed = true
				continue
			}
			filtered = append(filtered, item)
		}
		if len(filtered) == 0 {
			delete(hooks, key)
		} else {
			hooks[key] = filtered
		}
	}
	return changed, nil
}

func rtkModifiedAgentNames(ledger rtkOwnershipLedger) []string {
	seen := map[string]struct{}{}
	for name, artifact := range ledger.Artifacts {
		present, presentErr := rtkArtifactPresent(name, artifact)
		if presentErr != nil || !present {
			continue
		}
		seen[artifact.Agent] = struct{}{}
	}
	result := make([]string, 0, len(seen))
	for name := range seen {
		result = append(result, name)
	}
	sort.Strings(result)
	return result
}

type rtkIntegrationSnapshot struct {
	agentName string
	path      string
	exists    bool
	data      []byte
}

func snapshotRTKIntegrations() map[string]rtkIntegrationSnapshot {
	result := make(map[string]rtkIntegrationSnapshot, 4)
	if targets, err := discoverAssistantTargets(); err == nil {
		for _, target := range targets {
			snapshot := rtkIntegrationSnapshot{agentName: target.agentName, path: target.filePath}
			if data, readErr := os.ReadFile(target.filePath); readErr == nil {
				snapshot.exists = true
				snapshot.data = append([]byte(nil), data...)
			}
			result[target.snapshotKey] = snapshot
		}
	}
	for key, target := range map[string]struct {
		agentName string
		path      string
	}{
		"claude-code-hook": {agentName: "claude-code", path: rtkClaudeSettingsFile()},
		"copilot":          {agentName: "copilot", path: rtkCopilotHookFile()},
		"cursor":           {agentName: "cursor", path: rtkCursorHooksFile()},
	} {
		snapshot := rtkIntegrationSnapshot{agentName: target.agentName, path: target.path}
		if data, err := os.ReadFile(target.path); err == nil {
			snapshot.exists = true
			snapshot.data = append([]byte(nil), data...)
		}
		result[key] = snapshot
	}
	return result
}

func changedRTKIntegrations(before map[string]rtkIntegrationSnapshot) []string {
	result := make([]string, 0, len(before))
	for _, snapshot := range before {
		after, err := os.ReadFile(snapshot.path)
		changed := false
		if snapshot.exists {
			if err == nil && !bytes.Equal(snapshot.data, after) {
				changed = true
			}
		} else if err == nil {
			changed = true
		}
		if changed && !containsString(result, snapshot.agentName) {
			result = append(result, snapshot.agentName)
		}
	}
	sort.Strings(result)
	return result
}

func restoreRTKIntegrationSnapshots(before map[string]rtkIntegrationSnapshot) error {
	for _, snapshot := range before {
		current, err := os.ReadFile(snapshot.path)
		if !snapshot.exists {
			if errors.Is(err, os.ErrNotExist) {
				continue
			}
			if err != nil {
				return err
			}
			if err := os.Remove(snapshot.path); err != nil {
				return err
			}
			continue
		}
		if err != nil {
			if errors.Is(err, os.ErrNotExist) {
				if err := replaceUTF8File(snapshot.path, snapshot.data); err != nil {
					return err
				}
				continue
			}
			return err
		}
		if !bytes.Equal(current, snapshot.data) {
			if err := replaceUTF8File(snapshot.path, snapshot.data); err != nil {
				return err
			}
		}
	}
	return nil
}

func installRTKCursorHook(executable string) error {
	filePath := rtkCursorHooksFile()
	directoryInfo, err := os.Stat(filepath.Dir(filePath))
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	if err != nil {
		return err
	}
	if !directoryInfo.IsDir() {
		return fmt.Errorf("Cursor 配置路径不是目录: %s", filepath.Dir(filePath))
	}
	data, _, err := readRTKJSONObject(filePath)
	if err != nil {
		return fmt.Errorf("解析 Cursor hooks.json 失败: %w", err)
	}
	hooks, hooksPresent := data["hooks"].(map[string]any)
	if _, exists := data["hooks"]; exists && !hooksPresent {
		return errors.New("Cursor hooks 不是对象，拒绝覆盖用户配置")
	}
	changed := false
	if hooks == nil {
		hooks = map[string]any{}
		data["hooks"] = hooks
		changed = true
	}
	command := rtkCursorHookCommand(executable)
	items := []any{}
	if raw, exists := hooks[rtkCursorPreToolUseEvent]; exists {
		var ok bool
		items, ok = raw.([]any)
		if !ok {
			return fmt.Errorf("Cursor hooks.%s 不是数组，拒绝覆盖用户配置", rtkCursorPreToolUseEvent)
		}
	}
	found := false
	for _, item := range items {
		obj, ok := item.(map[string]any)
		itemCommand, _ := obj["command"].(string)
		if ok && strings.EqualFold(strings.TrimSpace(itemCommand), strings.TrimSpace(command)) {
			found = true
			break
		}
	}
	if !found {
		items = append(items, map[string]any{"command": command, "matcher": "Shell"})
		hooks[rtkCursorPreToolUseEvent] = items
		changed = true
	}
	if !changed {
		return nil
	}
	encoded, err := json.MarshalIndent(data, "", "  ")
	if err != nil {
		return err
	}
	return replaceUTF8File(filePath, append(encoded, '\n'))
}

func removeRTKCursorHook(executable string) error {
	return removeRTKCursorHookCommand(rtkCursorHooksFile(), rtkCursorHookCommand(executable))
}

func removeRTKCursorHookCommand(filePath, command string) error {
	data, exists, err := readRTKJSONObject(filePath)
	if err != nil {
		return fmt.Errorf("解析 Cursor hooks.json 失败: %w", err)
	}
	if !exists {
		return nil
	}
	hooks, hooksPresent := data["hooks"].(map[string]any)
	if _, exists := data["hooks"]; exists && !hooksPresent {
		return errors.New("Cursor hooks 不是对象，拒绝覆盖用户配置")
	}
	if hooks == nil {
		return nil
	}
	changed, err := removeRTKCursorEntries(hooks, []string{rtkCursorPreToolUseEvent}, command)
	if err != nil {
		return err
	}
	if !changed {
		return nil
	}
	if len(hooks) == 0 {
		delete(data, "hooks")
	}
	return writeRTKJSONObject(filePath, data)
}
