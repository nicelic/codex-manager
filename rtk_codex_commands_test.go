package main

import (
	"bytes"
	"os"
	"path/filepath"
	"testing"
)

func TestWriteRTKCodexCommandsWritesReferenceAndAssistantInstructions(t *testing.T) {
	directory := t.TempDir()
	if err := writeRTKCodexCommands(directory); err != nil {
		t.Fatalf("writeRTKCodexCommands() error = %v", err)
	}
	documents := []struct {
		fileName string
		want     []byte
	}{
		{fileName: rtkCodexCommandsFileName, want: rtkCodexCommands},
		{fileName: rtkCodexAgentInstructionsFileName, want: rtkCodexAgentInstructions},
		{fileName: rtkClaudeAgentInstructionsFileName, want: rtkClaudeAgentInstructions},
	}
	for _, document := range documents {
		actual, err := os.ReadFile(filepath.Join(directory, document.fileName))
		if err != nil {
			t.Fatalf("read %s: %v", document.fileName, err)
		}
		if !bytes.Equal(actual, document.want) {
			t.Fatalf("%s content differs from embedded document", document.fileName)
		}
	}

	// 模拟已存在旧版本/过期文件，验证先删除旧文件并重新释放最新版本
	for _, document := range documents {
		targetPath := filepath.Join(directory, document.fileName)
		if err := os.WriteFile(targetPath, []byte("旧版过时指令内容"), 0o644); err != nil {
			t.Fatalf("write stale file %s: %v", document.fileName, err)
		}
	}
	if err := writeRTKCodexCommands(directory); err != nil {
		t.Fatalf("writeRTKCodexCommands() with existing stale files error = %v", err)
	}
	for _, document := range documents {
		actual, err := os.ReadFile(filepath.Join(directory, document.fileName))
		if err != nil {
			t.Fatalf("read updated %s: %v", document.fileName, err)
		}
		if !bytes.Equal(actual, document.want) {
			t.Fatalf("%s content differs from embedded document after replacing stale file", document.fileName)
		}
	}
}
