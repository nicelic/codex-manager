package main

import (
	"os"
	"path/filepath"
	"testing"
)

func snipTestHookCommand(agent, executable string) string {
	args := "hook"
	if agent == "codex" || agent == "copilot" {
		args += " " + agent
	}
	return "\"" + executable + "\" " + args
}

func writeSnipTestHook(t *testing.T, agent, path, command string) {
	t.Helper()
	var config map[string]any
	if agent == "copilot" {
		config = map[string]any{
			"hooks": map[string]any{
				"preToolUse": []any{map[string]any{"type": "command", "bash": command}},
			},
		}
	} else {
		event, _ := snipHookEventForAgent(agent)
		config = map[string]any{
			"hooks": map[string]any{
				event: []any{map[string]any{
					"matcher": ".*",
					"hooks":   []any{map[string]any{"type": "command", "command": command}},
				}},
			},
		}
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := writeSnipHookJSON(path, config); err != nil {
		t.Fatal(err)
	}
}

func TestSnipOwnershipLedgerRemovesEachRecordedPlatformHook(t *testing.T) {
	home := t.TempDir()
	setDefaultSnipAgentTestEnvironment(t, home)
	managedExecutable := filepath.Join(home, "managed", "Snip", "snip.exe")
	manualExecutable := filepath.Join(home, "manual", "Snip", "snip.exe")
	ledger := emptySnipOwnershipLedger()

	for _, spec := range snipAgentSpecs {
		directory := snipAgentDirectories()[spec.Name]
		path := snipAgentHookFile(spec.Name, directory)
		if spec.Name == "copilot" {
			config := map[string]any{
				"hooks": map[string]any{
					"preToolUse": []any{
						map[string]any{"type": "command", "bash": snipTestHookCommand(spec.Name, manualExecutable)},
						map[string]any{"type": "command", "bash": snipTestHookCommand(spec.Name, managedExecutable)},
					},
				},
			}
			if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
				t.Fatal(err)
			}
			if err := writeSnipHookJSON(path, config); err != nil {
				t.Fatal(err)
			}
		} else {
			event, _ := snipHookEventForAgent(spec.Name)
			config := map[string]any{
				"hooks": map[string]any{
					event: []any{map[string]any{
						"matcher": ".*",
						"hooks": []any{
							map[string]any{"type": "command", "command": snipTestHookCommand(spec.Name, manualExecutable)},
							map[string]any{"type": "command", "command": snipTestHookCommand(spec.Name, managedExecutable)},
						},
					}},
				},
			}
			if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
				t.Fatal(err)
			}
			if err := writeSnipHookJSON(path, config); err != nil {
				t.Fatal(err)
			}
		}
		locations, _, err := snipHookLocationsMatchingExecutableAt(spec.Name, path, managedExecutable)
		if err != nil {
			t.Fatal(err)
		}
		if len(locations) != 1 {
			t.Fatalf("%s managed locations = %#v", spec.Name, locations)
		}
		ledger.Artifacts[snipArtifactForAgent(spec.Name)] = snipHookOwnershipFromLocation(locations[0])
	}

	if err := removeSnipOwnedArtifacts(ledger); err != nil {
		t.Fatal(err)
	}
	for _, spec := range snipAgentSpecs {
		path := snipAgentHookFile(spec.Name, snipAgentDirectories()[spec.Name])
		managed, _, err := snipHookLocationsMatchingExecutableAt(spec.Name, path, managedExecutable)
		if err != nil {
			t.Fatal(err)
		}
		if len(managed) != 0 {
			t.Fatalf("%s recorded Hook remains: %#v", spec.Name, managed)
		}
		manual, _, err := snipHookLocationsMatchingExecutableAt(spec.Name, path, manualExecutable)
		if err != nil {
			t.Fatal(err)
		}
		if len(manual) != 1 {
			t.Fatalf("%s manual Hook was removed: %#v", spec.Name, manual)
		}
	}
}

