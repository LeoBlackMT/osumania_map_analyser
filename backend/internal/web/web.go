// Package web serves the public dashboard and the aggregate stats JSON.
package web

import (
	_ "embed"
	"encoding/json"
	"fmt"
	"log"
	"net/http"
	"strconv"
	"strings"
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

// maxWindowDays bounds custom ranges (a guard against hand-typed giants; the
// aggregate tables could answer more, but the UI is designed for <= 90 days).
const maxWindowDays = 400

// dayClamp for the quick window chips (1d/7d/30d/90d).
const maxDays = 90

const dateLayout = "2006-01-02"

type Handler struct {
	store           *store.Store
	onlineWindowMin int
	activeMin       int
	statsCache      time.Duration
	version         string

	mu     sync.Mutex
	cached map[string]cachedStats
}

type cachedStats struct {
	json []byte
	at   time.Time
}

func NewHandler(st *store.Store, onlineWindowMin, activeMin, statsCacheSeconds int, version string) *Handler {
	return &Handler{
		store:           st,
		onlineWindowMin: onlineWindowMin,
		activeMin:       activeMin,
		statsCache:      time.Duration(statsCacheSeconds) * time.Second,
		version:         version,
		cached:          make(map[string]cachedStats),
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

// window is a resolved UTC day-aligned range [fromMs, toMs] (inclusive).
type window struct {
	fromMs int64
	toMs   int64
}

// parseWindow resolves ?days=N or ?from=YYYY-MM-DD&to=YYYY-MM-DD.
//
// Rules:
//   - days: clamp to [1, maxDays]; default 30 when absent.
//   - from/to: from must not precede the earliest day with data (dataStartDay),
//     to is clamped to today, range length is capped by maxWindowDays.
//   - from/to win when both are present (otherwise days).
func parseWindow(q map[string][]string, dataStartDay int64) (window, error) {
	now := time.Now().UTC()
	todayDay := now.UnixMilli() / store.DayMs * store.DayMs
	if dataStartDay <= 0 {
		dataStartDay = todayDay
	}

	get := func(k string) string {
		v := q[k]
		if len(v) == 0 {
			return ""
		}
		return strings.TrimSpace(v[0])
	}
	fromStr, toStr := get("from"), get("to")
	if fromStr != "" || toStr != "" {
		from, errFrom := time.Parse(dateLayout, fromStr)
		to, errTo := time.Parse(dateLayout, toStr)
		if errFrom != nil || errTo != nil {
			return window{}, fmt.Errorf("from/to must use YYYY-MM-DD format")
		}
		fromMs := time.Date(from.Year(), from.Month(), from.Day(), 0, 0, 0, 0, time.UTC).UnixMilli()
		toMs := time.Date(to.Year(), to.Month(), to.Day(), 0, 0, 0, 0, time.UTC).UnixMilli()
		if fromMs < dataStartDay {
			return window{}, fmt.Errorf("from cannot be earlier than %s (first day with data)",
				time.UnixMilli(dataStartDay).UTC().Format(dateLayout))
		}
		if fromMs > toMs {
			return window{}, fmt.Errorf("from must not be after to")
		}
		if toMs > todayDay {
			toMs = todayDay
		}
		if (toMs-fromMs)/store.DayMs+1 > maxWindowDays {
			return window{}, fmt.Errorf("range must not exceed %d days", maxWindowDays)
		}
		return window{fromMs: fromMs, toMs: toMs}, nil
	}

	days := 30
	if v := get("days"); v != "" {
		if n, err := strconv.Atoi(v); err == nil && n >= 1 {
			days = n
			if days > maxDays {
				days = maxDays
			}
		}
	}
	return window{fromMs: todayDay - int64(days-1)*store.DayMs, toMs: todayDay}, nil
}

// stats serves the cached aggregate JSON. The mutex is held across a stale
// recompute so concurrent requests wait once instead of each hitting the DB
// (a cheap stampede guard without extra dependencies).
func (h *Handler) stats(w http.ResponseWriter, r *http.Request) {
	h.mu.Lock()
	defer h.mu.Unlock()

	dataStartDay, err := h.store.DataStartDay()
	if err != nil {
		log.Printf("stats: %v", err)
		w.WriteHeader(http.StatusInternalServerError)
		return
	}
	win, err := parseWindow(r.URL.Query(), dataStartDay)
	if err != nil {
		writeError(w, http.StatusUnprocessableEntity, err.Error())
		return
	}

	key := fmt.Sprintf("%d:%d", win.fromMs, win.toMs)
	if e, ok := h.cached[key]; ok && time.Since(e.at) < h.statsCache {
		w.Header().Set("Content-Type", "application/json; charset=utf-8")
		w.Write(e.json)
		return
	}

	stats, err := analytics.Compute(h.store, h.onlineWindowMin, h.activeMin, win.fromMs, win.toMs)
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

	h.cached[key] = cachedStats{json: data, at: time.Now()}

	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	w.Write(data)
}

func writeError(w http.ResponseWriter, code int, msg string) {
	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	w.WriteHeader(code)
	json.NewEncoder(w).Encode(map[string]string{"error": msg})
}
