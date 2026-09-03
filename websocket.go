package main

import (
	"bufio"
	"context"
	"crypto/rand"
	"crypto/sha1"
	"crypto/tls"
	"crypto/x509"
	"encoding/base64"
	"errors"
	"fmt"
	"io"
	"log"
	"net"
	"net/http"
	"net/url"
	"os"
	"strings"
	"sync"

	"github.com/gobwas/httphead"
	"github.com/gobwas/ws"
	"github.com/gobwas/ws/wsutil"
	"golang.org/x/net/proxy"
)

const webSocketAcceptGUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"

var (
	errWebSocketTransportUnavailable = errors.New("websocket transport is unavailable")
	errWebSocketHandshakeRejected    = errors.New("websocket handshake rejected")
	errWebSocketCarrierEncoding      = errors.New("websocket carrier used a non-identity content encoding")
)

// websocketTransportBypassKey prevents the ordinary-stream fallback from
// recursively attempting the HTTP-over-WebSocket path.
type websocketTransportBypassKey struct{}

// upstreamWebSocketEnabledKey applies only to direct upstream requests. The
// local HTTP/1.1 listener and the llmtrim route deliberately do not consult it.
type upstreamWebSocketEnabledKey struct{}

type webSocketHandshakeRejectionBody struct {
	io.ReadCloser
}

func markWebSocketHandshakeRejection(response *http.Response) *http.Response {
	if response == nil || response.Body == nil {
		return response
	}
	response.Body = &webSocketHandshakeRejectionBody{ReadCloser: response.Body}
	return response
}

func isWebSocketHandshakeRejection(response *http.Response) bool {
	if response == nil || response.Body == nil {
		return false
	}
	_, ok := response.Body.(*webSocketHandshakeRejectionBody)
	return ok
}

type websocketConnector interface {
	OpenWebSocket(context.Context, *url.URL, http.Header) (*webSocketStream, *http.Response, error)
}

// webSocketStream represents the raw octet stream after a WebSocket handshake.
// It intentionally does not decode frames: direct WS-to-WS forwarding must
// preserve masking, fragmentation, extensions, and control frames byte for byte.
type webSocketStream struct {
	reader    io.Reader
	writer    io.WriteCloser
	headers   http.Header
	accept    string
	closeFn   func()
	closeOnce sync.Once
}

func (stream *webSocketStream) Close() {
	if stream == nil {
		return
	}
	stream.closeOnce.Do(func() {
		if stream.closeFn != nil {
			stream.closeFn()
		}
	})
}

type webSocketSession struct {
	closeOnce sync.Once
	closeFn   func()
}

func (session *webSocketSession) Close() {
	if session == nil {
		return
	}
	session.closeOnce.Do(func() {
		if session.closeFn != nil {
			session.closeFn()
		}
	})
}

func (g *gateway) registerWebSocketSession(session *webSocketSession) {
	if session == nil {
		return
	}
	g.webSocketMu.Lock()
	if g.webSocketSessions == nil {
		g.webSocketSessions = make(map[*webSocketSession]struct{})
	}
	g.webSocketSessions[session] = struct{}{}
	g.webSocketMu.Unlock()
}

func (g *gateway) unregisterWebSocketSession(session *webSocketSession) {
	if session == nil {
		return
	}
	g.webSocketMu.Lock()
	delete(g.webSocketSessions, session)
	g.webSocketMu.Unlock()
}

func (g *gateway) closeWebSocketSessions() {
	g.webSocketMu.Lock()
	sessions := make([]*webSocketSession, 0, len(g.webSocketSessions))
	for session := range g.webSocketSessions {
		sessions = append(sessions, session)
	}
	g.webSocketMu.Unlock()
	for _, session := range sessions {
		session.Close()
	}
}

func isWebSocketUpgradeAttempt(request *http.Request) bool {
	if request == nil {
		return false
	}
	return headerHasToken(request.Header, "Connection", "upgrade") || strings.EqualFold(strings.TrimSpace(request.Header.Get("Upgrade")), "websocket")
}

func validateWebSocketUpgrade(request *http.Request) (string, error) {
	if request == nil {
		return "", errors.New("missing websocket request")
	}
	if request.ProtoMajor != 1 {
		return "", errors.New("websocket upgrade requires HTTP/1.1")
	}
	if request.Method != http.MethodGet {
		return "", errors.New("websocket upgrade requires GET")
	}
	if !headerHasToken(request.Header, "Connection", "upgrade") || !strings.EqualFold(strings.TrimSpace(request.Header.Get("Upgrade")), "websocket") {
		return "", errors.New("invalid websocket upgrade headers")
	}
	if strings.TrimSpace(request.Header.Get("Sec-WebSocket-Version")) != "13" {
		return "", errors.New("unsupported websocket version")
	}
	key := strings.TrimSpace(request.Header.Get("Sec-WebSocket-Key"))
	decoded, err := base64.StdEncoding.DecodeString(key)
	if err != nil || len(decoded) != 16 {
		return "", errors.New("invalid Sec-WebSocket-Key")
	}
	return key, nil
}

