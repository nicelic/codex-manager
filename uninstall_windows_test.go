//go:build windows

package main

import (
	"bytes"
	"os"
	"path/filepath"
	"testing"
)

func TestValidateReleaseUninstallRootRejectsDevelopmentDirectory(t *testing.T) {
	releaseDir := t.TempDir()
	if err := validateReleaseUninstallRoot(releaseDir); err != nil {
		t.Fatalf("expected release directory to pass validation: %v", err)
	}
	if err := os.WriteFile(filepath.Join(releaseDir, "main.go"), []byte("package main\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := validateReleaseUninstallRoot(releaseDir); err == nil {
		t.Fatal("expected development directory to be rejected")
	}
}

func TestEnsureUninstallScriptInDirectoryWritesEmbeddedTemplate(t *testing.T) {
	releaseDir := t.TempDir()
	scriptPath := filepath.Join(releaseDir, uninstallBatFileName)
	if err := os.WriteFile(scriptPath, []byte("old script\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := ensureUninstallScriptInDirectory(releaseDir); err != nil {
		t.Fatal(err)
	}
	contents, err := os.ReadFile(scriptPath)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(contents, uninstallBatTemplate) {
		t.Fatal("uninstall script does not match the embedded template")
	}
}

func TestEnsureUninstallScriptForExecutableSkipsDevelopmentDirectory(t *testing.T) {
	developmentDir := t.TempDir()
	if err := os.WriteFile(filepath.Join(developmentDir, "main.go"), []byte("package main\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	executable := filepath.Join(developmentDir, "code-Manager.exe")
	if err := ensureUninstallScriptForExecutable(executable); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(filepath.Join(developmentDir, uninstallBatFileName)); !os.IsNotExist(err) {
		t.Fatalf("development directory received an uninstall script: %v", err)
	}
}

func TestRemoveReleaseRuntimeArtifactsKeepsReleaseSiblings(t *testing.T) {
	releaseDir := t.TempDir()
	configDir := filepath.Join(releaseDir, "config")
	if err := os.Mkdir(configDir, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(configDir, "config.yaml"), []byte("startup_enabled: false\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	keepPath := filepath.Join(releaseDir, "keep-me.txt")
	if err := os.WriteFile(keepPath, []byte("user file\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := removeReleaseRuntimeArtifacts(releaseDir); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Lstat(configDir); !os.IsNotExist(err) {
		t.Fatalf("config directory still exists or could not be checked: %v", err)
	}
	if _, err := os.Stat(keepPath); err != nil {
		t.Fatalf("unrelated release sibling was removed: %v", err)
	}
}
