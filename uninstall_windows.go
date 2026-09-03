//go:build windows

package main

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"
)

const uninstallBatFileName = "uninstall.bat"

// runApplicationUninstall is only reachable from the runtime-generated
// uninstall.bat. The batch file owns deletion of the EXE and itself after this
// process has released its executable handle.
func runApplicationUninstall() error {
	releaseDir, err := validateReleaseUninstallDirectory()
	if err != nil {
		return err
	}

	instance, err := acquireUninstallInstance(60 * time.Second)
	if err != nil {
		return err
	}
	defer instance.Close()

	configPath := defaultConfigPath()
	config := Config{}
	if loaded, loadErr := loadConfig(configPath); loadErr == nil {
		config = loaded
	} else if !errors.Is(loadErr, os.ErrNotExist) {
		// A damaged config must not prevent removal of the managed binaries and
		// system settings. Avoid rewriting the damaged file; the final cleanup
		// removes the release-owned config directory instead.
		configPath = ""
	}
	cleaner := &gateway{config: config, configPath: configPath}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Minute)
	defer cancel()

	// Match the page workflow: every tool is stopped and confirmed first, then
	// its existing Delete implementation performs the owned-artifact cleanup.
	if err := stopAllManagedToolsForUninstall(ctx, cleaner); err != nil {
		return err
	}
	if err := cleaner.uninstallRTK(ctx); err != nil {
		return fmt.Errorf("删除 RTK 失败: %w", err)
	}
	if err := cleaner.uninstallSnip(ctx); err != nil {
		return fmt.Errorf("删除 snip 失败: %w", err)
	}
	if err := cleaner.uninstallLLMTrim(ctx); err != nil {
		return fmt.Errorf("删除 llmtrim 失败: %w", err)
	}

	if err := removeCodeManagerStartupArtifacts(); err != nil {
		return fmt.Errorf("清理 code-Manager 开机启动失败: %w", err)
	}
	if present, err := codeManagerStartupArtifactsPresent(); err != nil {
		return fmt.Errorf("校验 code-Manager 开机启动失败: %w", err)
	} else if present {
		return errors.New("code-Manager 开机启动项仍然存在")
	}
	if err := removeReleaseRuntimeArtifacts(releaseDir); err != nil {
		return err
	}
	return verifyReleaseRuntimeArtifactsRemoved(releaseDir)
}

func acquireUninstallInstance(timeout time.Duration) (*singleInstance, error) {
	deadline := time.Now().Add(timeout)
	for {
		instance, err := acquireSingleInstance(instanceMutexName)
		if !errors.Is(err, errAlreadyRunning) {
			return instance, err
		}
		if time.Now().After(deadline) {
			return nil, errors.New("code-Manager 仍在退出或运行中，请关闭后重试卸载")
		}
		time.Sleep(250 * time.Millisecond)
	}
}

func stopAllManagedToolsForUninstall(ctx context.Context, cleaner *gateway) error {
	var failures []string
	if err := cleaner.stopLLMTrim(ctx); err != nil {
		failures = append(failures, "停止 llmtrim: "+err.Error())
	}
	if err := cleaner.stopRTK(ctx); err != nil {
		failures = append(failures, "停止 RTK: "+err.Error())
	}
	if err := cleaner.stopSnip(ctx); err != nil {
		failures = append(failures, "停止 snip: "+err.Error())
	}
	if len(failures) > 0 {
		return errors.New(strings.Join(failures, "；"))
	}
	return nil
}

func validateReleaseUninstallDirectory() (string, error) {
	executable, err := os.Executable()
	if err != nil {
		return "", fmt.Errorf("无法定位 code-Manager.exe: %w", err)
	}
	executable, err = filepath.Abs(executable)
	if err != nil {
		return "", fmt.Errorf("无法规范化 code-Manager.exe 路径: %w", err)
	}
	if !strings.EqualFold(filepath.Base(executable), "code-Manager.exe") {
		return "", fmt.Errorf("拒绝从非 code-Manager.exe 文件执行卸载: %s", executable)
	}
	releaseDir := filepath.Dir(executable)
	if err := validateReleaseUninstallRoot(releaseDir); err != nil {
		return "", err
	}
	return releaseDir, nil
}

