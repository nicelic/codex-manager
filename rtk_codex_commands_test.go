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
	for _, document := range []struct {
		fileName string
		want     []byte
	}{
		{fileName: rtkCodexCommandsFileName, want: rtkCodexCommands},
		{fileName: rtkCodexAgentInstructionsFileName, want: rtkCodexAgentInstructions},
		{fileName: rtkClaudeAgentInstructionsFileName, want: rtkClaudeAgentInstructions},
	} {
		actual, err := os.ReadFile(filepath.Join(directory, document.fileName))
		if err != nil {
			t.Fatalf("read %s: %v", document.fileName, err)
		}
		if !bytes.Equal(actual, document.want) {
			t.Fatalf("%s content differs from embedded document", document.fileName)
		}
	}
}
