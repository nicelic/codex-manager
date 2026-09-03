package main

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os/exec"
	"regexp"
	"strconv"
	"strings"
	"unicode/utf8"

	"github.com/BurntSushi/toml"
)

const (
	snipMinimumCodexMajor    = 0
	snipMinimumCodexMinor    = 131
	snipMinimumCodexPatch    = 0
	codexDefaultHookTimeout  = 600
	codexDefaultContextLimit = 2500
)

var codexVersionPattern = regexp.MustCompile(`\b(\d+)\.(\d+)\.(\d+)`)

type codexCLIVersion struct {
	Major int
	Minor int
	Patch int
}

type codexSnipHookIdentity struct {
	GroupIndex             int
	HandlerIndex           int
	Matcher                *string
	Command                string
	Timeout                uint64
	Async                  bool
	StatusMessage          *string
	AdditionalContextLimit *uint64
}

// ensureSnipCodexRuntimeHookSupported 防止旧版 Codex 接受配置却不支持
// PreToolUse.updatedInput，从而在页面上造成“已接入但实际不生效”的假象。
func ensureSnipCodexRuntimeHookSupported(ctx context.Context, agents []snipAgentState) error {
	if !containsSnipAgent(agents, "codex") {
		return nil
	}
	codexPath, err := locateCodexExecutable()
	if err != nil {
		return fmt.Errorf("检测到 Codex 用户目录，但无法定位 Codex CLI 以确认 Hook 兼容性: %w", err)
	}
	output, err := exec.CommandContext(ctx, codexPath, "--version").CombinedOutput()
	if err != nil {
		detail := strings.TrimSpace(string(bytes.ToValidUTF8(output, []byte("?"))))
		if detail != "" {
			return fmt.Errorf("无法读取 Codex CLI 版本（%s）: %w: %s", codexPath, err, detail)
		}
		return fmt.Errorf("无法读取 Codex CLI 版本（%s）: %w", codexPath, err)
	}
	version, err := parseCodexCLIVersion(string(output))
	if err != nil {
		return fmt.Errorf("无法识别 Codex CLI 版本 %q: %w", strings.TrimSpace(string(output)), err)
	}
	minimum := codexCLIVersion{Major: snipMinimumCodexMajor, Minor: snipMinimumCodexMinor, Patch: snipMinimumCodexPatch}
	if !version.atLeast(minimum) {
		return fmt.Errorf("Codex CLI %d.%d.%d 低于 Snip 原生 Hook 所需的 %d.%d.%d，请先升级 Codex 后再启动 Snip", version.Major, version.Minor, version.Patch, minimum.Major, minimum.Minor, minimum.Patch)
	}
	return nil
}

func containsSnipAgent(agents []snipAgentState, name string) bool {
	for _, agent := range agents {
		if agent.Name == name {
			return true
		}
	}
	return false
}

func parseCodexCLIVersion(text string) (codexCLIVersion, error) {
	match := codexVersionPattern.FindStringSubmatch(text)
	if len(match) != 4 {
		return codexCLIVersion{}, fmt.Errorf("未找到 x.y.z 版本号")
	}
	values := [3]int{}
	for index := range values {
		value, err := strconv.Atoi(match[index+1])
		if err != nil {
			return codexCLIVersion{}, err
		}
		values[index] = value
	}
	return codexCLIVersion{Major: values[0], Minor: values[1], Patch: values[2]}, nil
}

func (version codexCLIVersion) atLeast(minimum codexCLIVersion) bool {
	if version.Major != minimum.Major {
		return version.Major > minimum.Major
	}
	if version.Minor != minimum.Minor {
		return version.Minor > minimum.Minor
	}
	return version.Patch >= minimum.Patch
}

