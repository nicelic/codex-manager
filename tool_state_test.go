package main

import (
	"os"
	"path/filepath"
	"testing"
)

func TestManagedToolStateRoundTrip(t *testing.T) {
	directory := t.TempDir()
	state := managedToolState{DesiredRunning: true, Running: true, UserPath: true, SystemPath: true, OwnedAgents: []string{"codex", "cursor"}}
	if err := writeManagedToolState(directory, state); err != nil {
		t.Fatalf("writeManagedToolState() error = %v", err)
	}
	actual, err := readManagedToolState(directory)
	if err != nil {
		t.Fatalf("readManagedToolState() error = %v", err)
	}
	if !actual.DesiredRunning || !actual.Running || !actual.UserPath || !actual.SystemPath || len(actual.OwnedAgents) != 2 || actual.UpdatedAt.IsZero() {
		t.Fatalf("unexpected state: %#v", actual)
	}
	if err := removeManagedToolState(directory); err != nil {
		t.Fatalf("removeManagedToolState() error = %v", err)
	}
	if _, err := os.Stat(stateFilePath(directory)); !os.IsNotExist(err) {
		t.Fatalf("state file still exists: %v", err)
	}
}

func TestReadManagedToolStateMissingAndCorrupt(t *testing.T) {
	directory := t.TempDir()
	state, err := readManagedToolState(directory)
	if err != nil || state.DesiredRunning || state.Running {
		t.Fatalf("missing state = %#v, %v", state, err)
	}
	if err := os.WriteFile(filepath.Join(directory, ".code-manager-state.json"), []byte("not-json"), 0o644); err != nil {
		t.Fatal(err)
	}
	if _, err := readManagedToolState(directory); err == nil {
		t.Fatal("corrupt state unexpectedly accepted")
	}
}

func TestWindowsPathEntriesUseExactNormalizedMatch(t *testing.T) {
	entry := `C:\EXEXX\edit\RTK-AI`
	current := `C:\Windows;C:\EXEXX\edit\RTK-AI\;C:\EXEXX\edit\RTK-AI-tools`
	removed := updateWindowsPathEntries(current, entry, false)
	if windowsPathContains(removed, entry) {
		t.Fatalf("exact entry was not removed: %q", removed)
	}
	if !windowsPathContains(removed, `C:\EXEXX\edit\RTK-AI-tools`) {
		t.Fatalf("similar user entry was removed: %q", removed)
	}
	added := updateWindowsPathEntries(removed, entry, true)
	if !windowsPathContains(added, entry) {
		t.Fatalf("entry was not added: %q", added)
	}
}

func TestManagedToolAttention(t *testing.T) {
	state := managedToolState{Metadata: map[string]string{"last_error": "清理 PATH 失败"}}
	if !managedToolStateHasAttention(state) {
		t.Fatal("attention state was not detected")
	}
}

func TestContainsString(t *testing.T) {
	values := []string{"codex", "cursor"}
	if !containsString(values, "cursor") {
		t.Fatal("containsString() did not find existing value")
	}
	if containsString(values, "copilot") {
		t.Fatal("containsString() found missing value")
	}
}
