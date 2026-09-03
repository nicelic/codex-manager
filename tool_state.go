package main

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"
)

// managedToolState 是 code-Manager 自己维护的期望状态与归属记录。
// 配置内容本身仍由各工具的原生文件保存，状态文件只记录本程序写入的部分。
type managedToolState struct {
	DesiredRunning bool              `json:"desired_running"`
	Running        bool              `json:"running"`
	UpdatedAt      time.Time         `json:"updated_at"`
	UserPath       bool              `json:"user_path,omitempty"`
	SystemPath     bool              `json:"system_path,omitempty"`
	OwnedAgents    []string          `json:"owned_agents,omitempty"`
	OwnedFiles     []string          `json:"owned_files,omitempty"`
	Metadata       map[string]string `json:"metadata,omitempty"`
}

func stateFilePath(directory string) string {
	return filepath.Join(directory, ".code-manager-state.json")
}

func readManagedToolState(directory string) (managedToolState, error) {
	var state managedToolState
	data, err := os.ReadFile(stateFilePath(directory))
	if errors.Is(err, os.ErrNotExist) {
		return state, nil
	}
	if err != nil {
		return state, err
	}
	if err := json.Unmarshal(data, &state); err != nil {
		return state, fmt.Errorf("读取状态文件失败: %w", err)
	}
	return state, nil
}

func writeManagedToolState(directory string, state managedToolState) error {
	if err := os.MkdirAll(directory, 0o755); err != nil {
		return err
	}
	state.UpdatedAt = time.Now().UTC()
	data, err := json.MarshalIndent(state, "", "  ")
	if err != nil {
		return err
	}
	data = append(data, '\n')
	temporary, err := os.CreateTemp(directory, ".code-manager-state-*")
	if err != nil {
		return err
	}
	temporaryPath := temporary.Name()
	keep := false
	defer func() {
		_ = temporary.Close()
		if !keep {
			_ = os.Remove(temporaryPath)
		}
	}()
	if _, err := temporary.Write(data); err != nil {
		return err
	}
	if err := temporary.Close(); err != nil {
		return err
	}
	if err := os.Rename(temporaryPath, stateFilePath(directory)); err != nil {
		return err
	}
	keep = true
	return nil
}

func removeManagedToolState(directory string) error {
	err := os.Remove(stateFilePath(directory))
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	return err
}

func desiredManagedToolState(directory string) (bool, bool, error) {
	state, err := readManagedToolState(directory)
	if err != nil {
		return false, false, err
	}
	_, statErr := os.Stat(stateFilePath(directory))
	return state.DesiredRunning, statErr == nil, nil
}

func managedToolStateHasAttention(state managedToolState) bool {
	return state.Metadata != nil && strings.TrimSpace(state.Metadata["last_error"]) != ""
}

func containsString(values []string, target string) bool {
	for _, value := range values {
		if value == target {
			return true
		}
	}
	return false
}

func markManagedToolAttention(directory string, state managedToolState, cause error) {
	if cause == nil {
		return
	}
	if state.Metadata == nil {
		state.Metadata = map[string]string{}
	}
	state.Metadata["last_error"] = cause.Error()
	_ = writeManagedToolState(directory, state)
}

var managedToolMu sync.Mutex

func withManagedToolLock(fn func() error) error {
	managedToolMu.Lock()
	defer managedToolMu.Unlock()
	return fn()
}
