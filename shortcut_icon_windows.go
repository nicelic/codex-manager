//go:build windows

package main

import (
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"syscall"
	"unsafe"

	"golang.org/x/sys/windows"
)

const (
	clsctxInprocServer      = 0x1
	coinitApartmentThreaded = 0x2
	stgmRead                = 0x0
	slgpRawPath             = 0x4
	shcneAssocChanged       = 0x08000000
	shcneUpdateItem         = 0x00002000
	shcnfIDList             = 0x0000
	shcnfPathW              = 0x0005
	codeManagerExeName      = "code-Manager.exe"
)

var (
	ole32                = windows.NewLazySystemDLL("ole32.dll")
	coInitializeExProc   = ole32.NewProc("CoInitializeEx")
	coCreateInstanceProc = ole32.NewProc("CoCreateInstance")
	coUninitializeProc   = ole32.NewProc("CoUninitialize")
	shell32              = windows.NewLazySystemDLL("shell32.dll")
	shChangeNotifyProc   = shell32.NewProc("SHChangeNotify")

	clsidShellLink  = windows.GUID{Data1: 0x00021401, Data4: [8]byte{0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46}}
	iidIShellLinkW  = windows.GUID{Data1: 0x000214f9, Data4: [8]byte{0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46}}
	iidIPersistFile = windows.GUID{Data1: 0x0000010b, Data4: [8]byte{0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46}}
)

type comObject struct {
	vtable *[21]uintptr
}

type persistFile struct {
	vtable *[9]uintptr
}

// refreshCodeManagerShortcutIcons updates only shortcuts whose target filename
// is code-Manager.exe. Their icon stays inside the active executable at resource
// index zero, so this repair never extracts or writes an icon file at runtime.
func refreshCodeManagerShortcutIcons(executable string) error {
	if strings.TrimSpace(executable) == "" {
		return fmt.Errorf("code-Manager executable path is empty")
	}
	if _, err := os.Stat(executable); err != nil {
		return fmt.Errorf("code-Manager executable is unavailable: %w", err)
	}

	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	needUninit, err := initializeShellCOM()
	if err != nil {
		return err
	}
	if needUninit {
		defer coUninitializeProc.Call()
	}

	for _, root := range codeManagerShortcutRoots() {
		_ = filepath.WalkDir(root, func(path string, entry fs.DirEntry, walkErr error) error {
			if walkErr != nil || entry.IsDir() || !strings.EqualFold(filepath.Ext(path), ".lnk") {
				return nil
			}
			updated, refreshErr := refreshCodeManagerShortcut(path, executable)
			if refreshErr == nil && updated {
				notifyShortcutUpdated(path)
			}
			return nil
		})
	}

	// Explorer may have cached the previous icon for the shortcut. This is a
	// best-effort shell notification and must never delay or prevent startup.
	shChangeNotifyProc.Call(shcneAssocChanged, shcnfIDList, 0, 0)
	return nil
}

func initializeShellCOM() (needUninit bool, err error) {
	result, _, _ := coInitializeExProc.Call(0, coinitApartmentThreaded)
	if hresultFailed(result) {
		return false, hresultError("initialize Windows Shell COM", result)
	}
	// S_OK means this call owns uninitialization. S_FALSE means COM was already
	// initialized on the thread, so CoUninitialize must not run here.
	return result == 0, nil
}

func isCodeManagerShortcutTarget(target string) bool {
	return strings.EqualFold(filepath.Base(strings.TrimSpace(target)), codeManagerExeName)
}

func codeManagerShortcutRoots() []string {
	roots := []string{
		filepath.Join(os.Getenv("USERPROFILE"), "Desktop"),
		filepath.Join(os.Getenv("PUBLIC"), "Desktop"),
		filepath.Join(os.Getenv("APPDATA"), "Microsoft", "Windows", "Start Menu"),
		filepath.Join(os.Getenv("PROGRAMDATA"), "Microsoft", "Windows", "Start Menu"),
		filepath.Join(os.Getenv("APPDATA"), "Microsoft", "Windows", "Start Menu", "Programs", "Startup"),
	}
	seen := make(map[string]struct{}, len(roots))
	valid := make([]string, 0, len(roots))
	for _, root := range roots {
		if root == "" {
			continue
		}
		if _, err := os.Stat(root); err != nil {
			continue
		}
		key := strings.ToLower(filepath.Clean(root))
		if _, ok := seen[key]; ok {
			continue
		}
		seen[key] = struct{}{}
		valid = append(valid, root)
	}
	return valid
}

