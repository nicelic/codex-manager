package main

import (
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"

	"github.com/BurntSushi/toml"
	"gopkg.in/yaml.v3"
)

func TestGortexCopilotHooksMigrateNestedAndStaleEntries(t *testing.T) {
	path := filepath.Join(t.TempDir(), "hooks", "gortex.json")
	oldCommand := `C:\old\Gortex\bin\gortex.exe hook --agent=copilot-cli`
	userCommand := "company-hook.exe"
	fullMatcher := "^(?:bash|create|edit|glob|grep|powershell|rg|view)$"
	root := map[string]any{
		"version":         1,
		"disableAllHooks": false,
		"hooks": map[string]any{
			"sessionStart": []any{
				map[string]any{"matcher": "", "hooks": []any{map[string]any{"type": "command", "command": oldCommand}}},
				map[string]any{"type": "command", "bash": userCommand},
			},
			"userPromptSubmitted": []any{
				map[string]any{"type": "command", "bash": oldCommand, "powershell": oldCommand},
			},
			"preToolUse": []any{
				map[string]any{"type": "command", "bash": oldCommand, "powershell": oldCommand, "matcher": fullMatcher},
				map[string]any{"type": "command", "bash": "user-pre-hook"},
			},
			"postToolUse": []any{
				map[string]any{"type": "command", "bash": "user-post-hook"},
			},
		},
	}
	data, err := json.Marshal(root)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, data, 0o600); err != nil {
		t.Fatal(err)
	}

	current := `D:\new\Gortex\bin\gortex.exe`
	changed, _, err := upsertCopilotHooks(path, current)
	if err != nil {
		t.Fatalf("upsertCopilotHooks() error = %v", err)
	}
	if !changed {
		t.Fatal("stale/nested Copilot hooks were not migrated")
	}
	parsed, err := readGortexJSONObject(path)
	if err != nil {
		t.Fatal(err)
	}
	hooks := parsed["hooks"].(map[string]any)
	for _, event := range []string{"sessionStart", "userPromptSubmitted", "preToolUse", "postToolUse"} {
		entries, ok := gortexCopilotHookList(hooks[event])
		if !ok {
			t.Fatalf("hooks.%s is not a list", event)
		}
		ours := 0
		for _, raw := range entries {
			if !gortexCopilotHookEntryIsOurs(raw, current) {
				continue
			}
			ours++
			entry := raw.(map[string]any)
			if _, nested := entry["hooks"]; nested {
				t.Fatalf("hooks.%s retained the legacy nested schema: %#v", event, entry)
			}
			if entry["type"] != "command" {
				t.Fatalf("hooks.%s type = %v, want command", event, entry["type"])
			}
			if !strings.Contains(strings.ToLower(entry["bash"].(string)), "d:/new/gortex/bin/gortex.exe") {
				t.Fatalf("hooks.%s did not migrate the stale executable path: %#v", event, entry)
			}
		}
		if ours != 1 {
			t.Fatalf("hooks.%s Gortex entry count = %d, want 1", event, ours)
		}
	}
	if !strings.Contains(string(data), userCommand) {
		t.Fatal("seed sanity check failed")
	}
	encoded, _ := os.ReadFile(path)
	if !strings.Contains(string(encoded), "user-pre-hook") || !strings.Contains(string(encoded), "user-post-hook") || !strings.Contains(string(encoded), userCommand) {
		t.Fatalf("user Copilot hooks were not preserved: %s", encoded)
	}
	changed, _, err = upsertCopilotHooks(path, current)
	if err != nil {
		t.Fatalf("second upsertCopilotHooks() error = %v", err)
	}
	if changed {
		t.Fatal("native Copilot hooks are not idempotent")
	}
}

func TestGortexCopilotHookIdentityIgnoresExecutablePath(t *testing.T) {
	current := `D:\new\Gortex\bin\gortex.exe`
	direct := map[string]any{
		"type":       "command",
		"bash":       `C:\old\Gortex\bin\gortex.exe hook --agent=copilot-cli`,
		"powershell": `& 'C:\old\Gortex\bin\gortex.exe' hook --agent=copilot-cli`,
	}
	nested := map[string]any{
		"hooks": []any{map[string]any{"type": "command", "command": `C:\old\Gortex\bin\gortex.exe hook --agent=copilot-cli`}},
	}
	if !gortexCopilotHookEntryIsOurs(direct, current) || !gortexCopilotHookEntryIsOurs(nested, current) {
		t.Fatal("stale Copilot hooks were not recognized by agent identity")
	}
	if gortexCopilotHookEntryIsOurs(map[string]any{"type": "command", "bash": "company-hook --agent=copilot-cli"}, current) {
		t.Fatal("an unrelated Copilot command was misidentified as Gortex")
	}
}

func TestEmbeddedGortexPromptDocumentLeavesMarkersToIntegration(t *testing.T) {
	text := string(gortexPromptDocument)
	if text == "" {
		t.Fatal("embedded Gortex prompt is empty")
	}
	if strings.Contains(text, gortexRulesStartMarker) || strings.Contains(text, gortexRulesEndMarker) {
		t.Fatal("embedded Gortex prompt source must not contain integration markers")
	}
	if gortexInstructionBody == "" {
		t.Fatal("embedded Gortex prompt body is empty")
	}
	block := gortexPromptBlock(gortexInstructionBody)
	if strings.Count(block, gortexRulesStartMarker) != 1 {
		t.Fatalf("generated start marker count = %d, want 1", strings.Count(block, gortexRulesStartMarker))
	}
	if strings.Count(block, gortexRulesEndMarker) != 1 {
		t.Fatalf("generated end marker count = %d, want 1", strings.Count(block, gortexRulesEndMarker))
	}
	if !strings.Contains(gortexInstructionBody, "capabilities") || !strings.Contains(gortexInstructionBody, "workspace_admin") {
		t.Fatal("embedded Gortex prompt does not contain the complete compact MCP guidance")
	}
}

