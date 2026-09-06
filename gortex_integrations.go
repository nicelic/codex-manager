package main

import (
	"bytes"
	"context"
	"crypto/sha256"
	_ "embed"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/BurntSushi/toml"
	"gopkg.in/yaml.v3"
)

var gortexTrackedStatusCache = struct {
	sync.Mutex
	at       time.Time
	path     string
	projects []string
}{}

func queryGortexPathStatus(directory string) (bool, bool, string) {
	return queryIndependentPathStatus(directory)
}

func configureGortexPath(ctx context.Context, directory string) (bool, bool, error) {
	return configureIndependentPathNamed(ctx, directory, "Gortex")
}

func removeGortexPath(ctx context.Context, directory string, userOwned, systemOwned bool) error {
	return removeOwnedIndependentPathNamed(ctx, directory, userOwned, systemOwned, "Gortex")
}

func repairInstalledGortexPath() {
	directory := gortexManagedPath("bin")
	if directory == "" || !fileExists(filepath.Join(directory, gortexExecutableName)) {
		return
	}
	user, system, message := queryGortexPathStatus(directory)
	if message != "" || (user && system) {
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), 45*time.Second)
	defer cancel()
	if _, _, err := configureGortexPath(ctx, directory); err != nil {
		log.Printf("检测到已安装的 Gortex 但 PATH 不完整，自动补齐失败: %v", err)
	}
}

const gortexWatchDebounceMilliseconds = 50

func yamlMappingValue(node *yaml.Node, key string) (*yaml.Node, bool) {
	if node == nil || node.Kind != yaml.MappingNode {
		return nil, false
	}
	for i := 0; i+1 < len(node.Content); i += 2 {
		if node.Content[i].Value == key {
			return node.Content[i+1], true
		}
	}
	return nil, false
}

func yamlScalar(value, tag string) *yaml.Node {
	return &yaml.Node{Kind: yaml.ScalarNode, Tag: tag, Value: value}
}

func yamlEnsureMappingChild(parent *yaml.Node, key string) (*yaml.Node, bool, error) {
	if value, ok := yamlMappingValue(parent, key); ok {
		if value.Kind != yaml.MappingNode {
			return nil, false, fmt.Errorf(".gortex.yaml 的 %s 必须是对象", key)
		}
		return value, false, nil
	}
	value := &yaml.Node{Kind: yaml.MappingNode, Tag: "!!map"}
	parent.Content = append(parent.Content, yamlScalar(key, "!!str"), value)
	return value, true, nil
}

func yamlEnsureScalar(parent *yaml.Node, key, value, tag string) bool {
	if existing, ok := yamlMappingValue(parent, key); ok {
		if existing.Kind == yaml.ScalarNode && existing.Value == value && existing.Tag == tag {
			return false
		}
		*existing = *yamlScalar(value, tag)
		return true
	}
	parent.Content = append(parent.Content, yamlScalar(key, "!!str"), yamlScalar(value, tag))
	return true
}

func yamlSequenceContains(seq *yaml.Node, value string) bool {
	for _, item := range seq.Content {
		if item.Kind == yaml.ScalarNode && strings.TrimSpace(item.Value) == value {
			return true
		}
	}
	return false
}

// ensureGortexWatchConfig makes the repository watcher explicit using the
// official YAML shape and opts Markdown/prose nodes into the text index without
// replacing unrelated user settings. Test files are indexed by Gortex's normal
// walker unless a repository's own ignore rules exclude them; we deliberately
// do not add broad Include globs because Include is applied after all excludes
// and would re-admit dependency/build test trees such as node_modules.
// Missing repository directories are ignored so a stale daemon entry cannot
// be recreated by the background enforcer.
func ensureGortexWatchConfig(project string) error {
	project = filepath.Clean(project)
	info, err := os.Stat(project)
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	if err != nil {
		return err
	}
	if !info.IsDir() {
		return fmt.Errorf("Gortex track 路径不是目录: %s", project)
	}

	path := filepath.Join(project, ".gortex.yaml")
	data, err := os.ReadFile(path)
	if errors.Is(err, os.ErrNotExist) {
		data = []byte("{}\n")
	} else if err != nil {
		return err
	}
	var document yaml.Node
	if len(bytes.TrimSpace(data)) == 0 {
		document = yaml.Node{Kind: yaml.DocumentNode, Content: []*yaml.Node{{Kind: yaml.MappingNode, Tag: "!!map"}}}
	} else if err := yaml.Unmarshal(data, &document); err != nil {
		return fmt.Errorf("解析 %s 失败: %w", path, err)
	}
	if len(document.Content) == 0 || document.Content[0].Kind != yaml.MappingNode {
		return fmt.Errorf("解析 %s 失败: 根节点必须是 YAML 对象", path)
	}
	root := document.Content[0]
	changed := false

	watch, created, err := yamlEnsureMappingChild(root, "watch")
	if err != nil {
		return err
	}
	changed = changed || created
	changed = yamlEnsureScalar(watch, "enabled", "true", "!!bool") || changed
	changed = yamlEnsureScalar(watch, "debounce_ms", fmt.Sprint(gortexWatchDebounceMilliseconds), "!!int") || changed

	search, created, err := yamlEnsureMappingChild(root, "search")
	if err != nil {
		return err
	}
	changed = changed || created
	changed = yamlEnsureScalar(search, "index_prose", "true", "!!bool") || changed

	if !changed {
		return nil
	}
	var encoded bytes.Buffer
	encoder := yaml.NewEncoder(&encoded)
	encoder.SetIndent(2)
	if err := encoder.Encode(&document); err != nil {
		_ = encoder.Close()
		return err
	}
	if err := encoder.Close(); err != nil {
		return err
	}
	return replaceUTF8File(path, encoded.Bytes())
}

func startGortexWatchEnforcer() {
	enforceGortexWatchConfigs()
	go func() {
		ticker := time.NewTicker(5 * time.Second)
		defer ticker.Stop()
		for range ticker.C {
			enforceGortexWatchConfigs()
		}
	}()
}

func enforceGortexWatchConfigs() {
	projects := []string{}
	if local, err := gortexTrackedProjects(); err == nil {
		projects = append(projects, local...)
	}
	// Include repositories tracked outside this manager but visible to the same
	// Gortex daemon, so their watch flag is repaired as well.
	projects = append(projects, gortexDaemonTrackedProjects(gortexManagedExecutablePath())...)
	for _, project := range normalizeGortexProjects(projects) {
		if err := ensureGortexWatchConfig(project); err != nil {
			log.Printf("Gortex watcher 配置修复失败（%s）: %v", project, err)
		}
	}
}

const (
	gortexRulesStartMarker = "<!-- code-manager:gortex:rules:start -->"
	gortexRulesEndMarker   = "<!-- code-manager:gortex:rules:end -->"
	gortexArtifactPrompt   = "prompt:"
	gortexArtifactHook     = "hook:"
	gortexPromptFileName   = "gortex提示词.md"
)

//go:embed assets/gortex提示词.md
var gortexPromptDocument []byte

// gortexInstructionBody is the complete, marker-free source document shipped
// with the managed Gortex installation. Integration code alone adds and removes
// the marker pair around this body in agent configuration files.
var gortexInstructionBody = embeddedGortexInstructionBody(gortexPromptDocument)

func embeddedGortexInstructionBody(data []byte) string {
	return normalizeGortexLineEndings(string(data))
}

func writeGortexPromptDocument(installRoot string) error {
	if strings.TrimSpace(installRoot) == "" {
		return errors.New("Gortex 安装目录不能为空")
	}
	if len(gortexPromptDocument) == 0 {
		return errors.New("内置 Gortex 提示词为空")
	}
	if err := validateEmbeddedRTKDocument(gortexPromptFileName, gortexPromptDocument); err != nil {
		return err
	}
	return writeEmbeddedRTKDocument(installRoot, gortexPromptFileName, gortexPromptDocument)
}

type gortexOwnedArtifact struct {
	Kind               string `json:"kind"`
	Agent              string `json:"agent"`
	Path               string `json:"path"`
	Event              string `json:"event,omitempty"`
	Fingerprint        string `json:"fingerprint"`
	Command            string `json:"command,omitempty"`
	GroupFingerprint   string `json:"group_fingerprint,omitempty"`
	HandlerFingerprint string `json:"handler_fingerprint,omitempty"`
}

func gortexPromptPaths(agent string) []string {
	profile := userProfileDir()
	if profile == "" {
		return nil
	}
	switch agent {
	case "codex":
		return []string{filepath.Join(profile, ".codex", "AGENTS.md")}
	case "claude":
		return []string{filepath.Join(gortexClaudeConfigDir(), "CLAUDE.md")}
	case "copilot":
		return []string{filepath.Join(gortexCopilotConfigDir(), "copilot-instructions.md")}
	default:
		return nil
	}
}

