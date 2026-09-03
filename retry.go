package main

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"math"
	"net/http"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"sync"
	"time"
	"unicode"
	"unsafe"

	"golang.org/x/sys/windows"
	"gopkg.in/yaml.v3"
)

func (config *Config) UnmarshalYAML(node *yaml.Node) error {
	if node.Kind != yaml.MappingNode {
		return errors.New("config root must be a YAML mapping")
	}
	for index := 0; index+1 < len(node.Content); index += 2 {
		key := node.Content[index].Value
		value := node.Content[index+1]
		switch key {
		case "listen_address":
			if err := value.Decode(&config.ListenAddress); err != nil {
				return err
			}
		case "upstream_base_url":
			if err := value.Decode(&config.UpstreamBaseURL); err != nil {
				return err
			}
		case "upstream_api_key":
			if err := value.Decode(&config.UpstreamAPIKey); err != nil {
				return err
			}
		case "outbound_proxy":
			if err := value.Decode(&config.OutboundProxy); err != nil {
				return err
			}
		case "llmtrim_path":
			if err := value.Decode(&config.LLMTrimPath); err != nil {
				return err
			}
		case "local_api_key":
			if err := value.Decode(&config.LocalAPIKey); err != nil {
				return err
			}
		case "upstream_websocket_enabled":
			if err := value.Decode(&config.UpstreamWebSocketEnabled); err != nil {
				return err
			}
		case "startup_enabled":
			if err := value.Decode(&config.StartupEnabled); err != nil {
				return err
			}
		case "background_start":
			if err := value.Decode(&config.BackgroundStart); err != nil {
				return err
			}
		case "retry_enabled":
			if err := value.Decode(&config.RetryEnabled); err != nil {
				return err
			}
		case "retry_count":
			config.RetryCount = value.Value
		case "retry_interval_seconds":
			config.RetryIntervalSeconds = value.Value
		case "retry_status_codes":
			config.RetryStatusCodes = value.Value
		}
	}
	return nil
}

func applyRetryConfigDefaults(data []byte, config *Config) error {
	var document yaml.Node
	if err := yaml.Unmarshal(data, &document); err != nil {
		return err
	}
	if len(document.Content) != 1 || document.Content[0].Kind != yaml.MappingNode {
		return errors.New("config root must be a YAML mapping")
	}
	present := make(map[string]bool)
	mapping := document.Content[0]
	for index := 0; index+1 < len(mapping.Content); index += 2 {
		present[mapping.Content[index].Value] = true
	}
	if !present["retry_count"] {
		config.RetryCount = defaultRetryCount
	}
	if !present["retry_interval_seconds"] {
		config.RetryIntervalSeconds = defaultRetryIntervalSeconds
	}
	if !present["retry_status_codes"] {
		config.RetryStatusCodes = defaultRetryStatusCodes
	}
	if !present["upstream_websocket_enabled"] {
		config.UpstreamWebSocketEnabled = true
	}
	return nil
}

const (
	defaultRetryCount           = "5"
	defaultRetryIntervalSeconds = "1"
	defaultRetryStatusCodes     = "100-199,300-399,401-407,409-499,500-503,505-523,525-599"
	retryMemoryLimit            = int64(16_000_000_000)
	retryMinimumInterval        = 500 * time.Millisecond
	retryBodyFilePrefix         = ".code-manager-retry-"
	retryBodyChunkSize          = 32 * 1024
	retryTempFileAttempts       = 32
	minimumRetryStatusCode      = uint64(http.StatusContinue)
	maximumRetryStatusCode      = uint64(599)
)

type retryRange struct {
	start uint64
	end   uint64
}

type retryStatusMatcher struct {
	ranges []retryRange
}

func (matcher retryStatusMatcher) empty() bool {
	return len(matcher.ranges) == 0
}

func (matcher retryStatusMatcher) matches(status int) bool {
	if status < 0 {
		return false
	}
	value := uint64(status)
	for _, current := range matcher.ranges {
		if value < current.start {
			return false
		}
		if value <= current.end {
			return true
		}
	}
	return false
}

type retrySettings struct {
	enabled     bool
	maxRetries  int
	interval    time.Duration
	statusCodes string
	matcher     retryStatusMatcher
	wait        func(context.Context, time.Duration) error
}

