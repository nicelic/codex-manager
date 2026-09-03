package main

import (
	"bytes"
	"context"
	"encoding/pem"
	"errors"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"path/filepath"
	"sync"
	"testing"
	"time"
)

type testTunnelProxy struct {
	server       *httptest.Server
	mu           sync.Mutex
	connectHosts []string
	nonConnect   []string
}

type idleCloseTrackingTransport struct {
	closeIdleCalls int
}

func (transport *idleCloseTrackingTransport) RoundTrip(*http.Request) (*http.Response, error) {
	return nil, errors.New("unexpected request")
}

func (transport *idleCloseTrackingTransport) CloseIdleConnections() {
	transport.closeIdleCalls++
}

func newTestTunnelProxy(t *testing.T) *testTunnelProxy {
	t.Helper()
	proxy := &testTunnelProxy{}
	proxy.server = httptest.NewServer(http.HandlerFunc(proxy.handle))
	t.Cleanup(proxy.server.Close)
	return proxy
}

func (proxy *testTunnelProxy) handle(w http.ResponseWriter, request *http.Request) {
	if request.Method != http.MethodConnect {
		proxy.mu.Lock()
		proxy.nonConnect = append(proxy.nonConnect, request.Method+" "+request.URL.String())
		proxy.mu.Unlock()
		http.Error(w, "expected CONNECT", http.StatusBadRequest)
		return
	}
	proxy.mu.Lock()
	proxy.connectHosts = append(proxy.connectHosts, request.Host)
	proxy.mu.Unlock()

	upstream, err := net.Dial("tcp", request.Host)
	if err != nil {
		http.Error(w, err.Error(), http.StatusBadGateway)
		return
	}
	hijacker, ok := w.(http.Hijacker)
	if !ok {
		_ = upstream.Close()
		http.Error(w, "proxy cannot tunnel", http.StatusInternalServerError)
		return
	}
	client, _, err := hijacker.Hijack()
	if err != nil {
		_ = upstream.Close()
		return
	}
	if _, err := client.Write([]byte("HTTP/1.1 200 Connection Established\r\n\r\n")); err != nil {
		_ = client.Close()
		_ = upstream.Close()
		return
	}
	go func() {
		defer client.Close()
		defer upstream.Close()
		go func() {
			_, _ = io.Copy(upstream, client)
			_ = upstream.Close()
		}()
		_, _ = io.Copy(client, upstream)
	}()
}

func (proxy *testTunnelProxy) snapshot() ([]string, []string) {
	proxy.mu.Lock()
	defer proxy.mu.Unlock()
	return append([]string(nil), proxy.connectHosts...), append([]string(nil), proxy.nonConnect...)
}

func testLLMTrimProxyClient(t *testing.T, proxyURL string, upstream *httptest.Server) *http.Client {
	t.Helper()
	certificate := upstream.Certificate()
	if certificate == nil {
		t.Fatal("TLS test server did not expose a certificate")
	}
	caPEM := pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: certificate.Raw})
	caPath := filepath.Join(t.TempDir(), "ca.pem")
	if err := os.WriteFile(caPath, caPEM, 0600); err != nil {
		t.Fatal(err)
	}
	client, err := newLLMTrimProxyClientWithCA(proxyURL, caPath)
	if err != nil {
		t.Fatal(err)
	}
	return client
}

func testGatewayForLLMTrimProxy(t *testing.T, upstreamBaseURL string, baseline, llmtrimClient *http.Client, running bool) *gateway {
	t.Helper()
	return &gateway{
		config: Config{
			ListenAddress:            "127.0.0.1:7780",
			UpstreamBaseURL:          upstreamBaseURL,
			UpstreamAPIKey:           "upstream-key",
			LLMTrimPath:              `C:\\tools\\llmtrim.exe`,
			UpstreamWebSocketEnabled: true,
			RetryCount:               "5",
			RetryIntervalSeconds:     "0",
			RetryStatusCodes:         "429",
		},
		client:              baseline,
		llmtrimProxyClient:  llmtrimClient,
		llmtrimRunningCheck: func(string) bool { return running },
		proxyRunning:        true,
		retryBodyCache:      newRetryBodyCache(t.TempDir()),
	}
}

