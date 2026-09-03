package main

import (
	"bytes"
	"context"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"
	"unsafe"

	"golang.org/x/sys/windows"
)

type retryRoundTripperFunc func(*http.Request) (*http.Response, error)

func (fn retryRoundTripperFunc) RoundTrip(request *http.Request) (*http.Response, error) {
	return fn(request)
}

func retryResponse(status int, body string, headers map[string]string) *http.Response {
	responseHeaders := make(http.Header)
	for key, value := range headers {
		responseHeaders.Set(key, value)
	}
	return &http.Response{
		StatusCode: status,
		Header:     responseHeaders,
		Body:       io.NopCloser(strings.NewReader(body)),
	}
}

func noWait(context.Context, time.Duration) error {
	return nil
}

func TestNormalizeRetryStatusCodes(t *testing.T) {
	value, matcher := normalizeRetryStatusCodes(" 99,99-200,100-200,99,201-202,300-301,302,300-399,400-399,500-600,599,600,abc,0, ")
	if value != "100-200,201-202,300-399,599" {
		t.Fatalf("normalized value = %q", value)
	}
	for _, status := range []int{100, 200, 202, 300, 399, 599} {
		if !matcher.matches(status) {
			t.Fatalf("expected status %d to match", status)
		}
	}
	for _, status := range []int{0, 1, 99, 203, 400, 500, 600} {
		if matcher.matches(status) {
			t.Fatalf("expected status %d not to match", status)
		}
	}
}

func TestRetryValueNormalization(t *testing.T) {
	if got := normalizeRetryCount(" 005 "); got != "5" {
		t.Fatalf("retry count = %q", got)
	}
	if got := normalizeRetryCount("1000"); got != "" {
		t.Fatalf("out-of-range retry count = %q", got)
	}
	if got := normalizeRetryInterval(" 0 "); got != "0" {
		t.Fatalf("retry interval = %q", got)
	}
	if got := normalizeRetryInterval("-1"); got != "" {
		t.Fatalf("invalid retry interval = %q", got)
	}
	settings := makeRetrySettings(Config{RetryEnabled: true, RetryCount: "1", RetryIntervalSeconds: "0", RetryStatusCodes: "429"})
	if settings.interval != retryMinimumInterval {
		t.Fatalf("zero interval = %s, want %s", settings.interval, retryMinimumInterval)
	}
}

func TestWaitRetryIntervalEnforcesMinimum(t *testing.T) {
	started := time.Now()
	if err := waitRetryInterval(context.Background(), 0); err != nil {
		t.Fatal(err)
	}
	if elapsed := time.Since(started); elapsed < retryMinimumInterval-50*time.Millisecond {
		t.Fatalf("waited %s, want at least about %s", elapsed, retryMinimumInterval)
	}
}