func TestSnipOwnershipLedgerSupportsMultipleTargetsForOneAgent(t *testing.T) {
	directory := t.TempDir()
	executable := filepath.Join(directory, "managed", "Snip", "snip.exe")
	oldPath := filepath.Join(directory, "old-claude", "settings.json")
	newPath := filepath.Join(directory, "new-claude", "settings.json")
	command := snipTestHookCommand("claude-code", executable)
	writeSnipTestHook(t, "claude-code", oldPath, command)
	writeSnipTestHook(t, "claude-code", newPath, command)
	ledger := emptySnipOwnershipLedger()
	for _, path := range []string{oldPath, newPath} {
		locations, _, err := snipHookLocationsMatchingExecutableAt("claude-code", path, executable)
		if err != nil {
			t.Fatal(err)
		}
		if len(locations) != 1 {
			t.Fatalf("locations for %s = %#v", path, locations)
		}
		artifact := snipHookOwnershipFromLocation(locations[0])
		ledger.Artifacts[snipArtifactKeyForOwnership("claude-code", artifact.TargetPath)] = artifact
	}
	state := managedToolState{}
	if err := writeSnipOwnershipLedger(&state, ledger); err != nil {
		t.Fatal(err)
	}
	parsed, err := readSnipOwnershipLedger(state)
	if err != nil {
		t.Fatal(err)
	}
	if len(parsed.Artifacts) != 2 {
		t.Fatalf("parsed artifacts = %#v", parsed.Artifacts)
	}
	if len(state.OwnedAgents) != 1 || state.OwnedAgents[0] != "claude-code" {
		t.Fatalf("owned agents = %#v", state.OwnedAgents)
	}
	for _, path := range []string{oldPath, newPath} {
		if _, _, found := snipOwnershipArtifactForTarget(ledger, "claude-code", path); !found {
			t.Fatalf("missing ownership for %s", path)
		}
	}
	if err := removeSnipOwnedArtifacts(ledger); err != nil {
		t.Fatal(err)
	}
	for _, path := range []string{oldPath, newPath} {
		locations, _, err := snipHookLocationsMatchingExecutableAt("claude-code", path, executable)
		if err != nil {
			t.Fatal(err)
		}
		if len(locations) != 0 {
			t.Fatalf("recorded Hook remains in %s: %#v", path, locations)
		}
	}
}

func TestSnipAgentsNeedingInitUsesCurrentTargetInsteadOfOldAgentRecord(t *testing.T) {
	home := t.TempDir()
	oldClaude := filepath.Join(home, "old-claude")
	newClaude := filepath.Join(home, "new-claude")
	t.Setenv("HOME", home)
	t.Setenv("USERPROFILE", home)
	t.Setenv("CODEX_HOME", "")
	t.Setenv("CLAUDE_CONFIG_DIR", newClaude)
	if err := os.MkdirAll(newClaude, 0o755); err != nil {
		t.Fatal(err)
	}
	executable := filepath.Join(home, "managed", "Snip", "snip.exe")
	oldPath := filepath.Join(oldClaude, "settings.json")
	writeSnipTestHook(t, "claude-code", oldPath, snipTestHookCommand("claude-code", executable))
	locations, _, err := snipHookLocationsMatchingExecutableAt("claude-code", oldPath, executable)
	if err != nil {
		t.Fatal(err)
	}
	ledger := emptySnipOwnershipLedger()
	artifact := snipHookOwnershipFromLocation(locations[0])
	ledger.Artifacts[snipArtifactKeyForOwnership("claude-code", artifact.TargetPath)] = artifact
	agents, err := snipAgentsNeedingInit(snipExistingHookAgents(), ledger, executable, true)
	if err != nil {
		t.Fatal(err)
	}
	if len(agents) != 1 || agents[0].Name != "claude-code" || agents[0].TargetFile != filepath.Join(newClaude, "settings.json") {
		t.Fatalf("agents needing init = %#v", agents)
	}
}

