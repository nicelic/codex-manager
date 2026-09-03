package main

import (
	"bytes"
	"errors"
	"fmt"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"unicode/utf8"

	"github.com/BurntSushi/toml"
)

const llmtrimManagedConfigMarkerName = ".code-manager-managed"
const llmtrimManagedConfigMarker = "code-manager-llmtrim-config-v1\n"
const llmtrimTrackingDatabaseName = "tracking.db"

type managedLLMTrimTOML struct {
	ExtraHosts []string `toml:"extra_hosts"`
	DBPath     string   `toml:"db_path,omitempty"`
}

type llmtrimConfigMutation struct {
	directory        string
	configPath       string
	markerPath       string
	directoryExisted bool
	configExisted    bool
	configContents   []byte
	markerExisted    bool
	markerContents   []byte
}

func llmtrimManagedConfigDirectory() (string, error) {
	userProfile := strings.TrimSpace(os.Getenv("USERPROFILE"))
	if userProfile == "" {
		return "", errors.New("USERPROFILE is not set; cannot locate llmtrim config")
	}
	absoluteUserProfile, err := filepath.Abs(userProfile)
	if err != nil {
		return "", fmt.Errorf("resolve USERPROFILE: %w", err)
	}
	return filepath.Join(absoluteUserProfile, ".config", "llmtrim"), nil
}

// llmtrimTrackingDatabasePath 将 code-Manager 受管的统计数据固定在
// 受管可执行文件旁，而不是 llmtrim 的用户级默认账本。
func llmtrimTrackingDatabasePath(installDir string) (string, error) {
	absolute, err := filepath.Abs(strings.TrimSpace(installDir))
	if err != nil {
		return "", fmt.Errorf("解析 llmtrim 安装目录失败: %w", err)
	}
	if !strings.EqualFold(filepath.Base(filepath.Clean(absolute)), "llmtrim") {
		return "", fmt.Errorf("llmtrim 统计目录必须命名为 llmtrim: %s", absolute)
	}
	return filepath.Join(absolute, llmtrimTrackingDatabaseName), nil
}

func managedLLMTrimTrackingDatabasePath(executablePath string) (string, bool, error) {
	pathValue := strings.TrimSpace(executablePath)
	if pathValue == "" {
		return "", false, nil
	}
	absolute, err := filepath.Abs(pathValue)
	if err != nil {
		return "", false, fmt.Errorf("解析 llmtrim 可执行文件路径失败: %w", err)
	}
	if !strings.EqualFold(filepath.Base(absolute), llmtrimExecutableName) {
		return "", false, nil
	}
	installDir := filepath.Dir(absolute)
	if !isManagedToolInstallationDirectory("llmtrim", installDir) {
		return "", false, nil
	}
	databasePath, err := llmtrimTrackingDatabasePath(installDir)
	if err != nil {
		return "", false, err
	}
	return databasePath, true, nil
}

// llmtrimDefaultTrackingDatabasePath 对齐 llmtrim 0.13.x 在运行时配置没有
// db_path 时的默认位置；本机通常为 %USERPROFILE%\.local\share\llmtrim\tracking.db。
func llmtrimDefaultTrackingDatabasePath() (string, error) {
	if xdgDataHome := strings.TrimSpace(os.Getenv("XDG_DATA_HOME")); xdgDataHome != "" {
		absolute, err := filepath.Abs(xdgDataHome)
		if err != nil {
			return "", fmt.Errorf("解析 XDG_DATA_HOME 失败: %w", err)
		}
		return filepath.Join(absolute, "llmtrim", llmtrimTrackingDatabaseName), nil
	}
	home := strings.TrimSpace(os.Getenv("HOME"))
	if home == "" {
		home = strings.TrimSpace(os.Getenv("USERPROFILE"))
	}
	if home == "" {
		return "", errors.New("未设置 HOME 或 USERPROFILE，无法定位 llmtrim 统计数据库")
	}
	absolute, err := filepath.Abs(home)
	if err != nil {
		return "", fmt.Errorf("解析 llmtrim 主目录失败: %w", err)
	}
	return filepath.Join(absolute, ".local", "share", "llmtrim", llmtrimTrackingDatabaseName), nil
}