func headerHasToken(headers http.Header, name, expected string) bool {
	for _, value := range headers.Values(name) {
		for _, token := range strings.Split(value, ",") {
			if strings.EqualFold(strings.TrimSpace(token), expected) {
				return true
			}
		}
	}
	return false
}

func webSocketAccept(key string) string {
	hash := sha1.Sum([]byte(key + webSocketAcceptGUID))
	return base64.StdEncoding.EncodeToString(hash[:])
}

func newWebSocketKey() (string, error) {
	key := make([]byte, 16)
	if _, err := rand.Read(key); err != nil {
		return "", err
	}
	return base64.StdEncoding.EncodeToString(key), nil
}

func websocketUpstreamHeaders(source http.Header, upstreamAPIKey string) http.Header {
	headers := source.Clone()
	headers.Del("Host")
	headers.Del("Proxy-Authorization")
	headers.Set("Authorization", "Bearer "+upstreamAPIKey)
	return headers
}

func websocketExtendedHeaders(source http.Header) http.Header {
	connectionTokens := connectionHeaderTokens(source)
	headers := make(http.Header)
	for key, values := range source {
		lowerKey := strings.ToLower(key)
		if _, namedByConnection := connectionTokens[lowerKey]; namedByConnection || isWebSocketExtendedHopByHopHeader(lowerKey) {
			continue
		}
		for _, value := range values {
			headers.Add(key, value)
		}
	}
	headers.Del("Sec-WebSocket-Key")
	headers.Del("Sec-WebSocket-Accept")
	if headers.Get("Sec-WebSocket-Version") == "" {
		headers.Set("Sec-WebSocket-Version", "13")
	}
	headers.Set("Accept-Encoding", "identity")
	return headers
}

func websocketHTTPStreamHeaders(source http.Header) http.Header {
	connectionTokens := connectionHeaderTokens(source)
	headers := make(http.Header)
	for key, values := range source {
		lowerKey := strings.ToLower(key)
		if _, namedByConnection := connectionTokens[lowerKey]; namedByConnection || isWebSocketExtendedHopByHopHeader(lowerKey) {
			continue
		}
		for _, value := range values {
			headers.Add(key, value)
		}
	}
	headers.Del("Sec-WebSocket-Key")
	headers.Del("Sec-WebSocket-Accept")
	headers.Del("Sec-WebSocket-Version")
	headers.Del("Sec-WebSocket-Protocol")
	headers.Del("Sec-WebSocket-Extensions")
	headers.Set("Accept-Encoding", "identity")
	return headers
}

func websocketHTTP1Headers(source http.Header) http.Header {
	headers := source.Clone()
	headers.Del("Host")
	headers.Del("Proxy-Authorization")
	headers.Set("Connection", "Upgrade")
	headers.Set("Upgrade", "websocket")
	if headers.Get("Sec-WebSocket-Version") == "" {
		headers.Set("Sec-WebSocket-Version", "13")
	}
	headers.Set("Accept-Encoding", "identity")
	return headers
}

func websocketCarrierHeaders(source http.Header) http.Header {
	headers := websocketExtendedHeaders(source)
	headers.Del("Sec-WebSocket-Key")
	headers.Del("Sec-WebSocket-Accept")
	headers.Set("Sec-WebSocket-Version", "13")
	return headers
}

func connectionHeaderTokens(headers http.Header) map[string]struct{} {
	tokens := make(map[string]struct{})
	for _, value := range headers.Values("Connection") {
		for _, token := range strings.Split(value, ",") {
			token = strings.ToLower(strings.TrimSpace(token))
			if token != "" {
				tokens[token] = struct{}{}
			}
		}
	}
	return tokens
}

func isWebSocketExtendedHopByHopHeader(key string) bool {
	switch key {
	case "connection", "keep-alive", "proxy-authenticate", "proxy-authorization", "te", "trailer", "transfer-encoding", "upgrade", "host":
		return true
	default:
		return false
	}
}

func upstreamWebSocketEnabled(ctx context.Context) bool {
	enabled, set := ctx.Value(upstreamWebSocketEnabledKey{}).(bool)
	return !set || enabled
}

func withUpstreamWebSocketEnabled(ctx context.Context, enabled bool) context.Context {
	return context.WithValue(ctx, upstreamWebSocketEnabledKey{}, enabled)
}

func isWebSocketTransportUnavailable(err error) bool {
	return errors.Is(err, errWebSocketTransportUnavailable)
}

func isExtendedConnectCapabilityError(err error) bool {
	if err == nil {
		return false
	}
	message := strings.ToLower(err.Error())
	return strings.Contains(message, "extended connect not supported by peer") ||
		strings.Contains(message, "server didn't enable extended connect") ||
		strings.Contains(message, "server did not enable extended connect")
}

func validateWebSocketCarrierEncoding(headers http.Header) error {
	values := headers.Values("Content-Encoding")
	if len(values) == 0 {
		return nil
	}
	for _, value := range values {
		for _, coding := range strings.Split(value, ",") {
			if !strings.EqualFold(strings.TrimSpace(coding), "identity") {
				return errWebSocketCarrierEncoding
			}
		}
	}
	return nil
}

