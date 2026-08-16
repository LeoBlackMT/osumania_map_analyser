// Package analytics turns raw events into aggregate statistics for the
// dashboard. It only ever returns aggregated numbers — never install ids,
// individual events, or IPs.
package analytics

import (
	"encoding/json"
	"fmt"
	"math"
	"sort"
	"strings"
	"time"

	"osumania-telemetry/internal/store"
)

const (
	dayMs = int64(24 * 60 * 60 * 1000)
)

type KV struct {
	Key   string `json:"key"`
	Count int64  `json:"count"`
}

type TrendDay struct {
	Date   string `json:"date"`
	Active int64  `json:"active"`
	New    int64  `json:"new"`
}

type Bucket struct {
	Key   string  `json:"key"`
	Value float64 `json:"value"`
	Count int64   `json:"count"`
}

type DurationStats struct {
	AvgMs int64 `json:"avgMs"`
	P50Ms int64 `json:"p50Ms"`
	P90Ms int64 `json:"p90Ms"`
}

type Stats struct {
	ServerVersion    string        `json:"serverVersion"`
	TotalInstalls    int64         `json:"totalInstalls"`
	OnlineNow        int64         `json:"onlineNow"`
	TodayActive      int64         `json:"todayActive"`
	WeekActive       int64         `json:"weekActive"`
	MonthActive      int64         `json:"monthActive"`
	NewToday         int64         `json:"newToday"`
	NewWeek          int64         `json:"newWeek"`
	TotalEvents      int64         `json:"totalEvents"`
	AnalyzeCount     int64         `json:"analyzeCount"`
	AvgStar          float64       `json:"avgStar"`
	AvgLnRatio       float64       `json:"avgLnRatio"`
	PeakOnline       int64         `json:"peakOnline"`
	PeakOnlineHour   int           `json:"peakOnlineHour"`
	AvgDailyActive   int64         `json:"avgDailyActive"`
	OnlineByHour     [24]int64     `json:"onlineByHour"`
	ActiveTrend      []TrendDay    `json:"activeTrend"`
	Algorithms       []KV          `json:"algorithms"`
	ActualAlgorithms []KV          `json:"actualAlgorithms"`
	Keycounts        []KV          `json:"keycounts"`
	Mods             []KV          `json:"mods"`
	Modes            []KV          `json:"modes"`
	Versions         []KV          `json:"versions"`
	StarHistogram    []Bucket      `json:"starHistogram"`
	LnRatioHistogram []Bucket      `json:"lnRatioHistogram"`
	DurationStats    DurationStats `json:"durationStats"`
}

type analyzeData struct {
	Algorithm       string          `json:"algorithm"`
	ActualAlgorithm string          `json:"actualAlgorithm"`
	Keycount        int             `json:"keycount"`
	Mods            []string        `json:"mods"`
	SpeedRate       float64         `json:"speedRate"`
	Mode            string          `json:"mode"`
	Star            float64         `json:"star"`
	LnRatio         float64         `json:"lnRatio"`
	TypeBreakdown   json.RawMessage `json:"typeBreakdown"`
	DurationMs      float64         `json:"durationMs"`
}

// Compute builds the full aggregate snapshot over the requested window.
// days <= 0 means "all time" (no lower bound); otherwise the last `days` days.
func Compute(st *store.Store, onlineWindowMin, days int) (*Stats, error) {
	now := time.Now().UTC()
	nowMs := now.UnixMilli()
	dayStart := time.Date(now.Year(), now.Month(), now.Day(), 0, 0, 0, 0, time.UTC).UnixMilli()
	windowMs := int64(onlineWindowMin) * 60 * 1000
	var since int64
	if days > 0 {
		since = nowMs - int64(days)*dayMs
	}

	s := &Stats{}
	var err error

	if s.TotalInstalls, err = st.CountInstalls(); err != nil {
		return nil, err
	}
	if s.OnlineNow, err = st.CountInstallsOnline(nowMs - windowMs); err != nil {
		return nil, err
	}
	if s.TodayActive, err = st.CountActiveSince(dayStart); err != nil {
		return nil, err
	}
	if s.WeekActive, err = st.CountActiveSince(nowMs - 7*dayMs); err != nil {
		return nil, err
	}
	if s.MonthActive, err = st.CountActiveSince(since); err != nil {
		return nil, err
	}
	if s.NewToday, err = st.CountInstallsNew(dayStart); err != nil {
		return nil, err
	}
	if s.NewWeek, err = st.CountInstallsNew(nowMs - 7*dayMs); err != nil {
		return nil, err
	}
	if s.TotalEvents, err = st.CountEvents(); err != nil {
		return nil, err
	}
	if s.AnalyzeCount, err = st.CountAnalyzeSince(since); err != nil {
		return nil, err
	}
	if s.OnlineByHour, err = st.OnlineByHour(since); err != nil {
		return nil, err
	}
	for h, c := range s.OnlineByHour {
		if c > s.PeakOnline {
			s.PeakOnline = c
			s.PeakOnlineHour = h
		}
	}
	if s.ActiveTrend, err = buildTrend(st, days); err != nil {
		return nil, err
	}
	var activeSum int64
	for _, d := range s.ActiveTrend {
		activeSum += d.Active
	}
	if len(s.ActiveTrend) > 0 {
		s.AvgDailyActive = activeSum / int64(len(s.ActiveTrend))
	}
	if err = buildDistributions(st, since, s); err != nil {
		return nil, err
	}

	return s, nil
}

