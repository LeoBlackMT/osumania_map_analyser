// Package analytics turns write-path aggregates into dashboard statistics.
//
// Nothing here touches the `events` table: every query reads daily_agg,
// install_days or install_hours (bounded, milliseconds regardless of volume).
// It also never returns install ids, individual events, or IPs.
package analytics

import (
	"fmt"
	"math"
	"sort"
	"strconv"
	"strings"
	"time"

	"osumania-telemetry/internal/store"
)

type KV struct {
	Key   string  `json:"key"`
	Count int64   `json:"count"`
	Pct   float64 `json:"pct"`
}

type TrendDay struct {
	Date   string `json:"date"`
	Active int64  `json:"active"`
	New    int64  `json:"new"`
}

// TrendHour is one point of the hourly trend (windows of one day or less).
type TrendHour struct {
	Hour   int64 `json:"hour"` // epoch ms, UTC, truncated to the hour
	Active int64 `json:"active"`
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
	ServerVersion     string        `json:"serverVersion"`
	DataStart         string        `json:"dataStart"`
	WindowFrom        string        `json:"windowFrom"`
	WindowTo          string        `json:"windowTo"`
	TotalInstalls     int64         `json:"totalInstalls"`
	OnlineNow         int64         `json:"onlineNow"`
	TodayActive       int64         `json:"todayActive"`
	WeekActive        int64         `json:"weekActive"`
	WeekActivePrev    int64         `json:"weekActivePrev"`
	MonthActive       int64         `json:"monthActive"`
	NewToday          int64         `json:"newToday"`
	NewWeek           int64         `json:"newWeek"`
	TotalEvents       int64         `json:"totalEvents"`
	AnalyzeCount      int64         `json:"analyzeCount"`
	AvgStar           float64       `json:"avgStar"`
	AvgLnRatio        float64       `json:"avgLnRatio"`
	PeakOnline        int64         `json:"peakOnline"`
	PeakOnlineHour    int           `json:"peakOnlineHour"`
	AvgDailyActive    int64         `json:"avgDailyActive"`
	OnlineByHour      [24]int64     `json:"onlineByHour"`
	ActiveTrend       []TrendDay    `json:"activeTrend"`
	ActiveTrendHourly []TrendHour   `json:"activeTrendHourly,omitempty"`
	Algorithms        []KV          `json:"algorithms"`
	ActualAlgorithms  []KV          `json:"actualAlgorithms"`
	Keycounts         []KV          `json:"keycounts"`
	Mods              []KV          `json:"mods"`
	Modes             []KV          `json:"modes"`
	Versions          []KV          `json:"versions"`
	StarHistogram     []Bucket      `json:"starHistogram"`
	LnRatioHistogram  []Bucket      `json:"lnRatioHistogram"`
	NumericHistogram  []Bucket      `json:"numericHistogram"`
	DurationHistogram []Bucket      `json:"durationHistogram"`
	DurationStats     DurationStats `json:"durationStats"`
	PlayFreq          []KV          `json:"playFreq"`
}

// Compute builds the dashboard snapshot over the window [fromMs, toMs]
// (UTC day-aligned, inclusive). `minAnalyze` is the daily threshold an
// install must reach to count as active that day.
func Compute(st *store.Store, onlineWindowMin, minAnalyze int, fromMs, toMs int64) (*Stats, error) {
	now := time.Now().UTC()
	nowMs := now.UnixMilli()
	day := store.DayMs
	todayDay := nowMs / day * day

	s := &Stats{}
	var err error
	s.DataStart = dataStart(st)

	// Fixed-window headline cards (anchored to "now", independent of the
	// chart window, so their labels always mean what they say).
	if s.TotalInstalls, err = st.CountInstalls(); err != nil {
		return nil, err
	}
	if s.OnlineNow, err = st.CountInstallsOnline(nowMs - int64(onlineWindowMin)*60*1000); err != nil {
		return nil, err
	}
	if s.TodayActive, err = st.ActiveCountBetweenDays(todayDay, todayDay+day, minAnalyze); err != nil {
		return nil, err
	}
	if s.WeekActive, err = st.ActiveCountBetweenDays(todayDay-6*day, todayDay+day, minAnalyze); err != nil {
		return nil, err
	}
	if s.WeekActivePrev, err = st.ActiveCountBetweenDays(todayDay-13*day, todayDay-6*day, minAnalyze); err != nil {
		return nil, err
	}
	if s.MonthActive, err = st.ActiveCountBetweenDays(todayDay-29*day, todayDay+day, minAnalyze); err != nil {
		return nil, err
	}
	if s.NewToday, err = st.CountInstallsNew(todayDay); err != nil {
		return nil, err
	}
	if s.NewWeek, err = st.CountInstallsNew(todayDay - 6*day); err != nil {
		return nil, err
	}

	// Lifetime event totals (from the permanent kind counter, not the raw
	// events table which is pruned after ~14 days).
	if s.TotalEvents, err = lifetimeEvents(st); err != nil {
		return nil, err
	}

	if err = buildDistributions(st, fromMs, toMs+day, s); err != nil {
		return nil, err
	}

	if s.OnlineByHour, err = st.OnlineByHourBetween(fromMs, toMs+day, minAnalyze); err != nil {
		return nil, err
	}
	for h, c := range s.OnlineByHour {
		if c > s.PeakOnline {
			s.PeakOnline = c
			s.PeakOnlineHour = h
		}
	}

	if err = buildTrend(st, fromMs, toMs, minAnalyze, s); err != nil {
		return nil, err
	}
	var activeSum int64
	for _, d := range s.ActiveTrend {
		activeSum += d.Active
	}
	if len(s.ActiveTrend) > 0 {
		s.AvgDailyActive = activeSum / int64(len(s.ActiveTrend))
	}

	s.WindowFrom = time.UnixMilli(fromMs).UTC().Format("2006-01-02")
	s.WindowTo = time.UnixMilli(toMs).UTC().Format("2006-01-02")

	return s, nil
}

