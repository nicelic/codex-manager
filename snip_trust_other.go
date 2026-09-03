//go:build !windows

package main

import "errors"

func launchVisiblePowerShell(command, workdir string) error {
	return errors.New("当前平台不支持打开 PowerShell 信任窗口")
}
