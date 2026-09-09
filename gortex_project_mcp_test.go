package main

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/BurntSushi/toml"
)

func allProjectMCPAgents() map[string]bool {
	available := map[string]bool{}
	for _, agent := range gortexProjectMCPAgents {
		available[agent] = true
	}
	return available
}

func readProjectJSONServers(t *testing.T, path string) map[string]any {
	t.Helper()
	root, err := readGortexJSONObject(path)
	if err != nil {
		t.Fatal(err)
	}
	servers, ok := root["mcpServers"].(map[string]any)
	if !ok {
		t.Fatalf("%s has no mcpServers object: %#v", path, root)
	}
	return servers
}

func TestGortexProjectMCPWritesOfficialProjectFilesAndOnlyConfirmedCWD(t *testing.T) {
	preserveGortexOwnershipForTest(t)
	project := t.TempDir()
	executable := filepath.Join(t.TempDir(), gortexExecutableName)
	if warnings := gortexRegisterProjectMCPForProject(project, executable, allProjectMCPAgents()); len(warnings) != 0 {
		t.Fatalf("project registration warnings = %v", warnings)
	}

	for _, agent := range gortexProjectMCPAgents {
		path := gortexProjectMCPConfigPath(agent, project)
		if _, err := os.Stat(path); err != nil {
			t.Fatalf("%s project config was not created at %s: %v", agent, path, err)
		}
		var entry any
		switch agent {
		case "codex":
			data, err := os.ReadFile(path)
			if err != nil {
				t.Fatal(err)
			}
			root := map[string]any{}
			if _, err := toml.Decode(string(data), &root); err != nil {
				t.Fatal(err)
			}
			servers := root["mcp_servers"].(map[string]any)
			entry = servers[gortexMCPName]
		case "opencode":
			root, err := readGortexJSONObject(path)
			if err != nil {
				t.Fatal(err)
			}
			entry = root["mcp"].(map[string]any)[gortexMCPName]
		default:
			entry = readProjectJSONServers(t, path)[gortexMCPName]
		}
		if entry == nil {
			t.Fatalf("%s project entry is missing", agent)
		}
		if gortexProjectMCPUsesCWD(agent) {
			if !gortexProjectMCPEntryCWDMatches(entry, project) {
				t.Fatalf("%s project entry has no matching cwd: %#v", agent, entry)
			}
		} else if entryMap, ok := entry.(map[string]any); ok {
			if _, exists := entryMap["cwd"]; exists {
				t.Fatalf("%s project entry contains unconfirmed cwd: %#v", agent, entryMap)
			}
		}
		if agent == "antigravity" {
			entryMap, _ := entry.(map[string]any)
			env, _ := entryMap["env"].(map[string]any)
			if ws, _ := env["ANTIGRAVITY_WORKSPACE"].(string); !strings.EqualFold(filepath.Clean(ws), filepath.Clean(project)) {
				t.Fatalf("antigravity entry missing ANTIGRAVITY_WORKSPACE: %#v", entryMap)
			}
		}
	}

	ownership, err := readGortexOwnership()
	if err != nil {
		t.Fatal(err)
	}
	if got := len(ownership.ProjectMCP); got != len(gortexProjectMCPAgents) {
		t.Fatalf("project ownership records = %d, want %d", got, len(gortexProjectMCPAgents))
	}
}

func TestGortexProjectMCPSkipsUnavailablePlatforms(t *testing.T) {
	preserveGortexOwnershipForTest(t)
	project := t.TempDir()
	available := map[string]bool{"cursor": true}
	if warnings := gortexRegisterProjectMCPForProject(project, "gortex.exe", available); len(warnings) != 0 {
		t.Fatalf("project registration warnings = %v", warnings)
	}
	for _, agent := range gortexProjectMCPAgents {
		path := gortexProjectMCPConfigPath(agent, project)
		_, err := os.Stat(path)
		if agent == "cursor" {
			if err != nil {
				t.Fatalf("available Cursor project config missing: %v", err)
			}
		} else if !os.IsNotExist(err) {
			t.Fatalf("unavailable %s project config was created at %s", agent, path)
		}
	}
}