func makeRetrySettings(config Config) retrySettings {
	countValue := normalizeRetryCount(config.RetryCount)
	count, _ := strconv.Atoi(countValue)
	intervalValue := normalizeRetryInterval(config.RetryIntervalSeconds)
	intervalSeconds, _ := strconv.ParseInt(intervalValue, 10, 64)
	interval := retryMinimumInterval
	if intervalSeconds > 0 && intervalSeconds <= math.MaxInt64/int64(time.Second) {
		interval = time.Duration(intervalSeconds) * time.Second
	}
	statusCodes, matcher := normalizeRetryStatusCodes(config.RetryStatusCodes)
	return retrySettings{
		enabled:     config.RetryEnabled,
		maxRetries:  count,
		interval:    interval,
		statusCodes: statusCodes,
		matcher:     matcher,
	}
}

func retryEnabled(settings retrySettings) bool {
	return settings.enabled && settings.maxRetries > 0 && !settings.matcher.empty()
}

func normalizeRetryCount(value string) string {
	value = removeUnicodeWhitespace(value)
	if value == "" {
		return ""
	}
	if !allASCIIDigits(value) {
		return ""
	}
	parsed, err := strconv.ParseUint(value, 10, 64)
	if err != nil || parsed > 999 {
		return ""
	}
	return strconv.FormatUint(parsed, 10)
}

func normalizeRetryInterval(value string) string {
	value = removeUnicodeWhitespace(value)
	if value == "" {
		return ""
	}
	if !allASCIIDigits(value) {
		return ""
	}
	parsed, err := strconv.ParseUint(value, 10, 64)
	if err != nil || parsed > uint64(math.MaxInt64/int64(time.Second)) {
		return ""
	}
	return strconv.FormatUint(parsed, 10)
}

func normalizeRetryStatusCodes(value string) (string, retryStatusMatcher) {
	value = removeUnicodeWhitespace(value)
	if value == "" {
		return "", retryStatusMatcher{}
	}
	parsed := make([]retryRange, 0)
	for _, token := range strings.Split(value, ",") {
		if token == "" {
			continue
		}
		parts := strings.Split(token, "-")
		if len(parts) > 2 || len(parts) == 0 {
			continue
		}
		if !allASCIIDigits(parts[0]) {
			continue
		}
		start, err := strconv.ParseUint(parts[0], 10, 64)
		if err != nil || start < minimumRetryStatusCode || start > maximumRetryStatusCode {
			continue
		}
		end := start
		if len(parts) == 2 {
			if !allASCIIDigits(parts[1]) {
				continue
			}
			end, err = strconv.ParseUint(parts[1], 10, 64)
			if err != nil || end < minimumRetryStatusCode || end > maximumRetryStatusCode || start > end {
				continue
			}
		}
		parsed = append(parsed, retryRange{start: start, end: end})
	}
	if len(parsed) == 0 {
		return "", retryStatusMatcher{}
	}
	sort.Slice(parsed, func(i, j int) bool {
		if parsed[i].start == parsed[j].start {
			return parsed[i].end < parsed[j].end
		}
		return parsed[i].start < parsed[j].start
	})
	merged := make([]retryRange, 0, len(parsed))
	for _, current := range parsed {
		if len(merged) == 0 {
			merged = append(merged, current)
			continue
		}
		previous := &merged[len(merged)-1]
		if current.start <= previous.end {
			if current.end > previous.end {
				previous.end = current.end
			}
			continue
		}
		merged = append(merged, current)
	}
	parts := make([]string, 0, len(merged))
	for _, current := range merged {
		if current.start == current.end {
			parts = append(parts, strconv.FormatUint(current.start, 10))
		} else {
			parts = append(parts, strconv.FormatUint(current.start, 10)+"-"+strconv.FormatUint(current.end, 10))
		}
	}
	return strings.Join(parts, ","), retryStatusMatcher{ranges: merged}
}

func removeUnicodeWhitespace(value string) string {
	return strings.Map(func(r rune) rune {
		if unicode.IsSpace(r) {
			return -1
		}
		return r
	}, value)
}

func allASCIIDigits(value string) bool {
	if value == "" {
		return false
	}
	for _, char := range value {
		if char < '0' || char > '9' {
			return false
		}
	}
	return true
}

