package main

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestApplicationVersionFromVision(t *testing.T) {
	tests := []struct {
		name    string
		content string
		want    string
		wantErr bool
	}{
		{name: "valid", content: "vision: v0.1.1\n", want: "v0.1.1"},
		{name: "missing separator", content: "v0.1.1", wantErr: true},
		{name: "wrong key", content: "version: v0.1.1", wantErr: true},
		{name: "empty version", content: "vision:   ", wantErr: true},
		{name: "multiple lines", content: "vision: v0.1.1\nextra", wantErr: true},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			got, err := applicationVersionFromVision(test.content)
			if test.wantErr {
				if err == nil {
					t.Fatal("applicationVersionFromVision() error = nil, want error")
				}
				return
			}
			if err != nil {
				t.Fatalf("applicationVersionFromVision() error = %v", err)
			}
			if got != test.want {
				t.Fatalf("applicationVersionFromVision() = %q, want %q", got, test.want)
			}
		})
	}
}

func TestApplicationIdentityIncludesEmbeddedVersion(t *testing.T) {
	recorder := httptest.NewRecorder()
	(&gateway{}).applicationIdentity(recorder, httptest.NewRequest(http.MethodGet, "/api/application/identity", nil))

	if recorder.Code != http.StatusOK {
		t.Fatalf("applicationIdentity() status = %d, want %d", recorder.Code, http.StatusOK)
	}
	var response applicationIdentityResponse
	if err := json.NewDecoder(recorder.Body).Decode(&response); err != nil {
		t.Fatalf("decode application identity response: %v", err)
	}
	wantVersion, err := embeddedApplicationVersion()
	if err != nil {
		t.Fatalf("embeddedApplicationVersion() error = %v", err)
	}
	if response.Version != wantVersion {
		t.Fatalf("response version = %q, want %q", response.Version, wantVersion)
	}
	if response.ExecutablePath == "" {
		t.Fatal("response executable path is empty")
	}
}
