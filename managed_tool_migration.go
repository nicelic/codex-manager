package main

import (
	"context"
	"errors"
	"fmt"
	"log"
	"os"
	"path/filepath"
)

func readManagedToolStateIfPresent(directory string) (managedToolState, bool, error) {
	if _, err := os.Stat(stateFilePath(directory)); errors.Is(err, os.ErrNotExist) {
		return managedToolState{}, false, nil
	} else if err != nil {
		return managedToolState{}, false, err
	}
	state, err := readManagedToolState(directory)
	return state, err == nil, err
}

// migrateSnipInstallationsBeforeInstall 只移除账本明确记录、且仍指向旧发布目录
// 的 Hook。不能使用 snip init --uninstall，因为上游命令会匹配同一文件中的所有
// Snip Hook，可能删除用户手工接入。
func migrateSnipInstallationsBeforeInstall(ctx context.Context, currentDirectory string) ([]string, error) {
	previous, err := discoverPreviousManagedToolInstallations("snip", currentDirectory)
	if err != nil {
		return nil, err
	}
	candidates := append([]string{currentDirectory}, previous...)
	migrated := make([]string, 0, len(candidates))
	for index, directory := range candidates {
		state, managed, stateErr := readManagedToolStateIfPresent(directory)
		if stateErr != nil {
			return nil, fmt.Errorf("读取 snip 状态失败（%s）: %w", directory, stateErr)
		}
		if index > 0 && !managed {
			continue
		}
		if managed {
			ledger, ownershipErr := recoverLegacySnipOwnership(state, filepath.Join(directory, "snip.exe"))
			if ownershipErr != nil {
				return nil, fmt.Errorf("读取 snip 精确归属失败（%s）: %w", directory, ownershipErr)
			}
			if err := removeSnipOwnedArtifacts(ledger); err != nil {
				return nil, fmt.Errorf("清理受管 snip Hook 失败（%s）: %w", directory, err)
			}
			if err := removeOwnedIndependentPath(ctx, directory, state.UserPath, state.SystemPath); err != nil {
				return nil, fmt.Errorf("清理 snip PATH 失败（%s）: %w", directory, err)
			}
		}
		if err := removeOwnedSnipDirectory(directory); err != nil {
			return nil, fmt.Errorf("删除 Snip 目录失败（%s）: %w", directory, err)
		}
		if managed {
			if err := forgetManagedToolInstallation("snip", directory); err != nil {
				log.Printf("移除旧 snip 安装记录失败（%s）: %v", directory, err)
			}
		}
		if index > 0 || managed {
			migrated = append(migrated, directory)
		}
	}
	return migrated, nil
}

// managedLLMTrimInstallations 只返回具有 code-Manager 状态文件的安装目录。
// llmtrim 的用户级 CA、环境变量和 .llmtrim 状态目录是共享资源，只有存在这类归属证据时才允许清理。
func managedLLMTrimInstallations(currentDirectory, configuredPath string) ([]string, error) {
	result := make(map[string]string)
	if isManagedToolInstallationDirectory("llmtrim", currentDirectory) {
		absolute, err := filepath.Abs(currentDirectory)
		if err != nil {
			return nil, err
		}
		result[filepath.Clean(absolute)] = absolute
	}
	previous, err := discoverPreviousManagedToolInstallations("llmtrim", currentDirectory)
	if err != nil {
		return nil, err
	}
	for _, directory := range previous {
		result[filepath.Clean(directory)] = directory
	}
	if configuredPath != "" {
		parent := filepath.Dir(configuredPath)
		if isManagedToolInstallationDirectory("llmtrim", parent) {
			absolute, absErr := filepath.Abs(parent)
			if absErr != nil {
				return nil, absErr
			}
			result[filepath.Clean(absolute)] = absolute
		}
	}
	directories := make([]string, 0, len(result))
	for _, directory := range result {
		directories = append(directories, directory)
	}
	return directories, nil
}