type retryBodyCache struct {
	mu         sync.Mutex
	memoryUsed int64
	limit      int64
	directory  string
}

type replayBody struct {
	cache     *retryBodyCache
	chunks    [][]byte
	path      string
	size      int64
	closeOnce sync.Once
}

func newRetryBodyCache(directory string) *retryBodyCache {
	return &retryBodyCache{limit: retryMemoryLimit, directory: directory}
}

func (cache *retryBodyCache) cleanupStaleFiles() {
	entries, err := os.ReadDir(cache.directory)
	if err != nil {
		return
	}
	for _, entry := range entries {
		if entry.IsDir() || !strings.HasPrefix(entry.Name(), retryBodyFilePrefix) {
			continue
		}
		_ = os.Remove(filepath.Join(cache.directory, entry.Name()))
	}
}

func (cache *retryBodyCache) reserve(size int64) bool {
	if size < 0 {
		return false
	}
	cache.mu.Lock()
	defer cache.mu.Unlock()
	if size > cache.limit-cache.memoryUsed {
		return false
	}
	cache.memoryUsed += size
	return true
}

func (cache *retryBodyCache) release(size int64) {
	if size <= 0 {
		return
	}
	cache.mu.Lock()
	cache.memoryUsed -= size
	if cache.memoryUsed < 0 {
		cache.memoryUsed = 0
	}
	cache.mu.Unlock()
}

func (cache *retryBodyCache) capture(reader io.Reader) (*replayBody, error) {
	chunks := make([][]byte, 0)
	var file *os.File
	var filePath string
	var reserved int64
	cleanup := func() {
		if file != nil {
			_ = file.Close()
		}
		if filePath != "" {
			_ = os.Remove(filePath)
		}
		cache.release(reserved)
	}
	chunk := make([]byte, retryBodyChunkSize)
	var total int64
	for {
		read, err := reader.Read(chunk)
		if read > 0 {
			total += int64(read)
			if file == nil && cache.reserve(int64(read)) {
				reserved += int64(read)
				chunkCopy := make([]byte, read)
				copy(chunkCopy, chunk[:read])
				chunks = append(chunks, chunkCopy)
			} else {
				if file == nil {
					created, createErr := createSecureRetryBodyFile(cache.directory)
					if createErr != nil {
						cleanup()
						return nil, fmt.Errorf("create retry body file: %w", createErr)
					}
					file = created
					filePath = created.Name()
					for _, bufferedChunk := range chunks {
						if _, writeErr := file.Write(bufferedChunk); writeErr != nil {
							cleanup()
							return nil, fmt.Errorf("write retry body file: %w", writeErr)
						}
					}
					cache.release(reserved)
					reserved = 0
					chunks = nil
				}
				if _, writeErr := file.Write(chunk[:read]); writeErr != nil {
					cleanup()
					return nil, fmt.Errorf("write retry body file: %w", writeErr)
				}
			}
		}
		if err != nil {
			if errors.Is(err, io.EOF) {
				break
			}
			cleanup()
			return nil, err
		}
	}
	if file != nil {
		if err := file.Close(); err != nil {
			_ = os.Remove(filePath)
			return nil, fmt.Errorf("close retry body file: %w", err)
		}
		return &replayBody{cache: cache, path: filePath, size: total}, nil
	}
	return &replayBody{cache: cache, chunks: chunks, size: total}, nil
}

func createSecureRetryBodyFile(directory string) (*os.File, error) {
	user, err := windows.GetCurrentProcessToken().GetTokenUser()
	if err != nil {
		return nil, fmt.Errorf("get current user SID: %w", err)
	}
	sid := user.User.Sid.String()
	securityDescriptor, err := windows.SecurityDescriptorFromString("D:P(A;;0x0013019f;;;" + sid + ")")
	if err != nil {
		return nil, fmt.Errorf("create protected retry file ACL: %w", err)
	}
	attributes := &windows.SecurityAttributes{
		Length:             uint32(unsafe.Sizeof(windows.SecurityAttributes{})),
		SecurityDescriptor: securityDescriptor,
	}
	for attempt := 0; attempt < retryTempFileAttempts; attempt++ {
		name, err := retryTempFileName()
		if err != nil {
			return nil, err
		}
		filePath := filepath.Join(directory, name)
		pathPointer, err := windows.UTF16PtrFromString(filePath)
		if err != nil {
			return nil, err
		}
		handle, err := windows.CreateFile(
			pathPointer,
			windows.GENERIC_READ|windows.GENERIC_WRITE|uint32(windows.DELETE),
			0,
			attributes,
			windows.CREATE_NEW,
			windows.FILE_ATTRIBUTE_TEMPORARY,
			0,
		)
		if err == nil {
			file := os.NewFile(uintptr(handle), filePath)
			if file != nil {
				return file, nil
			}
			_ = windows.CloseHandle(handle)
			return nil, errors.New("wrap secure retry body file")
		}
		if errors.Is(err, windows.ERROR_FILE_EXISTS) || errors.Is(err, windows.ERROR_ALREADY_EXISTS) {
			continue
		}
		return nil, err
	}
	return nil, errors.New("create unique secure retry body file")
}

