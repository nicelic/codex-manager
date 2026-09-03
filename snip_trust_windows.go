//go:build windows

package main

import (
	"os"
	"os/exec"
	"strings"
	"syscall"

	"golang.org/x/sys/windows"
)

func launchVisiblePowerShell(command, workdir string) error {
	// A GUI-subsystem parent cannot reliably give a directly-created PowerShell
	// process interactive console handles. Use a hidden cmd launcher and let
	// `start` create PowerShell as the top-level visible console instead.
	// EncodedCommand keeps cmd.exe from treating the hook invocation's `&` or
	// paths containing spaces as its own syntax.
	powershellScript := "$env:TERM = $null; " + command
	encoded := encodePowerShellCommand(powershellScript)
	// Pass `start` and each PowerShell argument separately. Passing one fully
	// quoted `start ...` string makes Go quote the `/c` payload again, which can
	// leave cmd.exe waiting instead of creating the console.
	process := exec.Command("cmd.exe", "/d", "/c", "start", "powershell.exe", "-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass", "-NoExit", "-EncodedCommand", encoded)
	process.Dir = workdir
	// Codex's interactive TUI treats TERM=dumb as a non-interactive terminal and
	// may stop at a warning instead of rendering the review screen. Normal
	// Windows PowerShell does not need TERM, so remove only that inherited value.
	env := make([]string, 0, len(os.Environ()))
	for _, entry := range os.Environ() {
		key, _, ok := strings.Cut(entry, "=")
		if ok && strings.EqualFold(key, "TERM") {
			continue
		}
		env = append(env, entry)
	}
	process.Env = env
	process.SysProcAttr = &syscall.SysProcAttr{CreationFlags: windows.CREATE_NO_WINDOW}
	return process.Run()
}