func TestSnipOwnershipTreatsOldExecutableAsRepairNeeded(t *testing.T) {
	home := t.TempDir()
	setDefaultSnipAgentTestEnvironment(t, home)
	oldExecutable := filepath.Join(home, "old-release", "Snip", "snip.exe")
	currentExecutable := filepath.Join(home, "current-release", "Snip", "snip.exe")
	path := filepath.Join(home, ".codex", "hooks.json")
	writeSnipTestHook(t, "codex", path, snipTestHookCommand("codex", oldExecutable))
	locations, _, err := snipHookLocationsMatchingExecutableAt("codex", path, oldExecutable)
	if err != nil {
		t.Fatal(err)
	}
	ledger := emptySnipOwnershipLedger()
	artifact := snipHookOwnershipFromLocation(locations[0])
	ledger.Artifacts[snipArtifactKeyForOwnership("codex", artifact.TargetPath)] = artifact
	repair, err := snipOwnedAgentsNeedingRepair(ledger, currentExecutable)
	if err != nil {
		t.Fatal(err)
	}
	if len(repair) != 1 || repair[0] != "codex" {
		t.Fatalf("repair agents = %#v", repair)
	}
	if _, err := snipAgentsNeedingInit(snipExistingHookAgents(), ledger, currentExecutable, true); err == nil {
		t.Fatal("old-release Hook must not be overwritten by current activation")
	}
}