func websocketHeaderTokens(headers http.Header, name string) ([]string, error) {
	var tokens []string
	for _, value := range headers.Values(name) {
		for _, token := range strings.Split(value, ",") {
			token = strings.TrimSpace(token)
			if token == "" {
				return nil, fmt.Errorf("invalid %s header", name)
			}
			tokens = append(tokens, token)
		}
	}
	return tokens, nil
}

func validateWebSocketSubprotocolNegotiation(responseHeaders, offeredHeaders http.Header) error {
	selected, err := websocketHeaderTokens(responseHeaders, "Sec-WebSocket-Protocol")
	if err != nil {
		return err
	}
	if len(selected) == 0 {
		return nil
	}
	if len(selected) != 1 {
		return errors.New("upstream returned more than one websocket subprotocol")
	}
	offered, err := websocketHeaderTokens(offeredHeaders, "Sec-WebSocket-Protocol")
	if err != nil {
		return fmt.Errorf("invalid offered websocket subprotocol: %w", err)
	}
	for _, candidate := range offered {
		if selected[0] == candidate {
			return nil
		}
	}
	return errors.New("upstream selected a websocket subprotocol that was not offered")
}

func parseWebSocketExtensions(headers http.Header) ([]httphead.Option, error) {
	var options []httphead.Option
	for _, value := range headers.Values("Sec-WebSocket-Extensions") {
		parsed, ok := httphead.ParseOptions([]byte(value), options)
		if !ok {
			return nil, errors.New("invalid Sec-WebSocket-Extensions header")
		}
		options = parsed
	}
	return options, nil
}

func validateWebSocketExtensionNegotiation(responseHeaders, offeredHeaders http.Header) error {
	offered, err := parseWebSocketExtensions(offeredHeaders)
	if err != nil {
		return fmt.Errorf("invalid offered websocket extensions: %w", err)
	}
	selected, err := parseWebSocketExtensions(responseHeaders)
	if err != nil {
		return err
	}
	allowed := make(map[string]struct{}, len(offered))
	for _, extension := range offered {
		allowed[strings.ToLower(string(extension.Name))] = struct{}{}
	}
	seen := make(map[string]struct{}, len(selected))
	for _, extension := range selected {
		name := strings.ToLower(string(extension.Name))
		if _, ok := allowed[name]; !ok {
			return errors.New("upstream selected a websocket extension that was not offered")
		}
		if _, duplicate := seen[name]; duplicate {
			return errors.New("upstream selected a websocket extension more than once")
		}
		seen[name] = struct{}{}
	}
	return nil
}

func validateWebSocketNegotiation(responseHeaders, offeredHeaders http.Header) error {
	if err := validateWebSocketSubprotocolNegotiation(responseHeaders, offeredHeaders); err != nil {
		return err
	}
	return validateWebSocketExtensionNegotiation(responseHeaders, offeredHeaders)
}

func writeWebSocketHandshake(writer *bufio.ReadWriter, key string, headers http.Header, upstreamAccept string) error {
	accept := upstreamAccept
	if accept == "" {
		accept = webSocketAccept(key)
	}
	if _, err := fmt.Fprint(writer, "HTTP/1.1 101 Switching Protocols\r\n"); err != nil {
		return err
	}
	if _, err := fmt.Fprint(writer, "Upgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: "+accept+"\r\n"); err != nil {
		return err
	}
	for header, values := range headers {
		if !isWebSocketHandshakeResponseHeader(header) {
			continue
		}
		for _, value := range values {
			if strings.ContainsAny(header, "\r\n") || strings.ContainsAny(value, "\r\n") {
				continue
			}
			if _, err := fmt.Fprintf(writer, "%s: %s\r\n", header, value); err != nil {
				return err
			}
		}
	}
	if _, err := fmt.Fprint(writer, "\r\n"); err != nil {
		return err
	}
	return writer.Flush()
}

func isWebSocketHandshakeResponseHeader(header string) bool {
	switch strings.ToLower(header) {
	case "connection", "upgrade", "sec-websocket-accept", "content-length", "transfer-encoding", "trailer":
		return false
	default:
		return true
	}
}

func writeWebSocketRejection(writer http.ResponseWriter, response *http.Response) {
	if response == nil {
		http.Error(writer, "upstream websocket handshake failed", http.StatusBadGateway)
		return
	}
	defer response.Body.Close()
	for header, values := range response.Header {
		if strings.EqualFold(header, "Transfer-Encoding") {
			continue
		}
		for _, value := range values {
			writer.Header().Add(header, value)
		}
	}
	writer.WriteHeader(response.StatusCode)
	_, _ = io.Copy(writer, response.Body)
}

