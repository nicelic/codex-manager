package main

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"golang.org/x/sys/windows/registry"
)

const codeManagerStartupValueName = "code-Manager"

func syncCodeManagerStartup(enabled bool) error {
	executable, err := currentCodeManagerExecutable()
	if err != nil {
		return err
	}
	key, err := registry.OpenKey(registry.CURRENT_USER, llmtrimRunKey, registry.QUERY_VALUE|registry.SET_VALUE)
	if err != nil {
		key, _, err = registry.CreateKey(registry.CURRENT_USER, llmtrimRunKey, registry.QUERY_VALUE|registry.SET_VALUE)
		if err != nil {
			return fmt.Errorf("open Windows startup registry: %w", err)
		}
	}
	defer key.Close()
	if !enabled {
		return removeCodeManagerStartupArtifacts()
	}
	command := `"` + executable + `" --startup`
	if existing, _, getErr := key.GetStringValue(codeManagerStartupValueName); getErr == nil && !strings.EqualFold(strings.TrimSpace(existing), command) {
		return fmt.Errorf("Windows 开机启动项 %q 已指向另一份程序，拒绝覆盖: %s", codeManagerStartupValueName, existing)
	} else if getErr != nil && !errors.Is(getErr, registry.ErrNotExist) {
		return fmt.Errorf("read Windows startup entry: %w", getErr)
	}
	if err := key.SetStringValue(codeManagerStartupValueName, command); err != nil {
		return fmt.Errorf("write Windows startup entry: %w", err)
	}
	return nil
}

func removeCodeManagerStartupArtifacts() error {
	executable, err := currentCodeManagerExecutable()
	if err != nil {
		return err
	}
	expectedCommand := `"` + executable + `" --startup`
	runKey, err := registry.OpenKey(registry.CURRENT_USER, llmtrimRunKey, registry.QUERY_VALUE|registry.SET_VALUE)
	if err != nil && !errors.Is(err, registry.ErrNotExist) {
		return fmt.Errorf("open Windows startup registry: %w", err)
	}
	if err == nil {
		defer runKey.Close()
		command, _, valueErr := runKey.GetStringValue(codeManagerStartupValueName)
		switch {
		case valueErr == nil:
			if !strings.EqualFold(strings.TrimSpace(command), expectedCommand) {
				return fmt.Errorf("Windows 开机启动项 %q 已指向另一份程序，拒绝删除: %s", codeManagerStartupValueName, command)
			}
			if err := runKey.DeleteValue(codeManagerStartupValueName); err != nil && !errors.Is(err, registry.ErrNotExist) {
				return fmt.Errorf("remove Windows startup entry: %w", err)
			}
		case errors.Is(valueErr, registry.ErrNotExist):
			if approved, approvalErr := codeManagerStartupApprovedPresent(); approvalErr != nil {
				return approvalErr
			} else if approved {
				return errors.New("检测到没有可验证 Run 路径的 code-Manager 启动审批记录，拒绝删除")
			}
		default:
			return fmt.Errorf("read Windows startup entry: %w", valueErr)
		}
	} else if approved, approvalErr := codeManagerStartupApprovedPresent(); approvalErr != nil {
		return approvalErr
	} else if approved {
		return errors.New("检测到没有可验证 Run 路径的 code-Manager 启动审批记录，拒绝删除")
	}
	if err := deleteRegistryValue(registry.CURRENT_USER, llmtrimStartupApprovedKey, codeManagerStartupValueName); err != nil {
		return fmt.Errorf("remove Windows startup approval: %w", err)
	}
	return nil
}

func codeManagerStartupArtifactsPresent() (bool, error) {
	for _, subkey := range []string{llmtrimRunKey, llmtrimStartupApprovedKey} {
		exists, err := registryValueExists(registry.CURRENT_USER, subkey, codeManagerStartupValueName)
		if err != nil {
			return false, err
		}
		if exists {
			return true, nil
		}
	}
	return false, nil
}

func currentCodeManagerExecutable() (string, error) {
	executable, err := os.Executable()
	if err != nil {
		return "", fmt.Errorf("locate code-Manager.exe: %w", err)
	}
	executable, err = filepath.Abs(executable)
	if err != nil {
		return "", fmt.Errorf("normalize code-Manager.exe path: %w", err)
	}
	return executable, nil
}

func codeManagerStartupApprovedPresent() (bool, error) {
	exists, err := registryValueExists(registry.CURRENT_USER, llmtrimStartupApprovedKey, codeManagerStartupValueName)
	if err != nil {
		return false, fmt.Errorf("read Windows startup approval: %w", err)
	}
	return exists, nil
}
