package main

import (
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestManagementHandlerRejectsOperationsDuringShutdown(t *testing.T) {
	gateway := &gateway{}
	handler := gateway.managementHandler(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusNoContent)
	}))
	gateway.beginShutdown()

	blocked := httptest.NewRecorder()
	handler.ServeHTTP(blocked, httptest.NewRequest(http.MethodPut, "/api/settings/listen_address", nil))
	if blocked.Code != http.StatusServiceUnavailable {
		t.Fatalf("管理操作状态码 = %d，want %d", blocked.Code, http.StatusServiceUnavailable)
	}

	exit := httptest.NewRecorder()
	handler.ServeHTTP(exit, httptest.NewRequest(http.MethodPost, "/api/application/exit", nil))
	if exit.Code != http.StatusNoContent {
		t.Fatalf("退出接口状态码 = %d，want %d", exit.Code, http.StatusNoContent)
	}
}
