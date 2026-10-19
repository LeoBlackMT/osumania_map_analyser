package feedback

import (
	"fmt"
	"math"
	"sort"
	"time"

	"osumania-telemetry/internal/store"
)

// MapAggRow is one row of feedback_map_agg.
type MapAggRow struct {
	Bid             int64   `json:"bid"`
	Checksum        string  `json:"checksum"`
	Rate            float64 `json:"rate"`
	ActualAlgorithm string  `json:"actual_algorithm"`
	Mods            string  `json:"mods"`
	KeyCount        int     `json:"key_count"`
	Pattern         string  `json:"pattern"`
	LnBucket        string  `json:"ln_bucket"`
	N               int     `json:"n"`
	WeightSum       float64 `json:"weight_sum"`
	Distinct        int     `json:"distinct_installs"`
	Median          float64 `json:"target_median"`
	Q25             float64 `json:"target_q25"`
	Q75             float64 `json:"target_q75"`
	IQR             float64 `json:"iqr"`
	UpdatedAt       int64   `json:"updated_at"`
}

type sample struct {
	target float64
	weight float64
	token  string
}

type mapGroup struct {
	key     string
	row     MapAggRow
	samples []sample
	seen    map[string]bool
}

// RefreshAll rebuilds both aggregate tables.
func RefreshAll(st *store.FeedbackStore, minN int, trim float64) error {
	if err := refreshMapAgg(st, minN, trim); err != nil {
		return err
	}
	return refreshFeatureDaily(st)
}

func refreshMapAgg(st *store.FeedbackStore, minN int, trim float64) error {
	rows, err := st.DB().Query(`
SELECT bid, checksum, rate, actual_algorithm, mods, key_count, pattern, ln_bucket,
       target, weight, dedup_token
FROM feedback_events
ORDER BY bid, checksum, rate, actual_algorithm, mods`)
	if err != nil {
		return err
	}
	defer rows.Close()

	groups := make(map[string]*mapGroup)
	order := make([]string, 0)
	for rows.Next() {
		var (
			bid, keyCount                         int64
			checksum, algo, mods, pattern, bucket string
			rate, target, weight                  float64
			token                                 string
		)
		if err := rows.Scan(&bid, &checksum, &rate, &algo, &mods, &keyCount, &pattern, &bucket,
			&target, &weight, &token); err != nil {
			return err
		}
		// 分组键必须含 bid 与 rate：checksum 可能被两个不同 bid 共用，
		// 同一张图的 NM / DT 也是两次不同的游玩（不能混成一个中位数）。
		key := fmt.Sprintf("%d|%s|%s|%s|%.3f", bid, checksum, algo, mods, rate)
		group := groups[key]
		if group == nil {
			group = &mapGroup{
				key: key,
				row: MapAggRow{
					Bid: bid, Checksum: checksum, Rate: rate, ActualAlgorithm: algo,
					Mods: mods, KeyCount: int(keyCount), Pattern: pattern, LnBucket: bucket,
				},
				seen: make(map[string]bool),
			}
			groups[key] = group
			order = append(order, key)
		}
		group.samples = append(group.samples, sample{target: target, weight: weight, token: token})
		group.seen[token] = true
	}
	if err := rows.Err(); err != nil {
		return err
	}

	out := make([]MapAggRow, 0, len(order))
	now := time.Now().UnixMilli()
	for _, key := range order {
		group := groups[key]
		if len(group.samples) < minN {
			continue
		}
		targets := make([]float64, 0, len(group.samples))
		for _, s := range group.samples {
			targets = append(targets, s.target)
		}
		sort.Float64s(targets)
		row := group.row
		row.N = len(group.samples)
		row.Distinct = len(group.seen)
		row.Median = trimmedWeightedMedian(group.samples, trim)
		row.Q25 = quantile(targets, 0.25)
		row.Q75 = quantile(targets, 0.75)
		row.IQR = math.Round((row.Q75-row.Q25)*100) / 100
		row.WeightSum = math.Round(sumWeights(group.samples)*1000) / 1000
		row.UpdatedAt = now
		out = append(out, row)
	}

	tx, err := st.DB().Begin()
	if err != nil {
		return err
	}
	defer tx.Rollback()
	if _, err := tx.Exec("DELETE FROM feedback_map_agg"); err != nil {
		return err
	}
	stmt, err := tx.Prepare(`
INSERT INTO feedback_map_agg(
	bid, checksum, rate, actual_algorithm, mods, key_count, pattern, ln_bucket,
	n, weight_sum, distinct_installs, target_median, target_q25, target_q75, updated_at
) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)`)
	if err != nil {
		return err
	}
	defer stmt.Close()
	for _, row := range out {
		if _, err := stmt.Exec(row.Bid, row.Checksum, row.Rate, row.ActualAlgorithm, row.Mods,
			row.KeyCount, row.Pattern, row.LnBucket, row.N, row.WeightSum, row.Distinct,
			row.Median, row.Q25, row.Q75, row.UpdatedAt); err != nil {
			return err
		}
	}
	return tx.Commit()
}

