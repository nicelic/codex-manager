package main

import (
	"bufio"
	"bytes"
	"context"
	"crypto/tls"
	"crypto/x509"
	"encoding/pem"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/gobwas/ws"
	"github.com/gobwas/ws/wsutil"
	"github.com/quic-go/quic-go"
	"github.com/quic-go/quic-go/http3"
	"github.com/quic-go/quic-go/quicvarint"
	"golang.org/x/net/http2"
	"golang.org/x/net/http2/hpack"
)

type websocketToggleTransport struct {
	openCalls int
}

func (transport *websocketToggleTransport) RoundTrip(request *http.Request) (*http.Response, error) {
	if shouldTryHTTPOverWebSocket(request) {
		return nil, errors.New("disabled websocket carrier was still attempted")
	}
	return &http.Response{StatusCode: http.StatusOK, Header: make(http.Header), Body: io.NopCloser(strings.NewReader("ordinary"))}, nil
}

func (transport *websocketToggleTransport) OpenWebSocket(context.Context, *url.URL, http.Header) (*webSocketStream, *http.Response, error) {
	transport.openCalls++
	return nil, nil, errors.New("OpenWebSocket must not run while the switch is off")
}

func TestValidateWebSocketUpgrade(t *testing.T) {
	request := httptest.NewRequest(http.MethodGet, "http://gateway.test/v1/chat", nil)
	request.Header.Set("Connection", "keep-alive, Upgrade")
	request.Header.Set("Upgrade", "websocket")
	request.Header.Set("Sec-WebSocket-Version", "13")
	request.Header.Set("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
	key, err := validateWebSocketUpgrade(request)
	if err != nil {
		t.Fatalf("validateWebSocketUpgrade() error = %v", err)
	}
	if key != "dGhlIHNhbXBsZSBub25jZQ==" {
		t.Fatalf("key = %q", key)
	}

	invalid := request.Clone(context.Background())
	invalid.Method = http.MethodPost
	if _, err := validateWebSocketUpgrade(invalid); err == nil {
		t.Fatal("POST websocket upgrade was accepted")
	}
	invalid = request.Clone(context.Background())
	invalid.Header.Set("Sec-WebSocket-Key", "not-base64")
	if _, err := validateWebSocketUpgrade(invalid); err == nil {
		t.Fatal("invalid websocket key was accepted")
	}
	invalid = request.Clone(context.Background())
	invalid.Header.Set("Sec-WebSocket-Version", "12")
	if _, err := validateWebSocketUpgrade(invalid); err == nil {
		t.Fatal("unsupported websocket version was accepted")
	}
}

func TestWriteWebSocketHandshake(t *testing.T) {
	var output bytes.Buffer
	writer := bufio.NewReadWriter(bufio.NewReader(&output), bufio.NewWriter(&output))
	key := "dGhlIHNhbXBsZSBub25jZQ=="
	if err := writeWebSocketHandshake(writer, key, http.Header{"Sec-WebSocket-Protocol": {"chat"}, "X-Upstream": {"ready"}}, ""); err != nil {
		t.Fatal(err)
	}
	response, err := http.ReadResponse(bufio.NewReader(&output), &http.Request{Method: http.MethodGet})
	if err != nil {
		t.Fatal(err)
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusSwitchingProtocols {
		t.Fatalf("status = %d", response.StatusCode)
	}
	if got := response.Header.Get("Sec-WebSocket-Accept"); got != webSocketAccept(key) {
		t.Fatalf("accept = %q", got)
	}
	if got := response.Header.Get("Sec-WebSocket-Protocol"); got != "chat" {
		t.Fatalf("subprotocol = %q", got)
	}
	if got := response.Header.Get("X-Upstream"); got != "ready" {
		t.Fatalf("X-Upstream = %q", got)
	}
}

func TestProxyWebSocketStreamPreservesRawFrames(t *testing.T) {
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()

	upstreamInput, gatewayOutput := io.Pipe()
	gatewayInput, upstreamOutput := io.Pipe()
	defer upstreamInput.Close()
	defer upstreamOutput.Close()
	gateway := &gateway{}
	handlerDone := make(chan struct{})
	server := &http.Server{Handler: http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		key, err := validateWebSocketUpgrade(request)
		if err != nil {
			http.Error(writer, err.Error(), http.StatusBadRequest)
			close(handlerDone)
			return
		}
		stream := &webSocketStream{
			reader:  gatewayInput,
			writer:  gatewayOutput,
			headers: http.Header{"Sec-WebSocket-Protocol": {"chat"}},
			closeFn: func() {
				_ = gatewayInput.Close()
				_ = gatewayOutput.Close()
				_ = upstreamOutput.Close()
			},
		}
		finishLocalHTTP1Request := gateway.beginLocalHTTP1Request()
		gateway.proxyWebSocketStream(writer, key, stream, finishLocalHTTP1Request)
		close(handlerDone)
	})}
	go func() { _ = server.Serve(listener) }()
	defer server.Close()

	connection, err := net.DialTimeout("tcp", listener.Addr().String(), time.Second)
	if err != nil {
		t.Fatal(err)
	}
	defer connection.Close()
	key := "dGhlIHNhbXBsZSBub25jZQ=="
	if _, err := fmt.Fprintf(connection, "GET /v1/chat HTTP/1.1\r\nHost: %s\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: %s\r\n\r\n", listener.Addr().String(), key); err != nil {
		t.Fatal(err)
	}
	response, err := http.ReadResponse(bufio.NewReader(connection), &http.Request{Method: http.MethodGet})
	if err != nil {
		t.Fatal(err)
	}
	if response.StatusCode != http.StatusSwitchingProtocols {
		t.Fatalf("handshake status = %d", response.StatusCode)
	}
	status := gateway.connectionStatusSnapshot()
	if status.LocalHTTP1 != 0 || status.LocalWebSocket != 1 {
		t.Fatalf("local status after upgrade = %#v", status)
	}
	if got := response.Header.Get("Sec-WebSocket-Protocol"); got != "chat" {
		t.Fatalf("subprotocol = %q", got)
	}
	_ = response.Body.Close()

	clientFrame := []byte{0x82, 0x82, 0x01, 0x02, 0x03, 0x04, 'o' ^ 0x01, 'k' ^ 0x02}
	if _, err := connection.Write(clientFrame); err != nil {
		t.Fatal(err)
	}
	received := make([]byte, len(clientFrame))
	if _, err := io.ReadFull(upstreamInput, received); err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(received, clientFrame) {
		t.Fatalf("client frame was changed: %x != %x", received, clientFrame)
	}

	upstreamFrame := []byte{0x82, 0x02, 'o', 'k'}
	if _, err := upstreamOutput.Write(upstreamFrame); err != nil {
		t.Fatal(err)
	}
	received = make([]byte, len(upstreamFrame))
	if _, err := io.ReadFull(connection, received); err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(received, upstreamFrame) {
		t.Fatalf("upstream frame was changed: %x != %x", received, upstreamFrame)
	}
	closeFrame := []byte{0x88, 0x02, 0x03, 0xe8}
	if _, err := upstreamOutput.Write(closeFrame); err != nil {
		t.Fatal(err)
	}
	received = make([]byte, len(closeFrame))
	if _, err := io.ReadFull(connection, received); err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(received, closeFrame) {
		t.Fatalf("close frame was changed: %x != %x", received, closeFrame)
	}

	_ = connection.Close()
	select {
	case <-handlerDone:
	case <-time.After(time.Second):
		gateway.closeWebSocketSessions()
		t.Fatal("hijacked websocket session did not stop")
	}
	status = gateway.connectionStatusSnapshot()
	if status.LocalHTTP1 != 0 || status.LocalWebSocket != 0 {
		t.Fatalf("local status after websocket close = %#v", status)
	}
}

