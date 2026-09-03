package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"unicode/utf8"
)

func snipAgentHookPresentAt(name, path string) (bool, error) {
	locations, exists, err := snipHookLocationsAt(name, path, func(command string) bool {
		return snipHookCommandTargetsAgent(command, name)
	})
	return exists && len(locations) > 0, err
}

func readSnipHookJSON(path string) (map[string]any, bool, error) {
	data, err := os.ReadFile(path)
	if errors.Is(err, os.ErrNotExist) {
		return nil, false, nil
	}
	if err != nil {
		return nil, false, err
	}
	config, err := parseSnipHookJSON(data, filepath.Base(path))
	return config, true, err
}

func parseSnipHookJSON(data []byte, name string) (map[string]any, error) {
	if !utf8.Valid(data) {
		return nil, fmt.Errorf("%s 不是有效 UTF-8", name)
	}
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.UseNumber()
	config := make(map[string]any)
	if err := decoder.Decode(&config); err != nil {
		return nil, fmt.Errorf("解析 %s 失败: %w", name, err)
	}
	var trailing any
	if err := decoder.Decode(&trailing); !errors.Is(err, io.EOF) {
		if err == nil {
			return nil, fmt.Errorf("%s 包含多个 JSON 值", name)
		}
		return nil, fmt.Errorf("解析 %s 尾部失败: %w", name, err)
	}
	return config, nil
}

func writeSnipHookJSON(path string, config map[string]any) error {
	data, err := json.MarshalIndent(config, "", "  ")
	if err != nil {
		return err
	}
	data = append(data, '\n')
	return replaceUTF8File(path, data)
}

type snipHookLocation struct {
	Agent            string
	TargetPath       string
	Event            string
	GroupIndex       int
	HandlerIndex     int
	GroupFingerprint string
	Command          string
	Fingerprint      string
}

type snipOwnedHookPresence uint8

const (
	snipOwnedHookAbsent snipOwnedHookPresence = iota
	snipOwnedHookExact
	snipOwnedHookMoved
)

func snipHookEventForAgent(name string) (event string, grouped bool) {
	switch name {
	case "codex", "claude-code":
		return "PreToolUse", true
	case "cursor":
		return "beforeShellExecution", true
	case "copilot":
		return "preToolUse", false
	default:
		return "", false
	}
}

func snipHookLocationsMatchingExecutableAt(name, path, executable string) ([]snipHookLocation, bool, error) {
	return snipHookLocationsAt(name, path, func(command string) bool {
		return snipHookCommandMatchesExecutable(command, name, executable)
	})
}

func snipHookLocationsMatchingExecutableInData(name, path string, data []byte, executable string) ([]snipHookLocation, error) {
	config, err := parseSnipHookJSON(data, filepath.Base(path))
	if err != nil {
		return nil, err
	}
	return snipHookLocationsFromConfig(name, path, config, func(command string) bool {
		return snipHookCommandMatchesExecutable(command, name, executable)
	})
}

// snipAgentHasAnySnipHookAt intentionally recognises every native Snip command
// in the Agent's hook event, including commands whose arguments are not one of
// the forms code-Manager writes. This keeps official init away from a user's
// existing Snip configuration instead of assuming it is safe to replace.
func snipAgentHasAnySnipHookAt(name, path string) (bool, error) {
	locations, exists, err := snipHookLocationsAt(name, path, func(command string) bool {
		executable, _, ok := splitSnipHookCommand(command)
		return ok && isSnipExecutableName(executable)
	})
	return exists && len(locations) > 0, err
}

func snipHookLocationsAt(name, path string, matches func(string) bool) ([]snipHookLocation, bool, error) {
	config, exists, err := readSnipHookJSON(path)
	if err != nil || !exists {
		return nil, exists, err
	}
	locations, err := snipHookLocationsFromConfig(name, path, config, matches)
	return locations, true, err
}