func TestGortexProjectMCPIsIdempotentAndPreservesOtherServers(t *testing.T) {
	preserveGortexOwnershipForTest(t)
	project := t.TempDir()
	path := gortexProjectMCPConfigPath("cursor", project)
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := writeGortexJSONObject(path, map[string]any{"mcpServers": map[string]any{
		"other": map[string]any{"command": "other-mcp"},
	}}); err != nil {
		t.Fatal(err)
	}
	executable := filepath.Join(t.TempDir(), gortexExecutableName)
	available := map[string]bool{"cursor": true}
	if warnings := gortexRegisterProjectMCPForProject(project, executable, available); len(warnings) != 0 {
		t.Fatalf("first registration warnings = %v", warnings)
	}
	first, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if warnings := gortexRegisterProjectMCPForProject(project, executable, available); len(warnings) != 0 {
		t.Fatalf("second registration warnings = %v", warnings)
	}
	second, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if string(first) != string(second) {
		t.Fatalf("repeated project registration changed the file")
	}
	servers := readProjectJSONServers(t, path)
	if _, ok := servers["other"]; !ok {
		t.Fatal("other project MCP was removed")
	}
	if _, ok := servers[gortexMCPName]; !ok {
		t.Fatal("Gortex project MCP was not added")
	}
}

func TestGortexUntrackProjectMCPDoesNotRemoveGlobalMCP(t *testing.T) {
	profile := t.TempDir()
	t.Setenv("USERPROFILE", profile)
	preserveGortexOwnershipForTest(t)
	project := t.TempDir()
	executable := filepath.Join(profile, gortexExecutableName)
	if _, err := updateJSONMCPConfigOwned("cursor", executable, false); err != nil {
		t.Fatal(err)
	}
	if warnings := gortexRegisterProjectMCPForProject(project, executable, map[string]bool{"cursor": true}); len(warnings) != 0 {
		t.Fatalf("project registration warnings = %v", warnings)
	}
	if warnings := gortexRemoveProjectMCP(executable, project); len(warnings) != 0 {
		t.Fatalf("project removal warnings = %v", warnings)
	}
	projectRoot, err := readGortexJSONObject(gortexProjectMCPConfigPath("cursor", project))
	if err != nil {
		t.Fatal(err)
	}
	if servers, ok := projectRoot["mcpServers"].(map[string]any); ok {
		if _, exists := servers[gortexMCPName]; exists {
			t.Fatal("project Gortex MCP survived untrack")
		}
	}
	globalRoot, err := readGortexJSONObject(gortexConfigPath("cursor"))
	if err != nil {
		t.Fatal(err)
	}
	globalServers := globalRoot["mcpServers"].(map[string]any)
	if _, ok := globalServers[gortexMCPName]; !ok {
		t.Fatal("untrack removed the global Gortex MCP")
	}
	ownership, err := readGortexOwnership()
	if err != nil {
		t.Fatal(err)
	}
	if len(ownership.ProjectMCP) != 0 {
		t.Fatalf("project ownership survived untrack: %#v", ownership.ProjectMCP)
	}
	if len(ownership.Platforms) == 0 {
		t.Fatal("global ownership was removed by untrack")
	}
}

