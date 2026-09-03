package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"net"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/gobwas/ws/wsutil"
)

func TestManagementEventsRejectsForeignOrigin(t *testing.T) {
	gateway := &gateway{managementAddress: managementListenAddress}
	request := httptest.NewRequest(http.MethodGet, "http://"+managementListenAddress+"/api/events", nil)
	request.Header.Set("Origin", "https://example.invalid")
	request.Header.Set("Connection", "Upgrade")
	request.Header.Set("Upgrade", "websocket")
	request.Header.Set("Sec-WebSocket-Version", "13")
	request.Header.Set("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
	recorder := httptest.NewRecorder()

	gateway.managementEvents(recorder, request)

	if recorder.Code != http.StatusForbidden {
		t.Fatalf("status = %d, want %d", recorder.Code, http.StatusForbidden)
	}
}

func TestManagementEventsSendsStatusSnapshot(t *testing.T) {
	gateway := &gateway{}
	server := httptest.NewServer(http.HandlerFunc(gateway.managementEvents))
	defer server.Close()
	gateway.managementAddress = strings.TrimPrefix(server.URL, "http://")

	connection, err := net.DialTimeout("tcp", gateway.managementAddress, time.Second)
	if err != nil {
		t.Fatal(err)
	}
	defer connection.Close()
	if _, err := fmt.Fprintf(connection, "GET /api/events HTTP/1.1\r\nHost: %s\r\nOrigin: %s\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n", gateway.managementAddress, server.URL); err != nil {
		t.Fatal(err)
	}
	response, err := http.ReadResponse(bufio.NewReader(connection), &http.Request{Method: http.MethodGet})
	if err != nil {
		t.Fatal(err)
	}
	if response.StatusCode != http.StatusSwitchingProtocols {
		response.Body.Close()
		t.Fatalf("status = %d, want %d", response.StatusCode, http.StatusSwitchingProtocols)
	}
	response.Body.Close()
	if err := connection.SetReadDeadline(time.Now().Add(5 * time.Second)); err != nil {
		t.Fatal(err)
	}
	payload, err := wsutil.ReadServerText(connection)
	if err != nil {
		t.Fatal(err)
	}
	var event managementStatusEvent
	if err := json.Unmarshal(payload, &event); err != nil {
		t.Fatal(err)
	}
	if event.Type != "status" {
		t.Fatalf("event type = %q, want status", event.Type)
	}
	if len(event.Proxy) == 0 || len(event.LLMTrim) == 0 || len(event.RTK) == 0 || len(event.Snip) == 0 {
		t.Fatalf("status snapshot is missing a required section: %#v", event)
	}
	var proxy proxyStatusResponse
	if err := json.Unmarshal(event.Proxy, &proxy); err != nil {
		t.Fatal(err)
	}
	if proxy.Connections.LocalHTTP1 != 0 || proxy.Connections.LocalWebSocket != 0 || proxy.Connections.UpstreamH2 != 0 || proxy.Connections.UpstreamH3 != 0 || proxy.Connections.UpstreamWebSocket != 0 || proxy.Connections.UpstreamStreams != 0 {
		t.Fatalf("unexpected initial connection status: %#v", proxy.Connections)
	}
}