func snipHookLocationsFromConfig(name, path string, config map[string]any, matches func(string) bool) ([]snipHookLocation, error) {
	event, grouped := snipHookEventForAgent(name)
	if event == "" {
		return nil, fmt.Errorf("未知 Snip Agent: %s", name)
	}
	rawHooks, exists := config["hooks"]
	if !exists || rawHooks == nil {
		return nil, nil
	}
	hooks, ok := rawHooks.(map[string]any)
	if !ok {
		return nil, errors.New("hooks 必须是对象")
	}
	rawEvent, exists := hooks[event]
	if !exists || rawEvent == nil {
		return nil, nil
	}
	if grouped {
		groups, ok := rawEvent.([]any)
		if !ok {
			return nil, fmt.Errorf("hooks.%s 必须是数组", event)
		}
		locations := make([]snipHookLocation, 0, len(groups))
		for groupIndex, rawGroup := range groups {
			group, ok := rawGroup.(map[string]any)
			if !ok {
				return nil, fmt.Errorf("hooks.%s[%d] 必须是对象", event, groupIndex)
			}
			groupFingerprint, err := snipHookGroupFingerprint(group)
			if err != nil {
				return nil, fmt.Errorf("计算 hooks.%s[%d] 的组指纹失败: %w", event, groupIndex, err)
			}
			rawHandlers, exists := group["hooks"]
			if !exists || rawHandlers == nil {
				continue
			}
			handlers, ok := rawHandlers.([]any)
			if !ok {
				return nil, fmt.Errorf("hooks.%s[%d].hooks 必须是数组", event, groupIndex)
			}
			for handlerIndex, rawHandler := range handlers {
				command, commandHandler, err := snipRawHookCommand(rawHandler, "command")
				if err != nil {
					return nil, fmt.Errorf("hooks.%s[%d].hooks[%d] 无效: %w", event, groupIndex, handlerIndex, err)
				}
				if !commandHandler || !matches(command) {
					continue
				}
				fingerprint, err := snipHookHandlerFingerprint(rawHandler)
				if err != nil {
					return nil, err
				}
				locations = append(locations, snipHookLocation{Agent: name, TargetPath: path, Event: event, GroupIndex: groupIndex, HandlerIndex: handlerIndex, GroupFingerprint: groupFingerprint, Command: command, Fingerprint: fingerprint})
			}
		}
		return locations, nil
	}
	handlers, ok := rawEvent.([]any)
	if !ok {
		return nil, fmt.Errorf("hooks.%s 必须是数组", event)
	}
	locations := make([]snipHookLocation, 0, len(handlers))
	for handlerIndex, rawHandler := range handlers {
		command, commandHandler, err := snipRawHookCommand(rawHandler, "bash")
		if err != nil {
			return nil, fmt.Errorf("hooks.%s[%d] 无效: %w", event, handlerIndex, err)
		}
		if !commandHandler || !matches(command) {
			continue
		}
		fingerprint, err := snipHookHandlerFingerprint(rawHandler)
		if err != nil {
			return nil, err
		}
		locations = append(locations, snipHookLocation{Agent: name, TargetPath: path, Event: event, GroupIndex: -1, HandlerIndex: handlerIndex, Command: command, Fingerprint: fingerprint})
	}
	return locations, nil
}

func snipRawHookCommand(raw any, field string) (string, bool, error) {
	handler, ok := raw.(map[string]any)
	if !ok {
		return "", false, errors.New("处理器必须是对象")
	}
	if handler["type"] != "command" {
		return "", false, nil
	}
	if field == "command" {
		if rawWindows, exists := handler["commandWindows"]; exists && rawWindows != nil {
			commandWindows, ok := rawWindows.(string)
			if !ok {
				return "", false, errors.New("commandWindows 必须是字符串")
			}
			if strings.TrimSpace(commandWindows) != "" {
				return commandWindows, true, nil
			}
		}
	}
	value, exists := handler[field]
	if !exists || value == nil {
		return "", false, fmt.Errorf("%s 必须是字符串", field)
	}
	command, ok := value.(string)
	if !ok {
		return "", false, fmt.Errorf("%s 必须是字符串", field)
	}
	return command, true, nil
}

func snipHookHandlerFingerprint(raw any) (string, error) {
	data, err := json.Marshal(raw)
	if err != nil {
		return "", err
	}
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:]), nil
}

func snipHookGroupFingerprint(group map[string]any) (string, error) {
	context := make(map[string]any, len(group))
	for key, value := range group {
		if key != "hooks" {
			context[key] = value
		}
	}
	data, err := json.Marshal(context)
	if err != nil {
		return "", err
	}
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:]), nil
}