func TestGortexRemoveAllProjectMCPClearsFlagAndPreservesGlobalMCP(t *testing.T) {
	profile := t.TempDir()
	t.Setenv("USERPROFILE", profile)
	preserveGortexOwnershipForTest(t)
	project := t.TempDir()
	executable := filepath.Join(profile, gortexExecutableName)
	if _, err := updateJSONMCPConfigOwned("cursor", executable, false); err != nil {
		t.Fatal(err)
	}
	if warnings := gortexRegisterProjectMCPForProject(project, executable, map[string]bool{"cursor": true}); len(warnings) != 0 {
		t.Fatalf("project registration warnings = %v", warnings)
	}
	ownership, err := readGortexOwnership()
	if err != nil {
		t.Fatal(err)
	}
	ownership.ProjectMCPEnabled = true
	if err := writeGortexOwnership(ownership); err != nil {
		t.Fatal(err)
	}
	if warnings := gortexRemoveAllProjectMCP(""); len(warnings) != 0 {
		t.Fatalf("remove all project MCP warnings = %v", warnings)
	}
	projectRoot, err := readGortexJSONObject(gortexProjectMCPConfigPath("cursor", project))
	if err != nil {
		t.Fatal(err)
	}
	if servers, ok := projectRoot["mcpServers"].(map[string]any); ok {
		if _, exists := servers[gortexMCPName]; exists {
			t.Fatal("project Gortex MCP survived remove all")
		}
	}
	globalRoot, err := readGortexJSONObject(gortexConfigPath("cursor"))
	if err != nil {
		t.Fatal(err)
	}
	if _, ok := globalRoot["mcpServers"].(map[string]any)[gortexMCPName]; !ok {
		t.Fatal("remove all project MCP removed the global MCP")
	}
	latest, err := readGortexOwnership()
	if err != nil {
		t.Fatal(err)
	}
	if latest.ProjectMCPEnabled || len(latest.ProjectMCP) != 0 {
		t.Fatalf("project MCP state survived remove all: enabled=%v records=%#v", latest.ProjectMCPEnabled, latest.ProjectMCP)
	}
	if len(latest.Platforms) == 0 {
		t.Fatal("global ownership was removed by project cleanup")
	}
}

func TestGortexProjectMCPPreservesUserModifiedEntry(t *testing.T) {
	preserveGortexOwnershipForTest(t)
	project := t.TempDir()
	path := gortexProjectMCPConfigPath("antigravity", project)
	executable := filepath.Join(t.TempDir(), gortexExecutableName)
	if warnings := gortexRegisterProjectMCPForProject(project, executable, map[string]bool{"antigravity": true}); len(warnings) != 0 {
		t.Fatalf("project registration warnings = %v", warnings)
	}
	root, err := readGortexJSONObject(path)
	if err != nil {
		t.Fatal(err)
	}
	entry := root["mcpServers"].(map[string]any)[gortexMCPName].(map[string]any)
	entry["cwd"] = filepath.Join(project, "changed")
	if err := writeGortexJSONObject(path, root); err != nil {
		t.Fatal(err)
	}
	warnings := gortexRemoveProjectMCP(executable, project)
	if len(warnings) == 0 {
		t.Fatal("user-modified project MCP removal produced no warning")
	}
	servers := readProjectJSONServers(t, path)
	if _, ok := servers[gortexMCPName]; !ok {
		t.Fatal("user-modified project MCP was removed")
	}
	ownership, err := readGortexOwnership()
	if err != nil {
		t.Fatal(err)
	}
	if len(ownership.ProjectMCP) == 0 {
		t.Fatal("ownership record was discarded after refusing modified project MCP")
	}
	if !strings.Contains(strings.Join(warnings, " "), "antigravity") {
		t.Fatalf("warning did not identify the platform: %v", warnings)
	}
}

func TestGortexProjectMCPEntryDoesNotAddCWDToUnconfirmedPlatforms(t *testing.T) {
	project := filepath.Clean(t.TempDir())
	for _, agent := range []string{"antigravity", "claude", "cursor", "copilot"} {
		entry := gortexProjectMCPEntry(agent, "gortex.exe", project)
		if _, ok := entry["cwd"]; ok {
			t.Fatalf("%s entry unexpectedly contains cwd: %#v", agent, entry)
		}
	}
}

func preserveGortexProjectRegistryForTest(t *testing.T) {
	t.Helper()
	path, err := gortexProjectRegistryPath()
	if err != nil {
		t.Fatal(err)
	}
	old, oldErr := os.ReadFile(path)
	oldExists := oldErr == nil
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := writeGortexProjectRegistry(gortexProjectRegistry{}); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if oldExists {
			_ = os.MkdirAll(filepath.Dir(path), 0o700)
			_ = os.WriteFile(path, old, 0o600)
		} else {
			_ = os.Remove(path)
		}
	})
}

