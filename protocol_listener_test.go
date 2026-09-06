package main

import (
	"context"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestValidateListenAddress(t *testing.T) {
	valid := []string{"127.0.0.1:57321", "0.0.0.0:57321", "192.168.1.10:443", "[::1]:57321", "[::]:57321", "[2001:db8::10]:443"}
	for _, address := range valid {
		if err := validateListenAddress(address); err != nil {
			t.Fatalf("validateListenAddress(%q) error = %v", address, err)
		}
	}
	invalid := []string{"localhost:57321", "http://127.0.0.1:57321", "127.0.0.1:0", "127.0.0.1:65536", "[127.0.0.1]:57321", "::1:57321", "127.0.0.1:57321,127.0.0.1:57322"}
	for _, address := range invalid {
		if err := validateListenAddress(address); err == nil {
			t.Fatalf("validateListenAddress(%q) returned nil error", address)
		}
	}
}

func TestUpstreamHostPort(t *testing.T) {
	cases := []struct{ input, host, port string }{
		{"api.example.test:9443", "api.example.test", "9443"},
		{"api.example.test", "api.example.test", "443"},
		{"203.0.113.9", "203.0.113.9", "443"},
		{"203.0.113.9:8443", "203.0.113.9", "8443"},
		{"[2001:db8::9]", "2001:db8::9", "443"},
		{"[2001:db8::9]:8443", "2001:db8::9", "8443"},
	}
	for _, testCase := range cases {
		host, port := upstreamHostPort(testCase.input)
		if host != testCase.host || port != testCase.port {
			t.Fatalf("upstreamHostPort(%q) = %q, %q", testCase.input, host, port)
		}
	}
}

func TestValidateUpstreamBaseURLRejectsUnbracketedIPv6(t *testing.T) {
	valid := []string{"https://api.example.test/v1", "https://203.0.113.9", "https://203.0.113.9:8443/v1", "https://[2001:db8::9]", "https://[2001:db8::9]:8443/v1"}
	for _, rawURL := range valid {
		if err := validateUpstreamBaseURL(rawURL); err != nil {
			t.Fatalf("validateUpstreamBaseURL(%q) error = %v", rawURL, err)
		}
	}
	if err := validateUpstreamBaseURL("https://2001:db8::9"); err == nil {
		t.Fatal("unbracketed IPv6 URL was accepted")
	}
	for _, rawURL := range []string{"https://api.example.test:", "https://api.example.test:0", "https://api.example.test:65536", "https://api.example.test:not-a-port", "http://api.example.test"} {
		if err := validateUpstreamBaseURL(rawURL); err == nil {
			t.Fatalf("invalid upstream URL was accepted: %q", rawURL)
		}
	}
}

func TestNormalizeUpstreamBaseURL(t *testing.T) {
	cases := []struct {
		input string
		want  string
	}{
		{"  https://api.example.test/v1/  ", "https://api.example.test/v1"},
		{"https://api.example.test///", "https://api.example.test"},
		{"https://api.example.test/v1/?query=/", "https://api.example.test/v1?query=/"},
		{"https://api.example.test", "https://api.example.test"},
		{"https://", "https://"},
	}
	for _, testCase := range cases {
		if got := normalizeUpstreamBaseURL(testCase.input); got != testCase.want {
			t.Errorf("normalizeUpstreamBaseURL(%q) = %q, want %q", testCase.input, got, testCase.want)
		}
	}
}

func TestUpdateSettingTrimsListenAddressAndAPIKey(t *testing.T) {
	configPath := filepath.Join(t.TempDir(), "config.yaml")
	configData := []byte("listen_address: \"127.0.0.1:7780\"\nupstream_base_url: \"https://upstream.example/v1\"\nupstream_api_key: \"initial-key\"\n")
	if err := os.WriteFile(configPath, configData, 0600); err != nil {
		t.Fatal(err)
	}
	config, err := loadConfig(configPath)
	if err != nil {
		t.Fatal(err)
	}
	gateway := &gateway{config: config, configPath: configPath}

	listenRequest := httptest.NewRequest(http.MethodPut, "/api/settings/listen_address", strings.NewReader(`{"value":" 127.0.0.1:7781 "}`))
	listenRecorder := httptest.NewRecorder()
	gateway.updateSetting(listenRecorder, listenRequest)
	if listenRecorder.Code != http.StatusOK {
		t.Fatalf("listen status=%d body=%s", listenRecorder.Code, listenRecorder.Body.String())
	}
	if got := gateway.currentConfig().ListenAddress; got != "127.0.0.1:7781" {
		t.Fatalf("listen address = %q", got)
	}

	keyRequest := httptest.NewRequest(http.MethodPut, "/api/settings/upstream_api_key", strings.NewReader(`{"value":"  trimmed-key  "}`))
	keyRecorder := httptest.NewRecorder()
	gateway.updateSetting(keyRecorder, keyRequest)
	if keyRecorder.Code != http.StatusOK {
		t.Fatalf("API key status=%d body=%s", keyRecorder.Code, keyRecorder.Body.String())
	}
	if got := gateway.currentConfig().UpstreamAPIKey; got != "trimmed-key" {
		t.Fatalf("upstream API key = %q", got)
	}
}

func TestHTTPOnlyListenerServesHTTPAndStops(t *testing.T) {
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	server := &http.Server{Handler: http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		if request.URL.Path != "/health" {
			http.NotFound(writer, request)
			return
		}
		writer.WriteHeader(http.StatusNoContent)
	})}
	done := make(chan struct{})
	go func() { _ = server.Serve(listener); close(done) }()
	response, err := http.Get("http://" + listener.Addr().String() + "/health")
	if err != nil {
		t.Fatal(err)
	}
	response.Body.Close()
	if response.StatusCode != http.StatusNoContent {
		t.Fatalf("HTTP status = %d", response.StatusCode)
	}
	connection, err := net.DialTimeout("tcp", listener.Addr().String(), time.Second)
	if err != nil {
		t.Fatal(err)
	}
	_, _ = connection.Write([]byte{0x05, 0x01, 0x00})
	_ = connection.SetReadDeadline(time.Now().Add(time.Second))
	data, _ := io.ReadAll(connection)
	_ = connection.Close()
	if strings.Contains(string(data), "\x05\x00") {
		t.Fatalf("HTTP-only listener returned a SOCKS response: %v", data)
	}
	shutdownContext, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	if err := server.Shutdown(shutdownContext); err != nil {
		t.Fatal(err)
	}
	select {
	case <-done:
	case <-time.After(time.Second):
		t.Fatal("HTTP server did not stop")
	}
}