func gortexCursorRulePath(project string) string {
	return filepath.Join(project, ".cursor", "rules", "gortex-workflow.mdc")
}

func gortexHookPath(agent string) string {
	profile := userProfileDir()
	if profile == "" {
		return ""
	}
	switch agent {
	case "codex":
		return filepath.Join(profile, ".codex", "config.toml")
	case "claude":
		return filepath.Join(gortexClaudeConfigDir(), "settings.local.json")
	case "copilot":
		return filepath.Join(gortexCopilotConfigDir(), "hooks", "gortex.json")
	default:
		return ""
	}
}

func gortexArtifactFingerprint(data []byte) string {
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:])
}

func gortexJSONArtifactFingerprint(value any) string {
	data, err := json.Marshal(value)
	if err != nil {
		return ""
	}
	return gortexArtifactFingerprint(data)
}

func gortexMarkedBlock(data []byte) (string, bool) {
	text := string(data)
	start := strings.Index(text, gortexRulesStartMarker)
	end := strings.LastIndex(text, gortexRulesEndMarker)
	if start < 0 || end < start {
		return "", false
	}
	end += len(gortexRulesEndMarker)
	return text[start:end], true
}

func gortexPromptBlock(body string) string {
	body = normalizeGortexLineEndings(body)
	separator := "\n"
	if strings.HasSuffix(body, "\n") {
		separator = ""
	}
	return gortexRulesStartMarker + "\n" + body + separator + gortexRulesEndMarker + "\n"
}

// Cursor treats .mdc files as rules only when they carry YAML frontmatter.
// Keep this prefix separate from the shared Markdown instruction block so the
// other agents receive ordinary AGENTS.md/CLAUDE.md-style content.
const gortexCursorFrontmatter = "---\ndescription: Gortex code intelligence - prefer graph tools over file reads\nalwaysApply: true\n---\n\n"

func normalizeGortexLineEndings(value string) string {
	value = strings.ReplaceAll(value, "\r\n", "\n")
	return strings.ReplaceAll(value, "\r", "\n")
}

func gortexCursorFrontmatterInfo(value string) (end int, present, valid bool) {
	value = normalizeGortexLineEndings(value)
	if !strings.HasPrefix(value, "---\n") {
		return 0, false, false
	}
	separator := strings.Index(value[len("---\n"):], "\n---\n")
	if separator < 0 {
		return 0, true, false
	}
	end = len("---\n") + separator + len("\n---\n")
	frontmatter := value[:end]
	for _, line := range strings.Split(frontmatter, "\n") {
		if strings.EqualFold(strings.TrimSpace(line), "alwaysApply: true") {
			return end, true, true
		}
	}
	return end, true, false
}

func gortexCursorRepairFrontmatter(value string) (string, error) {
	value = normalizeGortexLineEndings(value)
	end, present, valid := gortexCursorFrontmatterInfo(value)
	if !present {
		return gortexCursorFrontmatter + value, nil
	}
	if end == 0 || !strings.HasPrefix(value, "---\n") {
		return "", errors.New("Cursor MDC frontmatter 无效")
	}
	if valid {
		return value, nil
	}
	// A file carrying our marker is ours to repair. Preserve custom metadata,
	// but force the one field that controls whether Cursor loads the rule.
	frontmatter := value[:end]
	lines := strings.Split(frontmatter, "\n")
	found := false
	for i, line := range lines {
		if strings.HasPrefix(strings.ToLower(strings.TrimSpace(line)), "alwaysapply:") {
			lines[i] = "alwaysApply: true"
			found = true
		}
	}
	if !found {
		insertAt := len(lines) - 2
		if insertAt < 1 {
			return "", errors.New("Cursor MDC frontmatter 无法修复")
		}
		lines = append(lines[:insertAt], append([]string{"alwaysApply: true"}, lines[insertAt:]...)...)
	}
	return strings.Join(lines, "\n") + value[end:], nil
}

func gortexCanonicalPromptFingerprint() string {
	return gortexArtifactFingerprint([]byte(gortexPromptBlock(gortexInstructionBody)))
}

func upsertGortexPrompt(path string, body string) (bool, string, error) {
	data, err := os.ReadFile(path)
	if errors.Is(err, os.ErrNotExist) {
		data = nil
	} else if err != nil {
		return false, "", err
	}
	block := gortexPromptBlock(body)
	text := string(data)
	start := strings.Index(text, gortexRulesStartMarker)
	endMarker := strings.LastIndex(text, gortexRulesEndMarker)
	var next string
	if start >= 0 && endMarker >= start {
		end := endMarker + len(gortexRulesEndMarker)
		if end < len(text) && text[end] == '\n' {
			end++
		}
		next = text[:start] + block + text[end:]
	} else if strings.TrimSpace(text) == "" {
		next = block
	} else {
		prefix := "\n\n"
		if strings.HasSuffix(text, "\n") {
			prefix = "\n"
		}
		next = text + prefix + block
	}
	if next == text {
		return false, gortexArtifactFingerprint([]byte(block)), nil
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return false, "", err
	}
	if err := replaceUTF8File(path, []byte(next)); err != nil {
		return false, "", err
	}
	return true, gortexArtifactFingerprint([]byte(block)), nil
}

// upsertGortexCursorPrompt is the Cursor-specific counterpart of
// upsertGortexPrompt. An unmarked file without frontmatter is treated as a
// user-owned rule and is never silently rewritten.
func upsertGortexCursorPrompt(path string, body string) (bool, string, error) {
	data, err := os.ReadFile(path)
	if errors.Is(err, os.ErrNotExist) {
		data = nil
	} else if err != nil {
		return false, "", err
	}
	text := normalizeGortexLineEndings(string(data))
	block := gortexPromptBlock(body)
	start := strings.Index(text, gortexRulesStartMarker)
	endMarker := strings.LastIndex(text, gortexRulesEndMarker)
	var next string
	if start >= 0 && endMarker >= start {
		end := endMarker + len(gortexRulesEndMarker)
		if end < len(text) && text[end] == '\n' {
			end++
		}
		next = text[:start] + block + text[end:]
		if _, present, valid := gortexCursorFrontmatterInfo(next); !present {
			next = gortexCursorFrontmatter + next
		} else if !valid {
			var repairErr error
			next, repairErr = gortexCursorRepairFrontmatter(next)
			if repairErr != nil {
				return false, "", repairErr
			}
		}
	} else if strings.TrimSpace(text) == "" {
		next = gortexCursorFrontmatter + block
	} else {
		frontEnd, present, valid := gortexCursorFrontmatterInfo(text)
		if !present {
			return false, "", fmt.Errorf("Cursor 规则文件 %s 没有 frontmatter，已保留用户文件", path)
		}
		if !valid {
			return false, "", fmt.Errorf("Cursor 规则文件 %s 的 frontmatter 未启用 alwaysApply，已保留用户文件", path)
		}
		// Keep any user-authored frontmatter and prose, inserting our block
		// immediately after the YAML header so the MDC remains valid.
		next = text[:frontEnd] + block + text[frontEnd:]
	}
	if next == text {
		return false, gortexArtifactFingerprint([]byte(block)), nil
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return false, "", err
	}
	if err := replaceUTF8File(path, []byte(next)); err != nil {
		return false, "", err
	}
	return true, gortexArtifactFingerprint([]byte(block)), nil
}

