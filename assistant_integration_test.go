package main

import (
	"bytes"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestRTKCommandBlockReplacement(t *testing.T) {
	data := []byte("用户内容\r\n\r\n" + rtkCommandBlockStart + "\r\n旧内容\r\n" + rtkCommandBlockEnd + "\r\n结尾\r\n")
	base, count, err := stripRTKCommandBlocks(data)
	if err != nil {
		t.Fatalf("stripRTKCommandBlocks() error = %v", err)
	}
	if count != 1 {
		t.Fatalf("block count = %d, want 1", count)
	}
	if want := "用户内容\r\n\r\n结尾\r\n"; string(base) != want {
		t.Fatalf("base = %q, want %q", base, want)
	}
	updated := appendRTKCommandBlock(base, []byte("新内容\n"))
	want := "用户内容\r\n\r\n结尾\r\n\r\n" + rtkCommandBlockStart + "\r\n新内容\r\n" + rtkCommandBlockEnd + "\r\n"
	if string(updated) != want {
		t.Fatalf("updated = %q, want %q", updated, want)
	}
	secondBase, _, err := stripRTKCommandBlocks(updated)
	if err != nil {
		t.Fatalf("second strip error = %v", err)
	}
	if !bytes.Equal(appendRTKCommandBlock(secondBase, []byte("新内容\n")), updated) {
		t.Fatal("replacing the same block is not idempotent")
	}
}

func TestRTKCommandBlockMalformed(t *testing.T) {
	for _, data := range [][]byte{
		[]byte(rtkCommandBlockStart + "\n内容\n"),
		[]byte(rtkCommandBlockEnd + "\n"),
		[]byte(rtkCommandBlockStart + "\n" + rtkCommandBlockStart + "\n" + rtkCommandBlockEnd + "\n"),
	} {
		if _, _, err := stripRTKCommandBlocks(data); err == nil {
			t.Fatalf("stripRTKCommandBlocks(%q) returned nil error", data)
		}
	}
}

func TestRTKCodexAgentInstructionsFitDefaultLimit(t *testing.T) {
	if err := validateRTKCodexDocuments(); err != nil {
		t.Fatalf("validateRTKCodexDocuments() error = %v", err)
	}
	if got := len(rtkCodexAgentInstructions); got > rtkCodexAgentInstructionsMaxBytes {
		t.Fatalf("resident instruction size = %d, want <= %d", got, rtkCodexAgentInstructionsMaxBytes)
	}
}

func TestRTKCodexAgentInstructionsRetainCommandDecisionMatrix(t *testing.T) {
	resident := string(rtkCodexAgentInstructions)
	for _, want := range []string{
		"## 2. 正式 CLI 完整索引与精确语法",
		"## 4. 静态 rewrite 决策表",
		"### 89 条原始静态模式",
		"### 63 条原始 fallback 模式",
		"`rtk prisma db-push`",
		"`npm.cmd ...`",
		"`rtk hook codex`",
		"`^(?:git|yadm)\\s+",
		"`^basedpyright\\b`",
		"`^swift\\s+(build|test)\\b`",
		"PowerShell cmdlet/表达式",
	} {
		if !strings.Contains(resident, want) {
			t.Errorf("resident instructions are missing %q", want)
		}
	}
	if got := strings.Count(resident, " -> `rtk "); got < 89 {
		t.Errorf("rewrite rule count = %d, want at least 89", got)
	}
	if got := strings.Count(resident, "：`^"); got < 63 {
		t.Errorf("fallback rule count = %d, want at least 63", got)
	}
	if strings.Contains(resident, "使用 RTK 前必须读取完整参考") {
		t.Error("resident instructions regressed to a reference-only dispatcher")
	}
}

func TestRTKMarkdownAssistantPayloadsUseResidentInstructions(t *testing.T) {
	if got := rtkAssistantCommandPayload(); !bytes.Equal(got, rtkCodexAgentInstructions) {
		t.Fatal("Codex payload does not use resident instructions")
	}
	if got := rtkClaudeAssistantCommandPayload(); !bytes.Equal(got, rtkClaudeAgentInstructions) {
		t.Fatal("Claude payload does not use resident instructions")
	}
	if len(rtkClaudeAgentInstructions) == 0 {
		t.Fatal("Claude resident instructions are empty")
	}
}

func TestAssistantIntegrationsRejectMalformedClaudePromptTransactionally(t *testing.T) {
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
	codexFile := filepath.Join(codexDir, "AGENTS.md")
	claudeFile := filepath.Join(claudeDir, "CLAUDE.md")
	codexBefore := []byte("Codex 用户内容\n")
	claudeBefore := []byte(rtkCommandBlockStart + "\nClaude 旧提示词\n")
	if err := os.WriteFile(codexFile, codexBefore, 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(claudeFile, claudeBefore, 0o644); err != nil {
		t.Fatal(err)
	}
	if err := installRTKAssistantIntegrations(); err == nil {
		t.Fatal("installRTKAssistantIntegrations() unexpectedly accepted malformed Claude prompt")
	}
	codexAfter, err := os.ReadFile(codexFile)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(codexAfter, codexBefore) {
		t.Fatalf("Codex Markdown changed after Claude preflight failure: %q", codexAfter)
	}
	claudeAfter, err := os.ReadFile(claudeFile)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(claudeAfter, claudeBefore) {
		t.Fatalf("Claude Markdown changed after failed preflight: %q", claudeAfter)
	}
}

func TestUpsertExistingRTKCommandBlock(t *testing.T) {
	directory := t.TempDir()
	missingFile := filepath.Join(directory, "missing.md")
	if err := upsertExistingRTKCommandBlock(missingFile, []byte("内容\n")); err != nil {
		t.Fatalf("upsert missing file error = %v", err)
	}
	if _, err := os.Stat(missingFile); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("missing target was unexpectedly created: %v", err)
	}

	existingFile := filepath.Join(directory, "AGENTS.md")
	legacyBackup := existingFile + ".codex-rtk-backup"
	if err := os.WriteFile(legacyBackup, []byte("用户文件"), 0o644); err != nil {
		t.Fatalf("write legacy backup sentinel: %v", err)
	}
	if err := os.WriteFile(existingFile, []byte("用户内容\n"), 0o644); err != nil {
		t.Fatalf("write existing file: %v", err)
	}
	if err := upsertExistingRTKCommandBlock(existingFile, []byte("RTK 内容\n")); err != nil {
		t.Fatalf("upsert existing file error = %v", err)
	}
	data, err := os.ReadFile(existingFile)
	if err != nil {
		t.Fatalf("read existing file: %v", err)
	}
	want := "用户内容\n\n" + rtkCommandBlockStart + "\nRTK 内容\n" + rtkCommandBlockEnd + "\n"
	if string(data) != want {
		t.Fatalf("updated file = %q, want %q", data, want)
	}
	backupData, err := os.ReadFile(legacyBackup)
	if err != nil {
		t.Fatalf("read legacy backup sentinel: %v", err)
	}
	if string(backupData) != "用户文件" {
		t.Fatalf("legacy backup sentinel changed: %q", backupData)
	}
	if err := removeRTKCommandBlocks(existingFile); err != nil {
		t.Fatalf("remove blocks error = %v", err)
	}
	data, err = os.ReadFile(existingFile)
	if err != nil {
		t.Fatalf("read cleaned file: %v", err)
	}
	if string(data) != "用户内容\n\n" {
		t.Fatalf("cleaned file = %q", data)
	}
}

func TestAssistantIntegrationsAddClaudePromptWhenDirectoryExists(t *testing.T) {
	directory := t.TempDir()
	codexDir := filepath.Join(directory, "codex")
	claudeDir := filepath.Join(directory, "claude")
	if err := os.MkdirAll(codexDir, 0o755); err != nil {
		t.Fatalf("mkdir codex: %v", err)
	}
	if err := os.MkdirAll(claudeDir, 0o755); err != nil {
		t.Fatalf("mkdir claude: %v", err)
	}
	codexFile := filepath.Join(codexDir, "AGENTS.md")
	claudeFile := filepath.Join(claudeDir, "CLAUDE.md")
	codexBefore := []byte("Codex 用户内容\n")
	if err := os.WriteFile(codexFile, codexBefore, 0o644); err != nil {
		t.Fatalf("write codex: %v", err)
	}
	t.Setenv("CODEX_HOME", codexDir)
	t.Setenv("CLAUDE_CONFIG_DIR", claudeDir)
	if err := installRTKAssistantIntegrations(); err != nil {
		t.Fatalf("installRTKAssistantIntegrations() error = %v", err)
	}
	codexAfter, err := os.ReadFile(codexFile)
	if err != nil {
		t.Fatalf("read codex: %v", err)
	}
	if bytes.Equal(codexAfter, codexBefore) {
		t.Fatal("Codex file was not updated")
	}
	if !hasMatchingRTKCommandBlock(codexAfter, rtkCodexAgentInstructions) {
		t.Fatal("Codex prompt was not installed")
	}
	claudeAfter, err := os.ReadFile(claudeFile)
	if err != nil {
		t.Fatalf("read claude: %v", err)
	}
	if !hasMatchingRTKCommandBlock(claudeAfter, rtkClaudeAgentInstructions) {
		t.Fatalf("Claude prompt was not installed: %q", claudeAfter)
	}
	if !queryRTKClaudePromptConfigured() {
		t.Fatal("Claude prompt was not reported as configured")
	}
	if queryRTKClaudeConfigured() {
		t.Fatal("Claude was reported as fully configured without its Hook")
	}
}

func TestInstallAssistantIntegrationsDoesNotMigrateLegacyCodexReference(t *testing.T) {
	directory := t.TempDir()
	codexDir := filepath.Join(directory, "codex")
	claudeDir := filepath.Join(directory, "claude")
	if err := os.MkdirAll(codexDir, 0o755); err != nil {
		t.Fatalf("mkdir codex: %v", err)
	}
	if err := os.MkdirAll(claudeDir, 0o755); err != nil {
		t.Fatalf("mkdir claude: %v", err)
	}
	t.Setenv("CODEX_HOME", codexDir)
	t.Setenv("CLAUDE_CONFIG_DIR", claudeDir)
	rtkFile := filepath.Join(codexDir, "RTK.md")
	agentsBefore := []byte("用户内容\n@" + rtkFile + "\n")
	if err := os.WriteFile(filepath.Join(codexDir, "AGENTS.md"), agentsBefore, 0o644); err != nil {
		t.Fatalf("write agents: %v", err)
	}
	if err := os.WriteFile(filepath.Join(claudeDir, "CLAUDE.md"), []byte("Claude 用户内容\n"), 0o644); err != nil {
		t.Fatalf("write claude: %v", err)
	}
	if err := os.WriteFile(rtkFile, []byte("# RTK - Rust Token Killer (Codex CLI)\n旧版内容\n"), 0o644); err != nil {
		t.Fatalf("write legacy RTK: %v", err)
	}
	if err := installRTKAssistantIntegrations(); err != nil {
		t.Fatalf("installRTKAssistantIntegrations() error = %v", err)
	}
	agentsAfter, err := os.ReadFile(filepath.Join(codexDir, "AGENTS.md"))
	if err != nil {
		t.Fatalf("read agents: %v", err)
	}
	if !hasMatchingRTKCommandBlock(agentsAfter, rtkCodexAgentInstructions) || !strings.Contains(string(agentsAfter), "@"+rtkFile) {
		t.Fatalf("legacy Codex reference or resident instructions changed unexpectedly: %q", agentsAfter)
	}
	legacyAfter, err := os.ReadFile(rtkFile)
	if err != nil {
		t.Fatalf("read legacy RTK file: %v", err)
	}
	if string(legacyAfter) != "# RTK - Rust Token Killer (Codex CLI)\n旧版内容\n" {
		t.Fatalf("legacy RTK file changed unexpectedly: %q", legacyAfter)
	}
}

func TestInstallAssistantIntegrationsSkipsMissingCodexAgentsButAddsClaudePrompt(t *testing.T) {
	directory := t.TempDir()
	codexDir := filepath.Join(directory, "codex")
	claudeDir := filepath.Join(directory, "claude")
	if err := os.MkdirAll(codexDir, 0o755); err != nil {
		t.Fatalf("mkdir codex: %v", err)
	}
	if err := os.MkdirAll(claudeDir, 0o755); err != nil {
		t.Fatalf("mkdir claude: %v", err)
	}
	t.Setenv("CODEX_HOME", codexDir)
	t.Setenv("CLAUDE_CONFIG_DIR", claudeDir)
	rtkFile := filepath.Join(codexDir, "RTK.md")
	legacy := []byte("# RTK - Rust Token Killer (Codex CLI)\n旧版内容\n")
	if err := os.WriteFile(rtkFile, legacy, 0o644); err != nil {
		t.Fatalf("write legacy RTK: %v", err)
	}
	if err := installRTKAssistantIntegrations(); err != nil {
		t.Fatalf("installRTKAssistantIntegrations() error = %v", err)
	}
	data, err := os.ReadFile(rtkFile)
	if err != nil {
		t.Fatalf("read legacy RTK: %v", err)
	}
	if !bytes.Equal(data, legacy) {
		t.Fatalf("legacy RTK file changed without AGENTS.md: %q", data)
	}
	if _, err := os.Stat(filepath.Join(codexDir, "AGENTS.md")); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("AGENTS.md was unexpectedly created: %v", err)
	}
	claudePrompt, err := os.ReadFile(filepath.Join(claudeDir, "CLAUDE.md"))
	if err != nil {
		t.Fatalf("read Claude prompt: %v", err)
	}
	if !hasMatchingRTKCommandBlock(claudePrompt, rtkClaudeAgentInstructions) {
		t.Fatalf("Claude prompt was not created: %q", claudePrompt)
	}
}