func TestInvalidWebSocketUpgradeDoesNotRegisterLocalWebSocket(t *testing.T) {
	gateway := &gateway{}
	request := httptest.NewRequest(http.MethodGet, "http://gateway.test/v1/realtime", nil)
	request.Header.Set("Connection", "Upgrade")
	request.Header.Set("Upgrade", "websocket")
	request.Header.Set("Sec-WebSocket-Version", "13")
	request.Header.Set("Sec-WebSocket-Key", "invalid")
	recorder := httptest.NewRecorder()

	gateway.forward(recorder, request)

	if recorder.Code != http.StatusBadRequest {
		t.Fatalf("status = %d, want %d", recorder.Code, http.StatusBadRequest)
	}
	status := gateway.connectionStatusSnapshot()
	if status.LocalHTTP1 != 0 || status.LocalWebSocket != 0 {
		t.Fatalf("invalid upgrade changed local websocket status: %#v", status)
	}
}

func TestStopProxyClosesHijackedWebSocketSessions(t *testing.T) {
	gateway := &gateway{}
	closed := make(chan struct{})
	session := &webSocketSession{closeFn: func() { close(closed) }}
	gateway.registerWebSocketSession(session)

	if err := gateway.stopProxy(context.Background()); err != nil {
		t.Fatal(err)
	}
	select {
	case <-closed:
	case <-time.After(time.Second):
		t.Fatal("stopProxy did not close the hijacked websocket session")
	}
}