func setProjectMCPTestEnvironment(t *testing.T, profile string) {
	t.Helper()
	t.Setenv("USERPROFILE", profile)
	t.Setenv("PATH", "")
	t.Setenv("LOCALAPPDATA", filepath.Join(profile, "missing-local-app-data"))
	t.Setenv("ProgramFiles", filepath.Join(profile, "missing-program-files"))
	t.Setenv("CLAUDE_CONFIG_DIR", filepath.Join(profile, "claude"))
	t.Setenv("COPILOT_HOME", filepath.Join(profile, "copilot"))
	t.Setenv("XDG_CONFIG_HOME", filepath.Join(profile, "xdg"))
}

func TestGortexProjectMCPSupportsBothOrdersAndReRegistration(t *testing.T) {
	preserveGortexOwnershipForTest(t)
	preserveGortexProjectRegistryForTest(t)
	profile := t.TempDir()
	setProjectMCPTestEnvironment(t, profile)
	if err := os.MkdirAll(filepath.Join(profile, ".cursor"), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := writeGortexJSONObject(gortexConfigPath("cursor"), map[string]any{
		"mcpServers": map[string]any{
			"other": map[string]any{"command": "other-mcp"},
		},
	}); err != nil {
		t.Fatal(err)
	}
	project := t.TempDir()
	if err := writeGortexProjectRegistry(gortexProjectRegistry{Projects: []string{project}}); err != nil {
		t.Fatal(err)
	}

	// Track first, then register: the register operation discovers the tracked
	// project and creates its project-level MCP entry.
	if warnings := gortexRegisterMCP(""); len(warnings) != 0 {
		t.Fatalf("track-first registration warnings = %v", warnings)
	}
	projectPath := gortexProjectMCPConfigPath("cursor", project)
	if _, err := os.Stat(projectPath); err != nil {
		t.Fatalf("track-first project MCP was not created: %v", err)
	}

	if warnings := gortexRemoveAllProjectMCP(""); len(warnings) != 0 {
		t.Fatalf("project cleanup warnings = %v", warnings)
	}
	if _, err := os.Stat(projectPath); err != nil {
		t.Fatalf("cleanup removed the config file instead of just the entry: %v", err)
	}

	// Register first, then track: the enabled flag survives without projects,
	// and a later successful track can create the project entry.
	if err := writeGortexProjectRegistry(gortexProjectRegistry{}); err != nil {
		t.Fatal(err)
	}
	if warnings := gortexEnableAndRegisterProjectMCP("", map[string]bool{"cursor": true}); len(warnings) != 0 {
		t.Fatalf("register-first enable warnings = %v", warnings)
	}
	ownership, err := readGortexOwnership()
	if err != nil {
		t.Fatal(err)
	}
	if !ownership.ProjectMCPEnabled {
		t.Fatal("register-first flow did not persist the project MCP enabled flag")
	}
	if warnings := gortexRegisterProjectMCPForProject(project, "", map[string]bool{"cursor": true}); len(warnings) != 0 {
		t.Fatalf("track-after-register warnings = %v", warnings)
	}
	if _, err := os.Stat(projectPath); err != nil {
		t.Fatalf("track-after-register project MCP was not created: %v", err)
	}

	// Removing and registering again starts the project-level lifecycle from
	// the clean disabled state and recreates the entry for the tracked project.
	if warnings := gortexRemoveAllProjectMCP(""); len(warnings) != 0 {
		t.Fatalf("second project cleanup warnings = %v", warnings)
	}
	if err := writeGortexProjectRegistry(gortexProjectRegistry{Projects: []string{project}}); err != nil {
		t.Fatal(err)
	}
	if warnings := gortexEnableAndRegisterProjectMCP("", map[string]bool{"cursor": true}); len(warnings) != 0 {
		t.Fatalf("re-registration warnings = %v", warnings)
	}
	servers := readProjectJSONServers(t, projectPath)
	if _, ok := servers[gortexMCPName]; !ok {
		t.Fatal("project MCP was not recreated after remove and register")
	}
}
