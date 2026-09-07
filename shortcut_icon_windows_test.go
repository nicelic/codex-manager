//go:build windows

package main

import (
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"unsafe"

	"golang.org/x/sys/windows"
)

func TestIsCodeManagerShortcutTarget(t *testing.T) {
	t.Parallel()
	tests := []struct {
		name   string
		target string
		want   bool
	}{
		{name: "exact exe", target: `C:\releases\code-Manager\code-Manager.exe`, want: true},
		{name: "case insensitive", target: `D:\Old\CODE-MANAGER.EXE`, want: true},
		{name: "quoted spacing", target: "  C:\\Users\\me\\Desktop\\code-Manager.exe  ", want: true},
		{name: "other program", target: `C:\Windows\notepad.exe`, want: false},
		{name: "similar name", target: `C:\tools\code-Manager-helper.exe`, want: false},
		{name: "empty", target: "   ", want: false},
	}
	for _, test := range tests {
		test := test
		t.Run(test.name, func(t *testing.T) {
			t.Parallel()
			if got := isCodeManagerShortcutTarget(test.target); got != test.want {
				t.Fatalf("isCodeManagerShortcutTarget(%q) = %v, want %v", test.target, got, test.want)
			}
		})
	}
}

func TestRefreshCodeManagerShortcutUpdatesMatchingLink(t *testing.T) {
	dir := t.TempDir()
	matchingExe := filepath.Join(dir, "code-Manager.exe")
	otherExe := filepath.Join(dir, "notepad-standin.exe")
	newExe := filepath.Join(dir, "current", "code-Manager.exe")
	if err := os.MkdirAll(filepath.Dir(newExe), 0o755); err != nil {
		t.Fatal(err)
	}
	for _, path := range []string{matchingExe, otherExe, newExe} {
		if err := os.WriteFile(path, []byte("exe"), 0o644); err != nil {
			t.Fatal(err)
		}
	}

	matchingLink := filepath.Join(dir, "code-Manager.lnk")
	otherLink := filepath.Join(dir, "other.lnk")
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	needUninit, err := initializeShellCOM()
	if err != nil {
		t.Fatalf("initialize COM: %v", err)
	}
	if needUninit {
		defer coUninitializeProc.Call()
	}

	if err := writeTestShortcut(matchingLink, matchingExe, otherExe); err != nil {
		t.Fatalf("create matching shortcut: %v", err)
	}
	if err := writeTestShortcut(otherLink, otherExe, otherExe); err != nil {
		t.Fatalf("create other shortcut: %v", err)
	}

	updated, err := refreshCodeManagerShortcut(matchingLink, newExe)
	if err != nil {
		t.Fatalf("refresh matching shortcut: %v", err)
	}
	if !updated {
		t.Fatal("matching shortcut was not updated")
	}
	iconPath, iconIndex, err := readShortcutIconLocation(matchingLink)
	if err != nil {
		t.Fatalf("read matching icon: %v", err)
	}
	if !equalWindowsPath(iconPath, newExe) {
		t.Fatalf("matching icon path = %q, want %q", iconPath, newExe)
	}
	if iconIndex != 0 {
		t.Fatalf("matching icon index = %d, want 0", iconIndex)
	}

	updated, err = refreshCodeManagerShortcut(otherLink, newExe)
	if err != nil {
		t.Fatalf("refresh other shortcut: %v", err)
	}
	if updated {
		t.Fatal("non-code-Manager shortcut was modified")
	}
	iconPath, _, err = readShortcutIconLocation(otherLink)
	if err != nil {
		t.Fatalf("read other icon: %v", err)
	}
	if !equalWindowsPath(iconPath, otherExe) {
		t.Fatalf("other icon path = %q, want %q", iconPath, otherExe)
	}
}

func writeTestShortcut(shortcutPath, target, icon string) error {
	link, err := createShellLink()
	if err != nil {
		return err
	}
	defer releaseCOM(link)
	persist, err := queryPersistFile(link)
	if err != nil {
		return err
	}
	defer releasePersistFile(persist)

	targetUTF16, err := windows.UTF16PtrFromString(target)
	if err != nil {
		return err
	}
	if result := comCall(link.vtable[20], uintptr(unsafe.Pointer(link)), uintptr(unsafe.Pointer(targetUTF16))); hresultFailed(result) {
		return hresultError("set shortcut path", result)
	}
	iconUTF16, err := windows.UTF16PtrFromString(icon)
	if err != nil {
		return err
	}
	if result := comCall(link.vtable[17], uintptr(unsafe.Pointer(link)), uintptr(unsafe.Pointer(iconUTF16)), 0); hresultFailed(result) {
		return hresultError("set shortcut icon location", result)
	}
	shortcutUTF16, err := windows.UTF16PtrFromString(shortcutPath)
	if err != nil {
		return err
	}
	if result := comCall(persist.vtable[6], uintptr(unsafe.Pointer(persist)), uintptr(unsafe.Pointer(shortcutUTF16)), 1); hresultFailed(result) {
		return hresultError("save shortcut", result)
	}
	return nil
}

func readShortcutIconLocation(shortcutPath string) (string, int32, error) {
	link, err := createShellLink()
	if err != nil {
		return "", 0, err
	}
	defer releaseCOM(link)
	persist, err := queryPersistFile(link)
	if err != nil {
		return "", 0, err
	}
	defer releasePersistFile(persist)
	shortcutUTF16, err := windows.UTF16PtrFromString(shortcutPath)
	if err != nil {
		return "", 0, err
	}
	if result := comCall(persist.vtable[5], uintptr(unsafe.Pointer(persist)), uintptr(unsafe.Pointer(shortcutUTF16)), stgmRead); hresultFailed(result) {
		return "", 0, hresultError("load shortcut", result)
	}
	buf := make([]uint16, 32768)
	var index int32
	if result := comCall(link.vtable[16], uintptr(unsafe.Pointer(link)), uintptr(unsafe.Pointer(&buf[0])), uintptr(len(buf)), uintptr(unsafe.Pointer(&index))); hresultFailed(result) {
		return "", 0, hresultError("read shortcut icon location", result)
	}
	return windows.UTF16ToString(buf), index, nil
}

func equalWindowsPath(got, want string) bool {
	gotAbs, err := filepath.Abs(got)
	if err != nil {
		return false
	}
	wantAbs, err := filepath.Abs(want)
	if err != nil {
		return false
	}
	return strings.EqualFold(filepath.Clean(gotAbs), filepath.Clean(wantAbs))
}