func TestExtendedConnectRequestForms(t *testing.T) {
	for _, protocol := range []upstreamProtocol{upstreamProtocolH2, upstreamProtocolH3} {
		t.Run(string(protocol), func(t *testing.T) {
			var captured *http.Request
			transport := newTestTransport()
			transport.sessions["upstream.test"] = []*upstreamSession{{
				protocol: protocol,
				available: func() bool {
					return true
				},
				createdAt: time.Now(),
				roundTripper: roundTripperFunc(func(request *http.Request) (*http.Response, error) {
					copyRequest := request.Clone(request.Context())
					copyRequest.Header = request.Header.Clone()
					captured = copyRequest
					return &http.Response{StatusCode: http.StatusOK, Header: make(http.Header), Body: io.NopCloser(strings.NewReader(""))}, nil
				}),
				close: func() {},
			}}
			target, err := url.Parse("https://upstream.test/v1/realtime")
			if err != nil {
				t.Fatal(err)
			}
			stream, response, err := transport.OpenWebSocket(context.Background(), target, http.Header{
				"Connection":            {"Upgrade"},
				"Upgrade":               {"websocket"},
				"Sec-WebSocket-Key":     {"dGhlIHNhbXBsZSBub25jZQ=="},
				"Sec-WebSocket-Version": {"13"},
			})
			if err != nil || response != nil {
				t.Fatalf("OpenWebSocket() stream=%v response=%v err=%v", stream, response, err)
			}
			if captured == nil {
				t.Fatal("extended CONNECT request was not sent")
			}
			if captured.Method != http.MethodConnect {
				t.Fatalf("method = %q", captured.Method)
			}
			if got := captured.Header.Get("Upgrade"); got != "" {
				t.Fatalf("Upgrade header leaked into extended CONNECT: %q", got)
			}
			if got := captured.Header.Get("Sec-WebSocket-Key"); got != "" {
				t.Fatalf("Sec-WebSocket-Key leaked into extended CONNECT: %q", got)
			}
			if got := captured.Header.Get("Accept-Encoding"); got != "identity" {
				t.Fatalf("Accept-Encoding = %q, want identity", got)
			}
			if protocol == upstreamProtocolH2 {
				if got := captured.Header.Get(":protocol"); got != "websocket" {
					t.Fatalf(":protocol = %q", got)
				}
			} else if captured.Proto != "websocket" {
				t.Fatalf("H3 :protocol = %q", captured.Proto)
			}
			transport.mu.Lock()
			activeStreams := transport.sessions["upstream.test"][0].activeStreams
			transport.mu.Unlock()
			if activeStreams != 1 {
				t.Fatalf("active streams = %d, want 1", activeStreams)
			}
			status := transport.connectionStatusSnapshot()
			if status.webSocketCarriers != 1 || status.activeStreams != 1 {
				t.Fatalf("upstream websocket status = %#v", status)
			}
			if protocol == upstreamProtocolH2 && (status.h2Connections != 1 || status.h2WebSocketCarriers != 1 || status.h3Connections != 0) {
				t.Fatalf("H2 websocket status = %#v", status)
			}
			if protocol == upstreamProtocolH3 && (status.h3Connections != 1 || status.h3WebSocketCarriers != 1 || status.h2Connections != 0) {
				t.Fatalf("H3 websocket status = %#v", status)
			}
			stream.Close()
			transport.mu.Lock()
			activeStreams = transport.sessions["upstream.test"][0].activeStreams
			transport.mu.Unlock()
			if activeStreams != 0 {
				t.Fatalf("active streams after close = %d, want 0", activeStreams)
			}
			status = transport.connectionStatusSnapshot()
			if status.webSocketCarriers != 0 || status.activeStreams != 0 {
				t.Fatalf("upstream websocket status after close = %#v", status)
			}
		})
	}
}

func TestHTTP2ExtendedConnectHonorsPeerSettings(t *testing.T) {
	for _, enabled := range []bool{false, true} {
		t.Run(fmt.Sprintf("enabled=%t", enabled), func(t *testing.T) {
			listener, err := net.Listen("tcp", "127.0.0.1:0")
			if err != nil {
				t.Fatal(err)
			}
			defer listener.Close()
			clientSide, err := net.Dial("tcp", listener.Addr().String())
			if err != nil {
				t.Fatal(err)
			}
			defer clientSide.Close()

			requestHeaders := make(chan map[string]string, 1)
			serverDone := make(chan error, 1)
			go func() {
				serverSide, acceptErr := listener.Accept()
				if acceptErr != nil {
					serverDone <- acceptErr
					return
				}
				serverDone <- serveHTTP2ExtendedConnectSettings(serverSide, enabled, requestHeaders)
			}()

			clientConnection, err := (&http2.Transport{}).NewClientConn(clientSide)
			if err != nil {
				t.Fatal(err)
			}
			defer clientConnection.Close()

			transport := newTestTransport()
			target, err := url.Parse("https://upstream.test/v1/realtime")
			if err != nil {
				t.Fatal(err)
			}
			transport.sessions[target.Host] = []*upstreamSession{{
				protocol:     upstreamProtocolH2,
				roundTripper: clientConnection,
				available: func() bool {
					state := clientConnection.State()
					return !state.Closed && !state.Closing
				},
				createdAt: time.Now(),
				close:     func() { _ = clientConnection.Close() },
			}}

			opened := make(chan webSocketOpenTestResult, 1)
			go func() {
				stream, response, openErr := transport.OpenWebSocket(context.Background(), target, make(http.Header))
				opened <- webSocketOpenTestResult{stream: stream, response: response, err: openErr}
			}()

			if !enabled {
				outcome := awaitWebSocketOpenResult(t, opened)
				if outcome.stream != nil || outcome.response != nil || !isWebSocketTransportUnavailable(outcome.err) {
					t.Fatalf("OpenWebSocket() stream=%v response=%v err=%v", outcome.stream, outcome.response, outcome.err)
				}
				select {
				case headers := <-requestHeaders:
					t.Fatalf("extended CONNECT was sent without peer capability: %#v", headers)
				case <-time.After(100 * time.Millisecond):
				}
				return
			}

			select {
			case headers := <-requestHeaders:
				if headers[":method"] != http.MethodConnect || headers[":protocol"] != "websocket" {
					t.Fatalf("extended CONNECT headers = %#v", headers)
				}
			case <-time.After(time.Second):
				t.Fatal("enabled peer SETTINGS did not permit extended CONNECT")
			}

			outcome := awaitWebSocketOpenResult(t, opened)
			if isWebSocketTransportUnavailable(outcome.err) {
				t.Fatalf("enabled peer SETTINGS was treated as unavailable: %v", outcome.err)
			}
			if outcome.stream != nil {
				outcome.stream.Close()
			}
			if outcome.response != nil {
				_ = outcome.response.Body.Close()
			}
			select {
			case err := <-serverDone:
				if err != nil {
					t.Fatal(err)
				}
			case <-time.After(time.Second):
				t.Fatal("HTTP/2 SETTINGS server did not stop")
			}
		})
	}
}