func TestRoundTripWithRetryReplaysPOSTBody(t *testing.T) {
	var attempts int
	var bodies []string
	client := &http.Client{Transport: retryRoundTripperFunc(func(request *http.Request) (*http.Response, error) {
		attempts++
		body, err := io.ReadAll(request.Body)
		if err != nil {
			return nil, err
		}
		bodies = append(bodies, string(body))
		if attempts < 3 {
			return retryResponse(http.StatusTooManyRequests, "retry", nil), nil
		}
		return retryResponse(http.StatusOK, "final", nil), nil
	})}
	cache := newRetryBodyCache(t.TempDir())
	replay, err := cache.capture(strings.NewReader(`{"model":"test"}`))
	if err != nil {
		t.Fatal(err)
	}
	defer replay.Close()
	settings := retrySettings{enabled: true, maxRetries: 5, matcher: retryStatusMatcher{ranges: []retryRange{{start: 429, end: 429}}}, wait: noWait}
	response, err := roundTripWithRetry(context.Background(), client, http.MethodPost, "https://upstream.example/v1/responses", http.Header{"Content-Type": {"application/json"}}, replay.size, nil, replay, settings)
	if err != nil {
		t.Fatal(err)
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK || attempts != 3 {
		t.Fatalf("status=%d attempts=%d", response.StatusCode, attempts)
	}
	if got := strings.Join(bodies, "|"); got != `{"model":"test"}|{"model":"test"}|{"model":"test"}` {
		t.Fatalf("replayed bodies = %q", got)
	}
}

func TestRoundTripWithRetryResetsCountWhenStatusChanges(t *testing.T) {
	statuses := []int{429, 503, 429, 200}
	attempt := 0
	client := &http.Client{Transport: retryRoundTripperFunc(func(*http.Request) (*http.Response, error) {
		status := statuses[attempt]
		attempt++
		return retryResponse(status, "body", nil), nil
	})}
	settings := retrySettings{
		enabled:    true,
		maxRetries: 1,
		matcher:    retryStatusMatcher{ranges: []retryRange{{start: 429, end: 429}, {start: 503, end: 503}}},
		wait:       noWait,
	}
	response, err := roundTripWithRetry(context.Background(), client, http.MethodGet, "https://upstream.example/v1/models", nil, -1, nil, nil, settings)
	if err != nil {
		t.Fatal(err)
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK || attempt != 4 {
		t.Fatalf("status=%d attempts=%d", response.StatusCode, attempt)
	}
}

func TestRoundTripWithRetryStopsAfterConfiguredRetries(t *testing.T) {
	attempts := 0
	client := &http.Client{Transport: retryRoundTripperFunc(func(*http.Request) (*http.Response, error) {
		attempts++
		return retryResponse(http.StatusTooManyRequests, "retry", nil), nil
	})}
	settings := retrySettings{enabled: true, maxRetries: 2, matcher: retryStatusMatcher{ranges: []retryRange{{start: 429, end: 429}}}, wait: noWait}
	response, err := roundTripWithRetry(context.Background(), client, http.MethodGet, "https://upstream.example/v1/models", nil, -1, nil, nil, settings)
	if err != nil {
		t.Fatal(err)
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusTooManyRequests || attempts != 3 {
		t.Fatalf("status=%d attempts=%d", response.StatusCode, attempts)
	}
}

func TestRoundTripWithRetryDoesNotRetryNetworkErrors(t *testing.T) {
	attempts := 0
	client := &http.Client{Transport: retryRoundTripperFunc(func(*http.Request) (*http.Response, error) {
		attempts++
		return nil, errors.New("dial failed")
	})}
	settings := retrySettings{enabled: true, maxRetries: 5, matcher: retryStatusMatcher{ranges: []retryRange{{start: 429, end: 429}}}, wait: noWait}
	_, err := roundTripWithRetry(context.Background(), client, http.MethodGet, "https://upstream.example/v1/models", nil, -1, nil, nil, settings)
	if err == nil || attempts != 1 {
		t.Fatalf("err=%v attempts=%d", err, attempts)
	}
}

func TestRoundTripWithRetryKeepsCountersPerFlow(t *testing.T) {
	var mu sync.Mutex
	attempts := make(map[string]int)
	client := &http.Client{Transport: retryRoundTripperFunc(func(request *http.Request) (*http.Response, error) {
		id := request.Header.Get("X-Flow")
		mu.Lock()
		attempts[id]++
		current := attempts[id]
		mu.Unlock()
		if current == 1 {
			return retryResponse(http.StatusTooManyRequests, "retry", nil), nil
		}
		return retryResponse(http.StatusOK, id, nil), nil
	})}
	settings := retrySettings{enabled: true, maxRetries: 1, matcher: retryStatusMatcher{ranges: []retryRange{{start: 429, end: 429}}}, wait: noWait}
	var workers sync.WaitGroup
	for _, id := range []string{"one", "two"} {
		workers.Add(1)
		go func(id string) {
			defer workers.Done()
			response, err := roundTripWithRetry(context.Background(), client, http.MethodGet, "https://upstream.example/v1/models", http.Header{"X-Flow": {id}}, -1, nil, nil, settings)
			if err != nil {
				t.Errorf("flow %s: %v", id, err)
				return
			}
			_ = response.Body.Close()
		}(id)
	}
	workers.Wait()
	if attempts["one"] != 2 || attempts["two"] != 2 {
		t.Fatalf("attempts = %#v", attempts)
	}
}

func TestRoundTripWithRetryStopsWaitingWhenClientCancels(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	client := &http.Client{Transport: retryRoundTripperFunc(func(*http.Request) (*http.Response, error) {
		cancel()
		return retryResponse(http.StatusTooManyRequests, "retry", nil), nil
	})}
	settings := retrySettings{enabled: true, maxRetries: 1, interval: 10 * time.Second, matcher: retryStatusMatcher{ranges: []retryRange{{start: 429, end: 429}}}}
	started := time.Now()
	_, err := roundTripWithRetry(ctx, client, http.MethodGet, "https://upstream.example/v1/models", nil, -1, nil, nil, settings)
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("err = %v", err)
	}
	if elapsed := time.Since(started); elapsed > time.Second {
		t.Fatalf("cancellation took %s", elapsed)
	}
}

func TestRetryBodyCacheFallsBackToFileAndCleansUp(t *testing.T) {
	directory := t.TempDir()
	cache := newRetryBodyCache(directory)
	cache.limit = 3
	body, err := cache.capture(strings.NewReader("1234"))
	if err != nil {
		t.Fatal(err)
	}
	if body.path == "" {
		t.Fatal("expected request body to fall back to a temporary file")
	}
	if _, err := os.Stat(body.path); err != nil {
		t.Fatal(err)
	}
	body.Close()
	if _, err := os.Stat(body.path); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("temporary file remains: %v", err)
	}
	stale := filepath.Join(directory, retryBodyFilePrefix+"stale")
	if err := os.WriteFile(stale, []byte("stale"), 0600); err != nil {
		t.Fatal(err)
	}
	cache.cleanupStaleFiles()
	if _, err := os.Stat(stale); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("stale temporary file remains: %v", err)
	}
}

