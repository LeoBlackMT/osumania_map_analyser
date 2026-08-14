// Package web serves the public dashboard and the aggregate stats JSON.
package web

import (
	_ "embed"
	"encoding/json"
	"net/http"
	"sync"
	"time"

	"osumania-telemetry/internal/analytics"
	"osumania-telemetry/internal/store"
)

//go:embed dashboard.html
var dashboardHTML []byte

type Handler struct {
	store           *store.Store
	onlineWindowMin int
	statsCache      time.Duration

	mu         sync.Mutex
	cachedJSON []byte
	cachedAt   time.Time
}

func NewHandler(st *store.Store, onlineWindowMin, statsCacheSeconds int) *Handler {
	return &Handler{
		store:           st,
		onlineWindowMin: onlineWindowMin,
		statsCache:      time.Duration(statsCacheSeconds) * time.Second,
	}
}

func (h *Handler) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	switch r.URL.Path {
	case "/", "/dashboard":
		if r.Method != http.MethodGet {
			w.WriteHeader(http.StatusMethodNotAllowed)
			return
		}
		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		w.Write(dashboardHTML)
	case "/api/v1/stats":
		if r.Method != http.MethodGet {
			w.WriteHeader(http.StatusMethodNotAllowed)
			return
		}
		h.stats(w)
	default:
		w.WriteHeader(http.StatusNotFound)
	}
}

// stats serves the cached aggregate JSON. The mutex is held across a stale
// recompute so concurrent requests wait once instead of each hitting the DB
// (a cheap stampede guard without extra dependencies).
func (h *Handler) stats(w http.ResponseWriter) {
	h.mu.Lock()
	defer h.mu.Unlock()

	if h.cachedJSON != nil && time.Since(h.cachedAt) < h.statsCache {
		w.Header().Set("Content-Type", "application/json; charset=utf-8")
		w.Write(h.cachedJSON)
		return
	}

	stats, err := analytics.Compute(h.store, h.onlineWindowMin)
	if err != nil {
		w.WriteHeader(http.StatusInternalServerError)
		return
	}
	data, err := json.Marshal(stats)
	if err != nil {
		w.WriteHeader(http.StatusInternalServerError)
		return
	}

	h.cachedJSON = data
	h.cachedAt = time.Now()

	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	w.Write(data)
}
