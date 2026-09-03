package main

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

func TestRTKClaudeIntegrationUsesOfficialHook(t *testing.T) {
	home := t.TempDir()
	t.Setenv("USERPROFILE", home)
	t.Setenv("HOME", home)
	claudeDir := filepath.Join(home, ".claude")
	t.Setenv("CLAUDE_CONFIG_DIR", claudeDir)
	if err := os.MkdirAll(claudeDir, 0o755); err != nil {
		t.Fatal(err)
	}
	settingsPath := filepath.Join(claudeDir, "settings.json")
	original := []byte(`{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"user-hook"}]}]}}`)
	if err := os.WriteFile(settingsPath, original, 0o644); err != nil {
		t.Fatal(err)
	}
	executable := filepath.Join(home, "RTK-AI", "rtk.exe")
	if err := installRTKClaudeIntegration(executable); err != nil {
		t.Fatalf("installRTKClaudeIntegration() error = %v", err)
	}
	if !rtkClaudeHookConfiguredForExecutable(executable) {
		t.Fatal("official Claude Hook was not detected")
	}
	if rtkClaudeConfiguredForExecutable(executable) {
		t.Fatal("Claude was reported as fully configured before its prompt was installed")
	}
	if err := installRTKAssistantIntegrations(); err != nil {
		t.Fatalf("installRTKAssistantIntegrations() error = %v", err)
	}
	if !queryRTKClaudePromptConfigured() {
		t.Fatal("Claude prompt was not detected")
	}
	if !rtkClaudeConfiguredForExecutable(executable) {
		t.Fatal("Claude Hook and prompt were not reported as a complete integration")
	}
	updated, err := os.ReadFile(settingsPath)
	if err != nil {
		t.Fatal(err)
	}
	var root map[string]any
	if err := json.Unmarshal(updated, &root); err != nil {
		t.Fatal(err)
	}
	preToolUse := root["hooks"].(map[string]any)[rtkClaudePreToolUseEvent].([]any)
	if len(preToolUse) != 2 {
		t.Fatalf("PreToolUse entry count = %d, want 2", len(preToolUse))
	}

	if err := removeRTKClaudeIntegrationCommand(settingsPath, rtkClaudeHookCommand(executable)); err != nil {
		t.Fatalf("removeRTKClaudeIntegrationCommand() error = %v", err)
	}
	cleaned, err := os.ReadFile(settingsPath)
	if err != nil {
		t.Fatal(err)
	}
	var cleanedRoot map[string]any
	if err := json.Unmarshal(cleaned, &cleanedRoot); err != nil {
		t.Fatal(err)
	}
	cleanedItems := cleanedRoot["hooks"].(map[string]any)[rtkClaudePreToolUseEvent].([]any)
	if len(cleanedItems) != 1 || cleanedItems[0].(map[string]any)["hooks"].([]any)[0].(map[string]any)["command"] != "user-hook" {
		t.Fatalf("user Claude settings changed after cleanup: %s", cleaned)
	}
	if rtkClaudeConfiguredForExecutable(executable) {
		t.Fatal("removed Claude hook is still reported as configured")
	}
	if !queryRTKClaudePromptConfigured() {
		t.Fatal("Claude cleanup unexpectedly removed the prompt with the Hook")
	}
	if err := removeRTKAssistantIntegrationsForAgents([]string{"claude-code"}); err != nil {
		t.Fatalf("removeRTKAssistantIntegrationsForAgents() error = %v", err)
	}
	if queryRTKClaudePromptConfigured() {
		t.Fatal("removed Claude prompt is still reported as configured")
	}
}

func TestRTKClaudeCleanupKeepsOtherRTKExecutable(t *testing.T) {
	directory := t.TempDir()
	settingsPath := filepath.Join(directory, "settings.json")
	current := filepath.Join(directory, "current", "rtk.exe")
	other := filepath.Join(directory, "other", "rtk.exe")
	root := map[string]any{
		"hooks": map[string]any{
			rtkClaudePreToolUseEvent: []any{
				map[string]any{"matcher": "Bash", "hooks": []any{map[string]any{"type": "command", "command": rtkClaudeHookCommand(current)}}},
				map[string]any{"matcher": "Bash", "hooks": []any{map[string]any{"type": "command", "command": rtkClaudeHookCommand(other)}}},
			},
		},
	}
	if err := writeRTKJSONObject(settingsPath, root); err != nil {
		t.Fatal(err)
	}
	if err := removeRTKClaudeIntegrationCommand(settingsPath, rtkClaudeHookCommand(current)); err != nil {
		t.Fatal(err)
	}
	if rtkClaudeHookConfiguredForCommand(settingsPath, rtkClaudeHookCommand(current)) {
		t.Fatal("current RTK Claude Hook remains")
	}
	if !rtkClaudeHookConfiguredForCommand(settingsPath, rtkClaudeHookCommand(other)) {
		t.Fatal("other RTK Claude Hook was removed")
	}
}

func TestRTKIntegrationSnapshotTracksAndRemovesNewHookFiles(t *testing.T) {
	home := t.TempDir()
	t.Setenv("USERPROFILE", home)
	t.Setenv("HOME", home)
	claudeDir := filepath.Join(home, ".claude")
	copilotDir := filepath.Join(home, ".copilot")
	cursorDir := filepath.Join(home, ".cursor")
	t.Setenv("CLAUDE_CONFIG_DIR", claudeDir)
	if err := os.MkdirAll(claudeDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(copilotDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(cursorDir, 0o755); err != nil {
		t.Fatal(err)
	}

	before := snapshotRTKIntegrations()
	executable := filepath.Join(home, "RTK-AI", "rtk.exe")
	if err := installRTKAssistantIntegrations(); err != nil {
		t.Fatal(err)
	}
	if err := installRTKClaudeIntegration(executable); err != nil {
		t.Fatal(err)
	}
	if err := installRTKCopilotIntegration(executable); err != nil {
		t.Fatal(err)
	}
	if err := installRTKCursorHook(executable); err != nil {
		t.Fatal(err)
	}
	changed := changedRTKIntegrations(before)
	for _, name := range []string{"claude-code", "copilot", "cursor"} {
		if !containsString(changed, name) {
			t.Fatalf("new %s Hook file was not tracked: %v", name, changed)
		}
	}

	if err := restoreRTKIntegrationSnapshots(before); err != nil {
		t.Fatalf("restoreRTKIntegrationSnapshots() error = %v", err)
	}
	for _, path := range []string{
		filepath.Join(claudeDir, "CLAUDE.md"),
		rtkClaudeSettingsFile(),
		rtkCopilotHookFile(),
		rtkCursorHooksFile(),
	} {
		if _, err := os.Stat(path); !os.IsNotExist(err) {
			t.Fatalf("new Hook file was not removed during restore: %s (%v)", path, err)
		}
	}
}
