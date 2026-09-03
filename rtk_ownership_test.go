package main

import (
	"bytes"
	"os"
	"path/filepath"
	"testing"
)

func TestRTKPromptOwnershipRemovesOnlyRecordedBody(t *testing.T) {
	directory := t.TempDir()
	path := filepath.Join(directory, "AGENTS.md")
	owned := []byte("# RTK：Codex 常驻高密度命令规则\n旧版内容\n")
	unmanaged := []byte("第三方 RTK 内容\n")
	content := appendRTKCommandBlock([]byte("用户内容\n"), owned)
	content = appendRTKCommandBlock(content, unmanaged)
	if err := os.WriteFile(path, content, 0o644); err != nil {
		t.Fatal(err)
	}
	artifact := rtkPromptOwnership("codex", path, owned)
	removed, err := removeRTKPromptArtifact(artifact)
	if err != nil {
		t.Fatal(err)
	}
	if !removed {
		t.Fatal("recorded prompt was not removed")
	}
	after, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if bytes.Contains(after, owned) {
		t.Fatalf("recorded prompt remains: %q", after)
	}
	if !bytes.Contains(after, unmanaged) {
		t.Fatalf("unmanaged prompt was removed: %q", after)
	}
}

func TestRTKPromptOwnershipPreservesUnmanagedBlockBytes(t *testing.T) {
	directory := t.TempDir()
	path := filepath.Join(directory, "AGENTS.md")
	owned := []byte("# RTK：Codex 常驻高密度命令规则\n受管内容\n")
	unmanaged := "  " + rtkCommandBlockStart + "\r\n第三方内容\r\n  " + rtkCommandBlockEnd + "\r\n"
	content := []byte("\ufeff用户内容\r\n" + unmanaged + rtkCommandBlockStart + "\n" + string(owned) + rtkCommandBlockEnd + "\n")
	if err := os.WriteFile(path, content, 0o644); err != nil {
		t.Fatal(err)
	}
	removed, err := removeRTKPromptArtifact(rtkPromptOwnership("codex", path, owned))
	if err != nil {
		t.Fatal(err)
	}
	if !removed {
		t.Fatal("recorded prompt was not removed")
	}
	after, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	want := []byte("\ufeff用户内容\r\n" + unmanaged)
	if !bytes.Equal(after, want) {
		t.Fatalf("unmanaged block bytes changed: got %q, want %q", after, want)
	}
}