func TestRetryBodyCacheUsesSharedBudgetAcrossBodies(t *testing.T) {
	cache := newRetryBodyCache(t.TempDir())
	cache.limit = 3
	first, err := cache.capture(strings.NewReader("123"))
	if err != nil {
		t.Fatal(err)
	}
	defer first.Close()
	if first.path != "" {
		t.Fatal("first body unexpectedly used a temporary file")
	}
	second, err := cache.capture(strings.NewReader("456"))
	if err != nil {
		t.Fatal(err)
	}
	defer second.Close()
	if second.path == "" {
		t.Fatal("second body did not use a temporary file after shared budget was exhausted")
	}
	cache.mu.Lock()
	memoryUsed := cache.memoryUsed
	cache.mu.Unlock()
	if memoryUsed != 3 {
		t.Fatalf("memoryUsed = %d, want 3", memoryUsed)
	}
}

func TestReplayBodyReplaysChunkedMemory(t *testing.T) {
	value := strings.Repeat("a", retryBodyChunkSize*2+1)
	cache := newRetryBodyCache(t.TempDir())
	body, err := cache.capture(strings.NewReader(value))
	if err != nil {
		t.Fatal(err)
	}
	defer body.Close()
	if len(body.chunks) != 3 {
		t.Fatalf("chunk count = %d, want 3", len(body.chunks))
	}
	for attempt := 0; attempt < 2; attempt++ {
		opened, err := body.Open()
		if err != nil {
			t.Fatal(err)
		}
		actual, readErr := io.ReadAll(opened)
		closeErr := opened.Close()
		if readErr != nil || closeErr != nil {
			t.Fatalf("read=%v close=%v", readErr, closeErr)
		}
		if string(actual) != value {
			t.Fatalf("attempt %d replayed %d bytes, want %d", attempt, len(actual), len(value))
		}
	}
}

