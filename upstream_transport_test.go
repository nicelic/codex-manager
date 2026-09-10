package main

import (
	"context"
	"errors"
	"io"
	"net"
	"net/http"
	"strings"
	"sync/atomic"
	"testing"
	"time"
)

func TestDefaultH3GracePeriod(t *testing.T) {
	if h3GracePeriod != 600*time.Millisecond {
		t.Fatalf("h3 grace period = %s, want 600ms", h3GracePeriod)
	}
}

func TestH3H2TransportPrefersH3DuringGracePeriod(t *testing.T) {
	h2Closed, h2Started, h3Started := make(chan struct{}), make(chan struct{}), make(chan struct{})
	transport := newTestTransport()
	transport.grace = 50 * time.Millisecond
	transport.lookupIP = func(_ context.Context, host string) (net.IP, error) {
		if host != "api.example.test" {
			t.Fatalf("lookup host = %q", host)
		}
		return net.ParseIP("203.0.113.9"), nil
	}
	transport.dialH2 = func(_ context.Context, host, address string) (*upstreamSession, error) {
		if host != "api.example.test" || address != "203.0.113.9:9443" {
			t.Fatalf("H2 dial = %s %s", host, address)
		}
		close(h2Started)
		return fakeUpstreamSession(upstreamProtocolH2, h2Closed), nil
	}
	transport.dialH3 = func(_ context.Context, host, address string) (*upstreamSession, error) {
		if host != "api.example.test" || address != "203.0.113.9:9443" {
			t.Fatalf("H3 dial = %s %s", host, address)
		}
		close(h3Started)
		time.Sleep(10 * time.Millisecond)
		return fakeUpstreamSession(upstreamProtocolH3, nil), nil
	}
	session, release, err := transport.sessionFor(context.Background(), "api.example.test:9443")
	if err != nil {
		t.Fatal(err)
	}
	defer release()
	if session.protocol != upstreamProtocolH3 {
		t.Fatalf("selected protocol = %q", session.protocol)
	}
	for _, started := range []chan struct{}{h2Started, h3Started} {
		select {
		case <-started:
		case <-time.After(time.Second):
			t.Fatal("handshake was not started")
		}
	}
	select {
	case <-h2Closed:
	case <-time.After(time.Second):
		t.Fatal("losing H2 session was not closed")
	}
	_ = transport.Close()
}

func TestH3H2TransportUsesH2WhenH3HandshakeFails(t *testing.T) {
	transport := newTestTransport()
	transport.lookupIP = func(context.Context, string) (net.IP, error) { return net.ParseIP("203.0.113.11"), nil }
	transport.dialH2 = func(context.Context, string, string) (*upstreamSession, error) {
		return fakeUpstreamSession(upstreamProtocolH2, nil), nil
	}
	transport.dialH3 = func(context.Context, string, string) (*upstreamSession, error) {
		return nil, errors.New("H3 unavailable")
	}
	session, release, err := transport.sessionFor(context.Background(), "api.example.test:443")
	if err != nil {
		t.Fatal(err)
	}
	defer release()
	if session.protocol != upstreamProtocolH2 {
		t.Fatalf("selected protocol = %q", session.protocol)
	}
	_ = transport.Close()
}

func TestH3H2TransportDirectIPv6SkipsDNS(t *testing.T) {
	transport := newTestTransport()
	transport.lookupIP = func(context.Context, string) (net.IP, error) {
		t.Fatal("literal IPv6 must not use DNS")
		return nil, nil
	}
	transport.dialH2 = func(_ context.Context, host, address string) (*upstreamSession, error) {
		if host != "2001:db8::9" || address != "[2001:db8::9]:443" {
			t.Fatalf("H2 dial = %q %q", host, address)
		}
		return fakeUpstreamSession(upstreamProtocolH2, nil), nil
	}
	transport.dialH3 = func(context.Context, string, string) (*upstreamSession, error) {
		return nil, errors.New("H3 unavailable")
	}
	session, release, err := transport.sessionFor(context.Background(), "[2001:db8::9]")
	if err != nil {
		t.Fatal(err)
	}
	if session.protocol != upstreamProtocolH2 {
		t.Fatalf("selected protocol = %q", session.protocol)
	}
	release()
	_ = transport.Close()
}