// codexSnipHookIdentities 从 hooks.json 中提取 Codex 实际用于信任的 Snip Hook。
// group/handler 的下标必须保留，因为 Codex 将其写入 hooks.state 的键中。
func codexSnipHookIdentities(data []byte) ([]codexSnipHookIdentity, error) {
	if !utf8.Valid(data) {
		return nil, fmt.Errorf("hooks.json 不是有效 UTF-8")
	}
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.UseNumber()
	config := make(map[string]any)
	if err := decoder.Decode(&config); err != nil {
		return nil, fmt.Errorf("解析 hooks.json 失败: %w", err)
	}
	hooks, _ := config["hooks"].(map[string]any)
	rawGroups, exists := hooks["PreToolUse"]
	if !exists {
		return nil, nil
	}
	groups, ok := rawGroups.([]any)
	if !ok {
		return nil, fmt.Errorf("hooks.PreToolUse 不是数组")
	}
	identities := make([]codexSnipHookIdentity, 0, len(groups))
	for groupIndex, rawGroup := range groups {
		group, ok := rawGroup.(map[string]any)
		if !ok {
			return nil, fmt.Errorf("hooks.PreToolUse[%d] 不是对象", groupIndex)
		}
		matcher, err := optionalJSONText(group, "matcher")
		if err != nil {
			return nil, fmt.Errorf("hooks.PreToolUse[%d].matcher 无效: %w", groupIndex, err)
		}
		rawHandlers, exists := group["hooks"]
		if !exists {
			continue
		}
		handlers, ok := rawHandlers.([]any)
		if !ok {
			return nil, fmt.Errorf("hooks.PreToolUse[%d].hooks 不是数组", groupIndex)
		}
		for handlerIndex, rawHandler := range handlers {
			handler, ok := rawHandler.(map[string]any)
			if !ok {
				return nil, fmt.Errorf("hooks.PreToolUse[%d].hooks[%d] 不是对象", groupIndex, handlerIndex)
			}
			if handler["type"] != "command" {
				continue
			}
			command, ok := handler["command"].(string)
			if !ok {
				return nil, fmt.Errorf("hooks.PreToolUse[%d].hooks[%d].command 无效", groupIndex, handlerIndex)
			}
			commandWindows, err := optionalJSONText(handler, "commandWindows")
			if err != nil {
				return nil, fmt.Errorf("hooks.PreToolUse[%d].hooks[%d].commandWindows 无效: %w", groupIndex, handlerIndex, err)
			}
			if commandWindows != nil {
				command = *commandWindows
			}
			if strings.TrimSpace(command) == "" || !snipHookCommandTargetsAgent(command, "codex") {
				continue
			}
			timeout, err := codexHookTimeout(handler)
			if err != nil {
				return nil, fmt.Errorf("hooks.PreToolUse[%d].hooks[%d].timeout 无效: %w", groupIndex, handlerIndex, err)
			}
			async, err := optionalJSONBool(handler, "async")
			if err != nil {
				return nil, fmt.Errorf("hooks.PreToolUse[%d].hooks[%d].async 无效: %w", groupIndex, handlerIndex, err)
			}
			statusMessage, err := optionalJSONText(handler, "statusMessage")
			if err != nil {
				return nil, fmt.Errorf("hooks.PreToolUse[%d].hooks[%d].statusMessage 无效: %w", groupIndex, handlerIndex, err)
			}
			additionalContextLimit, err := codexHookAdditionalContextLimit(handler)
			if err != nil {
				return nil, fmt.Errorf("hooks.PreToolUse[%d].hooks[%d].additionalContextLimit 无效: %w", groupIndex, handlerIndex, err)
			}
			identities = append(identities, codexSnipHookIdentity{
				GroupIndex:             groupIndex,
				HandlerIndex:           handlerIndex,
				Matcher:                matcher,
				Command:                command,
				Timeout:                timeout,
				Async:                  async,
				StatusMessage:          statusMessage,
				AdditionalContextLimit: additionalContextLimit,
			})
		}
	}
	return identities, nil
}

func optionalJSONText(values map[string]any, key string) (*string, error) {
	raw, exists := values[key]
	if !exists || raw == nil {
		return nil, nil
	}
	value, ok := raw.(string)
	if !ok {
		return nil, fmt.Errorf("必须是字符串")
	}
	return &value, nil
}