func TestRetryBodyFileUsesOnlyCurrentUserProtectedDACL(t *testing.T) {
	cache := newRetryBodyCache(t.TempDir())
	cache.limit = 0
	body, err := cache.capture(strings.NewReader("body"))
	if err != nil {
		t.Fatal(err)
	}
	defer body.Close()
	if body.path == "" {
		t.Fatal("expected a temporary file")
	}
	descriptor, err := windows.GetNamedSecurityInfo(body.path, windows.SE_FILE_OBJECT, windows.DACL_SECURITY_INFORMATION)
	if err != nil {
		t.Fatal(err)
	}
	control, _, err := descriptor.Control()
	if err != nil {
		t.Fatal(err)
	}
	if control&windows.SE_DACL_PROTECTED == 0 {
		t.Fatal("temporary retry file DACL is not protected")
	}
	dacl, _, err := descriptor.DACL()
	if err != nil {
		t.Fatal(err)
	}
	if dacl == nil {
		t.Fatal("temporary retry file has no DACL")
	}
	if dacl.AceCount != 1 {
		t.Fatalf("DACL ACE count = %d, want 1", dacl.AceCount)
	}
	var ace *windows.ACCESS_ALLOWED_ACE
	if err := windows.GetAce(dacl, 0, &ace); err != nil {
		t.Fatal(err)
	}
	if ace.Header.AceType != windows.ACCESS_ALLOWED_ACE_TYPE || ace.Header.AceFlags != 0 {
		t.Fatalf("unexpected ACE header: %#v", ace.Header)
	}
	if ace.Mask != windows.ACCESS_MASK(0x0013019f) {
		t.Fatalf("ACE mask = %#x", ace.Mask)
	}
	currentUser, err := windows.GetCurrentProcessToken().GetTokenUser()
	if err != nil {
		t.Fatal(err)
	}
	aceSID := (*windows.SID)(unsafe.Pointer(&ace.SidStart))
	if !aceSID.Equals(currentUser.User.Sid) {
		t.Fatalf("ACE SID %s does not match current user SID %s", aceSID.String(), currentUser.User.Sid.String())
	}
}

func TestUpdateRetryStatusCodesReturnsNormalizedValue(t *testing.T) {
	configPath := filepath.Join(t.TempDir(), "config.yaml")
	configData := []byte("listen_address: \"127.0.0.1:7780\"\nupstream_base_url: \"https://upstream.example/v1\"\nupstream_api_key: \"test-key\"\nretry_enabled: false\nretry_count: \"5\"\nretry_interval_seconds: \"1\"\nretry_status_codes: \"429\"\n")
	if err := os.WriteFile(configPath, configData, 0600); err != nil {
		t.Fatal(err)
	}
	config, err := loadConfig(configPath)
	if err != nil {
		t.Fatal(err)
	}
	gateway := &gateway{config: config, configPath: configPath}
	request := httptest.NewRequest(http.MethodPut, "/api/settings/retry_status_codes", strings.NewReader(`{"value":" 99,100-200,99,201-202,400-399,500-600 "}`))
	request.Header.Set("Content-Type", "application/json")
	recorder := httptest.NewRecorder()
	gateway.updateSetting(recorder, request)
	if recorder.Code != http.StatusOK {
		t.Fatalf("status=%d body=%s", recorder.Code, recorder.Body.String())
	}
	if !strings.Contains(recorder.Body.String(), `"value":"100-200,201-202"`) {
		t.Fatalf("response=%s", recorder.Body.String())
	}
	updated, err := os.ReadFile(configPath)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(updated), `retry_status_codes: "100-200,201-202"`) {
		t.Fatalf("config was not normalized: %s", updated)
	}
}