func refreshCodeManagerShortcut(shortcutPath, executable string) (bool, error) {
	link, err := createShellLink()
	if err != nil {
		return false, err
	}
	defer releaseCOM(link)

	persist, err := queryPersistFile(link)
	if err != nil {
		return false, err
	}
	defer releasePersistFile(persist)

	shortcutPathUTF16, err := windows.UTF16PtrFromString(shortcutPath)
	if err != nil {
		return false, err
	}
	if result := comCall(persist.vtable[5], uintptr(unsafe.Pointer(persist)), uintptr(unsafe.Pointer(shortcutPathUTF16)), stgmRead); hresultFailed(result) {
		return false, hresultError("load shortcut", result)
	}

	target := make([]uint16, 32768)
	if result := comCall(link.vtable[3], uintptr(unsafe.Pointer(link)), uintptr(unsafe.Pointer(&target[0])), uintptr(len(target)), 0, slgpRawPath); hresultFailed(result) {
		return false, hresultError("read shortcut target", result)
	}
	if !isCodeManagerShortcutTarget(windows.UTF16ToString(target)) {
		return false, nil
	}

	executableUTF16, err := windows.UTF16PtrFromString(executable)
	if err != nil {
		return false, err
	}
	if result := comCall(link.vtable[17], uintptr(unsafe.Pointer(link)), uintptr(unsafe.Pointer(executableUTF16)), 0); hresultFailed(result) {
		return false, hresultError("set shortcut icon location", result)
	}
	if result := comCall(persist.vtable[6], uintptr(unsafe.Pointer(persist)), 0, 1); hresultFailed(result) {
		return false, hresultError("save shortcut", result)
	}
	return true, nil
}

func notifyShortcutUpdated(shortcutPath string) {
	pathUTF16, err := windows.UTF16PtrFromString(shortcutPath)
	if err != nil {
		return
	}
	shChangeNotifyProc.Call(shcneUpdateItem, shcnfPathW, uintptr(unsafe.Pointer(pathUTF16)), 0)
}

func createShellLink() (*comObject, error) {
	var link *comObject
	result, _, _ := coCreateInstanceProc.Call(
		uintptr(unsafe.Pointer(&clsidShellLink)),
		0,
		clsctxInprocServer,
		uintptr(unsafe.Pointer(&iidIShellLinkW)),
		uintptr(unsafe.Pointer(&link)),
	)
	if hresultFailed(result) {
		return nil, hresultError("create Windows ShellLink", result)
	}
	return link, nil
}

func queryPersistFile(link *comObject) (*persistFile, error) {
	var persist *persistFile
	result := comCall(
		link.vtable[0],
		uintptr(unsafe.Pointer(link)),
		uintptr(unsafe.Pointer(&iidIPersistFile)),
		uintptr(unsafe.Pointer(&persist)),
	)
	if hresultFailed(result) {
		return nil, hresultError("query IPersistFile", result)
	}
	return persist, nil
}

func releaseCOM(object *comObject) {
	if object != nil {
		comCall(object.vtable[2], uintptr(unsafe.Pointer(object)))
	}
}

func releasePersistFile(object *persistFile) {
	if object != nil {
		comCall(object.vtable[2], uintptr(unsafe.Pointer(object)))
	}
}

func comCall(method uintptr, arguments ...uintptr) uintptr {
	result, _, _ := syscall.SyscallN(method, arguments...)
	return result
}

func hresultFailed(result uintptr) bool {
	return int32(uint32(result)) < 0
}

func hresultError(operation string, result uintptr) error {
	return fmt.Errorf("%s: HRESULT 0x%08X", operation, uint32(result))
}