func (t *h3H2Transport) OpenWebSocket(ctx context.Context, target *url.URL, headers http.Header) (*webSocketStream, *http.Response, error) {
	if target == nil || target.Scheme != "https" || target.Host == "" {
		return nil, nil, errors.New("websocket upstream must use https")
	}
	session, release, err := t.sessionFor(ctx, target.Host)
	if err != nil {
		return nil, nil, err
	}
	requestReader, requestWriter := io.Pipe()
	request := newExtendedWebSocketRequest(ctx, target, headers, requestReader, session.protocol)
	response, err := session.roundTripper.RoundTrip(request)
	if err != nil {
		_ = requestReader.CloseWithError(err)
		_ = requestWriter.CloseWithError(err)
		release()
		if isExtendedConnectCapabilityError(err) {
			return nil, nil, fmt.Errorf("%w: %v", errWebSocketTransportUnavailable, err)
		}
		return nil, nil, err
	}
	if response.Body == nil {
		response.Body = http.NoBody
	}
	managedBody := &releaseOnClose{ReadCloser: response.Body, release: release}
	response.Body = managedBody
	if response.StatusCode < http.StatusOK || response.StatusCode >= http.StatusMultipleChoices {
		_ = requestReader.Close()
		_ = requestWriter.Close()
		if response.StatusCode == http.StatusNotImplemented {
			return nil, response, errWebSocketTransportUnavailable
		}
		return nil, response, errWebSocketHandshakeRejected
	}
	if err := validateWebSocketCarrierEncoding(response.Header); err != nil {
		_ = requestReader.Close()
		_ = requestWriter.Close()
		_ = managedBody.Close()
		return nil, nil, err
	}
	if err := validateWebSocketNegotiation(response.Header, headers); err != nil {
		_ = requestReader.Close()
		_ = requestWriter.Close()
		_ = managedBody.Close()
		return nil, nil, err
	}
	releaseCarrier := t.retainWebSocketCarrier(session)
	stream := &webSocketStream{
		reader:  managedBody,
		writer:  requestWriter,
		headers: response.Header.Clone(),
		closeFn: func() {
			releaseCarrier()
			_ = requestWriter.Close()
			_ = requestReader.Close()
			_ = managedBody.Close()
		},
	}
	return stream, nil, nil
}

func newExtendedWebSocketRequest(ctx context.Context, target *url.URL, headers http.Header, body io.ReadCloser, protocol upstreamProtocol) *http.Request {
	copyURL := *target
	request := &http.Request{
		Method:        http.MethodConnect,
		URL:           &copyURL,
		Host:          target.Host,
		Header:        websocketExtendedHeaders(headers),
		Body:          body,
		ContentLength: -1,
	}
	request = request.WithContext(ctx)
	if protocol == upstreamProtocolH2 {
		request.Header.Set(":protocol", "websocket")
	} else {
		request.Proto = "websocket"
	}
	return request
}

func shouldTryHTTPOverWebSocket(request *http.Request) bool {
	if request == nil || request.URL == nil || request.URL.Scheme != "https" {
		return false
	}
	if request.Method == http.MethodConnect || request.Context().Value(websocketTransportBypassKey{}) != nil {
		return false
	}
	return upstreamWebSocketEnabled(request.Context()) && !isWebSocketUpgradeAttempt(request)
}

func websocketTransportBypass(ctx context.Context) context.Context {
	return context.WithValue(ctx, websocketTransportBypassKey{}, true)
}

func (t *h3H2Transport) tryRoundTripHTTPOverWebSocket(request *http.Request) (*http.Response, bool, error) {
	stream, response, err := t.OpenWebSocket(request.Context(), request.URL, websocketCarrierHeaders(request.Header))
	if err != nil {
		if isWebSocketTransportUnavailable(err) {
			if response != nil {
				_ = response.Body.Close()
			}
			return nil, false, nil
		}
		if response != nil {
			return markWebSocketHandshakeRejection(response), true, nil
		}
		return nil, true, err
	}
	if stream == nil {
		return nil, true, errors.New("websocket carrier returned no stream")
	}
	response, err = roundTripHTTPOverWebSocket(request, stream)
	return response, true, err
}

func roundTripHTTPOverWebSocket(request *http.Request, stream *webSocketStream) (*http.Response, error) {
	if stream == nil {
		return nil, errors.New("websocket stream is unavailable")
	}
	lockedWriter := &synchronizedWriteCloser{WriteCloser: stream.writer}
	writeDone := make(chan error, 1)
	go func() {
		if request.Body != nil {
			defer request.Body.Close()
		}
		frameWriter := wsutil.NewWriter(lockedWriter, ws.StateClientSide, ws.OpBinary)
		err := request.Write(frameWriter)
		if err == nil {
			err = frameWriter.Flush()
		}
		if err != nil {
			stream.Close()
		}
		writeDone <- err
	}()

	response, err := http.ReadResponse(bufio.NewReader(newWebSocketBinaryReader(stream.reader, lockedWriter)), request)
	if err != nil {
		stream.Close()
		if writeErr := <-writeDone; writeErr != nil {
			return nil, writeErr
		}
		return nil, err
	}
	if response.Body == nil {
		response.Body = http.NoBody
	}
	response.Body = &webSocketResponseBody{ReadCloser: response.Body, stream: stream}
	return response, nil
}

type synchronizedWriteCloser struct {
	io.WriteCloser
	mu sync.Mutex
}