func TestH3H2TransportAddsConnectionAtStreamLimit(t *testing.T) {
	transport := newTestTransport()
	transport.maxStreams = 2
	transport.lookupIP = func(context.Context, string) (net.IP, error) { return net.ParseIP("203.0.113.20"), nil }
	var dials atomic.Int32
	transport.dialH2 = func(context.Context, string, string) (*upstreamSession, error) {
		return fakeUpstreamSession(upstreamProtocolH2, nil), nil
	}
	transport.dialH3 = func(context.Context, string, string) (*upstreamSession, error) {
		dials.Add(1)
		return fakeUpstreamSession(upstreamProtocolH3, nil), nil
	}
	first, firstRelease, err := transport.sessionFor(context.Background(), "api.example.test:443")
	if err != nil {
		t.Fatal(err)
	}
	second, secondRelease, err := transport.sessionFor(context.Background(), "api.example.test:443")
	if err != nil {
		t.Fatal(err)
	}
	third, thirdRelease, err := transport.sessionFor(context.Background(), "api.example.test:443")
	if err != nil {
		t.Fatal(err)
	}
	defer firstRelease()
	defer secondRelease()
	defer thirdRelease()
	if first != second || third != first {
		t.Fatal("full session should be temporarily reused while expansion is pending")
	}
	deadline := time.Now().Add(time.Second)
	for time.Now().Before(deadline) {
		transport.mu.Lock()
		count := len(transport.sessions["api.example.test:443"])
		transport.mu.Unlock()
		if count >= 2 {
			break
		}
		time.Sleep(time.Millisecond)
	}
	transport.mu.Lock()
	count := len(transport.sessions["api.example.test:443"])
	transport.mu.Unlock()
	if count < 2 || dials.Load() < 2 {
		t.Fatalf("background expansion did not create a second session: sessions=%d dials=%d", count, dials.Load())
	}
	_ = transport.Close()
}

func TestH3H2TransportOverflowsWhenExpansionFails(t *testing.T) {
	transport := newTestTransport()
	transport.maxStreams = 1
	transport.lookupIP = func(context.Context, string) (net.IP, error) { return net.ParseIP("203.0.113.21"), nil }
	var attempts atomic.Int32
	transport.dialH2 = func(context.Context, string, string) (*upstreamSession, error) {
		if attempts.Add(1) == 1 {
			return fakeUpstreamSession(upstreamProtocolH2, nil), nil
		}
		return nil, errors.New("new H2 session failed")
	}
	transport.dialH3 = func(context.Context, string, string) (*upstreamSession, error) {
		return nil, errors.New("H3 unavailable")
	}
	first, firstRelease, err := transport.sessionFor(context.Background(), "api.example.test:443")
	if err != nil {
		t.Fatal(err)
	}
	second, secondRelease, err := transport.sessionFor(context.Background(), "api.example.test:443")
	if err != nil {
		t.Fatal(err)
	}
	defer firstRelease()
	defer secondRelease()
	if first != second {
		t.Fatal("failed expansion should overflow the existing session")
	}
	_ = transport.Close()
}

func TestH3H2TransportReleasesStreamWhenResponseCloses(t *testing.T) {
	transport := newTestTransport()
	session := fakeUpstreamSession(upstreamProtocolH2, nil)
	session.roundTripper = roundTripperFunc(func(*http.Request) (*http.Response, error) {
		return &http.Response{StatusCode: http.StatusOK, Body: io.NopCloser(strings.NewReader("ok")), Header: make(http.Header)}, nil
	})
	transport.sessions["api.example.test:443"] = []*upstreamSession{session}
	request, _ := http.NewRequest(http.MethodGet, "https://api.example.test:443/v1/models", nil)
	response, err := transport.roundTripHTTP(request)
	if err != nil {
		t.Fatal(err)
	}
	if session.activeStreams != 1 {
		t.Fatalf("active streams = %d", session.activeStreams)
	}
	_ = response.Body.Close()
	if session.activeStreams != 0 {
		t.Fatalf("active streams = %d after close", session.activeStreams)
	}
	_ = transport.Close()
}

