package main

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func setDefaultSnipAgentTestEnvironment(t *testing.T, home string) {
	t.Helper()
	t.Setenv("HOME", home)
	t.Setenv("USERPROFILE", home)
	t.Setenv("CODEX_HOME", "")
	t.Setenv("CLAUDE_CONFIG_DIR", "")
}

func TestSnipCodexHookTrustMissingToggleUsesDefaultWithoutWrite(t *testing.T) {
	home := t.TempDir()
	codexDir := filepath.Join(home, ".codex")
	if err := os.MkdirAll(codexDir, 0o755); err != nil {
		t.Fatal(err)
	}
	configPath := filepath.Join(codexDir, "config.toml")
	before := []byte("[features]\nother = true\n")
	if err := os.WriteFile(configPath, before, 0o644); err != nil {
		t.Fatal(err)
	}
	setDefaultSnipAgentTestEnvironment(t, home)
	notice, key, original, err := snipCodexHookTrust("")
	if err != nil {
		t.Fatal(err)
	}
	if key != "" || original != nil || !strings.Contains(notice, "默认开启") {
		t.Fatalf("notice=%q key=%q original=%v", notice, key, original)
	}
	after, err := os.ReadFile(configPath)
	if err != nil {
		t.Fatal(err)
	}
	if string(after) != string(before) {
		t.Fatalf("config.toml was modified: %q", after)
	}
}

func TestSnipCodexHookTrustExplicitFalseDoesNotEnable(t *testing.T) {
	home := t.TempDir()
	codexDir := filepath.Join(home, ".codex")
	if err := os.MkdirAll(codexDir, 0o755); err != nil {
		t.Fatal(err)
	}
	configPath := filepath.Join(codexDir, "config.toml")
	before := []byte("[features]\nhooks = false\n")
	if err := os.WriteFile(configPath, before, 0o644); err != nil {
		t.Fatal(err)
	}
	setDefaultSnipAgentTestEnvironment(t, home)
	notice, key, original, err := snipCodexHookTrust("")
	if err != nil {
		t.Fatal(err)
	}
	if key != "hooks" || original != nil || !strings.Contains(notice, "明确关闭") {
		t.Fatalf("notice=%q key=%q original=%v", notice, key, original)
	}
	after, err := os.ReadFile(configPath)
	if err != nil {
		t.Fatal(err)
	}
	if string(after) != string(before) {
		t.Fatalf("config.toml was modified: %q", after)
	}
}