func TestSnipOwnershipRemovesOnlyRecordedDuplicateHandler(t *testing.T) {
	home := t.TempDir()
	setDefaultSnipAgentTestEnvironment(t, home)
	executable := filepath.Join(home, "managed", "Snip", "snip.exe")
	path := filepath.Join(home, ".codex", "hooks.json")
	command := snipTestHookCommand("codex", executable)
	config := map[string]any{
		"hooks": map[string]any{
			"PreToolUse": []any{map[string]any{
				"matcher": ".*",
				"hooks": []any{
					map[string]any{"type": "command", "command": command},
					map[string]any{"type": "command", "command": command},
				},
			}},
		},
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := writeSnipHookJSON(path, config); err != nil {
		t.Fatal(err)
	}
	locations, _, err := snipHookLocationsMatchingExecutableAt("codex", path, executable)
	if err != nil {
		t.Fatal(err)
	}
	if len(locations) != 2 || locations[1].HandlerIndex != 1 {
		t.Fatalf("locations = %#v", locations)
	}
	artifact := snipHookOwnershipFromLocation(locations[1])
	if _, err := removeSnipOwnedHook(artifact); err != nil {
		t.Fatal(err)
	}
	remaining, _, err := snipHookLocationsMatchingExecutableAt("codex", path, executable)
	if err != nil {
		t.Fatal(err)
	}
	if len(remaining) != 1 || remaining[0].HandlerIndex != 0 {
		t.Fatalf("duplicate cleanup removed the wrong handler: %#v", remaining)
	}
}

func TestSnipOwnershipLeavesMovedHookForManualResolution(t *testing.T) {
	home := t.TempDir()
	setDefaultSnipAgentTestEnvironment(t, home)
	executable := filepath.Join(home, "managed", "Snip", "snip.exe")
	path := filepath.Join(home, ".codex", "hooks.json")
	writeSnipTestHook(t, "codex", path, snipTestHookCommand("codex", executable))
	locations, _, err := snipHookLocationsMatchingExecutableAt("codex", path, executable)
	if err != nil {
		t.Fatal(err)
	}
	artifact := snipHookOwnershipFromLocation(locations[0])
	config, _, err := readSnipHookJSON(path)
	if err != nil {
		t.Fatal(err)
	}
	hooks := config["hooks"].(map[string]any)
	groups := hooks["PreToolUse"].([]any)
	hooks["PreToolUse"] = append([]any{map[string]any{"matcher": "Bash", "hooks": []any{map[string]any{"type": "command", "command": "user-hook"}}}}, groups...)
	if err := writeSnipHookJSON(path, config); err != nil {
		t.Fatal(err)
	}
	if _, err := removeSnipOwnedHook(artifact); err == nil {
		t.Fatal("moved Hook must require manual resolution")
	}
	remaining, _, err := snipHookLocationsMatchingExecutableAt("codex", path, executable)
	if err != nil {
		t.Fatal(err)
	}
	if len(remaining) != 1 {
		t.Fatalf("moved Hook was removed: %#v", remaining)
	}
}

func TestSnipOwnershipLeavesHookWhenGroupContextWasModified(t *testing.T) {
	home := t.TempDir()
	setDefaultSnipAgentTestEnvironment(t, home)
	executable := filepath.Join(home, "managed", "Snip", "snip.exe")
	path := filepath.Join(home, ".codex", "hooks.json")
	writeSnipTestHook(t, "codex", path, snipTestHookCommand("codex", executable))
	locations, _, err := snipHookLocationsMatchingExecutableAt("codex", path, executable)
	if err != nil {
		t.Fatal(err)
	}
	artifact := snipHookOwnershipFromLocation(locations[0])
	config, _, err := readSnipHookJSON(path)
	if err != nil {
		t.Fatal(err)
	}
	group := config["hooks"].(map[string]any)["PreToolUse"].([]any)[0].(map[string]any)
	group["matcher"] = "Bash"
	if err := writeSnipHookJSON(path, config); err != nil {
		t.Fatal(err)
	}
	if _, err := removeSnipOwnedHook(artifact); err == nil {
		t.Fatal("modified Hook group must require manual resolution")
	}
	remaining, _, err := snipHookLocationsMatchingExecutableAt("codex", path, executable)
	if err != nil {
		t.Fatal(err)
	}
	if len(remaining) != 1 {
		t.Fatalf("Hook in a modified group was removed: %#v", remaining)
	}
}

func TestSnipOwnershipAddedByInitUsesNewDuplicatePosition(t *testing.T) {
	home := t.TempDir()
	setDefaultSnipAgentTestEnvironment(t, home)
	executable := filepath.Join(home, "managed", "Snip", "snip.exe")
	path := filepath.Join(home, ".codex", "hooks.json")
	command := snipTestHookCommand("codex", executable)
	writeSnipTestHook(t, "codex", path, command)
	beforeData, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	config, _, err := readSnipHookJSON(path)
	if err != nil {
		t.Fatal(err)
	}
	groups := config["hooks"].(map[string]any)["PreToolUse"].([]any)
	group := groups[0].(map[string]any)
	group["hooks"] = append(group["hooks"].([]any), map[string]any{"type": "command", "command": command})
	if err := writeSnipHookJSON(path, config); err != nil {
		t.Fatal(err)
	}
	artifact, err := snipHookOwnershipAddedByInit("codex", snipAgentSnapshot{Path: path, Exists: true, Data: beforeData}, executable)
	if err != nil {
		t.Fatal(err)
	}
	if artifact.GroupIndex != 0 || artifact.HandlerIndex != 1 {
		t.Fatalf("added artifact location = %#v", artifact)
	}
	if _, err := removeSnipOwnedHook(artifact); err != nil {
		t.Fatal(err)
	}
	remaining, _, err := snipHookLocationsMatchingExecutableAt("codex", path, executable)
	if err != nil {
		t.Fatal(err)
	}
	if len(remaining) != 1 || remaining[0].HandlerIndex != 0 {
		t.Fatalf("new duplicate cleanup did not preserve original handler: %#v", remaining)
	}
}

func TestRecoverLegacySnipOwnershipRequiresOneExactHandler(t *testing.T) {
	home := t.TempDir()
	setDefaultSnipAgentTestEnvironment(t, home)
	executable := filepath.Join(home, "managed", "Snip", "snip.exe")
	path := filepath.Join(home, ".codex", "hooks.json")
	writeSnipTestHook(t, "codex", path, snipTestHookCommand("codex", executable))
	ledger, err := recoverLegacySnipOwnership(managedToolState{OwnedAgents: []string{"codex"}}, executable)
	if err != nil {
		t.Fatal(err)
	}
	if _, ok := ledger.Artifacts[snipArtifactCodexHook]; !ok {
		t.Fatal("single legacy Hook was not recovered")
	}

	config, _, err := readSnipHookJSON(path)
	if err != nil {
		t.Fatal(err)
	}
	group := config["hooks"].(map[string]any)["PreToolUse"].([]any)[0].(map[string]any)
	command := snipTestHookCommand("codex", executable)
	group["hooks"] = append(group["hooks"].([]any), map[string]any{"type": "command", "command": command})
	if err := writeSnipHookJSON(path, config); err != nil {
		t.Fatal(err)
	}
	ledger, err = recoverLegacySnipOwnership(managedToolState{OwnedAgents: []string{"codex"}}, executable)
	if err != nil {
		t.Fatal(err)
	}
	if len(ledger.Artifacts) != 0 {
		t.Fatalf("ambiguous legacy handlers must remain unclaimed: %#v", ledger.Artifacts)
	}
}

func TestSnipOwnershipDoesNotReportUnownedCurrentHookAsResidual(t *testing.T) {
	home := t.TempDir()
	setDefaultSnipAgentTestEnvironment(t, home)
	executable := filepath.Join(home, "managed", "Snip", "snip.exe")
	path := filepath.Join(home, ".codex", "hooks.json")
	writeSnipTestHook(t, "codex", path, snipTestHookCommand("codex", executable))
	residual, err := snipActivationArtifactsPresent(managedToolState{}, emptySnipOwnershipLedger())
	if err != nil {
		t.Fatal(err)
	}
	if residual {
		t.Fatal("unowned Hook was reported as a snip residual")
	}
	locations, _, err := snipHookLocationsMatchingExecutableAt("codex", path, executable)
	if err != nil {
		t.Fatal(err)
	}
	ledger := emptySnipOwnershipLedger()
	ledger.Artifacts[snipArtifactCodexHook] = snipHookOwnershipFromLocation(locations[0])
	residual, err = snipActivationArtifactsPresent(managedToolState{}, ledger)
	if err != nil {
		t.Fatal(err)
	}
	if !residual {
		t.Fatal("recorded Hook was not reported as a residual")
	}
}

func TestSnipOwnershipReportsMissingAgentDirectoryAsRepairNeeded(t *testing.T) {
	home := t.TempDir()
	setDefaultSnipAgentTestEnvironment(t, home)
	executable := filepath.Join(home, "managed", "Snip", "snip.exe")
	path := filepath.Join(home, ".codex", "hooks.json")
	writeSnipTestHook(t, "codex", path, snipTestHookCommand("codex", executable))
	locations, _, err := snipHookLocationsMatchingExecutableAt("codex", path, executable)
	if err != nil {
		t.Fatal(err)
	}
	ledger := emptySnipOwnershipLedger()
	ledger.Artifacts[snipArtifactCodexHook] = snipHookOwnershipFromLocation(locations[0])
	if err := os.RemoveAll(filepath.Join(home, ".codex")); err != nil {
		t.Fatal(err)
	}
	repair, err := snipOwnedAgentsNeedingRepair(ledger, executable)
	if err != nil {
		t.Fatal(err)
	}
	if len(repair) != 1 || repair[0] != "codex" {
		t.Fatalf("repair agents = %#v", repair)
	}
}

func TestSnipAgentsNeedingInitRejectsMalformedHookConfiguration(t *testing.T) {
	home := t.TempDir()
	setDefaultSnipAgentTestEnvironment(t, home)
	codexDir := filepath.Join(home, ".codex")
	if err := os.MkdirAll(codexDir, 0o755); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(codexDir, "hooks.json")
	if err := os.WriteFile(path, []byte(`{"hooks":"invalid"}`), 0o644); err != nil {
		t.Fatal(err)
	}
	_, err := snipAgentsNeedingInit(snipExistingHookAgents(), emptySnipOwnershipLedger(), filepath.Join(home, "Snip", "snip.exe"), false)
	if err == nil {
		t.Fatal("malformed Hook configuration must not be passed to official init")
	}
}