func TestH3H2TransportKeepsSharedSessionAfterRequestCancellation(t *testing.T) {
	transport := newTestTransport()
	session := fakeUpstreamSession(upstreamProtocolH2, nil)
	session.roundTripper = roundTripperFunc(func(*http.Request) (*http.Response, error) {
		return nil, context.Canceled
	})
	transport.sessions["api.example.test:443"] = []*upstreamSession{session}
	request, _ := http.NewRequest(http.MethodGet, "https://api.example.test:443/v1/models", nil)
	if _, err := transport.RoundTrip(request); !errors.Is(err, context.Canceled) {
		t.Fatalf("RoundTrip error = %v", err)
	}
	transport.mu.Lock()
	sessions := transport.sessions["api.example.test:443"]
	activeStreams := session.activeStreams
	transport.mu.Unlock()
	if len(sessions) != 1 || sessions[0] != session || activeStreams != 0 {
		t.Fatalf("canceled request removed or leaked shared session: sessions=%d active=%d", len(sessions), activeStreams)
	}
	_ = transport.Close()
}

func TestH3H2TransportExpandsWithoutBlockingNewStream(t *testing.T) {
	transport := newTestTransport()
	transport.maxStreams = 1
	transport.lookupIP = func(context.Context, string) (net.IP, error) { return net.ParseIP("203.0.113.24"), nil }
	var h2Dials atomic.Int32
	expansionStarted := make(chan struct{})
	allowExpansion := make(chan struct{})
	transport.dialH2 = func(ctx context.Context, _ string, _ string) (*upstreamSession, error) {
		if h2Dials.Add(1) == 1 {
			return fakeUpstreamSession(upstreamProtocolH2, nil), nil
		}
		close(expansionStarted)
		select {
		case <-allowExpansion:
			return fakeUpstreamSession(upstreamProtocolH2, nil), nil
		case <-ctx.Done():
			return nil, context.Cause(ctx)
		}
	}
	transport.dialH3 = func(context.Context, string, string) (*upstreamSession, error) {
		return nil, errors.New("H3 unavailable")
	}
	first, releaseFirst, err := transport.sessionFor(context.Background(), "api.example.test:443")
	if err != nil {
		t.Fatal(err)
	}
	defer releaseFirst()
	result := make(chan struct {
		session *upstreamSession
		release func()
		err     error
	}, 1)
	go func() {
		session, release, err := transport.sessionFor(context.Background(), "api.example.test:443")
		result <- struct {
			session *upstreamSession
			release func()
			err     error
		}{session, release, err}
	}()
	select {
	case <-expansionStarted:
	case <-time.After(time.Second):
		t.Fatal("background expansion did not start")
	}
	select {
	case second := <-result:
		if second.err != nil {
			t.Fatal(second.err)
		}
		if second.session != first {
			t.Fatal("new stream should temporarily reuse the full session")
		}
		second.release()
	case <-time.After(100 * time.Millisecond):
		t.Fatal("new stream waited for background expansion")
	}
	close(allowExpansion)
	_ = transport.Close()
}

func TestH3H2TransportMaintainsAndRotatesExpiredIdleSession(t *testing.T) {
	transport := newTestTransport()
	now := time.Now()
	transport.now = func() time.Time { return now }
	transport.sessionLifetime = time.Minute
	closed := make(chan struct{})
	session := fakeUpstreamSession(upstreamProtocolH2, closed)
	session.createdAt = now.Add(-time.Minute)
	transport.sessions["api.example.test:443"] = []*upstreamSession{session}
	transport.lookupIP = func(context.Context, string) (net.IP, error) { return net.ParseIP("203.0.113.22"), nil }
	transport.dialH2 = func(context.Context, string, string) (*upstreamSession, error) {
		return fakeUpstreamSession(upstreamProtocolH2, nil), nil
	}
	transport.dialH3 = func(context.Context, string, string) (*upstreamSession, error) {
		return nil, errors.New("H3 unavailable")
	}
	transport.maintainSessions(context.Background())
	select {
	case <-closed:
	case <-time.After(time.Second):
		t.Fatal("expired session was not closed")
	}
	_, release, err := transport.sessionFor(context.Background(), "api.example.test:443")
	if err != nil {
		t.Fatal(err)
	}
	release()
	_ = transport.Close()
}

