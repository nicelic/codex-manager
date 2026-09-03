package main

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestNormalizeTrustPathHandlesTomlEscapesAndSlashes(t *testing.T) {
	got := normalizeTrustPath("C:/Users/tester\\\\.codex/hooks.json")
	if got != "c:\\users\\tester\\.codex\\hooks.json" {
		t.Fatalf("normalized path = %q", got)
	}
}

func TestDetectSnipCodexTrustUsesDefaultDirectoryAndIgnoresCodexHome(t *testing.T) {
	home := t.TempDir()
	setDefaultSnipAgentTestEnvironment(t, home)
	customCodexHome := filepath.Join(home, "other-codex-profile")
	if err := os.MkdirAll(customCodexHome, 0o755); err != nil {
		t.Fatal(err)
	}
	writeTrustedCodexSnipHook(t, filepath.Join(customCodexHome, "hooks.json"), filepath.Join(home, "other", "snip.exe"))
	t.Setenv("CODEX_HOME", customCodexHome)

	info, err := detectSnipCodexTrust()
	if err != nil {
		t.Fatal(err)
	}
	if info.Status != snipTrustNotApplicable {
		t.Fatalf("CODEX_HOME must not redirect Snip's Codex Hook lookup: status=%q", info.Status)
	}

	defaultCodexHome := filepath.Join(home, ".codex")
	if err := os.MkdirAll(defaultCodexHome, 0o755); err != nil {
		t.Fatal(err)
	}
	writeTrustedCodexSnipHook(t, filepath.Join(defaultCodexHome, "hooks.json"), filepath.Join(home, "managed", "Snip", "snip.exe"))
	info, err = detectSnipCodexTrust()
	if err != nil {
		t.Fatal(err)
	}
	if info.Status != snipTrustTrusted {
		t.Fatalf("default .codex Hook trust status = %q, want %q", info.Status, snipTrustTrusted)
	}
}

func TestCodexTrustStepsUseDirectTrustOption(t *testing.T) {
	steps := codexTrustSteps()
	if len(steps) != 4 {
		t.Fatalf("trust steps = %#v", steps)
	}
	if !strings.Contains(steps[1], "数字 2") || !strings.Contains(steps[1], "Trust all and continue") {
		t.Fatalf("direct trust step = %q", steps[1])
	}
	if !strings.Contains(steps[0], "Hooks need review") || !strings.Contains(steps[0], "不要输入") {
		t.Fatalf("trust flow should start from the PowerShell review screen: %q", steps[0])
	}
}
