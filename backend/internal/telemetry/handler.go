// Package telemetry exposes the ingest endpoint POST /api/v1/event. It rate
// limits by client IP (in memory only), enforces a server-side whitelist on the
// `data` object, and writes to the store. No client secret is required because
// the plugin is open source — a baked-in token would be public.
package telemetry

import (
	"encoding/json"
	"io"
	"net/http"
	"strings"
	"time"

	"osumania-telemetry/internal/ratelimit"
	"osumania-telemetry/internal/store"
)

const maxBodyBytes = 16 * 1024

// allowedDataKeys is the only data a client may contribute. Everything else is
// dropped server-side so a modified client cannot smuggle extra fields.
var allowedDataKeys = map[string]bool{
	"algorithm":         true,
	"actualAlgorithm":   true,
	"keycount":          true,
	"mods":              true,
	"speedRate":         true,
	"mode":              true,
	"star":              true,
	"lnRatio":           true,
	"typeBreakdown":     true,
	"durationMs":        true,
	"numericDifficulty": true,
	"client":            true,
}

type eventPayload struct {
	ID      string                 `json:"id"`
	Kind    string                 `json:"kind"`
	Version string                 `json:"version"`
	Data    map[string]interface{} `json:"data"`
}

type Handler struct {
	store   *store.Store
	limiter *ratelimit.Limiter
}

func NewHandler(st *store.Store, limiter *ratelimit.Limiter) *Handler {
	return &Handler{store: st, limiter: limiter}
}

func (h *Handler) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	setCORS(w)
	if r.Method == http.MethodOptions {
		w.WriteHeader(http.StatusNoContent)
		return
	}
	if r.Method != http.MethodPost {
		w.WriteHeader(http.StatusMethodNotAllowed)
		return
	}

	if !h.limiter.Allow(clientIP(r)) {
		w.WriteHeader(http.StatusTooManyRequests)
		return
	}

	body, err := io.ReadAll(io.LimitReader(r.Body, maxBodyBytes+1))
	if err != nil || len(body) > maxBodyBytes {
		w.WriteHeader(http.StatusBadRequest)
		return
	}

	var payload eventPayload
	if err := json.Unmarshal(body, &payload); err != nil {
		w.WriteHeader(http.StatusBadRequest)
		return
	}

	payload.ID = strings.TrimSpace(payload.ID)
	if payload.ID == "" || len(payload.ID) > 64 {
		w.WriteHeader(http.StatusBadRequest)
		return
	}
	if payload.Kind != "boot" && payload.Kind != "heartbeat" && payload.Kind != "analyze" {
		w.WriteHeader(http.StatusBadRequest)
		return
	}

	data := whitelist(payload.Data)
	dataJSON, err := json.Marshal(data)
	if err != nil {
		dataJSON = []byte("{}")
	}

	// The event row and every aggregate land in one transaction (see store.go).
	if err := h.store.RecordEvent(payload.ID, payload.Kind, payload.Version, string(dataJSON), data, time.Now().UnixMilli()); err != nil {
		w.WriteHeader(http.StatusInternalServerError)
		return
	}

	w.WriteHeader(http.StatusNoContent)
}

func whitelist(in map[string]interface{}) map[string]interface{} {
	out := make(map[string]interface{})
	for k, v := range in {
		if allowedDataKeys[k] {
			out[k] = v
		}
	}
	return out
}

// clientIP prefers X-Forwarded-For because the documented deployment binds the
// server to loopback behind a reverse proxy (Caddy/Nginx), so only the proxy
// can reach it. When accessed directly, it falls back to RemoteAddr.
func clientIP(r *http.Request) string {
	if xff := r.Header.Get("X-Forwarded-For"); xff != "" {
		if i := strings.IndexByte(xff, ','); i >= 0 {
			xff = xff[:i]
		}
		if ip := strings.TrimSpace(xff); ip != "" {
			return ip
		}
	}
	host := r.RemoteAddr
	if i := strings.LastIndexByte(host, ':'); i >= 0 {
		host = host[:i]
	}
	return host
}

func setCORS(w http.ResponseWriter) {
	w.Header().Set("Access-Control-Allow-Origin", "*")
	w.Header().Set("Access-Control-Allow-Headers", "Content-Type")
	w.Header().Set("Access-Control-Allow-Methods", "POST, OPTIONS")
}