func normalizeLLMTrimTrackingDatabasePath(databasePath string) (string, error) {
	pathValue := strings.TrimSpace(databasePath)
	if pathValue == "" {
		return "", nil
	}
	absolute, err := filepath.Abs(pathValue)
	if err != nil {
		return "", fmt.Errorf("解析 llmtrim 统计数据库失败: %w", err)
	}
	if !strings.EqualFold(filepath.Base(absolute), llmtrimTrackingDatabaseName) ||
		!strings.EqualFold(filepath.Base(filepath.Dir(absolute)), "llmtrim") {
		return "", fmt.Errorf("不安全的 llmtrim 统计数据库路径: %s", absolute)
	}
	return filepath.Clean(absolute), nil
}

// removeLLMTrimTrackingDatabase 只删除 SQLite 主文件及其 WAL/SHM 伴随文件，
// 不会删除所在目录中的无关文件。
func removeLLMTrimTrackingDatabase(databasePath string) (bool, error) {
	normalized, err := normalizeLLMTrimTrackingDatabasePath(databasePath)
	if err != nil {
		return false, err
	}
	if normalized == "" {
		return false, nil
	}
	removed := false
	var failures []string
	for _, candidate := range []string{normalized, normalized + "-wal", normalized + "-shm"} {
		info, statErr := os.Lstat(candidate)
		if errors.Is(statErr, os.ErrNotExist) {
			continue
		}
		if statErr != nil {
			failures = append(failures, fmt.Sprintf("检查 %s 失败: %v", candidate, statErr))
			continue
		}
		if info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
			failures = append(failures, fmt.Sprintf("不安全的 llmtrim 统计数据库文件: %s", candidate))
			continue
		}
		if removeErr := os.Remove(candidate); removeErr != nil {
			failures = append(failures, fmt.Sprintf("删除 %s 失败: %v", candidate, removeErr))
			continue
		}
		removed = true
		if _, verifyErr := os.Lstat(candidate); !errors.Is(verifyErr, os.ErrNotExist) {
			if verifyErr == nil {
				failures = append(failures, fmt.Sprintf("统计数据库文件仍存在: %s", candidate))
			} else {
				failures = append(failures, fmt.Sprintf("校验 %s 失败: %v", candidate, verifyErr))
			}
		}
	}
	if len(failures) > 0 {
		return removed, errors.New(strings.Join(failures, "; "))
	}
	return removed, nil
}

func llmtrimUpstreamHost(rawURL string) (string, error) {
	parsed, err := url.ParseRequestURI(rawURL)
	if err != nil {
		return "", err
	}
	host := strings.TrimSpace(parsed.Hostname())
	if host == "" {
		return "", errors.New("upstream_base_url does not contain a hostname")
	}
	return strings.ToLower(host), nil
}

func syncManagedLLMTrimConfig(rawURL string) (*llmtrimConfigMutation, error) {
	return syncManagedLLMTrimConfigWithDatabase(rawURL, "")
}

func syncManagedLLMTrimConfigForExecutable(rawURL, executablePath string) (*llmtrimConfigMutation, error) {
	databasePath, managed, err := managedLLMTrimTrackingDatabasePath(executablePath)
	if err != nil {
		return nil, err
	}
	if !managed {
		return syncManagedLLMTrimConfig(rawURL)
	}
	return syncManagedLLMTrimConfigWithDatabase(rawURL, databasePath)
}

// syncManagedLLMTrimDatabaseConfigIfOwned 会在下一次启动时迁移已有的受管配置，
// 但不会接管用户自己的 config.toml。
func syncManagedLLMTrimDatabaseConfigIfOwned(rawURL, executablePath string) error {
	databasePath, managed, err := managedLLMTrimTrackingDatabasePath(executablePath)
	if err != nil || !managed {
		return err
	}
	directory, err := llmtrimManagedConfigDirectory()
	if err != nil {
		return err
	}
	marker, markerExists, err := readOptionalFile(filepath.Join(directory, llmtrimManagedConfigMarkerName))
	if err != nil {
		return fmt.Errorf("读取 llmtrim 配置归属标记失败: %w", err)
	}
	if !markerExists || string(marker) != llmtrimManagedConfigMarker {
		return nil
	}
	_, err = syncManagedLLMTrimConfigWithDatabase(rawURL, databasePath)
	return err
}

