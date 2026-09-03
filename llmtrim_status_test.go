package main

import "testing"

func TestLLMTrimActivationState(t *testing.T) {
	tests := []struct {
		name     string
		snapshot llmtrimActivationSnapshot
		want     string
	}{
		{
			name: "not installed",
			want: "not_installed",
		},
		{
			name: "installed and stopped",
			snapshot: llmtrimActivationSnapshot{
				Installed:               true,
				ConfiguredPath:          `C:\tools\llmtrim.exe`,
				ConfiguredPathAvailable: true,
			},
			want: "installed_stopped",
		},
		{
			name: "healthy daemon is running",
			snapshot: llmtrimActivationSnapshot{
				Installed:               true,
				ConfiguredPath:          `C:\tools\llmtrim.exe`,
				ConfiguredPathAvailable: true,
				Running:                 true,
				WindowsConfigured:       true,
			},
			want: "running",
		},
		{
			name: "running daemon without Windows setup needs attention",
			snapshot: llmtrimActivationSnapshot{
				Installed:               true,
				ConfiguredPath:          `C:\tools\llmtrim.exe`,
				ConfiguredPathAvailable: true,
				Running:                 true,
			},
			want: "attention",
		},
		{
			name: "requested daemon did not recover",
			snapshot: llmtrimActivationSnapshot{
				Installed:               true,
				ConfiguredPath:          `C:\tools\llmtrim.exe`,
				ConfiguredPathAvailable: true,
				DesiredRunning:          true,
			},
			want: "attention",
		},
		{
			name: "tray left behind",
			snapshot: llmtrimActivationSnapshot{
				Installed:               true,
				ConfiguredPath:          `C:\tools\llmtrim.exe`,
				ConfiguredPathAvailable: true,
				TrayRunning:             true,
			},
			want: "attention",
		},
		{
			name: "configured path is unavailable",
			snapshot: llmtrimActivationSnapshot{
				Installed:      true,
				ConfiguredPath: `C:\tools\llmtrim.exe`,
			},
			want: "attention",
		},
		{
			name: "orphaned install directory",
			snapshot: llmtrimActivationSnapshot{
				DirectoryExists: true,
			},
			want: "attention",
		},
		{
			name: "unavailable state ledger",
			snapshot: llmtrimActivationSnapshot{
				StateReadFailed: true,
			},
			want: "attention",
		},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			if got := llmtrimActivationState(test.snapshot); got != test.want {
				t.Fatalf("llmtrimActivationState() = %q, want %q", got, test.want)
			}
		})
	}
}