func TestSnipExistingHookAgentsIncludesExistingAgentDirectories(t *testing.T) {
	home := t.TempDir()
	setDefaultSnipAgentTestEnvironment(t, home)
	for _, name := range []string{".codex", ".claude", ".cursor", ".copilot"} {
		if err := os.MkdirAll(filepath.Join(home, name), 0o755); err != nil {
			t.Fatal(err)
		}
	}
	if err := os.WriteFile(filepath.Join(home, ".claude", "settings.json"), []byte("{}"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(home, ".cursor", "hooks.json"), []byte("{}"), 0o644); err != nil {
		t.Fatal(err)
	}
	agents := snipExistingHookAgents()
	if len(agents) != 4 || agents[0].Name != "codex" || agents[1].Name != "claude-code" || agents[2].Name != "cursor" || agents[3].Name != "copilot" {
		t.Fatalf("existing agents = %#v", agents)
	}
}

func TestSnipAgentDirectoriesFollowOfficialInitRules(t *testing.T) {
	home := t.TempDir()
	customClaude := filepath.Join(home, "custom-claude")
	t.Setenv("HOME", home)
	t.Setenv("USERPROFILE", home)
	t.Setenv("CODEX_HOME", filepath.Join(home, "other-codex-profile"))
	t.Setenv("CLAUDE_CONFIG_DIR", customClaude)
	want := map[string]string{
		"codex":       filepath.Join(home, ".codex", "hooks.json"),
		"claude-code": filepath.Join(customClaude, "settings.json"),
		"cursor":      filepath.Join(home, ".cursor", "hooks.json"),
		"copilot":     filepath.Join(home, ".copilot", "hooks", "snip.json"),
	}
	directories := snipAgentDirectories()
	for name, expected := range want {
		if got := snipAgentHookFile(name, directories[name]); got != expected {
			t.Fatalf("%s target = %q, want %q", name, got, expected)
		}
	}
}

func TestSnipInitArgsFollowOfficialCommands(t *testing.T) {
	want := map[string][]string{
		"codex":       {"init", "--agent", "codex"},
		"claude-code": {"init"},
		"cursor":      {"init", "--agent", "cursor"},
		"copilot":     {"init", "--agent", "copilot"},
	}
	for name, expected := range want {
		got := snipInitArgs(name)
		if strings.Join(got, " ") != strings.Join(expected, " ") {
			t.Fatalf("%s init args = %#v, want %#v", name, got, expected)
		}
	}
}

func TestSnipStoppedForInstall(t *testing.T) {
	for _, testCase := range []struct {
		name  string
		state managedToolState
		want  bool
	}{
		{name: "stopped", state: managedToolState{}, want: false},
		{name: "running", state: managedToolState{Running: true}, want: true},
		{name: "desired running", state: managedToolState{DesiredRunning: true}, want: true},
	} {
		t.Run(testCase.name, func(t *testing.T) {
			err := ensureSnipStoppedForInstall(testCase.state)
			if (err != nil) != testCase.want {
				t.Fatalf("ensureSnipStoppedForInstall(%#v) error = %v, want error=%t", testCase.state, err, testCase.want)
			}
		})
	}
}

func TestSnipHookCommandPathSupport(t *testing.T) {
	withSpaces := filepath.Join("C:", "Program Files", "code-Manager", "Snip", "snip.exe")
	if err := ensureSnipHookCommandPathSupported(withSpaces, []snipAgentState{{Name: "codex"}, {Name: "copilot"}}); err != nil {
		t.Fatalf("Codex/Copilot path with spaces should be supported: %v", err)
	}
	if err := ensureSnipHookCommandPathSupported(withSpaces, []snipAgentState{{Name: "claude-code"}}); err == nil {
		t.Fatal("Claude Code init with a space-containing path must be rejected")
	}
	withoutSpaces := filepath.Join("C:", "EXEXX", "code-Manager", "Snip", "snip.exe")
	if err := ensureSnipHookCommandPathSupported(withoutSpaces, []snipAgentState{{Name: "claude-code"}, {Name: "cursor"}}); err != nil {
		t.Fatalf("path without spaces should be supported: %v", err)
	}
}

func TestSnipAgentsNeedingInitFindsNewOrMissingHooks(t *testing.T) {
	home := t.TempDir()
	setDefaultSnipAgentTestEnvironment(t, home)
	for _, name := range []string{".codex", ".claude", ".cursor", ".copilot"} {
		if err := os.MkdirAll(filepath.Join(home, name), 0o755); err != nil {
			t.Fatal(err)
		}
	}
	if err := os.WriteFile(filepath.Join(home, ".codex", "hooks.json"), []byte("{\"hooks\":{\"PreToolUse\":[{\"hooks\":[{\"type\":\"command\",\"command\":\"snip.exe hook codex\"}]}]}}"), 0o644); err != nil {
		t.Fatal(err)
	}
	agents, err := snipAgentsNeedingInit(snipExistingHookAgents(), emptySnipOwnershipLedger(), filepath.Join(home, "Snip", "snip.exe"), false)
	if err != nil {
		t.Fatal(err)
	}
	if len(agents) != 3 || agents[0].Name != "claude-code" || agents[1].Name != "cursor" || agents[2].Name != "copilot" {
		t.Fatalf("agents needing init = %#v", agents)
	}
}

func TestChangedAndRestoreSnipAgentFilesTracksNewHook(t *testing.T) {
	directory := t.TempDir()
	hookPath := filepath.Join(directory, "hooks.json")
	before := map[string]snipAgentSnapshot{
		"codex": {Path: hookPath},
	}
	if err := os.WriteFile(hookPath, []byte("{\"hooks\":[]}"), 0o644); err != nil {
		t.Fatal(err)
	}
	changed := changedSnipAgentFiles(before)
	if len(changed) != 1 || changed[0] != "codex" {
		t.Fatalf("changed agents = %#v", changed)
	}
	if err := restoreSnipAgentSnapshots(before); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(hookPath); !os.IsNotExist(err) {
		t.Fatalf("new hook still exists, err=%v", err)
	}
}

func TestSnapshotSnipAgentFilesRejectsUnreadableTarget(t *testing.T) {
	home := t.TempDir()
	setDefaultSnipAgentTestEnvironment(t, home)
	if err := os.MkdirAll(filepath.Join(home, ".codex", "hooks.json"), 0o755); err != nil {
		t.Fatal(err)
	}
	if _, err := snapshotSnipAgentFiles(); err == nil {
		t.Fatal("directory-shaped Hook target must stop activation before init")
	}
}

func TestSnipAgentsToCleanDoesNotClaimResidualManualHook(t *testing.T) {
	home := t.TempDir()
	setDefaultSnipAgentTestEnvironment(t, home)
	codexDir := filepath.Join(home, ".codex")
	if err := os.MkdirAll(codexDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(codexDir, "hooks.json"), []byte("{\"hooks\":{\"PreToolUse\":[{\"hooks\":[{\"type\":\"command\",\"command\":\"snip hook codex\"}]}]}}"), 0o644); err != nil {
		t.Fatal(err)
	}
	result := snipAgentsToClean(managedToolState{})
	if len(result) != 0 {
		t.Fatalf("unmanaged residual hook must not be claimed for cleanup: %#v", result)
	}
}

func TestSnipAgentsToCleanUsesOnlyKnownRecordedOwnership(t *testing.T) {
	result := snipAgentsToClean(managedToolState{OwnedAgents: []string{"codex", "unknown", "codex", "copilot"}})
	if got, want := strings.Join(result, ","), "codex,copilot"; got != want {
		t.Fatalf("cleanup agents = %q, want %q", got, want)
	}
}

func TestSnipOwnershipDoesNotClaimAnotherExecutablePath(t *testing.T) {
	home := t.TempDir()
	setDefaultSnipAgentTestEnvironment(t, home)
	codexDir := filepath.Join(home, ".codex")
	if err := os.MkdirAll(codexDir, 0o755); err != nil {
		t.Fatal(err)
	}
	currentExecutable := filepath.Join(home, "Snip", "snip.exe")
	manualExecutable := filepath.Join(home, "manual", "snip.exe")
	hookPath := filepath.Join(codexDir, "hooks.json")
	if err := os.WriteFile(hookPath, codexSnipHookJSON("\""+manualExecutable+"\" hook codex"), 0o644); err != nil {
		t.Fatal(err)
	}
	locations, _, err := snipHookLocationsMatchingExecutableAt("codex", hookPath, currentExecutable)
	if err != nil {
		t.Fatal(err)
	}
	if len(locations) != 0 {
		t.Fatalf("manual path must not be reported as current ownership: %#v", locations)
	}
	if err := os.WriteFile(hookPath, codexSnipHookJSON("\""+currentExecutable+"\" hook codex"), 0o644); err != nil {
		t.Fatal(err)
	}
	locations, _, err = snipHookLocationsMatchingExecutableAt("codex", hookPath, currentExecutable)
	if err != nil {
		t.Fatal(err)
	}
	if len(locations) != 1 {
		t.Fatalf("current executable locations = %#v, want one", locations)
	}
}