func TestForwardUsesLLMTrimHTTPProxyForAllMethods(t *testing.T) {
	var mu sync.Mutex
	var methods []string
	var bodies []string
	upstream := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, request *http.Request) {
		if request.Header.Get("Authorization") != "Bearer upstream-key" {
			t.Errorf("upstream Authorization = %q", request.Header.Get("Authorization"))
		}
		body, err := io.ReadAll(request.Body)
		if err != nil {
			t.Error(err)
			return
		}
		mu.Lock()
		methods = append(methods, request.Method+" "+request.URL.Path)
		bodies = append(bodies, string(body))
		mu.Unlock()
		w.Header().Set("X-Upstream", "seen")
		_, _ = io.WriteString(w, "upstream")
	}))
	defer upstream.Close()
	proxy := newTestTunnelProxy(t)
	baseline := &http.Client{Transport: retryRoundTripperFunc(func(*http.Request) (*http.Response, error) {
		t.Fatal("baseline client must not be used while llmtrim is running")
		return nil, errors.New("unexpected baseline request")
	})}
	gateway := testGatewayForLLMTrimProxy(t, upstream.URL+"/v1", baseline, testLLMTrimProxyClient(t, proxy.server.URL, upstream), true)

	requests := []struct {
		method string
		path   string
		body   string
	}{
		{method: http.MethodGet, path: "/v1/models"},
		{method: http.MethodHead, path: "/v1/models"},
		{method: http.MethodPost, path: "/v1/responses", body: `{"model":"test","input":"hello"}`},
	}
	for _, test := range requests {
		request := httptest.NewRequest(test.method, "http://localhost"+test.path, bytes.NewBufferString(test.body))
		recorder := httptest.NewRecorder()
		gateway.forward(recorder, request)
		if recorder.Code != http.StatusOK {
			t.Fatalf("%s status=%d body=%q", test.method, recorder.Code, recorder.Body.String())
		}
	}

	connectHosts, nonConnect := proxy.snapshot()
	if len(connectHosts) == 0 {
		t.Fatal("llmtrim proxy did not receive a CONNECT request")
	}
	if len(nonConnect) != 0 {
		t.Fatalf("proxy received non-CONNECT requests: %#v", nonConnect)
	}
	upstreamURL, err := url.Parse(upstream.URL)
	if err != nil {
		t.Fatal(err)
	}
	for _, connectHost := range connectHosts {
		if connectHost != upstreamURL.Host {
			t.Fatalf("CONNECT target = %q, want real upstream %q", connectHost, upstreamURL.Host)
		}
	}
	mu.Lock()
	gotMethods := append([]string(nil), methods...)
	gotBodies := append([]string(nil), bodies...)
	mu.Unlock()
	if len(gotMethods) != 3 || gotMethods[0] != "GET /v1/models" || gotMethods[1] != "HEAD /v1/models" || gotMethods[2] != "POST /v1/responses" {
		t.Fatalf("upstream methods = %#v", gotMethods)
	}
	if gotBodies[2] != `{"model":"test","input":"hello"}` {
		t.Fatalf("upstream POST body = %q", gotBodies[2])
	}
}

func TestForwardUsesBaselineClientWhenLLMTrimIsStopped(t *testing.T) {
	baselineCalls := 0
	baseline := &http.Client{Transport: retryRoundTripperFunc(func(request *http.Request) (*http.Response, error) {
		baselineCalls++
		body, err := io.ReadAll(request.Body)
		if err != nil {
			return nil, err
		}
		if string(body) != `{"prompt":"raw"}` {
			return nil, errors.New("baseline received the wrong request body")
		}
		return retryResponse(http.StatusOK, "baseline", nil), nil
	})}
	llmtrim := &http.Client{Transport: retryRoundTripperFunc(func(*http.Request) (*http.Response, error) {
		t.Fatal("llmtrim proxy client must not be used when stopped")
		return nil, errors.New("unexpected llmtrim request")
	})}
	gateway := testGatewayForLLMTrimProxy(t, "https://upstream.example/v1", baseline, llmtrim, false)
	recorder := httptest.NewRecorder()
	gateway.forward(recorder, httptest.NewRequest(http.MethodPost, "http://localhost/v1/responses", bytes.NewBufferString(`{"prompt":"raw"}`)))
	if recorder.Code != http.StatusOK || recorder.Body.String() != "baseline" || baselineCalls != 1 {
		t.Fatalf("status=%d body=%q baselineCalls=%d", recorder.Code, recorder.Body.String(), baselineCalls)
	}
}