// dataStart returns the earliest day with statistics as "YYYY-MM-DD".
func dataStart(st *store.Store) string {
	d, err := st.DataStartDay()
	if err != nil || d == 0 {
		return time.Now().UTC().Format("2006-01-02")
	}
	return time.UnixMilli(d).UTC().Format("2006-01-02")
}

// lifetimeEvents sums the permanent per-kind counters.
func lifetimeEvents(st *store.Store) (int64, error) {
	totals, err := st.KindTotals()
	if err != nil {
		return 0, err
	}
	var n int64
	for _, c := range totals {
		n += c
	}
	return n, nil
}

// buildTrend builds the daily active/new series across the window. Windows of
// one day produce ActiveTrendHourly instead (24 hourly points).
func buildTrend(st *store.Store, fromMs, toMs int64, minAnalyze int, s *Stats) error {
	n := (toMs-fromMs)/store.DayMs + 1
	if n <= 1 {
		hours, err := st.ActiveHourlyBetween(fromMs, toMs+store.DayMs, minAnalyze)
		if err != nil {
			return err
		}
		for i := int64(0); i < 24; i++ {
			h := fromMs + i*store.HourMs
			s.ActiveTrendHourly = append(s.ActiveTrendHourly, TrendHour{Hour: h, Active: hours[h]})
		}
		return nil
	}

	active, err := st.ActivePerDayBetween(fromMs, toMs+store.DayMs, minAnalyze)
	if err != nil {
		return err
	}
	newCounts, err := st.NewPerDay(fromMs)
	if err != nil {
		return err
	}
	s.ActiveTrend = make([]TrendDay, 0, n)
	for i := int64(0); i < n; i++ {
		d := fromMs + i*store.DayMs
		s.ActiveTrend = append(s.ActiveTrend, TrendDay{
			Date:   time.UnixMilli(d).UTC().Format("2006-01-02"),
			Active: active[d],
			New:    newCounts[d],
		})
	}
	return nil
}

// buildDistributions reads every distribution from daily_agg and install_days.
// Straight-line queries over bounded tables — milliseconds, no full scans.
func buildDistributions(st *store.Store, startDayMs, endDayMs int64, s *Stats) error {
	rowCounts := func(dim string) (map[string]int64, error) {
		rows, err := st.DailyAggRows(startDayMs, endDayMs, dim)
		if err != nil {
			return nil, err
		}
		out := map[string]int64{}
		for _, r := range rows {
			out[r.Val] += r.Count
		}
		return out, nil
	}

	var err error
	if s.Algorithms, err = kv(rowCounts("alg")); err != nil {
		return err
	}
	if s.ActualAlgorithms, err = kv(rowCounts("actual")); err != nil {
		return err
	}
	if s.Keycounts, err = kv(rowCounts("key")); err != nil {
		return err
	}
	if s.Mods, err = kv(rowCounts("mod")); err != nil {
		return err
	}
	if s.Modes, err = kv(rowCounts("mode")); err != nil {
		return err
	}
	// Windowed analyze count (distributions already include it, but the
	// cards/chips show it explicitly).
	kindRows, err := st.DailyAggRows(startDayMs, endDayMs, "kind")
	if err != nil {
		return err
	}
	for _, r := range kindRows {
		if r.Val == "analyze" {
			s.AnalyzeCount = r.Count
			break
		}
	}

	versionCounts, err := st.VersionCounts()
	if err != nil {
		return err
	}
	s.Versions = sortedKV(versionCounts)

	starCounts, err := rowCounts("star")
	if err != nil {
		return err
	}
	lnCounts, err := rowCounts("ln")
	if err != nil {
		return err
	}
	numCounts, err := rowCounts("numeric")
	if err != nil {
		return err
	}
	durRows, err := st.DailyAggRows(startDayMs, endDayMs, "dur")
	if err != nil {
		return err
	}

	s.StarHistogram = histogram(parseFloatKeys(starCounts), func(k float64) string { return fmt.Sprintf("%.1f", k) })
	s.LnRatioHistogram = histogram(parsePercentKeys(lnCounts), func(k float64) string { return fmt.Sprintf("%.0f%%", k*100) })
	s.NumericHistogram = histogram(parseFloatKeys(numCounts), func(k float64) string { return fmt.Sprintf("%.2f", k) })
	s.AvgStar = weightedAvg(starCounts, func(k string) (float64, bool) {
		f, err := strconv.ParseFloat(k, 64)
		return f, err == nil
	})
	s.AvgLnRatio = weightedAvg(lnCounts, func(k string) (float64, bool) {
		f, err := strconv.ParseFloat(strings.TrimSuffix(k, "%"), 64)
		return f / 100, err == nil
	})

	// Duration: exact average from sum/count, percentiles from the 30s-bin
	// CDF (error <= half a bin, deterministic, window-exact).
	s.DurationHistogram = durationHistogram(durRows)
	s.DurationStats = durationStats(durRows)

	// Plays per install per day (install_days.analyze_count).
	playFreq, err := st.PlayFreqBetween(startDayMs, endDayMs)
	if err != nil {
		return err
	}
	s.PlayFreq = playFreqKV(playFreq)

	return nil
}