func TestEmbeddedGortexInstructionBodyPreservesSourceContent(t *testing.T) {
	source := "\r\n第一行\r\n" + gortexRulesStartMarker + "\r\n最后一行\r\n"
	want := "\n第一行\n" + gortexRulesStartMarker + "\n最后一行\n"
	if actual := embeddedGortexInstructionBody([]byte(source)); actual != want {
		t.Fatalf("embeddedGortexInstructionBody() = %q, want %q", actual, want)
	}
}

func TestGortexPromptIntegrationKeepsMarkerTextInUserContent(t *testing.T) {
	path := filepath.Join(t.TempDir(), "AGENTS.md")
	body := "用户正文\n" + gortexRulesStartMarker + "\n继续正文\n" + gortexRulesEndMarker
	changed, fingerprint, err := upsertGortexPrompt(path, body)
	if err != nil || !changed {
		t.Fatalf("upsertGortexPrompt() changed = %v, err = %v", changed, err)
	}
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	block, ok := gortexMarkedBlock(data)
	if !ok || block != strings.TrimSuffix(gortexPromptBlock(body), "\n") {
		t.Fatalf("managed block did not retain complete user content:\n%s", block)
	}
	removed, err := removeGortexPrompt(path, fingerprint)
	if err != nil || !removed {
		t.Fatalf("removeGortexPrompt() removed = %v, err = %v", removed, err)
	}
	data, err = os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if string(data) != "" {
		t.Fatalf("removed prompt file = %q, want empty", string(data))
	}
}

func TestGortexPromptPathsKeepAntigravityAndGeminiIndependent(t *testing.T) {
	profile := t.TempDir()
	t.Setenv("USERPROFILE", profile)
	if got, want := gortexPromptPaths("opencode"), []string{filepath.Join(profile, ".config", "opencode", "AGENTS.md")}; !reflect.DeepEqual(got, want) {
		t.Fatalf("OpenCode prompt paths = %#v, want %#v", got, want)
	}
	wantGeminiPrompt := []string{filepath.Join(profile, ".gemini", "GEMINI.md")}
	if got := gortexPromptPaths("antigravity"); !reflect.DeepEqual(got, wantGeminiPrompt) {
		t.Fatalf("Antigravity prompt paths = %#v, want %#v", got, wantGeminiPrompt)
	}
	if got := gortexPromptPaths("gemini"); !reflect.DeepEqual(got, wantGeminiPrompt) {
		t.Fatalf("Gemini prompt paths = %#v, want %#v", got, wantGeminiPrompt)
	}
}

func TestGortexOpenCodePluginBridgeIsOwnedAndIdempotent(t *testing.T) {
	profile := t.TempDir()
	t.Setenv("USERPROFILE", profile)
	executable := filepath.Join(profile, "Gortex", "bin", gortexExecutableName)
	path := gortexOpenCodePluginPath()

	changed, artifacts, err := upsertOpenCodePlugin(path, executable)
	if err != nil || !changed || len(artifacts) != 1 {
		t.Fatalf("first OpenCode plugin write changed=%v artifacts=%d error=%v", changed, len(artifacts), err)
	}
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(data), gortexOpenCodePluginMarker) || !strings.Contains(string(data), "--agent=opencode") {
		t.Fatalf("OpenCode plugin bridge is missing its managed identity: %s", data)
	}
	if strings.Contains(string(data), gortexOpenCodePluginBinKey) {
		t.Fatal("OpenCode plugin executable placeholder was not rendered")
	}
	for _, placeholder := range []string{gortexOpenCodePluginArgvKey, gortexOpenCodePluginEnforceKey} {
		if strings.Contains(string(data), placeholder) {
			t.Fatalf("OpenCode plugin placeholder %q was not rendered", placeholder)
		}
	}
	for _, hook := range []string{"tool.execute.before", "tool.execute.after", "permission.ask", "chat.message"} {
		if !strings.Contains(string(data), `"`+hook+`"`) {
			t.Fatalf("OpenCode plugin is missing official hook %q", hook)
		}
	}
	if !strings.Contains(string(data), "decision.additional_context") || !strings.Contains(string(data), "decision.block") {
		t.Fatal("OpenCode plugin does not apply BridgeDecision context/block fields")
	}
	if present, complete := gortexOpenCodePluginStatus(path, executable); !present || !complete {
		t.Fatalf("OpenCode plugin status = %v,%v, want true,true", present, complete)
	}
	changed, _, err = upsertOpenCodePlugin(path, executable)
	if err != nil || changed {
		t.Fatalf("second OpenCode plugin write changed=%v error=%v, want idempotent", changed, err)
	}

	artifact := artifacts[0]
	if err := os.WriteFile(path, append(data, []byte("\n// user change\n")...), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := gortexRemoveOpenCodePluginArtifact(path, executable, artifact); err == nil {
		t.Fatal("modified OpenCode plugin was removed")
	}
	if _, _, err := upsertOpenCodePlugin(path, executable); err != nil {
		t.Fatalf("repair OpenCode plugin: %v", err)
	}
	removed, err := gortexRemoveOpenCodePluginArtifact(path, executable, artifact)
	if err != nil || !removed {
		t.Fatalf("remove OpenCode plugin = %v, error=%v", removed, err)
	}
	if _, err := os.Stat(path); !os.IsNotExist(err) {
		t.Fatalf("OpenCode plugin survived exact removal: %v", err)
	}
}

