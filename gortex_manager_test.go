package main

import (
	"archive/zip"
	"encoding/json"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

func TestValidateGortexProjectPathRequiresAbsoluteDirectory(t *testing.T) {
	directory := t.TempDir()
	validated, err := validateGortexProjectPath(directory)
	if err != nil {
		t.Fatalf("validateGortexProjectPath() error = %v", err)
	}
	if filepath.Clean(validated) != filepath.Clean(directory) {
		t.Fatalf("validated path = %q, want %q", validated, directory)
	}
	if _, err := validateGortexProjectPath("relative-project"); err == nil {
		t.Fatal("relative project path was accepted")
	}
	filePath := filepath.Join(directory, "file.txt")
	if err := os.WriteFile(filePath, []byte("x"), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := validateGortexProjectPath(filePath); err == nil {
		t.Fatal("file path was accepted as project directory")
	}
	missing := filepath.Join(directory, "removed-project")
	normalized, err := normalizeGortexProjectPath("  " + missing + "  ")
	if err != nil {
		t.Fatalf("normalizeGortexProjectPath() error for missing path = %v", err)
	}
	if filepath.Clean(normalized) != filepath.Clean(missing) {
		t.Fatalf("normalized missing path = %q, want %q", normalized, missing)
	}
}

func TestParseJSONBodyAcceptsSingleDocument(t *testing.T) {
	req := httptest.NewRequest("POST", "/api/gortex/install", strings.NewReader(`{"tag_name":"v0.63.10"}`))
	var input gortexInstallRequest
	if err := parseJSONBody(httptest.NewRecorder(), req, &input); err != nil {
		t.Fatalf("parseJSONBody() rejected a valid single JSON document: %v", err)
	}
	if input.TagName != "v0.63.10" {
		t.Fatalf("parsed tag_name = %q, want %q", input.TagName, "v0.63.10")
	}
}

func TestParseJSONBodyRejectsTrailingDocument(t *testing.T) {
	req := httptest.NewRequest("POST", "/api/gortex/install", strings.NewReader(`{"tag_name":"v0.63.10"}{"tag_name":"v0.63.9"}`))
	var input gortexInstallRequest
	if err := parseJSONBody(httptest.NewRecorder(), req, &input); err == nil {
		t.Fatal("parseJSONBody() accepted two concatenated JSON documents")
	}
}

func TestUpdateJSONMCPConfigPreservesOtherServers(t *testing.T) {
	profile := t.TempDir()
	t.Setenv("USERPROFILE", profile)
	cursorDirectory := filepath.Join(profile, ".cursor")
	if err := os.MkdirAll(cursorDirectory, 0o700); err != nil {
		t.Fatal(err)
	}
	configPath := filepath.Join(cursorDirectory, "mcp.json")
	original := map[string]any{"mcpServers": map[string]any{
		"other": map[string]any{"command": "other-mcp"},
	}}
	data, err := json.Marshal(original)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(configPath, data, 0o600); err != nil {
		t.Fatal(err)
	}

	if err := updateJSONMCPConfig("cursor", `C:\\Users\\test\\gortex.exe`, false); err != nil {
		t.Fatalf("register config: %v", err)
	}
	root, err := readGortexJSONObject(configPath)
	if err != nil {
		t.Fatal(err)
	}
	servers := root["mcpServers"].(map[string]any)
	if _, ok := servers["other"]; !ok {
		t.Fatal("other MCP entry was removed during registration")
	}
	if _, ok := servers[gortexMCPName]; !ok {
		t.Fatal("gortex MCP entry was not added")
	}

	if err := updateJSONMCPConfig("cursor", "", true); err != nil {
		t.Fatalf("remove config: %v", err)
	}
	root, err = readGortexJSONObject(configPath)
	if err != nil {
		t.Fatal(err)
	}
	servers = root["mcpServers"].(map[string]any)
	if _, ok := servers[gortexMCPName]; ok {
		t.Fatal("gortex MCP entry was not removed")
	}
	if _, ok := servers["other"]; !ok {
		t.Fatal("other MCP entry was removed during cleanup")
	}
}

func TestGortexManagedEnvUsesSiblingRoot(t *testing.T) {
	t.Setenv("GORTEX_RECONCILE_INTERVAL", "1h")
	root := gortexInstallRoot()
	env := gortexManagedEnv()
	joined := strings.Join(env, "\n")
	for key, suffix := range map[string]string{
		"XDG_CONFIG_HOME":           filepath.Join(root, "config"),
		"XDG_DATA_HOME":             filepath.Join(root, "data"),
		"XDG_CACHE_HOME":            filepath.Join(root, "cache"),
		"GORTEX_DAEMON_SOCKET":      filepath.Join(root, "run", "daemon.sock"),
		"GORTEX_RECONCILE_INTERVAL": "20m",
	} {
		if !strings.Contains(joined, key+"="+suffix) {
			t.Fatalf("managed env missing %s", key)
		}
	}
}

func TestGortexCommandHidesWindowsConsole(t *testing.T) {
	command := exec.Command("gortex.exe", "version")
	hideGortexCommandWindow(command)
	if command.SysProcAttr == nil || !command.SysProcAttr.HideWindow {
		t.Fatal("Gortex child command must hide the Windows console window")
	}
}

func TestGortexManagedControlPathDoesNotUsePATHFallback(t *testing.T) {
	managed := gortexManagedPath("bin", gortexExecutableName)
	if fileExists(managed) {
		t.Skip("受管 Gortex 已安装，无法在不触碰现有安装的情况下验证 PATH 回退")
	}
	directory := t.TempDir()
	external := filepath.Join(directory, gortexExecutableName)
	if err := os.WriteFile(external, []byte("external"), 0o600); err != nil {
		t.Fatal(err)
	}
	t.Setenv("PATH", directory+string(os.PathListSeparator)+os.Getenv("PATH"))
	if got := gortexManagedExecutablePath(); got != "" {
		t.Fatalf("managed control path unexpectedly used PATH executable %q", got)
	}
	if got := gortexExecutablePath(); !strings.EqualFold(filepath.Clean(got), filepath.Clean(external)) {
		t.Fatalf("detection path = %q, want external PATH executable %q", got, external)
	}
}

func TestGortexJSONOwnershipRefusesModifiedEntry(t *testing.T) {
	profile := t.TempDir()
	t.Setenv("USERPROFILE", profile)
	if err := os.MkdirAll(filepath.Dir(gortexConfigPath("cursor")), 0o700); err != nil {
		t.Fatal(err)
	}
	executable := filepath.Join(profile, "gortex.exe")
	if _, err := updateJSONMCPConfigOwned("cursor", executable, false); err != nil {
		t.Fatalf("register config: %v", err)
	}
	configPath := gortexConfigPath("cursor")
	root, err := readGortexJSONObject(configPath)
	if err != nil {
		t.Fatal(err)
	}
	servers := root["mcpServers"].(map[string]any)
	servers[gortexMCPName].(map[string]any)["command"] = "user-wrapper.exe"
	if err := writeGortexJSONObject(configPath, root); err != nil {
		t.Fatal(err)
	}
	if _, err := updateJSONMCPConfigOwned("cursor", "", true); err == nil {
		t.Fatal("modified user entry was removed")
	}
	root, err = readGortexJSONObject(configPath)
	if err != nil {
		t.Fatal(err)
	}
	if _, ok := root["mcpServers"].(map[string]any)[gortexMCPName]; !ok {
		t.Fatal("modified user entry was not preserved")
	}
}

func TestUpdateCodexMCPConfigWritesNativeTOML(t *testing.T) {
	profile := t.TempDir()
	t.Setenv("USERPROFILE", profile)
	if err := os.MkdirAll(filepath.Dir(gortexConfigPath("codex")), 0o700); err != nil {
		t.Fatal(err)
	}
	if _, err := updateCodexMCPConfig(`C:\\Tools\\gortex.exe`, false); err != nil {
		t.Fatalf("write Codex config: %v", err)
	}
	data, err := os.ReadFile(gortexConfigPath("codex"))
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(data), "[mcp_servers.gortex]") || !strings.Contains(string(data), "args = [\"mcp\"]") {
		t.Fatalf("Codex config is not native MCP TOML: %s", data)
	}
}

func TestGortexCustomAgentConfigPaths(t *testing.T) {
	profile := t.TempDir()
	claudeDir := filepath.Join(profile, "claude-profile")
	copilotDir := filepath.Join(profile, "copilot-profile")
	t.Setenv("USERPROFILE", profile)
	t.Setenv("CLAUDE_CONFIG_DIR", claudeDir)
	t.Setenv("COPILOT_HOME", copilotDir)
	if got, want := gortexConfigPath("claude"), filepath.Join(claudeDir, ".claude.json"); got != want {
		t.Fatalf("Claude config path = %q, want %q", got, want)
	}
	if got, want := gortexConfigPath("copilot"), filepath.Join(copilotDir, "mcp-config.json"); got != want {
		t.Fatalf("Copilot config path = %q, want %q", got, want)
	}
	if got, want := gortexClaudeConfigDir(), claudeDir; got != want {
		t.Fatalf("Claude config dir = %q, want %q", got, want)
	}
	if got, want := gortexCopilotConfigDir(), copilotDir; got != want {
		t.Fatalf("Copilot config dir = %q, want %q", got, want)
	}
	if got, want := gortexConfigPath("opencode"), filepath.Join(profile, ".config", "opencode", "opencode.json"); got != want {
		t.Fatalf("OpenCode config path = %q, want %q", got, want)
	}
	if got, want := gortexConfigPath("antigravity"), filepath.Join(profile, ".gemini", "config", "mcp_config.json"); got != want {
		t.Fatalf("Antigravity config path = %q, want %q", got, want)
	}
	if got, want := gortexConfigPath("gemini"), filepath.Join(profile, ".gemini", "settings.json"); got != want {
		t.Fatalf("Gemini config path = %q, want %q", got, want)
	}
}

func TestAntigravityDirectoryDoesNotImplyGeminiCLI(t *testing.T) {
	profile := t.TempDir()
	t.Setenv("USERPROFILE", profile)
	t.Setenv("PATH", "")
	if err := os.MkdirAll(gortexAntigravityConfigDir(), 0o700); err != nil {
		t.Fatal(err)
	}
	if gortexAgentAvailable("gemini") {
		t.Fatal("Antigravity configuration directory incorrectly implied Gemini CLI availability")
	}
}

func TestClaudeDirectoryDoesNotImplyClaudeCode(t *testing.T) {
	profile := t.TempDir()
	t.Setenv("USERPROFILE", profile)
	t.Setenv("CLAUDE_CONFIG_DIR", "")
	t.Setenv("PATH", "")
	if err := os.MkdirAll(gortexClaudeConfigDir(), 0o700); err != nil {
		t.Fatal(err)
	}
	if gortexAgentAvailable("claude") {
		t.Fatal("Claude configuration directory incorrectly implied Claude Code availability")
	}
}

func TestClaudeOnlyManagedMCPDoesNotImplyClaudeCode(t *testing.T) {
	profile := t.TempDir()
	t.Setenv("USERPROFILE", profile)
	t.Setenv("CLAUDE_CONFIG_DIR", "")
	t.Setenv("PATH", "")
	if err := writeGortexJSONObject(gortexConfigPath("claude"), map[string]any{
		"mcpServers": map[string]any{
			gortexMCPName: gortexMCPEntry("gortex.exe", false),
		},
	}); err != nil {
		t.Fatal(err)
	}
	if gortexAgentAvailable("claude") {
		t.Fatal("a config containing only Gortex MCP incorrectly implied Claude Code availability")
	}
}

func TestClaudeUserConfigEvidenceDetectsClaudeCode(t *testing.T) {
	profile := t.TempDir()
	t.Setenv("USERPROFILE", profile)
	t.Setenv("CLAUDE_CONFIG_DIR", "")
	t.Setenv("PATH", "")
	if err := writeGortexJSONObject(gortexConfigPath("claude"), map[string]any{
		"hasCompletedOnboarding": true,
	}); err != nil {
		t.Fatal(err)
	}
	if !gortexAgentAvailable("claude") {
		t.Fatal("Claude user configuration was not detected")
	}
}

func TestAntigravityExecutableDetectionDoesNotImplyGeminiCLI(t *testing.T) {
	profile := t.TempDir()
	localAppData := filepath.Join(profile, "AppData", "Local")
	t.Setenv("USERPROFILE", profile)
	t.Setenv("LOCALAPPDATA", localAppData)
	t.Setenv("ProgramFiles", filepath.Join(profile, "Program Files"))
	t.Setenv("PATH", "")
	executable := filepath.Join(localAppData, "Programs", "antigravity", "Antigravity.exe")
	if err := os.MkdirAll(filepath.Dir(executable), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(executable, []byte("stub"), 0o600); err != nil {
		t.Fatal(err)
	}
	if !gortexAgentAvailable("antigravity") {
		t.Fatal("installed Antigravity executable was not detected")
	}
	if gortexAgentAvailable("gemini") {
		t.Fatal("Antigravity executable incorrectly implied Gemini CLI availability")
	}
}

func TestGeminiCLIConfigDoesNotImplyAntigravity(t *testing.T) {
	profile := t.TempDir()
	t.Setenv("USERPROFILE", profile)
	t.Setenv("LOCALAPPDATA", filepath.Join(profile, "AppData", "Local"))
	t.Setenv("ProgramFiles", filepath.Join(profile, "Program Files"))
	t.Setenv("PATH", "")
	if err := os.MkdirAll(gortexGeminiConfigDir(), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(gortexConfigPath("gemini"), []byte("{\"theme\":\"Default\"}\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if !gortexAgentAvailable("gemini") {
		t.Fatal("Gemini CLI configuration was not detected")
	}
	if gortexAgentAvailable("antigravity") {
		t.Fatal("Gemini CLI configuration incorrectly implied Antigravity availability")
	}
}

func TestOpenCodeMCPConfigPreservesOtherServers(t *testing.T) {
	profile := t.TempDir()
	t.Setenv("USERPROFILE", profile)
	configPath := gortexConfigPath("opencode")
	if err := os.MkdirAll(filepath.Dir(configPath), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := writeGortexJSONObject(configPath, map[string]any{"mcp": map[string]any{"other": map[string]any{"type": "remote", "url": "https://example.test/mcp"}}}); err != nil {
		t.Fatal(err)
	}
	executable := filepath.Join(profile, "Gortex", "bin", gortexExecutableName)
	if _, err := updateOpenCodeMCPConfigOwned(executable, false); err != nil {
		t.Fatalf("register OpenCode MCP: %v", err)
	}
	root, err := readGortexJSONObject(configPath)
	if err != nil {
		t.Fatal(err)
	}
	servers := root["mcp"].(map[string]any)
	if _, ok := servers["other"]; !ok {
		t.Fatal("OpenCode other MCP was removed during registration")
	}
	if !gortexOpenCodeMCPEntryComplete(servers[gortexMCPName], executable) {
		t.Fatalf("OpenCode gortex MCP = %#v, want managed entry", servers[gortexMCPName])
	}
	if present, complete := queryOpenCodeMCPState(executable); !present || !complete {
		t.Fatalf("OpenCode MCP status present=%v complete=%v, want true,true", present, complete)
	}
	if _, err := updateOpenCodeMCPConfigOwned("", true); err != nil {
		t.Fatalf("remove OpenCode MCP: %v", err)
	}
	root, err = readGortexJSONObject(configPath)
	if err != nil {
		t.Fatal(err)
	}
	servers = root["mcp"].(map[string]any)
	if _, ok := servers[gortexMCPName]; ok {
		t.Fatal("OpenCode gortex MCP was not removed")
	}
	if _, ok := servers["other"]; !ok {
		t.Fatal("OpenCode other MCP was removed during cleanup")
	}
}

func TestAntigravityMCPUsesNativeConfigPath(t *testing.T) {
	profile := t.TempDir()
	t.Setenv("USERPROFILE", profile)
	executable := filepath.Join(profile, "Gortex", "bin", gortexExecutableName)
	if _, err := updateJSONMCPConfigOwned("antigravity", executable, false); err != nil {
		t.Fatalf("register Antigravity MCP: %v", err)
	}
	path := filepath.Join(profile, ".gemini", "config", "mcp_config.json")
	if got := gortexConfigPath("antigravity"); got != path {
		t.Fatalf("Antigravity config path = %q, want %q", got, path)
	}
	if present, complete := queryJSONMCPState("antigravity", executable); !present || !complete {
		t.Fatalf("Antigravity MCP status present=%v complete=%v, want true,true", present, complete)
	}
}

func TestGeminiMCPUsesNativeSettingsPath(t *testing.T) {
	profile := t.TempDir()
	t.Setenv("USERPROFILE", profile)
	executable := filepath.Join(profile, "Gortex", "bin", gortexExecutableName)
	if _, err := updateJSONMCPConfigOwned("gemini", executable, false); err != nil {
		t.Fatalf("register Gemini MCP: %v", err)
	}
	path := filepath.Join(profile, ".gemini", "settings.json")
	if got := gortexConfigPath("gemini"); got != path {
		t.Fatalf("Gemini config path = %q, want %q", got, path)
	}
	if present, complete := queryJSONMCPState("gemini", executable); !present || !complete {
		t.Fatalf("Gemini MCP status present=%v complete=%v, want true,true", present, complete)
	}
}

func TestLegacyAntigravityMCPMigrationRequiresOwnership(t *testing.T) {
	profile := t.TempDir()
	t.Setenv("USERPROFILE", profile)
	legacyPath := gortexLegacyAntigravityConfigPath()
	executable := filepath.Join(profile, "Gortex", "bin", gortexExecutableName)
	if err := os.MkdirAll(filepath.Dir(legacyPath), 0o700); err != nil {
		t.Fatal(err)
	}
	if _, err := updateJSONMCPConfigOwned("antigravity", executable, false); err != nil {
		t.Fatalf("register native Antigravity MCP: %v", err)
	}
	if err := writeGortexJSONObject(legacyPath, map[string]any{"mcpServers": map[string]any{gortexMCPName: gortexMCPEntry(executable, false), "other": map[string]any{"command": "other.exe"}}}); err != nil {
		t.Fatal(err)
	}
	ownership, err := readGortexOwnership()
	if err != nil {
		t.Fatal(err)
	}
	key := gortexOwnershipKey("antigravity", legacyPath)
	ownership.Platforms[key] = gortexOwnedMCP{Fingerprint: gortexFingerprint(gortexMCPEntry(executable, false))}
	if err := writeGortexOwnership(ownership); err != nil {
		t.Fatal(err)
	}
	if _, err := removeLegacyAntigravityMCP(); err != nil {
		t.Fatalf("remove legacy Antigravity MCP: %v", err)
	}
	root, err := readGortexJSONObject(legacyPath)
	if err != nil {
		t.Fatal(err)
	}
	servers := root["mcpServers"].(map[string]any)
	if _, ok := servers[gortexMCPName]; ok {
		t.Fatal("owned legacy Antigravity MCP was not removed")
	}
	if _, ok := servers["other"]; !ok {
		t.Fatal("unrelated legacy MCP was removed")
	}
}

func TestLegacyAntigravityMCPWithoutOwnershipIsPreserved(t *testing.T) {
	profile := t.TempDir()
	t.Setenv("USERPROFILE", profile)
	legacyPath := gortexLegacyAntigravityConfigPath()
	executable := filepath.Join(profile, "Gortex", "bin", gortexExecutableName)
	if err := os.MkdirAll(filepath.Dir(legacyPath), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := writeGortexJSONObject(legacyPath, map[string]any{"mcpServers": map[string]any{gortexMCPName: gortexMCPEntry(executable, false), "other": map[string]any{"command": "other.exe"}}}); err != nil {
		t.Fatal(err)
	}
	if _, err := removeLegacyAntigravityMCP(); err != nil {
		t.Fatalf("remove unowned legacy Antigravity MCP: %v", err)
	}
	root, err := readGortexJSONObject(legacyPath)
	if err != nil {
		t.Fatal(err)
	}
	servers := root["mcpServers"].(map[string]any)
	if _, ok := servers[gortexMCPName]; !ok {
		t.Fatal("unowned legacy Antigravity MCP was removed")
	}
	if _, ok := servers["other"]; !ok {
		t.Fatal("unrelated legacy MCP was removed")
	}
}

func TestGortexRegistrationRefusesModifiedOwnedEntry(t *testing.T) {
	profile := t.TempDir()
	t.Setenv("USERPROFILE", profile)
	configDir := filepath.Dir(gortexConfigPath("cursor"))
	if err := os.MkdirAll(configDir, 0o700); err != nil {
		t.Fatal(err)
	}
	executable := filepath.Join(profile, "gortex.exe")
	if _, err := updateJSONMCPConfigOwned("cursor", executable, false); err != nil {
		t.Fatalf("register config: %v", err)
	}
	configPath := gortexConfigPath("cursor")
	root, err := readGortexJSONObject(configPath)
	if err != nil {
		t.Fatal(err)
	}
	root["mcpServers"].(map[string]any)[gortexMCPName].(map[string]any)["command"] = "user-wrapper.exe"
	if err := writeGortexJSONObject(configPath, root); err != nil {
		t.Fatal(err)
	}
	if _, err := updateJSONMCPConfigOwned("cursor", executable, false); err == nil {
		t.Fatal("modified owned entry was overwritten during registration")
	}
}

func TestGortexRegistrationRepairsIncompleteManagedEntry(t *testing.T) {
	profile := t.TempDir()
	t.Setenv("USERPROFILE", profile)
	configDir := filepath.Dir(gortexConfigPath("cursor"))
	if err := os.MkdirAll(configDir, 0o700); err != nil {
		t.Fatal(err)
	}
	configPath := gortexConfigPath("cursor")
	root := map[string]any{"mcpServers": map[string]any{
		gortexMCPName: map[string]any{"command": "gortex", "args": []any{"mcp"}},
		"other":       map[string]any{"command": "other-mcp"},
	}}
	if err := writeGortexJSONObject(configPath, root); err != nil {
		t.Fatal(err)
	}
	if _, err := updateJSONMCPConfigOwned("cursor", filepath.Join(profile, "Gortex", "bin", gortexExecutableName), false); err != nil {
		t.Fatalf("repair incomplete config: %v", err)
	}
	root, err := readGortexJSONObject(configPath)
	if err != nil {
		t.Fatal(err)
	}
	entry := root["mcpServers"].(map[string]any)[gortexMCPName]
	if !gortexMCPEntryComplete(entry, filepath.Join(profile, "Gortex", "bin", gortexExecutableName), false) {
		t.Fatal("repaired entry is not complete")
	}
	if _, ok := root["mcpServers"].(map[string]any)["other"]; !ok {
		t.Fatal("repair removed unrelated MCP entry")
	}
}

func TestGortexRemoveDeletesIncompleteManagedEntryWithoutOwnership(t *testing.T) {
	profile := t.TempDir()
	t.Setenv("USERPROFILE", profile)
	configDir := filepath.Dir(gortexConfigPath("cursor"))
	if err := os.MkdirAll(configDir, 0o700); err != nil {
		t.Fatal(err)
	}
	configPath := gortexConfigPath("cursor")
	if err := writeGortexJSONObject(configPath, map[string]any{"mcpServers": map[string]any{
		gortexMCPName: map[string]any{"command": "gortex", "args": []any{"mcp", "--old"}},
		"other":       map[string]any{"command": "other-mcp"},
	}}); err != nil {
		t.Fatal(err)
	}
	if _, err := updateJSONMCPConfigOwned("cursor", "", true); err != nil {
		t.Fatalf("remove incomplete managed config: %v", err)
	}
	root, err := readGortexJSONObject(configPath)
	if err != nil {
		t.Fatal(err)
	}
	if _, ok := root["mcpServers"].(map[string]any)[gortexMCPName]; ok {
		t.Fatal("incomplete managed entry was not removed")
	}
}

func TestGortexProjectRegistryUsesManagedRoot(t *testing.T) {
	path, err := gortexProjectRegistryPath()
	if err != nil {
		t.Fatal(err)
	}
	wantRoot := filepath.Clean(gortexInstallRoot())
	if wantRoot == "" || !strings.HasPrefix(strings.ToLower(filepath.Clean(path)), strings.ToLower(wantRoot+string(filepath.Separator))) {
		t.Fatalf("project registry path = %q, want under managed root %q", path, wantRoot)
	}
}

func TestSameGortexExecutablePathNormalizesWindowsPrefix(t *testing.T) {
	if !sameGortexExecutablePath(`\\?\C:\EXEXX\code-Manager\Gortex\bin\gortex.exe`, `c:/exexx/code-manager/Gortex/bin/gortex.exe`) {
		t.Fatal("sameGortexExecutablePath() did not normalize the Windows path prefix")
	}
	if sameGortexExecutablePath(`C:\other\gortex.exe`, `C:\EXEXX\code-Manager\Gortex\bin\gortex.exe`) {
		t.Fatal("sameGortexExecutablePath() matched different executable paths")
	}
}

func TestParseGortexChecksum(t *testing.T) {
	const digest = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
	got, err := parseGortexChecksum("deadbeef  other.zip\n"+digest+"  gortex_windows_amd64.zip\n", gortexWindowsAssetName)
	if err != nil {
		t.Fatalf("parseGortexChecksum() error = %v", err)
	}
	if got != digest {
		t.Fatalf("checksum = %q, want %q", got, digest)
	}
	if _, err := parseGortexChecksum("deadbeef  gortex_windows_amd64.zip\n", gortexWindowsAssetName); err == nil {
		t.Fatal("parseGortexChecksum() accepted a line without a SHA-256 digest")
	}
}

func TestInstallGortexZIPWritesOnlyManagedBinary(t *testing.T) {
	archivePath := filepath.Join(t.TempDir(), "gortex.zip")
	file, err := os.Create(archivePath)
	if err != nil {
		t.Fatal(err)
	}
	writer := zip.NewWriter(file)
	entry, err := writer.Create("nested/gortex.exe")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := entry.Write([]byte("test-binary")); err != nil {
		t.Fatal(err)
	}
	if err := writer.Close(); err != nil {
		t.Fatal(err)
	}
	if err := file.Close(); err != nil {
		t.Fatal(err)
	}

	installDir := filepath.Join(t.TempDir(), "Gortex", "bin")
	if err := installGortexZIP(archivePath, installDir); err != nil {
		t.Fatalf("installGortexZIP() error = %v", err)
	}
	data, err := os.ReadFile(filepath.Join(installDir, gortexExecutableName))
	if err != nil {
		t.Fatal(err)
	}
	if string(data) != "test-binary" {
		t.Fatalf("installed binary = %q, want %q", data, "test-binary")
	}
}

func TestInstallGortexZIPRejectsEmptyExecutableWithoutReplacingTarget(t *testing.T) {
	archivePath := filepath.Join(t.TempDir(), "gortex-empty.zip")
	file, err := os.Create(archivePath)
	if err != nil {
		t.Fatal(err)
	}
	writer := zip.NewWriter(file)
	if _, err := writer.Create("gortex.exe"); err != nil {
		t.Fatal(err)
	}
	if err := writer.Close(); err != nil {
		t.Fatal(err)
	}
	if err := file.Close(); err != nil {
		t.Fatal(err)
	}

	installDir := filepath.Join(t.TempDir(), "Gortex", "bin")
	if err := os.MkdirAll(installDir, 0o700); err != nil {
		t.Fatal(err)
	}
	target := filepath.Join(installDir, gortexExecutableName)
	if err := os.WriteFile(target, []byte("old-binary"), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := installGortexZIP(archivePath, installDir); err == nil {
		t.Fatal("installGortexZIP() accepted an empty gortex.exe")
	}
	data, err := os.ReadFile(target)
	if err != nil {
		t.Fatal(err)
	}
	if string(data) != "old-binary" {
		t.Fatalf("old executable changed after invalid archive: %q", data)
	}
}

func TestReplaceGortexExecutableRestoresOldFileOnStagedRenameFailure(t *testing.T) {
	directory := t.TempDir()
	target := filepath.Join(directory, gortexExecutableName)
	if err := os.WriteFile(target, []byte("old-binary"), 0o700); err != nil {
		t.Fatal(err)
	}
	missing := filepath.Join(directory, "missing-staged.exe")
	if err := replaceGortexExecutable(missing, target); err == nil {
		t.Fatal("replaceGortexExecutable unexpectedly succeeded with a missing staged file")
	}
	data, err := os.ReadFile(target)
	if err != nil {
		t.Fatal(err)
	}
	if string(data) != "old-binary" {
		t.Fatalf("old executable was not restored: %q", data)
	}
}

func TestReplaceGortexExecutableReplacesTarget(t *testing.T) {
	directory := t.TempDir()
	target := filepath.Join(directory, gortexExecutableName)
	staged := filepath.Join(directory, "staged.exe")
	if err := os.WriteFile(target, []byte("old-binary"), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(staged, []byte("new-binary"), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := replaceGortexExecutable(staged, target); err != nil {
		t.Fatalf("replaceGortexExecutable() error = %v", err)
	}
	data, err := os.ReadFile(target)
	if err != nil {
		t.Fatal(err)
	}
	if string(data) != "new-binary" {
		t.Fatalf("installed executable = %q, want new-binary", data)
	}
	if _, err := os.Stat(staged); !os.IsNotExist(err) {
		t.Fatalf("staged file still exists, stat error = %v", err)
	}
}

func TestGortexRemoveMCPDoesNotRestoreRemovedPlatformLedger(t *testing.T) {
	profile := t.TempDir()
	t.Setenv("USERPROFILE", profile)
	ownershipPath := gortexOwnershipPath()
	if ownershipPath == "" {
		t.Skip("无法定位 Gortex 测试账本路径")
	}
	old, oldErr := os.ReadFile(ownershipPath)
	oldExists := oldErr == nil
	t.Cleanup(func() {
		if oldExists {
			_ = os.MkdirAll(filepath.Dir(ownershipPath), 0o700)
			_ = os.WriteFile(ownershipPath, old, 0o600)
		} else {
			_ = os.Remove(ownershipPath)
		}
	})
	_ = os.Remove(ownershipPath)

	if _, err := updateJSONMCPConfigOwned("cursor", filepath.Join(profile, "Gortex", "bin", gortexExecutableName), false); err != nil {
		t.Fatalf("register cursor MCP: %v", err)
	}
	if warnings := gortexRemoveMCP(); len(warnings) > 0 {
		t.Fatalf("gortexRemoveMCP() warnings = %v", warnings)
	}
	latest, err := readGortexOwnership()
	if err != nil {
		t.Fatal(err)
	}
	if len(latest.Platforms) != 0 {
		t.Fatalf("removed platform ownership was restored: %#v", latest.Platforms)
	}
	root, err := readGortexJSONObject(gortexConfigPath("cursor"))
	if err != nil {
		t.Fatal(err)
	}
	if servers, ok := root["mcpServers"].(map[string]any); ok {
		if _, exists := servers[gortexMCPName]; exists {
			t.Fatal("cursor Gortex MCP entry survived removal")
		}
	}
}

func preserveGortexOwnershipForTest(t *testing.T) {
	t.Helper()
	path := gortexOwnershipPath()
	if path == "" {
		t.Skip("无法定位 Gortex 测试账本路径")
	}
	old, oldErr := os.ReadFile(path)
	oldExists := oldErr == nil
	if err := os.Remove(path); err != nil && !os.IsNotExist(err) {
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

func TestGortexMCPRemovalClearsLedgerWhenJSONConfigIsMissing(t *testing.T) {
	profile := t.TempDir()
	t.Setenv("USERPROFILE", profile)
	preserveGortexOwnershipForTest(t)
	executable := filepath.Join(profile, "Gortex", "bin", gortexExecutableName)
	if _, err := updateJSONMCPConfigOwned("cursor", executable, false); err != nil {
		t.Fatal(err)
	}
	configPath := gortexConfigPath("cursor")
	if err := os.Remove(configPath); err != nil {
		t.Fatal(err)
	}
	if _, err := updateJSONMCPConfigOwned("cursor", "", true); err != nil {
		t.Fatalf("remove missing JSON config: %v", err)
	}
	ownership, err := readGortexOwnership()
	if err != nil {
		t.Fatal(err)
	}
	if len(ownership.Platforms) != 0 {
		t.Fatalf("stale JSON platform ownership survived: %#v", ownership.Platforms)
	}
}

func TestGortexMCPRemovalClearsLedgerWhenCodexEntryIsMissing(t *testing.T) {
	profile := t.TempDir()
	t.Setenv("USERPROFILE", profile)
	preserveGortexOwnershipForTest(t)
	executable := filepath.Join(profile, "Gortex", "bin", gortexExecutableName)
	if _, err := updateCodexMCPConfig(executable, false); err != nil {
		t.Fatal(err)
	}
	configPath := gortexConfigPath("codex")
	if err := os.Remove(configPath); err != nil {
		t.Fatal(err)
	}
	if _, err := updateCodexMCPConfig("", true); err != nil {
		t.Fatalf("remove missing Codex config: %v", err)
	}
	ownership, err := readGortexOwnership()
	if err != nil {
		t.Fatal(err)
	}
	if len(ownership.Platforms) != 0 {
		t.Fatalf("stale Codex platform ownership survived: %#v", ownership.Platforms)
	}
}

func TestAntigravityMCPInjectsActiveProjectCWD(t *testing.T) {
	profile := t.TempDir()
	t.Setenv("USERPROFILE", profile)
	preserveGortexOwnershipForTest(t)

	projectDir := t.TempDir()
	if err := writeGortexProjectRegistry(gortexProjectRegistry{Projects: []string{projectDir}}); err != nil {
		t.Fatal(err)
	}

	executable := filepath.Join(profile, "Gortex", "bin", gortexExecutableName)
	if _, err := updateJSONMCPConfigOwned("antigravity", executable, false); err != nil {
		t.Fatalf("register Antigravity MCP: %v", err)
	}

	path := gortexConfigPath("antigravity")
	root, err := readGortexJSONObject(path)
	if err != nil {
		t.Fatal(err)
	}
	servers, ok := root["mcpServers"].(map[string]any)
	if !ok {
		t.Fatalf("mcpServers not found in %s", path)
	}
	entry, ok := servers[gortexMCPName].(map[string]any)
	if !ok {
		t.Fatalf("%s entry not found", gortexMCPName)
	}

	cwd, ok := entry["cwd"].(string)
	if !ok || !strings.EqualFold(filepath.Clean(cwd), filepath.Clean(projectDir)) {
		t.Fatalf("Antigravity MCP cwd = %q, want %q", cwd, projectDir)
	}

	env, ok := entry["env"].(map[string]any)
	if !ok {
		t.Fatalf("Antigravity MCP env not a map: %#v", entry["env"])
	}
	if ws, ok := env["ANTIGRAVITY_WORKSPACE"].(string); !ok || !strings.EqualFold(filepath.Clean(ws), filepath.Clean(projectDir)) {
		t.Fatalf("Antigravity MCP ANTIGRAVITY_WORKSPACE = %q, want %q", ws, projectDir)
	}

	if present, complete := queryJSONMCPState("antigravity", executable); !present || !complete {
		t.Fatalf("Antigravity MCP status present=%v complete=%v, want true,true", present, complete)
	}
}

func TestCursorMCPInjectsActiveProjectCWDAndWorkspace(t *testing.T) {
	profile := t.TempDir()
	t.Setenv("USERPROFILE", profile)
	preserveGortexOwnershipForTest(t)

	projectDir := t.TempDir()
	if err := writeGortexProjectRegistry(gortexProjectRegistry{Projects: []string{projectDir}}); err != nil {
		t.Fatal(err)
	}

	executable := filepath.Join(profile, "Gortex", "bin", gortexExecutableName)
	if _, err := updateJSONMCPConfigOwned("cursor", executable, false); err != nil {
		t.Fatalf("register Cursor MCP: %v", err)
	}

	path := gortexConfigPath("cursor")
	root, err := readGortexJSONObject(path)
	if err != nil {
		t.Fatal(err)
	}
	servers, ok := root["mcpServers"].(map[string]any)
	if !ok {
		t.Fatalf("mcpServers not found in %s", path)
	}
	entry, ok := servers[gortexMCPName].(map[string]any)
	if !ok {
		t.Fatalf("%s entry not found", gortexMCPName)
	}

	cwd, ok := entry["cwd"].(string)
	if !ok || !strings.EqualFold(filepath.Clean(cwd), filepath.Clean(projectDir)) {
		t.Fatalf("Cursor MCP cwd = %q, want %q", cwd, projectDir)
	}

	env, ok := entry["env"].(map[string]any)
	if !ok {
		t.Fatalf("Cursor MCP env not a map: %#v", entry["env"])
	}
	if ws, ok := env["CURSOR_WORKSPACE"].(string); !ok || !strings.EqualFold(filepath.Clean(ws), filepath.Clean(projectDir)) {
		t.Fatalf("Cursor MCP CURSOR_WORKSPACE = %q, want %q", ws, projectDir)
	}

	if present, complete := queryJSONMCPState("cursor", executable); !present || !complete {
		t.Fatalf("Cursor MCP status present=%v complete=%v, want true,true", present, complete)
	}
}
