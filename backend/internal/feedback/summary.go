// summary.go —— 公开的 RC 难度反馈聚合（无需 token，不含 per-map 数据）。
//
// 口径（与 docs/features/telemetry.md §7、公开统计页一致）：
//   · 只统计**已接受**的反馈事件（reject/duplicate 不计）；
//   · 分组（算法 / 键数）只在 N ≥ 5 时返回——小样本可以从残差反推到单张图；
//   · 不返回 bid/checksum/install 相关字段，也不返回任何 per-map 明细
//     （per-map 只在私有 admin 页）。
package feedback

import (
	"encoding/json"
	"net/http"
	"sort"
	"strconv"
	"sync"
	"time"

	"osumania-telemetry/internal/store"
)

const summaryMinGroup = 5

// SummaryResponse is the public feedback aggregate.
type SummaryResponse struct {
	GeneratedAt int64          `json:"generated_at"`
	WindowDays  int            `json:"window_days"`
	Totals      SummaryTotals  `json:"totals"`
	Daily       []SummaryPoint `json:"daily"`
	Votes       []VoteBucket   `json:"votes"`
	Algorithms  []GroupBucket  `json:"algorithms"`
	KeyCounts   []GroupBucket  `json:"key_counts"`
}

type SummaryTotals struct {
	Votes       int64   `json:"votes"`
	Maps        int64   `json:"maps"`
	Days        int64   `json:"days"`
	MeanVote    float64 `json:"mean_vote"`
	MeanResidual float64 `json:"mean_residual"`
}

type SummaryPoint struct {
	Day   string `json:"day"`
	Votes int64  `json:"votes"`
}

type VoteBucket struct {
	Vote  float64 `json:"vote"`
	Count int64   `json:"count"`
}

type GroupBucket struct {
	Name         string  `json:"name"`
	N            int64   `json:"n"`
	Maps         int64   `json:"maps"`
	MeanResidual float64 `json:"mean_residual"`
}

// SummaryHandler serves GET /api/v1/feedback/summary?days=30.
type SummaryHandler struct {
	store  *store.FeedbackStore
	ttl    time.Duration
	mu     sync.Mutex
	cached map[int]cachedSummary
}

type cachedSummary struct {
	json []byte
	at   time.Time
}

func NewSummaryHandler(st *store.FeedbackStore) *SummaryHandler {
	return &SummaryHandler{
		store:  st,
		ttl:    time.Minute,
		cached: make(map[int]cachedSummary),
	}
}

func (h *SummaryHandler) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		w.Header().Set("Allow", "GET")
		writeJSON(w, http.StatusMethodNotAllowed, map[string]string{"error": "method_not_allowed"})
		return
	}
	days := 30
	if raw := r.URL.Query().Get("days"); raw != "" {
		parsed, err := strconv.Atoi(raw)
		if err != nil || parsed < 1 || parsed > 90 {
			writeJSON(w, http.StatusBadRequest, map[string]string{"error": "bad_days"})
			return
		}
		days = parsed
	}

	h.mu.Lock()
	if cached, ok := h.cached[days]; ok && time.Since(cached.at) < h.ttl {
		body := cached.json
		h.mu.Unlock()
		writeJSONBytes(w, http.StatusOK, body)
		return
	}
	h.mu.Unlock()

	summary, err := BuildSummary(h.store, days, time.Now())
	if err != nil {
		writeJSON(w, http.StatusServiceUnavailable, map[string]string{"error": "summary_unavailable"})
		return
	}
	body, err := json.Marshal(summary)
	if err != nil {
		writeJSON(w, http.StatusInternalServerError, map[string]string{"error": "encode_failed"})
		return
	}
	h.mu.Lock()
	h.cached[days] = cachedSummary{json: body, at: time.Now()}
	h.mu.Unlock()
	writeJSONBytes(w, http.StatusOK, body)
}

func writeJSONBytes(w http.ResponseWriter, status int, body []byte) {
	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("Cache-Control", "no-cache")
	w.WriteHeader(status)
	_, _ = w.Write(body)
}