func syncManagedLLMTrimConfigWithDatabase(rawURL, databasePath string) (*llmtrimConfigMutation, error) {
	host, err := llmtrimUpstreamHost(rawURL)
	if err != nil {
		return nil, fmt.Errorf("derive llmtrim extra_hosts: %w", err)
	}
	normalizedDatabasePath, err := normalizeLLMTrimTrackingDatabasePath(databasePath)
	if err != nil {
		return nil, err
	}
	directory, err := llmtrimManagedConfigDirectory()
	if err != nil {
		return nil, err
	}
	mutation, err := snapshotLLMTrimConfig(directory)
	if err != nil {
		return nil, err
	}
	if err := os.MkdirAll(directory, 0700); err != nil {
		return nil, fmt.Errorf("create llmtrim config directory: %w", err)
	}
	rollback := func(operation string, cause error) (*llmtrimConfigMutation, error) {
		if rollbackErr := mutation.rollback(); rollbackErr != nil {
			return nil, fmt.Errorf("%s: %w; rollback failed: %v", operation, cause, rollbackErr)
		}
		return nil, fmt.Errorf("%s: %w", operation, cause)
	}
	configContents, err := marshalManagedLLMTrimTOML(host, normalizedDatabasePath)
	if err != nil {
		return rollback("serialize llmtrim config", err)
	}
	if err := writeAtomicFile(mutation.configPath, configContents, 0600); err != nil {
		return rollback("write llmtrim config", err)
	}
	if err := writeAtomicFile(mutation.markerPath, []byte(llmtrimManagedConfigMarker), 0600); err != nil {
		return rollback("write llmtrim config marker", err)
	}
	return mutation, nil
}

func marshalManagedLLMTrimTOML(host, databasePath string) ([]byte, error) {
	var encoded bytes.Buffer
	if err := toml.NewEncoder(&encoded).Encode(managedLLMTrimTOML{ExtraHosts: []string{host}, DBPath: databasePath}); err != nil {
		return nil, err
	}
	return append([]byte("# Managed by code-Manager.\n"), encoded.Bytes()...), nil
}

func saveUpstreamBaseURLWithLLMTrimConfig(configPath, rawURL string) error {
	return saveUpstreamBaseURLWithLLMTrimConfigForExecutable(configPath, rawURL, "")
}

func saveUpstreamBaseURLWithLLMTrimConfigForExecutable(configPath, rawURL, executablePath string) error {
	mutation, err := syncManagedLLMTrimConfigForExecutable(rawURL, executablePath)
	if err != nil {
		return err
	}
	if err := updateConfigValue(configPath, "upstream_base_url", rawURL); err != nil {
		rollbackErr := mutation.rollback()
		if rollbackErr != nil {
			return fmt.Errorf("save code-Manager upstream URL: %w; rollback llmtrim config failed: %v", err, rollbackErr)
		}
		return fmt.Errorf("save code-Manager upstream URL: %w", err)
	}
	return nil
}

func snapshotLLMTrimConfig(directory string) (*llmtrimConfigMutation, error) {
	mutation := &llmtrimConfigMutation{
		directory:  directory,
		configPath: filepath.Join(directory, "config.toml"),
		markerPath: filepath.Join(directory, llmtrimManagedConfigMarkerName),
	}
	info, err := os.Stat(directory)
	if err == nil {
		if !info.IsDir() {
			return nil, fmt.Errorf("llmtrim config path is not a directory: %s", directory)
		}
		mutation.directoryExisted = true
	} else if !errors.Is(err, os.ErrNotExist) {
		return nil, fmt.Errorf("stat llmtrim config directory: %w", err)
	}
	if mutation.configContents, mutation.configExisted, err = readOptionalFile(mutation.configPath); err != nil {
		return nil, fmt.Errorf("read existing llmtrim config: %w", err)
	}
	if mutation.markerContents, mutation.markerExisted, err = readOptionalFile(mutation.markerPath); err != nil {
		return nil, fmt.Errorf("read existing llmtrim config marker: %w", err)
	}
	return mutation, nil
}

func readOptionalFile(filePath string) ([]byte, bool, error) {
	contents, err := os.ReadFile(filePath)
	if errors.Is(err, os.ErrNotExist) {
		return nil, false, nil
	}
	if err != nil {
		return nil, false, err
	}
	if !utf8.Valid(contents) {
		return nil, false, fmt.Errorf("文件不是有效 UTF-8: %s", filePath)
	}
	return contents, true, nil
}

func (mutation *llmtrimConfigMutation) rollback() error {
	if mutation == nil {
		return nil
	}
	var failures []string
	if err := restoreOptionalFile(mutation.configPath, mutation.configExisted, mutation.configContents); err != nil {
		failures = append(failures, "restore config.toml: "+err.Error())
	}
	if err := restoreOptionalFile(mutation.markerPath, mutation.markerExisted, mutation.markerContents); err != nil {
		failures = append(failures, "restore marker: "+err.Error())
	}
	if !mutation.directoryExisted {
		if err := os.Remove(mutation.directory); err != nil && !errors.Is(err, os.ErrNotExist) {
			failures = append(failures, "remove new config directory: "+err.Error())
		}
	}
	if len(failures) > 0 {
		return errors.New(strings.Join(failures, "; "))
	}
	return nil
}

