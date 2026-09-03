//go:build windows

package main

import (
	"context"
	"errors"
	"fmt"
	"log"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"syscall"
	"time"

	"golang.org/x/sys/windows"
)

const cleanupHelperTimeout = 90 * time.Second

// launchExitCleanupHelper 通过短命 cmd.exe 的 start 命令脱离 code-Manager 的进程树。
// 这样 taskkill 只结束主程序时，助手仍可等待主程序死亡并完成清理。
func launchExitCleanupHelper() error {
	executable, err := os.Executable()
	if err != nil {
		return fmt.Errorf("定位退出清理助手可执行文件失败: %w", err)
	}
	command := exec.Command("cmd.exe", "/d", "/c", "start", "", "/b", executable, "--cleanup-helper", strconv.Itoa(os.Getpid()))
	command.SysProcAttr = &syscall.SysProcAttr{CreationFlags: windows.CREATE_NO_WINDOW}
	if err := command.Start(); err != nil {
		return err
	}
	return command.Process.Release()
}

func runCleanupHelper(args []string) {
	logger, logFile := cleanupHelperLogger()
	if logFile != nil {
		defer logFile.Close()
	}
	if len(args) != 1 {
		if logger != nil {
			logger.Printf("参数错误: 期望父进程 PID，实际参数=%q", args)
		}
		return
	}
	parentPID, err := strconv.ParseUint(strings.TrimSpace(args[0]), 10, 32)
	if err != nil || parentPID == 0 {
		if logger != nil {
			logger.Printf("父进程 PID 无效: %q", args[0])
		}
		return
	}
	waitForProcessExit(uint32(parentPID))

	// 若旧进程退出后已有新实例取得单实例锁，说明用户正在重启；让新实例继续负责状态恢复。
	instance, err := acquireSingleInstance(instanceMutexName)
	if errors.Is(err, errAlreadyRunning) {
		if logger != nil {
			logger.Print("检测到新的 code-Manager 实例，跳过旧实例退出清理")
		}
		return
	}
	if err != nil {
		if logger != nil {
			logger.Printf("取得退出清理锁失败: %v", err)
		}
		return
	}
	defer instance.Close()

	ctx, cancel := context.WithTimeout(context.Background(), cleanupHelperTimeout)
	defer cancel()
	if err := cleanupManagedToolsAfterExit(ctx); err != nil && logger != nil {
		logger.Printf("退出清理未完全成功: %v", err)
	}
}

func cleanupHelperLogger() (*log.Logger, *os.File) {
	directory, err := runtimeConfigDirectory()
	if err != nil {
		return nil, nil
	}
	if err := os.MkdirAll(directory, 0700); err != nil {
		return nil, nil
	}
	path := filepath.Join(directory, "code-Manager-cleanup.log")
	file, err := os.OpenFile(path, os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0o600)
	if err != nil {
		return nil, nil
	}
	return log.New(file, "", log.Ldate|log.Ltime|log.Lmicroseconds), file
}

func waitForProcessExit(pid uint32) {
	handle, err := windows.OpenProcess(windows.SYNCHRONIZE, false, pid)
	if err != nil {
		return
	}
	defer windows.CloseHandle(handle)
	_, _ = windows.WaitForSingleObject(handle, windows.INFINITE)
}

func cleanupManagedToolsAfterExit(ctx context.Context) error {
	var failures []string
	cleaner := &gateway{}

	if err := cleaner.stopLLMTrimAndCleanup(ctx, configuredLLMTrimPathForCleanup()); err != nil {
		failures = append(failures, "llmtrim: "+err.Error())
	}
	if err := cleaner.stopRTK(ctx); err != nil {
		failures = append(failures, "RTK: "+err.Error())
	}
	if err := cleaner.stopSnip(ctx); err != nil {
		failures = append(failures, "snip: "+err.Error())
	}
	if len(failures) > 0 {
		return errors.New(strings.Join(failures, "；"))
	}
	return nil
}

func configuredLLMTrimPathForCleanup() string {
	config, err := loadConfig(defaultConfigPath())
	if err == nil && strings.TrimSpace(config.LLMTrimPath) != "" {
		return strings.TrimSpace(config.LLMTrimPath)
	}
	return discoverLLMTrimProcessPath()
}
