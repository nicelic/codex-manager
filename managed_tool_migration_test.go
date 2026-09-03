package main

import (
	"path/filepath"
	"testing"
)

func TestManagedToolInstallationRequiresStateFile(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "Snip")
	if isManagedToolInstallationDirectory("snip", directory) {
		t.Fatal("目录没有状态文件时不应视为 code-Manager 受管安装")
	}
	if err := writeManagedToolState(directory, managedToolState{}); err != nil {
		t.Fatalf("writeManagedToolState() error = %v", err)
	}
	if !isManagedToolInstallationDirectory("snip", directory) {
		t.Fatal("带有效状态文件的 Snip 目录应视为 code-Manager 受管安装")
	}
}

func TestLLMTrimSharedStateIsNotResidualWithoutManagedInstallation(t *testing.T) {
	t.Setenv("APPDATA", t.TempDir())
	t.Setenv("LOCALAPPDATA", t.TempDir())
	installDir := filepath.Join(t.TempDir(), "llmtrim")
	residual, err := queryLLMTrimResidual("", installDir, false, false, false, true)
	if err != nil {
		t.Fatalf("queryLLMTrimResidual() error = %v", err)
	}
	if residual {
		t.Fatal("没有受管 llmtrim 安装时，不应仅因共享 .llmtrim 状态目录而报告残留")
	}
}
