package main

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"syscall"

	"golang.org/x/sys/windows/registry"
)

func queryIndependentPathStatus(directory string) (bool, bool, string) {
	entry := normalizeWindowsPathEntry(directory)
	var message string
	user, _, userErr := readWindowsPathValue(registry.CURRENT_USER, `Environment`)
	if userErr == nil {
		userPresent := windowsPathContains(user, entry)
		system, _, systemErr := readWindowsPathValue(registry.LOCAL_MACHINE, `SYSTEM\CurrentControlSet\Control\Session Manager\Environment`)
		if systemErr == nil {
			return userPresent, windowsPathContains(system, entry), ""
		}
		if !errors.Is(systemErr, syscall.ERROR_FILE_NOT_FOUND) {
			message = "无法读取系统 PATH: " + systemErr.Error()
		}
		return userPresent, false, message
	}
	if !errors.Is(userErr, syscall.ERROR_FILE_NOT_FOUND) {
		message = "无法读取用户 PATH: " + userErr.Error()
	}
	system, _, systemErr := readWindowsPathValue(registry.LOCAL_MACHINE, `SYSTEM\CurrentControlSet\Control\Session Manager\Environment`)
	if systemErr == nil {
		return false, windowsPathContains(system, entry), message
	}
	if !errors.Is(systemErr, syscall.ERROR_FILE_NOT_FOUND) {
		if message != "" {
			message += "；"
		}
		message += "无法读取系统 PATH: " + systemErr.Error()
	}
	return false, false, message
}

func configureIndependentPath(ctx context.Context, directory string) (bool, bool, error) {
	entry := normalizeWindowsPathEntry(directory)
	user, system, message := queryIndependentPathStatus(directory)
	if message != "" {
		return user, system, errors.New(message)
	}
	userAdded := false
	if !user {
		if err := updateWindowsPathValue(registry.CURRENT_USER, `Environment`, entry, true); err != nil {
			return false, system, fmt.Errorf("写入 snip 用户 PATH 失败: %w", err)
		}
		userAdded = true
	}
	if !system {
		if err := updateWindowsPathValue(registry.LOCAL_MACHINE, `SYSTEM\CurrentControlSet\Control\Session Manager\Environment`, entry, true); err != nil {
			if isWindowsAccessDenied(err) {
				if elevatedErr := runElevatedIndependentPath(ctx, entry, true); elevatedErr != nil {
					if userAdded {
						_ = updateWindowsPathValue(registry.CURRENT_USER, `Environment`, entry, false)
					}
					return false, false, elevatedErr
				}
			} else {
				if userAdded {
					_ = updateWindowsPathValue(registry.CURRENT_USER, `Environment`, entry, false)
				}
				return false, false, err
			}
		}
	}
	broadcastWindowsEnvironmentChange()
	user, system, message = queryIndependentPathStatus(directory)
	if message != "" {
		return user, system, errors.New(message)
	}
	if !user || !system {
		return user, system, errors.New("用户或系统 PATH 写入后仍未检测到 Snip 目录")
	}
	return user, system, nil
}

func removeIndependentPath(ctx context.Context, directory string) error {
	return removeOwnedIndependentPath(ctx, directory, true, true)
}

func removeOwnedIndependentPath(ctx context.Context, directory string, userOwned, systemOwned bool) error {
	entry := normalizeWindowsPathEntry(directory)
	var failures []string
	if userOwned {
		if value, _, err := readWindowsPathValue(registry.CURRENT_USER, `Environment`); err == nil {
			if windowsPathContains(value, entry) {
				if err := updateWindowsPathValue(registry.CURRENT_USER, `Environment`, entry, false); err != nil {
					failures = append(failures, "用户 PATH: "+err.Error())
				}
			}
		} else if !errors.Is(err, syscall.ERROR_FILE_NOT_FOUND) {
			failures = append(failures, "用户 PATH: "+err.Error())
		}
	}
	if systemOwned {
		if value, _, err := readWindowsPathValue(registry.LOCAL_MACHINE, `SYSTEM\CurrentControlSet\Control\Session Manager\Environment`); err == nil {
			if windowsPathContains(value, entry) {
				if err := updateWindowsPathValue(registry.LOCAL_MACHINE, `SYSTEM\CurrentControlSet\Control\Session Manager\Environment`, entry, false); err != nil {
					if isWindowsAccessDenied(err) {
						if elevatedErr := runElevatedIndependentPath(ctx, entry, false); elevatedErr != nil {
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
	userPresent, systemPresent, message := queryIndependentPathStatus(directory)
	if message != "" {
		return errors.New(message)
	}
	if (userOwned && userPresent) || (systemOwned && systemPresent) {
		return errors.New("受管 Snip PATH 清理后仍存在")
	}
	return nil
}

func runElevatedIndependentPath(ctx context.Context, entry string, add bool) error {
	action := "remove"
	if add {
		action = "add"
	}
	script := fmt.Sprintf(`$entry=%s;$current=[Environment]::GetEnvironmentVariable('Path','Machine');$parts=@($current -split ';' | %% { $_.Trim() } | ? { $_ });if('%s' -eq 'add'){if(-not ($parts | ? { [String]::Equals($_,$entry,[StringComparison]::OrdinalIgnoreCase) })){$parts += $entry}}else{$parts=@($parts | ? { -not [String]::Equals($_,$entry,[StringComparison]::OrdinalIgnoreCase) })};[Environment]::SetEnvironmentVariable('Path',($parts -join ';'),'Machine')`, quotePowerShellString(entry), action)
	return runElevatedPowerShell(ctx, script)
}