func removeGortexPrompt(path, fingerprint string) (bool, error) {
	data, err := os.ReadFile(path)
	if errors.Is(err, os.ErrNotExist) {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	block, ok := gortexMarkedBlock(data)
	if !ok {
		return false, nil
	}
	if fingerprint == "" {
		// Recovery without a ledger is intentionally conservative. A marker
		// alone is not proof that the user did not edit the managed block;
		// only the exact current Gortex body is removed automatically.
		fingerprint = gortexCanonicalPromptFingerprint()
	}
	if gortexArtifactFingerprint([]byte(block+"\n")) != fingerprint && gortexArtifactFingerprint([]byte(block)) != fingerprint {
		return false, fmt.Errorf("提示词文件 %s 已被修改，已保留", path)
	}
	text := string(data)
	start := strings.Index(text, gortexRulesStartMarker)
	end := strings.LastIndex(text, gortexRulesEndMarker) + len(gortexRulesEndMarker)
	if end < len(text) && text[end] == '\n' {
		end++
	}
	next := text[:start] + text[end:]
	next = strings.TrimRight(next, "\n")
	if next != "" {
		next += "\n"
	}
	if strings.EqualFold(filepath.Ext(path), ".mdc") && strings.TrimSpace(normalizeGortexLineEndings(next)) == strings.TrimSpace(gortexCursorFrontmatter) {
		if err := os.Remove(path); err != nil && !errors.Is(err, os.ErrNotExist) {
			return false, err
		}
		return true, nil
	}
	if err := replaceUTF8File(path, []byte(next)); err != nil {
		return false, err
	}
	return true, nil
}

func gortexHookCommand(agent, executable string) string {
	switch agent {
	case "codex":
		return quoteWindowsExecutable(executable) + " hook --agent=codex --mode=enrich"
	case "claude":
		return gortexClaudeHookCommand(executable)
	case "copilot":
		return quoteWindowsExecutable(executable) + " hook --agent=copilot-cli"
	default:
		return quoteWindowsExecutable(executable) + " hook"
	}
}

// Claude Code executes hook commands through a POSIX-compatible shell even on
// Windows. Backslashes in a native path are therefore escape characters, not
// path separators; use forward slashes and quote only when the path needs it.
func gortexClaudeHookCommand(executable string) string {
	pathValue := strings.ReplaceAll(strings.TrimSpace(executable), `\`, "/")
	if pathValue == "" {
		return "gortex hook"
	}
	if strings.ContainsAny(pathValue, " \t\n\"'\\$`&;|<>()*?[]{}#~!") {
		return "'" + strings.ReplaceAll(pathValue, "'", `'\''`) + "' hook"
	}
	return pathValue + " hook"
}

func gortexHookEntry(command, matcher, status string) map[string]any {
	inner := map[string]any{"type": "command", "command": command, "timeout": 10, "statusMessage": status}
	entry := map[string]any{"hooks": []any{inner}}
	if matcher != "" {
		entry["matcher"] = matcher
	}
	return entry
}

func gortexClaudeHookEntry(command, matcher, status string, timeout int) map[string]any {
	inner := map[string]any{"type": "command", "command": command, "timeout": timeout, "statusMessage": status}
	entry := map[string]any{"hooks": []any{inner}}
	if matcher != "" {
		entry["matcher"] = matcher
	}
	return entry
}

func gortexJSONHookEntryIsOurs(value any, executable string) bool {
	entry, ok := value.(map[string]any)
	if !ok {
		return false
	}
	hooks := gortexHookList(entry["hooks"])
	for _, raw := range hooks {
		item, _ := raw.(map[string]any)
		command, _ := item["command"].(string)
		if gortexClaudeHookCommandMatches(command, executable) {
			return true
		}
	}
	return false
}

func gortexNestedCommandHandler(value any) map[string]any {
	entry, ok := value.(map[string]any)
	if !ok {
		return nil
	}
	for _, raw := range gortexHookList(entry["hooks"]) {
		item, ok := raw.(map[string]any)
		if !ok {
			continue
		}
		if command := gortexTomlEffectiveCommand(item); command != "" {
			return item
		}
	}
	return nil
}

func gortexTomlEffectiveCommand(value any) string {
	entry, ok := value.(map[string]any)
	if !ok {
		return ""
	}
	if command, ok := entry["commandWindows"].(string); ok && strings.TrimSpace(command) != "" {
		return command
	}
	if command, ok := entry["command"].(string); ok && strings.TrimSpace(command) != "" {
		return command
	}
	return ""
}

func gortexDirectHookCommand(value any) string {
	entry, ok := value.(map[string]any)
	if !ok {
		return ""
	}
	for _, key := range []string{"bash", "powershell", "command"} {
		if command, ok := entry[key].(string); ok && strings.TrimSpace(command) != "" {
			return command
		}
	}
	return ""
}

// BurntSushi decodes TOML arrays of tables as []map[string]interface{}, while
// JSON uses []any. Normalize both forms before inspecting or rewriting hooks.
func gortexHookList(value any) []any {
	switch list := value.(type) {
	case []any:
		return list
	case []map[string]any:
		result := make([]any, len(list))
		for i := range list {
			result[i] = list[i]
		}
		return result
	default:
		return nil
	}
}

func upsertClaudeHooks(path, executable string) (bool, []gortexOwnedArtifact, error) {
	root := map[string]any{}
	if data, err := os.ReadFile(path); err == nil {
		if err := json.Unmarshal(data, &root); err != nil {
			return false, nil, fmt.Errorf("解析 Claude Hook 配置失败: %w", err)
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return false, nil, err
	}
	hooks := map[string]any{}
	if raw, exists := root["hooks"]; exists {
		var ok bool
		hooks, ok = raw.(map[string]any)
		if !ok {
			return false, nil, errors.New("Claude Hook 配置的 hooks 必须是对象，已保留用户配置")
		}
	}
	command := gortexHookCommand("claude", executable)
	desired := map[string]map[string]any{
		"SessionStart":     gortexClaudeHookEntry(command, "", "Loading Gortex graph orientation...", 3000),
		"UserPromptSubmit": gortexClaudeHookEntry(command, "", "Surfacing Gortex graph context...", 3000),
		"PreToolUse":       gortexClaudeHookEntry(command, "*", "Checking Gortex graph guidance...", 3000),
		"PostToolUse":      gortexClaudeHookEntry(command, "Read|Grep|Glob", "Loading Gortex post-tool context...", 3000),
		"Stop":             gortexClaudeHookEntry(command, "", "Checking Gortex evidence authority...", 5000),
		"PreCompact":       gortexClaudeHookEntry(command, "", "Injecting Gortex orientation snapshot...", 3000),
		"SubagentStart":    gortexClaudeHookEntry(command, "", "Starting an isolated Gortex subagent turn...", 3000),
		"SubagentStop":     gortexClaudeHookEntry(command, "", "Clearing Gortex subagent turn state...", 3000),
	}
	changed := false
	owned := []gortexOwnedArtifact{}
	for event, want := range desired {
		list := gortexHookList(hooks[event])
		found := false
		managed := any(want)
		kept := make([]any, 0, len(list)+1)
		for _, item := range list {
			if !gortexJSONHookEntryIsOurs(item, executable) {
				kept = append(kept, item)
				continue
			}
			if !found {
				found = true
				if gortexFingerprint(item) == gortexFingerprint(want) {
					kept = append(kept, item)
					managed = item
				} else {
					kept = append(kept, want)
					changed = true
				}
			} else {
				changed = true
			}
		}
		if !found {
			kept = append(kept, want)
			changed = true
		}
		hooks[event] = kept
		handler := gortexNestedCommandHandler(managed)
		owned = append(owned, gortexOwnedArtifact{
			Kind: gortexArtifactHook, Agent: "claude", Path: path, Event: event,
			Fingerprint: gortexJSONArtifactFingerprint(managed),
			Command: func() string {
				if handler == nil {
					return ""
				}
				return gortexTomlEffectiveCommand(handler)
			}(),
			GroupFingerprint:   gortexFingerprint(managed),
			HandlerFingerprint: gortexFingerprint(handler),
		})
	}
	root["hooks"] = hooks
	data, err := json.MarshalIndent(root, "", "  ")
	if err != nil {
		return false, nil, err
	}
	if changed || !fileExists(path) {
		if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
			return false, nil, err
		}
		if err := replaceUTF8File(path, append(data, '\n')); err != nil {
			return false, nil, err
		}
	}
	return changed, owned, nil
}

func removeClaudeHooks(path, executable string) (bool, error) {
	data, err := os.ReadFile(path)
	if errors.Is(err, os.ErrNotExist) {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	root := map[string]any{}
	if err := json.Unmarshal(data, &root); err != nil {
		return false, err
	}
	hooks, _ := root["hooks"].(map[string]any)
	changed := false
	for _, event := range []string{"SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse", "Stop", "PreCompact", "SubagentStart", "SubagentStop"} {
		list := gortexHookList(hooks[event])
		if list == nil {
			continue
		}
		kept := make([]any, 0, len(list))
		for _, item := range list {
			if gortexJSONHookEntryIsOurs(item, executable) {
				changed = true
				continue
			}
			kept = append(kept, item)
		}
		if len(kept) == 0 {
			delete(hooks, event)
		} else {
			hooks[event] = kept
		}
	}
	if !changed {
		return false, nil
	}
	if len(hooks) == 0 {
		delete(root, "hooks")
	}
	encoded, _ := json.MarshalIndent(root, "", "  ")
	return true, replaceUTF8File(path, append(encoded, '\n'))
}

func upsertCopilotHooks(path, executable string) (bool, []gortexOwnedArtifact, error) {
	root := map[string]any{}
	if data, err := os.ReadFile(path); err == nil {
		if err := json.Unmarshal(data, &root); err != nil {
			return false, nil, err
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return false, nil, err
	}
	changed := false
	if _, ok := root["version"]; !ok {
		root["version"] = 1
		changed = true
	}
	if _, ok := root["disableAllHooks"]; !ok {
		root["disableAllHooks"] = false
		changed = true
	}
	hooks := map[string]any{}
	if raw, exists := root["hooks"]; exists {
		var ok bool
		hooks, ok = raw.(map[string]any)
		if !ok {
			return false, nil, errors.New("Copilot Hook 配置的 hooks 必须是对象，已保留用户配置")
		}
	}
	events := []struct {
		name    string
		matcher string
	}{
		{name: "sessionStart"},
		{name: "userPromptSubmitted"},
		{name: "preToolUse", matcher: "^(?:bash|create|edit|glob|grep|powershell|rg|view)$"},
		{name: "postToolUse", matcher: "^(?:bash|create|edit|glob|grep|powershell|rg|view)$"},
	}
	owned := []gortexOwnedArtifact{}
	bashCommand, powershellCommand := gortexCopilotHookCommands(executable)
	for _, event := range events {
		want := gortexCopilotHookEntry(bashCommand, powershellCommand, event.matcher)
		list, ok := gortexCopilotHookList(hooks[event.name])
		if !ok {
			return changed, owned, fmt.Errorf("Copilot Hook 事件 %s 不是数组，已保留用户配置", event.name)
		}
		found := false
		managed := any(want)
		kept := make([]any, 0, len(list)+1)
		for _, item := range list {
			if !gortexCopilotHookEntryIsOurs(item, executable) {
				kept = append(kept, item)
				continue
			}
			if !found {
				found = true
				// Preserve an intentionally narrowed *native* entry. The
				// previous manager build used Claude's grouped `{hooks:[...]}`
				// shape here; that shape is never retained because Copilot CLI
				// only consumes direct `bash`/`powershell` entries.
				if gortexCopilotNativeHookEntry(item) &&
					(gortexCopilotMatcherNarrowed(item, event.matcher) || gortexFingerprint(item) == gortexFingerprint(want)) {
					kept = append(kept, item)
					managed = item
				} else {
					kept = append(kept, want)
					changed = true
				}
			} else {
				changed = true
			}
		}
		if !found {
			kept = append(kept, want)
			changed = true
		}
		hooks[event.name] = kept
		owned = append(owned, gortexOwnedArtifact{
			Kind: gortexArtifactHook, Agent: "copilot", Path: path, Event: event.name,
			Fingerprint:      gortexJSONArtifactFingerprint(managed),
			Command:          gortexDirectHookCommand(managed),
			GroupFingerprint: gortexFingerprint(managed),
		})
	}
	root["hooks"] = hooks
	encoded, err := json.MarshalIndent(root, "", "  ")
	if err != nil {
		return false, nil, err
	}
	if changed || !fileExists(path) {
		if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
			return false, nil, err
		}
		if err := replaceUTF8File(path, append(encoded, '\n')); err != nil {
			return false, nil, err
		}
	}
	return changed, owned, nil
}

func removeCopilotHooks(path, executable string) (bool, error) {
	data, err := os.ReadFile(path)
	if errors.Is(err, os.ErrNotExist) {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	root := map[string]any{}
	if err := json.Unmarshal(data, &root); err != nil {
		return false, err
	}
	hooks, _ := root["hooks"].(map[string]any)
	if hooks == nil {
		return false, nil
	}
	changed := false
	for _, event := range []string{"sessionStart", "userPromptSubmitted", "preToolUse", "postToolUse"} {
		list, ok := gortexCopilotHookList(hooks[event])
		if !ok {
			continue
		}
		kept := make([]any, 0, len(list))
		for _, item := range list {
			if gortexCopilotHookEntryIsOurs(item, executable) {
				changed = true
				continue
			}
			kept = append(kept, item)
		}
		if len(kept) == 0 {
			delete(hooks, event)
		} else {
			hooks[event] = kept
		}
	}
	if !changed {
		return false, nil
	}
	if len(hooks) == 0 {
		delete(root, "hooks")
	}
	if len(root) == 2 && root["version"] != nil && root["disableAllHooks"] != nil && root["hooks"] == nil {
		return true, os.Remove(path)
	}
	encoded, _ := json.MarshalIndent(root, "", "  ")
	return true, replaceUTF8File(path, append(encoded, '\n'))
}

func gortexCopilotHookCommands(executable string) (bash, powershell string) {
	clean := filepath.Clean(strings.TrimSpace(executable))
	posix := strings.ReplaceAll(clean, `\`, "/")
	if posix == "" {
		bash = "gortex hook --agent=copilot-cli"
	} else if strings.ContainsAny(posix, " \t\n\"'\\$`&;|<>()*?[]{}#~!") {
		bash = "'" + strings.ReplaceAll(posix, "'", `'\''`) + "' hook --agent=copilot-cli"
	} else {
		bash = posix + " hook --agent=copilot-cli"
	}
	if clean == "" {
		powershell = "gortex hook --agent=copilot-cli"
	} else {
		powershell = "& " + quotePowerShellString(clean) + " hook --agent=copilot-cli"
	}
	return bash, powershell
}

func gortexCopilotHookEntry(bash, powershell, matcher string) map[string]any {
	entry := map[string]any{
		"type":       "command",
		"bash":       bash,
		"powershell": powershell,
		"cwd":        ".",
		"timeoutSec": 10,
	}
	if matcher != "" {
		entry["matcher"] = matcher
	}
	return entry
}

func gortexCopilotHookList(value any) ([]any, bool) {
	if value == nil {
		return nil, true
	}
	switch list := value.(type) {
	case []any:
		return append([]any(nil), list...), true
	case []map[string]any:
		out := make([]any, 0, len(list))
		for _, item := range list {
			out = append(out, item)
		}
		return out, true
	default:
		return nil, false
	}
}

func gortexCopilotHookEntryIsOurs(value any, executable string) bool {
	entry, ok := value.(map[string]any)
	if !ok {
		return false
	}
	for _, key := range []string{"bash", "powershell", "command"} {
		command, _ := entry[key].(string)
		if gortexCopilotCommandMatches(command, executable) {
			return true
		}
	}
	// Migrate the nested shape written by the previous code-Manager build.
	// Copilot's identity is the agent flag, not the absolute executable path:
	// a release upgrade necessarily changes that path while the Hook remains
	// ours. Keep this fallback separate from Claude's grouped schema so an old
	// nested entry is always rewritten to Copilot's native direct-entry form.
	for _, raw := range gortexHookList(entry["hooks"]) {
		item, ok := raw.(map[string]any)
		if !ok {
			continue
		}
		command, _ := item["command"].(string)
		if gortexCopilotCommandMatches(command, executable) {
			return true
		}
	}
	return false
}

func gortexCopilotCommandMatches(command, executable string) bool {
	return gortexCommandInvokesAgentHook(command, "copilot-cli")
}

func gortexClaudeHookCommandMatches(command, executable string) bool {
	_ = executable // a stale absolute path must still migrate on upgrade
	return gortexCommandInvokesAgentHook(command, "")
}

func gortexCodexHookCommandMatches(command, executable string) bool {
	_ = executable // Codex identity is the agent flag, not the binary path
	return gortexCommandInvokesAgentHook(command, "codex")
}

func gortexCommandInvokesAgentHook(command, agent string) bool {
	command = strings.TrimSpace(command)
	if command == "" {
		return false
	}
	lower := strings.ToLower(command)
	if !strings.Contains(lower, "gortex") || !strings.Contains(lower, "hook") {
		return false
	}
	if strings.TrimSpace(agent) == "" {
		return true
	}
	compact := strings.NewReplacer(" ", "", "\t", "", "\r", "", "\n", "").Replace(lower)
	if strings.Contains(compact, "--agent="+strings.ToLower(agent)) {
		return true
	}
	fields := strings.Fields(lower)
	for i := 0; i+1 < len(fields); i++ {
		if fields[i] == "--agent" && fields[i+1] == strings.ToLower(agent) {
			return true
		}
	}
	return false
}

func gortexCopilotNativeHookEntry(value any) bool {
	entry, ok := value.(map[string]any)
	if !ok {
		return false
	}
	if _, nested := entry["hooks"]; nested {
		return false
	}
	typeName, _ := entry["type"].(string)
	if typeName != "command" {
		return false
	}
	for _, key := range []string{"bash", "powershell", "command"} {
		if command, ok := entry[key].(string); ok && strings.TrimSpace(command) != "" {
			return true
		}
	}
	return false
}

func gortexCopilotMatcherNarrowed(value any, full string) bool {
	entry, ok := value.(map[string]any)
	if !ok || full == "" {
		return false
	}
	got, ok := matcherAlternation(entry["matcher"])
	if !ok || len(got) == 0 {
		return false
	}
	want, ok := matcherAlternation(full)
	if !ok || len(got) >= len(want) {
		return false
	}
	allowed := map[string]bool{}
	for _, name := range want {
		allowed[name] = true
	}
	for _, name := range got {
		if !allowed[name] {
			return false
		}
	}
	return true
}

func matcherAlternation(value any) ([]string, bool) {
	text, ok := value.(string)
	if !ok {
		return nil, false
	}
	if !strings.HasPrefix(text, "^(?:") || !strings.HasSuffix(text, ")$") {
		return nil, false
	}
	inner := strings.TrimSuffix(strings.TrimPrefix(text, "^(?:"), ")$")
	parts := strings.Split(inner, "|")
	result := make([]string, 0, len(parts))
	for _, part := range parts {
		part = strings.TrimSpace(part)
		if part == "" {
			return nil, false
		}
		result = append(result, part)
	}
	return result, true
}

func gortexTomlHookCommand(value any, executable string) bool {
	entry, ok := value.(map[string]any)
	if !ok {
		return false
	}
	if command := gortexTomlEffectiveCommand(entry); gortexCodexHookCommandMatches(command, executable) {
		return true
	}
	for _, raw := range gortexHookList(entry["hooks"]) {
		item, ok := raw.(map[string]any)
		if !ok {
			continue
		}
		if command := gortexTomlEffectiveCommand(item); gortexCodexHookCommandMatches(command, executable) {
			return true
		}
	}
	return false
}

func upsertCodexHooks(path, executable string) (bool, []gortexOwnedArtifact, error) {
	data, err := os.ReadFile(path)
	if errors.Is(err, os.ErrNotExist) {
		data = nil
	} else if err != nil {
		return false, nil, err
	}
	root := map[string]any{}
	if len(data) > 0 {
		if _, err := tomlDecode(string(data), &root); err != nil {
			return false, nil, fmt.Errorf("解析 Codex config.toml 失败: %w", err)
		}
	}
	hooks := map[string]any{}
	if raw, exists := root["hooks"]; exists {
		var ok bool
		hooks, ok = raw.(map[string]any)
		if !ok {
			return false, nil, errors.New("Codex Hook 配置的 hooks 必须是对象，已保留用户配置")
		}
	}
	command := gortexHookCommand("codex", executable)
	events := map[string]map[string]any{
		"SessionStart":     {"matcher": "startup|resume|clear|compact", "hooks": []any{map[string]any{"type": "command", "command": command, "timeout": 10, "statusMessage": "Loading Gortex graph orientation..."}}},
		"UserPromptSubmit": {"hooks": []any{map[string]any{"type": "command", "command": command, "timeout": 10, "statusMessage": "Surfacing Gortex graph context..."}}},
		"PreToolUse":       {"matcher": ".*", "hooks": []any{map[string]any{"type": "command", "command": command, "timeout": 5, "statusMessage": "Loading Gortex tool guidance..."}}},
		"PostToolUse":      {"matcher": "^(Bash|apply_patch|(mcp__gortex__|gortex__)(explore|search|read|relations|trace|analyze))$", "hooks": []any{map[string]any{"type": "command", "command": command, "timeout": 5, "statusMessage": "Loading Gortex post-tool context..."}}},
		"Stop":             {"hooks": []any{map[string]any{"type": "command", "command": command, "timeout": 10, "statusMessage": "Checking Gortex evidence authority..."}}},
	}
	changed := false
	owned := []gortexOwnedArtifact{}
	for event, want := range events {
		list := gortexHookList(hooks[event])
		found := false
		managed := any(want)
		kept := make([]any, 0, len(list)+1)
		for _, item := range list {
			if !gortexTomlHookCommand(item, executable) {
				kept = append(kept, item)
				continue
			}
			if !found {
				found = true
				if gortexFingerprint(item) == gortexFingerprint(want) {
					kept = append(kept, item)
					managed = item
				} else {
					kept = append(kept, want)
					changed = true
				}
			} else {
				changed = true
			}
		}
		if !found {
			kept = append(kept, want)
			changed = true
		}
		hooks[event] = kept
		handler := gortexNestedCommandHandler(managed)
		commandValue := ""
		if handler != nil {
			commandValue = gortexTomlEffectiveCommand(handler)
		}
		owned = append(owned, gortexOwnedArtifact{
			Kind: gortexArtifactHook, Agent: "codex", Path: path, Event: event,
			Fingerprint: gortexJSONArtifactFingerprint(managed), Command: commandValue,
			GroupFingerprint: gortexFingerprint(managed), HandlerFingerprint: gortexFingerprint(handler),
		})
	}
	root["hooks"] = hooks
	encoded, err := tomlEncode(root)
	if err != nil {
		return false, nil, err
	}
	if changed || !fileExists(path) {
		if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
			return false, nil, err
		}
		if err := replaceUTF8File(path, []byte(encoded)); err != nil {
			return false, nil, err
		}
	}
	return changed, owned, nil
}

func removeCodexHooks(path, executable string) (bool, error) {
	data, err := os.ReadFile(path)
	if errors.Is(err, os.ErrNotExist) {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	root := map[string]any{}
	if _, err := tomlDecode(string(data), &root); err != nil {
		return false, err
	}
	hooks, _ := root["hooks"].(map[string]any)
	changed := false
	for _, event := range []string{"SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse", "Stop"} {
		list := gortexHookList(hooks[event])
		if list == nil {
			continue
		}
		kept := make([]any, 0, len(list))
		for _, item := range list {
			if gortexTomlHookCommand(item, executable) {
				changed = true
				continue
			}
			kept = append(kept, item)
		}
		if len(kept) == 0 {
			delete(hooks, event)
		} else {
			hooks[event] = kept
		}
	}
	if !changed {
		return false, nil
	}
	if len(hooks) == 0 {
		delete(root, "hooks")
	} else {
		root["hooks"] = hooks
	}
	encoded, err := tomlEncode(root)
	if err != nil {
		return false, err
	}
	return true, replaceUTF8File(path, []byte(encoded))
}

// Small wrappers keep the integration code independent from the TOML package
// details used by the existing MCP writer.
func tomlDecode(data string, target *map[string]any) (map[string]any, error) {
	_, err := toml.Decode(data, target)
	return *target, err
}
func tomlEncode(root map[string]any) (string, error) {
	var builder strings.Builder
	err := toml.NewEncoder(&builder).Encode(root)
	return builder.String(), err
}

func gortexRegisterIntegrations(executable string) ([]gortexOwnedArtifact, []string) {
	var artifacts []gortexOwnedArtifact
	var warnings []string
	for _, agent := range []string{"codex", "claude", "copilot"} {
		if !gortexAgentAvailable(agent) {
			continue
		}
		for _, path := range gortexPromptPaths(agent) {
			changed, fingerprint, err := upsertGortexPrompt(path, gortexInstructionBody)
			if err != nil {
				warnings = append(warnings, agent+" prompt: "+err.Error())
				continue
			}
			if changed || fileExists(path) {
				artifacts = append(artifacts, gortexOwnedArtifact{Kind: gortexArtifactPrompt, Agent: agent, Path: path, Fingerprint: fingerprint})
			}
		}
		var changed bool
		var hooks []gortexOwnedArtifact
		var err error
		switch agent {
		case "codex":
			changed, hooks, err = upsertCodexHooks(gortexHookPath(agent), executable)
		case "claude":
			changed, hooks, err = upsertClaudeHooks(gortexHookPath(agent), executable)
		case "copilot":
			changed, hooks, err = upsertCopilotHooks(gortexHookPath(agent), executable)
		}
		_ = changed
		if err != nil {
			warnings = append(warnings, agent+" hook: "+err.Error())
		} else {
			artifacts = append(artifacts, hooks...)
		}
	}
	return artifacts, warnings
}

func gortexRemoveIntegrations(executable string, ownership gortexMCPOwnership) []string {
	return gortexRemoveIntegrationsOwned(executable, &ownership)
}

func gortexHookArtifactMatches(artifact gortexOwnedArtifact, group, handler any, command string) bool {
	if artifact.Command != "" && command != artifact.Command {
		return false
	}
	if artifact.GroupFingerprint != "" && gortexFingerprint(group) != artifact.GroupFingerprint {
		return false
	}
	if artifact.HandlerFingerprint != "" && gortexFingerprint(handler) != artifact.HandlerFingerprint {
		return false
	}
	if artifact.GroupFingerprint == "" && artifact.HandlerFingerprint == "" {
		// Compatibility with ledgers written before the structured identity
		// fields existed. Their fingerprint covered the complete group/entry.
		if artifact.Fingerprint == "" {
			return false
		}
		return gortexJSONArtifactFingerprint(group) == artifact.Fingerprint
	}
	return true
}

func gortexRemoveCodexHookArtifact(path, executable string, artifact gortexOwnedArtifact) (bool, error) {
	data, err := os.ReadFile(path)
	if errors.Is(err, os.ErrNotExist) {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	root := map[string]any{}
	if _, err := tomlDecode(string(data), &root); err != nil {
		return false, err
	}
	hooks, _ := root["hooks"].(map[string]any)
	if hooks == nil || artifact.Event == "" {
		return false, errors.New("Codex Hook 归属缺少事件信息，已保留")
	}
	list := gortexHookList(hooks[artifact.Event])
	if list == nil {
		return false, nil
	}
	candidates, exact := 0, 0
	groupIndex, handlerIndex := -1, -1
	for i, raw := range list {
		group, ok := raw.(map[string]any)
		if !ok {
			continue
		}
		handlers := gortexHookList(group["hooks"])
		if len(handlers) == 0 {
			command := gortexTomlEffectiveCommand(group)
			if !gortexCodexHookCommandMatches(command, executable) {
				continue
			}
			candidates++
			if gortexHookArtifactMatches(artifact, group, nil, command) {
				exact++
				groupIndex, handlerIndex = i, -1
			}
			continue
		}
		for j, rawHandler := range handlers {
			handler, ok := rawHandler.(map[string]any)
			if !ok {
				continue
			}
			command := gortexTomlEffectiveCommand(handler)
			if !gortexCodexHookCommandMatches(command, executable) {
				continue
			}
			candidates++
			if gortexHookArtifactMatches(artifact, group, handler, command) {
				exact++
				groupIndex, handlerIndex = i, j
			}
		}
	}
	if exact == 0 {
		if candidates > 0 {
			return false, fmt.Errorf("Codex %s Hook 已被修改或重复，已保留", artifact.Event)
		}
		return false, nil
	}
	if exact != 1 {
		return false, fmt.Errorf("Codex %s Hook 存在重复归属候选，已保留", artifact.Event)
	}
	keptGroups := make([]any, 0, len(list))
	for i, raw := range list {
		if i != groupIndex {
			keptGroups = append(keptGroups, raw)
			continue
		}
		group, _ := raw.(map[string]any)
		if handlerIndex < 0 {
			continue
		}
		handlers := gortexHookList(group["hooks"])
		keptHandlers := make([]any, 0, len(handlers)-1)
		for j, handler := range handlers {
			if j != handlerIndex {
				keptHandlers = append(keptHandlers, handler)
			}
		}
		if len(keptHandlers) == 0 {
			continue
		}
		clone := make(map[string]any, len(group))
		for key, value := range group {
			clone[key] = value
		}
		clone["hooks"] = keptHandlers
		keptGroups = append(keptGroups, clone)
	}
	if len(keptGroups) == 0 {
		delete(hooks, artifact.Event)
	} else {
		hooks[artifact.Event] = keptGroups
	}
	if len(hooks) == 0 {
		delete(root, "hooks")
	}
	encoded, err := tomlEncode(root)
	if err != nil {
		return false, err
	}
	return true, replaceUTF8File(path, []byte(encoded))
}

func gortexRemoveClaudeHookArtifact(path, executable string, artifact gortexOwnedArtifact) (bool, error) {
	data, err := os.ReadFile(path)
	if errors.Is(err, os.ErrNotExist) {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	root := map[string]any{}
	if err := json.Unmarshal(data, &root); err != nil {
		return false, err
	}
	hooks, _ := root["hooks"].(map[string]any)
	if hooks == nil || artifact.Event == "" {
		return false, errors.New("Claude Hook 归属缺少事件信息，已保留")
	}
	list := gortexHookList(hooks[artifact.Event])
	if list == nil {
		return false, nil
	}
	candidates, exact := 0, 0
	groupIndex, handlerIndex := -1, -1
	for i, raw := range list {
		group, ok := raw.(map[string]any)
		if !ok {
			continue
		}
		handlers := gortexHookList(group["hooks"])
		for j, rawHandler := range handlers {
			handler, ok := rawHandler.(map[string]any)
			if !ok {
				continue
			}
			command := gortexTomlEffectiveCommand(handler)
			if !gortexClaudeHookCommandMatches(command, executable) {
				continue
			}
			candidates++
			if gortexHookArtifactMatches(artifact, group, handler, command) {
				exact++
				groupIndex, handlerIndex = i, j
			}
		}
	}
	if exact == 0 {
		if candidates > 0 {
			return false, fmt.Errorf("Claude %s Hook 已被修改或重复，已保留", artifact.Event)
		}
		return false, nil
	}
	if exact != 1 {
		return false, fmt.Errorf("Claude %s Hook 存在重复归属候选，已保留", artifact.Event)
	}
	keptGroups := make([]any, 0, len(list))
	for i, raw := range list {
		if i != groupIndex {
			keptGroups = append(keptGroups, raw)
			continue
		}
		group, _ := raw.(map[string]any)
		handlers := gortexHookList(group["hooks"])
		keptHandlers := make([]any, 0, len(handlers)-1)
		for j, handler := range handlers {
			if j != handlerIndex {
				keptHandlers = append(keptHandlers, handler)
			}
		}
		if len(keptHandlers) == 0 {
			continue
		}
		clone := make(map[string]any, len(group))
		for key, value := range group {
			clone[key] = value
		}
		clone["hooks"] = keptHandlers
		keptGroups = append(keptGroups, clone)
	}
	if len(keptGroups) == 0 {
		delete(hooks, artifact.Event)
	} else {
		hooks[artifact.Event] = keptGroups
	}
	if len(hooks) == 0 {
		delete(root, "hooks")
	}
	encoded, _ := json.MarshalIndent(root, "", "  ")
	return true, replaceUTF8File(path, append(encoded, '\n'))
}

func gortexRemoveCopilotHookArtifact(path, executable string, artifact gortexOwnedArtifact) (bool, error) {
	data, err := os.ReadFile(path)
	if errors.Is(err, os.ErrNotExist) {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	root := map[string]any{}
	if err := json.Unmarshal(data, &root); err != nil {
		return false, err
	}
	hooks, _ := root["hooks"].(map[string]any)
	if hooks == nil || artifact.Event == "" {
		return false, errors.New("Copilot Hook 归属缺少事件信息，已保留")
	}
	list, ok := gortexCopilotHookList(hooks[artifact.Event])
	if !ok {
		return false, errors.New("Copilot Hook 事件不是数组，已保留用户配置")
	}
	candidates, exact, exactIndex := 0, 0, -1
	for i, item := range list {
		command := gortexDirectHookCommand(item)
		if !gortexCopilotHookEntryIsOurs(item, executable) {
			continue
		}
		candidates++
		if gortexHookArtifactMatches(artifact, item, nil, command) {
			exact++
			exactIndex = i
		}
	}
	if exact == 0 {
		if candidates > 0 {
			return false, fmt.Errorf("Copilot %s Hook 已被修改或重复，已保留", artifact.Event)
		}
		return false, nil
	}
	if exact != 1 {
		return false, fmt.Errorf("Copilot %s Hook 存在重复归属候选，已保留", artifact.Event)
	}
	kept := make([]any, 0, len(list)-1)
	for i, item := range list {
		if i != exactIndex {
			kept = append(kept, item)
		}
	}
	if len(kept) == 0 {
		delete(hooks, artifact.Event)
	} else {
		hooks[artifact.Event] = kept
	}
	if len(hooks) == 0 {
		delete(root, "hooks")
	}
	if len(root) == 2 && root["version"] != nil && root["disableAllHooks"] != nil && root["hooks"] == nil {
		return true, os.Remove(path)
	}
	encoded, _ := json.MarshalIndent(root, "", "  ")
	return true, replaceUTF8File(path, append(encoded, '\n'))
}

// gortexRemoveIntegrationsOwned removes only artifacts that were actually
// removed. Failed or user-modified entries stay in the ledger so a later retry
// still has the information needed to clean them safely.
func gortexRemoveIntegrationsOwned(executable string, ownership *gortexMCPOwnership) []string {
	warnings := []string{}
	if ownership == nil {
		return warnings
	}
	if ownership.Artifacts == nil {
		ownership.Artifacts = map[string]gortexOwnedArtifact{}
	}
	for key, artifact := range ownership.Artifacts {
		if artifact.Path == "" {
			continue
		}
		var err error
		if artifact.Kind == gortexArtifactPrompt {
			_, err = removeGortexPrompt(artifact.Path, artifact.Fingerprint)
		} else {
			switch artifact.Agent {
			case "codex":
				_, err = gortexRemoveCodexHookArtifact(artifact.Path, executable, artifact)
			case "claude":
				_, err = gortexRemoveClaudeHookArtifact(artifact.Path, executable, artifact)
			case "copilot":
				_, err = gortexRemoveCopilotHookArtifact(artifact.Path, executable, artifact)
			default:
				err = fmt.Errorf("未知 Gortex artifact 类型 %q，已保留", artifact.Agent)
			}
		}
		if err != nil {
			warnings = append(warnings, artifact.Agent+" "+artifact.Kind+": "+err.Error())
			continue
		}
		// A missing file or an already removed exact entry is also a successful
		// cleanup; delete only this artifact, never every artifact in the file.
		delete(ownership.Artifacts, key)
	}
	return warnings
}

// gortexRemoveKnownIntegrations is a recovery path for an absent or damaged
// ownership ledger. The marker/command identity checks in each remover keep
// unrelated platform settings intact; an unparsable file is reported and left
// untouched.
func gortexRemoveKnownIntegrations(executable string) []string {
	warnings := []string{}
	for _, agent := range []string{"codex", "claude", "copilot"} {
		for _, path := range gortexPromptPaths(agent) {
			if _, err := removeGortexPrompt(path, ""); err != nil {
				warnings = append(warnings, agent+" prompt: "+err.Error())
			}
		}
		path := gortexHookPath(agent)
		if path == "" {
			continue
		}
		var err error
		switch agent {
		case "codex":
			_, err = removeCodexHooks(path, executable)
		case "claude":
			_, err = removeClaudeHooks(path, executable)
		case "copilot":
			_, err = removeCopilotHooks(path, executable)
		}
		if err != nil {
			warnings = append(warnings, agent+" hook: "+err.Error())
		}
	}
	return warnings
}

func gortexRemoveCursorProjects(projects []string, ownership *gortexMCPOwnership) []string {
	warnings := []string{}
	if ownership == nil {
		return warnings
	}
	for _, project := range normalizeGortexProjects(projects) {
		if err := gortexRemoveCursorProject(project, ownership); err != nil {
			warnings = append(warnings, "cursor project rule: "+err.Error())
		}
	}
	return warnings
}

func gortexRegisterCursorProject(project string, ownership *gortexMCPOwnership) error {
	path := gortexCursorRulePath(project)
	changed, fingerprint, err := upsertGortexCursorPrompt(path, gortexInstructionBody)
	if err != nil {
		return err
	}
	if ownership.Artifacts == nil {
		ownership.Artifacts = map[string]gortexOwnedArtifact{}
	}
	if changed || fileExists(path) {
		ownership.Artifacts[gortexArtifactPrompt+"cursor:"+strings.ToLower(filepath.Clean(path))] = gortexOwnedArtifact{Kind: gortexArtifactPrompt, Agent: "cursor", Path: path, Fingerprint: fingerprint}
	}
	return nil
}

func gortexRemoveCursorProject(project string, ownership *gortexMCPOwnership) error {
	if ownership == nil {
		return errors.New("Gortex 归属账本为空")
	}
	path := gortexCursorRulePath(project)
	key := gortexArtifactPrompt + "cursor:" + strings.ToLower(filepath.Clean(path))
	artifact := ownership.Artifacts[key]
	_, err := removeGortexPrompt(path, artifact.Fingerprint)
	if err == nil {
		delete(ownership.Artifacts, key)
	}
	return err
}

type gortexArtifactStatus struct {
	Present  bool
	Complete bool
}

func gortexPromptBlockComplete(block string) bool {
	canonical := gortexCanonicalPromptFingerprint()
	normalized := normalizeGortexLineEndings(block)
	return gortexArtifactFingerprint([]byte(normalized)) == canonical ||
		gortexArtifactFingerprint([]byte(normalized+"\n")) == canonical
}

func gortexPromptStatus(agent string) (present, complete bool) {
	for _, path := range gortexPromptPaths(agent) {
		data, err := os.ReadFile(path)
		if err != nil {
			continue
		}
		block, ok := gortexMarkedBlock(data)
		if !ok {
			continue
		}
		present = true
		if gortexPromptBlockComplete(block) {
			complete = true
		}
	}
	return present, complete
}

func gortexPromptConfigured(agent string) bool {
	present, _ := gortexPromptStatus(agent)
	return present
}

func gortexCodexHookEvents(executable string) map[string]map[string]any {
	return map[string]map[string]any{
		"SessionStart":     {"matcher": "startup|resume|clear|compact", "hooks": []any{map[string]any{"type": "command", "command": gortexHookCommand("codex", executable), "timeout": 10, "statusMessage": "Loading Gortex graph orientation..."}}},
		"UserPromptSubmit": {"hooks": []any{map[string]any{"type": "command", "command": gortexHookCommand("codex", executable), "timeout": 10, "statusMessage": "Surfacing Gortex graph context..."}}},
		"PreToolUse":       {"matcher": ".*", "hooks": []any{map[string]any{"type": "command", "command": gortexHookCommand("codex", executable), "timeout": 5, "statusMessage": "Loading Gortex tool guidance..."}}},
		"PostToolUse":      {"matcher": "^(Bash|apply_patch|(mcp__gortex__|gortex__)(explore|search|read|relations|trace|analyze))$", "hooks": []any{map[string]any{"type": "command", "command": gortexHookCommand("codex", executable), "timeout": 5, "statusMessage": "Loading Gortex post-tool context..."}}},
		"Stop":             {"hooks": []any{map[string]any{"type": "command", "command": gortexHookCommand("codex", executable), "timeout": 10, "statusMessage": "Checking Gortex evidence authority..."}}},
	}
}

func gortexClaudeHookEvents(executable string) map[string]map[string]any {
	command := gortexHookCommand("claude", executable)
	return map[string]map[string]any{
		"SessionStart":     gortexClaudeHookEntry(command, "", "Loading Gortex graph orientation...", 3000),
		"UserPromptSubmit": gortexClaudeHookEntry(command, "", "Surfacing Gortex graph context...", 3000),
		"PreToolUse":       gortexClaudeHookEntry(command, "*", "Checking Gortex graph guidance...", 3000),
		"PostToolUse":      gortexClaudeHookEntry(command, "Read|Grep|Glob", "Loading Gortex post-tool context...", 3000),
		"Stop":             gortexClaudeHookEntry(command, "", "Checking Gortex evidence authority...", 5000),
		"PreCompact":       gortexClaudeHookEntry(command, "", "Injecting Gortex orientation snapshot...", 3000),
		"SubagentStart":    gortexClaudeHookEntry(command, "", "Starting an isolated Gortex subagent turn...", 3000),
		"SubagentStop":     gortexClaudeHookEntry(command, "", "Clearing Gortex subagent turn state...", 3000),
	}
}

func gortexCopilotHookEvents(executable string) map[string]map[string]any {
	bashCommand, powershellCommand := gortexCopilotHookCommands(executable)
	return map[string]map[string]any{
		"sessionStart":        gortexCopilotHookEntry(bashCommand, powershellCommand, ""),
		"userPromptSubmitted": gortexCopilotHookEntry(bashCommand, powershellCommand, ""),
		"preToolUse":          gortexCopilotHookEntry(bashCommand, powershellCommand, "^(?:bash|create|edit|glob|grep|powershell|rg|view)$"),
		"postToolUse":         gortexCopilotHookEntry(bashCommand, powershellCommand, "^(?:bash|create|edit|glob|grep|powershell|rg|view)$"),
	}
}

func gortexHookStatus(agent, executable string) (present, complete bool) {
	path := gortexHookPath(agent)
	if path == "" || !fileExists(path) {
		return false, false
	}
	data, err := os.ReadFile(path)
	if err != nil {
		return false, false
	}
	if agent == "codex" {
		var root map[string]any
		if _, err := tomlDecode(string(data), &root); err != nil {
			return false, false
		}
		hooks, _ := root["hooks"].(map[string]any)
		complete = true
		for event, want := range gortexCodexHookEvents(executable) {
			found, exact := false, false
			for _, item := range gortexHookList(hooks[event]) {
				if !gortexTomlHookCommand(item, executable) {
					continue
				}
				found = true
				present = true
				if gortexFingerprint(item) == gortexFingerprint(want) {
					exact = true
				}
			}
			if !found || !exact {
				complete = false
			}
		}
		return present, present && complete
	}
	var root map[string]any
	if json.Unmarshal(data, &root) != nil {
		return false, false
	}
	hooks, _ := root["hooks"].(map[string]any)
	complete = true
	if agent == "copilot" {
		for event, want := range gortexCopilotHookEvents(executable) {
			found, exact := false, false
			list, ok := gortexCopilotHookList(hooks[event])
			if !ok {
				complete = false
				continue
			}
			for _, item := range list {
				if !gortexCopilotHookEntryIsOurs(item, executable) {
					continue
				}
				found = true
				present = true
				if gortexFingerprint(item) == gortexFingerprint(want) {
					exact = true
				}
			}
			if !found || !exact {
				complete = false
			}
		}
		return present, present && complete
	}
	for event, want := range gortexClaudeHookEvents(executable) {
		found, exact := false, false
		for _, item := range gortexHookList(hooks[event]) {
			if !gortexJSONHookEntryIsOurs(item, executable) {
				continue
			}
			found = true
			present = true
			if gortexFingerprint(item) == gortexFingerprint(want) {
				exact = true
			}
		}
		if !found || !exact {
			complete = false
		}
	}
	return present, present && complete
}

func gortexHookConfigured(agent, executable string) bool {
	present, _ := gortexHookStatus(agent, executable)
	return present
}

func gortexCursorRuleStatus(project string) (present, complete bool) {
	data, err := os.ReadFile(gortexCursorRulePath(project))
	if err != nil {
		return false, false
	}
	block, ok := gortexMarkedBlock(data)
	if !ok {
		return false, false
	}
	return true, gortexPromptBlockComplete(block) && func() bool {
		_, present, valid := gortexCursorFrontmatterInfo(string(data))
		return present && valid
	}()
}

func gortexCursorRuleConfigured(project string) bool {
	present, _ := gortexCursorRuleStatus(project)
	return present
}

func gortexDaemonTrackedProjects(executable string) []string {
	if executable == "" {
		return nil
	}
	gortexTrackedStatusCache.Lock()
	if time.Since(gortexTrackedStatusCache.at) < 5*time.Second && sameGortexExecutablePath(gortexTrackedStatusCache.path, executable) {
		projects := append([]string(nil), gortexTrackedStatusCache.projects...)
		gortexTrackedStatusCache.Unlock()
		return projects
	}
	gortexTrackedStatusCache.Unlock()
	output, err := runGortexCommandWithTimeout(20*time.Second, executable, "status")
	if err != nil {
		return nil
	}
	projects := []string{}
	for _, line := range strings.Split(output, "\n") {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(strings.ToLower(line), "tracked repos:") {
			continue
		}
		if index := strings.Index(line, "  ("); index > 0 {
			candidate := strings.TrimSpace(line[:index])
			if drive := strings.Index(candidate, ":\\"); drive > 0 {
				candidate = candidate[drive-1:]
			} else if fields := strings.Fields(candidate); len(fields) > 1 {
				candidate = strings.TrimSpace(strings.Join(fields[1:], " "))
			}
			if filepath.IsAbs(candidate) {
				projects = append(projects, candidate)
			}
		}
	}
	projects = normalizeGortexProjects(projects)
	gortexTrackedStatusCache.Lock()
	gortexTrackedStatusCache.at = time.Now()
	gortexTrackedStatusCache.path = executable
	gortexTrackedStatusCache.projects = append([]string(nil), projects...)
	gortexTrackedStatusCache.Unlock()
	return projects
}

type gortexCodexTrustInfo struct {
	Status      string   `json:"trust_status"`
	Required    bool     `json:"trust_required"`
	Notice      string   `json:"trust_notice"`
	Command     string   `json:"trust_command"`
	Steps       []string `json:"trust_steps"`
	ShellOpened bool     `json:"trust_shell_open"`
}

func gortexCodexTrustStatus(executable string) gortexCodexTrustInfo {
	info := gortexCodexTrustInfo{Status: snipTrustNotApplicable, Steps: codexTrustSteps()}
	hookPath := gortexHookPath("codex")
	if hookPath == "" || !gortexHookConfigured("codex", executable) {
		return info
	}
	info.Status = snipTrustUntrusted
	info.Required = true
	info.Notice = "Codex skips new or changed Gortex hooks until they are trusted. Run /hooks in Codex, review the Gortex entries, and trust them."
	data, err := os.ReadFile(hookPath)
	if err != nil {
		info.Status = snipTrustUnknown
		info.Notice = "Unable to read the Codex Gortex hook configuration; review it in Codex /hooks."
		return info
	}
	trusted, modified, err := gortexCodexHooksTrusted(string(data), hookPath, executable)
	if err != nil {
		info.Status = snipTrustUnknown
		info.Notice = "Unable to read Codex Gortex hook trust records; review the hooks in Codex /hooks."
		return info
	}
	if trusted {
		info.Status = snipTrustTrusted
		info.Required = false
		info.Notice = "Codex Gortex hooks have persistent trust records."
	} else if modified {
		info.Notice = "Codex Gortex hook content changed and no longer matches trusted_hash; review it again in Codex /hooks."
	}
	return info
}

// gortexCodexHooksTrusted mirrors Codex's per-hook trusted_hash check for the
// inline config.toml representation used by the official Gortex adapter.
func gortexCodexHooksTrusted(configText, hookPath, executable string) (bool, bool, error) {
	var config map[string]any
	if _, err := toml.Decode(configText, &config); err != nil {
		return false, false, fmt.Errorf("parse Codex config.toml: %w", err)
	}
	hooks, _ := config["hooks"].(map[string]any)
	groups := gortexHookList(hooks["PreToolUse"])
	if len(groups) == 0 {
		return false, false, nil
	}
	trusted, modified, found := true, false, false
	for groupIndex, rawGroup := range groups {
		group, ok := rawGroup.(map[string]any)
		if !ok {
			continue
		}
		matcher, _ := group["matcher"].(string)
		for handlerIndex, rawHandler := range gortexHookList(group["hooks"]) {
			handler, ok := rawHandler.(map[string]any)
			if !ok || handler["type"] != "command" {
				continue
			}
			command := gortexTomlEffectiveCommand(handler)
			if !gortexCodexHookCommandMatches(command, executable) {
				continue
			}
			found = true
			identity := codexSnipHookIdentity{GroupIndex: groupIndex, HandlerIndex: handlerIndex, Command: command, Timeout: gortexHookTimeout(handler)}
			if matcher != "" {
				identity.Matcher = &matcher
			}
			if status, ok := handler["statusMessage"].(string); ok {
				identity.StatusMessage = &status
			}
			currentHash, err := codexSnipHookHash(identity)
			if err != nil {
				return false, false, err
			}
			storedHash, exists := codexTrustedHash(config, hookPath, groupIndex, handlerIndex)
			if !exists || strings.TrimSpace(storedHash) == "" {
				trusted = false
			} else if storedHash != currentHash {
				trusted, modified = false, true
			}
		}
	}
	return found && trusted, modified, nil
}

func gortexCommandMatchesExecutable(command, executable string) bool {
	command = strings.TrimSpace(command)
	if command == "" || !strings.Contains(strings.ToLower(command), " hook") || !strings.Contains(strings.ToLower(command), "gortex") {
		return false
	}
	if strings.TrimSpace(executable) == "" {
		return true
	}
	needle := strings.ToLower(filepath.Clean(strings.Trim(strings.TrimSpace(executable), `"'`)))
	normalizedCommand := strings.ReplaceAll(strings.ToLower(command), "/", `\`)
	return strings.Contains(normalizedCommand, strings.ReplaceAll(needle, "/", `\`))
}

func gortexHookTimeout(handler map[string]any) uint64 {
	switch value := handler["timeout"].(type) {
	case int:
		if value > 0 {
			return uint64(value)
		}
	case int64:
		if value > 0 {
			return uint64(value)
		}
	case uint64:
		return value
	case float64:
		if value > 0 {
			return uint64(value)
		}
	}
	return 0
}

func openGortexCodexTrustShell(info *gortexCodexTrustInfo) error {
	if info == nil {
		return errors.New("Gortex Codex 信任信息为空")
	}
	path, err := locateCodexExecutable()
	if err != nil {
		return err
	}
	command, projectDir, err := codexTrustCommand(path)
	if err != nil {
		return err
	}
	if err := launchVisiblePowerShell(command, projectDir); err != nil {
		return err
	}
	info.Command = command
	info.ShellOpened = true
	return nil
}