func TestGortexRegisterIntegrationsAddsOpenCodePluginAndAllSupportedPrompts(t *testing.T) {
	profile := t.TempDir()
	localAppData := filepath.Join(profile, "AppData", "Local")
	t.Setenv("USERPROFILE", profile)
	t.Setenv("LOCALAPPDATA", localAppData)
	t.Setenv("ProgramFiles", filepath.Join(profile, "Program Files"))
	t.Setenv("XDG_CONFIG_HOME", "")
	t.Setenv("PATH", "")
	if err := os.MkdirAll(gortexOpenCodeConfigDir(), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := writeGortexJSONObject(gortexConfigPath("opencode"), map[string]any{
		"mcp": map[string]any{
			"other": map[string]any{"type": "remote", "url": "https://example.test/mcp"},
		},
	}); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(gortexGeminiConfigDir(), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(gortexConfigPath("gemini"), []byte("{\"theme\":\"Default\"}\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	antigravityExecutable := filepath.Join(localAppData, "Programs", "antigravity", "Antigravity.exe")
	if err := os.MkdirAll(filepath.Dir(antigravityExecutable), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(antigravityExecutable, []byte("stub"), 0o600); err != nil {
		t.Fatal(err)
	}

	artifacts, warnings := gortexRegisterIntegrations(filepath.Join(profile, "Gortex", "bin", gortexExecutableName))
	if len(warnings) != 0 {
		t.Fatalf("registration warnings = %#v", warnings)
	}
	wantPaths := map[string]string{
		"opencode":    filepath.Join(profile, ".config", "opencode", "AGENTS.md"),
		"antigravity": filepath.Join(profile, ".gemini", "GEMINI.md"),
		"gemini":      filepath.Join(profile, ".gemini", "GEMINI.md"),
	}
	gotPaths := map[string]string{}
	foundOpenCodeHook := false
	foundAntigravityHook := false
	foundGeminiHook := false
	for _, artifact := range artifacts {
		switch {
		case artifact.Kind == gortexArtifactPrompt:
			gotPaths[artifact.Agent] = artifact.Path
		case artifact.Kind == gortexArtifactHook:
			switch artifact.Agent {
			case "opencode":
				foundOpenCodeHook = true
			case "antigravity":
				foundAntigravityHook = true
			case "gemini":
				foundGeminiHook = true
			default:
				t.Fatalf("unexpected hook artifact for %s", artifact.Agent)
			}
		default:
			t.Fatalf("unexpected %s artifact for %s", artifact.Kind, artifact.Agent)
		}
	}
	if !reflect.DeepEqual(gotPaths, wantPaths) {
		t.Fatalf("registered prompt artifacts = %#v, want %#v", gotPaths, wantPaths)
	}
	if !foundOpenCodeHook {
		t.Fatalf("OpenCode plugin artifact was not registered: %#v", artifacts)
	}
	if !foundAntigravityHook || !foundGeminiHook {
		t.Fatalf("Gemini-style hook artifacts were not registered: %#v", artifacts)
	}
	for agent := range wantPaths {
		if present, complete := gortexPromptStatus(agent); !present || !complete {
			t.Fatalf("%s prompt status = %v,%v, want true,true", agent, present, complete)
		}
	}
	if present, complete := gortexHookStatus("opencode", filepath.Join(profile, "Gortex", "bin", gortexExecutableName)); !present || !complete {
		t.Fatalf("OpenCode hook status = %v,%v, want true,true", present, complete)
	}
	for _, agent := range []string{"antigravity", "gemini"} {
		if present, complete := gortexHookStatus(agent, filepath.Join(profile, "Gortex", "bin", gortexExecutableName)); !present || !complete {
			t.Fatalf("%s hook status = %v,%v, want true,true", agent, present, complete)
		}
	}
}

func TestGortexGeminiStyleHooksShareSettingsAndPreserveUserHooks(t *testing.T) {
	profile := t.TempDir()
	t.Setenv("USERPROFILE", profile)
	executable := filepath.Join(profile, "Gortex", "bin", gortexExecutableName)
	path := gortexHookPath("antigravity")
	if err := writeGortexJSONObject(path, map[string]any{
		"hooks": map[string]any{
			"AfterTool": []any{
				map[string]any{"hooks": []any{map[string]any{"type": "command", "command": "user-hook", "name": "user"}}},
			},
		},
	}); err != nil {
		t.Fatal(err)
	}

	changed, artifacts, err := upsertGeminiHooks(path, "antigravity", executable)
	if err != nil || !changed || len(artifacts) != 2 {
		t.Fatalf("Antigravity hook write changed=%v artifacts=%d error=%v", changed, len(artifacts), err)
	}
	if changed, _, err := upsertGeminiHooks(path, "gemini", executable); err != nil || changed {
		t.Fatalf("shared Gemini hook write changed=%v error=%v, want idempotent reuse", changed, err)
	}
	for _, agent := range []string{"antigravity", "gemini"} {
		if present, complete := gortexHookStatus(agent, executable); !present || !complete {
			t.Fatalf("%s hook status = %v,%v, want true,true", agent, present, complete)
		}
	}

	root, err := readGortexJSONObject(path)
	if err != nil {
		t.Fatal(err)
	}
	hooks := root["hooks"].(map[string]any)
	afterTool := hooks["AfterTool"].([]any)
	if len(afterTool) != 2 {
		t.Fatalf("AfterTool groups = %d, want one user and one Gortex group", len(afterTool))
	}
	foundUser := false
	for _, raw := range afterTool {
		group := raw.(map[string]any)
		for _, handlerRaw := range gortexHookList(group["hooks"]) {
			handler := handlerRaw.(map[string]any)
			if handler["command"] == "user-hook" {
				foundUser = true
			}
		}
	}
	if !foundUser {
		t.Fatal("user Gemini-style hook was removed")
	}

	for _, artifact := range artifacts {
		if removed, err := gortexRemoveGeminiHookArtifact(path, "antigravity", executable, artifact); err != nil || !removed {
			t.Fatalf("remove %s hook = %v, error=%v", artifact.Event, removed, err)
		}
	}
	if present, _ := gortexHookStatus("antigravity", executable); present {
		t.Fatal("Antigravity Gortex hooks survived exact removal")
	}
}

func TestGortexRegisterIntegrationsAddsAntigravityPromptWithoutGeminiCLI(t *testing.T) {
	profile := t.TempDir()
	localAppData := filepath.Join(profile, "AppData", "Local")
	t.Setenv("USERPROFILE", profile)
	t.Setenv("LOCALAPPDATA", localAppData)
	t.Setenv("ProgramFiles", filepath.Join(profile, "Program Files"))
	t.Setenv("PATH", "")
	antigravityExecutable := filepath.Join(localAppData, "Programs", "antigravity", "Antigravity.exe")
	if err := os.MkdirAll(filepath.Dir(antigravityExecutable), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(antigravityExecutable, []byte("stub"), 0o600); err != nil {
		t.Fatal(err)
	}

	artifacts, warnings := gortexRegisterIntegrations(filepath.Join(profile, "Gortex", "bin", gortexExecutableName))
	if len(warnings) != 0 {
		t.Fatalf("registration warnings = %#v", warnings)
	}
	promptPath := filepath.Join(profile, ".gemini", "GEMINI.md")
	found := false
	for _, artifact := range artifacts {
		if artifact.Agent == "antigravity" && artifact.Kind == gortexArtifactPrompt && artifact.Path == promptPath {
			found = true
		}
	}
	if !found {
		t.Fatalf("Antigravity prompt artifact was not registered: %#v", artifacts)
	}
	if _, err := os.Stat(promptPath); err != nil {
		t.Fatalf("Antigravity prompt file was not written: %v", err)
	}
	if gortexAgentAvailable("gemini") {
		t.Fatal("Antigravity prompt registration incorrectly implied Gemini CLI availability")
	}
	if present, complete := gortexPromptStatus("antigravity"); !present || !complete {
		t.Fatalf("Antigravity prompt status = %v,%v, want true,true", present, complete)
	}
}

func TestWriteGortexPromptDocumentWritesEmbeddedDocument(t *testing.T) {
	directory := t.TempDir()
	if err := writeGortexPromptDocument(directory); err != nil {
		t.Fatalf("writeGortexPromptDocument() error = %v", err)
	}
	actual, err := os.ReadFile(filepath.Join(directory, gortexPromptFileName))
	if err != nil {
		t.Fatalf("read embedded Gortex prompt: %v", err)
	}
	if string(actual) != string(gortexPromptDocument) {
		t.Fatalf("written Gortex prompt differs from embedded document")
	}

	// 模拟已存在旧版提示词文件，验证先删除旧文件并重新释放最新版本
	targetPath := filepath.Join(directory, gortexPromptFileName)
	if err := os.WriteFile(targetPath, []byte("旧版过时提示词内容"), 0o644); err != nil {
		t.Fatalf("write stale prompt file: %v", err)
	}
	if err := writeGortexPromptDocument(directory); err != nil {
		t.Fatalf("writeGortexPromptDocument() with existing stale file error = %v", err)
	}
	overwritten, err := os.ReadFile(targetPath)
	if err != nil {
		t.Fatalf("read updated Gortex prompt: %v", err)
	}
	if string(overwritten) != string(gortexPromptDocument) {
		t.Fatalf("re-written Gortex prompt differs from embedded document after replacing stale file")
	}
}

func TestGortexCopilotHooksRemovePreservesUserEntries(t *testing.T) {
	path := filepath.Join(t.TempDir(), "gortex.json")
	root := map[string]any{
		"version":         1,
		"disableAllHooks": false,
		"hooks": map[string]any{
			"sessionStart": []any{
				map[string]any{"type": "command", "bash": `C:\old\gortex.exe hook --agent=copilot-cli`},
				map[string]any{"type": "command", "bash": "user-session"},
			},
			"preToolUse": []any{
				map[string]any{"hooks": []any{map[string]any{"type": "command", "command": `C:\old\gortex.exe hook --agent=copilot-cli`}}},
				map[string]any{"type": "command", "bash": "user-pre"},
			},
		},
	}
	data, _ := json.Marshal(root)
	if err := os.WriteFile(path, data, 0o600); err != nil {
		t.Fatal(err)
	}
	changed, err := removeCopilotHooks(path, `D:\new\gortex.exe`)
	if err != nil {
		t.Fatalf("removeCopilotHooks() error = %v", err)
	}
	if !changed {
		t.Fatal("removeCopilotHooks() reported no change")
	}
	cleaned, err := readGortexJSONObject(path)
	if err != nil {
		t.Fatal(err)
	}
	encoded, _ := json.Marshal(cleaned)
	if strings.Contains(strings.ToLower(string(encoded)), "gortex") {
		t.Fatalf("Gortex Copilot hook survived removal: %s", encoded)
	}
	for _, user := range []string{"user-session", "user-pre"} {
		if !strings.Contains(string(encoded), user) {
			t.Fatalf("user Copilot hook %q was removed: %s", user, encoded)
		}
	}
}

func gortexTestHookArtifact(t *testing.T, artifacts []gortexOwnedArtifact, event string) gortexOwnedArtifact {
	t.Helper()
	for _, artifact := range artifacts {
		if artifact.Event == event {
			return artifact
		}
	}
	t.Fatalf("managed Hook artifact for %s was not recorded: %#v", event, artifacts)
	return gortexOwnedArtifact{}
}

func TestGortexOwnedCodexHookRemovalPreservesUserGortexHook(t *testing.T) {
	path := filepath.Join(t.TempDir(), "config.toml")
	executable := `C:\managed\gortex.exe`
	if _, artifacts, err := upsertCodexHooks(path, executable); err != nil {
		t.Fatal(err)
	} else {
		artifact := gortexTestHookArtifact(t, artifacts, "PreToolUse")
		var root map[string]any
		data, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		if _, err := toml.Decode(string(data), &root); err != nil {
			t.Fatal(err)
		}
		hooks := root["hooks"].(map[string]any)
		groups := gortexHookList(hooks["PreToolUse"])
		groups = append(groups, map[string]any{"matcher": ".*", "hooks": []any{map[string]any{"type": "command", "command": `C:\user\gortex.exe hook --agent=codex --mode=enrich`}}})
		hooks["PreToolUse"] = groups
		encoded, err := tomlEncode(root)
		if err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte(encoded), 0o600); err != nil {
			t.Fatal(err)
		}
		ownership := gortexMCPOwnership{Artifacts: map[string]gortexOwnedArtifact{"codex": artifact}}
		if warnings := gortexRemoveIntegrationsOwned(executable, &ownership); len(warnings) > 0 {
			t.Fatalf("owned Codex removal warnings = %v", warnings)
		}
		cleaned, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		var after map[string]any
		if _, err := toml.Decode(string(cleaned), &after); err != nil {
			t.Fatal(err)
		}
		remaining := after["hooks"].(map[string]any)
		foundUser := false
		foundManaged := false
		for _, rawGroup := range gortexHookList(remaining["PreToolUse"]) {
			group := rawGroup.(map[string]any)
			for _, rawHandler := range gortexHookList(group["hooks"]) {
				command := rawHandler.(map[string]any)["command"].(string)
				foundUser = foundUser || strings.Contains(command, `C:\user\gortex.exe`)
				foundManaged = foundManaged || strings.Contains(command, `C:\managed\gortex.exe`)
			}
		}
		if !foundUser || foundManaged {
			t.Fatalf("Codex owned/user Hook result user=%v managed=%v: %#v", foundUser, foundManaged, remaining)
		}
	}
}

func TestGortexOwnedClaudeHookRemovalPreservesUserGortexHook(t *testing.T) {
	path := filepath.Join(t.TempDir(), "settings.local.json")
	executable := `C:\managed\gortex.exe`
	_, artifacts, err := upsertClaudeHooks(path, executable)
	if err != nil {
		t.Fatal(err)
	}
	artifact := gortexTestHookArtifact(t, artifacts, "PreToolUse")
	root, err := readGortexJSONObject(path)
	if err != nil {
		t.Fatal(err)
	}
	hooks := root["hooks"].(map[string]any)
	groups := gortexHookList(hooks["PreToolUse"])
	groups = append(groups, map[string]any{"matcher": "*", "hooks": []any{map[string]any{"type": "command", "command": `C:\user\gortex.exe hook --agent=claude`}}})
	hooks["PreToolUse"] = groups
	if err := writeGortexJSONObject(path, root); err != nil {
		t.Fatal(err)
	}
	ownership := gortexMCPOwnership{Artifacts: map[string]gortexOwnedArtifact{"claude": artifact}}
	if warnings := gortexRemoveIntegrationsOwned(executable, &ownership); len(warnings) > 0 {
		t.Fatalf("owned Claude removal warnings = %v", warnings)
	}
	cleaned, err := readGortexJSONObject(path)
	if err != nil {
		t.Fatal(err)
	}
	remaining := cleaned["hooks"].(map[string]any)
	foundUser, foundManaged := false, false
	for _, rawGroup := range gortexHookList(remaining["PreToolUse"]) {
		group := rawGroup.(map[string]any)
		for _, rawHandler := range gortexHookList(group["hooks"]) {
			command := rawHandler.(map[string]any)["command"].(string)
			foundUser = foundUser || strings.Contains(command, `C:\user\gortex.exe`)
			foundManaged = foundManaged || strings.Contains(command, `C:\managed\gortex.exe`)
		}
	}
	if !foundUser || foundManaged {
		t.Fatalf("Claude owned/user Hook result user=%v managed=%v: %#v", foundUser, foundManaged, remaining)
	}
}

func TestGortexOwnedCopilotHookRemovalPreservesUserGortexHook(t *testing.T) {
	path := filepath.Join(t.TempDir(), "gortex.json")
	executable := `C:\managed\gortex.exe`
	_, artifacts, err := upsertCopilotHooks(path, executable)
	if err != nil {
		t.Fatal(err)
	}
	artifact := gortexTestHookArtifact(t, artifacts, "sessionStart")
	root, err := readGortexJSONObject(path)
	if err != nil {
		t.Fatal(err)
	}
	hooks := root["hooks"].(map[string]any)
	entries := gortexHookList(hooks["sessionStart"])
	entries = append(entries, map[string]any{"type": "command", "bash": `C:/user/gortex.exe hook --agent=copilot-cli`, "powershell": "& 'C:\\user\\gortex.exe' hook --agent=copilot-cli", "cwd": ".", "timeoutSec": 10})
	hooks["sessionStart"] = entries
	if err := writeGortexJSONObject(path, root); err != nil {
		t.Fatal(err)
	}
	ownership := gortexMCPOwnership{Artifacts: map[string]gortexOwnedArtifact{"copilot": artifact}}
	if warnings := gortexRemoveIntegrationsOwned(executable, &ownership); len(warnings) > 0 {
		t.Fatalf("owned Copilot removal warnings = %v", warnings)
	}
	cleaned, err := readGortexJSONObject(path)
	if err != nil {
		t.Fatal(err)
	}
	remaining := cleaned["hooks"].(map[string]any)
	foundUser, foundManaged := false, false
	for _, raw := range gortexHookList(remaining["sessionStart"]) {
		entry := raw.(map[string]any)
		command := gortexDirectHookCommand(entry)
		foundUser = foundUser || strings.Contains(command, "C:/user/gortex.exe")
		foundManaged = foundManaged || strings.Contains(command, "C:/managed/gortex.exe")
	}
	if !foundUser || foundManaged {
		t.Fatalf("Copilot owned/user Hook result user=%v managed=%v: %#v", foundUser, foundManaged, remaining)
	}
}

func TestGortexClaudeHooksUseNativeEventsAndShellSafePath(t *testing.T) {
	command := gortexHookCommand("claude", `C:\Program Files\Gortex\bin\gortex.exe`)
	if strings.Contains(command, `\`) || !strings.Contains(command, "C:/Program Files/Gortex/bin/gortex.exe") {
		t.Fatalf("Claude hook command is not shell-safe: %q", command)
	}
	path := filepath.Join(t.TempDir(), "settings.local.json")
	changed, _, err := upsertClaudeHooks(path, `C:\Program Files\Gortex\bin\gortex.exe`)
	if err != nil || !changed {
		t.Fatalf("upsertClaudeHooks() changed=%v error=%v", changed, err)
	}
	root, err := readGortexJSONObject(path)
	if err != nil {
		t.Fatal(err)
	}
	hooks := root["hooks"].(map[string]any)
	for _, event := range []string{"SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse", "Stop", "PreCompact", "SubagentStart", "SubagentStop"} {
		if len(gortexHookList(hooks[event])) != 1 {
			t.Fatalf("Claude hooks.%s missing native Gortex entry", event)
		}
	}
	pre := gortexHookList(hooks["PreToolUse"])[0].(map[string]any)
	if pre["matcher"] != "*" {
		t.Fatalf("Claude PreToolUse matcher = %v, want *", pre["matcher"])
	}
	handler := gortexHookList(pre["hooks"])[0].(map[string]any)
	if handler["timeout"] != float64(3000) && handler["timeout"] != 3000 {
		t.Fatalf("Claude PreToolUse timeout = %v, want 3000", handler["timeout"])
	}
}

func TestGortexCodexHooksMatchersRoundTripAndRemove(t *testing.T) {
	path := filepath.Join(t.TempDir(), "config.toml")
	seed := map[string]any{"hooks": map[string]any{
		"PreToolUse": []any{map[string]any{"matcher": "UserOnly", "hooks": []any{map[string]any{"type": "command", "command": "echo user-pre"}}}},
	}}
	encoded, err := tomlEncode(seed)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(encoded), 0o600); err != nil {
		t.Fatal(err)
	}
	executable := `C:\Tools\Gortex\bin\gortex.exe`
	changed, _, err := upsertCodexHooks(path, executable)
	if err != nil || !changed {
		t.Fatalf("upsertCodexHooks() changed=%v error=%v", changed, err)
	}
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	var root map[string]any
	if _, err := toml.Decode(string(data), &root); err != nil {
		t.Fatal(err)
	}
	hooks := root["hooks"].(map[string]any)
	wantMatchers := map[string]string{
		"PreToolUse":  ".*",
		"PostToolUse": "^(Bash|apply_patch|(mcp__gortex__|gortex__)(explore|search|read|relations|trace|analyze))$",
	}
	for event, wantMatcher := range wantMatchers {
		found := false
		for _, raw := range gortexHookList(hooks[event]) {
			group, ok := raw.(map[string]any)
			if !ok || !gortexTomlHookCommand(group, executable) {
				continue
			}
			found = true
			if group["matcher"] != wantMatcher {
				t.Fatalf("Codex %s matcher = %v, want %q", event, group["matcher"], wantMatcher)
			}
			handlers := gortexHookList(group["hooks"])
			if len(handlers) != 1 || !strings.Contains(handlers[0].(map[string]any)["command"].(string), "--mode=enrich") {
				t.Fatalf("Codex %s command missing --mode=enrich: %#v", event, handlers)
			}
		}
		if !found {
			t.Fatalf("Codex %s Gortex hook not found", event)
		}
	}
	userFound := false
	for _, raw := range gortexHookList(hooks["PreToolUse"]) {
		group, _ := raw.(map[string]any)
		for _, handler := range gortexHookList(group["hooks"]) {
			if handler.(map[string]any)["command"] == "echo user-pre" {
				userFound = true
			}
		}
	}
	if !userFound {
		t.Fatal("user Codex hook was not preserved")
	}
	if _, err := removeCodexHooks(path, `D:\new\Gortex\bin\gortex.exe`); err != nil {
		t.Fatalf("removeCodexHooks() error = %v", err)
	}
	cleaned, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(cleaned), "--agent=codex") || !strings.Contains(string(cleaned), "echo user-pre") {
		t.Fatalf("Codex removal did not preserve user hook: %s", cleaned)
	}
}

func TestGortexCodexHookCommandWindowsIsRecognized(t *testing.T) {
	value := map[string]any{
		"matcher": ".*",
		"hooks": []any{map[string]any{
			"type":           "command",
			"command":        "gortex hook --agent=codex",
			"commandWindows": `C:\\Tools\\gortex.exe hook --agent=codex --mode=enrich`,
		}},
	}
	if !gortexTomlHookCommand(value, `C:\\Tools\\gortex.exe`) {
		t.Fatal("Codex commandWindows Hook was not recognized")
	}
	handler := gortexNestedCommandHandler(value)
	if handler == nil || !strings.Contains(handler["commandWindows"].(string), "--agent=codex") {
		t.Fatalf("commandWindows handler was not selected: %#v", handler)
	}
}

func TestGortexCodexTrustHashStates(t *testing.T) {
	path := filepath.Join(t.TempDir(), "config.toml")
	executable := `C:\Tools\Gortex\bin\gortex.exe`
	if _, _, err := upsertCodexHooks(path, executable); err != nil {
		t.Fatal(err)
	}
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	var root map[string]any
	if _, err := toml.Decode(string(data), &root); err != nil {
		t.Fatal(err)
	}
	hooks := root["hooks"].(map[string]any)
	groups := gortexHookList(hooks["PreToolUse"])
	found := false
	for groupIndex, rawGroup := range groups {
		group := rawGroup.(map[string]any)
		for handlerIndex, rawHandler := range gortexHookList(group["hooks"]) {
			handler := rawHandler.(map[string]any)
			command := handler["command"].(string)
			if !gortexCodexHookCommandMatches(command, executable) {
				continue
			}
			identity := codexSnipHookIdentity{GroupIndex: groupIndex, HandlerIndex: handlerIndex, Command: command, Timeout: gortexHookTimeout(handler)}
			matcher, _ := group["matcher"].(string)
			if matcher != "" {
				identity.Matcher = &matcher
			}
			status, _ := handler["statusMessage"].(string)
			if status != "" {
				identity.StatusMessage = &status
			}
			hash, err := codexSnipHookHash(identity)
			if err != nil {
				t.Fatal(err)
			}
			states := map[string]any{path + ":pre_tool_use:" + itoa(groupIndex) + ":" + itoa(handlerIndex): map[string]any{"trusted_hash": hash}}
			hooks["state"] = states
			trustedRoot, err := tomlEncode(root)
			if err != nil {
				t.Fatal(err)
			}
			trusted, modified, err := gortexCodexHooksTrusted(trustedRoot, path, executable)
			if err != nil || !trusted || modified {
				t.Fatalf("trusted Codex state = trusted:%v modified:%v error:%v", trusted, modified, err)
			}
			delete(hooks, "state")
			missingRoot, _ := tomlEncode(root)
			missing, _, err := gortexCodexHooksTrusted(missingRoot, path, executable)
			if err != nil || missing {
				t.Fatalf("missing trusted_hash reported trusted=%v error=%v", missing, err)
			}
			found = true
			break
		}
		if found {
			break
		}
	}
	if !found {
		t.Fatal("did not find a Codex Gortex PreToolUse hook")
	}
}

func itoa(value int) string {
	if value == 0 {
		return "0"
	}
	// The small helper keeps this test file independent from formatting code
	// used by the production paths.
	digits := ""
	for value > 0 {
		digits = string(rune('0'+value%10)) + digits
		value /= 10
	}
	return digits
}

func TestEnsureGortexWatchConfigPreservesUserYAML(t *testing.T) {
	project := t.TempDir()
	path := filepath.Join(project, ".gortex.yaml")
	original := "exclude: [vendor/**]\nwatch: {enabled: false, debounce_ms: 5000, paths: [src]}\nsearch: {index_prose: false, custom: keep}\ninclude: [custom/**]\n"
	if err := os.WriteFile(path, []byte(original), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := ensureGortexWatchConfig(project); err != nil {
		t.Fatalf("ensureGortexWatchConfig() error = %v", err)
	}
	first, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	var root map[string]any
	if err := yaml.Unmarshal(first, &root); err != nil {
		t.Fatal(err)
	}
	watch := root["watch"].(map[string]any)
	if watch["enabled"] != true || int(watch["debounce_ms"].(int)) != 300 {
		t.Fatalf("watch config = %#v", watch)
	}
	if watch["paths"].([]any)[0] != "src" {
		t.Fatalf("user watch.paths was not preserved: %#v", watch)
	}
	search := root["search"].(map[string]any)
	if search["index_prose"] != true || search["custom"] != "keep" {
		t.Fatalf("search config = %#v", search)
	}
	include := root["include"].([]any)
	if len(include) != 1 || include[0] != "custom/**" {
		t.Fatalf("user include entries were changed: %#v", include)
	}
	if err := ensureGortexWatchConfig(project); err != nil {
		t.Fatal(err)
	}
	second, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if string(first) != string(second) {
		t.Fatal("watch config is not idempotent")
	}
	missing := filepath.Join(t.TempDir(), "gone")
	if err := ensureGortexWatchConfig(missing); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(filepath.Join(missing, ".gortex.yaml")); !os.IsNotExist(err) {
		t.Fatalf("missing project was recreated: %v", err)
	}
}

func TestGortexCursorRuleUsesFrontmatterAndRemovesOwnedFile(t *testing.T) {
	project := t.TempDir()
	ownership := gortexMCPOwnership{Artifacts: map[string]gortexOwnedArtifact{}}
	if err := gortexRegisterCursorProject(project, &ownership); err != nil {
		t.Fatalf("gortexRegisterCursorProject() error = %v", err)
	}
	path := gortexCursorRulePath(project)
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	text := string(data)
	if !strings.HasPrefix(text, "---\n") || !strings.Contains(text, "alwaysApply: true") {
		t.Fatalf("Cursor rule is missing required frontmatter: %s", text)
	}
	if !gortexCursorRuleConfigured(project) {
		t.Fatal("Cursor rule was not reported as configured")
	}
	if err := gortexRemoveCursorProject(project, &ownership); err != nil {
		t.Fatalf("gortexRemoveCursorProject() error = %v", err)
	}
	if _, err := os.Stat(path); !os.IsNotExist(err) {
		t.Fatalf("owned Cursor rule file was not removed, stat error = %v", err)
	}
}

func TestGortexCursorRulePreservesUnmarkedUserFile(t *testing.T) {
	project := t.TempDir()
	path := gortexCursorRulePath(project)
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		t.Fatal(err)
	}
	original := []byte("# User Cursor rule\n")
	if err := os.WriteFile(path, original, 0o600); err != nil {
		t.Fatal(err)
	}
	ownership := gortexMCPOwnership{Artifacts: map[string]gortexOwnedArtifact{}}
	if _, _, err := upsertGortexCursorPrompt(path, gortexInstructionBody); err == nil {
		t.Fatal("unmarked user Cursor rule was overwritten")
	}
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if string(data) != string(original) {
		t.Fatalf("user Cursor rule changed: %q", data)
	}
	if err := gortexRegisterCursorProject(project, &ownership); err == nil {
		t.Fatal("gortexRegisterCursorProject accepted an unmarked user rule")
	}
}

func TestEnsureGortexGlobalConfigWorkspaces(t *testing.T) {
	tempRoot := t.TempDir()
	configDir := filepath.Join(tempRoot, "config", "gortex")
	if err := os.MkdirAll(configDir, 0o700); err != nil {
		t.Fatal(err)
	}
	cfgPath := filepath.Join(configDir, "config.yaml")
	// Initial YAML has 3 items: edit, kwor, and gortex, plus an extra untracked item
	initialYAML := []byte("repos:\n  - path: C:\\EXEXX\\edit\n  - path: E:\\111111\\kwor\\kwor\n  - path: D:\\untracked\\extra\n    workspace: other\n")
	if err := os.WriteFile(cfgPath, initialYAML, 0o600); err != nil {
		t.Fatal(err)
	}

	t.Setenv("XDG_CONFIG_HOME", filepath.Join(tempRoot, "config"))

	// Verify slug generation
	if slug := cleanGortexProjectSlug(`C:\EXEXX\edit`); slug != "edit" {
		t.Fatalf("cleanGortexProjectSlug(edit) = %q, want 'edit'", slug)
	}
	if slug := cleanGortexProjectSlug(`E:\111111\kwor\kwor`); slug != "kwor" {
		t.Fatalf("cleanGortexProjectSlug(kwor) = %q, want 'kwor'", slug)
	}
	if slug := cleanGortexProjectSlug(`E:\aex\Downloads\gortex-0.64.3\gortex-0.64.3`); slug != "gortex" {
		t.Fatalf("cleanGortexProjectSlug(gortex-0.64.3) = %q, want 'gortex'", slug)
	}

	// UI has 3 projects: edit, kwor, and gortex-0.64.3
	uiProjects := []string{
		`C:\EXEXX\edit`,
		`E:\111111\kwor\kwor`,
		`E:\aex\Downloads\gortex-0.64.3\gortex-0.64.3`,
	}

	if err := ensureGortexGlobalConfigWorkspacesAtPath(cfgPath, "default", uiProjects); err != nil {
		t.Fatalf("ensureGortexGlobalConfigWorkspacesAtPath() error = %v", err)
	}
	updated, err := os.ReadFile(cfgPath)
	if err != nil {
		t.Fatal(err)
	}
	text := string(updated)
	// Must contain workspace: default and project slugs for UI projects
	if !strings.Contains(text, "workspace: default") {
		t.Fatalf("updated YAML missing 'workspace: default':\n%s", text)
	}
	if !strings.Contains(text, "project: edit") {
		t.Fatalf("updated YAML missing 'project: edit':\n%s", text)
	}
	if !strings.Contains(text, "project: kwor") {
		t.Fatalf("updated YAML missing 'project: kwor':\n%s", text)
	}
	// Missing project gortex-0.64.3 should have been automatically added
	if !strings.Contains(text, "project: gortex") {
		t.Fatalf("updated YAML missing auto-added 'project: gortex':\n%s", text)
	}
	// Extra untracked repo must have been automatically removed
	if strings.Contains(text, "untracked") {
		t.Fatalf("updated YAML still contains untracked project that should have been removed:\n%s", text)
	}

	// Test idempotency
	if err := ensureGortexGlobalConfigWorkspacesAtPath(cfgPath, "default", uiProjects); err != nil {
		t.Fatalf("idempotent ensureGortexGlobalConfigWorkspacesAtPath() error = %v", err)
	}
	updated2, err := os.ReadFile(cfgPath)
	if err != nil {
		t.Fatal(err)
	}
	if string(updated2) != text {
		t.Fatalf("ensureGortexGlobalConfigWorkspacesAtPath not idempotent:\nBefore:\n%s\nAfter:\n%s", text, string(updated2))
	}
}

func TestEnsureGortexGlobalConfigWorkspaces_ExtraRemovalAndTagRepair(t *testing.T) {
	tempRoot := t.TempDir()
	cfgPath := filepath.Join(tempRoot, "config.yaml")

	// Start with messy YAML: wrong workspace, missing project, extra repos
	initialYAML := []byte(`repos:
  - path: C:\EXEXX\edit
    workspace: custom_ws
  - path: D:\rogue\repo1
    workspace: rogue
    project: rogue
  - path: E:\rogue\repo2
`)
	if err := os.WriteFile(cfgPath, initialYAML, 0o600); err != nil {
		t.Fatal(err)
	}

	// UI only has edit and kwor
	uiProjects := []string{`C:\EXEXX\edit`, `E:\111111\kwor\kwor`}

	if err := ensureGortexGlobalConfigWorkspacesAtPath(cfgPath, "default", uiProjects); err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	data, err := os.ReadFile(cfgPath)
	if err != nil {
		t.Fatal(err)
	}
	content := string(data)

	// Rogue repos should be completely gone
	if strings.Contains(content, "rogue") {
		t.Fatalf("rogue repos should be removed, got:\n%s", content)
	}

	// Edit should have workspace fixed to default and project set to edit
	if !strings.Contains(content, "workspace: default") {
		t.Fatalf("expected workspace: default, got:\n%s", content)
	}
	if !strings.Contains(content, "project: edit") {
		t.Fatalf("expected project: edit, got:\n%s", content)
	}

	// Kwor should be auto-added
	if !strings.Contains(content, "project: kwor") {
		t.Fatalf("expected kwor to be added, got:\n%s", content)
	}
}
