package main

import (
	"encoding/base64"
	"strings"
	"testing"
	"unicode/utf16"
)

func TestLogViewerCommandLineUsesNativeConsoleOutput(t *testing.T) {
	logPath := `C:\logs\with space\O'Brien\code-Manager.log`
	commandLine := logViewerCommandLine(`C:\Windows\System32\cmd.exe`, logPath)

	if strings.Contains(commandLine, "CONOUT$") {
		t.Fatalf("log viewer command must not redirect to CONOUT$: %q", commandLine)
	}
	const marker = "-EncodedCommand "
	start := strings.Index(commandLine, marker)
	if start == -1 {
		t.Fatalf("log viewer command is missing EncodedCommand: %q", commandLine)
	}
	encoded := strings.TrimSuffix(commandLine[start+len(marker):], `"`)
	bytesValue, err := base64.StdEncoding.DecodeString(encoded)
	if err != nil {
		t.Fatalf("decode EncodedCommand: %v", err)
	}
	if len(bytesValue)%2 != 0 {
		t.Fatalf("UTF-16LE byte length = %d, want even", len(bytesValue))
	}
	codeUnits := make([]uint16, len(bytesValue)/2)
	for index := range codeUnits {
		codeUnits[index] = uint16(bytesValue[index*2]) | uint16(bytesValue[index*2+1])<<8
	}
	got := string(utf16.Decode(codeUnits))
	want := `$Host.UI.RawUI.WindowTitle = 'code-Manager 日志'; [Console]::InputEncoding = [System.Text.Encoding]::UTF8; [Console]::OutputEncoding = [System.Text.Encoding]::UTF8; $OutputEncoding = [System.Text.Encoding]::UTF8; Get-Content -LiteralPath 'C:\logs\with space\O''Brien\code-Manager.log' -Encoding UTF8 -Wait`
	if got != want {
		t.Fatalf("decoded PowerShell script = %q, want %q", got, want)
	}
}