func snipOwnedHookPresenceAt(artifact snipHookOwnership) (snipOwnedHookPresence, error) {
	locations, _, err := snipHookLocationsAt(artifact.Agent, artifact.TargetPath, func(string) bool { return true })
	if err != nil {
		return snipOwnedHookAbsent, err
	}
	foundElsewhere := false
	for _, location := range locations {
		if location.Event != artifact.Event || location.Fingerprint != artifact.Fingerprint || strings.TrimSpace(location.Command) != strings.TrimSpace(artifact.Command) {
			continue
		}
		if location.GroupIndex == artifact.GroupIndex && location.HandlerIndex == artifact.HandlerIndex && (artifact.GroupIndex < 0 || location.GroupFingerprint == artifact.GroupFingerprint) {
			return snipOwnedHookExact, nil
		}
		foundElsewhere = true
	}
	if foundElsewhere {
		return snipOwnedHookMoved, nil
	}
	return snipOwnedHookAbsent, nil
}

func removeSnipOwnedHook(artifact snipHookOwnership) (bool, error) {
	presence, err := snipOwnedHookPresenceAt(artifact)
	if err != nil || presence == snipOwnedHookAbsent {
		return false, err
	}
	if presence == snipOwnedHookMoved {
		return false, errors.New("Hook 已被人工移动或重新排序，无法安全确认原处理器；请手工处理后再清理")
	}
	config, exists, err := readSnipHookJSON(artifact.TargetPath)
	if err != nil || !exists {
		return false, err
	}
	hooks, err := snipHooksObject(config)
	if err != nil || hooks == nil {
		return false, err
	}
	if artifact.GroupIndex < 0 {
		return removeSnipOwnedUngroupedHook(artifact.TargetPath, config, hooks, artifact)
	}
	return removeSnipOwnedGroupedHook(artifact.TargetPath, config, hooks, artifact)
}

func snipHooksObject(config map[string]any) (map[string]any, error) {
	rawHooks, exists := config["hooks"]
	if !exists || rawHooks == nil {
		return nil, nil
	}
	hooks, ok := rawHooks.(map[string]any)
	if !ok {
		return nil, errors.New("hooks 必须是对象")
	}
	return hooks, nil
}

func removeSnipOwnedGroupedHook(path string, config, hooks map[string]any, artifact snipHookOwnership) (bool, error) {
	rawGroups, exists := hooks[artifact.Event]
	if !exists {
		return false, nil
	}
	groups, ok := rawGroups.([]any)
	if !ok {
		return false, fmt.Errorf("hooks.%s 必须是数组", artifact.Event)
	}
	if artifact.GroupIndex >= len(groups) {
		return false, nil
	}
	group, ok := groups[artifact.GroupIndex].(map[string]any)
	if !ok {
		return false, fmt.Errorf("hooks.%s[%d] 必须是对象", artifact.Event, artifact.GroupIndex)
	}
	rawHandlers, exists := group["hooks"]
	if !exists {
		return false, nil
	}
	handlers, ok := rawHandlers.([]any)
	if !ok {
		return false, fmt.Errorf("hooks.%s[%d].hooks 必须是数组", artifact.Event, artifact.GroupIndex)
	}
	if artifact.HandlerIndex >= len(handlers) {
		return false, nil
	}
	if !snipHandlerMatchesOwnership(handlers[artifact.HandlerIndex], artifact) || !snipGroupMatchesOwnership(group, artifact) {
		return false, nil
	}
	updatedHandlers := append(append([]any(nil), handlers[:artifact.HandlerIndex]...), handlers[artifact.HandlerIndex+1:]...)
	if len(updatedHandlers) == 0 {
		updatedGroups := append(append([]any(nil), groups[:artifact.GroupIndex]...), groups[artifact.GroupIndex+1:]...)
		if len(updatedGroups) == 0 {
			delete(hooks, artifact.Event)
		} else {
			hooks[artifact.Event] = updatedGroups
		}
	} else {
		group["hooks"] = updatedHandlers
		groups[artifact.GroupIndex] = group
		hooks[artifact.Event] = groups
	}
	if len(hooks) == 0 {
		delete(config, "hooks")
	}
	return true, writeSnipHookJSON(path, config)
}