func TestHTTP3ExtendedConnectHonorsPeerSettings(t *testing.T) {
	for _, enabled := range []bool{false, true} {
		t.Run(fmt.Sprintf("enabled=%t", enabled), func(t *testing.T) {
			listener := newTestHTTP3SettingsListener(t)
			defer listener.Close()

			requestSeen := make(chan struct{}, 1)
			serverDone := make(chan error, 1)
			go func() {
				serverDone <- serveHTTP3ExtendedConnectSettings(listener, enabled, requestSeen)
			}()

			contextWithTimeout, cancel := context.WithTimeout(context.Background(), 2*time.Second)
			defer cancel()
			clientTLSConfig := &tls.Config{
				InsecureSkipVerify: true,
				MinVersion:         tls.VersionTLS13,
				NextProtos:         []string{http3.NextProtoH3},
			}
			quicConnection, err := quic.DialAddr(contextWithTimeout, listener.Addr().String(), clientTLSConfig, nil)
			if err != nil {
				t.Fatal(err)
			}
			defer quicConnection.CloseWithError(0, "test completed")

			clientConnection := (&http3.Transport{}).NewClientConn(quicConnection)
			select {
			case <-clientConnection.ReceivedSettings():
			case <-time.After(time.Second):
				t.Fatal("did not receive real HTTP/3 SETTINGS")
			}
			if got := clientConnection.Settings().EnableExtendedConnect; got != enabled {
				t.Fatalf("EnableExtendedConnect = %t, want %t", got, enabled)
			}
			defer clientConnection.CloseWithError(0, "test completed")

			transport := newTestTransport()
			target := &url.URL{Scheme: "https", Host: listener.Addr().String(), Path: "/v1/realtime"}
			transport.sessions[target.Host] = []*upstreamSession{{
				protocol:     upstreamProtocolH3,
				roundTripper: clientConnection,
				available: func() bool {
					select {
					case <-clientConnection.Context().Done():
						return false
					default:
						return true
					}
				},
				createdAt: time.Now(),
				close: func() {
					_ = clientConnection.CloseWithError(0, "test completed")
				},
			}}

			opened := make(chan webSocketOpenTestResult, 1)
			go func() {
				stream, response, openErr := transport.OpenWebSocket(contextWithTimeout, target, make(http.Header))
				opened <- webSocketOpenTestResult{stream: stream, response: response, err: openErr}
			}()

			if !enabled {
				outcome := awaitWebSocketOpenResult(t, opened)
				if outcome.stream != nil || outcome.response != nil || !isWebSocketTransportUnavailable(outcome.err) {
					t.Fatalf("OpenWebSocket() stream=%v response=%v err=%v", outcome.stream, outcome.response, outcome.err)
				}
				select {
				case <-requestSeen:
					t.Fatal("extended CONNECT was sent without peer capability")
				case <-time.After(100 * time.Millisecond):
				}
				return
			}

			select {
			case <-requestSeen:
			case <-time.After(time.Second):
				t.Fatal("enabled peer SETTINGS did not permit HTTP/3 extended CONNECT")
			}
			outcome := awaitWebSocketOpenResult(t, opened)
			if isWebSocketTransportUnavailable(outcome.err) {
				t.Fatalf("enabled peer SETTINGS was treated as unavailable: %v", outcome.err)
			}
			if outcome.stream != nil {
				outcome.stream.Close()
			}
			if outcome.response != nil {
				_ = outcome.response.Body.Close()
			}
			select {
			case err := <-serverDone:
				if err != nil {
					t.Fatal(err)
				}
			case <-time.After(time.Second):
				t.Fatal("HTTP/3 SETTINGS server did not stop")
			}
		})
	}
}

type webSocketOpenTestResult struct {
	stream   *webSocketStream
	response *http.Response
	err      error
}

func awaitWebSocketOpenResult(t *testing.T, results <-chan webSocketOpenTestResult) webSocketOpenTestResult {
	t.Helper()
	select {
	case result := <-results:
		return result
	case <-time.After(time.Second):
		t.Fatal("OpenWebSocket did not complete")
		return webSocketOpenTestResult{}
	}
}

func serveHTTP2ExtendedConnectSettings(connection net.Conn, enabled bool, requestHeaders chan<- map[string]string) error {
	defer connection.Close()
	if err := connection.SetDeadline(time.Now().Add(2 * time.Second)); err != nil {
		return err
	}
	preface := make([]byte, len(http2.ClientPreface))
	if _, err := io.ReadFull(connection, preface); err != nil {
		return err
	}
	if string(preface) != http2.ClientPreface {
		return fmt.Errorf("unexpected HTTP/2 client preface %q", preface)
	}

	framer := http2.NewFramer(connection, connection)
	frame, err := framer.ReadFrame()
	if err != nil {
		return err
	}
	if _, ok := frame.(*http2.SettingsFrame); !ok {
		return fmt.Errorf("first HTTP/2 frame = %T, want SETTINGS", frame)
	}
	if enabled {
		if err := framer.WriteSettings(http2.Setting{ID: http2.SettingEnableConnectProtocol, Val: 1}); err != nil {
			return err
		}
	} else if err := framer.WriteSettings(); err != nil {
		return err
	}

	decoder := hpack.NewDecoder(4096, nil)
	for {
		frame, err = framer.ReadFrame()
		if err != nil {
			if errors.Is(err, io.EOF) || errors.Is(err, net.ErrClosed) {
				return nil
			}
			return err
		}
		switch typedFrame := frame.(type) {
		case *http2.SettingsFrame:
			if !typedFrame.IsAck() {
				if err := framer.WriteSettingsAck(); err != nil {
					return err
				}
			}
		case *http2.HeadersFrame:
			fields, err := decoder.DecodeFull(typedFrame.HeaderBlockFragment())
			if err != nil {
				return err
			}
			headers := make(map[string]string, len(fields))
			for _, field := range fields {
				headers[field.Name] = field.Value
			}
			requestHeaders <- headers
			return nil
		}
	}
}