func TestInstallAssistantIntegrationsPreservesExistingBlocks(t *testing.T) {
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
	codexBefore := []byte("用户内容\n" + rtkCommandBlockStart + "\n第三方内容\n" + rtkCommandBlockEnd + "\n")
	claudeBefore := []byte("Claude 内容\n" + rtkCommandBlockStart + "\n第三方内容\n" + rtkCommandBlockEnd + "\n")
	if err := os.WriteFile(filepath.Join(codexDir, "AGENTS.md"), codexBefore, 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(claudeDir, "CLAUDE.md"), claudeBefore, 0o644); err != nil {
		t.Fatal(err)
	}
	if err := installRTKAssistantIntegrations(); err != nil {
		t.Fatal(err)
	}
	for _, item := range []struct {
		path string
		want []byte
	}{
		{filepath.Join(codexDir, "AGENTS.md"), codexBefore},
		{filepath.Join(claudeDir, "CLAUDE.md"), claudeBefore},
	} {
		actual, err := os.ReadFile(item.path)
		if err != nil {
			t.Fatal(err)
		}
		if string(actual) != string(item.want) {
			t.Fatalf("%s changed unexpectedly: %q", item.path, actual)
		}
	}
}

func TestInstallAssistantIntegrationsMigratesKnownLegacyCodexBlock(t *testing.T) {
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
	legacy := []byte("用户内容\n" + rtkCommandBlockStart + "\n" + string(rtkCodexCommands) + rtkCommandBlockEnd + "\n")
	agentsFile := filepath.Join(codexDir, "AGENTS.md")
	if err := os.WriteFile(agentsFile, legacy, 0o644); err != nil {
		t.Fatal(err)
	}
	if err := installRTKAssistantIntegrations(); err != nil {
		t.Fatalf("installRTKAssistantIntegrations() error = %v", err)
	}
	after, err := os.ReadFile(agentsFile)
	if err != nil {
		t.Fatal(err)
	}
	if bytes.Equal(after, legacy) {
		t.Fatalf("known legacy Codex block was not migrated: %q", after)
	}
	if !hasMatchingRTKCommandBlock(after, rtkCodexAgentInstructions) {
		t.Fatalf("migrated Codex block is not the current resident instruction: %q", after)
	}
	if strings.Contains(string(after), string(rtkCodexCommands)) {
		t.Fatalf("legacy full Codex payload remained after migration: %q", after)
	}
}