func TestH3H2TransportDoesNotReuseExpiredSessionWithActiveWebSocket(t *testing.T) {
	transport := newTestTransport()
	now := time.Now()
	transport.now = func() time.Time { return now }
	transport.sessionLifetime = time.Minute

	oldSession := fakeUpstreamSession(upstreamProtocolH2, nil)
	oldSession.createdAt = now.Add(-time.Minute)
	oldSession.activeStreams = 1 // Simulates an existing long-lived WebSocket.
	transport.sessions["api.example.test:443"] = []*upstreamSession{oldSession}
	transport.lookupIP = func(context.Context, string) (net.IP, error) { return net.ParseIP("203.0.113.25"), nil }
	transport.dialH2 = func(context.Context, string, string) (*upstreamSession, error) {
		return fakeUpstreamSession(upstreamProtocolH2, nil), nil
	}
	transport.dialH3 = func(context.Context, string, string) (*upstreamSession, error) {
		return nil, errors.New("H3 unavailable")
	}

	transport.maintainSessions(context.Background())
	replacementReady := false
	deadline := time.Now().Add(time.Second)
	for time.Now().Before(deadline) {
		transport.mu.Lock()
		sessionCount := len(transport.sessions["api.example.test:443"])
		transport.mu.Unlock()
		if sessionCount == 2 {
			replacementReady = true
			break
		}
		time.Sleep(5 * time.Millisecond)
	}
	if !replacementReady {
		t.Fatal("expired active session did not start a replacement connection")
	}

	selected, release, err := transport.sessionFor(context.Background(), "api.example.test:443")
	if err != nil {
		t.Fatal(err)
	}
	if selected == oldSession {
		t.Fatal("expired session accepted a new stream while its WebSocket was active")
	}
	transport.mu.Lock()
	activeStreams := oldSession.activeStreams
	transport.mu.Unlock()
	if activeStreams != 1 {
		t.Fatalf("active streams = %d, want 1", activeStreams)
	}
	release()

	_ = transport.Close()
}

func TestH3H2TransportRemovesSessionWhenKeepAliveFails(t *testing.T) {
	transport := newTestTransport()
	closed := make(chan struct{})
	session := fakeUpstreamSession(upstreamProtocolH2, closed)
	session.ping = func(context.Context) error { return errors.New("lost connection") }
	transport.sessions["api.example.test:443"] = []*upstreamSession{session}
	transport.maintainSessions(context.Background())
	select {
	case <-closed:
	case <-time.After(time.Second):
		t.Fatal("failed keep-alive session was not closed")
	}
	_ = transport.Close()
}

func TestH3H2TransportCloseCancelsPendingHandshake(t *testing.T) {
	transport := newTestTransport()
	transport.lookupIP = func(context.Context, string) (net.IP, error) { return net.ParseIP("203.0.113.23"), nil }
	handshakeCanceled := make(chan struct{}, 2)
	dial := func(ctx context.Context, _ string, _ string) (*upstreamSession, error) {
		<-ctx.Done()
		handshakeCanceled <- struct{}{}
		return nil, context.Cause(ctx)
	}
	transport.dialH2 = dial
	transport.dialH3 = dial
	result := make(chan error, 1)
	go func() {
		_, release, err := transport.sessionFor(context.Background(), "api.example.test:443")
		if release != nil {
			release()
		}
		result <- err
	}()
	time.Sleep(10 * time.Millisecond)
	if err := transport.Close(); err != nil {
		t.Fatal(err)
	}
	for range 2 {
		select {
		case <-handshakeCanceled:
		case <-time.After(time.Second):
			t.Fatal("pending handshake was not canceled")
		}
	}
	select {
	case err := <-result:
		if !errors.Is(err, errH3H2TransportClosed) && !errors.Is(err, context.Canceled) {
			t.Fatalf("sessionFor error = %v", err)
		}
	case <-time.After(time.Second):
		t.Fatal("sessionFor did not return after close")
	}
}