func TestUpdateUpstreamWebSocketEnabledPersistsImmediately(t *testing.T) {
	configPath := filepath.Join(t.TempDir(), "config.yaml")
	configData := []byte("listen_address: \"127.0.0.1:7780\"\nupstream_base_url: \"https://upstream.example/v1\"\nupstream_api_key: \"test-key\"\nupstream_websocket_enabled: true\n")
	if err := os.WriteFile(configPath, configData, 0600); err != nil {
		t.Fatal(err)
	}
	config, err := loadConfig(configPath)
	if err != nil {
		t.Fatal(err)
	}
	gateway := &gateway{config: config, configPath: configPath}
	request := httptest.NewRequest(http.MethodPut, "/api/settings/upstream_websocket_enabled", strings.NewReader(`{"value":"false"}`))
	request.Header.Set("Content-Type", "application/json")
	recorder := httptest.NewRecorder()
	gateway.updateSetting(recorder, request)
	if recorder.Code != http.StatusOK || gateway.currentConfig().UpstreamWebSocketEnabled {
		t.Fatalf("status=%d config=%#v", recorder.Code, gateway.currentConfig())
	}
	updated, err := loadConfig(configPath)
	if err != nil {
		t.Fatal(err)
	}
	if updated.UpstreamWebSocketEnabled {
		t.Fatalf("saved config = %#v", updated)
	}
}

func TestLoadConfigUsesRetryDefaultsForExistingConfig(t *testing.T) {
	configPath := filepath.Join(t.TempDir(), "config.yaml")
	configData := []byte("listen_address: \"127.0.0.1:7780\"\nupstream_base_url: \"https://upstream.example/v1\"\nupstream_api_key: \"test-key\"\n")
	if err := os.WriteFile(configPath, configData, 0600); err != nil {
		t.Fatal(err)
	}
	config, err := loadConfig(configPath)
	if err != nil {
		t.Fatal(err)
	}
	if config.RetryEnabled || !config.UpstreamWebSocketEnabled || config.RetryCount != defaultRetryCount || config.RetryIntervalSeconds != defaultRetryIntervalSeconds || config.RetryStatusCodes != defaultRetryStatusCodes {
		t.Fatalf("config defaults = %#v", config)
	}
}

func TestForwardReturnsOnlyFinalRetriedResponse(t *testing.T) {
	attempts := 0
	client := &http.Client{Transport: retryRoundTripperFunc(func(request *http.Request) (*http.Response, error) {
		attempts++
		body, err := io.ReadAll(request.Body)
		if err != nil {
			return nil, err
		}
		if string(body) != "payload" {
			return nil, errors.New("request body was not replayed")
		}
		if attempts < 3 {
			return retryResponse(http.StatusTooManyRequests, "intermediate", map[string]string{"X-Attempt": string(rune('0' + attempts))}), nil
		}
		return retryResponse(http.StatusOK, "final", map[string]string{"X-Attempt": "3"}), nil
	})}
	gateway := &gateway{
		config: Config{
			ListenAddress:        "127.0.0.1:7780",
			UpstreamBaseURL:      "https://upstream.example/v1",
			UpstreamAPIKey:       "test-key",
			RetryEnabled:         true,
			RetryCount:           "5",
			RetryIntervalSeconds: "0",
			RetryStatusCodes:     "429",
		},
		client:         client,
		proxyRunning:   true,
		retryBodyCache: newRetryBodyCache(t.TempDir()),
	}
	request := httptest.NewRequest(http.MethodPost, "http://localhost/v1/chat/completions", bytes.NewBufferString("payload"))
	recorder := httptest.NewRecorder()
	gateway.forward(recorder, request)
	if recorder.Code != http.StatusOK || recorder.Body.String() != "final" || recorder.Header().Get("X-Attempt") != "3" {
		t.Fatalf("status=%d body=%q headers=%#v", recorder.Code, recorder.Body.String(), recorder.Header())
	}
	if attempts != 3 {
		t.Fatalf("attempts=%d", attempts)
	}
	gateway.retryBodyCache.mu.Lock()
	memoryUsed := gateway.retryBodyCache.memoryUsed
	gateway.retryBodyCache.mu.Unlock()
	if memoryUsed != 0 {
		t.Fatalf("retry body cache still holds %d bytes after the final response", memoryUsed)
	}
}