func TestInstallAssistantIntegrationsDoesNotClaimExistingCurrentCodexBlock(t *testing.T) {
	directory := t.TempDir()
	codexDir := filepath.Join(directory, "codex")
	if err := os.MkdirAll(codexDir, 0o755); err != nil {
		t.Fatal(err)
	}
	t.Setenv("CODEX_HOME", codexDir)
	t.Setenv("CLAUDE_CONFIG_DIR", filepath.Join(directory, "missing-claude"))
	agentsFile := filepath.Join(codexDir, "AGENTS.md")
	original := appendRTKCommandBlock([]byte("用户内容\n"), rtkCodexAgentInstructions)
	if err := os.WriteFile(agentsFile, original, 0o644); err != nil {
		t.Fatal(err)
	}
	result, err := installRTKAssistantIntegrationsWithOwnership()
	if err != nil {
		t.Fatal(err)
	}
	if _, owned := result.Artifacts[rtkArtifactCodexPrompt]; owned {
		t.Fatal("existing current Codex prompt was incorrectly claimed")
	}
	after, err := os.ReadFile(agentsFile)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(after, original) {
		t.Fatalf("existing current Codex prompt changed: %q", after)
	}
}

func TestInstallAssistantIntegrationsReportsUnmanagedMarkerWithoutClaimingIt(t *testing.T) {
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
	codexFile := filepath.Join(codexDir, "AGENTS.md")
	original := []byte("用户内容\n" + rtkCommandBlockStart + "\n第三方内容\n" + rtkCommandBlockEnd + "\n")
	if err := os.WriteFile(codexFile, original, 0o644); err != nil {
		t.Fatal(err)
	}
	result, err := installRTKAssistantIntegrationsWithOwnership()
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Unmanaged) != 1 || result.Unmanaged[0] != string(assistantCodex) {
		t.Fatalf("unmanaged targets = %#v", result.Unmanaged)
	}
	if _, owned := result.Artifacts[rtkArtifactCodexPrompt]; owned {
		t.Fatal("unmanaged Codex marker was incorrectly claimed")
	}
	after, err := os.ReadFile(codexFile)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(after, original) {
		t.Fatalf("unmanaged Codex marker changed: %q", after)
	}
}

