package main

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestRTKCopilotIntegrationUsesOfficialHook(t *testing.T) {
	home := t.TempDir()
	t.Setenv("USERPROFILE", home)
	t.Setenv("HOME", home)
	copilotDir := filepath.Join(home, ".copilot")
	if err := os.MkdirAll(copilotDir, 0o755); err != nil {
		t.Fatal(err)
	}
	hookPath := filepath.Join(copilotDir, "hooks", "rtk-rewrite.json")
	original := []byte(`{"version":1,"hooks":{"PreToolUse":[{"type":"command","command":"user-hook"}]}}`)
	if err := os.MkdirAll(filepath.Dir(hookPath), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(hookPath, original, 0o644); err != nil {
		t.Fatal(err)
	}
	executable := filepath.Join(home, "RTK-AI", "rtk.exe")
	if err := installRTKCopilotIntegration(executable); err != nil {
		t.Fatalf("installRTKCopilotIntegration() error = %v", err)
	}
	after, err := os.ReadFile(hookPath)
	if err != nil {
		t.Fatal(err)
	}
	var root map[string]any
	if err := json.Unmarshal(after, &root); err != nil {
		t.Fatal(err)
	}
	hooks := root["hooks"].(map[string]any)
	items := hooks[rtkCopilotPreToolUseEvent].([]any)
	command := quoteWindowsExecutable(executable) + " hook copilot"
	found := false
	for _, item := range items {
		obj := item.(map[string]any)
		if obj["command"] == command {
			found = true
		}
	}
	if !found {
		t.Fatalf("Copilot PreToolUse does not contain the RTK command: %q", command)
	}
	if !rtkCopilotConfigured() {
		t.Fatal("official Copilot hook was not detected")
	}
	if err := removeRTKCopilotIntegrationCommand(hookPath, rtkCopilotHookCommand(executable)); err != nil {
		t.Fatalf("removeRTKCopilotIntegrationCommand() error = %v", err)
	}
	cleaned, err := os.ReadFile(hookPath)
	if err != nil {
		t.Fatal(err)
	}
	var cleanedRoot map[string]any
	if err := json.Unmarshal(cleaned, &cleanedRoot); err != nil {
		t.Fatal(err)
	}
	cleanedHooks := cleanedRoot["hooks"].(map[string]any)
	cleanedItems := cleanedHooks[rtkCopilotPreToolUseEvent].([]any)
	if len(cleanedItems) != 1 || cleanedItems[0].(map[string]any)["command"] != "user-hook" {
		t.Fatalf("user Copilot hook was not preserved: %s", cleaned)
	}
}

func TestRTKCopilotCleanupKeepsOtherRTKExecutable(t *testing.T) {
	directory := t.TempDir()
	hookPath := filepath.Join(directory, "rtk-rewrite.json")
	current := filepath.Join(directory, "current", "rtk.exe")
	other := filepath.Join(directory, "other", "rtk.exe")
	root := map[string]any{
		"hooks": map[string]any{
			rtkCopilotPreToolUseEvent: []any{
				map[string]any{"type": "command", "command": rtkCopilotHookCommand(current)},
				map[string]any{"type": "command", "command": rtkCopilotHookCommand(other)},
			},
		},
	}
	if err := writeRTKJSONObject(hookPath, root); err != nil {
		t.Fatal(err)
	}
	if err := removeRTKCopilotIntegrationCommand(hookPath, rtkCopilotHookCommand(current)); err != nil {
		t.Fatal(err)
	}
	if rtkCopilotConfiguredForCommand(hookPath, rtkCopilotHookCommand(current)) {
		t.Fatal("current RTK Copilot Hook remains")
	}
	if !rtkCopilotConfiguredForCommand(hookPath, rtkCopilotHookCommand(other)) {
		t.Fatal("other RTK Copilot Hook was removed")
	}
}

func TestRTKCursorHookUsesPreToolUse(t *testing.T) {
	home := t.TempDir()
	t.Setenv("USERPROFILE", home)
	t.Setenv("HOME", home)
	cursorDir := filepath.Join(home, ".cursor")
	if err := os.MkdirAll(cursorDir, 0o755); err != nil {
		t.Fatal(err)
	}
	filePath := filepath.Join(cursorDir, "hooks.json")
	executable := filepath.Join(home, "RTK-AI", "rtk.exe")
	command := quoteWindowsExecutable(executable) + " hook cursor"
	original := map[string]any{
		"version": 1,
		"hooks": map[string]any{
			"beforeShellExecution": []any{map[string]any{"command": "user-before"}},
			"afterShellExecution":  []any{map[string]any{"command": "user-after"}},
			"preToolUse": []any{
				map[string]any{"command": "user-pre", "matcher": "Shell"},
			},
		},
	}
	data, err := json.MarshalIndent(original, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filePath, append(data, '\n'), 0o644); err != nil {
		t.Fatal(err)
	}

	if err := installRTKCursorHook(executable); err != nil {
		t.Fatalf("installRTKCursorHook() error = %v", err)
	}
	updated, err := os.ReadFile(filePath)
	if err != nil {
		t.Fatal(err)
	}
	root := map[string]any{}
	if err := json.Unmarshal(updated, &root); err != nil {
		t.Fatal(err)
	}
	hooks := root["hooks"].(map[string]any)
	preToolUse := hooks[rtkCursorPreToolUseEvent].([]any)
	if len(preToolUse) != 2 {
		t.Fatalf("preToolUse entry count = %d, want 2", len(preToolUse))
	}
	found := false
	for _, item := range preToolUse {
		obj := item.(map[string]any)
		if obj["command"] == command && obj["matcher"] == "Shell" {
			found = true
		}
	}
	if !found {
		t.Fatalf("preToolUse does not contain the RTK command: %q", command)
	}
	if !rtkCursorConfigured() {
		t.Fatal("valid preToolUse RTK hook was not detected")
	}

	if err := installRTKCursorHook(executable); err != nil {
		t.Fatalf("second installRTKCursorHook() error = %v", err)
	}
	second, err := os.ReadFile(filePath)
	if err != nil {
		t.Fatal(err)
	}
	if string(second) != string(updated) {
		t.Fatal("second Cursor hook installation was not idempotent")
	}

	if err := removeRTKCursorHook(executable); err != nil {
		t.Fatalf("removeRTKCursorHook() error = %v", err)
	}
	cleaned, err := os.ReadFile(filePath)
	if err != nil {
		t.Fatal(err)
	}
	var cleanedRoot map[string]any
	if err := json.Unmarshal(cleaned, &cleanedRoot); err != nil {
		t.Fatal(err)
	}
	cleanedHooks := cleanedRoot["hooks"].(map[string]any)
	if _, exists := cleanedHooks[rtkCursorPreToolUseEvent]; !exists {
		t.Fatal("user preToolUse hook was removed with RTK hook")
	}
	if cleanedHooks["beforeShellExecution"].([]any)[0].(map[string]any)["command"] != "user-before" {
		t.Fatal("user beforeShellExecution hook was not preserved")
	}
	if cleanedHooks["afterShellExecution"].([]any)[0].(map[string]any)["command"] != "user-after" {
		t.Fatal("user afterShellExecution hook was not preserved")
	}
	if rtkCursorConfigured() {
		t.Fatal("removed Cursor RTK hook is still reported as configured")
	}
}

func TestRTKCursorCleanupKeepsOtherRTKExecutable(t *testing.T) {
	directory := t.TempDir()
	filePath := filepath.Join(directory, "hooks.json")
	current := filepath.Join(directory, "current", "rtk.exe")
	other := filepath.Join(directory, "other", "rtk.exe")
	root := map[string]any{
		"hooks": map[string]any{
			rtkCursorPreToolUseEvent: []any{
				map[string]any{"command": rtkCursorHookCommand(current), "matcher": "Shell"},
				map[string]any{"command": rtkCursorHookCommand(other), "matcher": "Shell"},
			},
		},
	}
	if err := writeRTKJSONObject(filePath, root); err != nil {
		t.Fatal(err)
	}
	if err := removeRTKCursorHookCommand(filePath, rtkCursorHookCommand(current)); err != nil {
		t.Fatal(err)
	}
	if rtkCursorConfiguredForCommand(filePath, rtkCursorHookCommand(current)) {
		t.Fatal("current RTK Cursor Hook remains")
	}
	if !rtkCursorConfiguredForCommand(filePath, rtkCursorHookCommand(other)) {
		t.Fatal("other RTK Cursor Hook was removed")
	}
}

func TestRTKCursorHookCreatesMissingOfficialFile(t *testing.T) {
	home := t.TempDir()
	t.Setenv("USERPROFILE", home)
	t.Setenv("HOME", home)
	cursorDir := filepath.Join(home, ".cursor")
	if err := os.MkdirAll(cursorDir, 0o755); err != nil {
		t.Fatal(err)
	}
	executable := filepath.Join(home, "RTK-AI", "rtk.exe")
	if err := installRTKCursorHook(executable); err != nil {
		t.Fatalf("installRTKCursorHook() error = %v", err)
	}
	if _, err := os.Stat(filepath.Join(cursorDir, "hooks.json")); err != nil {
		t.Fatalf("hooks.json was not created: %v", err)
	}
	if !rtkCursorConfigured() {
		t.Fatal("new Cursor official hook was not detected")
	}
}

func TestRTKCursorHookDoesNotDuplicateExistingNativeHook(t *testing.T) {
	home := t.TempDir()
	t.Setenv("USERPROFILE", home)
	t.Setenv("HOME", home)
	cursorDir := filepath.Join(home, ".cursor")
	if err := os.MkdirAll(cursorDir, 0o755); err != nil {
		t.Fatal(err)
	}
	filePath := filepath.Join(cursorDir, "hooks.json")
	content := []byte(`{"version":1,"hooks":{"preToolUse":[{"command":"rtk hook cursor","matcher":"Shell"}]}}`)
	if err := os.WriteFile(filePath, content, 0o644); err != nil {
		t.Fatal(err)
	}

	if err := installRTKCursorHook(filepath.Join(home, "RTK-AI", "rtk.exe")); err != nil {
		t.Fatalf("installRTKCursorHook() error = %v", err)
	}
	updated, err := os.ReadFile(filePath)
	if err != nil {
		t.Fatal(err)
	}
	if string(updated) == string(content) {
		t.Fatalf("current Cursor Hook was not added beside the existing external RTK Hook: %s", updated)
	}
	if !rtkCursorConfiguredForExecutable(filepath.Join(home, "RTK-AI", "rtk.exe")) {
		t.Fatal("current Cursor Hook was not detected")
	}
	if !strings.Contains(string(updated), `"command": "rtk hook cursor"`) {
		t.Fatal("existing external Cursor Hook was removed")
	}
}
