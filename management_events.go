package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"net"
	"net/http"
	"net/url"
	"strings"
	"sync"
	"time"

	"github.com/gobwas/ws/wsutil"
)

const managementStatusEventInterval = time.Second
const managementWebSocketWriteTimeout = 5 * time.Second

type managementWebSocketSession struct {
	closeOnce sync.Once
	done      chan struct{}
	closeFn   func()
}

func (session *managementWebSocketSession) Close() {
	if session == nil {
		return
	}
	session.closeOnce.Do(func() {
		close(session.done)
		if session.closeFn != nil {
			session.closeFn()
		}
	})
}

type managementStatusEvent struct {
	Type        string            `json:"type"`
	Timestamp   string            `json:"timestamp"`
	Proxy       json.RawMessage   `json:"proxy,omitempty"`
	Logs        json.RawMessage   `json:"logs,omitempty"`
	LLMTrimLogs json.RawMessage   `json:"llmtrim_logs,omitempty"`
	LLMTrim     json.RawMessage   `json:"llmtrim,omitempty"`
	RTK         json.RawMessage   `json:"rtk,omitempty"`
	Snip        json.RawMessage   `json:"snip,omitempty"`
	Gortex      json.RawMessage   `json:"gortex,omitempty"`
	Errors      map[string]string `json:"errors,omitempty"`
}

type managementStatusCapture struct {
	header http.Header
	status int
	body   bytes.Buffer
}

func (capture *managementStatusCapture) Header() http.Header {
	if capture.header == nil {
		capture.header = make(http.Header)
	}
	return capture.header
}

func (capture *managementStatusCapture) WriteHeader(status int) {
	if capture.status == 0 {
		capture.status = status
	}
}

func (capture *managementStatusCapture) Write(data []byte) (int, error) {
	if capture.status == 0 {
		capture.status = http.StatusOK
	}
	return capture.body.Write(data)
}

type managementStatusTask struct {
	name    string
	path    string
	handler http.HandlerFunc
}

type managementStatusResult struct {
	name string
	data json.RawMessage
	err  error
}

func captureManagementStatus(task managementStatusTask) (json.RawMessage, error) {
	request, err := http.NewRequest(http.MethodGet, "http://"+managementListenAddress+task.path, nil)
	if err != nil {
		return nil, err
	}
	capture := &managementStatusCapture{}
	task.handler(capture, request)
	if capture.status == 0 {
		capture.status = http.StatusOK
	}
	payload := bytes.TrimSpace(capture.body.Bytes())
	if capture.status != http.StatusOK {
		return nil, fmt.Errorf("HTTP %d: %s", capture.status, string(payload))
	}
	if !json.Valid(payload) {
		return nil, fmt.Errorf("status handler returned invalid JSON")
	}
	return append(json.RawMessage(nil), payload...), nil
}

func (g *gateway) managementStatusSnapshot() managementStatusEvent {
	tasks := []managementStatusTask{
		{name: "proxy", path: "/api/proxy", handler: http.HandlerFunc(g.proxyStatus)},
		{name: "logs", path: "/api/logs", handler: http.HandlerFunc(g.logStatus)},
		{name: "llmtrim_logs", path: "/api/llmtrim/logs", handler: http.HandlerFunc(g.llmtrimLogStatus)},
		{name: "llmtrim", path: "/api/llmtrim", handler: http.HandlerFunc(g.llmtrimStatus)},
		{name: "rtk", path: "/api/rtk", handler: http.HandlerFunc(g.rtkStatus)},
		{name: "snip", path: "/api/snip", handler: http.HandlerFunc(g.snipStatus)},
		{name: "gortex", path: "/api/gortex", handler: http.HandlerFunc(g.gortexStatus)},
	}
	results := make(chan managementStatusResult, len(tasks))
	for _, task := range tasks {
		go func(task managementStatusTask) {
			data, err := captureManagementStatus(task)
			results <- managementStatusResult{name: task.name, data: data, err: err}
		}(task)
	}

	event := managementStatusEvent{Type: "status", Timestamp: time.Now().UTC().Format(time.RFC3339Nano)}
	for range tasks {
		result := <-results
		if result.err != nil {
			if event.Errors == nil {
				event.Errors = make(map[string]string)
			}
			event.Errors[result.name] = result.err.Error()
			continue
		}
		switch result.name {
		case "proxy":
			event.Proxy = result.data
		case "logs":
			event.Logs = result.data
		case "llmtrim_logs":
			event.LLMTrimLogs = result.data
		case "llmtrim":
			event.LLMTrim = result.data
		case "rtk":
			event.RTK = result.data
		case "snip":
			event.Snip = result.data
		case "gortex":
			event.Gortex = result.data
		}
	}
	return event
}