func TestRTKCommandBlockPreservesUTF8BOM(t *testing.T) {
	data := []byte("\ufeff用户内容\n" + rtkCommandBlockStart + "\n旧内容\n" + rtkCommandBlockEnd + "\n")
	base, count, err := stripRTKCommandBlocks(data)
	if err != nil {
		t.Fatalf("stripRTKCommandBlocks() error = %v", err)
	}
	if count != 1 {
		t.Fatalf("block count = %d, want 1", count)
	}
	if want := "\ufeff用户内容\n"; string(base) != want {
		t.Fatalf("base = %q, want %q", base, want)
	}
	updated := appendRTKCommandBlock(base, []byte("新内容\n"))
	if !bytes.HasPrefix(updated, []byte{0xef, 0xbb, 0xbf}) {
		t.Fatal("updated file lost UTF-8 BOM")
	}
}

func TestRemoveAssistantIntegrationsOnlyRemovesResidentInstructions(t *testing.T) {
	directory := t.TempDir()
	codexDir := filepath.Join(directory, "codex")
	claudeDir := filepath.Join(directory, "claude")
	if err := os.MkdirAll(codexDir, 0o755); err != nil {
		t.Fatalf("mkdir codex: %v", err)
	}
	if err := os.MkdirAll(claudeDir, 0o755); err != nil {
		t.Fatalf("mkdir claude: %v", err)
	}
	t.Setenv("CODEX_HOME", codexDir)
	t.Setenv("CLAUDE_CONFIG_DIR", claudeDir)
	rtkFile := filepath.Join(codexDir, "RTK.md")
	fullBlock := rtkCommandBlockStart + "\n" + string(rtkCodexCommands) + rtkCommandBlockEnd + "\n"
	agentsContent := "用户内容\n@" + rtkFile + "\n" + fullBlock + rtkCommandBlockStart + "\n" + string(rtkCodexAgentInstructions) + rtkCommandBlockEnd + "\n"
	claudeContent := "Claude 用户内容\n" + rtkCommandBlockStart + "\n" + string(rtkCodexAgentInstructions) + rtkCommandBlockEnd + "\n"
	if err := os.WriteFile(filepath.Join(codexDir, "AGENTS.md"), []byte(agentsContent), 0o644); err != nil {
		t.Fatalf("write agents: %v", err)
	}
	if err := os.WriteFile(filepath.Join(claudeDir, "CLAUDE.md"), []byte(claudeContent), 0o644); err != nil {
		t.Fatalf("write claude: %v", err)
	}
	if err := os.WriteFile(rtkFile, []byte("# RTK - Rust Token Killer (Codex CLI)\n旧版内容\n"), 0o644); err != nil {
		t.Fatalf("write legacy RTK: %v", err)
	}
	if err := removeRTKCodexIntegration(); err != nil {
		t.Fatalf("removeRTKCodexIntegration() error = %v", err)
	}
	agentsAfter, err := os.ReadFile(filepath.Join(codexDir, "AGENTS.md"))
	if err != nil {
		t.Fatalf("read agents: %v", err)
	}
	if string(agentsAfter) != "用户内容\n@"+rtkFile+"\n"+fullBlock {
		t.Fatalf("agents after cleanup = %q", agentsAfter)
	}
	claudeAfter, err := os.ReadFile(filepath.Join(claudeDir, "CLAUDE.md"))
	if err != nil {
		t.Fatalf("read claude: %v", err)
	}
	if string(claudeAfter) != claudeContent {
		t.Fatalf("Claude Markdown was changed by Codex cleanup: %q", claudeAfter)
	}
	legacyAfter, err := os.ReadFile(rtkFile)
	if err != nil {
		t.Fatalf("read legacy RTK file: %v", err)
	}
	if string(legacyAfter) != "# RTK - Rust Token Killer (Codex CLI)\n旧版内容\n" {
		t.Fatalf("legacy RTK file changed unexpectedly: %q", legacyAfter)
	}
}

