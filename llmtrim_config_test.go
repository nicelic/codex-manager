package main

import (
	"bytes"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"unicode/utf8"

	"github.com/BurntSushi/toml"
)

func writeTestGatewayConfig(t *testing.T) string {
	t.Helper()
	configPath := filepath.Join(t.TempDir(), "config.yaml")
	if err := os.WriteFile(configPath, []byte(defaultConfigYAML), 0600); err != nil {
		t.Fatal(err)
	}
	return configPath
}

func TestSavingUpstreamBaseURLTakesOverLLMTrimConfig(t *testing.T) {
	t.Setenv("USERPROFILE", t.TempDir())
	configPath := writeTestGatewayConfig(t)
	gateway := &gateway{
		config: Config{
			ListenAddress:   "127.0.0.1:7780",
			UpstreamBaseURL: "https://old.example/v1",
			UpstreamAPIKey:  "upstream-secret",
		},
		configPath: configPath,
	}
	request := httptest.NewRequest(http.MethodPut, "/api/settings/upstream_base_url", strings.NewReader(`{"value":"  https://Api.Example.test:32400/v1/  "}`))
	recorder := httptest.NewRecorder()

	gateway.updateSetting(recorder, request)

	if recorder.Code != http.StatusOK {
		t.Fatalf("status=%d body=%q", recorder.Code, recorder.Body.String())
	}
	directory, err := llmtrimManagedConfigDirectory()
	if err != nil {
		t.Fatal(err)
	}
	contents, err := os.ReadFile(filepath.Join(directory, "config.toml"))
	if err != nil {
		t.Fatal(err)
	}
	if got, want := string(contents), "# Managed by code-Manager.\nextra_hosts = [\"api.example.test\"]\n"; got != want {
		t.Fatalf("config.toml = %q, want %q", got, want)
	}
	if bytes.Contains(contents, []byte("upstream-secret")) {
		t.Fatal("upstream API key leaked into llmtrim config")
	}
	if _, err := os.Stat(filepath.Join(directory, llmtrimManagedConfigMarkerName)); err != nil {
		t.Fatalf("managed marker missing: %v", err)
	}
	updated, err := loadConfig(configPath)
	if err != nil {
		t.Fatal(err)
	}
	if updated.UpstreamBaseURL != "https://Api.Example.test:32400/v1" {
		t.Fatalf("upstream URL = %q", updated.UpstreamBaseURL)
	}
	if !strings.Contains(recorder.Body.String(), "停止后再次启动 llmtrim") {
		t.Fatalf("save response did not explain manual restart: %q", recorder.Body.String())
	}
}

