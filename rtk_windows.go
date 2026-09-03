package main

import (
	"context"
	"errors"
	"fmt"
	"os/exec"
	"strings"
	"syscall"

	"golang.org/x/sys/windows/registry"
)

const (
	rtkUserEnvironmentKey   = `Environment`
	rtkSystemEnvironmentKey = `SYSTEM\CurrentControlSet\Control\Session Manager\Environment`
)

func queryRTKPathStatus(installDir string) (userPath, systemPath bool, message string) {
	entry := normalizeWindowsPathEntry(installDir)
	userValue, _, userErr := readWindowsPathValue(registry.CURRENT_USER, rtkUserEnvironmentKey)
	if userErr == nil {
		userPath = windowsPathContains(userValue, entry)
	} else if !errors.Is(userErr, syscall.ERROR_FILE_NOT_FOUND) {
		message = "无法读取用户 PATH: " + userErr.Error()
	}
	systemValue, _, systemErr := readWindowsPathValue(registry.LOCAL_MACHINE, rtkSystemEnvironmentKey)
	if systemErr == nil {
		systemPath = windowsPathContains(systemValue, entry)
	} else if !errors.Is(systemErr, syscall.ERROR_FILE_NOT_FOUND) {
		if message != "" {
			message += "；"
		}
		message += "无法读取系统 PATH: " + systemErr.Error()
	}
	return userPath, systemPath, message
}

func configureRTKPath(ctx context.Context, installDir string) (bool, bool, error) {
	entry := normalizeWindowsPathEntry(installDir)
	userPath, systemPath, pathMessage := queryRTKPathStatus(installDir)
	if pathMessage != "" {
		return userPath, systemPath, errors.New(pathMessage)
	}
	userAdded := false
	if !userPath {
		if err := updateWindowsPathValue(registry.CURRENT_USER, rtkUserEnvironmentKey, entry, true); err != nil {
			return false, systemPath, fmt.Errorf("写入用户 PATH 失败: %w", err)
		}
		userAdded = true
	}
	if !systemPath {
		if err := updateWindowsPathValue(registry.LOCAL_MACHINE, rtkSystemEnvironmentKey, entry, true); err != nil {
			if !isWindowsAccessDenied(err) {
				var rollbackErr error
				if userAdded {
					rollbackErr = updateWindowsPathValue(registry.CURRENT_USER, rtkUserEnvironmentKey, entry, false)
				}
				broadcastWindowsEnvironmentChange()
				if rollbackErr != nil {
					return false, systemPath, fmt.Errorf("写入系统 PATH 失败: %w；回滚用户 PATH 失败: %v", err, rollbackErr)
				}
				return userPath, systemPath, fmt.Errorf("写入系统 PATH 失败: %w", err)
			}
			if elevatedErr := runElevatedRTKPathUpdate(ctx, entry, true); elevatedErr != nil {
				var rollbackErr error
				if userAdded {
					rollbackErr = updateWindowsPathValue(registry.CURRENT_USER, rtkUserEnvironmentKey, entry, false)
				}
				broadcastWindowsEnvironmentChange()
				if rollbackErr != nil {
					return false, systemPath, fmt.Errorf("写入系统 PATH 需要管理员权限，UAC 操作未完成: %w；回滚用户 PATH 失败: %v", elevatedErr, rollbackErr)
				}
				return userPath, systemPath, fmt.Errorf("写入系统 PATH 需要管理员权限，UAC 操作未完成: %w", elevatedErr)
			}
		}
	}
	broadcastWindowsEnvironmentChange()
	userPath, systemPath, pathMessage = queryRTKPathStatus(installDir)
	if pathMessage != "" {
		return userPath, systemPath, errors.New(pathMessage)
	}
	if !userPath || !systemPath {
		return userPath, systemPath, errors.New("用户或系统 PATH 写入后仍未检测到 RTK-AI 目录")
	}
	return userPath, systemPath, nil
}

func removeRTKPath(ctx context.Context, installDir string) error {
	return removeOwnedRTKPath(ctx, installDir, true, true)
}

func removeOwnedRTKPath(ctx context.Context, installDir string, userOwned, systemOwned bool) error {
	entry := normalizeWindowsPathEntry(installDir)
	var failures []string
	if userOwned {
		if current, _, err := readWindowsPathValue(registry.CURRENT_USER, rtkUserEnvironmentKey); err == nil {
			if windowsPathContains(current, entry) {
				if err := updateWindowsPathValue(registry.CURRENT_USER, rtkUserEnvironmentKey, entry, false); err != nil {
					failures = append(failures, "用户 PATH: "+err.Error())
				}
			}
		} else if !errors.Is(err, syscall.ERROR_FILE_NOT_FOUND) {
			failures = append(failures, "用户 PATH: "+err.Error())
		}
	}
	if systemOwned {
		if current, _, err := readWindowsPathValue(registry.LOCAL_MACHINE, rtkSystemEnvironmentKey); err == nil {
			if windowsPathContains(current, entry) {
				if err := updateWindowsPathValue(registry.LOCAL_MACHINE, rtkSystemEnvironmentKey, entry, false); err != nil {
					if isWindowsAccessDenied(err) {
						if elevatedErr := runElevatedRTKPathUpdate(ctx, entry, false); elevatedErr != nil {
							failures = append(failures, "系统 PATH: "+elevatedErr.Error())
						}
					} else {
						failures = append(failures, "系统 PATH: "+err.Error())
					}
				}
			}
		} else if !errors.Is(err, syscall.ERROR_FILE_NOT_FOUND) {
			failures = append(failures, "系统 PATH: "+err.Error())
		}
	}
	broadcastWindowsEnvironmentChange()
	if len(failures) > 0 {
		return errors.New(strings.Join(failures, "；"))
	}
	return nil
}