func newTestHTTP3SettingsListener(t *testing.T) *quic.Listener {
	t.Helper()
	certificateServer := httptest.NewUnstartedServer(http.NotFoundHandler())
	certificateServer.StartTLS()
	tlsConfig := certificateServer.TLS.Clone()
	certificateServer.Close()
	tlsConfig.MinVersion = tls.VersionTLS13
	tlsConfig.NextProtos = []string{http3.NextProtoH3}
	listener, err := quic.ListenAddr("127.0.0.1:0", tlsConfig, &quic.Config{HandshakeIdleTimeout: time.Second})
	if err != nil {
		t.Fatal(err)
	}
	return listener
}

func serveHTTP3ExtendedConnectSettings(listener *quic.Listener, enabled bool, requestSeen chan<- struct{}) error {
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	connection, err := listener.Accept(ctx)
	if err != nil {
		return err
	}
	controlStream, err := connection.OpenUniStream()
	if err != nil {
		return err
	}
	if _, err := controlStream.Write(http3ExtendedConnectSettingsFrame(enabled)); err != nil {
		return err
	}
	if !enabled {
		select {
		case <-connection.Context().Done():
			return nil
		case <-ctx.Done():
			return context.Cause(ctx)
		}
	}
	stream, err := connection.AcceptStream(ctx)
	if err != nil {
		return err
	}
	requestSeen <- struct{}{}
	stream.CancelRead(0)
	stream.CancelWrite(0)
	return nil
}

func http3ExtendedConnectSettingsFrame(enabled bool) []byte {
	value := uint64(0)
	if enabled {
		value = 1
	}
	payload := quicvarint.Append(nil, 0x8)
	payload = quicvarint.Append(payload, value)
	frame := quicvarint.Append(nil, 0x0)
	frame = quicvarint.Append(frame, 0x4)
	frame = quicvarint.Append(frame, uint64(len(payload)))
	return append(frame, payload...)
}