func removeSnipOwnedUngroupedHook(path string, config, hooks map[string]any, artifact snipHookOwnership) (bool, error) {
	rawHandlers, exists := hooks[artifact.Event]
	if !exists {
		return false, nil
	}
	handlers, ok := rawHandlers.([]any)
	if !ok {
		return false, fmt.Errorf("hooks.%s 必须是数组", artifact.Event)
	}
	if artifact.HandlerIndex >= len(handlers) || !snipHandlerMatchesOwnership(handlers[artifact.HandlerIndex], artifact) {
		return false, nil
	}
	updatedHandlers := append(append([]any(nil), handlers[:artifact.HandlerIndex]...), handlers[artifact.HandlerIndex+1:]...)
	if len(updatedHandlers) == 0 {
		delete(hooks, artifact.Event)
	} else {
		hooks[artifact.Event] = updatedHandlers
	}
	if len(hooks) == 0 {
		delete(config, "hooks")
	}
	return true, writeSnipHookJSON(path, config)
}

func snipHandlerMatchesOwnership(raw any, artifact snipHookOwnership) bool {
	field := "command"
	if artifact.GroupIndex < 0 {
		field = "bash"
	}
	command, commandHandler, err := snipRawHookCommand(raw, field)
	if err != nil || !commandHandler || strings.TrimSpace(command) != strings.TrimSpace(artifact.Command) {
		return false
	}
	fingerprint, err := snipHookHandlerFingerprint(raw)
	return err == nil && fingerprint == artifact.Fingerprint
}

func snipGroupMatchesOwnership(group map[string]any, artifact snipHookOwnership) bool {
	if artifact.GroupIndex < 0 {
		return true
	}
	fingerprint, err := snipHookGroupFingerprint(group)
	return err == nil && fingerprint == artifact.GroupFingerprint
}

func snipHookCommandTargetsAgent(command, agent string) bool {
	executable, args, ok := splitSnipHookCommand(command)
	if !ok || !isSnipExecutableName(executable) {
		return false
	}
	return snipHookArgumentsMatch(args, agent)
}

func snipHookCommandMatchesExecutable(command, agent, executable string) bool {
	commandExecutable, args, ok := splitSnipHookCommand(command)
	if !ok || !snipHookArgumentsMatch(args, agent) {
		return false
	}
	return strings.EqualFold(normalizeSnipExecutablePath(commandExecutable), normalizeSnipExecutablePath(executable))
}

func splitSnipHookCommand(command string) (string, []string, bool) {
	command = strings.TrimSpace(command)
	if strings.HasPrefix(command, "&") {
		command = strings.TrimSpace(strings.TrimPrefix(command, "&"))
	}
	if command == "" {
		return "", nil, false
	}
	var executable string
	var remaining string
	if strings.HasPrefix(command, "\\\"") {
		// Snip 的 Windows Codex init 会写入 \"C:/Program Files/...\"。这是 Hook
		// 命令字符串中的转义引号，不是路径的一部分。
		closing := strings.Index(command[2:], "\\\"")
		if closing < 0 {
			return "", nil, false
		}
		closing += 2
		executable = command[2:closing]
		remaining = strings.TrimSpace(command[closing+2:])
	} else if quote := command[0]; quote == '\'' || quote == '"' {
		closing := strings.IndexByte(command[1:], quote)
		if closing < 0 {
			return "", nil, false
		}
		closing++
		executable = command[1:closing]
		remaining = strings.TrimSpace(command[closing+1:])
	} else {
		parts := strings.Fields(command)
		if len(parts) == 0 {
			return "", nil, false
		}
		executable = parts[0]
		remaining = strings.TrimSpace(strings.TrimPrefix(command, executable))
	}
	args := strings.Fields(remaining)
	return executable, args, true
}

func snipHookArgumentsMatch(args []string, agent string) bool {
	want := []string{"hook"}
	if agent == "codex" || agent == "copilot" {
		want = append(want, agent)
	}
	if len(args) != len(want) {
		return false
	}
	for index := range want {
		if args[index] != want[index] {
			return false
		}
	}
	return true
}

func isSnipExecutableName(value string) bool {
	base := strings.ToLower(filepath.Base(strings.ReplaceAll(strings.TrimSpace(value), "/", string(filepath.Separator))))
	return base == "snip" || base == "snip.exe"
}

func normalizeSnipExecutablePath(value string) string {
	value = strings.Trim(strings.TrimSpace(value), "\"'")
	value = strings.ReplaceAll(value, "/", string(filepath.Separator))
	if absolute, err := filepath.Abs(value); err == nil {
		value = absolute
	}
	return strings.TrimRight(filepath.Clean(value), `\\`)
}
