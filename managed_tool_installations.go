package main

import (
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

const managedToolInstallRegistryName = "managed-tools.json"

// managedToolInstallRegistry 只记录 code-Manager 自己成功安装过的 Snip/llmtrim 目录。
// 它用于这两个工具的发布目录变更，不记录用户 PATH、提示词或 Hook 的原始内容。
type managedToolInstallRegistry struct {
	Tools map[string][]string `json:"tools"`
}

func managedToolInstallRegistryPath() (string, error) {
	directory, err := os.UserConfigDir()
	if err != nil {
		return "", fmt.Errorf("无法定位当前用户配置目录: %w", err)
	}
	return filepath.Join(directory, "code-Manager", managedToolInstallRegistryName), nil
}

func readManagedToolInstallRegistry() (managedToolInstallRegistry, error) {
	registry := managedToolInstallRegistry{Tools: map[string][]string{}}
	pathValue, err := managedToolInstallRegistryPath()
	if err != nil {
		return registry, err
	}
	data, err := os.ReadFile(pathValue)
	if errors.Is(err, os.ErrNotExist) {
		return registry, nil
	}
	if err != nil {
		return registry, err
	}
	if err := json.Unmarshal(data, &registry); err != nil {
		return registry, fmt.Errorf("读取受管工具安装记录失败: %w", err)
	}
	if registry.Tools == nil {
		registry.Tools = map[string][]string{}
	}
	return registry, nil
}

func writeManagedToolInstallRegistry(registry managedToolInstallRegistry) error {
	pathValue, err := managedToolInstallRegistryPath()
	if err != nil {
		return err
	}
	if registry.Tools == nil {
		registry.Tools = map[string][]string{}
	}
	data, err := json.MarshalIndent(registry, "", "  ")
	if err != nil {
		return err
	}
	data = append(data, '\n')
	if err := os.MkdirAll(filepath.Dir(pathValue), 0o700); err != nil {
		return err
	}
	return replaceUTF8File(pathValue, data)
}

func recordManagedToolInstallation(tool, directory string) error {
	absolute, err := filepath.Abs(directory)
	if err != nil {
		return err
	}
	registry, err := readManagedToolInstallRegistry()
	if err != nil {
		return err
	}
	for _, existing := range registry.Tools[tool] {
		if strings.EqualFold(filepath.Clean(existing), filepath.Clean(absolute)) {
			return nil
		}
	}
	registry.Tools[tool] = append(registry.Tools[tool], absolute)
	sort.Strings(registry.Tools[tool])
	return writeManagedToolInstallRegistry(registry)
}

func forgetManagedToolInstallation(tool, directory string) error {
	absolute, err := filepath.Abs(directory)
	if err != nil {
		return err
	}
	registry, err := readManagedToolInstallRegistry()
	if err != nil {
		return err
	}
	entries := registry.Tools[tool]
	filtered := entries[:0]
	for _, existing := range entries {
		if !strings.EqualFold(filepath.Clean(existing), filepath.Clean(absolute)) {
			filtered = append(filtered, existing)
		}
	}
	if len(filtered) == 0 {
		delete(registry.Tools, tool)
	} else {
		registry.Tools[tool] = filtered
	}
	return writeManagedToolInstallRegistry(registry)
}

func expectedManagedToolDirectoryName(tool string) string {
	switch tool {
	case "snip":
		return "snip"
	case "llmtrim":
		return "llmtrim"
	default:
		return ""
	}
}

func isManagedToolInstallationDirectory(tool, directory string) bool {
	expected := expectedManagedToolDirectoryName(tool)
	if expected == "" || !strings.EqualFold(filepath.Base(filepath.Clean(directory)), expected) {
		return false
	}
	if _, err := os.Stat(stateFilePath(directory)); err != nil {
		return false
	}
	_, err := readManagedToolState(directory)
	return err == nil
}

// discoverPreviousManagedToolInstallations 返回已验证为 code-Manager 所有的旧目录。
// 新版本优先读取用户级索引；为了兼容旧版本，再只扫描当前发布目录同级的一层目录。
func discoverPreviousManagedToolInstallations(tool, currentDirectory string) ([]string, error) {
	currentAbsolute, err := filepath.Abs(currentDirectory)
	if err != nil {
		return nil, err
	}
	result := make(map[string]string)
	registry, registryErr := readManagedToolInstallRegistry()
	if registryErr == nil {
		for _, directory := range registry.Tools[tool] {
			absolute, absErr := filepath.Abs(directory)
			if absErr == nil && !strings.EqualFold(filepath.Clean(absolute), filepath.Clean(currentAbsolute)) && isManagedToolInstallationDirectory(tool, absolute) {
				result[strings.ToLower(filepath.Clean(absolute))] = absolute
			}
		}
	} else {
		// 索引不可读不妨碍同级旧版本迁移，但不接受未验证路径。
		log.Printf("读取受管工具安装记录失败，改用同级旧目录发现: %v", registryErr)
	}

	parent := filepath.Dir(filepath.Dir(currentAbsolute))
	entries, readErr := os.ReadDir(parent)
	if readErr == nil {
		for _, entry := range entries {
			if !entry.IsDir() {
				continue
			}
			candidate := filepath.Join(parent, entry.Name(), filepath.Base(currentAbsolute))
			absolute, absErr := filepath.Abs(candidate)
			if absErr != nil || strings.EqualFold(filepath.Clean(absolute), filepath.Clean(currentAbsolute)) {
				continue
			}
			if isManagedToolInstallationDirectory(tool, absolute) {
				result[strings.ToLower(filepath.Clean(absolute))] = absolute
			}
		}
	} else if !errors.Is(readErr, os.ErrNotExist) {
		log.Printf("读取同级旧发布目录失败，跳过兼容扫描: %v", readErr)
	}

	directories := make([]string, 0, len(result))
	for _, directory := range result {
		directories = append(directories, directory)
	}
	sort.Strings(directories)
	return directories, nil
}