func TestForwardDoesNotFallBackWhenLLMTrimProxyIsUnavailable(t *testing.T) {
	running := true
	baselineCalls := 0
	baseline := &http.Client{Transport: retryRoundTripperFunc(func(*http.Request) (*http.Response, error) {
		baselineCalls++
		return retryResponse(http.StatusOK, "baseline", nil), nil
	})}
	gateway := testGatewayForLLMTrimProxy(t, "https://upstream.example/v1", baseline, nil, true)
	gateway.llmtrimRunningCheck = func(string) bool { return running }
	gateway.llmtrimProxyFactory = func() (*http.Client, error) {
		return nil, errors.New("llmtrim CA is missing")
	}

	first := httptest.NewRecorder()
	gateway.forward(first, httptest.NewRequest(http.MethodGet, "http://localhost/v1/models", nil))
	if first.Code != http.StatusBadGateway || baselineCalls != 0 {
		t.Fatalf("status=%d baselineCalls=%d", first.Code, baselineCalls)
	}
	running = false
	second := httptest.NewRecorder()
	gateway.forward(second, httptest.NewRequest(http.MethodGet, "http://localhost/v1/models", nil))
	if second.Code != http.StatusOK || baselineCalls != 1 {
		t.Fatalf("status=%d baselineCalls=%d", second.Code, baselineCalls)
	}
}

func TestForwardUsesBaselineWhenLLMTrimIsNotFullyRunning(t *testing.T) {
	baselineCalls := 0
	baseline := &http.Client{Transport: retryRoundTripperFunc(func(*http.Request) (*http.Response, error) {
		baselineCalls++
		return retryResponse(http.StatusOK, "baseline", nil), nil
	})}
	gateway := testGatewayForLLMTrimProxy(t, "https://upstream.example/v1", baseline, nil, false)
	gateway.llmtrimRunningCheck = nil

	recorder := httptest.NewRecorder()
	gateway.forward(recorder, httptest.NewRequest(http.MethodPost, "http://localhost/v1/responses", bytes.NewBufferString(`{"model":"test"}`)))
	if recorder.Code != http.StatusOK || recorder.Body.String() != "baseline" || baselineCalls != 1 {
		t.Fatalf("status=%d baselineCalls=%d", recorder.Code, baselineCalls)
	}
}

func TestForwardDoesNotFallBackWhenLLMTrimProxyConnectionFails(t *testing.T) {
	var upstreamCalls int
	upstream := httptest.NewTLSServer(http.HandlerFunc(func(http.ResponseWriter, *http.Request) {
		upstreamCalls++
	}))
	defer upstream.Close()
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	proxyURL := "http://" + listener.Addr().String()
	if err := listener.Close(); err != nil {
		t.Fatal(err)
	}
	baselineCalls := 0
	baseline := &http.Client{Transport: retryRoundTripperFunc(func(*http.Request) (*http.Response, error) {
		baselineCalls++
		return retryResponse(http.StatusOK, "baseline", nil), nil
	})}
	gateway := testGatewayForLLMTrimProxy(t, upstream.URL+"/v1", baseline, testLLMTrimProxyClient(t, proxyURL, upstream), true)
	recorder := httptest.NewRecorder()
	gateway.forward(recorder, httptest.NewRequest(http.MethodPost, "http://localhost/v1/responses", bytes.NewBufferString(`{"model":"test"}`)))
	if recorder.Code != http.StatusBadGateway {
		t.Fatalf("status=%d body=%q", recorder.Code, recorder.Body.String())
	}
	if baselineCalls != 0 || upstreamCalls != 0 {
		t.Fatalf("baselineCalls=%d upstreamCalls=%d", baselineCalls, upstreamCalls)
	}
}

