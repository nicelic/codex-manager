package main

import (
	"net/http"
	"testing"
)

func TestGatewayConnectionStatusSnapshotAggregatesDirectH2H3Sessions(t *testing.T) {
	transport := newTestTransport()
	h2 := fakeUpstreamSession(upstreamProtocolH2, nil)
	h2.activeStreams = 3
	h2.activeWebSocketCarriers = 1
	h3 := fakeUpstreamSession(upstreamProtocolH3, nil)
	h3.activeStreams = 2
	transport.sessions["upstream.test:443"] = []*upstreamSession{h2, h3}
	firstWebSocket := &webSocketSession{}
	secondWebSocket := &webSocketSession{}
	gateway := &gateway{
		client: &http.Client{Transport: transport},
		webSocketSessions: map[*webSocketSession]struct{}{
			firstWebSocket:  {},
			secondWebSocket: {},
		},
	}
	gateway.localHTTP1Requests.Store(4)

	status := gateway.connectionStatusSnapshot()

	if status.LocalHTTP1 != 4 || status.LocalWebSocket != 2 {
		t.Fatalf("local status = %#v", status)
	}
	if status.UpstreamH2 != 1 || !status.UpstreamH2WebSocket || status.UpstreamH3 != 1 || status.UpstreamH3WebSocket {
		t.Fatalf("upstream protocol status = %#v", status)
	}
	if status.UpstreamWebSocket != 1 || status.UpstreamStreams != 5 {
		t.Fatalf("upstream activity status = %#v", status)
	}
}