func kv(m map[string]int64, err error) ([]KV, error) {
	if err != nil {
		return nil, err
	}
	return sortedKV(m), nil
}

// weightedAvg computes value*count/count from string-keyed per-day counts.
// Keys that cannot be parsed are skipped (bin labels only).
func weightedAvg(m map[string]int64, parse func(string) (float64, bool)) float64 {
	var sum float64
	var n int64
	for k, v := range m {
		if f, ok := parse(k); ok {
			sum += f * float64(v)
			n += v
		}
	}
	if n == 0 {
		return 0
	}
	return math.Round(sum/float64(n)*100) / 100
}

func playFreqKV(m map[int64]int64) []KV {
	buckets := []struct {
		label    string
		min, max int64
	}{
		{"1", 1, 1},
		{"2-3", 2, 3},
		{"4-9", 4, 9},
		{"10-19", 10, 19},
		{"20-49", 20, 49},
		{"50+", 50, math.MaxInt64},
	}
	out := make([]KV, 0, len(buckets))
	var total int64
	for _, n := range m {
		total += n
	}
	for _, b := range buckets {
		var c int64
		for k, n := range m {
			if k >= b.min && k <= b.max {
				c += n
			}
		}
		pct := 0.0
		if total > 0 {
			pct = math.Round(float64(c)/float64(total)*1000) / 10
		}
		out = append(out, KV{Key: b.label, Count: c, Pct: pct})
	}
	return out
}

func durationHistogram(rows []store.AggRow) []Bucket {
	sort.Slice(rows, func(i, j int) bool {
		return store.DurBinStart(rows[i].Val) < store.DurBinStart(rows[j].Val)
	})
	out := make([]Bucket, 0, len(rows))
	for _, r := range rows {
		if r.Count == 0 {
			continue
		}
		out = append(out, Bucket{Key: r.Val + "ms", Value: float64(store.DurBinStart(r.Val)), Count: r.Count})
	}
	return out
}

func durationStats(rows []store.AggRow) DurationStats {
	var total int64
	var sum float64
	for _, r := range rows {
		total += r.Count
		sum += r.SumVal
	}
	if total == 0 {
		return DurationStats{}
	}
	stats := DurationStats{AvgMs: int64(sum) / total}
	stats.P50Ms = durationPercentile(rows, 50, total)
	stats.P90Ms = durationPercentile(rows, 90, total)
	return stats
}

// durationPercentile walks the sorted 30s-bin CDF and interpolates inside the
// crossing bin. The overflow bin ("7200+") saturates at its start.
func durationPercentile(rows []store.AggRow, p float64, total int64) int64 {
	sort.Slice(rows, func(i, j int) bool {
		return store.DurBinStart(rows[i].Val) < store.DurBinStart(rows[j].Val)
	})
	target := float64(p) / 100 * float64(total)
	var cum int64
	for _, r := range rows {
		start := store.DurBinStart(r.Val)
		if float64(cum+int64(r.Count)) >= target {
			if r.Val == store.DurOverflowV {
				return start
			}
			frac := (target - float64(cum)) / float64(r.Count)
			if frac < 0 {
				frac = 0
			}
			return start + int64(frac*float64(store.DurBinMs))
		}
		cum += int64(r.Count)
	}
	return 0
}

func sortedKV(m map[string]int64) []KV {
	var total int64
	for _, v := range m {
		total += v
	}
	out := make([]KV, 0, len(m))
	for k, v := range m {
		pct := 0.0
		if total > 0 {
			pct = math.Round(float64(v)/float64(total)*1000) / 10
		}
		out = append(out, KV{Key: k, Count: v, Pct: pct})
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

func parseFloatKeys(m map[string]int64) map[float64]int64 {
	out := map[float64]int64{}
	for k, v := range m {
		if f, err := strconv.ParseFloat(k, 64); err == nil {
			out[f] = v
		}
	}
	return out
}

func parsePercentKeys(m map[string]int64) map[float64]int64 {
	out := map[float64]int64{}
	for k, v := range m {
		if f, err := strconv.ParseFloat(strings.TrimSuffix(k, "%"), 64); err == nil {
			out[f/100] = v
		}
	}
	return out
}