func (writer *synchronizedWriteCloser) Write(data []byte) (int, error) {
	writer.mu.Lock()
	defer writer.mu.Unlock()
	return writer.WriteCloser.Write(data)
}

func (writer *synchronizedWriteCloser) Close() error {
	writer.mu.Lock()
	defer writer.mu.Unlock()
	return writer.WriteCloser.Close()
}

type webSocketResponseBody struct {
	io.ReadCloser
	stream *webSocketStream
	once   sync.Once
}

func (body *webSocketResponseBody) Read(buffer []byte) (int, error) {
	count, err := body.ReadCloser.Read(buffer)
	if err != nil {
		body.finish()
	}
	return count, err
}

func (body *webSocketResponseBody) Close() error {
	err := body.ReadCloser.Close()
	body.finish()
	return err
}

func (body *webSocketResponseBody) finish() {
	body.once.Do(func() {
		body.stream.Close()
	})
}

type webSocketBinaryReader struct {
	reader      *wsutil.Reader
	writer      io.Writer
	readingData bool
}

func newWebSocketBinaryReader(source io.Reader, destination io.Writer) *webSocketBinaryReader {
	reader := wsutil.NewReader(source, ws.StateClientSide)
	reader.OnIntermediate = func(header ws.Header, payload io.Reader) error {
		return handleWebSocketControlFrame(header, payload, destination)
	}
	return &webSocketBinaryReader{reader: reader, writer: destination}
}

func (reader *webSocketBinaryReader) Read(buffer []byte) (int, error) {
	for {
		if reader.readingData {
			count, err := reader.reader.Read(buffer)
			if err == io.EOF {
				reader.readingData = false
				if count > 0 {
					return count, nil
				}
				continue
			}
			if err != nil {
				var closed wsutil.ClosedError
				if errors.As(err, &closed) {
					return count, io.EOF
				}
			}
			return count, err
		}

		header, err := reader.reader.NextFrame()
		if err != nil {
			return 0, err
		}
		switch {
		case header.OpCode == ws.OpBinary:
			reader.readingData = true
		case header.OpCode.IsControl():
			if err := handleWebSocketControlFrame(header, reader.reader, reader.writer); err != nil {
				var closed wsutil.ClosedError
				if errors.As(err, &closed) {
					return 0, io.EOF
				}
				return 0, err
			}
		default:
			if err := reader.reader.Discard(); err != nil {
				return 0, err
			}
			return 0, fmt.Errorf("unexpected websocket opcode %v in HTTP carrier", header.OpCode)
		}
	}
}

func handleWebSocketControlFrame(header ws.Header, payload io.Reader, destination io.Writer) error {
	return (wsutil.ControlHandler{
		Src:                 payload,
		Dst:                 destination,
		State:               ws.StateClientSide,
		DisableSrcCiphering: true,
	}).Handle(header)
}

// webSocketHTTPTransport keeps configured HTTP and SOCKS5 proxy routes intact.
// Its WebSocket attempt is HTTP/1.1 over that same proxy. It falls back before
// consuming the application request body only when WS capability is unavailable.
type webSocketHTTPTransport struct {
	fallback http.RoundTripper
	rawProxy string
}

func newWebSocketHTTPTransport(fallback http.RoundTripper, rawProxy string) *webSocketHTTPTransport {
	return &webSocketHTTPTransport{fallback: fallback, rawProxy: rawProxy}
}

func (transport *webSocketHTTPTransport) RoundTrip(request *http.Request) (*http.Response, error) {
	if shouldTryHTTPOverWebSocket(request) {
		stream, response, err := transport.OpenWebSocket(request.Context(), request.URL, websocketHTTP1CarrierHeaders(request.Header))
		if err == nil && stream != nil {
			return roundTripHTTPOverWebSocket(request, stream)
		}
		if isWebSocketTransportUnavailable(err) {
			if response != nil {
				_ = response.Body.Close()
			}
		} else if response != nil {
			return markWebSocketHandshakeRejection(response), nil
		} else if err != nil {
			return nil, err
		} else {
			return nil, errors.New("websocket carrier returned no stream")
		}
	}
	return transport.fallback.RoundTrip(request)
}

func (transport *webSocketHTTPTransport) OpenWebSocket(ctx context.Context, target *url.URL, headers http.Header) (*webSocketStream, *http.Response, error) {
	return openHTTP1WebSocket(ctx, target, websocketHTTP1Headers(headers), transport.rawProxy, nil)
}

func (transport *webSocketHTTPTransport) CloseIdleConnections() {
	if closer, ok := transport.fallback.(interface{ CloseIdleConnections() }); ok {
		closer.CloseIdleConnections()
	}
}

func websocketHTTP1CarrierHeaders(source http.Header) http.Header {
	headers := websocketHTTP1Headers(source)
	key, err := newWebSocketKey()
	if err == nil {
		headers.Set("Sec-WebSocket-Key", key)
	}
	headers.Set("Sec-WebSocket-Version", "13")
	return headers
}