func (g *gateway) registerManagementWebSocketSession(session *managementWebSocketSession) {
	if session == nil {
		return
	}
	g.managementWebSocketMu.Lock()
	if g.managementWebSocketSessions == nil {
		g.managementWebSocketSessions = make(map[*managementWebSocketSession]struct{})
	}
	g.managementWebSocketSessions[session] = struct{}{}
	g.managementWebSocketMu.Unlock()
}

func (g *gateway) unregisterManagementWebSocketSession(session *managementWebSocketSession) {
	if session == nil {
		return
	}
	g.managementWebSocketMu.Lock()
	delete(g.managementWebSocketSessions, session)
	g.managementWebSocketMu.Unlock()
}

func (g *gateway) closeManagementWebSocketSessions() {
	g.managementWebSocketMu.Lock()
	sessions := make([]*managementWebSocketSession, 0, len(g.managementWebSocketSessions))
	for session := range g.managementWebSocketSessions {
		sessions = append(sessions, session)
	}
	g.managementWebSocketMu.Unlock()
	for _, session := range sessions {
		session.Close()
	}
}

func (g *gateway) managementWebSocketOriginAllowed(request *http.Request) bool {
	origin, err := url.Parse(strings.TrimSpace(request.Header.Get("Origin")))
	if err != nil || origin.Scheme != "http" || origin.Host == "" || origin.Path != "" || origin.RawQuery != "" || origin.Fragment != "" {
		return false
	}
	address := g.managementAddress
	if address == "" {
		address = managementListenAddress
	}
	return strings.EqualFold(origin.Host, address)
}

func (g *gateway) writeManagementStatusEvent(connection net.Conn) error {
	payload, err := json.Marshal(g.managementStatusSnapshot())
	if err != nil {
		return err
	}
	if err := connection.SetWriteDeadline(time.Now().Add(managementWebSocketWriteTimeout)); err != nil {
		return err
	}
	err = wsutil.WriteServerText(connection, payload)
	_ = connection.SetWriteDeadline(time.Time{})
	return err
}

func (g *gateway) managementEvents(writer http.ResponseWriter, request *http.Request) {
	if request.Method != http.MethodGet {
		http.Error(writer, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	if g.isShuttingDown() {
		http.Error(writer, "code-Manager 正在退出，管理事件流已停止", http.StatusServiceUnavailable)
		return
	}
	if !g.managementWebSocketOriginAllowed(request) {
		http.Error(writer, "forbidden websocket origin", http.StatusForbidden)
		return
	}
	key, err := validateWebSocketUpgrade(request)
	if err != nil {
		http.Error(writer, err.Error(), http.StatusBadRequest)
		return
	}
	hijacker, ok := writer.(http.Hijacker)
	if !ok {
		http.Error(writer, "websocket upgrade is unavailable on this listener", http.StatusInternalServerError)
		return
	}
	connection, buffered, err := hijacker.Hijack()
	if err != nil {
		return
	}
	if err := writeWebSocketHandshake(buffered, key, nil, ""); err != nil {
		_ = connection.Close()
		return
	}
	session := &managementWebSocketSession{done: make(chan struct{}), closeFn: func() { _ = connection.Close() }}
	g.registerManagementWebSocketSession(session)
	defer func() {
		g.unregisterManagementWebSocketSession(session)
		session.Close()
	}()

	ticker := time.NewTicker(managementStatusEventInterval)
	defer ticker.Stop()
	for {
		select {
		case <-session.done:
			return
		default:
		}
		if err := g.writeManagementStatusEvent(connection); err != nil {
			return
		}
		select {
		case <-session.done:
			return
		case <-ticker.C:
		}
	}
}
