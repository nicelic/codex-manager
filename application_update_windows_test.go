//go:build windows

package main

import (
	"fmt"
	"os"
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

func TestCodeManagerUpdateBatTemplateContainsKeyOperations(t *testing.T) {
	script := codeManagerUpdateBatTemplate
	for _, fragment := range []string{
		"taskkill /F /IM gortex.exe /T",
		"taskkill /F /IM rtk.exe /T",
		"taskkill /F /IM snip.exe /T",
		"taskkill /F /IM llmtrim.exe /T",
		"taskkill /F /IM code-Manager.exe /T",
		`move /y "%TARGET%" "%BACKUP%"`,
		`copy /y "%STAGED%" "%TARGET%"`,
		`start "" "%TARGET%"`,
		`(goto) 2>nul & del "%~f0"`,
		"code-Manager-update.log",
	} {
		if !strings.Contains(script, fragment) {
			t.Fatalf("update bat template is missing %q", fragment)
		}
	}
}
