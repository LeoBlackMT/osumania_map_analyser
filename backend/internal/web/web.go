// Package web serves the public dashboard and the aggregate stats JSON.
package web

import (
	_ "embed"
	"encoding/json"
	"log"
	"net/http"
	"strconv"
	"sync"
	"time"

	"osumania-telemetry/internal/analytics"
	"osumania-telemetry/internal/store"
)

//go:embed dashboard.html
var dashboardHTML []byte

//go:embed dashboard.css
var dashboardCSS []byte

//go:embed dashboard.js
var dashboardJS []byte

const maxDays = 365

type Handler struct {
	store           *store.Store
	onlineWindowMin int
	statsCache      time.Duration
	version         string

	mu     sync.Mutex
	cached map[int]cachedStats
}

type cachedStats struct {
	json []byte
	at   time.Time
}

func NewHandler(st *store.Store, onlineWindowMin, statsCacheSeconds int, version string) *Handler {
	return &Handler{
		store:           st,
		onlineWindowMin: onlineWindowMin,
		statsCache:      time.Duration(statsCacheSeconds) * time.Second,
		version:         version,
		cached:          make(map[int]cachedStats),
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
		w.Header().Set("Cache-Control", "no-cache")
		w.Write(dashboardHTML)
	case "/dashboard.css":
		if r.Method != http.MethodGet {
			w.WriteHeader(http.StatusMethodNotAllowed)
			return
		}
		w.Header().Set("Content-Type", "text/css; charset=utf-8")
		w.Header().Set("Cache-Control", "no-cache")
		w.Write(dashboardCSS)
	case "/dashboard.js":
		if r.Method != http.MethodGet {
			w.WriteHeader(http.StatusMethodNotAllowed)
			return
		}
		w.Header().Set("Content-Type", "application/javascript; charset=utf-8")
		w.Header().Set("Cache-Control", "no-cache")
		w.Write(dashboardJS)
	case "/api/v1/stats":
		if r.Method != http.MethodGet {
			w.WriteHeader(http.StatusMethodNotAllowed)
			return
		}
		h.stats(w, r)
	default:
		w.WriteHeader(http.StatusNotFound)
	}
}

// parseDays maps the ?days= query (or "all") to a window length in days.
// 0 means "all time"; empty/invalid values default to 30.
func parseDays(r *http.Request) int {
	v := r.URL.Query().Get("days")
	if v == "" {
		return 30
	}
	if v == "all" {
		return 0
	}
	n, err := strconv.Atoi(v)
	if err != nil || n < 0 {
		return 30
	}
	if n > maxDays {
		return maxDays
	}
	return n
}

// stats serves the cached aggregate JSON. The mutex is held across a stale
// recompute so concurrent requests wait once instead of each hitting the DB
// (a cheap stampede guard without extra dependencies). The cache is keyed by
// the days window.
func (h *Handler) stats(w http.ResponseWriter, r *http.Request) {
	days := parseDays(r)
	h.mu.Lock()
	defer h.mu.Unlock()

	if e, ok := h.cached[days]; ok && time.Since(e.at) < h.statsCache {
		w.Header().Set("Content-Type", "application/json; charset=utf-8")
		w.Write(e.json)
		return
	}

	stats, err := analytics.Compute(h.store, h.onlineWindowMin, days)
	if err != nil {
		log.Printf("stats: %v", err)
		w.WriteHeader(http.StatusInternalServerError)
		return
	}
	stats.ServerVersion = h.version
	data, err := json.Marshal(stats)
	if err != nil {
		w.WriteHeader(http.StatusInternalServerError)
		return
	}

	h.cached[days] = cachedStats{json: data, at: time.Now()}

	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	w.Write(data)
}
