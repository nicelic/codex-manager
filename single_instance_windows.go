//go:build windows

package main

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"
	"unsafe"

	"golang.org/x/sys/windows"
)

var errAlreadyRunning = errors.New("code-Manager is already running")

type singleInstance struct {
	handle windows.Handle
}

type runningProcess struct {
	Path string
	ID   int
}

func acquireSingleInstance(name string) (*singleInstance, error) {
	mutexName, err := windows.UTF16PtrFromString(name)
	if err != nil {
		return nil, fmt.Errorf("create instance name: %w", err)
	}
	handle, err := windows.CreateMutex(nil, true, mutexName)
	if err == windows.ERROR_ALREADY_EXISTS {
		_ = windows.CloseHandle(handle)
		return nil, errAlreadyRunning
	}
	if err != nil {
		if handle != 0 {
			_ = windows.CloseHandle(handle)
		}
		return nil, fmt.Errorf("create instance mutex: %w", err)
	}
	return &singleInstance{handle: handle}, nil
}

func (instance *singleInstance) Close() {
	if instance == nil || instance.handle == 0 {
		return
	}
	_ = windows.ReleaseMutex(instance.handle)
	_ = windows.CloseHandle(instance.handle)
	instance.handle = 0
}

func terminateProcessesByPath(target string) error {
	snapshot, err := windows.CreateToolhelp32Snapshot(windows.TH32CS_SNAPPROCESS, 0)
	if err != nil {
		return err
	}
	defer windows.CloseHandle(snapshot)
	var entry windows.ProcessEntry32
	entry.Size = uint32(unsafe.Sizeof(entry))
	err = windows.Process32First(snapshot, &entry)
	for err == nil {
		pid := entry.ProcessID
		if pid != uint32(os.Getpid()) {
			handle, openErr := windows.OpenProcess(windows.PROCESS_QUERY_LIMITED_INFORMATION|windows.PROCESS_TERMINATE, false, pid)
			if openErr == nil {
				imagePath, pathErr := processImagePath(handle)
				if pathErr == nil && strings.EqualFold(filepath.Clean(imagePath), filepath.Clean(target)) {
					_ = windows.TerminateProcess(handle, 1)
				}
				_ = windows.CloseHandle(handle)
			}
		}
		err = windows.Process32Next(snapshot, &entry)
	}
	if err != windows.ERROR_NO_MORE_FILES {
		return err
	}
	return nil
}

// terminateProcessesByNames 仅结束名称精确匹配的进程，用于成对管理 llmtrim CLI 和托盘助手。
func terminateProcessesByNames(names ...string) error {
	targets := make(map[string]struct{}, len(names))
	for _, name := range names {
		if normalized := strings.ToLower(strings.TrimSpace(name)); normalized != "" {
			targets[normalized] = struct{}{}
		}
	}
	if len(targets) == 0 {
		return nil
	}
	snapshot, err := windows.CreateToolhelp32Snapshot(windows.TH32CS_SNAPPROCESS, 0)
	if err != nil {
		return err
	}
	defer windows.CloseHandle(snapshot)
	var failures []string
	var entry windows.ProcessEntry32
	entry.Size = uint32(unsafe.Sizeof(entry))
	err = windows.Process32First(snapshot, &entry)
	for err == nil {
		pid := entry.ProcessID
		if pid != uint32(os.Getpid()) {
			handle, openErr := windows.OpenProcess(windows.PROCESS_QUERY_LIMITED_INFORMATION|windows.PROCESS_TERMINATE, false, pid)
			if openErr == nil {
				imagePath, pathErr := processImagePath(handle)
				if pathErr == nil {
					if _, ok := targets[strings.ToLower(filepath.Base(imagePath))]; ok {
						if terminateErr := windows.TerminateProcess(handle, 1); terminateErr != nil && terminateErr != windows.ERROR_INVALID_PARAMETER {
							failures = append(failures, fmt.Sprintf("%s（PID %d）: %v", imagePath, pid, terminateErr))
						}
					}
				}
				_ = windows.CloseHandle(handle)
			}
		}
		err = windows.Process32Next(snapshot, &entry)
	}
	if err != windows.ERROR_NO_MORE_FILES {
		return err
	}
	if len(failures) > 0 {
		return errors.New(strings.Join(failures, "；"))
	}
	return nil
}