func TestForwardRetriesRawBodyThroughLLMTrimProxy(t *testing.T) {
	var mu sync.Mutex
	upstreamCalls := 0
	var bodies []string
	upstream := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, request *http.Request) {
		body, err := io.ReadAll(request.Body)
		if err != nil {
			t.Error(err)
			return
		}
		mu.Lock()
		upstreamCalls++
		bodies = append(bodies, string(body))
		call := upstreamCalls
		mu.Unlock()
		if call == 1 {
			w.WriteHeader(http.StatusTooManyRequests)
			_, _ = io.WriteString(w, "retry")
			return
		}
		_, _ = io.WriteString(w, "final")
	}))
	defer upstream.Close()
	proxy := newTestTunnelProxy(t)
	baseline := &http.Client{Transport: retryRoundTripperFunc(func(*http.Request) (*http.Response, error) {
		t.Fatal("baseline client must not be used while llmtrim is running")
		return nil, errors.New("unexpected baseline request")
	})}
	gateway := testGatewayForLLMTrimProxy(t, upstream.URL+"/v1", baseline, testLLMTrimProxyClient(t, proxy.server.URL, upstream), true)
	gateway.config.RetryEnabled = true
	gateway.config.RetryCount = "1"
	recorder := httptest.NewRecorder()
	gateway.forward(recorder, httptest.NewRequest(http.MethodPost, "http://localhost/v1/responses", bytes.NewBufferString(`{"model":"test","input":"raw"}`)))
	if recorder.Code != http.StatusOK || recorder.Body.String() != "final" {
		t.Fatalf("status=%d body=%q", recorder.Code, recorder.Body.String())
	}
	mu.Lock()
	gotCalls := upstreamCalls
	gotBodies := append([]string(nil), bodies...)
	mu.Unlock()
	if gotCalls != 2 || len(gotBodies) != 2 || gotBodies[0] != `{"model":"test","input":"raw"}` || gotBodies[1] != `{"model":"test","input":"raw"}` {
		t.Fatalf("upstream calls=%d bodies=%#v", gotCalls, gotBodies)
	}
	connectHosts, nonConnect := proxy.snapshot()
	if len(connectHosts) == 0 || len(nonConnect) != 0 {
		t.Fatalf("connect=%#v nonConnect=%#v", connectHosts, nonConnect)
	}
}

func TestNewLLMTrimProxyClientRejectsMissingOrInvalidCA(t *testing.T) {
	if _, err := newLLMTrimProxyClientWithCA("http://127.0.0.1:43117", filepath.Join(t.TempDir(), "missing.pem")); err == nil {
		t.Fatal("missing llmtrim CA must fail")
	}
	invalidPath := filepath.Join(t.TempDir(), "invalid.pem")
	if err := os.WriteFile(invalidPath, []byte("not a certificate"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := newLLMTrimProxyClientWithCA("http://127.0.0.1:43117", invalidPath); err == nil {
		t.Fatal("invalid llmtrim CA must fail")
	}
}

func TestNewLLMTrimProxyClientUsesExplicitProxyAndFiveMinuteIdlePool(t *testing.T) {
	upstream := httptest.NewTLSServer(http.NotFoundHandler())
	defer upstream.Close()
	certificate := upstream.Certificate()
	if certificate == nil {
		t.Fatal("TLS test server did not expose a certificate")
	}
	userProfile := t.TempDir()
	caDirectory := filepath.Join(userProfile, ".llmtrim")
	if err := os.MkdirAll(caDirectory, 0700); err != nil {
		t.Fatal(err)
	}
	caPEM := pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: certificate.Raw})
	if err := os.WriteFile(filepath.Join(caDirectory, "ca.pem"), caPEM, 0600); err != nil {
		t.Fatal(err)
	}
	t.Setenv("USERPROFILE", userProfile)
	t.Setenv("HTTP_PROXY", "http://127.0.0.1:1")
	t.Setenv("HTTPS_PROXY", "http://127.0.0.1:1")
	client, err := newLLMTrimProxyClient()
	if err != nil {
		t.Fatal(err)
	}
	transport, ok := client.Transport.(*http.Transport)
	if !ok {
		t.Fatalf("transport type = %T", client.Transport)
	}
	request := httptest.NewRequest(http.MethodGet, "https://upstream.example/v1/models", nil)
	proxyURL, err := transport.Proxy(request)
	if err != nil {
		t.Fatal(err)
	}
	if got := proxyURL.String(); got != llmtrimProxyURL {
		t.Fatalf("proxy = %q, want %q", got, llmtrimProxyURL)
	}
	if transport.IdleConnTimeout != 5*time.Minute {
		t.Fatalf("IdleConnTimeout = %s, want 5m", transport.IdleConnTimeout)
	}
	if roots := transport.TLSClientConfig.RootCAs; roots == nil || len(roots.Subjects()) != 1 {
		t.Fatalf("llmtrim client must trust only the dedicated CA pool, roots=%d", len(roots.Subjects()))
	}
}