func TestRemoveAssistantIntegrationsRemovesOnlyOwnedClaudePrompt(t *testing.T) {
	directory := t.TempDir()
	claudeDir := filepath.Join(directory, "claude")
	if err := os.MkdirAll(claudeDir, 0o755); err != nil {
		t.Fatal(err)
	}
	t.Setenv("CLAUDE_CONFIG_DIR", claudeDir)
	claudeFile := filepath.Join(claudeDir, "CLAUDE.md")
	thirdPartyBlock := rtkCommandBlockStart + "\n第三方内容\n" + rtkCommandBlockEnd + "\n"
	content := "Claude 用户内容\n" + thirdPartyBlock + rtkCommandBlockStart + "\n" + string(rtkClaudeAgentInstructions) + rtkCommandBlockEnd + "\n"
	if err := os.WriteFile(claudeFile, []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := removeRTKAssistantIntegrationsForAgents([]string{"claude-code"}); err != nil {
		t.Fatalf("removeRTKAssistantIntegrationsForAgents() error = %v", err)
	}
	after, err := os.ReadFile(claudeFile)
	if err != nil {
		t.Fatal(err)
	}
	if string(after) != "Claude 用户内容\n"+thirdPartyBlock {
		t.Fatalf("Claude cleanup removed unrelated content: %q", after)
	}
	if queryRTKClaudePromptConfigured() {
		t.Fatal("removed Claude prompt is still reported as configured")
	}
}