func TestH3H2TransportFallsBackBeforeHTTPBodyWhenWebSocketIsUnavailable(t *testing.T) {
	transport := newTestTransport()
	var calls int
	transport.sessions["upstream.test"] = []*upstreamSession{{
		protocol: upstreamProtocolH2,
		available: func() bool {
			return true
		},
		createdAt: time.Now(),
		roundTripper: roundTripperFunc(func(request *http.Request) (*http.Response, error) {
			calls++
			if request.Method == http.MethodConnect {
				return nil, errWebSocketTransportUnavailable
			}
			body, err := io.ReadAll(request.Body)
			if err != nil {
				return nil, err
			}
			if string(body) != "payload" {
				return nil, fmt.Errorf("fallback body = %q", body)
			}
			return &http.Response{StatusCode: http.StatusAccepted, Header: make(http.Header), Body: io.NopCloser(strings.NewReader("ok"))}, nil
		}),
		close: func() {},
	}}
	request, err := http.NewRequest(http.MethodPost, "https://upstream.test/v1/chat", strings.NewReader("payload"))
	if err != nil {
		t.Fatal(err)
	}
	response, err := transport.RoundTrip(request)
	if err != nil {
		t.Fatal(err)
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusAccepted || calls != 2 {
		t.Fatalf("response=%d calls=%d", response.StatusCode, calls)
	}
}

func TestH3H2TransportFallsBackForWebSocketNotImplemented(t *testing.T) {
	transport := newTestTransport()
	var calls int
	transport.sessions["upstream.test"] = []*upstreamSession{{
		protocol: upstreamProtocolH2,
		available: func() bool {
			return true
		},
		createdAt: time.Now(),
		roundTripper: roundTripperFunc(func(request *http.Request) (*http.Response, error) {
			calls++
			if request.Method == http.MethodConnect {
				return &http.Response{StatusCode: http.StatusNotImplemented, Header: make(http.Header), Body: io.NopCloser(strings.NewReader("not implemented"))}, nil
			}
			return &http.Response{StatusCode: http.StatusAccepted, Header: make(http.Header), Body: io.NopCloser(strings.NewReader("ordinary"))}, nil
		}),
		close: func() {},
	}}
	request, err := http.NewRequest(http.MethodGet, "https://upstream.test/v1/models", nil)
	if err != nil {
		t.Fatal(err)
	}
	response, err := transport.RoundTrip(request)
	if err != nil {
		t.Fatal(err)
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusAccepted || calls != 2 {
		t.Fatalf("status=%d calls=%d", response.StatusCode, calls)
	}
}

func TestH3H2TransportRelaysWebSocketHandshakeRejectionWithoutRetry(t *testing.T) {
	for _, status := range []int{http.StatusTooManyRequests, http.StatusBadGateway, http.StatusServiceUnavailable} {
		t.Run(http.StatusText(status), func(t *testing.T) {
			transport := newTestTransport()
			var calls int
			transport.sessions["upstream.test"] = []*upstreamSession{{
				protocol: upstreamProtocolH2,
				available: func() bool {
					return true
				},
				createdAt: time.Now(),
				roundTripper: roundTripperFunc(func(request *http.Request) (*http.Response, error) {
					calls++
					if request.Method != http.MethodConnect {
						t.Fatalf("ordinary request was sent after carrier rejection: %s", request.Method)
					}
					headers := make(http.Header)
					headers.Set("X-Carrier-Rejection", "true")
					return &http.Response{StatusCode: status, Header: headers, Body: io.NopCloser(strings.NewReader("carrier rejection"))}, nil
				}),
				close: func() {},
			}}
			client := &http.Client{Transport: transport}
			settings := retrySettings{
				enabled:    true,
				maxRetries: 3,
				matcher: retryStatusMatcher{ranges: []retryRange{{
					start: uint64(status),
					end:   uint64(status),
				}}},
			}
			response, err := roundTripWithRetry(context.Background(), client, http.MethodGet, "https://upstream.test/v1/models", nil, 0, nil, nil, settings)
			if err != nil {
				t.Fatal(err)
			}
			defer response.Body.Close()
			body, err := io.ReadAll(response.Body)
			if err != nil {
				t.Fatal(err)
			}
			if response.StatusCode != status || response.Header.Get("X-Carrier-Rejection") != "true" || string(body) != "carrier rejection" || calls != 1 {
				t.Fatalf("status=%d headers=%#v body=%q calls=%d", response.StatusCode, response.Header, body, calls)
			}
		})
	}
}

func TestOpenWebSocketRejectsCompressedCarrierResponse(t *testing.T) {
	transport := newTestTransport()
	transport.sessions["upstream.test"] = []*upstreamSession{{
		protocol: upstreamProtocolH2,
		available: func() bool {
			return true
		},
		createdAt: time.Now(),
		roundTripper: roundTripperFunc(func(*http.Request) (*http.Response, error) {
			headers := make(http.Header)
			headers.Set("Content-Encoding", "gzip")
			return &http.Response{StatusCode: http.StatusOK, Header: headers, Body: io.NopCloser(strings.NewReader("encoded"))}, nil
		}),
		close: func() {},
	}}
	target, err := url.Parse("https://upstream.test/v1/realtime")
	if err != nil {
		t.Fatal(err)
	}
	stream, response, err := transport.OpenWebSocket(context.Background(), target, make(http.Header))
	if stream != nil || response != nil || !errors.Is(err, errWebSocketCarrierEncoding) {
		t.Fatalf("OpenWebSocket() stream=%v response=%v err=%v", stream, response, err)
	}
}

func TestValidateWebSocketNegotiation(t *testing.T) {
	offered := make(http.Header)
	offered.Set("Sec-WebSocket-Protocol", "chat, realtime")
	offered.Set("Sec-WebSocket-Extensions", "permessage-deflate; client_max_window_bits, x-trace")

	accepted := make(http.Header)
	accepted.Set("Sec-WebSocket-Protocol", "realtime")
	accepted.Set("Sec-WebSocket-Extensions", "permessage-deflate; server_no_context_takeover")
	if err := validateWebSocketNegotiation(accepted, offered); err != nil {
		t.Fatalf("valid negotiation rejected: %v", err)
	}

	badProtocol := accepted.Clone()
	badProtocol.Set("Sec-WebSocket-Protocol", "not-offered")
	if err := validateWebSocketNegotiation(badProtocol, offered); err == nil {
		t.Fatal("unoffered subprotocol was accepted")
	}

	badExtension := accepted.Clone()
	badExtension.Set("Sec-WebSocket-Extensions", "x-not-offered")
	if err := validateWebSocketNegotiation(badExtension, offered); err == nil {
		t.Fatal("unoffered extension was accepted")
	}
}

func TestWebSocketCarrierHeadersForceIdentityEncoding(t *testing.T) {
	for name, headers := range map[string]http.Header{
		"extended": websocketExtendedHeaders(http.Header{"Accept-Encoding": {"gzip"}}),
		"http1":    websocketHTTP1Headers(http.Header{"Accept-Encoding": {"br"}}),
		"stream":   websocketHTTPStreamHeaders(http.Header{"Accept-Encoding": {"deflate"}}),
	} {
		if got := headers.Get("Accept-Encoding"); got != "identity" {
			t.Fatalf("%s Accept-Encoding = %q", name, got)
		}
	}
}

func TestUpstreamWebSocketSwitchDisablesOnlyDirectCarrierNegotiation(t *testing.T) {
	transport := &websocketToggleTransport{}
	gateway := &gateway{
		config: Config{
			ListenAddress:            "127.0.0.1:7780",
			UpstreamBaseURL:          "https://upstream.test/v1",
			UpstreamAPIKey:           "upstream-key",
			UpstreamWebSocketEnabled: false,
			RetryCount:               "0",
		},
		client:              &http.Client{Transport: transport},
		llmtrimRunningCheck: func(string) bool { return false },
		proxyRunning:        true,
	}
	recorder := httptest.NewRecorder()
	gateway.forward(recorder, httptest.NewRequest(http.MethodGet, "http://gateway.test/v1/models", nil))
	if recorder.Code != http.StatusOK || recorder.Body.String() != "ordinary" || transport.openCalls != 0 {
		t.Fatalf("status=%d body=%q OpenWebSocket calls=%d", recorder.Code, recorder.Body.String(), transport.openCalls)
	}
}

func TestCloseGatewayProxyConnectionsClosesTrackedV1ConnectionsOnly(t *testing.T) {
	gateway := &gateway{}
	proxyConnection, proxyPeer := net.Pipe()
	defer proxyPeer.Close()
	managementConnection, managementPeer := net.Pipe()
	defer managementConnection.Close()
	defer managementPeer.Close()

	proxyContext := gateway.proxyConnectionContext(context.Background(), proxyConnection)
	gateway.registerProxyConnection(proxyContext)
	gateway.closeGatewayProxyConnections()

	if _, err := proxyPeer.Read(make([]byte, 1)); !errors.Is(err, io.EOF) {
		t.Fatalf("tracked /v1 connection read error = %v, want EOF", err)
	}
	managementPeer.SetReadDeadline(time.Now().Add(20 * time.Millisecond))
	if _, err := managementPeer.Read(make([]byte, 1)); err == nil || errors.Is(err, io.EOF) {
		t.Fatalf("management connection was unexpectedly closed: %v", err)
	}
	gateway.proxyConnectionMu.Lock()
	remaining := len(gateway.proxyConnections)
	gateway.proxyConnectionMu.Unlock()
	if remaining != 0 {
		t.Fatalf("tracked connection count = %d", remaining)
	}
}

func TestRoundTripHTTPOverWebSocketUsesBinaryHTTPWire(t *testing.T) {
	client, server := net.Pipe()
	defer client.Close()
	defer server.Close()
	stream := &webSocketStream{reader: client, writer: client, closeFn: func() { _ = client.Close() }}
	serverDone := make(chan error, 1)
	go func() {
		payload, opcode, err := wsutil.ReadClientData(server)
		if err != nil {
			serverDone <- err
			return
		}
		if opcode != ws.OpBinary {
			serverDone <- fmt.Errorf("opcode = %v", opcode)
			return
		}
		wire := string(payload)
		if !strings.Contains(wire, "POST /v1/chat?stream=true HTTP/1.1\r\n") || !strings.Contains(wire, "Authorization: Bearer upstream-key\r\n") || !strings.HasSuffix(wire, "hello") {
			serverDone <- fmt.Errorf("unexpected HTTP wire: %q", wire)
			return
		}
		serverDone <- wsutil.WriteServerBinary(server, []byte("HTTP/1.1 201 Created\r\nContent-Length: 2\r\nX-Carrier: websocket\r\n\r\nok"))
	}()

	request, err := http.NewRequest(http.MethodPost, "https://upstream.test/v1/chat?stream=true", strings.NewReader("hello"))
	if err != nil {
		t.Fatal(err)
	}
	request.Header.Set("Authorization", "Bearer upstream-key")
	response, err := roundTripHTTPOverWebSocket(request, stream)
	if err != nil {
		t.Fatal(err)
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusCreated || response.Header.Get("X-Carrier") != "websocket" {
		t.Fatalf("response = %d %#v", response.StatusCode, response.Header)
	}
	body, err := io.ReadAll(response.Body)
	if err != nil {
		t.Fatal(err)
	}
	if string(body) != "ok" {
		t.Fatalf("body = %q", body)
	}
	if err := <-serverDone; err != nil {
		t.Fatal(err)
	}
}

func TestOpenWebSocketHTTPStreamUsesOrdinaryCarrierHeaders(t *testing.T) {
	called := false
	client := &http.Client{Transport: roundTripperFunc(func(request *http.Request) (*http.Response, error) {
		called = true
		if request.Method != http.MethodGet {
			t.Fatalf("method = %q", request.Method)
		}
		if request.Context().Value(websocketTransportBypassKey{}) == nil {
			t.Fatal("fallback request did not bypass HTTP-over-WebSocket recursion")
		}
		if request.Header.Get("Connection") != "" || request.Header.Get("Upgrade") != "" {
			t.Fatalf("hop-by-hop upgrade headers leaked: %#v", request.Header)
		}
		if request.Header.Get("Sec-WebSocket-Key") != "" || request.Header.Get("Sec-WebSocket-Protocol") != "" || request.Header.Get("Sec-WebSocket-Extensions") != "" {
			t.Fatalf("websocket negotiation headers leaked into ordinary carrier: %#v", request.Header)
		}
		if request.Header.Get("Authorization") != "Bearer upstream-key" || request.Header.Get("Accept-Encoding") != "identity" {
			t.Fatalf("carrier headers = %#v", request.Header)
		}
		responseHeaders := make(http.Header)
		responseHeaders.Set("X-Carrier", "ordinary")
		return &http.Response{StatusCode: http.StatusOK, Header: responseHeaders, Body: io.NopCloser(strings.NewReader(""))}, nil
	})}
	headers := make(http.Header)
	headers.Set("Connection", "Upgrade")
	headers.Set("Upgrade", "websocket")
	headers.Set("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
	headers.Set("Sec-WebSocket-Version", "13")
	headers.Set("Sec-WebSocket-Protocol", "chat")
	headers.Set("Sec-WebSocket-Extensions", "permessage-deflate")
	headers.Set("Authorization", "Bearer upstream-key")
	stream, response, err := openWebSocketHTTPStream(context.Background(), client, "https://upstream.test/v1/chat", headers)
	if err != nil || response != nil {
		t.Fatalf("openWebSocketHTTPStream() stream=%v response=%v err=%v", stream, response, err)
	}
	if !called {
		t.Fatal("fallback client was not called")
	}
	if stream.headers.Get("X-Carrier") != "ordinary" {
		t.Fatalf("response headers = %#v", stream.headers)
	}
	stream.Close()
}

func TestHTTP1WebSocketUsesConfiguredHTTPProxy(t *testing.T) {
	upstream := httptest.NewTLSServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		if request.Header.Get("Authorization") != "Bearer upstream-key" {
			http.Error(writer, "missing upstream authorization", http.StatusUnauthorized)
			return
		}
		key := request.Header.Get("Sec-WebSocket-Key")
		if request.Header.Get("Accept-Encoding") != "identity" {
			http.Error(writer, "missing identity encoding", http.StatusBadRequest)
			return
		}
		hijacker, ok := writer.(http.Hijacker)
		if !ok {
			http.Error(writer, "hijack unavailable", http.StatusInternalServerError)
			return
		}
		connection, buffered, err := hijacker.Hijack()
		if err != nil {
			return
		}
		defer connection.Close()
		_, _ = fmt.Fprintf(buffered, "HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Accept: %s\r\n\r\n", webSocketAccept(key))
		_ = buffered.Flush()
	}))
	defer upstream.Close()
	proxyServer := newTestTunnelProxy(t)
	roots := x509.NewCertPool()
	roots.AddCert(upstream.Certificate())
	target, err := url.Parse(upstream.URL + "/v1/realtime")
	if err != nil {
		t.Fatal(err)
	}
	headers := make(http.Header)
	headers.Set("Connection", "Upgrade")
	headers.Set("Upgrade", "websocket")
	headers.Set("Sec-WebSocket-Version", "13")
	headers.Set("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
	headers.Set("Authorization", "Bearer upstream-key")
	stream, response, err := openHTTP1WebSocket(context.Background(), target, headers, proxyServer.server.URL, roots)
	if err != nil || response != nil {
		t.Fatalf("openHTTP1WebSocket() stream=%v response=%v err=%v", stream, response, err)
	}
	stream.Close()
	connectHosts, nonConnect := proxyServer.snapshot()
	if len(nonConnect) != 0 || len(connectHosts) != 1 || connectHosts[0] != target.Host {
		t.Fatalf("proxy routes = CONNECT %#v non-CONNECT %#v", connectHosts, nonConnect)
	}
}

func TestOpenLLMTrimWebSocketUsesDedicatedCAAndRelaysRejection(t *testing.T) {
	upstream := httptest.NewTLSServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		if request.Header.Get("Authorization") != "Bearer upstream-key" {
			http.Error(writer, "missing upstream authorization", http.StatusUnauthorized)
			return
		}
		writer.Header().Set("X-LLMTrim-Rejection", "websocket-not-enabled")
		writer.WriteHeader(http.StatusUpgradeRequired)
		_, _ = writer.Write([]byte("retry with HTTP"))
	}))
	defer upstream.Close()
	proxyServer := newTestTunnelProxy(t)

	profile := t.TempDir()
	caDirectory := filepath.Join(profile, ".llmtrim")
	if err := os.MkdirAll(caDirectory, 0700); err != nil {
		t.Fatal(err)
	}
	caPEM := pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: upstream.Certificate().Raw})
	if err := os.WriteFile(filepath.Join(caDirectory, "ca.pem"), caPEM, 0600); err != nil {
		t.Fatal(err)
	}
	oldProfile, hadProfile := os.LookupEnv("USERPROFILE")
	if err := os.Setenv("USERPROFILE", profile); err != nil {
		t.Fatal(err)
	}
	defer func() {
		if hadProfile {
			_ = os.Setenv("USERPROFILE", oldProfile)
		} else {
			_ = os.Unsetenv("USERPROFILE")
		}
	}()
	oldProxyURL := llmtrimWebSocketProxyURL
	llmtrimWebSocketProxyURL = proxyServer.server.URL
	defer func() { llmtrimWebSocketProxyURL = oldProxyURL }()

	target, err := url.Parse(upstream.URL + "/v1/realtime")
	if err != nil {
		t.Fatal(err)
	}
	headers := make(http.Header)
	headers.Set("Connection", "Upgrade")
	headers.Set("Upgrade", "websocket")
	headers.Set("Sec-WebSocket-Version", "13")
	headers.Set("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
	headers.Set("Authorization", "Bearer upstream-key")
	stream, response, err := openLLMTrimWebSocket(context.Background(), target, headers)
	if stream != nil || response == nil || !errors.Is(err, errWebSocketHandshakeRejected) {
		t.Fatalf("openLLMTrimWebSocket() stream=%v response=%v err=%v", stream, response, err)
	}
	if response.StatusCode != http.StatusUpgradeRequired || response.Header.Get("X-LLMTrim-Rejection") != "websocket-not-enabled" {
		t.Fatalf("rejection = %d %#v", response.StatusCode, response.Header)
	}
	body, readErr := io.ReadAll(response.Body)
	_ = response.Body.Close()
	if readErr != nil || string(body) != "retry with HTTP" {
		t.Fatalf("rejection body = %q, err = %v", body, readErr)
	}
	connectHosts, nonConnect := proxyServer.snapshot()
	if len(nonConnect) != 0 || len(connectHosts) != 1 || connectHosts[0] != target.Host {
		t.Fatalf("llmtrim route = CONNECT %#v non-CONNECT %#v", connectHosts, nonConnect)
	}
}

func TestForwardRejectsUnauthorizedWebSocketBeforeUpstream(t *testing.T) {
	gateway := &gateway{config: Config{LocalAPIKey: "local-key"}}
	request := httptest.NewRequest(http.MethodGet, "http://gateway.test/v1/realtime", nil)
	request.Header.Set("Connection", "Upgrade")
	request.Header.Set("Upgrade", "websocket")
	request.Header.Set("Sec-WebSocket-Version", "13")
	request.Header.Set("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
	response := httptest.NewRecorder()
	gateway.forward(response, request)
	if response.Code != http.StatusUnauthorized {
		t.Fatalf("status = %d", response.Code)
	}
}
