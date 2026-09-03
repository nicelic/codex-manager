package main

import (
	"bytes"
	_ "embed"
	"fmt"
	"os"
	"path/filepath"
	"unicode/utf8"
)

const (
	rtkCodexCommandsFileName           = "RTK-Codex-commands.md"
	rtkCodexAgentInstructionsFileName  = "RTK-Codex-agent-instructions.md"
	rtkClaudeAgentInstructionsFileName = "RTK-Claude-agent-instructions.md"
	rtkCodexAgentInstructionsMaxBytes  = 32 * 1024
)

//go:embed assets/RTK-Codex-commands.md
var rtkCodexCommands []byte

//go:embed assets/RTK-Codex-agent-instructions.md
var rtkCodexAgentInstructions []byte

//go:embed assets/RTK-Claude-agent-instructions.md
var rtkClaudeAgentInstructions []byte

func writeRTKCodexCommands(installDir string) error {
	if err := validateRTKCodexDocuments(); err != nil {
		return err
	}
	for _, document := range []struct {
		fileName string
		data     []byte
	}{
		{fileName: rtkCodexCommandsFileName, data: rtkCodexCommands},
		{fileName: rtkCodexAgentInstructionsFileName, data: rtkCodexAgentInstructions},
		{fileName: rtkClaudeAgentInstructionsFileName, data: rtkClaudeAgentInstructions},
	} {
		if err := writeEmbeddedRTKDocument(installDir, document.fileName, document.data); err != nil {
			return fmt.Errorf("写入 %s 失败: %w", document.fileName, err)
		}
	}
	return nil
}

func validateRTKCodexDocuments() error {
	if err := validateEmbeddedRTKDocument(rtkCodexCommandsFileName, rtkCodexCommands); err != nil {
		return err
	}
	if err := validateEmbeddedRTKDocument(rtkCodexAgentInstructionsFileName, rtkCodexAgentInstructions); err != nil {
		return err
	}
	if err := validateEmbeddedRTKDocument(rtkClaudeAgentInstructionsFileName, rtkClaudeAgentInstructions); err != nil {
		return err
	}
	if len(rtkCodexAgentInstructions) > rtkCodexAgentInstructionsMaxBytes {
		return fmt.Errorf("内置 %s 超过 Codex 默认 AGENTS 指令上限: %d > %d 字节", rtkCodexAgentInstructionsFileName, len(rtkCodexAgentInstructions), rtkCodexAgentInstructionsMaxBytes)
	}
	return nil
}

func validateEmbeddedRTKDocument(fileName string, data []byte) error {
	if len(data) == 0 {
		return fmt.Errorf("内置 %s 内容为空", fileName)
	}
	if !utf8.Valid(data) {
		return fmt.Errorf("内置 %s 不是有效 UTF-8", fileName)
	}
	if bytes.HasPrefix(data, []byte{0xef, 0xbb, 0xbf}) {
		return fmt.Errorf("内置 %s 不得包含 UTF-8 BOM", fileName)
	}
	if bytes.Contains(data, []byte("\r")) {
		return fmt.Errorf("内置 %s 不得包含 CRLF 或 CR 换行", fileName)
	}
	if bytes.Contains(data, []byte{0x00}) {
		return fmt.Errorf("内置 %s 不得包含 NUL 字节", fileName)
	}
	if bytes.Contains(data, []byte{0xef, 0xbf, 0xbd}) {
		return fmt.Errorf("内置 %s 不得包含 Unicode 替换字符", fileName)
	}
	return nil
}

func writeEmbeddedRTKDocument(installDir, fileName string, data []byte) error {
	temporary, err := os.CreateTemp(installDir, "."+fileName+"-*")
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
	targetPath := filepath.Join(installDir, fileName)
	if err := os.Rename(temporaryPath, targetPath); err != nil {
		backup, backupErr := os.CreateTemp(installDir, "."+fileName+"-backup-*")
		if backupErr != nil {
			return err
		}
		backupPath := backup.Name()
		if closeErr := backup.Close(); closeErr != nil {
			_ = os.Remove(backupPath)
			return err
		}
		if removeErr := os.Remove(backupPath); removeErr != nil {
			return err
		}
		if moveErr := os.Rename(targetPath, backupPath); moveErr != nil {
			return err
		}
		if retryErr := os.Rename(temporaryPath, targetPath); retryErr != nil {
			_ = os.Rename(backupPath, targetPath)
			return retryErr
		}
		if removeErr := os.Remove(backupPath); removeErr != nil {
			return removeErr
		}
	}
	keep = true
	return nil
}