func findProcessIDByExecutable(target string) int {
	snapshot, err := windows.CreateToolhelp32Snapshot(windows.TH32CS_SNAPPROCESS, 0)
	if err != nil {
		return 0
	}
	defer windows.CloseHandle(snapshot)
	var entry windows.ProcessEntry32
	entry.Size = uint32(unsafe.Sizeof(entry))
	if err := windows.Process32First(snapshot, &entry); err != nil {
		return 0
	}
	for {
		pid := entry.ProcessID
		if pid != uint32(os.Getpid()) {
			handle, openErr := windows.OpenProcess(windows.PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
			if openErr == nil {
				imagePath, pathErr := processImagePath(handle)
				_ = windows.CloseHandle(handle)
				if pathErr == nil && strings.EqualFold(filepath.Clean(imagePath), filepath.Clean(target)) {
					return int(pid)
				}
			}
		}
		if err := windows.Process32Next(snapshot, &entry); err != nil {
			break
		}
	}
	return 0
}

func waitForNoProcessByExecutable(target string, timeout time.Duration) bool {
	deadline := time.Now().Add(timeout)
	for time.Now().Before(deadline) {
		if findProcessIDByExecutable(target) == 0 {
			return true
		}
		time.Sleep(200 * time.Millisecond)
	}
	return findProcessIDByExecutable(target) == 0
}

// findRunningProcessByName 只枚举当前进程快照，不扫描磁盘目录。
func findRunningProcessByName(name string) (string, int) {
	processes := findRunningProcessesByNames(name)
	if len(processes) == 0 {
		return "", 0
	}
	return processes[0].Path, processes[0].ID
}

// findRunningProcessesByNames 只枚举当前进程快照，不扫描磁盘目录。
func findRunningProcessesByNames(names ...string) []runningProcess {
	targets := make(map[string]struct{}, len(names))
	for _, name := range names {
		if normalized := strings.ToLower(strings.TrimSpace(name)); normalized != "" {
			targets[normalized] = struct{}{}
		}
	}
	if len(targets) == 0 {
		return nil
	}
	snapshot, err := windows.CreateToolhelp32Snapshot(windows.TH32CS_SNAPPROCESS, 0)
	if err != nil {
		return nil
	}
	defer windows.CloseHandle(snapshot)
	var entry windows.ProcessEntry32
	entry.Size = uint32(unsafe.Sizeof(entry))
	if err := windows.Process32First(snapshot, &entry); err != nil {
		return nil
	}
	processes := make([]runningProcess, 0, len(targets))
	for {
		pid := entry.ProcessID
		if pid != uint32(os.Getpid()) {
			handle, openErr := windows.OpenProcess(windows.PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
			if openErr == nil {
				imagePath, pathErr := processImagePath(handle)
				_ = windows.CloseHandle(handle)
				if pathErr == nil {
					if _, ok := targets[strings.ToLower(filepath.Base(imagePath))]; ok {
						processes = append(processes, runningProcess{Path: filepath.Clean(imagePath), ID: int(pid)})
					}
				}
			}
		}
		if err := windows.Process32Next(snapshot, &entry); err != nil {
			break
		}
	}
	return processes
}

func waitForNoProcessesByNames(timeout time.Duration, names ...string) bool {
	deadline := time.Now().Add(timeout)
	for time.Now().Before(deadline) {
		if len(findRunningProcessesByNames(names...)) == 0 {
			return true
		}
		time.Sleep(200 * time.Millisecond)
	}
	return len(findRunningProcessesByNames(names...)) == 0
}

func processImagePath(handle windows.Handle) (string, error) {
	buffer := make([]uint16, windows.MAX_PATH)
	for {
		size := uint32(len(buffer))
		if err := windows.QueryFullProcessImageName(handle, 0, &buffer[0], &size); err != nil {
			return "", err
		}
		if size < uint32(len(buffer)) {
			return windows.UTF16ToString(buffer[:size]), nil
		}
		buffer = make([]uint16, len(buffer)*2)
		if len(buffer) > 32768 {
			return "", fmt.Errorf("process image path is too long")
		}
	}
}