func openLLMTrimWebSocket(ctx context.Context, target *url.URL, headers http.Header) (*webSocketStream, *http.Response, error) {
	caPath, err := llmtrimCAPEMPath()
	if err != nil {
		return nil, nil, err
	}
	caPEM, err := osReadFile(caPath)
	if err != nil {
		return nil, nil, fmt.Errorf("read llmtrim CA %q: %w", caPath, err)
	}
	roots := x509.NewCertPool()
	if !roots.AppendCertsFromPEM(caPEM) {
		return nil, nil, fmt.Errorf("parse llmtrim CA %q", caPath)
	}
	return openHTTP1WebSocket(ctx, target, websocketHTTP1Headers(headers), llmtrimWebSocketProxyURL, roots)
}

// osReadFile is a variable so focused WebSocket tests can supply a temporary CA
// without replacing process-wide certificate state.
var osReadFile = func(name string) ([]byte, error) {
	return os.ReadFile(name)
}

var llmtrimWebSocketProxyURL = llmtrimProxyURL

func openHTTP1WebSocket(ctx context.Context, target *url.URL, headers http.Header, rawProxy string, roots *x509.CertPool) (*webSocketStream, *http.Response, error) {
	if target == nil || target.Scheme != "https" || target.Host == "" {
		return nil, nil, errors.New("websocket upstream must use https")
	}
	connection, _, response, err := dialWebSocketTunnel(ctx, target, rawProxy)
	if err != nil || response != nil {
		return nil, response, err
	}
	tlsConnection := tls.Client(connection, &tls.Config{
		ServerName: target.Hostname(),
		RootCAs:    roots,
		MinVersion: tls.VersionTLS12,
		NextProtos: []string{"http/1.1"},
	})
	if err := tlsConnection.HandshakeContext(ctx); err != nil {
		_ = connection.Close()
		return nil, nil, err
	}
	reader := bufio.NewReader(tlsConnection)
	requestHeaders := websocketHTTP1Headers(headers)
	key := strings.TrimSpace(requestHeaders.Get("Sec-WebSocket-Key"))
	if _, err := validateWebSocketKey(key); err != nil {
		_ = tlsConnection.Close()
		return nil, nil, err
	}
	copyURL := *target
	request := &http.Request{
		Method:     http.MethodGet,
		URL:        &copyURL,
		Host:       target.Host,
		Header:     requestHeaders,
		Proto:      "HTTP/1.1",
		ProtoMajor: 1,
		ProtoMinor: 1,
	}
	if err := request.Write(tlsConnection); err != nil {
		_ = tlsConnection.Close()
		return nil, nil, err
	}
	response, err = http.ReadResponse(reader, request)
	if err != nil {
		_ = tlsConnection.Close()
		return nil, nil, err
	}
	if response.StatusCode != http.StatusSwitchingProtocols {
		response.Body = &releaseOnClose{ReadCloser: response.Body, release: func() { _ = tlsConnection.Close() }}
		if response.StatusCode == http.StatusNotImplemented {
			return nil, response, errWebSocketTransportUnavailable
		}
		return nil, response, errWebSocketHandshakeRejected
	}
	if err := validateHTTP1WebSocketResponse(response, key, requestHeaders); err != nil {
		_ = response.Body.Close()
		_ = tlsConnection.Close()
		return nil, nil, err
	}
	stream := &webSocketStream{
		reader:  reader,
		writer:  tlsConnection,
		headers: response.Header.Clone(),
		accept:  response.Header.Get("Sec-WebSocket-Accept"),
		closeFn: func() {
			_ = response.Body.Close()
			_ = tlsConnection.Close()
		},
	}
	return stream, nil, nil
}

func validateWebSocketKey(key string) ([]byte, error) {
	decoded, err := base64.StdEncoding.DecodeString(key)
	if err != nil || len(decoded) != 16 {
		return nil, errors.New("invalid Sec-WebSocket-Key")
	}
	return decoded, nil
}

func validateHTTP1WebSocketResponse(response *http.Response, key string, offeredHeaders http.Header) error {
	if !strings.EqualFold(strings.TrimSpace(response.Header.Get("Upgrade")), "websocket") || !headerHasToken(response.Header, "Connection", "upgrade") {
		return errors.New("upstream did not return a websocket upgrade response")
	}
	if response.Header.Get("Sec-WebSocket-Accept") != webSocketAccept(key) {
		return errors.New("upstream returned an invalid Sec-WebSocket-Accept")
	}
	if err := validateWebSocketCarrierEncoding(response.Header); err != nil {
		return err
	}
	return validateWebSocketNegotiation(response.Header, offeredHeaders)
}