func buildTrend(st *store.Store, days int) ([]TrendDay, error) {
	n := days
	if n <= 0 {
		n = 90 // "all time" still shows a bounded recent window
	}
	if n > 366 {
		n = 366
	}
	now := time.Now().UTC()
	start := time.Date(now.Year(), now.Month(), now.Day(), 0, 0, 0, 0, time.UTC).AddDate(0, 0, -(n - 1))
	since := start.UnixMilli()

	active, err := st.ActivePerDay(since)
	if err != nil {
		return nil, err
	}
	newCounts, err := st.NewPerDay(since)
	if err != nil {
		return nil, err
	}

	out := make([]TrendDay, 0, n)
	for i := 0; i < n; i++ {
		d := start.AddDate(0, 0, i)
		dayEpoch := d.UnixMilli() / dayMs
		out = append(out, TrendDay{
			Date:   d.Format("2006-01-02"),
			Active: active[dayEpoch],
			New:    newCounts[dayEpoch],
		})
	}
	return out, nil
}

func buildDistributions(st *store.Store, since int64, s *Stats) error {
	alg := map[string]int64{}
	actual := map[string]int64{}
	keys := map[string]int64{}
	mods := map[string]int64{}
	modes := map[string]int64{}
	stars := map[float64]int64{}
	lns := map[float64]int64{}
	var starSum, lnSum float64
	var starN, lnN int64
	var durations []int64

	err := st.ScanAnalyzeData(since, func(dataJSON string) error {
		var d analyzeData
		if err := json.Unmarshal([]byte(dataJSON), &d); err != nil {
			return nil // skip malformed rows
		}
		if d.Algorithm != "" {
			alg[d.Algorithm]++
		}
		if d.ActualAlgorithm != "" {
			actual[d.ActualAlgorithm]++
		}
		if d.Keycount > 0 {
			keys[fmt.Sprintf("%dK", d.Keycount)]++
		}
		if len(d.Mods) == 0 {
			mods["NM"]++
		} else {
			for _, m := range d.Mods {
				if m = strings.TrimSpace(m); m != "" {
					mods[m]++
				}
			}
		}
		if d.Mode != "" {
			modes[d.Mode]++
		}
		if d.Star > 0 {
			// aggregate into 0.5-star bins for the bar chart
			stars[math.Round(d.Star*2)/2]++
			starSum += d.Star
			starN++
		}
		if d.LnRatio >= 0 && d.LnRatio <= 1 {
			// aggregate into 5% bins; labels are percentages
			lns[math.Round(d.LnRatio*20)/20]++
			lnSum += d.LnRatio
			lnN++
		}
		if d.DurationMs >= 0 {
			durations = append(durations, int64(d.DurationMs))
		}
		return nil
	})
	if err != nil {
		return err
	}

	s.Algorithms = sortedKV(alg)
	s.ActualAlgorithms = sortedKV(actual)
	s.Keycounts = sortedKV(keys)
	s.Mods = sortedKV(mods)
	s.Modes = sortedKV(modes)

	versionCounts, err := st.VersionCounts()
	if err != nil {
		return err
	}
	s.Versions = sortedKV(versionCounts)

	s.StarHistogram = histogram(stars, func(k float64) string { return fmt.Sprintf("%.1f", k) })
	s.LnRatioHistogram = histogram(lns, func(k float64) string { return fmt.Sprintf("%.0f%%", k*100) })
	if starN > 0 {
		s.AvgStar = math.Round(starSum/float64(starN)*100) / 100
	}
	if lnN > 0 {
		s.AvgLnRatio = math.Round(lnSum/float64(lnN)*100) / 100
	}
	s.DurationStats = durationStats(durations)

	return nil
}

func sortedKV(m map[string]int64) []KV {
	out := make([]KV, 0, len(m))
	for k, v := range m {
		out = append(out, KV{Key: k, Count: v})
	}
	sort.Slice(out, func(i, j int) bool {
		if out[i].Count != out[j].Count {
			return out[i].Count > out[j].Count
		}
		return out[i].Key < out[j].Key
	})
	return out
}

func histogram(m map[float64]int64, key func(float64) string) []Bucket {
	keys := make([]float64, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.Float64s(keys)
	out := make([]Bucket, 0, len(keys))
	for _, k := range keys {
		out = append(out, Bucket{Key: key(k), Value: k, Count: m[k]})
	}
	return out
}

func durationStats(durations []int64) DurationStats {
	if len(durations) == 0 {
		return DurationStats{}
	}
	sort.Slice(durations, func(i, j int) bool { return durations[i] < durations[j] })
	var sum int64
	for _, d := range durations {
		sum += d
	}
	return DurationStats{
		AvgMs: sum / int64(len(durations)),
		P50Ms: percentile(durations, 50),
		P90Ms: percentile(durations, 90),
	}
}

func percentile(sorted []int64, p float64) int64 {
	if len(sorted) == 0 {
		return 0
	}
	idx := int(math.Ceil(p/100*float64(len(sorted)))) - 1
	if idx < 0 {
		idx = 0
	}
	if idx >= len(sorted) {
		idx = len(sorted) - 1
	}
	return sorted[idx]
}
