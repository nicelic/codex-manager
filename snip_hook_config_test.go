package main

import "testing"

func TestSnipHookCommandMatchesOfficialWindowsEscapedQuote(t *testing.T) {
	executable := `C:/Program Files/code-Manager/Snip/snip.exe`
	command := `\"C:/Program Files/code-Manager/Snip/snip.exe\" hook codex`
	if !snipHookCommandTargetsAgent(command, "codex") {
		t.Fatalf("official Windows Codex command was not recognized: %q", command)
	}
	if !snipHookCommandMatchesExecutable(command, "codex", executable) {
		t.Fatalf("official Windows Codex command did not match executable: %q", command)
	}
}