func TestPrewarmUpstreamWithRetryRetriesFiveTimes(t *testing.T) {
	transport := newTestTransport()
	previousDelay := upstreamHandshakeRetryDelay
	upstreamHandshakeRetryDelay = time.Millisecond
	t.Cleanup(func() { upstreamHandshakeRetryDelay = previousDelay })
	var attempts atomic.Int32
	transport.lookupIP = func(context.Context, string) (net.IP, error) {
		attempts.Add(1)
		return nil, errors.New("DNS unavailable")
	}
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	err := prewarmUpstreamWithRetry(ctx, transport, "https://api.example.test")
	if err == nil || !strings.Contains(err.Error(), "5") {
		t.Fatalf("prewarm error = %v", err)
	}
	if attempts.Load() != upstreamHandshakeAttempts {
		t.Fatalf("attempts = %d, want %d", attempts.Load(), upstreamHandshakeAttempts)
	}
}

func TestH3H2TransportKeepsSingleConnectionUnderStreamLimit(t *testing.T) {
	transport := newTestTransport()
	transport.maxStreams = 500
	transport.lookupIP = func(context.Context, string) (net.IP, error) { return net.ParseIP("203.0.113.30"), nil }
	var dials atomic.Int32
	transport.dialH2 = func(context.Context, string, string) (*upstreamSession, error) {
		dials.Add(1)
		return fakeUpstreamSession(upstreamProtocolH2, nil), nil
	}
	transport.dialH3 = func(context.Context, string, string) (*upstreamSession, error) {
		return nil, errors.New("H3 unavailable")
	}

	err := transport.Prewarm(context.Background(), "https://api.example.test")
	if err != nil {
		t.Fatal(err)
	}
	if dials.Load() != 1 {
		t.Fatalf("expected 1 dial during prewarm, got %d", dials.Load())
	}

	first, firstRelease, err := transport.sessionFor(context.Background(), "api.example.test")
	if err != nil {
		t.Fatal(err)
	}
	defer firstRelease()

	second, secondRelease, err := transport.sessionFor(context.Background(), "api.example.test:443")
	if err != nil {
		t.Fatal(err)
	}
	defer secondRelease()

	third, thirdRelease, err := transport.sessionFor(context.Background(), "api.example.test")
	if err != nil {
		t.Fatal(err)
	}
	defer thirdRelease()

	if first != second || second != third {
		t.Fatalf("sessions are different under stream limit: first=%p second=%p third=%p", first, second, third)
	}

	status := transport.connectionStatusSnapshot()
	if status.h2Connections != 1 {
		t.Fatalf("expected exactly 1 H2 connection, got %d", status.h2Connections)
	}
	if status.activeStreams != 3 {
		t.Fatalf("expected 3 active streams, got %d", status.activeStreams)
	}
	_ = transport.Close()
}

func TestH3H2TransportPrunesRedundantIdleConnectionsOnMaintain(t *testing.T) {
	transport := newTestTransport()
	transport.maxStreams = 500
	sessionActive := fakeUpstreamSession(upstreamProtocolH2, nil)
	sessionActive.activeStreams = 2
	sessionIdle := fakeUpstreamSession(upstreamProtocolH2, nil)
	sessionIdle.activeStreams = 0

	transport.sessions["api.example.test:443"] = []*upstreamSession{sessionActive, sessionIdle}
	transport.maintainSessions(context.Background())

	transport.mu.Lock()
	sessions := transport.sessions["api.example.test:443"]
	count := len(sessions)
	transport.mu.Unlock()

	if count != 1 || sessions[0] != sessionActive {
		t.Fatalf("redundant idle session was not pruned: count=%d", count)
	}
	_ = transport.Close()
}

func newTestTransport() *h3H2Transport {
	transport := newH3H2Transport()
	transport.keepAliveInterval = time.Hour
	return transport
}

func fakeUpstreamSession(protocol upstreamProtocol, closed chan<- struct{}) *upstreamSession {
	return &upstreamSession{protocol: protocol, available: func() bool { return true }, createdAt: time.Now(), close: func() {
		if closed != nil {
			close(closed)
		}
	}}
}

type roundTripperFunc func(*http.Request) (*http.Response, error)

func (fn roundTripperFunc) RoundTrip(request *http.Request) (*http.Response, error) {
	return fn(request)
}
