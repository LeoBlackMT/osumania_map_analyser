// Aggregate computation shared by the ingest write path and Migrate.
//
// Every aggregation decision lives here so the two rebuild paths (live event,
// replayed event) can never diverge.
package store

import (
	"encoding/json"
	"fmt"
	"math"
)

// AggInc is one daily_agg counter increment.
type AggInc struct {
	Day    int64
	Dim    string
	Val    string
	Count  int64
	SumVal float64
	MaxVal float64 // per-event value (window max via MAX over day rows)
	MinVal float64 // per-event value (window min via MIN over day rows)
}

// Duration binning for the analysis-duration metric (plugin sends
// performance.now() - start, i.e. milliseconds of compute time). 20ms bins
// up to 5s, then an overflow bin ("5000+"). The first bin is split at 10ms:
//
//	"0"  = 0-<10ms  — the result-cache-hit bin (plugin skips fetch/estimators
//	                  on a cache hit, so these analyses finish in a few ms)
//	"10" = 10-<20ms — a half-width bin so the display can hide <10ms data
//	                  without swallowing 10-19ms (cacheHitPct = bin "0" / total)
//	"20","40",...   — regular 20ms bins
//
// The overflow bin accumulates real sum/max/min values so the average and
// the extreme chips stay exact; only percentiles saturate at the overflow.
const (
	DurBinMs      = 20
	DurCacheMs    = 10 // cache-hit threshold: durations below this are hits
	DurOverflowMs = 5000
	DurOverflowV  = "5000+"
)

// durationBin returns the bin label for a duration in ms.
func durationBin(durMs int64) string {
	if durMs >= DurOverflowMs {
		return DurOverflowV
	}
	if durMs < DurCacheMs {
		return "0"
	}
	if durMs < DurBinMs {
		return fmt.Sprintf("%d", DurCacheMs)
	}
	return fmt.Sprintf("%d", durMs/DurBinMs*DurBinMs)
}

// durationBinStart returns the bin start (ms) for a bin label.
func durationBinStart(val string) int64 {
	if val == DurOverflowV {
		return DurOverflowMs
	}
	var ms int64
	if _, err := fmt.Sscanf(val, "%d", &ms); err != nil {
		return 0
	}
	return ms
}

// ParseData unmarshals an events.data JSON object; malformed rows yield nil.
func ParseData(dataJSON string) map[string]interface{} {
	var m map[string]interface{}
	if err := json.Unmarshal([]byte(dataJSON), &m); err != nil {
		return nil
	}
	return m
}

// AnalyzeAggIncs computes the daily_agg increments for one analyze event.
// Dimension inventory (v3):
//
//	alg/actual/key/mod/mode  count distributions
//	star/ln/numeric          binned histograms (count = events, sum_val = raw)
//	dur                      duration histogram 20ms bins (percentiles via CDF)
func AnalyzeAggIncs(data map[string]interface{}, ts int64) []AggInc {
	if data == nil {
		return nil
	}
	day := ts / DayMs * DayMs
	incs := make([]AggInc, 0, 12)
	inc := func(dim, val string, sum float64) {
		incs = append(incs, AggInc{Day: day, Dim: dim, Val: val, Count: 1, SumVal: sum, MaxVal: sum, MinVal: sum})
	}
	num := func(k string) (float64, bool) {
		v, ok := data[k].(float64)
		return v, ok
	}

	if alg, _ := data["algorithm"].(string); alg != "" {
		inc("alg", alg, 0)
	}
	if actual, _ := data["actualAlgorithm"].(string); actual != "" && actual != "Mixed" {
		inc("actual", actual, 0)
	}
	if kc, ok := num("keycount"); ok && kc > 0 {
		inc("key", fmt.Sprintf("%dK", int(kc)), 0)
	}
	if mods, _ := data["mods"].([]interface{}); len(mods) == 0 {
		inc("mod", "NM", 0)
	} else {
		for _, m := range mods {
			if ms, ok := m.(string); ok && ms != "" {
				inc("mod", ms, 0)
			}
		}
	}
	if mode, _ := data["mode"].(string); mode != "" {
		inc("mode", mode, 0)
	}
	if client, _ := data["client"].(string); client != "" {
		inc("client", client, 0)
	}

	// Star histogram: 0.5★ bins; sum_val keeps raw star for the exact average.
	if star, ok := num("star"); ok && star > 0 && star <= 20 {
		bin := math.Round(star*2) / 2
		inc("star", fmt.Sprintf("%.1f", bin), star)
	}
	// LN ratio histogram: 5% bins (<5% collapses into "0%").
	if ln, ok := num("lnRatio"); ok && ln >= 0 && ln <= 1 {
		bin := math.Round(ln*20) / 20
		inc("ln", fmt.Sprintf("%.0f%%", bin*100), ln)
	}
	// Numeric difficulty histogram: 0.25 bins.
	if num, ok := num("numericDifficulty"); ok && num >= -3 && num <= 25 {
		bin := math.Round(num*4) / 4
		inc("numeric", fmt.Sprintf("%.2f", bin), num)
	}
	// Duration: 20ms bins (0/<10ms = cache hits); count + real sum per bin
	// (exact avg/max/min, CDF percentiles).
	if dur, ok := num("durationMs"); ok && dur >= 0 {
		inc("dur", durationBin(int64(dur)), dur)
	}
	// Extremes dim ("ext"): window min/max WITHOUT the histogram bounds, so
	// whatever the client sent (including outliers beyond the display caps,
	// e.g. a 1200★ anomaly) is still reflected in the extreme chips. The
	// histograms/averages keep their legacy bounds and stay readable.
	if star, ok := num("star"); ok && star > 0 && star < 1e6 {
		inc("ext", "star", star)
	}
	if nv, ok := num("numericDifficulty"); ok && nv >= -3 && nv < 1e6 {
		inc("ext", "numeric", nv)
	}
	return incs
}

// DurBinStart exposes bin starts to analytics (percentile CDF / histogram).
func DurBinStart(val string) int64 { return durationBinStart(val) }