func dialWebSocketTunnel(ctx context.Context, target *url.URL, rawProxy string) (net.Conn, *bufio.Reader, *http.Response, error) {
	targetAddress := websocketAuthority(target)
	if rawProxy == "" {
		connection, err := (&net.Dialer{}).DialContext(ctx, "tcp", targetAddress)
		if err != nil {
			return nil, nil, nil, err
		}
		return connection, bufio.NewReader(connection), nil, nil
	}
	proxyURL, err := url.Parse(rawProxy)
	if err != nil || proxyURL.Host == "" {
		return nil, nil, nil, errors.New("invalid outbound proxy URL")
	}
	switch proxyURL.Scheme {
	case "http":
		connection, err := (&net.Dialer{}).DialContext(ctx, "tcp", proxyURL.Host)
		if err != nil {
			return nil, nil, nil, err
		}
		reader := bufio.NewReader(connection)
		if err := writeHTTPConnect(connection, targetAddress, proxyURL); err != nil {
			_ = connection.Close()
			return nil, nil, nil, err
		}
		response, err := http.ReadResponse(reader, &http.Request{Method: http.MethodConnect})
		if err != nil {
			_ = connection.Close()
			return nil, nil, nil, err
		}
		if response.StatusCode < http.StatusOK || response.StatusCode >= http.StatusMultipleChoices {
			response.Body = &releaseOnClose{ReadCloser: response.Body, release: func() { _ = connection.Close() }}
			return connection, reader, response, errWebSocketHandshakeRejected
		}
		_ = response.Body.Close()
		return connection, reader, nil, nil
	case "socks5":
		dialer, err := proxy.SOCKS5("tcp", proxyURL.Host, nil, proxy.Direct)
		if err != nil {
			return nil, nil, nil, err
		}
		connection, err := dialWithContext(ctx, dialer, targetAddress)
		if err != nil {
			return nil, nil, nil, err
		}
		return connection, bufio.NewReader(connection), nil, nil
	default:
		return nil, nil, nil, errors.New("outbound proxy must use http or socks5")
	}
}

func websocketAuthority(target *url.URL) string {
	if _, _, err := net.SplitHostPort(target.Host); err == nil {
		return target.Host
	}
	return net.JoinHostPort(target.Hostname(), "443")
}

func writeHTTPConnect(writer io.Writer, targetAddress string, proxyURL *url.URL) error {
	if _, err := fmt.Fprintf(writer, "CONNECT %s HTTP/1.1\r\nHost: %s\r\n", targetAddress, targetAddress); err != nil {
		return err
	}
	if proxyURL.User != nil {
		password, _ := proxyURL.User.Password()
		credentials := base64.StdEncoding.EncodeToString([]byte(proxyURL.User.Username() + ":" + password))
		if _, err := fmt.Fprint(writer, "Proxy-Authorization: Basic "+credentials+"\r\n"); err != nil {
			return err
		}
	}
	_, err := fmt.Fprint(writer, "\r\n")
	return err
}

func dialWithContext(ctx context.Context, dialer proxy.Dialer, address string) (net.Conn, error) {
	type dialResult struct {
		connection net.Conn
		err        error
	}
	result := make(chan dialResult, 1)
	go func() {
		connection, err := dialer.Dial("tcp", address)
		result <- dialResult{connection: connection, err: err}
	}()
	select {
	case outcome := <-result:
		return outcome.connection, outcome.err
	case <-ctx.Done():
		go func() {
			outcome := <-result
			if outcome.connection != nil {
				_ = outcome.connection.Close()
			}
		}()
		return nil, context.Cause(ctx)
	}
}

func openWebSocketHTTPStream(ctx context.Context, client *http.Client, target string, headers http.Header) (*webSocketStream, *http.Response, error) {
	if client == nil {
		return nil, nil, errors.New("HTTP client is unavailable")
	}
	requestReader, requestWriter := io.Pipe()
	request, err := http.NewRequestWithContext(websocketTransportBypass(ctx), http.MethodGet, target, requestReader)
	if err != nil {
		_ = requestReader.Close()
		_ = requestWriter.Close()
		return nil, nil, err
	}
	request.Header = websocketHTTPStreamHeaders(headers)
	request.ContentLength = -1
	response, err := client.Do(request)
	if err != nil {
		_ = requestReader.CloseWithError(err)
		_ = requestWriter.CloseWithError(err)
		return nil, nil, err
	}
	if response.Body == nil {
		response.Body = http.NoBody
	}
	if response.StatusCode < http.StatusOK || response.StatusCode >= http.StatusMultipleChoices {
		_ = requestReader.Close()
		_ = requestWriter.Close()
		return nil, response, errWebSocketHandshakeRejected
	}
	if err := validateWebSocketCarrierEncoding(response.Header); err != nil {
		_ = requestReader.Close()
		_ = requestWriter.Close()
		_ = response.Body.Close()
		return nil, nil, err
	}
	if err := validateWebSocketNegotiation(response.Header, headers); err != nil {
		_ = requestReader.Close()
		_ = requestWriter.Close()
		_ = response.Body.Close()
		return nil, nil, err
	}
	stream := &webSocketStream{
		reader:  response.Body,
		writer:  requestWriter,
		headers: response.Header.Clone(),
		closeFn: func() {
			_ = requestWriter.Close()
			_ = requestReader.Close()
			_ = response.Body.Close()
		},
	}
	return stream, nil, nil
}