func readWindowsPathValue(hive registry.Key, subkey string) (string, uint32, error) {
	key, err := registry.OpenKey(hive, subkey, registry.QUERY_VALUE)
	if err != nil {
		return "", 0, err
	}
	defer key.Close()
	return key.GetStringValue("Path")
}

func updateWindowsPathValue(hive registry.Key, subkey, entry string, add bool) error {
	key, err := registry.OpenKey(hive, subkey, registry.QUERY_VALUE|registry.SET_VALUE)
	if errors.Is(err, syscall.ERROR_FILE_NOT_FOUND) && hive == registry.CURRENT_USER && add {
		key, _, err = registry.CreateKey(hive, subkey, registry.QUERY_VALUE|registry.SET_VALUE)
	}
	if errors.Is(err, syscall.ERROR_FILE_NOT_FOUND) && !add {
		return nil
	}
	if err != nil {
		return err
	}
	defer key.Close()
	current, valueType, err := key.GetStringValue("Path")
	if errors.Is(err, syscall.ERROR_FILE_NOT_FOUND) {
		if !add {
			return nil
		}
		current = ""
		valueType = registry.SZ
	} else if err != nil {
		return err
	}
	updated := updateWindowsPathEntries(current, entry, add)
	if updated == current {
		return nil
	}
	if valueType == registry.EXPAND_SZ {
		return key.SetExpandStringValue("Path", updated)
	}
	return key.SetStringValue("Path", updated)
}

func updateWindowsPathEntries(current, entry string, add bool) string {
	parts := strings.Split(current, ";")
	updated := make([]string, 0, len(parts)+1)
	for _, part := range parts {
		if !add && strings.EqualFold(normalizeWindowsPathEntry(part), entry) {
			continue
		}
		updated = append(updated, part)
	}
	if add && !windowsPathContains(current, entry) {
		if len(updated) == 1 && updated[0] == "" {
			updated = updated[:0]
		}
		updated = append(updated, entry)
	}
	return strings.Join(updated, ";")
}

func windowsPathContains(pathValue, entry string) bool {
	for _, part := range strings.Split(pathValue, ";") {
		if strings.EqualFold(normalizeWindowsPathEntry(part), entry) {
			return true
		}
	}
	return false
}

func normalizeWindowsPathEntry(value string) string {
	trimmed := strings.TrimSpace(strings.Trim(value, `"`))
	trimmed = strings.TrimRight(trimmed, `\/`)
	return trimmed
}

func isWindowsAccessDenied(err error) bool {
	return errors.Is(err, syscall.ERROR_ACCESS_DENIED)
}

func runElevatedRTKPathUpdate(ctx context.Context, entry string, add bool) error {
	action := "remove"
	if add {
		action = "add"
	}
	quotedEntry := quotePowerShellString(entry)
	script := fmt.Sprintf(`$entry=%s;$current=[Environment]::GetEnvironmentVariable('Path','Machine');$parts=@($current -split ';' | ForEach-Object { $_.Trim() } | Where-Object { $_ });if('%s' -eq 'add'){if(-not ($parts | Where-Object { [String]::Equals($_,$entry,[StringComparison]::OrdinalIgnoreCase) })){$parts += $entry}}else{$parts=@($parts | Where-Object { -not [String]::Equals($_,$entry,[StringComparison]::OrdinalIgnoreCase) })};[Environment]::SetEnvironmentVariable('Path',($parts -join ';'),'Machine')`, quotedEntry, action)
	return runElevatedPowerShell(ctx, script)
}

func runElevatedPowerShell(ctx context.Context, script string) error {
	encoded := encodePowerShellCommand(script)
	outer := fmt.Sprintf(`$p=Start-Process -FilePath 'powershell.exe' -Verb RunAs -WindowStyle Hidden -ArgumentList @('-NoLogo','-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-EncodedCommand','%s') -Wait -PassThru;exit $p.ExitCode`, encoded)
	command := exec.CommandContext(ctx, "powershell.exe", "-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", outer)
	output, err := command.CombinedOutput()
	if err != nil {
		message := strings.TrimSpace(string(output))
		if message == "" {
			message = err.Error()
		}
		return errors.New(message)
	}
	return nil
}