func retryTempFileName() (string, error) {
	data := make([]byte, 16)
	if _, err := rand.Read(data); err != nil {
		return "", fmt.Errorf("generate retry body file name: %w", err)
	}
	return retryBodyFilePrefix + hex.EncodeToString(data), nil
}

func (body *replayBody) Open() (io.ReadCloser, error) {
	if body == nil {
		return http.NoBody, nil
	}
	if body.path != "" {
		return os.Open(body.path)
	}
	readers := make([]io.Reader, 0, len(body.chunks))
	for _, chunk := range body.chunks {
		readers = append(readers, bytes.NewReader(chunk))
	}
	return io.NopCloser(io.MultiReader(readers...)), nil
}

func (body *replayBody) Close() {
	if body == nil {
		return
	}
	body.closeOnce.Do(func() {
		if body.path != "" {
			_ = os.Remove(body.path)
			return
		}
		if body.cache != nil {
			body.cache.release(body.size)
		}
		body.chunks = nil
	})
}

func waitRetryInterval(ctx context.Context, interval time.Duration) error {
	if interval < retryMinimumInterval {
		interval = retryMinimumInterval
	}
	timer := time.NewTimer(interval)
	defer timer.Stop()
	select {
	case <-timer.C:
		return nil
	case <-ctx.Done():
		return context.Cause(ctx)
	}
}

func roundTripWithRetry(ctx context.Context, client *http.Client, method, target string, headers http.Header, contentLength int64, body io.Reader, replay *replayBody, settings retrySettings) (*http.Response, error) {
	if client == nil {
		return nil, errors.New("HTTP 客户端尚未就绪")
	}
	lastStatus := -1
	consecutive := 0
	firstBodyPending := true
	for {
		var requestBody io.Reader
		var requestBodyCloser io.Closer
		if replay != nil {
			opened, err := replay.Open()
			if err != nil {
				return nil, err
			}
			requestBody = opened
			requestBodyCloser = opened
		} else if firstBodyPending {
			requestBody = body
			firstBodyPending = false
		}
		request, err := http.NewRequestWithContext(ctx, method, target, requestBody)
		if err != nil {
			if requestBodyCloser != nil {
				_ = requestBodyCloser.Close()
			}
			return nil, err
		}
		request.Header = headers.Clone()
		if contentLength >= 0 {
			request.ContentLength = contentLength
		}
		request.Header.Del("Content-Length")
		response, err := client.Do(request)
		if requestBodyCloser != nil {
			_ = requestBodyCloser.Close()
		}
		if err != nil {
			return nil, err
		}
		// A WS carrier handshake can return a meaningful upstream HTTP error
		// (for example 429 or 503). It is not an application response that
		// may be replayed through the ordinary transport.
		if isWebSocketHandshakeRejection(response) {
			return response, nil
		}
		if !retryEnabled(settings) || !settings.matcher.matches(response.StatusCode) {
			return response, nil
		}
		if response.StatusCode == lastStatus {
			consecutive++
		} else {
			lastStatus = response.StatusCode
			consecutive = 1
		}
		if consecutive > settings.maxRetries {
			return response, nil
		}
		if response.Body != nil {
			_ = response.Body.Close()
		}
		wait := settings.wait
		if wait == nil {
			wait = waitRetryInterval
		}
		if err := wait(ctx, settings.interval); err != nil {
			return nil, err
		}
	}
}
