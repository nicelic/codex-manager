package main

import (
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
)

const (
	snipTrustNotApplicable = "not_applicable"
	snipTrustTrusted       = "trusted"
	snipTrustUntrusted     = "untrusted"
	snipTrustUnknown       = "unknown"
	snipTrustDisabled      = "disabled"
)

type snipCodexTrustInfo struct {
	Status      string
	HookPath    string
	CodexPath   string
	Command     string
	Notice      string
	Steps       []string
	ShellOpened bool
}

func codexTrustSteps() []string {
	return []string{
		"等待 PowerShell 中的 Codex CLI 界面显示 Hooks need review。不要输入 /hook 或 /hooks。",
		"在当前 PowerShell 窗口选择第 2 项 Trust all and continue；不是输入数字 2。",
		"按键盘 Enter（回车）确认；不需要先进入 Review hooks，也不需要输入 t。",
		"回到 code-Manager 点击“刷新状态”，确认 Codex Hook 显示“已信任”。",
	}
}

func detectSnipCodexTrust() (snipCodexTrustInfo, error) {
	directory := snipAgentDirectories()["codex"]
	info := snipCodexTrustInfo{Status: snipTrustNotApplicable, Steps: codexTrustSteps()}
	if directory == "" {
		return info, nil
	}
	info.HookPath = snipAgentHookFile("codex", directory)
	if info.HookPath == "" {
		return info, nil
	}
	hookData, err := os.ReadFile(info.HookPath)
	if errors.Is(err, os.ErrNotExist) {
		return info, nil
	}
	if err != nil {
		info.Status = snipTrustUnknown
		info.Notice = "无法读取 Codex Snip Hook 文件，请修复后再进行信任审核。"
		return info, err
	}
	identities, err := codexSnipHookIdentities(hookData)
	if err != nil {
		info.Status = snipTrustUnknown
		info.Notice = "无法解析 Codex Snip Hook 文件，请修复 hooks.json 后再进行信任审核。"
		return info, err
	}
	if len(identities) == 0 {
		return info, nil
	}
	info.Status = snipTrustUntrusted
	info.Notice = "Codex Hook 尚未检测到持久信任。请点击“添加信任”，在 PowerShell 审核界面选择第 2 项，然后按键盘 Enter（回车）。"
	if codexPath, locateErr := locateCodexExecutable(); locateErr == nil {
		info.CodexPath = codexPath
		if command, _, commandErr := codexTrustCommand(codexPath); commandErr == nil {
			info.Command = command
		}
	}
	configPath := filepath.Join(directory, "config.toml")
	data, err := os.ReadFile(configPath)
	if errors.Is(err, os.ErrNotExist) {
		return info, nil
	}
	if err != nil {
		info.Status = snipTrustUnknown
		info.Notice = "无法读取 Codex 信任记录，请点击“添加信任”，并在 PowerShell 的 Hooks need review 界面选择第 2 项后按键盘 Enter（回车）。"
		return info, err
	}
	if _, _, disabled := findCodexFeatureHookToggle(string(data), false); disabled {
		info.Status = snipTrustDisabled
		info.Notice = "Codex Hook 已明确关闭，请先在 Codex 配置中开启，再进行信任审核。"
		return info, nil
	}
	trusted, modified, trustErr := codexSnipHooksTrusted(string(data), info.HookPath, identities)
	if trustErr != nil {
		info.Status = snipTrustUnknown
		info.Notice = "无法读取 Codex 信任记录，请点击“添加信任”，并在 PowerShell 的 Hooks need review 界面选择第 2 项后按键盘 Enter（回车）。"
		return info, trustErr
	}
	if trusted {
		info.Status = snipTrustTrusted
		info.Notice = "Codex Hook 已检测到持久信任。"
	} else if modified {
		info.Notice = "Codex Snip Hook 内容已变化，原有 trusted_hash 不再匹配；请点击“添加信任”重新审核。"
	}
	return info, nil
}

func normalizeTrustPath(value string) string {
	value = strings.ReplaceAll(value, "\\\\", "\\")
	value = strings.ReplaceAll(value, "/", "\\")
	return strings.ToLower(strings.TrimSpace(value))
}

func locateCodexExecutable() (string, error) {
	for _, name := range []string{"codex.exe", "codex.cmd", "codex"} {
		if path, err := exec.LookPath(name); err == nil {
			if absolute, absErr := filepath.Abs(path); absErr == nil {
				return absolute, nil
			}
			return path, nil
		}
	}
	localAppData := strings.TrimSpace(os.Getenv("LOCALAPPDATA"))
	if localAppData == "" {
		home, err := os.UserHomeDir()
		if err != nil {
			return "", fmt.Errorf("无法定位 Codex 安装目录: %w", err)
		}
		localAppData = filepath.Join(home, "AppData", "Local")
	}
	root := filepath.Join(localAppData, "OpenAI", "Codex", "bin")
	entries, err := os.ReadDir(root)
	if err != nil {
		return "", fmt.Errorf("未找到 codex.exe，请先安装 Codex CLI: %w", err)
	}
	type candidate struct {
		path    string
		modTime int64
	}
	candidates := make([]candidate, 0, len(entries))
	for _, entry := range entries {
		if !entry.IsDir() {
			continue
		}
		path := filepath.Join(root, entry.Name(), "codex.exe")
		stat, statErr := os.Stat(path)
		if statErr == nil && !stat.IsDir() {
			candidates = append(candidates, candidate{path: path, modTime: stat.ModTime().UnixNano()})
		}
	}
	if len(candidates) == 0 {
		return "", fmt.Errorf("未找到 codex.exe，请检查 %s", root)
	}
	sort.Slice(candidates, func(i, j int) bool {
		if candidates[i].modTime == candidates[j].modTime {
			return candidates[i].path > candidates[j].path
		}
		return candidates[i].modTime > candidates[j].modTime
	})
	return filepath.Abs(candidates[0].path)
}

func codexTrustCommand(codexPath string) (string, string, error) {
	if strings.TrimSpace(codexPath) == "" {
		return "", "", errors.New("codex.exe 路径为空")
	}
	// The manager may be launched from a shortcut, startup task, or another
	// shell whose working directory is unrelated to the project. Anchor -C to
	// the manager executable directory first, then fall back to the process CWD.
	projectDir := ""
	var err error
	if executable, executableErr := os.Executable(); executableErr == nil {
		projectDir = filepath.Dir(executable)
	}
	if strings.TrimSpace(projectDir) == "" {
		projectDir, err = os.Getwd()
		if err != nil || strings.TrimSpace(projectDir) == "" {
			return "", "", fmt.Errorf("无法定位当前项目目录: %w", err)
		}
	}
	projectDir, err = filepath.Abs(projectDir)
	if err != nil {
		return "", "", err
	}
	command := fmt.Sprintf("& %s -C %s", quotePowerShellString(codexPath), quotePowerShellString(projectDir))
	return command, projectDir, nil
}

func openCodexTrustShell(info *snipCodexTrustInfo) error {
	if info == nil {
		return errors.New("Codex 信任信息为空")
	}
	codexPath, err := locateCodexExecutable()
	if err != nil {
		return err
	}
	command, projectDir, err := codexTrustCommand(codexPath)
	if err != nil {
		return err
	}
	if err := launchVisiblePowerShell(command, projectDir); err != nil {
		return err
	}
	info.CodexPath = codexPath
	info.Command = command
	info.ShellOpened = true
	return nil
}
