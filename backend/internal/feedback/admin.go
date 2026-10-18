package feedback

import (
	"crypto/subtle"
	_ "embed"
	"encoding/csv"
	"encoding/json"
	"fmt"
	"net/http"
	"strconv"
	"strings"
	"time"

	"osumania-telemetry/internal/store"
)

//go:embed admin.html
var adminHTML []byte

// Admin serves the private feedback dashboard and its CSV export.
//
// The HTML shell contains no data and is safe to serve without a token; every
// JSON/CSV endpoint requires `Authorization: Bearer <FEEDBACK_ADMIN_TOKEN>` and
// returns 404 (not 401) when the token is missing or wrong, so the endpoint is
// indistinguishable from a non-existent route.
type Admin struct {
	store       *store.FeedbackStore
	token       string
	minN        int
	minDistinct int
	minDelta    float64
	api         func() map[string]int64
	refresh     func() error
}

// NewAdmin wires the admin surface.
func NewAdmin(st *store.FeedbackStore, token string, minN, minDistinct int, minDelta float64,
	metrics func() map[string]int64, refresh func() error) *Admin {
	return &Admin{
		store: st, token: token, minN: minN, minDistinct: minDistinct, minDelta: minDelta,
		api: metrics, refresh: refresh,
	}
}

func (a *Admin) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	switch r.URL.Path {
	case "/feedback":
		if r.Method != http.MethodGet {
			http.NotFound(w, r)
			return
		}
		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		w.Header().Set("Cache-Control", "no-store")
		_, _ = w.Write(adminHTML)
	case "/api/v1/feedback/stats":
		if !a.authorized(r) || r.Method != http.MethodGet {
			http.NotFound(w, r)
			return
		}
		a.writeStats(w, r)
	case "/api/v1/feedback/export.csv":
		if !a.authorized(r) || r.Method != http.MethodGet {
			http.NotFound(w, r)
			return
		}
		a.writeCSV(w)
	case "/api/v1/feedback/refresh":
		if !a.authorized(r) || r.Method != http.MethodPost {
			http.NotFound(w, r)
			return
		}
		if err := a.refresh(); err != nil {
			http.Error(w, `{"error":"refresh_failed"}`, http.StatusInternalServerError)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"status":"ok"}`))
	default:
		http.NotFound(w, r)
	}
}

func (a *Admin) authorized(r *http.Request) bool {
	if a.token == "" {
		return false
	}
	header := r.Header.Get("Authorization")
	value, ok := strings.CutPrefix(header, "Bearer ")
	if !ok {
		return false
	}
	return subtle.ConstantTimeCompare([]byte(strings.TrimSpace(value)), []byte(a.token)) == 1
}

func (a *Admin) writeStats(w http.ResponseWriter, r *http.Request) {
	days := 30
	if raw := r.URL.Query().Get("days"); raw != "" {
		if parsed, err := strconv.Atoi(raw); err == nil && parsed > 0 && parsed <= 365 {
			days = parsed
		}
	}
	since := time.Now().Add(-time.Duration(days) * 24 * time.Hour)
	sinceDay := store.DayStartMs(since.UnixMilli())

	totals, err := LoadTotals(a.store, a.minN, a.minDistinct, a.minDelta)
	if err != nil {
		http.Error(w, `{"error":"stats_failed"}`, http.StatusInternalServerError)
		return
	}
	maps, err := MapAggRows(a.store, 500)
	if err != nil {
		http.Error(w, `{"error":"stats_failed"}`, http.StatusInternalServerError)
		return
	}
	rejects, err := RejectRows(a.store, sinceDay)
	if err != nil {
		http.Error(w, `{"error":"stats_failed"}`, http.StatusInternalServerError)
		return
	}
	features, err := FeatureRows(a.store, sinceDay)
	if err != nil {
		http.Error(w, `{"error":"stats_failed"}`, http.StatusInternalServerError)
		return
	}
	candidates, err := CandidateRows(a.store, a.minN, a.minDistinct, a.minDelta, 200)
	if err != nil {
		http.Error(w, `{"error":"stats_failed"}`, http.StatusInternalServerError)
		return
	}

	payload := map[string]any{
		"days":       days,
		"totals":     totals,
		"api":        a.api(),
		"rejects":    rejects,
		"maps":       maps,
		"features":   features,
		"candidates": candidates,
		"thresholds": map[string]any{
			"min_n": a.minN, "min_distinct": a.minDistinct, "min_delta": a.minDelta,
		},
	}
	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("Cache-Control", "no-store")
	_ = json.NewEncoder(w).Encode(payload)
}

func (a *Admin) writeCSV(w http.ResponseWriter) {
	rows, err := MapAggRows(a.store, 0)
	if err != nil {
		http.Error(w, `{"error":"export_failed"}`, http.StatusInternalServerError)
		return
	}
	w.Header().Set("Content-Type", "text/csv; charset=utf-8")
	w.Header().Set("Content-Disposition", `attachment; filename="feedback-map-agg.csv"`)
	writer := csv.NewWriter(w)
	defer writer.Flush()
	_ = writer.Write([]string{
		"bid", "checksum", "rate", "actual_algorithm", "mods", "key_count", "pattern",
		"ln_bucket", "n", "weight_sum", "distinct_installs", "target_median",
		"target_q25", "target_q75", "iqr", "updated_at",
	})
	for _, row := range rows {
		_ = writer.Write([]string{
			strconv.FormatInt(row.Bid, 10), row.Checksum, fmt.Sprintf("%.3f", row.Rate),
			row.ActualAlgorithm, row.Mods, strconv.Itoa(row.KeyCount), row.Pattern,
			row.LnBucket, strconv.Itoa(row.N), fmt.Sprintf("%.3f", row.WeightSum),
			strconv.Itoa(row.Distinct), fmt.Sprintf("%.2f", row.Median),
			fmt.Sprintf("%.2f", row.Q25), fmt.Sprintf("%.2f", row.Q75),
			fmt.Sprintf("%.2f", row.IQR),
			time.UnixMilli(row.UpdatedAt).UTC().Format(time.RFC3339),
		})
	}
}