func TestSavingUpstreamBaseURLOverwritesTakenOverConfigAndSupportsIPHosts(t *testing.T) {
	t.Setenv("USERPROFILE", t.TempDir())
	directory, err := llmtrimManagedConfigDirectory()
	if err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(directory, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(directory, "config.toml"), []byte("custom = \"discarded\"\nextra_hosts = [\"old.example\"]\n"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := syncManagedLLMTrimConfig("https://[2001:db8::1]:32400/v1"); err != nil {
		t.Fatal(err)
	}
	contents, err := os.ReadFile(filepath.Join(directory, "config.toml"))
	if err != nil {
		t.Fatal(err)
	}
	if got, want := string(contents), "# Managed by code-Manager.\nextra_hosts = [\"2001:db8::1\"]\n"; got != want {
		t.Fatalf("config.toml = %q, want %q", got, want)
	}
}

func TestSavingUpstreamBaseURLRollsBackLLMTrimConfigWhenYAMLSaveFails(t *testing.T) {
	t.Setenv("USERPROFILE", t.TempDir())
	missingConfigPath := filepath.Join(t.TempDir(), "missing", "config.yaml")
	if err := saveUpstreamBaseURLWithLLMTrimConfig(missingConfigPath, "https://api.example.test/v1"); err == nil {
		t.Fatal("saving a missing YAML config must fail")
	}
	directory, err := llmtrimManagedConfigDirectory()
	if err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(directory); !os.IsNotExist(err) {
		t.Fatalf("llmtrim config directory remains after rollback: %v", err)
	}
}

func TestRemoveManagedLLMTrimConfigDirectoryRemovesEntireDirectory(t *testing.T) {
	t.Setenv("USERPROFILE", t.TempDir())
	if _, err := syncManagedLLMTrimConfig("https://api.example.test/v1"); err != nil {
		t.Fatal(err)
	}
	directory, err := llmtrimManagedConfigDirectory()
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(directory, "other-managed-file"), []byte("remove me"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := removeManagedLLMTrimConfigDirectory(); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(directory); !os.IsNotExist(err) {
		t.Fatalf("managed config directory remains: %v", err)
	}
}

func TestRemoveManagedLLMTrimConfigDirectoryRejectsUnmanagedDirectory(t *testing.T) {
	t.Setenv("USERPROFILE", t.TempDir())
	directory, err := llmtrimManagedConfigDirectory()
	if err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(directory, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(directory, "config.toml"), []byte("extra_hosts = [\"user.example\"]\n"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := removeManagedLLMTrimConfigDirectory(); err == nil {
		t.Fatal("unmanaged llmtrim config directory must not be removed")
	}
	if _, err := os.Stat(directory); err != nil {
		t.Fatalf("unmanaged directory was removed: %v", err)
	}
}

func TestValidateManagedLLMTrimConfigTargetRejectsOtherDirectory(t *testing.T) {
	t.Setenv("USERPROFILE", t.TempDir())
	unsafeDirectory := filepath.Join(t.TempDir(), ".config", "llmtrim")
	if _, _, err := validateManagedLLMTrimConfigTarget(unsafeDirectory); err == nil {
		t.Fatal("a directory outside USERPROFILE must not be accepted for managed deletion")
	}
}

func TestManagedLLMTrimConfigUsesInstallLocalTrackingDatabase(t *testing.T) {
	t.Setenv("USERPROFILE", t.TempDir())
	installDir := filepath.Join(t.TempDir(), "llmtrim")
	if err := os.MkdirAll(installDir, 0700); err != nil {
		t.Fatal(err)
	}
	if err := writeManagedToolState(installDir, managedToolState{DesiredRunning: false, Running: false}); err != nil {
		t.Fatal(err)
	}
	executablePath := filepath.Join(installDir, llmtrimExecutableName)
	databasePath, managed, err := managedLLMTrimTrackingDatabasePath(executablePath)
	if err != nil {
		t.Fatal(err)
	}
	if !managed {
		t.Fatal("managed llmtrim installation was not recognized")
	}
	if want := filepath.Join(installDir, llmtrimTrackingDatabaseName); databasePath != want {
		t.Fatalf("database path = %q, want %q", databasePath, want)
	}
	if _, err := syncManagedLLMTrimConfigForExecutable("https://api.example.test/v1", executablePath); err != nil {
		t.Fatal(err)
	}
	directory, err := llmtrimManagedConfigDirectory()
	if err != nil {
		t.Fatal(err)
	}
	contents, err := os.ReadFile(filepath.Join(directory, "config.toml"))
	if err != nil {
		t.Fatal(err)
	}
	if !utf8.Valid(contents) {
		t.Fatal("managed config.toml is not valid UTF-8")
	}
	var decoded managedLLMTrimTOML
	if _, err := toml.Decode(string(contents), &decoded); err != nil {
		t.Fatal(err)
	}
	if got, want := decoded.ExtraHosts, []string{"api.example.test"}; strings.Join(got, ",") != strings.Join(want, ",") {
		t.Fatalf("extra_hosts = %#v, want %#v", got, want)
	}
	if decoded.DBPath != databasePath {
		t.Fatalf("db_path = %q, want %q", decoded.DBPath, databasePath)
	}
}

func TestRemoveLLMTrimTrackingDatabaseRemovesSQLiteArtifactsOnly(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "llmtrim")
	if err := os.MkdirAll(directory, 0700); err != nil {
		t.Fatal(err)
	}
	databasePath := filepath.Join(directory, llmtrimTrackingDatabaseName)
	for _, candidate := range []string{databasePath, databasePath + "-wal", databasePath + "-shm"} {
		if err := os.WriteFile(candidate, []byte("sqlite-test"), 0600); err != nil {
			t.Fatal(err)
		}
	}
	keepPath := filepath.Join(directory, "keep.txt")
	if err := os.WriteFile(keepPath, []byte("keep"), 0600); err != nil {
		t.Fatal(err)
	}
	removed, err := removeLLMTrimTrackingDatabase(databasePath)
	if err != nil {
		t.Fatal(err)
	}
	if !removed {
		t.Fatal("expected SQLite tracking files to be removed")
	}
	for _, candidate := range []string{databasePath, databasePath + "-wal", databasePath + "-shm"} {
		if _, err := os.Stat(candidate); !os.IsNotExist(err) {
			t.Fatalf("tracking database artifact remains %q: %v", candidate, err)
		}
	}
	contents, err := os.ReadFile(keepPath)
	if err != nil {
		t.Fatal(err)
	}
	if string(contents) != "keep" {
		t.Fatalf("sibling file content = %q, want keep", contents)
	}
}

func TestLLMTrimDefaultTrackingDatabasePathUsesUserProfile(t *testing.T) {
	userProfile := t.TempDir()
	t.Setenv("USERPROFILE", userProfile)
	t.Setenv("HOME", "")
	t.Setenv("XDG_DATA_HOME", "")
	pathValue, err := llmtrimDefaultTrackingDatabasePath()
	if err != nil {
		t.Fatal(err)
	}
	want := filepath.Join(userProfile, ".local", "share", "llmtrim", llmtrimTrackingDatabaseName)
	if pathValue != want {
		t.Fatalf("default tracking database = %q, want %q", pathValue, want)
	}
}

func TestSavingUpstreamBaseURLUsesManagedInstallationLedger(t *testing.T) {
	t.Setenv("USERPROFILE", t.TempDir())
	installDir := filepath.Join(t.TempDir(), "llmtrim")
	if err := os.MkdirAll(installDir, 0700); err != nil {
		t.Fatal(err)
	}
	if err := writeManagedToolState(installDir, managedToolState{DesiredRunning: false, Running: false}); err != nil {
		t.Fatal(err)
	}
	configPath := writeTestGatewayConfig(t)
	gateway := &gateway{
		config: Config{
			ListenAddress:   "127.0.0.1:7780",
			UpstreamBaseURL: "https://old.example/v1",
			UpstreamAPIKey:  "upstream-secret",
			LLMTrimPath:     filepath.Join(installDir, llmtrimExecutableName),
		},
		configPath: configPath,
	}
	recorder := httptest.NewRecorder()

	gateway.updateUpstreamBaseURL(recorder, "https://api.example.test/v1")

	if recorder.Code != http.StatusOK {
		t.Fatalf("status=%d body=%q", recorder.Code, recorder.Body.String())
	}
	directory, err := llmtrimManagedConfigDirectory()
	if err != nil {
		t.Fatal(err)
	}
	contents, err := os.ReadFile(filepath.Join(directory, "config.toml"))
	if err != nil {
		t.Fatal(err)
	}
	var decoded managedLLMTrimTOML
	if _, err := toml.Decode(string(contents), &decoded); err != nil {
		t.Fatal(err)
	}
	if want := filepath.Join(installDir, llmtrimTrackingDatabaseName); decoded.DBPath != want {
		t.Fatalf("db_path = %q, want %q", decoded.DBPath, want)
	}
}

func TestRemoveLLMTrimDefaultTrackingDatabaseDeletesLegacyArtifacts(t *testing.T) {
	userProfile := t.TempDir()
	t.Setenv("USERPROFILE", userProfile)
	t.Setenv("HOME", "")
	t.Setenv("XDG_DATA_HOME", "")
	databasePath, err := llmtrimDefaultTrackingDatabasePath()
	if err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(filepath.Dir(databasePath), 0700); err != nil {
		t.Fatal(err)
	}
	for _, candidate := range []string{databasePath, databasePath + "-wal", databasePath + "-shm"} {
		if err := os.WriteFile(candidate, []byte("legacy-sqlite-test"), 0600); err != nil {
			t.Fatal(err)
		}
	}
	removed, err := removeLLMTrimDefaultTrackingDatabase()
	if err != nil {
		t.Fatal(err)
	}
	if !removed {
		t.Fatal("expected legacy tracking database artifacts to be removed")
	}
	for _, candidate := range []string{databasePath, databasePath + "-wal", databasePath + "-shm"} {
		if _, err := os.Stat(candidate); !os.IsNotExist(err) {
			t.Fatalf("legacy tracking database artifact remains %q: %v", candidate, err)
		}
	}
}
