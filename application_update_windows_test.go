//go:build windows

package main

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

func TestCodeManagerReleaseAssetRequiresExactName(t *testing.T) {
	release := githubRelease{Assets: []llmtrimReleaseAsset{
		{Name: "code-Manager-v0.1.2.exe", Digest: "sha256:" + strings.Repeat("a", 64)},
		{Name: codeManagerExecutableName, Digest: "sha256:" + strings.Repeat("b", 64)},
	}}
	asset, ok := codeManagerReleaseAsset(release)
	if !ok {
		t.Fatal("codeManagerReleaseAsset() found no exact asset")
	}
	if asset.Name != codeManagerExecutableName {
		t.Fatalf("asset name = %q, want %q", asset.Name, codeManagerExecutableName)
	}
}

func TestCodeManagerAssetSHA256(t *testing.T) {
	want := strings.Repeat("a", 64)
	got, err := codeManagerAssetSHA256(githubRelease{}, llmtrimReleaseAsset{Digest: "sha256:" + strings.ToUpper(want)})
	if err != nil {
		t.Fatalf("codeManagerAssetSHA256() error = %v", err)
	}
	if got != want {
		t.Fatalf("codeManagerAssetSHA256() = %q, want %q", got, want)
	}
	if _, err := codeManagerAssetSHA256(githubRelease{}, llmtrimReleaseAsset{Digest: "sha256:short"}); err == nil {
		t.Fatal("codeManagerAssetSHA256() accepted an invalid digest")
	}

	releaseWithBody := githubRelease{
		Body: fmt.Sprintf("Release notes\r\nSHA-256: %s\r\ncode-Manager.exe", strings.ToUpper(want)),
	}
	assetNoDigest := llmtrimReleaseAsset{Name: codeManagerExecutableName}
	gotFromBody, err := codeManagerAssetSHA256(releaseWithBody, assetNoDigest)
	if err != nil {
		t.Fatalf("codeManagerAssetSHA256() fallback from body error = %v", err)
	}
	if gotFromBody != want {
		t.Fatalf("codeManagerAssetSHA256() fallback = %q, want %q", gotFromBody, want)
	}
}

func TestCompareVersions(t *testing.T) {
	tests := []struct {
		v1   string
		v2   string
		want int
	}{
		{"v0.1.20", "v0.1.20", 0},
		{"v0.1.20", "v0.1.19", 1},
		{"v0.1.19", "v0.1.20", -1},
		{"v0.2.0", "v0.1.99", 1},
		{"1.0.0", "v0.9.9", 1},
	}
	for _, tt := range tests {
		if got := compareVersions(tt.v1, tt.v2); got != tt.want {
			t.Errorf("compareVersions(%q, %q) = %d, want %d", tt.v1, tt.v2, got, tt.want)
		}
	}
}

func TestValidateCodeManagerExecutable(t *testing.T) {
	directory := t.TempDir()
	validPath := filepath.Join(directory, "valid.exe")
	if err := os.WriteFile(validPath, []byte("MZtest"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := validateCodeManagerExecutable(validPath); err != nil {
		t.Fatalf("validateCodeManagerExecutable(valid) error = %v", err)
	}
	invalidPath := filepath.Join(directory, "invalid.exe")
	if err := os.WriteFile(invalidPath, []byte("not an executable"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := validateCodeManagerExecutable(invalidPath); err == nil {
		t.Fatal("validateCodeManagerExecutable(invalid) error = nil")
	}
}

func TestCodeManagerUpdatePowerShellScriptRestoresVerifiedPreviousVersion(t *testing.T) {
	state := codeManagerUpdateState{
		ParentPID:     123,
		TargetVersion: "v0.1.2",
		Executable:    `C:\\release\\code-Manager.exe`,
		Staged:        `C:\\release\\.code-manager-update-123.new`,
		Backup:        `C:\\release\\.code-manager-backup-123.exe`,
		ResumeProxy:   true,
	}
	script := codeManagerUpdatePowerShellScript(state, `C:\\release\\config\\.code-manager-update.json`)
	for _, fragment := range []string{
		"function Restore-PreviousVersion",
		"function Resume-Proxy",
		"-WorkingDirectory (Split-Path -LiteralPath $filePath -Parent)",
		"$identity.executable_path",
		"-Uri 'http://127.0.0.1:7780/healthz'",
		"Wait-CodeManagerReady $targetVersion $true",
		"if (Restore-PreviousVersion) {",
		"if ($originalMoved) {",
		"Get-Process -Name 'gortex'",
		"Name='code-Manager.exe'",
	} {
		if !strings.Contains(script, fragment) {
			t.Fatalf("update script is missing %q", fragment)
		}
	}
}

func TestCodeManagerUpdatePowerShellScriptParses(t *testing.T) {
	powerShell, err := exec.LookPath("powershell.exe")
	if err != nil {
		t.Skip("powershell.exe is not available")
	}
	state := codeManagerUpdateState{
		ParentPID:     123,
		TargetVersion: "v0.1.2",
		Executable:    `C:\\release\\code-Manager.exe`,
		Staged:        `C:\\release\\.code-manager-update-123.new`,
		Backup:        `C:\\release\\.code-manager-backup-123.exe`,
		ResumeProxy:   true,
	}
	script := codeManagerUpdatePowerShellScript(state, `C:\\release\\config\\.code-manager-update.json`)
	parser := fmt.Sprintf("$script = %s; $tokens = $null; $errors = $null; [System.Management.Automation.Language.Parser]::ParseInput($script, [ref]$tokens, [ref]$errors) | Out-Null; if ($errors.Count -gt 0) { $errors | ForEach-Object { $_.ToString() }; exit 1 }", quotePowerShellString(script))
	output, err := exec.Command(powerShell, "-NoLogo", "-NoProfile", "-NonInteractive", "-EncodedCommand", encodePowerShellCommand(parser)).CombinedOutput()
	if err != nil {
		t.Fatalf("PowerShell script parse failed: %v\n%s", err, output)
	}
}