func validateReleaseUninstallRoot(releaseDir string) error {
	developmentEntry, err := developmentDirectoryEntry(releaseDir)
	if err != nil {
		return err
	}
	if developmentEntry != "" {
		return fmt.Errorf("检测到开发环境文件 %s，已拒绝卸载", developmentEntry)
	}
	return nil
}

func ensureRuntimeUninstallScript() error {
	executable, err := os.Executable()
	if err != nil {
		return fmt.Errorf("无法定位 code-Manager.exe: %w", err)
	}
	return ensureUninstallScriptForExecutable(executable)
}

func ensureUninstallScriptForExecutable(executable string) error {
	normalizedExecutable, err := filepath.Abs(executable)
	if err != nil {
		return fmt.Errorf("无法规范化 code-Manager.exe 路径: %w", err)
	}
	if !strings.EqualFold(filepath.Base(normalizedExecutable), "code-Manager.exe") {
		return nil
	}

	releaseDir := filepath.Dir(normalizedExecutable)
	developmentEntry, err := developmentDirectoryEntry(releaseDir)
	if err != nil {
		return err
	}
	if developmentEntry != "" {
		return nil
	}
	return ensureUninstallScriptInDirectory(releaseDir)
}

func ensureUninstallScriptInDirectory(releaseDir string) error {
	scriptPath := filepath.Join(releaseDir, uninstallBatFileName)
	existing, err := os.ReadFile(scriptPath)
	if err == nil && bytes.Equal(existing, uninstallBatTemplate) {
		return nil
	}
	if err != nil && !errors.Is(err, os.ErrNotExist) {
		return fmt.Errorf("读取卸载脚本失败: %w", err)
	}

	temporary, err := os.CreateTemp(releaseDir, ".code-manager-uninstall-*")
	if err != nil {
		return fmt.Errorf("创建卸载脚本临时文件失败: %w", err)
	}
	temporaryPath := temporary.Name()
	defer func() { _ = os.Remove(temporaryPath) }()
	if _, err := temporary.Write(uninstallBatTemplate); err != nil {
		_ = temporary.Close()
		return fmt.Errorf("写入卸载脚本失败: %w", err)
	}
	if err := temporary.Close(); err != nil {
		return fmt.Errorf("关闭卸载脚本临时文件失败: %w", err)
	}
	if err := os.Remove(scriptPath); err != nil && !errors.Is(err, os.ErrNotExist) {
		return fmt.Errorf("替换旧卸载脚本失败: %w", err)
	}
	if err := os.Rename(temporaryPath, scriptPath); err != nil {
		return fmt.Errorf("发布卸载脚本失败: %w", err)
	}
	return nil
}

func developmentDirectoryEntry(releaseDir string) (string, error) {
	for _, name := range []string{"go.mod", "main.go", "build.bat", "build.ps1", "frontend", "web"} {
		if _, err := os.Lstat(filepath.Join(releaseDir, name)); err == nil {
			return name, nil
		} else if !errors.Is(err, os.ErrNotExist) {
			return "", fmt.Errorf("检查开发环境标志 %s 失败: %w", name, err)
		}
	}
	return "", nil
}

// removeReleaseRuntimeArtifacts owns only the fixed runtime directories the
// application creates below its EXE. It never walks to the release parent.
func removeReleaseRuntimeArtifacts(releaseDir string) error {
	for _, name := range []string{"config"} {
		target := filepath.Join(releaseDir, name)
		if filepath.Clean(filepath.Dir(target)) != filepath.Clean(releaseDir) {
			return fmt.Errorf("拒绝删除发布目录外的目标: %s", target)
		}
		if err := os.RemoveAll(target); err != nil {
			return fmt.Errorf("删除 code-Manager 运行目录 %s 失败: %w", target, err)
		}
	}
	return nil
}

func verifyReleaseRuntimeArtifactsRemoved(releaseDir string) error {
	for _, name := range []string{"RTK-AI", "Snip", "llmtrim", "config"} {
		target := filepath.Join(releaseDir, name)
		if _, err := os.Lstat(target); err == nil {
			return fmt.Errorf("卸载后目录仍存在: %s", target)
		} else if !errors.Is(err, os.ErrNotExist) {
			return fmt.Errorf("校验卸载目录 %s 失败: %w", target, err)
		}
	}
	return nil
}