func restoreOptionalFile(filePath string, existed bool, contents []byte) error {
	if existed {
		return writeAtomicFile(filePath, contents, 0600)
	}
	err := os.Remove(filePath)
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	return err
}

func writeAtomicFile(filePath string, contents []byte, fallbackMode os.FileMode) error {
	if !utf8.Valid(contents) {
		return errors.New("拒绝写入非 UTF-8 文件")
	}
	directory := filepath.Dir(filePath)
	info, err := os.Stat(filePath)
	mode := fallbackMode
	if err == nil {
		mode = info.Mode().Perm()
	} else if !errors.Is(err, os.ErrNotExist) {
		return err
	}
	temporary, err := os.CreateTemp(directory, ".code-manager-llmtrim-*")
	if err != nil {
		return err
	}
	temporaryPath := temporary.Name()
	defer func() { _ = os.Remove(temporaryPath) }()
	if err := temporary.Chmod(mode); err != nil {
		_ = temporary.Close()
		return err
	}
	if _, err := temporary.Write(contents); err != nil {
		_ = temporary.Close()
		return err
	}
	if err := temporary.Close(); err != nil {
		return err
	}
	return os.Rename(temporaryPath, filePath)
}

func removeManagedLLMTrimConfigDirectory() error {
	directory, err := llmtrimManagedConfigDirectory()
	if err != nil {
		return err
	}
	absoluteDirectory, parent, err := validateManagedLLMTrimConfigTarget(directory)
	if err != nil {
		return err
	}
	info, err := os.Lstat(absoluteDirectory)
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	if err != nil {
		return fmt.Errorf("stat llmtrim config directory: %w", err)
	}
	if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
		return fmt.Errorf("unsafe llmtrim config directory: %s", absoluteDirectory)
	}
	parentInfo, err := os.Lstat(parent)
	if err != nil {
		return fmt.Errorf("stat llmtrim config parent directory: %w", err)
	}
	if !parentInfo.IsDir() || parentInfo.Mode()&os.ModeSymlink != 0 {
		return fmt.Errorf("unsafe llmtrim config parent directory: %s", parent)
	}
	markerPath := filepath.Join(absoluteDirectory, llmtrimManagedConfigMarkerName)
	marker, markerExists, err := readOptionalFile(markerPath)
	if err != nil {
		return fmt.Errorf("read llmtrim config ownership marker: %w", err)
	}
	if !markerExists {
		return errors.New("llmtrim config ownership marker is missing")
	}
	if string(marker) != llmtrimManagedConfigMarker {
		return errors.New("llmtrim config directory is not managed by code-Manager")
	}
	if err := os.RemoveAll(absoluteDirectory); err != nil {
		return fmt.Errorf("remove managed llmtrim config directory: %w", err)
	}
	if _, err := os.Stat(absoluteDirectory); !errors.Is(err, os.ErrNotExist) {
		if err == nil {
			return errors.New("managed llmtrim config directory still exists after removal")
		}
		return fmt.Errorf("verify managed llmtrim config removal: %w", err)
	}
	return nil
}

func validateManagedLLMTrimConfigTarget(directory string) (string, string, error) {
	expected, err := llmtrimManagedConfigDirectory()
	if err != nil {
		return "", "", err
	}
	expectedAbsolute, err := filepath.Abs(expected)
	if err != nil {
		return "", "", fmt.Errorf("resolve expected llmtrim config directory: %w", err)
	}
	absoluteDirectory, err := filepath.Abs(directory)
	if err != nil {
		return "", "", fmt.Errorf("resolve llmtrim config directory: %w", err)
	}
	if !strings.EqualFold(filepath.Clean(absoluteDirectory), filepath.Clean(expectedAbsolute)) {
		return "", "", fmt.Errorf("unsafe llmtrim config directory: %s", absoluteDirectory)
	}
	parent := filepath.Dir(absoluteDirectory)
	expectedParent := filepath.Dir(expectedAbsolute)
	if !strings.EqualFold(filepath.Clean(parent), filepath.Clean(expectedParent)) ||
		!strings.EqualFold(filepath.Base(filepath.Clean(absoluteDirectory)), "llmtrim") ||
		!strings.EqualFold(filepath.Base(filepath.Clean(parent)), ".config") {
		return "", "", fmt.Errorf("unsafe llmtrim config parent directory: %s", parent)
	}
	return absoluteDirectory, parent, nil
}