type featureGroup struct {
	day      int64
	modeTag  string
	pattern  string
	keyCount int
	lnBucket string
	odBucket string
	algo     string
	n        int
	weight   float64
	vote     float64
	weighted float64
	min      float64
	max      float64
}

func refreshFeatureDaily(st *store.FeedbackStore) error {
	rows, err := st.DB().Query(`
SELECT day, mode_tag, pattern, key_count, ln_bucket, COALESCE(od_bucket,'unknown'),
       actual_algorithm, target, weight, vote
FROM feedback_events`)
	if err != nil {
		return err
	}
	defer rows.Close()

	groups := make(map[string]*featureGroup)
	for rows.Next() {
		var (
			day, keyCount                              int64
			modeTag, pattern, lnBucket, odBucket, algo string
			target, weight, vote                       float64
		)
		if err := rows.Scan(&day, &modeTag, &pattern, &keyCount, &lnBucket, &odBucket, &algo,
			&target, &weight, &vote); err != nil {
			return err
		}
		key := fmt.Sprintf("%d|%s|%s|%d|%s|%s|%s", day, modeTag, pattern, keyCount, lnBucket, odBucket, algo)
		g := groups[key]
		if g == nil {
			g = &featureGroup{
				day: day, modeTag: modeTag, pattern: pattern, keyCount: int(keyCount),
				lnBucket: lnBucket, odBucket: odBucket, algo: algo,
				min: target, max: target,
			}
			groups[key] = g
		}
		g.n++
		g.weight += weight
		g.vote += vote
		g.weighted += target * weight
		if target < g.min {
			g.min = target
		}
		if target > g.max {
			g.max = target
		}
	}
	if err := rows.Err(); err != nil {
		return err
	}

	tx, err := st.DB().Begin()
	if err != nil {
		return err
	}
	defer tx.Rollback()
	if _, err := tx.Exec("DELETE FROM feedback_feature_daily"); err != nil {
		return err
	}
	stmt, err := tx.Prepare(`
INSERT INTO feedback_feature_daily(
	day, mode_tag, pattern, key_count, ln_bucket, od_bucket, actual_algorithm,
	n, weight_sum, vote_sum, target_weighted_sum, target_min, target_max, updated_at
) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?)`)
	if err != nil {
		return err
	}
	defer stmt.Close()
	now := time.Now().UnixMilli()
	for _, g := range groups {
		if _, err := stmt.Exec(g.day, g.modeTag, g.pattern, g.keyCount, g.lnBucket, g.odBucket,
			g.algo, g.n, g.weight, g.vote, g.weighted, g.min, g.max, now); err != nil {
			return err
		}
	}
	return tx.Commit()
}

func sumWeights(samples []sample) float64 {
	total := 0.0
	for _, s := range samples {
		total += s.weight
	}
	return total
}