func TestRTKOwnershipLedgerRemovesEveryRecordedPlatformArtifact(t *testing.T) {
	directory := t.TempDir()
	executable := filepath.Join(directory, "RTK-AI", "rtk.exe")
	codexPath := filepath.Join(directory, "codex", "AGENTS.md")
	claudePromptPath := filepath.Join(directory, "claude", "CLAUDE.md")
	claudeSettingsPath := filepath.Join(directory, "claude", "settings.json")
	copilotPath := filepath.Join(directory, "copilot", "hooks", "rtk-rewrite.json")
	cursorPath := filepath.Join(directory, "cursor", "hooks.json")
	for _, path := range []string{filepath.Dir(codexPath), filepath.Dir(claudePromptPath)} {
		if err := os.MkdirAll(path, 0o755); err != nil {
			t.Fatal(err)
		}
	}

	codexPayload := []byte("RTK Codex 受管规则\n")
	claudePayload := []byte("RTK Claude 受管规则\n")
	if err := os.WriteFile(codexPath, appendRTKCommandBlock([]byte("Codex 用户内容\n"), codexPayload), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(claudePromptPath, appendRTKCommandBlock([]byte("Claude 用户内容\n"), claudePayload), 0o644); err != nil {
		t.Fatal(err)
	}

	claudeCommand := rtkClaudeHookCommand(executable)
	copilotCommand := rtkCopilotHookCommand(executable)
	cursorCommand := rtkCursorHookCommand(executable)
	if err := writeRTKJSONObject(claudeSettingsPath, map[string]any{
		"hooks": map[string]any{
			rtkClaudePreToolUseEvent: []any{map[string]any{
				"matcher": "Bash",
				"hooks": []any{
					map[string]any{"type": "command", "command": claudeCommand},
					map[string]any{"type": "command", "command": "user-claude-hook"},
				},
			}},
		},
	}); err != nil {
		t.Fatal(err)
	}
	if err := writeRTKJSONObject(copilotPath, map[string]any{
		"hooks": map[string]any{
			rtkCopilotPreToolUseEvent: []any{
				map[string]any{"command": copilotCommand},
				map[string]any{"command": "user-copilot-hook"},
			},
		},
	}); err != nil {
		t.Fatal(err)
	}
	if err := writeRTKJSONObject(cursorPath, map[string]any{
		"hooks": map[string]any{
			rtkCursorPreToolUseEvent: []any{
				map[string]any{"command": cursorCommand, "matcher": "Shell"},
				map[string]any{"command": "user-cursor-hook", "matcher": "Shell"},
			},
		},
	}); err != nil {
		t.Fatal(err)
	}

	ledger := emptyRTKOwnershipLedger()
	ledger.Artifacts[rtkArtifactCodexPrompt] = rtkPromptOwnership("codex", codexPath, codexPayload)
	ledger.Artifacts[rtkArtifactClaudePrompt] = rtkPromptOwnership("claude-code", claudePromptPath, claudePayload)
	ledger.Artifacts[rtkArtifactClaudeHook] = rtkHookOwnership("claude-code", claudeSettingsPath, claudeCommand)
	ledger.Artifacts[rtkArtifactCopilotHook] = rtkHookOwnership("copilot", copilotPath, copilotCommand)
	ledger.Artifacts[rtkArtifactCursorHook] = rtkHookOwnership("cursor", cursorPath, cursorCommand)
	if err := removeRTKOwnedArtifacts(ledger); err != nil {
		t.Fatal(err)
	}

	for _, artifact := range []struct {
		name string
		item rtkArtifactOwnership
	}{
		{rtkArtifactCodexPrompt, ledger.Artifacts[rtkArtifactCodexPrompt]},
		{rtkArtifactClaudePrompt, ledger.Artifacts[rtkArtifactClaudePrompt]},
	} {
		present, err := rtkArtifactPresent(artifact.name, artifact.item)
		if err != nil {
			t.Fatal(err)
		}
		if present {
			t.Fatalf("%s remains after cleanup", artifact.name)
		}
	}
	if rtkClaudeHookConfiguredForCommand(claudeSettingsPath, claudeCommand) ||
		rtkCopilotConfiguredForCommand(copilotPath, copilotCommand) ||
		rtkCursorConfiguredForCommand(cursorPath, cursorCommand) {
		t.Fatal("recorded RTK Hook remains after cleanup")
	}
	if !rtkClaudeHookConfiguredForCommand(claudeSettingsPath, "user-claude-hook") ||
		!rtkCopilotConfiguredForCommand(copilotPath, "user-copilot-hook") ||
		!rtkCursorConfiguredForCommand(cursorPath, "user-cursor-hook") {
		t.Fatal("user Hook was removed with recorded RTK Hook")
	}
}

func TestRTKOwnershipLedgerClearsLegacyAgentList(t *testing.T) {
	state := managedToolState{}
	ledger := emptyRTKOwnershipLedger()
	ledger.Artifacts[rtkArtifactCodexPrompt] = rtkPromptOwnership("codex", filepath.Join(t.TempDir(), "AGENTS.md"), rtkCodexAgentInstructions)
	if err := writeRTKOwnershipLedger(&state, ledger); err != nil {
		t.Fatal(err)
	}
	if !containsString(state.OwnedAgents, "codex") {
		t.Fatalf("owned agents = %#v", state.OwnedAgents)
	}
	clearRTKOwnershipLedger(&state)
	if len(state.OwnedAgents) != 0 {
		t.Fatalf("stale owned agents remain: %#v", state.OwnedAgents)
	}
	if state.Metadata != nil {
		t.Fatalf("stale RTK metadata remains: %#v", state.Metadata)
	}
}

func TestRTKActivationArtifactsPresentIgnoresUnownedMatchingPrompt(t *testing.T) {
	directory := t.TempDir()
	path := filepath.Join(directory, "AGENTS.md")
	payload := []byte("RTK 规则\n")
	if err := os.WriteFile(path, appendRTKCommandBlock(nil, payload), 0o644); err != nil {
		t.Fatal(err)
	}
	present, err := rtkActivationArtifactsPresent(managedToolState{}, emptyRTKOwnershipLedger())
	if err != nil {
		t.Fatal(err)
	}
	if present {
		t.Fatal("unowned matching prompt was reported as an RTK residual")
	}
	ledger := emptyRTKOwnershipLedger()
	ledger.Artifacts[rtkArtifactCodexPrompt] = rtkPromptOwnership("codex", path, payload)
	present, err = rtkActivationArtifactsPresent(managedToolState{}, ledger)
	if err != nil {
		t.Fatal(err)
	}
	if !present {
		t.Fatal("recorded prompt was not reported as an RTK residual")
	}
}

func TestRecoverLegacyRTKOwnershipOnlyClaimsRecognizedPrompt(t *testing.T) {
	directory := t.TempDir()
	codexDir := filepath.Join(directory, "codex")
	claudeDir := filepath.Join(directory, "claude")
	if err := os.MkdirAll(codexDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(claudeDir, 0o755); err != nil {
		t.Fatal(err)
	}
	t.Setenv("CODEX_HOME", codexDir)
	t.Setenv("CLAUDE_CONFIG_DIR", claudeDir)
	legacy := []byte("# RTK - Rust Token Killer (Codex CLI)\n旧版内容\n")
	if err := os.WriteFile(filepath.Join(codexDir, "AGENTS.md"), appendRTKCommandBlock(nil, legacy), 0o644); err != nil {
		t.Fatal(err)
	}
	ledger, err := recoverLegacyRTKOwnership(managedToolState{OwnedAgents: []string{"codex"}}, "")
	if err != nil {
		t.Fatal(err)
	}
	artifact, ok := ledger.Artifacts[rtkArtifactCodexPrompt]
	if !ok {
		t.Fatal("recognized legacy Codex prompt was not recovered")
	}
	if artifact.Fingerprint != rtkPromptFingerprint(legacy) {
		t.Fatalf("legacy fingerprint = %q", artifact.Fingerprint)
	}
}

func TestRecoverLegacyRTKOwnershipFindsKnownPromptBesideUnmanagedBlock(t *testing.T) {
	directory := t.TempDir()
	codexDir := filepath.Join(directory, "codex")
	claudeDir := filepath.Join(directory, "claude")
	if err := os.MkdirAll(codexDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(claudeDir, 0o755); err != nil {
		t.Fatal(err)
	}
	t.Setenv("CODEX_HOME", codexDir)
	t.Setenv("CLAUDE_CONFIG_DIR", claudeDir)
	legacy := []byte("# RTK - Rust Token Killer (Codex CLI)\n旧版内容\n")
	unmanaged := []byte("第三方标记内容\n")
	path := filepath.Join(codexDir, "AGENTS.md")
	content := appendRTKCommandBlock([]byte("用户内容\n"), legacy)
	content = appendRTKCommandBlock(content, unmanaged)
	if err := os.WriteFile(path, content, 0o644); err != nil {
		t.Fatal(err)
	}
	ledger, err := recoverLegacyRTKOwnership(managedToolState{OwnedAgents: []string{"codex"}}, "")
	if err != nil {
		t.Fatal(err)
	}
	artifact, ok := ledger.Artifacts[rtkArtifactCodexPrompt]
	if !ok {
		t.Fatal("known legacy prompt was not recovered beside unmanaged block")
	}
	if err := removeRTKOwnedArtifacts(ledger); err != nil {
		t.Fatal(err)
	}
	after, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if bytes.Contains(after, legacy) || !bytes.Contains(after, unmanaged) {
		t.Fatalf("legacy cleanup did not preserve unmanaged block: %q", after)
	}
	if artifact.Fingerprint != rtkPromptFingerprint(legacy) {
		t.Fatalf("legacy fingerprint = %q", artifact.Fingerprint)
	}
}