func (g *gateway) forwardWebSocket(writer http.ResponseWriter, request *http.Request, config Config, key string, finishLocalHTTP1Request func()) {
	proxyContext, leaveProxyRequest, entered := g.enterProxyRequest(request.Context())
	if !entered {
		state, message := g.proxyRequestUnavailable()
		status := http.StatusServiceUnavailable
		if state == proxyStateUnavailable {
			status = http.StatusBadGateway
		}
		writeJSON(writer, status, proxyStatusResponse{
			Running:       false,
			State:         state,
			ListenAddress: config.ListenAddress,
			Message:       message,
			Connections:   g.connectionStatusSnapshot(),
		})
		return
	}
	defer leaveProxyRequest()

	target, err := joinUpstreamURL(config.UpstreamBaseURL, strings.TrimPrefix(request.URL.Path, "/v1/"), request.URL.RawQuery)
	if err != nil {
		http.Error(writer, "invalid upstream target", http.StatusBadGateway)
		return
	}
	upstreamURL, err := url.Parse(target)
	if err != nil {
		http.Error(writer, "invalid upstream target", http.StatusBadGateway)
		return
	}
	headers := websocketUpstreamHeaders(request.Header, config.UpstreamAPIKey)
	llmtrimRunning := g.observeLLMTrimRunning(config.LLMTrimPath)

	if llmtrimRunning {
		stream, response, err := openLLMTrimWebSocket(proxyContext, upstreamURL, headers)
		if response != nil {
			writeWebSocketRejection(writer, response)
			return
		}
		if err != nil {
			if proxyContext.Err() == nil {
				log.Printf("llmtrim websocket handshake failed for %s: %v", request.URL.Path, err)
				http.Error(writer, "llmtrim websocket proxy unavailable", http.StatusBadGateway)
			}
			return
		}
		g.proxyWebSocketStream(writer, key, stream, finishLocalHTTP1Request)
		return
	}

	client, err := g.selectUpstreamClient(false)
	if err != nil {
		http.Error(writer, "upstream client unavailable", http.StatusBadGateway)
		return
	}
	if config.UpstreamWebSocketEnabled {
		connector, ok := clientTransport(client).(websocketConnector)
		if !ok {
			log.Printf("upstream websocket carrier is not available for %s; using ordinary stream", request.URL.Path)
		} else {
			stream, response, connectErr := connector.OpenWebSocket(proxyContext, upstreamURL, headers)
			if connectErr == nil && stream != nil {
				g.proxyWebSocketStream(writer, key, stream, finishLocalHTTP1Request)
				return
			}
			if isWebSocketTransportUnavailable(connectErr) {
				if response != nil {
					_ = response.Body.Close()
				}
				if proxyContext.Err() == nil {
					log.Printf("upstream websocket carrier unavailable for %s; using ordinary stream: %v", request.URL.Path, connectErr)
				}
			} else if response != nil {
				writeWebSocketRejection(writer, response)
				return
			} else if connectErr != nil {
				if proxyContext.Err() == nil {
					log.Printf("upstream websocket handshake failed for %s: %v", request.URL.Path, connectErr)
					http.Error(writer, "upstream websocket handshake failed", http.StatusBadGateway)
				}
				return
			} else {
				http.Error(writer, "upstream websocket handshake returned no stream", http.StatusBadGateway)
				return
			}
		}
	}

	stream, response, err := openWebSocketHTTPStream(proxyContext, client, target, headers)
	if response != nil {
		writeWebSocketRejection(writer, response)
		return
	}
	if err != nil {
		if proxyContext.Err() == nil {
			log.Printf("upstream websocket stream fallback failed for %s: %v", request.URL.Path, err)
			http.Error(writer, "upstream websocket request failed", http.StatusBadGateway)
		}
		return
	}
	g.proxyWebSocketStream(writer, key, stream, finishLocalHTTP1Request)
}

func (g *gateway) proxyWebSocketStream(writer http.ResponseWriter, key string, stream *webSocketStream, finishLocalHTTP1Request func()) {
	hijacker, ok := writer.(http.Hijacker)
	if !ok {
		stream.Close()
		http.Error(writer, "websocket upgrade is unavailable on this listener", http.StatusInternalServerError)
		return
	}
	connection, buffered, err := hijacker.Hijack()
	if err != nil {
		stream.Close()
		return
	}
	if err := writeWebSocketHandshake(buffered, key, stream.headers, stream.accept); err != nil {
		stream.Close()
		_ = connection.Close()
		g.unregisterProxyConnection(connection)
		return
	}
	session := &webSocketSession{closeFn: func() {
		stream.Close()
		_ = connection.Close()
	}}
	g.registerWebSocketSession(session)
	if finishLocalHTTP1Request != nil {
		finishLocalHTTP1Request()
	}
	defer func() {
		g.unregisterWebSocketSession(session)
		g.unregisterProxyConnection(connection)
		session.Close()
	}()

	result := make(chan error, 2)
	go func() {
		_, err := io.Copy(stream.writer, buffered)
		_ = stream.writer.Close()
		result <- err
	}()
	go func() {
		_, err := io.Copy(connection, stream.reader)
		result <- err
	}()
	<-result
}