// trimmedWeightedMedian sorts by target, drops `trim` of each tail, then takes
// the weighted median of the remaining samples.
func trimmedWeightedMedian(samples []sample, trim float64) float64 {
	ordered := make([]sample, len(samples))
	copy(ordered, samples)
	sort.Slice(ordered, func(i, j int) bool { return ordered[i].target < ordered[j].target })

	cut := int(math.Floor(float64(len(ordered)) * trim))
	if cut > 0 && len(ordered)-2*cut >= 1 {
		ordered = ordered[cut : len(ordered)-cut]
	}
	total := sumWeights(ordered)
	if total <= 0 {
		return ordered[len(ordered)/2].target
	}
	half := total / 2
	acc := 0.0
	for _, s := range ordered {
		acc += s.weight
		if acc >= half {
			return math.Round(s.target*100) / 100
		}
	}
	return math.Round(ordered[len(ordered)-1].target*100) / 100
}

func quantile(sorted []float64, q float64) float64 {
	if len(sorted) == 0 {
		return 0
	}
	if len(sorted) == 1 {
		return sorted[0]
	}
	pos := q * float64(len(sorted)-1)
	lower := int(math.Floor(pos))
	upper := int(math.Ceil(pos))
	if lower == upper {
		return sorted[lower]
	}
	frac := pos - float64(lower)
	return sorted[lower] + (sorted[upper]-sorted[lower])*frac
}