func TestLLMTrimProxyClientReusesTunnelAndReconnectsAfterClose(t *testing.T) {
	upstreamRequests := 0
	upstream := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, request *http.Request) {
		upstreamRequests++
		if upstreamRequests == 3 {
			w.Header().Set("Connection", "close")
		}
		_, _ = io.WriteString(w, request.URL.Path)
	}))
	defer upstream.Close()
	proxy := newTestTunnelProxy(t)
	client := testLLMTrimProxyClient(t, proxy.server.URL, upstream)
	defer closeHTTPClientTransport(client)

	for requestNumber := 1; requestNumber <= 4; requestNumber++ {
		response, err := client.Get(upstream.URL + "/v1/models")
		if err != nil {
			t.Fatalf("request %d failed: %v", requestNumber, err)
		}
		if _, err := io.Copy(io.Discard, response.Body); err != nil {
			_ = response.Body.Close()
			t.Fatalf("read response %d: %v", requestNumber, err)
		}
		if err := response.Body.Close(); err != nil {
			t.Fatalf("close response %d: %v", requestNumber, err)
		}
	}

	connectHosts, nonConnect := proxy.snapshot()
	if len(nonConnect) != 0 {
		t.Fatalf("proxy received non-CONNECT requests: %#v", nonConnect)
	}
	if len(connectHosts) != 2 {
		t.Fatalf("CONNECT count = %d, want 2 (reuse then reconnect)", len(connectHosts))
	}
}

func TestCloseLLMTrimProxyClientIdleConnectionsDiscardsClient(t *testing.T) {
	transport := &idleCloseTrackingTransport{}
	gateway := &gateway{llmtrimProxyClient: &http.Client{Transport: transport}}
	gateway.closeLLMTrimProxyClientIdleConnections()
	if transport.closeIdleCalls != 1 {
		t.Fatalf("CloseIdleConnections calls = %d, want 1", transport.closeIdleCalls)
	}
	if gateway.llmtrimProxyClient != nil {
		t.Fatal("llmtrim proxy client was not discarded")
	}
}

func TestStopProxyDiscardsLLMTrimProxyClient(t *testing.T) {
	transport := &idleCloseTrackingTransport{}
	gateway := &gateway{llmtrimProxyClient: &http.Client{Transport: transport}}
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	if err := gateway.stopProxy(ctx); err != nil {
		t.Fatal(err)
	}
	if transport.closeIdleCalls != 1 {
		t.Fatalf("CloseIdleConnections calls = %d, want 1", transport.closeIdleCalls)
	}
	if gateway.llmtrimProxyClient != nil {
		t.Fatal("llmtrim proxy client was not discarded after gateway stop")
	}
}

func TestStartLLMTrimDiscardsStaleProxyClientBeforeSetup(t *testing.T) {
	transport := &idleCloseTrackingTransport{}
	gateway := &gateway{
		configPath:         filepath.Join(t.TempDir(), "missing", "config.yaml"),
		llmtrimProxyClient: &http.Client{Transport: transport},
	}
	if err := gateway.startLLMTrimWithPath(context.Background(), `C:\\tools\\llmtrim.exe`); err == nil {
		t.Fatal("start must fail after the intentionally invalid config save")
	}
	if transport.closeIdleCalls != 1 {
		t.Fatalf("CloseIdleConnections calls = %d, want 1", transport.closeIdleCalls)
	}
	if gateway.llmtrimProxyClient != nil {
		t.Fatal("stale llmtrim proxy client was not discarded before setup")
	}
}
