package main

import (
	"fmt"
	"os"
	"path/filepath"
	"testing"
)

func TestParseCodexCLIVersionAndMinimum(t *testing.T) {
	version, err := parseCodexCLIVersion("codex-cli 0.131.0")
	if err != nil {
		t.Fatal(err)
	}
	minimum := codexCLIVersion{Major: snipMinimumCodexMajor, Minor: snipMinimumCodexMinor, Patch: snipMinimumCodexPatch}
	if !version.atLeast(minimum) {
		t.Fatalf("version %#v should satisfy %#v", version, minimum)
	}
	older, err := parseCodexCLIVersion("codex 0.130.9")
	if err != nil {
		t.Fatal(err)
	}
	if older.atLeast(minimum) {
		t.Fatalf("version %#v must not satisfy %#v", older, minimum)
	}
	if _, err := parseCodexCLIVersion("codex latest"); err == nil {
		t.Fatal("missing semantic version must be rejected")
	}
}

func TestCodexSnipHookTrustBindsToNormalizedHookContent(t *testing.T) {
	command := "C:\\Snip\\snip.exe hook codex"
	data := codexSnipHookJSON(command)
	identities, err := codexSnipHookIdentities(data)
	if err != nil {
		t.Fatal(err)
	}
	if len(identities) != 1 {
		t.Fatalf("identities = %#v", identities)
	}
	hash, err := codexSnipHookHash(identities[0])
	if err != nil {
		t.Fatal(err)
	}
	const want = "sha256:c2a0c9869fdc0b6138dee7886cb29499f7f7c441a84f35419d76dae7ef213aa7"
	if hash != want {
		t.Fatalf("normalized Codex Hook hash = %q, want %q", hash, want)
	}
	hookPath := "C:\\Users\\tester\\.codex\\hooks.json"
	config := fmt.Sprintf("[hooks.state.'%s:pre_tool_use:0:0']\ntrusted_hash = \"%s\"\n", hookPath, hash)
	trusted, modified, err := codexSnipHooksTrusted(config, hookPath, identities)
	if err != nil {
		t.Fatal(err)
	}
	if !trusted || modified {
		t.Fatalf("trusted=%t modified=%t, want trusted current Hook", trusted, modified)
	}

	changedIdentities, err := codexSnipHookIdentities(codexSnipHookJSON("D:\\Snip\\snip.exe hook codex"))
	if err != nil {
		t.Fatal(err)
	}
	trusted, modified, err = codexSnipHooksTrusted(config, hookPath, changedIdentities)
	if err != nil {
		t.Fatal(err)
	}
	if trusted || !modified {
		t.Fatalf("trusted=%t modified=%t after Hook change, want false/true", trusted, modified)
	}
}

func writeTrustedCodexSnipHook(t *testing.T, hookPath, executable string) {
	t.Helper()
	data := codexSnipHookJSON("\"" + executable + "\" hook codex")
	identities, err := codexSnipHookIdentities(data)
	if err != nil {
		t.Fatal(err)
	}
	if len(identities) != 1 {
		t.Fatalf("identities = %#v", identities)
	}
	hash, err := codexSnipHookHash(identities[0])
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(hookPath, data, 0o644); err != nil {
		t.Fatal(err)
	}
	config := fmt.Sprintf("[hooks.state.'%s:pre_tool_use:0:0']\ntrusted_hash = \"%s\"\n", hookPath, hash)
	if err := os.WriteFile(filepath.Join(filepath.Dir(hookPath), "config.toml"), []byte(config), 0o644); err != nil {
		t.Fatal(err)
	}
}

func codexSnipHookJSON(command string) []byte {
	return []byte(fmt.Sprintf("{\"hooks\":{\"PreToolUse\":[{\"matcher\":\"Bash\",\"hooks\":[{\"type\":\"command\",\"command\":%q}]}]}}", command))
}