func optionalJSONBool(values map[string]any, key string) (bool, error) {
	raw, exists := values[key]
	if !exists || raw == nil {
		return false, nil
	}
	value, ok := raw.(bool)
	if !ok {
		return false, fmt.Errorf("必须是布尔值")
	}
	return value, nil
}

func codexHookTimeout(handler map[string]any) (uint64, error) {
	raw, exists := handler["timeout"]
	if !exists || raw == nil {
		return codexDefaultHookTimeout, nil
	}
	value, ok := raw.(json.Number)
	if !ok {
		return 0, fmt.Errorf("必须是无符号整数")
	}
	timeout, err := strconv.ParseUint(value.String(), 10, 64)
	if err != nil {
		return 0, err
	}
	if timeout == 0 {
		return 1, nil
	}
	return timeout, nil
}

func codexHookAdditionalContextLimit(handler map[string]any) (*uint64, error) {
	raw, exists := handler["additionalContextLimit"]
	if !exists || raw == nil {
		return nil, nil
	}
	value, ok := raw.(json.Number)
	if !ok {
		return nil, fmt.Errorf("必须是无符号整数")
	}
	limit, err := strconv.ParseUint(value.String(), 10, 64)
	if err != nil {
		return nil, err
	}
	if limit == codexDefaultContextLimit {
		return nil, nil
	}
	return &limit, nil
}

// codexSnipHooksTrusted 复刻 Codex 对 hooks.state.trusted_hash 的判断：只有记录的
// 哈希与当前规范化 Hook 身份完全相同，才显示为已信任。
func codexSnipHooksTrusted(configText, hookPath string, identities []codexSnipHookIdentity) (trusted bool, modified bool, err error) {
	if len(identities) == 0 {
		return false, false, nil
	}
	var config map[string]any
	if _, err := toml.Decode(configText, &config); err != nil {
		return false, false, fmt.Errorf("解析 config.toml 失败: %w", err)
	}
	trusted = true
	for _, identity := range identities {
		currentHash, err := codexSnipHookHash(identity)
		if err != nil {
			return false, false, err
		}
		storedHash, exists := codexTrustedHash(config, hookPath, identity.GroupIndex, identity.HandlerIndex)
		if !exists || strings.TrimSpace(storedHash) == "" {
			trusted = false
			continue
		}
		if storedHash != currentHash {
			trusted = false
			modified = true
		}
	}
	return trusted, modified, nil
}

func codexTrustedHash(config map[string]any, hookPath string, groupIndex, handlerIndex int) (string, bool) {
	hooks, _ := config["hooks"].(map[string]any)
	states, _ := hooks["state"].(map[string]any)
	suffix := fmt.Sprintf(":pre_tool_use:%d:%d", groupIndex, handlerIndex)
	for key, rawState := range states {
		if !strings.HasSuffix(key, suffix) {
			continue
		}
		source := strings.TrimSuffix(key, suffix)
		if normalizeTrustPath(source) != normalizeTrustPath(hookPath) {
			continue
		}
		state, _ := rawState.(map[string]any)
		hash, ok := state["trusted_hash"].(string)
		return hash, ok
	}
	return "", false
}

func codexSnipHookHash(identity codexSnipHookIdentity) (string, error) {
	handler := map[string]any{
		"async":   identity.Async,
		"command": identity.Command,
		"timeout": identity.Timeout,
		"type":    "command",
	}
	if identity.StatusMessage != nil {
		handler["statusMessage"] = *identity.StatusMessage
	}
	if identity.AdditionalContextLimit != nil {
		handler["additionalContextLimit"] = *identity.AdditionalContextLimit
	}
	value := map[string]any{
		"event_name": "pre_tool_use",
		"hooks":      []any{handler},
	}
	if identity.Matcher != nil {
		value["matcher"] = *identity.Matcher
	}
	var encoded bytes.Buffer
	encoder := json.NewEncoder(&encoded)
	encoder.SetEscapeHTML(false)
	if err := encoder.Encode(value); err != nil {
		return "", err
	}
	serialized := bytes.TrimSuffix(encoded.Bytes(), []byte{'\n'})
	sum := sha256.Sum256(serialized)
	return "sha256:" + hex.EncodeToString(sum[:]), nil
}