// BuildSummary aggregates accepted feedback inside the last `days` UTC days.
func BuildSummary(st *store.FeedbackStore, days int, now time.Time) (SummaryResponse, error) {
	if days < 1 {
		days = 1
	}
	todayStart := store.DayStartMs(now.UnixMilli())
	from := todayStart - int64(days-1)*24*60*60*1000

	out := SummaryResponse{
		GeneratedAt: now.UnixMilli(),
		WindowDays:  days,
		Daily:       []SummaryPoint{},
		Votes:       []VoteBucket{},
		Algorithms:  []GroupBucket{},
		KeyCounts:   []GroupBucket{},
	}
	db := st.DB()

	// Totals. SUM(weight) 作分母避免把低权重票与高权重票等同。
	var weightSum, weightedResidual float64
	if err := db.QueryRow(`
SELECT COUNT(*), COUNT(DISTINCT bid), COUNT(DISTINCT day), COALESCE(AVG(vote), 0),
       COALESCE(SUM(weight), 0), COALESCE(SUM(weight * (target - displayed_val)), 0)
FROM feedback_events WHERE created_at >= ?`, from).Scan(
		&out.Totals.Votes, &out.Totals.Maps, &out.Totals.Days, &out.Totals.MeanVote,
		&weightSum, &weightedResidual,
	); err != nil {
		return out, err
	}
	if weightSum > 0 {
		out.Totals.MeanResidual = weightedResidual / weightSum
	}

	rows, err := db.Query(`SELECT day, COUNT(*) FROM feedback_events WHERE created_at >= ? GROUP BY day ORDER BY day`, from)
	if err != nil {
		return out, err
	}
	for rows.Next() {
		var day, votes int64
		if err := rows.Scan(&day, &votes); err != nil {
			rows.Close()
			return out, err
		}
		out.Daily = append(out.Daily, SummaryPoint{Day: time.UnixMilli(day).UTC().Format("2006-01-02"), Votes: votes})
	}
	rows.Close()
	if err := rows.Err(); err != nil {
		return out, err
	}

	rows, err = db.Query(`SELECT vote, COUNT(*) FROM feedback_events WHERE created_at >= ? GROUP BY vote ORDER BY vote`, from)
	if err != nil {
		return out, err
	}
	for rows.Next() {
		var vote float64
		var count int64
		if err := rows.Scan(&vote, &count); err != nil {
			rows.Close()
			return out, err
		}
		out.Votes = append(out.Votes, VoteBucket{Vote: vote, Count: count})
	}
	rows.Close()
	if err := rows.Err(); err != nil {
		return out, err
	}

	for _, group := range []struct {
		column string
		target *[]GroupBucket
	}{
		{"actual_algorithm", &out.Algorithms},
		{"key_count", &out.KeyCounts},
	} {
		query := `SELECT ` + group.column + `, COUNT(*), COUNT(DISTINCT bid),
       COALESCE(SUM(weight), 0), COALESCE(SUM(weight * (target - displayed_val)), 0)
FROM feedback_events WHERE created_at >= ? GROUP BY ` + group.column + ` ORDER BY COUNT(*) DESC`
		groupRows, err := db.Query(query, from)
		if err != nil {
			return out, err
		}
		for groupRows.Next() {
			var name string
			var n, maps int64
			var wsum, residual float64
			if err := groupRows.Scan(&name, &n, &maps, &wsum, &residual); err != nil {
				groupRows.Close()
				return out, err
			}
			if n < summaryMinGroup {
				continue
			}
			value := 0.0
			if wsum > 0 {
				value = residual / wsum
			}
			*group.target = append(*group.target, GroupBucket{Name: name, N: n, Maps: maps, MeanResidual: value})
		}
		groupRows.Close()
		if err := groupRows.Err(); err != nil {
			return out, err
		}
	}

	sort.Slice(out.Votes, func(i, j int) bool { return out.Votes[i].Vote < out.Votes[j].Vote })
	sort.Slice(out.Daily, func(i, j int) bool { return out.Daily[i].Day < out.Daily[j].Day })
	return out, nil
}
