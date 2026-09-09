package main

import (
	"archive/zip"
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/url"
	"os"
	"os/exec"
	"os/signal"
	"path"
	"path/filepath"
	"regexp"
	"runtime"
	"sort"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"syscall"
	"time"

	"github.com/BurntSushi/toml"
)

const (
	gortexExecutableName     = "gortex.exe"
	gortexMCPName            = "gortex"
	gortexCommandTimeout     = 30 * time.Second
	gortexDaemonTimeout      = 2 * time.Minute
	gortexProcessStopTimeout = 15 * time.Second
	gortexTrackTimeout       = 35 * time.Minute
	gortexUntrackTimeout     = 10 * time.Minute
	gortexGitHubReleasesURL  = "https://api.github.com/repos/zzet/gortex/releases"
	gortexReleasePageSize    = 5
	gortexWindowsAssetName   = "gortex_windows_amd64.zip"
	gortexOwnershipFileName  = "mcp-ownership.json"
)

var gortexTagPattern = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._/-]*$`)
var gortexChecksumPattern = regexp.MustCompile(`(?i)[a-f0-9]{64}`)

type gortexStatusResponse struct {
	Path                      string   `json:"path"`
	ManagedRoot               string   `json:"managed_root"`
	ManagedRootExists         bool     `json:"managed_root_exists"`
	Version                   string   `json:"version"`
	Installed                 bool     `json:"installed"`
	ManagedInstalled          bool     `json:"managed_installed"`
	Installing                bool     `json:"installing"`
	Running                   bool     `json:"running"`
	AnyProcessRunning         bool     `json:"any_process_running"`
	ProcessID                 int      `json:"process_id,omitempty"`
	ActivationState           string   `json:"activation_state"`
	CodexAvailable            bool     `json:"codex_available"`
	CodexConfigured           bool     `json:"codex_configured"`
	CodexComplete             bool     `json:"codex_complete"`
	ClaudeAvailable           bool     `json:"claude_available"`
	ClaudeConfigured          bool     `json:"claude_configured"`
	ClaudeComplete            bool     `json:"claude_complete"`
	CursorAvailable           bool     `json:"cursor_available"`
	CursorConfigured          bool     `json:"cursor_configured"`
	CursorComplete            bool     `json:"cursor_complete"`
	CopilotAvailable          bool     `json:"copilot_available"`
	CopilotConfigured         bool     `json:"copilot_configured"`
	CopilotComplete           bool     `json:"copilot_complete"`
	OpenCodeAvailable         bool     `json:"opencode_available"`
	OpenCodeConfigured        bool     `json:"opencode_configured"`
	OpenCodeComplete          bool     `json:"opencode_complete"`
	OpenCodeHook              bool     `json:"opencode_hook"`
	OpenCodeHookComplete      bool     `json:"opencode_hook_complete"`
	AntigravityAvailable      bool     `json:"antigravity_available"`
	AntigravityConfigured     bool     `json:"antigravity_configured"`
	AntigravityComplete       bool     `json:"antigravity_complete"`
	GeminiAvailable           bool     `json:"gemini_available"`
	GeminiConfigured          bool     `json:"gemini_configured"`
	GeminiComplete            bool     `json:"gemini_complete"`
	CodexPrompt               bool     `json:"codex_prompt"`
	CodexPromptComplete       bool     `json:"codex_prompt_complete"`
	ClaudePrompt              bool     `json:"claude_prompt"`
	ClaudePromptComplete      bool     `json:"claude_prompt_complete"`
	CursorPrompt              bool     `json:"cursor_prompt"`
	CursorPromptComplete      bool     `json:"cursor_prompt_complete"`
	CopilotPrompt             bool     `json:"copilot_prompt"`
	CopilotPromptComplete     bool     `json:"copilot_prompt_complete"`
	OpenCodePrompt            bool     `json:"opencode_prompt"`
	OpenCodePromptComplete    bool     `json:"opencode_prompt_complete"`
	AntigravityPrompt         bool     `json:"antigravity_prompt"`
	AntigravityPromptComplete bool     `json:"antigravity_prompt_complete"`
	GeminiPrompt              bool     `json:"gemini_prompt"`
	GeminiPromptComplete      bool     `json:"gemini_prompt_complete"`
	CodexHook                 bool     `json:"codex_hook"`
	CodexHookComplete         bool     `json:"codex_hook_complete"`
	ClaudeHook                bool     `json:"claude_hook"`
	ClaudeHookComplete        bool     `json:"claude_hook_complete"`
	CopilotHook               bool     `json:"copilot_hook"`
	CopilotHookComplete       bool     `json:"copilot_hook_complete"`
	AntigravityHook           bool     `json:"antigravity_hook"`
	AntigravityHookComplete   bool     `json:"antigravity_hook_complete"`
	GeminiHook                bool     `json:"gemini_hook"`
	GeminiHookComplete        bool     `json:"gemini_hook_complete"`
	UserPath                  bool     `json:"user_path"`
	SystemPath                bool     `json:"system_path"`
	UnmanagedProcessRunning   bool     `json:"unmanaged_process_running"`
	CodexTrustStatus          string   `json:"codex_trust_status,omitempty"`
	CodexTrustRequired        bool     `json:"codex_trust_required"`
	CodexTrustNotice          string   `json:"codex_trust_notice,omitempty"`
	CodexTrustSteps           []string `json:"codex_trust_steps,omitempty"`
	TrackedProjects           []string `json:"tracked_projects"`
	ProjectMCPEnabled         bool     `json:"project_mcp_enabled"`
	ProjectMCPProjects        []string `json:"project_mcp_projects,omitempty"`
	DefaultProject            string   `json:"default_project"`
	IntegrationPresent        bool     `json:"integration_present"`
	Message                   string   `json:"message"`
}

type gortexOperationRequest struct {
	Path string `json:"path"`
}
type gortexOperationResponse struct {
	Message  string   `json:"message"`
	Warnings []string `json:"warnings,omitempty"`
}
type gortexReleaseOption struct {
	TagName     string `json:"tag_name"`
	Name        string `json:"name"`
	PublishedAt string `json:"published_at"`
	Prerelease  bool   `json:"prerelease"`
	Available   bool   `json:"available"`
	AssetName   string `json:"asset_name,omitempty"`
}
type gortexReleaseListResponse struct {
	Releases []gortexReleaseOption `json:"releases"`
	Page     int                   `json:"page"`
	PerPage  int                   `json:"per_page"`
	HasMore  bool                  `json:"has_more"`
}
type gortexInstallRequest struct {
	TagName string `json:"tag_name"`
}
type gortexProjectRegistry struct {
	Projects []string `json:"projects"`
}
type gortexOwnedMCP struct {
	Fingerprint string `json:"fingerprint"`
}
type gortexMCPOwnership struct {
	Platforms         map[string]gortexOwnedMCP        `json:"platforms"`
	ProjectMCP        map[string]gortexOwnedProjectMCP `json:"project_mcp,omitempty"`
	ProjectMCPEnabled bool                             `json:"project_mcp_enabled,omitempty"`
	Artifacts         map[string]gortexOwnedArtifact   `json:"artifacts,omitempty"`
	UserPath          bool                             `json:"user_path,omitempty"`
	SystemPath        bool                             `json:"system_path,omitempty"`
}

var gortexMCPStatusCache = struct {
	sync.Mutex
	at       time.Time
	values   map[string]bool
	versions map[string]string
}{values: map[string]bool{}, versions: map[string]string{}}

var gortexInstallInProgress atomic.Bool

func gortexInstallRoot() string {
	exe, err := os.Executable()
	if err != nil {
		return ""
	}
	return filepath.Join(filepath.Dir(exe), "Gortex")
}
func gortexManagedPath(name ...string) string {
	root := gortexInstallRoot()
	if root == "" {
		return ""
	}
	return filepath.Join(append([]string{root}, name...)...)
}
func setEnvValue(env []string, key, value string) []string {
	prefix := key + "="
	for i, item := range env {
		if strings.HasPrefix(item, prefix) {
			env[i] = prefix + value
			return env
		}
	}
	return append(env, prefix+value)
}
func gortexManagedEnv() []string {
	root := gortexInstallRoot()
	if root == "" {
		return os.Environ()
	}
	values := map[string]string{"XDG_CONFIG_HOME": filepath.Join(root, "config"), "XDG_DATA_HOME": filepath.Join(root, "data"), "XDG_CACHE_HOME": filepath.Join(root, "cache"), "GORTEX_DAEMON_SOCKET": filepath.Join(root, "run", "daemon.sock"), "GORTEX_DAEMON_PIDFILE": filepath.Join(root, "run", "daemon.pid"), "GORTEX_DAEMON_LOGFILE": filepath.Join(root, "run", "daemon.log"), "GORTEX_DAEMON_STATEFILE": filepath.Join(root, "run", "daemon.state.json"), "GORTEX_RECONCILE_INTERVAL": "1h", "GORTEX_DAEMON_IDLE_TIMEOUT": "0"}
	env := os.Environ()
	for key, value := range values {
		env = setEnvValue(env, key, value)
	}
	return env
}
func gortexMCPEnv() map[string]string {
	root := gortexInstallRoot()
	if root == "" {
		return map[string]string{}
	}
	return map[string]string{"XDG_CONFIG_HOME": filepath.Join(root, "config"), "XDG_DATA_HOME": filepath.Join(root, "data"), "XDG_CACHE_HOME": filepath.Join(root, "cache"), "GORTEX_DAEMON_SOCKET": filepath.Join(root, "run", "daemon.sock"), "GORTEX_DAEMON_PIDFILE": filepath.Join(root, "run", "daemon.pid"), "GORTEX_DAEMON_LOGFILE": filepath.Join(root, "run", "daemon.log"), "GORTEX_DAEMON_STATEFILE": filepath.Join(root, "run", "daemon.state.json"), "GORTEX_INDEX_WORKERS": "8", "GORTEX_RECONCILE_INTERVAL": "1h", "GORTEX_DAEMON_IDLE_TIMEOUT": "0"}
}

func uniqueCleanPaths(paths []string) []string {
	seen := map[string]struct{}{}
	result := []string{}
	for _, value := range paths {
		value = strings.TrimSpace(value)
		if value == "" {
			continue
		}
		absolute, err := filepath.Abs(value)
		if err != nil {
			continue
		}
		clean := filepath.Clean(absolute)
		key := strings.ToLower(clean)
		if _, ok := seen[key]; ok {
			continue
		}
		seen[key] = struct{}{}
		result = append(result, clean)
	}
	return result
}
func gortexExecutableCandidates() []string {
	paths := []string{}
	if p, err := exec.LookPath("gortex"); err == nil {
		paths = append(paths, p)
	}
	if p, err := exec.LookPath(gortexExecutableName); err == nil {
		paths = append(paths, p)
	}
	if local := strings.TrimSpace(os.Getenv("LOCALAPPDATA")); local != "" {
		paths = append(paths, filepath.Join(local, "Programs", "gortex", gortexExecutableName))
	}
	if profile := strings.TrimSpace(os.Getenv("USERPROFILE")); profile != "" {
		paths = append(paths, filepath.Join(profile, "bin", gortexExecutableName), filepath.Join(profile, ".local", "bin", gortexExecutableName))
	}
	if exe, err := os.Executable(); err == nil {
		base := filepath.Dir(exe)
		paths = append(paths, filepath.Join(base, "Gortex", "bin", gortexExecutableName), filepath.Join(base, "Gortex", gortexExecutableName), filepath.Join(base, gortexExecutableName))
	}
	return uniqueCleanPaths(paths)
}
func gortexExecutablePath() string {
	if managed := gortexManagedExecutablePath(); managed != "" {
		return managed
	}
	for _, candidate := range gortexExecutableCandidates() {
		if fileExists(candidate) {
			return candidate
		}
	}
	return ""
}

// gortexManagedExecutablePath is the only executable allowed to receive
// lifecycle commands from code-Manager. PATH and other conventional install
// locations are detection-only so an external Gortex installation cannot be
// stopped or removed accidentally.
func gortexManagedExecutablePath() string {
	managed := gortexManagedPath("bin", gortexExecutableName)
	if managed != "" && fileExists(managed) {
		return managed
	}
	return ""
}

func hideGortexCommandWindow(command *exec.Cmd) {
	if command != nil {
		command.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	}
}

func gortexDaemonSocketPath() string {
	return gortexManagedPath("run", "daemon.sock")
}

func gortexDaemonPIDPath() string {
	return gortexManagedPath("run", "daemon.pid")
}

func gortexDaemonPID() int {
	path := gortexDaemonPIDPath()
	if path == "" {
		return 0
	}
	data, err := os.ReadFile(path)
	if err != nil {
		return 0
	}
	pid, err := strconv.Atoi(strings.TrimSpace(string(data)))
	if err != nil || pid <= 0 {
		return 0
	}
	return pid
}

// gortexDaemonStatus checks the daemon's own IPC socket and PID file. MCP
// stdio processes use the same executable but are intentionally excluded.
func gortexDaemonStatus(executable string) (bool, int) {
	executable = strings.TrimSpace(executable)
	managedExecutable := gortexManagedExecutablePath()
	socketPath := gortexDaemonSocketPath()
	if executable == "" || managedExecutable == "" || !sameGortexExecutablePath(executable, managedExecutable) || socketPath == "" {
		return false, 0
	}
	connection, err := net.DialTimeout("unix", socketPath, 400*time.Millisecond)
	if err != nil {
		return false, 0
	}
	_ = connection.Close()

	pid := gortexDaemonPID()
	if pid == 0 {
		return true, 0
	}
	for _, process := range findRunningProcessesByNames(gortexExecutableName) {
		if process.ID == pid && sameGortexExecutablePath(process.Path, executable) {
			return true, pid
		}
	}
	// The socket is the authoritative daemon signal. A stale or briefly
	// delayed PID file must not hide a reachable daemon from the UI.
	return true, 0
}

func gortexProcessRunning(executable string) bool {
	running, _ := gortexDaemonStatus(executable)
	return running
}

func gortexAnyProcessRunning() bool {
	return len(findRunningProcessesByNames(gortexExecutableName)) > 0
}

// gortexUnmanagedProcessRunning reports an exact-name Gortex process whose
// image is not the currently installed managed binary. A managed MCP stdio
// process is intentionally not classified as unmanaged; both kinds still
// contribute to AnyProcessRunning and therefore block install/upgrade.
func gortexUnmanagedProcessRunning() bool {
	managed := gortexManagedExecutablePath()
	for _, process := range findRunningProcessesByNames(gortexExecutableName) {
		if managed == "" || !sameGortexExecutablePath(process.Path, managed) {
			return true
		}
	}
	return false
}

// stopAllGortexProcesses is used before install/upgrade and uninstall. It
// deliberately stops every exact-name gortex.exe process, including MCP clients from another
// installation or launcher, because the executable and its shared data are
// about to be removed.
func stopAllGortexProcesses(timeout time.Duration) error {
	deadline := time.Now().Add(timeout)
	var lastErr error
	for {
		if !gortexAnyProcessRunning() {
			return nil
		}
		if err := terminateProcessesByNames(gortexExecutableName); err != nil {
			lastErr = err
		}
		if timeout <= 0 || time.Now().After(deadline) {
			if lastErr != nil {
				return lastErr
			}
			return errors.New("Gortex 进程仍未退出")
		}
		time.Sleep(200 * time.Millisecond)
	}
}

func sameGortexExecutablePath(left, right string) bool {
	clean := func(value string) string {
		value = strings.TrimSpace(strings.ReplaceAll(value, "/", `\`))
		value = strings.TrimPrefix(value, `\\?\`)
		return strings.ToLower(filepath.Clean(value))
	}
	return clean(left) != "" && clean(left) == clean(right)
}

func waitForGortexProcessExit(executable string, timeout time.Duration) bool {
	deadline := time.Now().Add(timeout)
	for {
		if !gortexProcessRunning(executable) {
			return true
		}
		if timeout <= 0 || time.Now().After(deadline) {
			return !gortexProcessRunning(executable)
		}
		time.Sleep(200 * time.Millisecond)
	}
}

func waitForGortexProcessStart(executable string, timeout time.Duration) bool {
	deadline := time.Now().Add(timeout)
	for {
		if gortexProcessRunning(executable) {
			return true
		}
		if timeout <= 0 || time.Now().After(deadline) {
			return gortexProcessRunning(executable)
		}
		time.Sleep(200 * time.Millisecond)
	}
}
func gortexDefaultProject() string {
	wd, err := os.Getwd()
	if err != nil {
		return ""
	}
	absolute, err := filepath.Abs(wd)
	if err != nil {
		return ""
	}
	return filepath.Clean(absolute)
}
func userProfileDir() string {
	if profile := strings.TrimSpace(os.Getenv("USERPROFILE")); profile != "" {
		return profile
	}
	home, _ := os.UserHomeDir()
	return home
}

func gortexAbsoluteEnvPath(name string) string {
	raw := strings.TrimSpace(os.Getenv(name))
	if raw == "" {
		return ""
	}
	absolute, err := filepath.Abs(raw)
	if err != nil {
		return ""
	}
	return filepath.Clean(absolute)
}

func gortexClaudeConfigDir() string {
	if dir := gortexAbsoluteEnvPath("CLAUDE_CONFIG_DIR"); dir != "" {
		return dir
	}
	profile := userProfileDir()
	if profile == "" {
		return ""
	}
	return filepath.Join(profile, ".claude")
}

func gortexCopilotConfigDir() string {
	if dir := gortexAbsoluteEnvPath("COPILOT_HOME"); dir != "" {
		return dir
	}
	profile := userProfileDir()
	if profile == "" {
		return ""
	}
	return filepath.Join(profile, ".copilot")
}

func gortexOpenCodeConfigDir() string {
	if dir := gortexAbsoluteEnvPath("XDG_CONFIG_HOME"); dir != "" {
		return filepath.Join(dir, "opencode")
	}
	profile := userProfileDir()
	if profile == "" {
		return ""
	}
	return filepath.Join(profile, ".config", "opencode")
}

func gortexAntigravityConfigDir() string {
	profile := userProfileDir()
	if profile == "" {
		return ""
	}
	return filepath.Join(profile, ".gemini", "config")
}

func gortexGeminiConfigDir() string {
	profile := userProfileDir()
	if profile == "" {
		return ""
	}
	return filepath.Join(profile, ".gemini")
}

// gortexLegacyAntigravityConfigPath is the path used by older code-Manager
// builds. It is retained only for ownership-checked cleanup during migration.
func gortexLegacyAntigravityConfigPath() string {
	profile := userProfileDir()
	if profile == "" {
		return ""
	}
	return filepath.Join(profile, ".gemini", "antigravity", "mcp_config.json")
}

func gortexAntigravityExecutableCandidates() []string {
	if runtime.GOOS != "windows" {
		return nil
	}
	paths := []string{}
	if local := strings.TrimSpace(os.Getenv("LOCALAPPDATA")); local != "" {
		paths = append(paths, filepath.Join(local, "Programs", "antigravity", "Antigravity.exe"))
	}
	if programFiles := strings.TrimSpace(os.Getenv("ProgramFiles")); programFiles != "" {
		paths = append(paths, filepath.Join(programFiles, "Antigravity", "Antigravity.exe"))
	}
	return uniqueCleanPaths(paths)
}

func gortexAntigravityInstalled() bool {
	if gortexCommandAvailable("antigravity") {
		return true
	}
	for _, path := range gortexAntigravityExecutableCandidates() {
		if fileExists(path) {
			return true
		}
	}
	return false
}

func fileExists(path string) bool      { info, err := os.Stat(path); return err == nil && !info.IsDir() }
func directoryExists(path string) bool { info, err := os.Stat(path); return err == nil && info.IsDir() }

func gortexProjectRegistryPath() (string, error) {
	if managed := gortexManagedPath("config", "projects.json"); managed != "" {
		return managed, nil
	}
	config, err := os.UserConfigDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(config, "code-Manager", "gortex-projects.json"), nil
}

func gortexLegacyProjectRegistryPath() (string, error) {
	config, err := os.UserConfigDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(config, "code-Manager", "gortex-projects.json"), nil
}
func normalizeGortexProjects(projects []string) []string {
	seen := map[string]string{}
	for _, project := range projects {
		project = strings.TrimSpace(project)
		if project == "" {
			continue
		}
		absolute, err := filepath.Abs(project)
		if err != nil {
			continue
		}
		clean := filepath.Clean(absolute)
		seen[strings.ToLower(clean)] = clean
	}
	result := make([]string, 0, len(seen))
	for _, project := range seen {
		result = append(result, project)
	}
	sort.Strings(result)
	return result
}
func readGortexProjectRegistry() (gortexProjectRegistry, error) {
	result := gortexProjectRegistry{Projects: []string{}}
	path, err := gortexProjectRegistryPath()
	if err != nil {
		return result, err
	}
	data, err := os.ReadFile(path)
	if errors.Is(err, os.ErrNotExist) {
		// Migrate the pre-existing user-config registry on first read, but
		// keep all subsequent writes inside the managed Gortex tree.
		if legacy, legacyErr := gortexLegacyProjectRegistryPath(); legacyErr == nil && legacy != path {
			if legacyData, readErr := os.ReadFile(legacy); readErr == nil {
				data = legacyData
				goto decode
			}
		}
		return result, nil
	}
	if err != nil {
		return result, err
	}
decode:
	if err := json.Unmarshal(data, &result); err != nil {
		return result, err
	}
	result.Projects = normalizeGortexProjects(result.Projects)
	return result, nil
}
func writeGortexProjectRegistry(registry gortexProjectRegistry) error {
	path, err := gortexProjectRegistryPath()
	if err != nil {
		return err
	}
	registry.Projects = normalizeGortexProjects(registry.Projects)
	data, err := json.MarshalIndent(registry, "", "  ")
	if err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return err
	}
	return replaceUTF8File(path, append(data, '\n'))
}
func gortexTrackedProjects() ([]string, error) {
	registry, err := readGortexProjectRegistry()
	return registry.Projects, err
}

func gortexCommandAvailable(name string) bool { _, err := exec.LookPath(name); return err == nil }
func runExternalCommand(ctx context.Context, executable string, args ...string) (string, error) {
	cmd := exec.CommandContext(ctx, executable, args...)
	hideGortexCommandWindow(cmd)
	output, err := cmd.CombinedOutput()
	text := strings.TrimSpace(string(output))
	if err != nil {
		if ctx.Err() != nil {
			return text, ctx.Err()
		}
		if text != "" {
			return text, fmt.Errorf("%w: %s", err, text)
		}
		return text, err
	}
	return text, nil
}
func runGortexCommand(executable string, args ...string) (string, error) {
	return runGortexCommandWithTimeout(gortexCommandTimeout, executable, args...)
}
func runGortexCommandWithTimeout(timeout time.Duration, executable string, args ...string) (string, error) {
	ctx, cancel := context.WithTimeout(context.Background(), timeout)
	defer cancel()
	cmd := exec.CommandContext(ctx, executable, args...)
	cmd.Env = gortexManagedEnv()
	hideGortexCommandWindow(cmd)
	output, err := cmd.CombinedOutput()
	text := strings.TrimSpace(string(output))
	if err != nil {
		if ctx.Err() != nil {
			return text, ctx.Err()
		}
		if text != "" {
			return text, fmt.Errorf("%w: %s", err, text)
		}
		return text, err
	}
	return text, nil
}
func queryGortexVersion(executable string) string {
	if executable == "" {
		return ""
	}
	key := strings.ToLower(filepath.Clean(executable))
	gortexMCPStatusCache.Lock()
	if time.Since(gortexMCPStatusCache.at) < 5*time.Second {
		if version, ok := gortexMCPStatusCache.versions[key]; ok {
			gortexMCPStatusCache.Unlock()
			return version
		}
	}
	gortexMCPStatusCache.Unlock()
	output, err := runGortexCommand(executable, "version")
	if err != nil {
		gortexMCPStatusCache.Lock()
		gortexMCPStatusCache.versions[key] = ""
		gortexMCPStatusCache.at = time.Now()
		gortexMCPStatusCache.Unlock()
		return ""
	}
	for _, line := range strings.Split(output, "\n") {
		if line = strings.TrimSpace(line); line != "" {
			gortexMCPStatusCache.Lock()
			gortexMCPStatusCache.versions[key] = line
			gortexMCPStatusCache.at = time.Now()
			gortexMCPStatusCache.Unlock()
			return line
		}
	}
	gortexMCPStatusCache.Lock()
	gortexMCPStatusCache.versions[key] = ""
	gortexMCPStatusCache.at = time.Now()
	gortexMCPStatusCache.Unlock()
	return ""
}
func invalidateGortexMCPStatusCache() {
	gortexMCPStatusCache.Lock()
	defer gortexMCPStatusCache.Unlock()
	gortexMCPStatusCache.at = time.Time{}
	gortexMCPStatusCache.values = map[string]bool{}
	gortexMCPStatusCache.versions = map[string]string{}
}

func gortexConfigPath(agent string) string {
	profile := userProfileDir()
	if profile == "" {
		return ""
	}
	switch agent {
	case "codex":
		return filepath.Join(profile, ".codex", "config.toml")
	case "claude":
		if override := gortexAbsoluteEnvPath("CLAUDE_CONFIG_DIR"); override != "" {
			return filepath.Join(override, ".claude.json")
		}
		return filepath.Join(profile, ".claude.json")
	case "cursor":
		return filepath.Join(profile, ".cursor", "mcp.json")
	case "copilot":
		if dir := gortexCopilotConfigDir(); dir != "" {
			return filepath.Join(dir, "mcp-config.json")
		}
		return ""
	case "opencode":
		if dir := gortexOpenCodeConfigDir(); dir != "" {
			return filepath.Join(dir, "opencode.json")
		}
		return ""
	case "antigravity":
		if dir := gortexAntigravityConfigDir(); dir != "" {
			return filepath.Join(dir, "mcp_config.json")
		}
		return ""
	case "gemini":
		if dir := gortexGeminiConfigDir(); dir != "" {
			return filepath.Join(dir, "settings.json")
		}
		return ""
	}
	return ""
}

// gortexJSONConfigHasPlatformEvidence avoids treating a directory or a file
// containing only Gortex's own MCP entry as proof that the host is installed.
// The latter state is common after removing Gortex MCP from an otherwise empty
// configuration file.
func gortexJSONConfigHasPlatformEvidence(path, serverKey string) bool {
	if !fileExists(path) {
		return false
	}
	root, err := readGortexJSONObject(path)
	if err != nil || len(root) == 0 {
		return false
	}
	for key, value := range root {
		if key != serverKey {
			return true
		}
		servers, ok := value.(map[string]any)
		if !ok {
			return false
		}
		for name := range servers {
			if name != gortexMCPName {
				return true
			}
		}
	}
	return false
}

// Gemini CLI and Antigravity share ~/.gemini/settings.json for lifecycle
// hooks. A hooks-only file therefore proves that Gortex configured a host,
// not that Gemini CLI itself is installed. Ignore the shared hooks key while
// retaining ordinary user settings and non-Gortex MCP servers as evidence.
func gortexGeminiConfigHasPlatformEvidence(path string) bool {
	if !fileExists(path) {
		return false
	}
	root, err := readGortexJSONObject(path)
	if err != nil || len(root) == 0 {
		return false
	}
	for key, value := range root {
		switch key {
		case "hooks":
			continue
		case "mcpServers":
			servers, ok := value.(map[string]any)
			if !ok {
				return false
			}
			for name := range servers {
				if name != gortexMCPName {
					return true
				}
			}
		default:
			return true
		}
	}
	return false
}

func gortexTOMLConfigHasPlatformEvidence(path, serverKey string) bool {
	if !fileExists(path) {
		return false
	}
	data, err := os.ReadFile(path)
	if err != nil || len(bytes.TrimSpace(data)) == 0 {
		return false
	}
	root := map[string]any{}
	if _, err := toml.Decode(string(data), &root); err != nil || len(root) == 0 {
		return false
	}
	for key, value := range root {
		if key != serverKey {
			return true
		}
		servers, ok := value.(map[string]any)
		if !ok {
			return false
		}
		for name := range servers {
			if name != gortexMCPName {
				return true
			}
		}
	}
	return false
}

func gortexAgentConfigEvidence(agent string) bool {
	switch agent {
	case "codex":
		return gortexTOMLConfigHasPlatformEvidence(gortexConfigPath(agent), "mcp_servers")
	case "opencode":
		return gortexJSONConfigHasPlatformEvidence(gortexConfigPath(agent), "mcp")
	case "antigravity":
		return gortexJSONConfigHasPlatformEvidence(gortexConfigPath(agent), "mcpServers") ||
			gortexJSONConfigHasPlatformEvidence(gortexLegacyAntigravityConfigPath(), "mcpServers")
	default:
		return gortexJSONConfigHasPlatformEvidence(gortexConfigPath(agent), "mcpServers")
	}
}

func gortexAgentAvailable(agent string) bool {
	switch agent {
	case "codex":
		return gortexCommandAvailable("codex") || gortexAgentConfigEvidence(agent)
	case "claude":
		return gortexCommandAvailable("claude") || gortexAgentConfigEvidence(agent)
	case "cursor":
		return gortexCommandAvailable("cursor") || gortexAgentConfigEvidence(agent)
	case "copilot":
		return gortexCommandAvailable("copilot") || gortexAgentConfigEvidence(agent)
	case "opencode":
		return gortexCommandAvailable("opencode") || gortexAgentConfigEvidence(agent)
	case "antigravity":
		return gortexAntigravityInstalled() || gortexAgentConfigEvidence(agent)
	case "gemini":
		return gortexCommandAvailable("gemini") || gortexGeminiConfigHasPlatformEvidence(gortexConfigPath(agent))
	}
	return false
}
func readGortexJSONObject(path string) (map[string]any, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	if strings.TrimSpace(string(data)) == "" {
		return map[string]any{}, nil
	}
	root := map[string]any{}
	if err := json.Unmarshal(data, &root); err != nil {
		return nil, fmt.Errorf("解析 %s 失败: %w", path, err)
	}
	return root, nil
}
func writeGortexJSONObject(path string, root map[string]any) error {
	data, err := json.MarshalIndent(root, "", "  ")
	if err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return err
	}
	return replaceUTF8File(path, append(data, '\n'))
}
func gortexMCPEntry(executable string, copilot bool) map[string]any {
	entry := map[string]any{"command": executable, "args": []string{"mcp"}, "env": gortexMCPEnv()}
	if copilot {
		entry["type"] = "local"
	}
	return entry
}

func gortexActiveProjectForAgent(agent, executable string) string {
	projects, _ := gortexMCPProjectsForRegistration(executable)
	for i := len(projects) - 1; i >= 0; i-- {
		p := filepath.Clean(projects[i])
		if directoryExists(p) {
			return p
		}
	}
	return ""
}

func gortexBridgeExecutable(fallbackExecutable string) string {
	if override := strings.TrimSpace(os.Getenv("CODE_MANAGER_EXECUTABLE")); override != "" {
		return filepath.Clean(override)
	}
	if exe, err := currentCodeManagerExecutable(); err == nil && exe != "" {
		return exe
	}
	return fallbackExecutable
}

func gortexAntigravityMCPEntry(executable string) map[string]any {
	bridgeExe := gortexBridgeExecutable(executable)
	return map[string]any{
		"command": bridgeExe,
		"args":    []string{"gortex-bridge", "--gortex", executable},
		"env":     gortexMCPEnv(),
	}
}

func gortexPlatformMCPEntry(agent, executable string) map[string]any {
	if agent == "antigravity" {
		return gortexAntigravityMCPEntry(executable)
	}
	return gortexMCPEntry(executable, agent == "copilot")
}

func gortexAgentMCPEntryComplete(existing any, agent, executable string) bool {
	if executable == "" {
		return false
	}
	if agent == "antigravity" || agent == "cursor" {
		return gortexFingerprint(existing) == gortexFingerprint(gortexPlatformMCPEntry(agent, executable))
	}
	return gortexMCPEntryComplete(existing, executable, agent == "copilot")
}


func gortexOpenCodeMCPEntry(executable string) map[string]any {
	return map[string]any{
		"type":        "local",
		"command":     []string{executable, "mcp"},
		"enabled":     true,
		"environment": gortexMCPEnv(),
	}
}

func gortexOpenCodeMCPEntryLooksManaged(value any) bool {
	entry, ok := value.(map[string]any)
	if !ok {
		return false
	}
	var command []string
	switch value := entry["command"].(type) {
	case []string:
		command = value
	case []any:
		for _, item := range value {
			text, ok := item.(string)
			if !ok {
				return false
			}
			command = append(command, text)
		}
	default:
		return false
	}
	if len(command) < 2 || command[1] != "mcp" {
		return false
	}
	return strings.TrimSuffix(strings.ToLower(filepath.Base(strings.ReplaceAll(command[0], "/", `\\`))), ".exe") == "gortex"
}

func gortexOpenCodeMCPEntryComplete(value any, executable string) bool {
	return executable != "" && gortexFingerprint(value) == gortexFingerprint(gortexOpenCodeMCPEntry(executable))
}

func gortexCodexMCPEntry(executable string) map[string]any {
	entry := gortexMCPEntry(executable, false)
	entry["startup_timeout_sec"] = 90
	return entry
}
func canonicalJSONValue(value any) any {
	switch typed := value.(type) {
	case map[string]any:
		result := map[string]any{}
		for key, item := range typed {
			result[key] = canonicalJSONValue(item)
		}
		return result
	case map[interface{}]interface{}:
		result := map[string]any{}
		for key, item := range typed {
			result[fmt.Sprint(key)] = canonicalJSONValue(item)
		}
		return result
	case []interface{}:
		result := make([]any, len(typed))
		for i, item := range typed {
			result[i] = canonicalJSONValue(item)
		}
		return result
	case []string:
		result := make([]any, len(typed))
		for i, item := range typed {
			result[i] = item
		}
		return result
	default:
		return value
	}
}
func gortexFingerprint(value any) string {
	data, err := json.Marshal(canonicalJSONValue(value))
	if err != nil {
		return ""
	}
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:])
}
func gortexOwnershipPath() string { return gortexManagedPath("config", gortexOwnershipFileName) }
func readGortexOwnership() (gortexMCPOwnership, error) {
	result := gortexMCPOwnership{Platforms: map[string]gortexOwnedMCP{}, ProjectMCP: map[string]gortexOwnedProjectMCP{}, Artifacts: map[string]gortexOwnedArtifact{}}
	path := gortexOwnershipPath()
	if path == "" {
		return result, nil
	}
	data, err := os.ReadFile(path)
	if errors.Is(err, os.ErrNotExist) {
		return result, nil
	}
	if err != nil {
		return result, err
	}
	if err := json.Unmarshal(data, &result); err != nil {
		return result, err
	}
	if result.Platforms == nil {
		result.Platforms = map[string]gortexOwnedMCP{}
	}
	if result.ProjectMCP == nil {
		result.ProjectMCP = map[string]gortexOwnedProjectMCP{}
	}
	if result.Artifacts == nil {
		result.Artifacts = map[string]gortexOwnedArtifact{}
	}
	return result, nil
}
func writeGortexOwnership(value gortexMCPOwnership) error {
	if value.Platforms == nil {
		value.Platforms = map[string]gortexOwnedMCP{}
	}
	if value.ProjectMCP == nil {
		value.ProjectMCP = map[string]gortexOwnedProjectMCP{}
	}
	if value.Artifacts == nil {
		value.Artifacts = map[string]gortexOwnedArtifact{}
	}
	path := gortexOwnershipPath()
	if path == "" {
		return nil
	}
	if len(value.Platforms) == 0 && len(value.ProjectMCP) == 0 && !value.ProjectMCPEnabled && len(value.Artifacts) == 0 && !value.UserPath && !value.SystemPath {
		_ = os.Remove(path)
		return nil
	}
	data, err := json.MarshalIndent(value, "", "  ")
	if err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return err
	}
	return replaceUTF8File(path, append(data, '\n'))
}
func gortexOwnershipKey(agent, path string) string {
	return agent + "|" + strings.ToLower(filepath.Clean(path))
}

func gortexMCPRegistrationAllowed(existing, desired any, owned gortexOwnedMCP) bool {
	existingFingerprint := gortexFingerprint(existing)
	desiredFingerprint := gortexFingerprint(desired)
	if existingFingerprint == desiredFingerprint {
		return true
	}
	// An entry we previously owned may be refreshed when it still matches
	// the last fingerprint we recorded (for example after a binary update).
	// Any other mismatch means the user edited or replaced it; preserve it.
	if owned.Fingerprint != "" && existingFingerprint == owned.Fingerprint {
		return true
	}
	// A stale or incomplete entry may not have an ownership record (for
	// example after an older code-Manager build or a manual path change).
	// The command/args shape is the safe identity signal: repair only entries
	// that still clearly launch Gortex MCP, never an arbitrary user wrapper.
	return gortexMCPEntryLooksManaged(existing)
}

func gortexMCPEntryLooksManaged(value any) bool {
	entry, ok := value.(map[string]any)
	if !ok {
		return false
	}
	command, _ := entry["command"].(string)
	base := strings.TrimSuffix(strings.ToLower(filepath.Base(strings.ReplaceAll(command, "/", `\`))), ".exe")
	args, ok := entry["args"].([]any)
	var first string
	if ok && len(args) > 0 {
		first, _ = args[0].(string)
	} else if stringArgs, stringOK := entry["args"].([]string); stringOK && len(stringArgs) > 0 {
		first = stringArgs[0]
	}
	if first == "gortex-bridge" {
		return true
	}
	if base == "gortex" && first == "mcp" {
		return true
	}
	if (strings.Contains(base, "code-manager") || strings.Contains(base, "llmtrim")) && (first == "gortex-bridge" || first == "mcp") {
		return true
	}
	return false
}

func gortexMCPEntryComplete(value any, executable string, copilot bool) bool {
	return executable != "" && gortexFingerprint(value) == gortexFingerprint(gortexMCPEntry(executable, copilot))
}

func jsonServers(root map[string]any, path string) (map[string]any, error) {
	value, exists := root["mcpServers"]
	if !exists {
		servers := map[string]any{}
		root["mcpServers"] = servers
		return servers, nil
	}
	servers, ok := value.(map[string]any)
	if !ok {
		return nil, fmt.Errorf("%s 的 mcpServers 不是对象，拒绝覆盖用户配置", path)
	}
	return servers, nil
}
func updateJSONMCPConfig(agent, executable string, remove bool) error {
	_, err := updateJSONMCPConfigOwned(agent, executable, remove)
	return err
}
func updateJSONMCPConfigOwned(agent, executable string, remove bool) (bool, error) {
	path := gortexConfigPath(agent)
	if path == "" {
		return false, nil
	}
	root := map[string]any{}
	if _, err := os.Stat(path); err == nil {
		root, err = readGortexJSONObject(path)
		if err != nil {
			return false, err
		}
	} else if errors.Is(err, os.ErrNotExist) {
		// A missing platform file can still have a stale ownership record. Keep
		// the empty root in memory so removal below can clear only that record.
		if !remove {
			// Registration will create the platform file normally.
			root = map[string]any{}
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return false, err
	}
	servers, err := jsonServers(root, path)
	if err != nil {
		return false, err
	}
	ownership, ownershipErr := readGortexOwnership()
	if ownershipErr != nil {
		if !remove {
			return false, ownershipErr
		}
		// Removal can still be performed from the MCP entry's safe shape when
		// the ledger is damaged; never overwrite the damaged ledger here.
		ownership = gortexMCPOwnership{Platforms: map[string]gortexOwnedMCP{}, Artifacts: map[string]gortexOwnedArtifact{}}
	}
	key := gortexOwnershipKey(agent, path)
	existing, exists := servers[gortexMCPName]
	if remove {
		if !exists {
			if ownershipErr == nil {
				if _, stale := ownership.Platforms[key]; stale {
					delete(ownership.Platforms, key)
					if err := writeGortexOwnership(ownership); err != nil {
						return false, err
					}
					return true, nil
				}
			}
			return false, nil
		}
		owned := ownership.Platforms[key]
		if (owned.Fingerprint == "" || owned.Fingerprint != gortexFingerprint(existing)) && !gortexMCPEntryLooksManaged(existing) {
			return false, fmt.Errorf("%s 的 gortex MCP 已被用户修改，已保留", agent)
		}
		delete(servers, gortexMCPName)
		delete(ownership.Platforms, key)
	} else {
		entry := gortexPlatformMCPEntry(agent, executable)
		if exists {
			owned := ownership.Platforms[key]
			if !gortexMCPRegistrationAllowed(existing, entry, owned) {
				return false, fmt.Errorf("%s 已存在用户配置的 gortex MCP，已保留", agent)
			}
		}
		servers[gortexMCPName] = entry
		ownership.Platforms[key] = gortexOwnedMCP{Fingerprint: gortexFingerprint(entry)}
	}
	if len(servers) == 0 {
		delete(root, "mcpServers")
	}
	if err := writeGortexJSONObject(path, root); err != nil {
		return false, err
	}
	if ownershipErr == nil {
		if err := writeGortexOwnership(ownership); err != nil {
			return false, err
		}
	}
	return true, nil
}

// removeLegacyAntigravityMCP clears only entries owned by older code-Manager
// releases at Antigravity's former MCP path. It never infers ownership from a command.
func removeLegacyAntigravityMCP() (bool, error) {
	path := gortexLegacyAntigravityConfigPath()
	if path == "" {
		return false, nil
	}
	ownership, err := readGortexOwnership()
	if err != nil {
		return false, err
	}
	key := gortexOwnershipKey("antigravity", path)
	owned, recorded := ownership.Platforms[key]
	if !recorded {
		return false, nil
	}
	root, err := readGortexJSONObject(path)
	if errors.Is(err, os.ErrNotExist) {
		delete(ownership.Platforms, key)
		return true, writeGortexOwnership(ownership)
	}
	if err != nil {
		return false, err
	}
	servers, err := jsonServers(root, path)
	if err != nil {
		return false, err
	}
	existing, present := servers[gortexMCPName]
	if !present {
		delete(ownership.Platforms, key)
		return true, writeGortexOwnership(ownership)
	}
	if owned.Fingerprint == "" || owned.Fingerprint != gortexFingerprint(existing) {
		return false, errors.New("旧 Antigravity MCP 已被用户修改，已保留")
	}
	delete(servers, gortexMCPName)
	if len(servers) == 0 {
		delete(root, "mcpServers")
	}
	if err := writeGortexJSONObject(path, root); err != nil {
		return false, err
	}
	delete(ownership.Platforms, key)
	if err := writeGortexOwnership(ownership); err != nil {
		return false, err
	}
	return true, nil
}

func updateOpenCodeMCPConfigOwned(executable string, remove bool) (bool, error) {
	path := gortexConfigPath("opencode")
	if path == "" {
		return false, nil
	}
	root := map[string]any{}
	if _, err := os.Stat(path); err == nil {
		var err error
		root, err = readGortexJSONObject(path)
		if err != nil {
			return false, err
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return false, err
	}
	servers := map[string]any{}
	if value, exists := root["mcp"]; exists {
		var ok bool
		servers, ok = value.(map[string]any)
		if !ok {
			return false, fmt.Errorf("%s 的 mcp 不是对象，拒绝覆盖用户配置", path)
		}
	}
	ownership, ownershipErr := readGortexOwnership()
	if ownershipErr != nil {
		if !remove {
			return false, ownershipErr
		}
		ownership = gortexMCPOwnership{Platforms: map[string]gortexOwnedMCP{}, Artifacts: map[string]gortexOwnedArtifact{}}
	}
	key := gortexOwnershipKey("opencode", path)
	existing, exists := servers[gortexMCPName]
	if remove {
		if !exists {
			if ownershipErr == nil {
				if _, stale := ownership.Platforms[key]; stale {
					delete(ownership.Platforms, key)
					if err := writeGortexOwnership(ownership); err != nil {
						return false, err
					}
					return true, nil
				}
			}
			return false, nil
		}
		owned := ownership.Platforms[key]
		if (owned.Fingerprint == "" || owned.Fingerprint != gortexFingerprint(existing)) && !gortexOpenCodeMCPEntryLooksManaged(existing) {
			return false, errors.New("OpenCode 的 gortex MCP 已被用户修改，已保留")
		}
		delete(servers, gortexMCPName)
		delete(ownership.Platforms, key)
	} else {
		entry := gortexOpenCodeMCPEntry(executable)
		if exists {
			owned := ownership.Platforms[key]
			if !gortexMCPRegistrationAllowed(existing, entry, owned) && !gortexOpenCodeMCPEntryLooksManaged(existing) {
				return false, errors.New("OpenCode 已存在用户配置的 gortex MCP，已保留")
			}
		}
		servers[gortexMCPName] = entry
		ownership.Platforms[key] = gortexOwnedMCP{Fingerprint: gortexFingerprint(entry)}
	}
	if len(servers) == 0 {
		delete(root, "mcp")
	} else {
		root["mcp"] = servers
	}
	if err := writeGortexJSONObject(path, root); err != nil {
		return false, err
	}
	if ownershipErr == nil {
		if err := writeGortexOwnership(ownership); err != nil {
			return false, err
		}
	}
	return true, nil
}

func updateCodexMCPConfig(executable string, remove bool) (bool, error) {
	path := gortexConfigPath("codex")
	if path == "" {
		return false, nil
	}
	root := map[string]any{}
	if data, err := os.ReadFile(path); err == nil {
		if _, err := toml.Decode(string(data), &root); err != nil {
			return false, fmt.Errorf("解析 Codex config.toml 失败: %w", err)
		}
	} else if errors.Is(err, os.ErrNotExist) {
		// A missing Codex config may still have a stale ownership record; the
		// removal branch below clears only that record without recreating TOML.
		if !remove {
			root = map[string]any{}
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return false, err
	}
	servers := map[string]any{}
	if value, exists := root["mcp_servers"]; exists {
		var ok bool
		servers, ok = value.(map[string]any)
		if !ok {
			return false, errors.New("Codex mcp_servers 不是对象，拒绝覆盖用户配置")
		}
	}
	ownership, ownershipErr := readGortexOwnership()
	if ownershipErr != nil {
		if !remove {
			return false, ownershipErr
		}
		ownership = gortexMCPOwnership{Platforms: map[string]gortexOwnedMCP{}, Artifacts: map[string]gortexOwnedArtifact{}}
	}
	key := gortexOwnershipKey("codex", path)
	existing, exists := servers[gortexMCPName]
	if remove {
		if !exists {
			if ownershipErr == nil {
				if _, stale := ownership.Platforms[key]; stale {
					delete(ownership.Platforms, key)
					if err := writeGortexOwnership(ownership); err != nil {
						return false, err
					}
					return true, nil
				}
			}
			return false, nil
		}
		owned := ownership.Platforms[key]
		if (owned.Fingerprint == "" || owned.Fingerprint != gortexFingerprint(existing)) && !gortexMCPEntryLooksManaged(existing) {
			return false, errors.New("Codex 的 gortex MCP 已被用户修改，已保留")
		}
		delete(servers, gortexMCPName)
		delete(ownership.Platforms, key)
	} else {
		entry := gortexCodexMCPEntry(executable)
		if exists {
			owned := ownership.Platforms[key]
			if !gortexMCPRegistrationAllowed(existing, entry, owned) {
				return false, errors.New("Codex 已存在用户配置的 gortex MCP，已保留")
			}
		}
		servers[gortexMCPName] = entry
		ownership.Platforms[key] = gortexOwnedMCP{Fingerprint: gortexFingerprint(entry)}
	}
	if len(servers) == 0 {
		delete(root, "mcp_servers")
	} else {
		root["mcp_servers"] = servers
	}
	var encoded strings.Builder
	if err := toml.NewEncoder(&encoded).Encode(root); err != nil {
		return false, err
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return false, err
	}
	if err := replaceUTF8File(path, []byte(encoded.String())); err != nil {
		return false, err
	}
	if ownershipErr == nil {
		if err := writeGortexOwnership(ownership); err != nil {
			return false, err
		}
	}
	return true, nil
}
func queryJSONMCPState(agent string, executable string) (present, complete bool) {
	path := gortexConfigPath(agent)
	if !fileExists(path) {
		return false, false
	}
	root, err := readGortexJSONObject(path)
	if err != nil {
		return false, false
	}
	servers, ok := root["mcpServers"].(map[string]any)
	if !ok {
		return false, false
	}
	existing, ok := servers[gortexMCPName]
	if !ok {
		return false, false
	}
	return true, gortexAgentMCPEntryComplete(existing, agent, executable)
}

func queryOpenCodeMCPState(executable string) (present, complete bool) {
	path := gortexConfigPath("opencode")
	if !fileExists(path) {
		return false, false
	}
	root, err := readGortexJSONObject(path)
	if err != nil {
		return false, false
	}
	servers, ok := root["mcp"].(map[string]any)
	if !ok {
		return false, false
	}
	existing, ok := servers[gortexMCPName]
	if !ok {
		return false, false
	}
	return true, gortexOpenCodeMCPEntryComplete(existing, executable)
}

func queryCodexMCPState(executable string) (present, complete bool) {
	path := gortexConfigPath("codex")
	data, err := os.ReadFile(path)
	if err != nil {
		return false, false
	}
	root := map[string]any{}
	if _, err := toml.Decode(string(data), &root); err != nil {
		return false, false
	}
	servers, ok := root["mcp_servers"].(map[string]any)
	if !ok {
		return false, false
	}
	existing, ok := servers[gortexMCPName]
	if !ok {
		return false, false
	}
	return true, gortexFingerprint(existing) == gortexFingerprint(gortexCodexMCPEntry(executable))
}
func gortexRegisterMCP(executable string) []string {
	warnings := []string{}
	available := gortexDetectedMCPAgents()
	if available["codex"] {
		if _, err := updateCodexMCPConfig(executable, false); err != nil {
			warnings = append(warnings, "Codex: "+err.Error())
		}
	}
	for _, agent := range []string{"claude", "cursor", "copilot", "antigravity", "gemini"} {
		if available[agent] {
			if _, err := updateJSONMCPConfigOwned(agent, executable, false); err != nil {
				warnings = append(warnings, agent+": "+err.Error())
			}
		}
	}
	if _, err := removeLegacyAntigravityMCP(); err != nil {
		warnings = append(warnings, "Antigravity 旧配置: "+err.Error())
	}
	if available["opencode"] {
		if _, err := updateOpenCodeMCPConfigOwned(executable, false); err != nil {
			warnings = append(warnings, "OpenCode: "+err.Error())
		}
	}
	ownership, err := readGortexOwnership()
	if err != nil {
		warnings = append(warnings, "读取 Gortex 归属账本失败: "+err.Error())
	} else {
		artifacts, integrationWarnings := gortexRegisterIntegrations(executable, available)
		for _, warning := range integrationWarnings {
			warnings = append(warnings, warning)
		}
		if ownership.Artifacts == nil {
			ownership.Artifacts = map[string]gortexOwnedArtifact{}
		}
		for _, artifact := range artifacts {
			key := artifact.Kind + artifact.Agent + ":" + strings.ToLower(filepath.Clean(artifact.Path)) + ":" + artifact.Event
			ownership.Artifacts[key] = artifact
		}
		if err := writeGortexOwnership(ownership); err != nil {
			warnings = append(warnings, "保存 Gortex 接入归属失败: "+err.Error())
		}
	}
	warnings = append(warnings, gortexEnableAndRegisterProjectMCP(executable, available)...)
	return warnings
}
func gortexRemoveMCP() []string {
	warnings := []string{}
	warnings = append(warnings, gortexRemoveAllProjectMCP(gortexManagedExecutablePath())...)
	_, ownershipErr := readGortexOwnership()
	if ownershipErr != nil {
		warnings = append(warnings, "读取 Gortex 归属账本失败: "+ownershipErr.Error())
	}

	// Each MCP writer updates the ownership ledger independently. Remove MCP
	// entries first, then re-read the ledger before touching prompts/hooks so a
	// stale snapshot cannot restore platform records that were just deleted.
	if gortexConfigPath("codex") != "" {
		if _, err := updateCodexMCPConfig("", true); err != nil {
			warnings = append(warnings, "Codex: "+err.Error())
		}
	}
	for _, agent := range []string{"claude", "cursor", "copilot", "antigravity", "gemini"} {
		if gortexConfigPath(agent) != "" {
			if _, err := updateJSONMCPConfigOwned(agent, "", true); err != nil {
				warnings = append(warnings, agent+": "+err.Error())
			}
		}
	}
	if _, err := removeLegacyAntigravityMCP(); err != nil {
		warnings = append(warnings, "Antigravity 旧配置: "+err.Error())
	}
	if gortexConfigPath("opencode") != "" {
		if _, err := updateOpenCodeMCPConfigOwned("", true); err != nil {
			warnings = append(warnings, "OpenCode: "+err.Error())
		}
	}

	if ownershipErr != nil {
		// A damaged ledger must not strand marker-based integrations. The
		// individual removers only touch explicit Gortex markers/commands, and
		// they intentionally leave the damaged ledger untouched for recovery.
		warnings = append(warnings, gortexRemoveKnownIntegrations(gortexManagedExecutablePath())...)
		return warnings
	}

	latest, latestErr := readGortexOwnership()
	if latestErr != nil {
		warnings = append(warnings, "重新读取 Gortex 归属账本失败: "+latestErr.Error())
		// Do not overwrite a ledger that became unreadable during the platform
		// cleanup. A conservative marker-based pass is still safe to retry.
		warnings = append(warnings, gortexRemoveKnownIntegrations(gortexManagedExecutablePath())...)
		return warnings
	}
	warnings = append(warnings, gortexRemoveIntegrationsOwned(gortexManagedExecutablePath(), &latest)...)
	if err := writeGortexOwnership(latest); err != nil {
		warnings = append(warnings, "清理 Gortex 接入归属失败: "+err.Error())
	}
	return warnings
}

func gortexStatusSnapshot() gortexStatusResponse {
	exe := gortexExecutablePath()
	managedPath := gortexManagedPath("bin", gortexExecutableName)
	managedRoot := gortexInstallRoot()
	response := gortexStatusResponse{Path: exe, ManagedRoot: managedRoot, ManagedRootExists: directoryExists(managedRoot), Installed: exe != "", ManagedInstalled: fileExists(managedPath), Installing: gortexInstallInProgress.Load(), AnyProcessRunning: gortexAnyProcessRunning(), UnmanagedProcessRunning: gortexUnmanagedProcessRunning(), DefaultProject: gortexDefaultProject()}
	response.CodexAvailable = gortexAgentAvailable("codex")
	response.ClaudeAvailable = gortexAgentAvailable("claude")
	response.CursorAvailable = gortexAgentAvailable("cursor")
	response.CopilotAvailable = gortexAgentAvailable("copilot")
	response.OpenCodeAvailable = gortexAgentAvailable("opencode")
	response.AntigravityAvailable = gortexAgentAvailable("antigravity")
	response.GeminiAvailable = gortexAgentAvailable("gemini")
	response.CodexConfigured, response.CodexComplete = queryCodexMCPState(exe)
	response.ClaudeConfigured, response.ClaudeComplete = queryJSONMCPState("claude", exe)
	response.CursorConfigured, response.CursorComplete = queryJSONMCPState("cursor", exe)
	response.CopilotConfigured, response.CopilotComplete = queryJSONMCPState("copilot", exe)
	response.OpenCodeConfigured, response.OpenCodeComplete = queryOpenCodeMCPState(exe)
	response.AntigravityConfigured, response.AntigravityComplete = queryJSONMCPState("antigravity", exe)
	response.GeminiConfigured, response.GeminiComplete = queryJSONMCPState("gemini", exe)
	response.CodexPrompt, response.CodexPromptComplete = gortexPromptStatus("codex")
	response.ClaudePrompt, response.ClaudePromptComplete = gortexPromptStatus("claude")
	response.CursorPrompt = false
	response.CursorPromptComplete = false
	response.CopilotPrompt, response.CopilotPromptComplete = gortexPromptStatus("copilot")
	response.OpenCodePrompt, response.OpenCodePromptComplete = gortexPromptStatus("opencode")
	response.AntigravityPrompt, response.AntigravityPromptComplete = gortexPromptStatus("antigravity")
	response.GeminiPrompt, response.GeminiPromptComplete = gortexPromptStatus("gemini")
	response.CodexHook, response.CodexHookComplete = gortexHookStatus("codex", exe)
	response.ClaudeHook, response.ClaudeHookComplete = gortexHookStatus("claude", exe)
	response.CopilotHook, response.CopilotHookComplete = gortexHookStatus("copilot", exe)
	response.OpenCodeHook, response.OpenCodeHookComplete = gortexHookStatus("opencode", exe)
	response.AntigravityHook, response.AntigravityHookComplete = gortexHookStatus("antigravity", exe)
	response.GeminiHook, response.GeminiHookComplete = gortexHookStatus("gemini", exe)
	trust := gortexCodexTrustStatus(exe)
	response.CodexTrustStatus, response.CodexTrustRequired, response.CodexTrustNotice, response.CodexTrustSteps = trust.Status, trust.Required, trust.Notice, trust.Steps
	if managedRoot != "" {
		response.UserPath, response.SystemPath, _ = queryGortexPathStatus(filepath.Join(managedRoot, "bin"))
	}
	if projects, err := gortexTrackedProjects(); err == nil {
		response.TrackedProjects = projects
	} else {
		response.Message = "读取 Gortex 项目记录失败: " + err.Error()
	}
	if exe == "" {
		response.ActivationState = "not_installed"
		if response.Message == "" {
			response.Message = "未检测到 gortex.exe；请先选择版本并安装。"
		}
		return response
	}
	if !response.Installing {
		response.Version = queryGortexVersion(exe)
	}
	response.Running, response.ProcessID = gortexDaemonStatus(exe)
	if response.Running {
		response.TrackedProjects = normalizeGortexProjects(append(response.TrackedProjects, gortexDaemonTrackedProjects(exe)...))
	}
	cursorPresent, cursorComplete := false, len(response.TrackedProjects) > 0
	for _, project := range response.TrackedProjects {
		present, complete := gortexCursorRuleStatus(project)
		cursorPresent = cursorPresent || present
		if !present || !complete {
			cursorComplete = false
		}
	}
	response.CursorPrompt = cursorPresent
	response.CursorPromptComplete = cursorPresent && cursorComplete
	// Include project rules and ledger remnants in the same integration signal
	// used by the UI's remove/uninstall buttons. This is intentionally computed
	// after daemon projects are merged so repositories tracked elsewhere are
	// not invisible when they are the only remaining Gortex artifact.
	for _, project := range response.TrackedProjects {
		if gortexCursorRuleConfigured(project) {
			response.IntegrationPresent = true
			break
		}
	}
	if ownership, err := readGortexOwnership(); err == nil {
		response.ProjectMCPEnabled = ownership.ProjectMCPEnabled
		response.ProjectMCPProjects = gortexProjectMCPProjectsFromOwnership(ownership)
		response.IntegrationPresent = response.IntegrationPresent || len(ownership.Platforms) > 0 || len(ownership.ProjectMCP) > 0 || ownership.ProjectMCPEnabled || len(ownership.Artifacts) > 0 || ownership.UserPath || ownership.SystemPath
	} else {
		// A damaged ownership file is itself a cleanup residual; expose it so
		// the UI keeps the removal/uninstall controls available and can report
		// the repair warning instead of presenting a falsely clean state.
		response.IntegrationPresent = true
		if response.Message == "" {
			response.Message = "Gortex 归属账本需要修复: " + err.Error()
		}
	}
	response.IntegrationPresent = response.IntegrationPresent || response.CodexConfigured || response.ClaudeConfigured || response.CursorConfigured || response.CopilotConfigured || response.OpenCodeConfigured || response.AntigravityConfigured || response.GeminiConfigured || response.CodexPrompt || response.ClaudePrompt || response.CursorPrompt || response.CopilotPrompt || response.OpenCodePrompt || response.AntigravityPrompt || response.GeminiPrompt || response.CodexHook || response.ClaudeHook || response.CopilotHook || response.OpenCodeHook || response.AntigravityHook || response.GeminiHook || response.UserPath || response.SystemPath || len(response.TrackedProjects) > 0
	if response.Running {
		response.ActivationState = "running"
		if response.Message == "" {
			if response.ManagedInstalled {
				response.Message = "Gortex daemon 正在运行。"
			} else {
				response.Message = "检测到未受管 Gortex 进程；仅用于检测，code-Manager 不会控制它。"
			}
		}
	} else {
		response.ActivationState = "installed_stopped"
		if response.Message == "" {
			if response.ManagedInstalled {
				response.Message = "Gortex 已安装但 daemon 已停止。"
			} else if response.AnyProcessRunning {
				response.Message = "检测到未受管或 MCP Gortex 进程；安装/升级前请先结束相关进程。"
			} else {
				response.Message = "检测到 PATH 中的外部 Gortex；仅用于检测，请先安装受管版本。"
			}
		}
	}
	return response
}

func (g *gateway) gortexStatus(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "method not allowed", 405)
		return
	}
	writeJSON(w, 200, gortexStatusSnapshot())
}
func (g *gateway) gortexReleases(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "method not allowed", 405)
		return
	}
	if gortexInstallInProgress.Load() {
		http.Error(w, "Gortex 正在安装，暂不能加载远端版本", http.StatusConflict)
		return
	}
	page := 1
	if raw := strings.TrimSpace(r.URL.Query().Get("page")); raw != "" {
		parsed, err := strconv.Atoi(raw)
		if err != nil || parsed < 1 || parsed > 100 {
			http.Error(w, "page must be between 1 and 100", 400)
			return
		}
		page = parsed
	}
	var releases []githubRelease
	requestURL := fmt.Sprintf("%s?per_page=%d&page=%d", gortexGitHubReleasesURL, gortexReleasePageSize, page)
	if err := g.githubJSON(r.Context(), requestURL, &releases); err != nil {
		http.Error(w, "读取 Gortex 版本失败: "+err.Error(), 502)
		return
	}
	options := []gortexReleaseOption{}
	for _, release := range releases {
		if release.Draft || strings.TrimSpace(release.TagName) == "" {
			continue
		}
		available := false
		for _, asset := range release.Assets {
			if asset.Name == gortexWindowsAssetName {
				available = true
				break
			}
		}
		options = append(options, gortexReleaseOption{TagName: release.TagName, Name: release.Name, PublishedAt: release.PublishedAt.Format(time.RFC3339), Prerelease: release.Prerelease, Available: available, AssetName: gortexWindowsAssetName})
	}
	writeJSON(w, 200, gortexReleaseListResponse{Releases: options, Page: page, PerPage: gortexReleasePageSize, HasMore: len(releases) == gortexReleasePageSize})
}
func parseJSONBody(w http.ResponseWriter, r *http.Request, target any) error {
	decoder := json.NewDecoder(http.MaxBytesReader(w, r.Body, 16<<10))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(target); err != nil {
		return err
	}
	if err := decoder.Decode(&struct{}{}); err != io.EOF {
		return err
	}
	return nil
}
func (g *gateway) gortexInstall(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", 405)
		return
	}
	g.gortexMu.Lock()
	defer g.gortexMu.Unlock()
	if gortexInstallInProgress.Load() {
		http.Error(w, "Gortex 正在安装，请等待当前安装完成", http.StatusConflict)
		return
	}
	if runtime.GOOS != "windows" {
		http.Error(w, "Gortex 一键安装目前只支持 Windows", 501)
		return
	}
	if g.isShuttingDown() {
		http.Error(w, "code-Manager 正在退出，不能安装 Gortex", 503)
		return
	}
	if !gortexInstallInProgress.CompareAndSwap(false, true) {
		http.Error(w, "Gortex 正在安装，请等待当前安装完成", http.StatusConflict)
		return
	}
	defer gortexInstallInProgress.Store(false)
	input := gortexInstallRequest{TagName: "latest"}
	if r.Body != nil && r.ContentLength != 0 {
		if err := parseJSONBody(w, r, &input); err != nil {
			http.Error(w, "invalid JSON request", 400)
			return
		}
	}
	input.TagName = strings.TrimSpace(input.TagName)
	if input.TagName == "" {
		input.TagName = "latest"
	}
	if input.TagName != "latest" && !gortexTagPattern.MatchString(input.TagName) {
		http.Error(w, "版本标识无效", 400)
		return
	}
	if err := stopAllGortexProcesses(gortexProcessStopTimeout); err != nil {
		http.Error(w, "Gortex 安装失败：无法停止所有 Gortex 进程: "+err.Error(), http.StatusConflict)
		return
	}
	invalidateGortexMCPStatusCache()
	root := gortexInstallRoot()
	bin := gortexManagedPath("bin")
	for _, path := range []string{root, bin, gortexManagedPath("config"), gortexManagedPath("data"), gortexManagedPath("cache"), gortexManagedPath("run")} {
		if err := os.MkdirAll(path, 0o700); err != nil {
			http.Error(w, fmt.Sprintf("创建 Gortex 目录失败（%s）: %v", path, err), http.StatusInternalServerError)
			return
		}
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Minute)
	defer cancel()
	if err := g.downloadAndInstallGortex(ctx, input.TagName, bin); err != nil {
		http.Error(w, "Gortex 安装失败: "+err.Error(), http.StatusBadGateway)
		return
	}
	invalidateGortexMCPStatusCache()
	ownership, ownershipErr := readGortexOwnership()
	userPath, systemPath, pathErr := configureGortexPath(ctx, bin)
	if pathErr != nil {
		writeJSON(w, http.StatusOK, gortexOperationResponse{Message: fmt.Sprintf("Gortex %s 已下载到 %s，但 PATH 配置未完成。", input.TagName, root), Warnings: []string{pathErr.Error()}})
		return
	}
	warnings := []string{"如需接入四个平台，请点击“注册 MCP”；该操作也会写入提示词和可用 Hook。"}
	if ownershipErr != nil {
		// Never replace a damaged ledger with an empty one merely because a
		// binary install happened to succeed. PATH is still configured, but the
		// existing ledger is left untouched for a later recovery/uninstall.
		warnings = append(warnings, "读取 Gortex 归属账本失败，已保留原文件；请在完成备份后重新注册接入: "+ownershipErr.Error())
	} else {
		ownership.UserPath, ownership.SystemPath = userPath, systemPath
		if err := writeGortexOwnership(ownership); err != nil {
			warnings = append(warnings, "保存 Gortex PATH 归属失败: "+err.Error())
		}
	}
	writeJSON(w, http.StatusOK, gortexOperationResponse{Message: fmt.Sprintf("Gortex %s 已安装到 %s，并已加入用户/系统 PATH；不会自动启动 daemon 或注册 MCP。", input.TagName, root), Warnings: warnings})
}

func (g *gateway) downloadAndInstallGortex(ctx context.Context, tagName, installDir string) error {
	tagName = strings.TrimSpace(tagName)
	if tagName == "" {
		return errors.New("tag_name is required")
	}
	requestURL := gortexGitHubReleasesURL + "/latest"
	if tagName != "latest" {
		requestURL = gortexGitHubReleasesURL + "/tags/" + url.PathEscape(tagName)
	}
	var release githubRelease
	if err := g.githubJSON(ctx, requestURL, &release); err != nil {
		return fmt.Errorf("读取版本 %q 失败: %w", tagName, err)
	}
	var zipAsset, checksumAsset *llmtrimReleaseAsset
	for index := range release.Assets {
		asset := &release.Assets[index]
		switch asset.Name {
		case gortexWindowsAssetName:
			zipAsset = asset
		case "checksums.txt":
			checksumAsset = asset
		}
	}
	if zipAsset == nil {
		return fmt.Errorf("版本 %q 没有 Windows 安装包 %s", tagName, gortexWindowsAssetName)
	}
	if checksumAsset == nil {
		return fmt.Errorf("版本 %q 没有校验文件 checksums.txt", tagName)
	}
	zipPath, err := g.downloadTempFile(ctx, zipAsset.BrowserDownloadURL, "gortex-*.zip")
	if err != nil {
		return fmt.Errorf("下载 Gortex Windows 安装包失败: %w", err)
	}
	defer os.Remove(zipPath)
	checksumText, err := g.downloadText(ctx, checksumAsset.BrowserDownloadURL)
	if err != nil {
		return fmt.Errorf("下载 Gortex 校验文件失败: %w", err)
	}
	expectedHash, err := parseGortexChecksum(checksumText, gortexWindowsAssetName)
	if err != nil {
		return err
	}
	actualHash, err := fileSHA256(zipPath)
	if err != nil {
		return fmt.Errorf("计算 Gortex 安装包校验失败: %w", err)
	}
	if !strings.EqualFold(expectedHash, actualHash) {
		return fmt.Errorf("Gortex 安装包 SHA-256 校验失败，期望 %s，实际 %s", expectedHash, actualHash)
	}
	if err := installGortexZIP(zipPath, installDir); err != nil {
		return err
	}
	if err := writeGortexPromptDocument(filepath.Dir(installDir)); err != nil {
		return fmt.Errorf("Gortex prompt document write failed: %w", err)
	}
	return nil
}

func parseGortexChecksum(text, assetName string) (string, error) {
	for _, line := range strings.Split(text, "\n") {
		if !strings.Contains(line, assetName) {
			continue
		}
		if hash := gortexChecksumPattern.FindString(line); hash != "" {
			return hash, nil
		}
	}
	return "", fmt.Errorf("checksums.txt 中没有找到 %s 的 SHA-256 摘要", assetName)
}

// replaceGortexExecutable keeps the previous binary recoverable while a
// staged release is moved into place. Windows os.Rename cannot replace an
// existing executable reliably, so the backup and staged file live beside the
// target and every failure path attempts to restore the old file.
func replaceGortexExecutable(stagedPath, targetPath string) error {
	if strings.TrimSpace(stagedPath) == "" || strings.TrimSpace(targetPath) == "" {
		return errors.New("Gortex 可执行文件路径不能为空")
	}
	stagedInfo, err := os.Stat(stagedPath)
	if err != nil {
		return fmt.Errorf("读取暂存的 Gortex 可执行文件失败: %w", err)
	}
	if !stagedInfo.Mode().IsRegular() || stagedInfo.Size() == 0 {
		return errors.New("暂存的 Gortex 可执行文件无效或为空")
	}
	if info, err := os.Stat(targetPath); err == nil && info.IsDir() {
		return fmt.Errorf("Gortex 可执行文件目标是目录: %s", targetPath)
	} else if err != nil && !errors.Is(err, os.ErrNotExist) {
		return err
	}

	directory := filepath.Dir(targetPath)
	backupPath := ""
	if _, err := os.Stat(targetPath); err == nil {
		placeholder, err := os.CreateTemp(directory, ".gortex-backup-*")
		if err != nil {
			return fmt.Errorf("创建 Gortex 备份文件失败: %w", err)
		}
		backupPath = placeholder.Name()
		if err := placeholder.Close(); err != nil {
			_ = os.Remove(backupPath)
			return err
		}
		if err := os.Remove(backupPath); err != nil {
			return err
		}
		if err := os.Rename(targetPath, backupPath); err != nil {
			_ = os.Remove(backupPath)
			return fmt.Errorf("暂存旧 Gortex 可执行文件失败: %w", err)
		}
	}

	restore := func() error {
		if backupPath == "" {
			return nil
		}
		if _, err := os.Stat(targetPath); err == nil {
			_ = os.Remove(targetPath)
		}
		if err := os.Rename(backupPath, targetPath); err != nil {
			return err
		}
		backupPath = ""
		return nil
	}

	if err := os.Rename(stagedPath, targetPath); err != nil {
		restoreErr := restore()
		if restoreErr != nil {
			return fmt.Errorf("写入新 Gortex 可执行文件失败: %w；恢复旧文件也失败: %v", err, restoreErr)
		}
		return fmt.Errorf("写入新 Gortex 可执行文件失败: %w", err)
	}
	if backupPath != "" {
		if err := os.Remove(backupPath); err != nil {
			// The new executable is already in place. Keep the old backup and
			// report it so a later install can clean the directory explicitly.
			return fmt.Errorf("新 Gortex 已写入，但删除旧备份失败: %w", err)
		}
		backupPath = ""
	}
	return nil
}

func installGortexZIP(zipPath, installDir string) error {
	archive, err := zip.OpenReader(zipPath)
	if err != nil {
		return fmt.Errorf("打开 Gortex ZIP 失败: %w", err)
	}
	defer archive.Close()
	if err := os.MkdirAll(filepath.Dir(installDir), 0o700); err != nil {
		return fmt.Errorf("创建 Gortex 安装父目录失败（%s）: %w", filepath.Dir(installDir), err)
	}
	stagingDir, err := os.MkdirTemp(filepath.Dir(installDir), ".gortex-install-*")
	if err != nil {
		return fmt.Errorf("创建 Gortex 临时安装目录失败: %w", err)
	}
	defer os.RemoveAll(stagingDir)
	found := false
	for _, entry := range archive.File {
		if entry.FileInfo().IsDir() || !strings.EqualFold(path.Base(strings.ReplaceAll(entry.Name, `\`, "/")), gortexExecutableName) {
			continue
		}
		if found {
			return errors.New("Gortex ZIP 中存在多个 gortex.exe")
		}
		reader, err := entry.Open()
		if err != nil {
			return fmt.Errorf("读取 Gortex ZIP 文件失败: %w", err)
		}
		target := filepath.Join(stagingDir, gortexExecutableName)
		output, err := os.OpenFile(target, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, 0o755)
		if err == nil {
			_, err = io.Copy(output, reader)
			closeErr := output.Close()
			if err == nil {
				err = closeErr
			}
		}
		_ = reader.Close()
		if err != nil {
			return fmt.Errorf("解压 Gortex ZIP 文件失败: %w", err)
		}
		found = true
	}
	if !found {
		return errors.New("Gortex ZIP 中没有 gortex.exe")
	}
	stagedPath := filepath.Join(stagingDir, gortexExecutableName)
	stagedInfo, err := os.Stat(stagedPath)
	if err != nil {
		return fmt.Errorf("读取暂存的 Gortex 可执行文件失败: %w", err)
	}
	if !stagedInfo.Mode().IsRegular() || stagedInfo.Size() == 0 {
		return errors.New("Gortex ZIP 中的 gortex.exe 无效或为空")
	}
	if err := os.MkdirAll(installDir, 0o700); err != nil {
		return fmt.Errorf("创建 Gortex bin 目录失败（%s）: %w", installDir, err)
	}
	target := filepath.Join(installDir, gortexExecutableName)
	if err := stopAllGortexProcesses(gortexProcessStopTimeout); err != nil {
		return fmt.Errorf("替换前停止全部 Gortex 进程失败: %w", err)
	}
	if err := replaceGortexExecutable(stagedPath, target); err != nil {
		return fmt.Errorf("替换 Gortex 可执行文件失败（%s）: %w", target, err)
	}
	return nil
}
func (g *gateway) gortexStart(w http.ResponseWriter, r *http.Request) {
	g.gortexDaemonAction(w, r, "start")
}
func (g *gateway) gortexStop(w http.ResponseWriter, r *http.Request) {
	g.gortexDaemonAction(w, r, "stop")
}
func (g *gateway) gortexDaemonAction(w http.ResponseWriter, r *http.Request, action string) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", 405)
		return
	}
	g.gortexMu.Lock()
	defer g.gortexMu.Unlock()
	if g.isShuttingDown() {
		http.Error(w, "code-Manager 正在退出，Gortex 控制已停止", 503)
		return
	}
	exe := gortexManagedExecutablePath()
	if exe == "" {
		http.Error(w, "未检测到受管 Gortex，请先安装到 code-Manager.exe 同级目录", 400)
		return
	}
	if action == "stop" {
		warnings := []string{}
		if _, err := runGortexCommandWithTimeout(gortexDaemonTimeout, exe, "daemon", "stop"); err != nil && gortexProcessRunning(exe) {
			warnings = append(warnings, "停止 daemon 失败: "+err.Error())
		}
		if gortexProcessRunning(exe) && !waitForGortexProcessExit(exe, 15*time.Second) {
			warnings = append(warnings, "daemon stop 已返回，但 daemon 仍未退出")
		}
		invalidateGortexMCPStatusCache()
		writeJSON(w, 200, gortexOperationResponse{Message: "Gortex daemon 已停止；MCP 注册、项目和数据保留。", Warnings: warnings})
		return
	}
	if _, err := runGortexCommandWithTimeout(gortexDaemonTimeout, exe, "daemon", "start", "--detach"); err != nil {
		http.Error(w, "Gortex daemon 启动失败: "+err.Error(), 502)
		return
	}
	if !waitForGortexProcessStart(exe, 15*time.Second) {
		http.Error(w, "Gortex daemon 启动命令已返回，但尚未检测到受管进程", 502)
		return
	}
	invalidateGortexMCPStatusCache()
	writeJSON(w, 200, gortexOperationResponse{Message: "Gortex daemon 已启动；MCP 注册请使用“注册 MCP”。"})
}
func (g *gateway) gortexRegister(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", 405)
		return
	}
	g.gortexMu.Lock()
	defer g.gortexMu.Unlock()
	exe := gortexManagedExecutablePath()
	if exe == "" {
		http.Error(w, "未检测到受管 Gortex，请先安装到 code-Manager.exe 同级目录", 400)
		return
	}
	warnings := gortexRegisterMCP(exe)
	invalidateGortexMCPStatusCache()
	writeJSON(w, 200, gortexOperationResponse{Message: "Gortex MCP 已按已检测到的平台完成注册。", Warnings: warnings})
}
func (g *gateway) gortexRemove(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", 405)
		return
	}
	g.gortexMu.Lock()
	defer g.gortexMu.Unlock()
	warnings := gortexRemoveMCP()
	invalidateGortexMCPStatusCache()
	writeJSON(w, 200, gortexOperationResponse{Message: "仅本程序拥有的 gortex MCP 配置已移除，其他 MCP 保持不变。", Warnings: warnings})
}

func (g *gateway) gortexTrust(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	g.gortexMu.Lock()
	defer g.gortexMu.Unlock()
	info := gortexCodexTrustStatus(gortexManagedExecutablePath())
	if !info.Required {
		writeJSON(w, http.StatusOK, info)
		return
	}
	if err := openGortexCodexTrustShell(&info); err != nil {
		http.Error(w, "打开 Codex Gortex Hook 信任窗口失败: "+err.Error(), http.StatusBadGateway)
		return
	}
	info.Notice = "已打开 PowerShell 中的 Codex 审核界面；在 /hooks 中审核并信任 Gortex Hook，然后回到页面刷新状态。"
	writeJSON(w, http.StatusOK, info)
}

func (g *gateway) gortexDiagnostics(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost && r.Method != http.MethodGet {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	g.gortexMu.Lock()
	defer g.gortexMu.Unlock()
	exe := gortexManagedExecutablePath()
	if exe == "" {
		http.Error(w, "未检测到受管 Gortex，请先安装", http.StatusBadRequest)
		return
	}
	doctorOutput, doctorErr := runGortexCommandWithTimeout(45*time.Second, exe, "doctor", "--json")
	statusOutput, statusErr := runGortexCommandWithTimeout(25*time.Second, exe, "status")
	writeJSON(w, http.StatusOK, map[string]any{
		"doctor_ok":     doctorErr == nil,
		"doctor_output": doctorOutput,
		"doctor_error":  errorText(doctorErr),
		"status_ok":     statusErr == nil,
		"status_output": statusOutput,
		"status_error":  errorText(statusErr),
		"message":       "已执行 gortex doctor --json 与 gortex status。",
	})
}

func errorText(err error) string {
	if err == nil {
		return ""
	}
	return err.Error()
}

func normalizeGortexProjectPath(pathValue string) (string, error) {
	pathValue = strings.TrimSpace(pathValue)
	if pathValue == "" {
		return "", errors.New("项目路径不能为空")
	}
	if !filepath.IsAbs(pathValue) {
		return "", errors.New("项目路径必须是绝对路径")
	}
	absolute, err := filepath.Abs(pathValue)
	if err != nil {
		return "", err
	}
	return filepath.Clean(absolute), nil
}

func validateGortexProjectPath(pathValue string) (string, error) {
	absolute, err := normalizeGortexProjectPath(pathValue)
	if err != nil {
		return "", err
	}
	info, err := os.Stat(absolute)
	if err != nil {
		return "", fmt.Errorf("项目路径不可用: %w", err)
	}
	if !info.IsDir() {
		return "", errors.New("项目路径必须是目录")
	}
	return absolute, nil
}
func (g *gateway) gortexTrack(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", 405)
		return
	}
	g.gortexMu.Lock()
	defer g.gortexMu.Unlock()
	var input gortexOperationRequest
	if err := parseJSONBody(w, r, &input); err != nil {
		http.Error(w, "invalid JSON request", 400)
		return
	}
	project, err := validateGortexProjectPath(input.Path)
	if err != nil {
		http.Error(w, err.Error(), 400)
		return
	}
	exe := gortexManagedExecutablePath()
	if exe == "" {
		http.Error(w, "未检测到受管 Gortex，请先安装到 code-Manager.exe 同级目录", 400)
		return
	}
	// Tracking is a daemon-backed operation. Start the managed daemon on
	// demand so a prior manual stop does not turn a valid track request into
	// an avoidable "daemon not running" error.
	if !gortexProcessRunning(exe) {
		if _, err := runGortexCommandWithTimeout(gortexDaemonTimeout, exe, "daemon", "start", "--detach"); err != nil {
			http.Error(w, "Gortex daemon 启动失败，无法开始 track: "+err.Error(), 502)
			return
		}
		if !waitForGortexProcessStart(exe, 15*time.Second) {
			http.Error(w, "Gortex daemon 启动命令已返回，但尚未检测到受管进程", 502)
			return
		}
	}
	if _, err := runGortexCommandWithTimeout(gortexTrackTimeout, exe, "track", project, "--wait", "--wait-timeout", "30m"); err != nil {
		http.Error(w, "Gortex track 失败: "+err.Error(), 502)
		return
	}
	registry, err := readGortexProjectRegistry()
	if err != nil {
		http.Error(w, "保存 Gortex 项目记录失败: "+err.Error(), 500)
		return
	}
	registry.Projects = append(registry.Projects, project)
	if err := writeGortexProjectRegistry(registry); err != nil {
		http.Error(w, "保存 Gortex 项目记录失败: "+err.Error(), 500)
		return
	}
	warnings := []string{}
	if err := ensureGortexWatchConfig(project); err != nil {
		warnings = append(warnings, "自动监视配置: "+err.Error())
	}
	available := gortexDetectedMCPAgents()
	ownership, ownershipErr := readGortexOwnership()
	if ownershipErr != nil {
		warnings = append(warnings, "读取 Gortex 归属账本失败: "+ownershipErr.Error())
	} else {
		if ownership.ProjectMCPEnabled {
			warnings = append(warnings, gortexRegisterProjectMCPForProject(project, exe, available)...)
		}
		ownership, ownershipErr = readGortexOwnership()
		if ownershipErr != nil {
			warnings = append(warnings, "重新读取 Gortex 归属账本失败: "+ownershipErr.Error())
		} else {
			if available["cursor"] {
				if err := gortexRegisterCursorProject(project, &ownership); err != nil {
					warnings = append(warnings, "Cursor 项目规则: "+err.Error())
				}
			}
			if err := writeGortexOwnership(ownership); err != nil {
				warnings = append(warnings, "保存 Gortex 接入归属失败: "+err.Error())
			}
		}
	}
	message := "已建立 Gortex 项目代码图谱；后续深度分析由 AI 按任务需要调用 MCP。"
	if len(warnings) > 0 {
		message = "已建立 Gortex 项目代码图谱，但部分接入配置需要处理。"
	}
	writeJSON(w, 200, gortexOperationResponse{Message: message, Warnings: warnings})
}
func (g *gateway) gortexUntrack(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", 405)
		return
	}
	g.gortexMu.Lock()
	defer g.gortexMu.Unlock()
	var input gortexOperationRequest
	if err := parseJSONBody(w, r, &input); err != nil {
		http.Error(w, "invalid JSON request", 400)
		return
	}
	project, err := normalizeGortexProjectPath(input.Path)
	if err != nil {
		http.Error(w, err.Error(), 400)
		return
	}
	exe := gortexManagedExecutablePath()
	if exe == "" {
		http.Error(w, "未检测到受管 Gortex，请先安装到 code-Manager.exe 同级目录", 400)
		return
	}
	if _, err := runGortexCommandWithTimeout(gortexUntrackTimeout, exe, "untrack", project); err != nil {
		http.Error(w, "Gortex untrack 失败: "+err.Error(), 502)
		return
	}
	registry, err := readGortexProjectRegistry()
	if err != nil {
		http.Error(w, "读取 Gortex 项目记录失败: "+err.Error(), 500)
		return
	}
	filtered := []string{}
	for _, existing := range registry.Projects {
		if !strings.EqualFold(filepath.Clean(existing), filepath.Clean(project)) {
			filtered = append(filtered, existing)
		}
	}
	registry.Projects = filtered
	if err := writeGortexProjectRegistry(registry); err != nil {
		http.Error(w, "保存 Gortex 项目记录失败: "+err.Error(), 500)
		return
	}
	warnings := gortexRemoveProjectMCP(exe, project)
	ownership, ownershipErr := readGortexOwnership()
	if ownershipErr != nil {
		warnings = append(warnings, "读取 Gortex 归属账本失败: "+ownershipErr.Error())
	} else {
		if err := gortexRemoveCursorProject(project, &ownership); err != nil {
			warnings = append(warnings, "Cursor 项目规则: "+err.Error())
		}
		if err := writeGortexOwnership(ownership); err != nil {
			warnings = append(warnings, "保存 Gortex 接入归属失败: "+err.Error())
		}
	}
	message := "已取消该项目的 Gortex track；不会删除项目文件。"
	if len(warnings) > 0 {
		message = "已取消该项目的 Gortex track，但部分项目接入配置需要处理。"
	}
	writeJSON(w, 200, gortexOperationResponse{Message: message, Warnings: warnings})
}
func (g *gateway) gortexUninstall(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", 405)
		return
	}
	if gortexInstallInProgress.Load() {
		http.Error(w, "Gortex 正在安装，请等待安装完成后再卸载", http.StatusConflict)
		return
	}
	g.gortexMu.Lock()
	defer g.gortexMu.Unlock()
	if gortexInstallInProgress.Load() {
		http.Error(w, "Gortex 正在安装，请等待安装完成后再卸载", http.StatusConflict)
		return
	}
	exe := gortexManagedExecutablePath()
	// The UI deliberately enables uninstall only after the daemon is stopped.
	// Keep the same guard server-side so a stale page cannot race a newly
	// started daemon and delete its live data directory.
	if exe != "" && gortexProcessRunning(exe) {
		http.Error(w, "Gortex daemon 正在运行，请先停止 daemon 后再卸载", http.StatusConflict)
		return
	}

	warnings := []string{}
	incomplete := false
	projects := []string{}
	if registry, err := readGortexProjectRegistry(); err == nil {
		projects = append(projects, registry.Projects...)
	} else {
		warnings = append(warnings, "读取项目记录失败: "+err.Error())
		incomplete = true
	}
	// The daemon may know repositories that this manager did not originally
	// track. status is best-effort when the daemon is already stopped; the
	// local registry above remains authoritative for our own entries.
	projects = normalizeGortexProjects(append(projects, gortexDaemonTrackedProjects(exe)...))

	// Remove all global integrations first. This function updates the ownership
	// ledger atomically and leaves failed/user-modified artifacts in it.
	removeWarnings := gortexRemoveMCP()
	if len(removeWarnings) > 0 {
		warnings = append(warnings, removeWarnings...)
		incomplete = true
	}

	// Re-read ownership after gortexRemoveMCP. Do not reuse a pre-removal
	// snapshot: doing so would resurrect already-removed MCP/Hook artifacts.
	ownership, ownershipErr := readGortexOwnership()
	if ownershipErr != nil {
		// A damaged ledger is intentionally preserved. Marker-based removal is
		// still safe because removeGortexPrompt validates the canonical body.
		warnings = append(warnings, "读取 Gortex 归属账本失败: "+ownershipErr.Error())
		incomplete = true
		fallback := gortexMCPOwnership{Platforms: map[string]gortexOwnedMCP{}, Artifacts: map[string]gortexOwnedArtifact{}}
		cursorWarnings := gortexRemoveCursorProjects(projects, &fallback)
		if len(cursorWarnings) > 0 {
			warnings = append(warnings, cursorWarnings...)
			incomplete = true
		}
	} else {
		cursorWarnings := gortexRemoveCursorProjects(projects, &ownership)
		if len(cursorWarnings) > 0 {
			warnings = append(warnings, cursorWarnings...)
			incomplete = true
		}
		if err := writeGortexOwnership(ownership); err != nil {
			warnings = append(warnings, "保存 Gortex 项目规则归属失败: "+err.Error())
			incomplete = true
		}
	}

	// MCP clients are removed above before process termination. Now terminate
	// every exact-name gortex.exe, including stdio clients or daemons launched
	// outside code-Manager, so the managed directory can be removed safely.
	if err := stopAllGortexProcesses(gortexProcessStopTimeout); err != nil {
		warnings = append(warnings, "停止全部 Gortex 进程失败: "+err.Error())
		incomplete = true
	}

	registryCleared := true
	if err := writeGortexProjectRegistry(gortexProjectRegistry{}); err != nil {
		warnings = append(warnings, "清空 Gortex 项目记录失败: "+err.Error())
		incomplete = true
		registryCleared = false
	}
	pathWarnings := removeGortexPathForUninstall(r.Context())
	if len(pathWarnings) > 0 {
		warnings = append(warnings, pathWarnings...)
		incomplete = true
	}

	// Keep PATH ownership flags truthful. If removal failed, the ledger must
	// retain the flag and the managed root must stay available for retry.
	pathPresent := false
	userPathPresent := false
	systemPathPresent := false
	pathStateKnown := true
	if root := gortexInstallRoot(); root != "" {
		userPath, systemPath, message := queryGortexPathStatus(filepath.Join(root, "bin"))
		if message != "" {
			pathStateKnown = false
			incomplete = true
			warnings = append(warnings, "读取 Gortex PATH 状态失败: "+message)
		} else {
			userPathPresent, systemPathPresent = userPath, systemPath
			pathPresent = userPath || systemPath
		}
	}
	if ownershipErr == nil {
		if pathStateKnown {
			ownership.UserPath, ownership.SystemPath = userPathPresent, systemPathPresent
		}
		if err := writeGortexOwnership(ownership); err != nil {
			warnings = append(warnings, "清理 Gortex 接入归属失败: "+err.Error())
			incomplete = true
		}
	}

	if legacy, err := gortexLegacyProjectRegistryPath(); err == nil {
		if managed, managedErr := gortexProjectRegistryPath(); managedErr == nil && legacy != managed {
			if removeErr := os.Remove(legacy); removeErr != nil && !errors.Is(removeErr, os.ErrNotExist) {
				warnings = append(warnings, "删除旧版 Gortex 项目记录失败: "+removeErr.Error())
				incomplete = true
			}
		}
	} else {
		warnings = append(warnings, "定位旧版 Gortex 项目记录失败: "+err.Error())
		incomplete = true
	}

	// A failed cleanup must retain the ledger and managed root so the user can
	// retry safely. Only remove the root after every ownership and process
	// operation completed without warnings.
	if ownershipErr == nil && (len(ownership.Platforms) > 0 || len(ownership.ProjectMCP) > 0 || ownership.ProjectMCPEnabled || len(ownership.Artifacts) > 0 || ownership.UserPath || ownership.SystemPath) {
		incomplete = true
	}
	root := gortexInstallRoot()
	if !incomplete && registryCleared && !pathPresent && root != "" && directoryExists(root) {
		if err := os.RemoveAll(root); err != nil {
			warnings = append(warnings, "删除受管 Gortex 目录失败: "+err.Error())
			incomplete = true
		}
	}
	invalidateGortexMCPStatusCache()
	message := "Gortex MCP、daemon、项目记录及同级 Gortex 目录已清理；其他 MCP 和项目文件保留。"
	if incomplete || len(warnings) > 0 {
		message = "Gortex 清理完成，但有部分项目或平台需要人工处理；归属账本已保留以便重试。"
	}
	writeJSON(w, 200, gortexOperationResponse{Message: message, Warnings: warnings})
}

func removeGortexPathForUninstall(ctx context.Context) []string {
	root := gortexInstallRoot()
	if root == "" {
		return nil
	}
	bin := filepath.Join(root, "bin")
	userPath, systemPath, message := queryGortexPathStatus(bin)
	warnings := []string{}
	if message != "" {
		warnings = append(warnings, "读取 Gortex PATH 状态失败: "+message)
	}
	if !userPath && !systemPath {
		return warnings
	}
	if err := removeGortexPath(ctx, bin, userPath, systemPath); err != nil {
		warnings = append(warnings, "清理 Gortex PATH 失败: "+err.Error())
	}
	return warnings
}
func (g *gateway) stopGortex(ctx context.Context) error {
	exe := gortexManagedExecutablePath()
	if exe == "" || !gortexProcessRunning(exe) {
		return nil
	}
	cmd := exec.CommandContext(ctx, exe, "daemon", "stop")
	cmd.Env = gortexManagedEnv()
	hideGortexCommandWindow(cmd)
	output, err := cmd.CombinedOutput()
	if err != nil {
		text := strings.TrimSpace(string(output))
		if text != "" {
			return fmt.Errorf("Gortex daemon stop failed: %s", text)
		}
		return err
	}
	return nil
}

func isAmbiguousDirectory(p string) bool {
	clean := filepath.Clean(p)
	if clean == "" || clean == "/" || clean == "." {
		return true
	}
	if filepath.IsAbs(clean) && clean == filepath.Dir(clean) {
		return true
	}
	return false
}

func isHostProgramDirectory(p string) bool {
	lower := strings.ToLower(filepath.Clean(p))
	return strings.Contains(lower, `\programs\antigravity`) ||
		strings.Contains(lower, `/programs/antigravity`) ||
		strings.Contains(lower, `\microsoft vs code`) ||
		strings.Contains(lower, `/microsoft vs code`)
}

func resolveBridgeTargetCWD(gortexExe string) string {
	// 1. 优先检查显式环境变量
	for _, key := range []string{
		"ANTIGRAVITY_WORKSPACE",
		"WORKSPACE",
		"WORKSPACE_DIR",
		"PROJECT_DIR",
	} {
		if val := strings.TrimSpace(os.Getenv(key)); val != "" && directoryExists(val) {
			clean := filepath.Clean(val)
			if !isAmbiguousDirectory(clean) && !isHostProgramDirectory(clean) {
				return clean
			}
		}
	}

	// 2. 检查当前启动目录（若有效且非宿主程序安装目录）
	if cwd, err := os.Getwd(); err == nil && cwd != "" && directoryExists(cwd) {
		clean := filepath.Clean(cwd)
		if !isAmbiguousDirectory(clean) && !isHostProgramDirectory(clean) {
			return clean
		}
	}

	// 3. 从 Daemon 获取当前已 track 的项目
	if gortexExe != "" {
		tracked := gortexDaemonTrackedProjects(gortexExe)
		for _, p := range tracked {
			clean := filepath.Clean(p)
			if directoryExists(clean) && !isAmbiguousDirectory(clean) && !isHostProgramDirectory(clean) {
				return clean
			}
		}
	}

	// 4. 从本地注册的 tracked 列表获取
	if local, err := gortexTrackedProjects(); err == nil {
		for _, p := range local {
			clean := filepath.Clean(p)
			if directoryExists(clean) && !isAmbiguousDirectory(clean) && !isHostProgramDirectory(clean) {
				return clean
			}
		}
	}

	// 5. 兜底回退到用户主目录
	if home, err := os.UserHomeDir(); err == nil && home != "" && directoryExists(home) {
		return filepath.Clean(home)
	}
	return ""
}

func gortexManagedEnvForExecutable(gortexExe string) []string {
	root := ""
	if gortexExe != "" {
		dir := filepath.Dir(filepath.Clean(gortexExe))
		if strings.EqualFold(filepath.Base(dir), "bin") {
			root = filepath.Dir(dir)
		} else {
			root = dir
		}
	}
	if root == "" {
		root = gortexInstallRoot()
	}
	if root == "" {
		return os.Environ()
	}
	values := map[string]string{
		"XDG_CONFIG_HOME":            filepath.Join(root, "config"),
		"XDG_DATA_HOME":              filepath.Join(root, "data"),
		"XDG_CACHE_HOME":             filepath.Join(root, "cache"),
		"GORTEX_DAEMON_SOCKET":       filepath.Join(root, "run", "daemon.sock"),
		"GORTEX_DAEMON_PIDFILE":      filepath.Join(root, "run", "daemon.pid"),
		"GORTEX_DAEMON_LOGFILE":      filepath.Join(root, "run", "daemon.log"),
		"GORTEX_DAEMON_STATEFILE":    filepath.Join(root, "run", "daemon.state.json"),
		"GORTEX_RECONCILE_INTERVAL":  "1h",
		"GORTEX_DAEMON_IDLE_TIMEOUT": "0",
	}
	env := os.Environ()
	for key, value := range values {
		env = setEnvValue(env, key, value)
	}
	return env
}

func runGortexBridge(args []string) {
	var gortexExe string
	for i := 0; i < len(args); i++ {
		if args[i] == "--gortex" && i+1 < len(args) {
			gortexExe = args[i+1]
			i++
		}
	}
	if gortexExe == "" {
		gortexExe = os.Getenv("GORTEX_EXECUTABLE")
	}
	if gortexExe == "" {
		gortexExe = gortexManagedExecutablePath()
	}
	if gortexExe == "" {
		if found, err := exec.LookPath(gortexExecutableName); err == nil {
			gortexExe = found
		}
	}
	if gortexExe == "" || !fileExists(gortexExe) {
		fmt.Fprintf(os.Stderr, "[gortex-bridge] 未找到有效的 Gortex 可执行文件: %s\n", gortexExe)
		os.Exit(1)
	}

	targetCWD := resolveBridgeTargetCWD(gortexExe)

	cmd := exec.Command(gortexExe, "mcp")
	if targetCWD != "" {
		cmd.Dir = targetCWD
	}
	cmd.Stdin = os.Stdin
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr

	env := gortexManagedEnvForExecutable(gortexExe)
	if targetCWD != "" {
		env = setEnvValue(env, "ANTIGRAVITY_WORKSPACE", targetCWD)
	}
	cmd.Env = env
	hideGortexCommandWindow(cmd)

	sigChan := make(chan os.Signal, 1)
	signal.Notify(sigChan, os.Interrupt, syscall.SIGTERM)
	go func() {
		for sig := range sigChan {
			if cmd.Process != nil {
				_ = cmd.Process.Signal(sig)
			}
		}
	}()

	if err := cmd.Run(); err != nil {
		var exitErr *exec.ExitError
		if errors.As(err, &exitErr) {
			os.Exit(exitErr.ExitCode())
		}
		fmt.Fprintf(os.Stderr, "[gortex-bridge] Gortex 进程退出: %v\n", err)
		os.Exit(1)
	}
}