// MapAggRows loads the map aggregate (newest first) for the admin page.
func MapAggRows(st *store.FeedbackStore, limit int) ([]MapAggRow, error) {
	query := `
SELECT bid, checksum, rate, actual_algorithm, mods, key_count, pattern, ln_bucket,
       n, weight_sum, distinct_installs, target_median, target_q25, target_q75, updated_at
FROM feedback_map_agg
ORDER BY n DESC, bid ASC`
	args := []any{}
	if limit > 0 {
		query += " LIMIT ?"
		args = append(args, limit)
	}
	rows, err := st.DB().Query(query, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := make([]MapAggRow, 0)
	for rows.Next() {
		var row MapAggRow
		if err := rows.Scan(&row.Bid, &row.Checksum, &row.Rate, &row.ActualAlgorithm, &row.Mods,
			&row.KeyCount, &row.Pattern, &row.LnBucket, &row.N, &row.WeightSum, &row.Distinct,
			&row.Median, &row.Q25, &row.Q75, &row.UpdatedAt); err != nil {
			return nil, err
		}
		row.IQR = math.Round((row.Q75-row.Q25)*100) / 100
		out = append(out, row)
	}
	return out, rows.Err()
}

// RejectCount is one row of the rejection distribution.
type RejectCount struct {
	Day    int64  `json:"day"`
	Reason string `json:"reason"`
	Count  int64  `json:"count"`
}

// RejectRows returns recent rejection counters.
func RejectRows(st *store.FeedbackStore, sinceDay int64) ([]RejectCount, error) {
	rows, err := st.DB().Query(
		"SELECT day, reason, count FROM feedback_rejects WHERE day>=? ORDER BY day DESC, count DESC",
		sinceDay)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := make([]RejectCount, 0)
	for rows.Next() {
		var row RejectCount
		if err := rows.Scan(&row.Day, &row.Reason, &row.Count); err != nil {
			return nil, err
		}
		out = append(out, row)
	}
	return out, rows.Err()
}

// Totals is the admin overview.
type Totals struct {
	Events        int64 `json:"events"`
	Maps          int64 `json:"maps"`
	DistinctDays  int64 `json:"distinct_days"`
	AggRows       int64 `json:"agg_rows"`
	CandidateRows int64 `json:"candidate_rows"`
	Rejects       int64 `json:"rejects"`
}

// LoadTotals computes the overview counters.
func LoadTotals(st *store.FeedbackStore, minN, minDistinct int, minDelta float64) (Totals, error) {
	var t Totals
	db := st.DB()
	if err := db.QueryRow("SELECT COUNT(*) FROM feedback_events").Scan(&t.Events); err != nil {
		return t, err
	}
	if err := db.QueryRow("SELECT COUNT(DISTINCT bid) FROM feedback_events").Scan(&t.Maps); err != nil {
		return t, err
	}
	if err := db.QueryRow("SELECT COUNT(DISTINCT day) FROM feedback_events").Scan(&t.DistinctDays); err != nil {
		return t, err
	}
	if err := db.QueryRow("SELECT COUNT(*) FROM feedback_map_agg").Scan(&t.AggRows); err != nil {
		return t, err
	}
	candidates, err := CandidateRows(st, minN, minDistinct, minDelta, 1000000)
	if err != nil {
		return t, err
	}
	t.CandidateRows = int64(len(candidates))
	if err := db.QueryRow("SELECT COALESCE(SUM(count),0) FROM feedback_rejects").Scan(&t.Rejects); err != nil {
		return t, err
	}
	return t, nil
}

// FeatureRow is one row of feedback_feature_daily.
type FeatureRow struct {
	Day       int64   `json:"day"`
	ModeTag   string  `json:"mode_tag"`
	Pattern   string  `json:"pattern"`
	KeyCount  int     `json:"key_count"`
	LnBucket  string  `json:"ln_bucket"`
	ODBucket  string  `json:"od_bucket"`
	Algorithm string  `json:"actual_algorithm"`
	N         int     `json:"n"`
	WeightSum float64 `json:"weight_sum"`
	VoteSum   float64 `json:"vote_sum"`
	TargetSum float64 `json:"target_weighted_sum"`
	Min       float64 `json:"target_min"`
	Max       float64 `json:"target_max"`
}

// FeatureRows returns feature aggregates since a day (epoch ms).
func FeatureRows(st *store.FeedbackStore, sinceDay int64) ([]FeatureRow, error) {
	rows, err := st.DB().Query(`
SELECT day, mode_tag, pattern, key_count, ln_bucket, od_bucket, actual_algorithm,
       n, weight_sum, vote_sum, target_weighted_sum, target_min, target_max
FROM feedback_feature_daily WHERE day>=? ORDER BY day DESC, n DESC`, sinceDay)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := make([]FeatureRow, 0)
	for rows.Next() {
		var row FeatureRow
		if err := rows.Scan(&row.Day, &row.ModeTag, &row.Pattern, &row.KeyCount, &row.LnBucket,
			&row.ODBucket, &row.Algorithm, &row.N, &row.WeightSum, &row.VoteSum, &row.TargetSum,
			&row.Min, &row.Max); err != nil {
			return nil, err
		}
		out = append(out, row)
	}
	return out, rows.Err()
}

// CandidateRows returns maps whose consensus differs from the displayed value
// by at least minDelta. `displayed_val` is not stored in agg, so the query
// joins the most recent event for the same key.
func CandidateRows(st *store.FeedbackStore, minN, minDistinct int, minDelta float64, limit int) ([]MapAggRow, error) {
	query := `
SELECT a.bid, a.checksum, a.rate, a.actual_algorithm, a.mods, a.key_count, a.pattern, a.ln_bucket,
       a.n, a.weight_sum, a.distinct_installs, a.target_median, a.target_q25, a.target_q75, a.updated_at
FROM feedback_map_agg a
JOIN (
	SELECT bid, checksum, rate, actual_algorithm, mods, displayed_val,
	       ROW_NUMBER() OVER (PARTITION BY bid, checksum, rate, actual_algorithm, mods ORDER BY created_at DESC) AS rn
	FROM feedback_events
) e ON e.bid=a.bid AND e.checksum=a.checksum AND e.rate=a.rate
     AND e.actual_algorithm=a.actual_algorithm AND e.mods=a.mods AND e.rn=1
WHERE a.n>=? AND a.distinct_installs>=? AND ABS(a.target_median-e.displayed_val)>=?
ORDER BY ABS(a.target_median-e.displayed_val) DESC, a.n DESC
LIMIT ?`
	rows, err := st.DB().Query(query, minN, minDistinct, minDelta, limit)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := make([]MapAggRow, 0)
	for rows.Next() {
		var row MapAggRow
		if err := rows.Scan(&row.Bid, &row.Checksum, &row.Rate, &row.ActualAlgorithm, &row.Mods,
			&row.KeyCount, &row.Pattern, &row.LnBucket, &row.N, &row.WeightSum, &row.Distinct,
			&row.Median, &row.Q25, &row.Q75, &row.UpdatedAt); err != nil {
			return nil, err
		}
		row.IQR = math.Round((row.Q75-row.Q25)*100) / 100
		out = append(out, row)
	}
	return out, rows.Err()
}
