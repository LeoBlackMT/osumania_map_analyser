// Package store owns the SQLite persistence layer.
//
// Layout after the v3 redesign (1.1.0): the `events` table is a bounded raw
// log (debug / re-derivation source) and is NEVER read by the dashboard.
// Every dashboard query reads the small write-path aggregates:
//
//	daily_agg     per-day counters per (dim, val) — distributions & averages
//	install_days  one row per (install, day) with analyze_count — active counts,
//	              trends, plays-per-day distribution ("active" = >= ACTIVE_MIN
//	              analyze events that day)
//	install_hours one row per (install, hour) — 24h-of-day distribution and
//	              hourly trend for short windows (rolling ~90 days)
package store

import (
	"database/sql"
	"os"
	"strings"
	"time"

	_ "modernc.org/sqlite"
)

// Time units (epoch milliseconds, truncated).
const (
	DayMs  = int64(24 * 60 * 60 * 1000)
	HourMs = int64(60 * 60 * 1000)
)

type Store struct {
	db     *sql.DB
	dbPath string
}

const schema = `
CREATE TABLE IF NOT EXISTS installs (
	id TEXT PRIMARY KEY,
	first_seen INTEGER NOT NULL,
	last_seen INTEGER NOT NULL,
	version TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS events (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	install_id TEXT NOT NULL,
	kind TEXT NOT NULL,
	ts INTEGER NOT NULL,
	data TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX IF NOT EXISTS idx_events_ts ON events(ts);

-- Pre-aggregated per-day counters, updated inside the same transaction as the
-- event insert. Summing this tiny table answers every distribution query;
-- nobody scans the events table for statistics. max_val/min_val keep the
-- per-row extrema (MIN/MAX over day rows = the window extrema, no raw data).
CREATE TABLE IF NOT EXISTS daily_agg (
	day INTEGER NOT NULL,
	dim TEXT NOT NULL,
	val TEXT NOT NULL,
	count INTEGER NOT NULL DEFAULT 0,
	sum_val REAL NOT NULL DEFAULT 0,
	max_val REAL NOT NULL DEFAULT 0,
	min_val REAL NOT NULL DEFAULT 0,
	PRIMARY KEY (day, dim, val)
);

-- Per-install daily presence with analyze event count.
CREATE TABLE IF NOT EXISTS install_days (
	install_id TEXT NOT NULL,
	day INTEGER NOT NULL,
	analyze_count INTEGER NOT NULL DEFAULT 0,
	PRIMARY KEY (install_id, day)
);
CREATE INDEX IF NOT EXISTS idx_install_days_day ON install_days(day);

-- Per-install hourly presence (any event kind), rolling retention ~90 days.
CREATE TABLE IF NOT EXISTS install_hours (
	install_id TEXT NOT NULL,
	hour INTEGER NOT NULL,
	PRIMARY KEY (install_id, hour)
);
CREATE INDEX IF NOT EXISTS idx_install_hours_hour ON install_hours(hour);
`

// legacyIndexes are dropped on open: nothing queries (install_id, ts) or
// (kind, ts) anymore, and every extra index costs write speed and disk space.
var legacyIndexes = []string{
	"idx_events_install_ts",
	"idx_events_kind_ts",
}

func Open(path string) (*Store, error) {
	db, err := sql.Open("sqlite", path)
	if err != nil {
		return nil, err
	}
	// A single connection keeps SQLite's write semantics simple and safe.
	db.SetMaxOpenConns(1)
	if _, err := db.Exec("PRAGMA busy_timeout=5000"); err != nil {
		db.Close()
		return nil, err
	}
	// WAL improves read concurrency when stats queries overlap writes.
	db.Exec("PRAGMA journal_mode=WAL")
	if _, err := db.Exec(schema); err != nil {
		db.Close()
		return nil, err
	}
	for _, idx := range legacyIndexes {
		db.Exec("DROP INDEX IF EXISTS " + idx)
	}
	// 1.1.0: max_val/min_val columns were added for the window-extreme chips.
	// Pre-1.1.0 databases get them via ALTER; the values are rebuilt by
	// `telemetry-server -migrate` (a rebuild resets them to the true extrema).
	if err := ensureDailyAggExtremes(db); err != nil {
		db.Close()
		return nil, err
	}
	return &Store{db: db, dbPath: path}, nil
}

// ensureDailyAggExtremes adds missing max_val/min_val columns to existing
// daily_agg tables (idempotent: checks each column individually).
func ensureDailyAggExtremes(db *sql.DB) error {
	cols := map[string]bool{}
	rows, err := db.Query(`PRAGMA table_info(daily_agg)`)
	if err != nil {
		return err
	}
	for rows.Next() {
		var cid int
		var name, ctype string
		var notnull, pk int
		var dflt sql.NullString
		if err := rows.Scan(&cid, &name, &ctype, &notnull, &dflt, &pk); err != nil {
			rows.Close()
			return err
		}
		cols[name] = true
	}
	if err := rows.Err(); err != nil {
		rows.Close()
		return err
	}
	rows.Close()
	if !cols["max_val"] {
		if _, err := db.Exec(`ALTER TABLE daily_agg ADD COLUMN max_val REAL NOT NULL DEFAULT 0`); err != nil {
			return err
		}
	}
	if !cols["min_val"] {
		if _, err := db.Exec(`ALTER TABLE daily_agg ADD COLUMN min_val REAL NOT NULL DEFAULT 0`); err != nil {
			return err
		}
	}
	return nil
}

func (s *Store) Close() error { return s.db.Close() }

// RecordEvent upserts the install row, appends one raw event, and applies all
// write-path aggregates — all in one transaction (one fsync per event).
// `dataJSON` is the whitelisted JSON object string persisted in events.data;
// `data` is the same payload as a map, used for aggregation.
func (s *Store) RecordEvent(installID, kind, version, dataJSON string, data map[string]interface{}, ts int64) error {
	tx, err := s.db.Begin()
	if err != nil {
		return err
	}
	if _, err := tx.Exec(
		`INSERT INTO installs (id, first_seen, last_seen, version)
		 VALUES (?, ?, ?, ?)
		 ON CONFLICT(id) DO UPDATE SET last_seen=excluded.last_seen, version=excluded.version`,
		installID, ts, ts, version,
	); err != nil {
		tx.Rollback()
		return err
	}
	if _, err := tx.Exec(
		`INSERT INTO events (install_id, kind, ts, data) VALUES (?, ?, ?, ?)`,
		installID, kind, ts, dataJSON,
	); err != nil {
		tx.Rollback()
		return err
	}
	if err := applyAggTx(tx, installID, kind, data, ts); err != nil {
		tx.Rollback()
		return err
	}
	return tx.Commit()
}

// applyAggTx writes the aggregate side of one event into an open transaction.
// Shared by the ingest path and by Migrate (which replays existing events).
func applyAggTx(tx *sql.Tx, installID, kind string, data map[string]interface{}, ts int64) error {
	day := ts / DayMs * DayMs

	// Lifetime event counter per kind (survives raw-event pruning).
	if _, err := tx.Exec(
		`INSERT INTO daily_agg (day, dim, val, count, sum_val, max_val) VALUES (?, 'kind', ?, 1, 0, 0)
		 ON CONFLICT(day, dim, val) DO UPDATE SET count = count + excluded.count`,
		day, kind,
	); err != nil {
		return err
	}

	if kind == "analyze" {
		// Per-install daily presence + analyze count ("active" = day count >= min).
		if _, err := tx.Exec(
			`INSERT INTO install_days (install_id, day, analyze_count) VALUES (?, ?, 1)
			 ON CONFLICT(install_id, day) DO UPDATE SET analyze_count = analyze_count + 1`,
			installID, day,
		); err != nil {
			return err
		}
		for _, inc := range AnalyzeAggIncs(data, ts) {
			if _, err := tx.Exec(
				`INSERT INTO daily_agg (day, dim, val, count, sum_val, max_val, min_val) VALUES (?, ?, ?, ?, ?, ?, ?)
				 ON CONFLICT(day, dim, val) DO UPDATE SET
				   count = count + excluded.count, sum_val = sum_val + excluded.sum_val,
				   max_val = MAX(max_val, excluded.max_val), min_val = MIN(min_val, excluded.min_val)`,
				inc.Day, inc.Dim, inc.Val, inc.Count, inc.SumVal, inc.MaxVal, inc.MinVal,
			); err != nil {
				return err
			}
		}
	}

	// Per-install hourly presence (any kind) — feeds the 24h distribution and
	// short-window hourly trend.
	_, err := tx.Exec(
		`INSERT OR IGNORE INTO install_hours (install_id, hour) VALUES (?, ?)`,
		installID, ts/HourMs*HourMs,
	)
	return err
}

func (s *Store) CountInstalls() (int64, error) {
	var n int64
	err := s.db.QueryRow(`SELECT COUNT(*) FROM installs`).Scan(&n)
	return n, err
}

func (s *Store) CountInstallsOnline(since int64) (int64, error) {
	var n int64
	err := s.db.QueryRow(`SELECT COUNT(*) FROM installs WHERE last_seen >= ?`, since).Scan(&n)
	return n, err
}

func (s *Store) CountInstallsNew(since int64) (int64, error) {
	var n int64
	err := s.db.QueryRow(`SELECT COUNT(*) FROM installs WHERE first_seen >= ?`, since).Scan(&n)
	return n, err
}

// ActiveCountBetweenDays counts distinct installs with >= minAnalyze analyze
// events on any day in [startDayMs, endDayMs).
func (s *Store) ActiveCountBetweenDays(startDayMs, endDayMs int64, minAnalyze int) (int64, error) {
	var n int64
	err := s.db.QueryRow(
		`SELECT COUNT(DISTINCT install_id) FROM install_days WHERE day >= ? AND day < ? AND analyze_count >= ?`,
		startDayMs, endDayMs, minAnalyze,
	).Scan(&n)
	return n, err
}

// ActivePerDayBetween returns, per UTC day epoch (ms), how many installs had
// >= minAnalyze analyze events that day.
func (s *Store) ActivePerDayBetween(startDayMs, endDayMs int64, minAnalyze int) (map[int64]int64, error) {
	out := map[int64]int64{}
	rows, err := s.db.Query(
		`SELECT day, COUNT(*) FROM install_days WHERE day >= ? AND day < ? AND analyze_count >= ? GROUP BY day`,
		startDayMs, endDayMs, minAnalyze,
	)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	for rows.Next() {
		var d, c int64
		if err := rows.Scan(&d, &c); err != nil {
			return nil, err
		}
		out[d] = c
	}
	return out, rows.Err()
}

// NewPerDay returns new installs per UTC day (keyed by day epoch ms).
func (s *Store) NewPerDay(startDayMs int64) (map[int64]int64, error) {
	out := map[int64]int64{}
	rows, err := s.db.Query(
		`SELECT first_seen/86400000 AS d, COUNT(*) FROM installs WHERE first_seen >= ? GROUP BY d`,
		startDayMs,
	)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	for rows.Next() {
		var d, c int64
		if err := rows.Scan(&d, &c); err != nil {
			return nil, err
		}
		out[d*DayMs] = c
	}
	return out, rows.Err()
}

func (s *Store) VersionCounts() (map[string]int64, error) {
	out := map[string]int64{}
	rows, err := s.db.Query(`SELECT version, COUNT(*) FROM installs GROUP BY version`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	for rows.Next() {
		var v string
		var c int64
		if err := rows.Scan(&v, &c); err != nil {
			return nil, err
		}
		if v == "" {
			v = "unknown"
		}
		out[v] = c
	}
	return out, rows.Err()
}

// activeJoin is the common join used by install_hours queries: an install's
// hourly presence counts only on days where it is active (>= minAnalyze).
const activeJoin = `
JOIN install_days id ON id.install_id = ih.install_id AND id.day = ih.hour / 86400000 * 86400000
`

// OnlineByHourBetween returns, for each hour-of-day (0-23, UTC), how many
// active installs had an event in that hour within the window.
func (s *Store) OnlineByHourBetween(startMs, endMs int64, minAnalyze int) ([24]int64, error) {
	var out [24]int64
	rows, err := s.db.Query(
		`SELECT (ih.hour / 3600000) % 24 AS h, COUNT(DISTINCT ih.install_id)
		 FROM install_hours ih `+activeJoin+`
		 WHERE ih.hour >= ? AND ih.hour < ? AND id.analyze_count >= ?
		 GROUP BY h`,
		startMs, endMs, minAnalyze,
	)
	if err != nil {
		return out, err
	}
	defer rows.Close()
	for rows.Next() {
		var h, c int64
		if err := rows.Scan(&h, &c); err != nil {
			return out, err
		}
		if h >= 0 && h < 24 {
			out[h] = c
		}
	}
	return out, rows.Err()
}

// ActiveHourlyBetween returns active installs per UTC hour (epoch ms key) in
// the window — the trend line for windows of one day or less.
func (s *Store) ActiveHourlyBetween(startMs, endMs int64, minAnalyze int) (map[int64]int64, error) {
	out := map[int64]int64{}
	rows, err := s.db.Query(
		`SELECT ih.hour, COUNT(DISTINCT ih.install_id)
		 FROM install_hours ih `+activeJoin+`
		 WHERE ih.hour >= ? AND ih.hour < ? AND id.analyze_count >= ?
		 GROUP BY ih.hour`,
		startMs, endMs, minAnalyze,
	)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	for rows.Next() {
		var h, c int64
		if err := rows.Scan(&h, &c); err != nil {
			return nil, err
		}
		out[h] = c
	}
	return out, rows.Err()
}

// AggRow is one daily_agg row of a dimension.
type AggRow struct {
	Val    string
	Count  int64
	SumVal float64
	MaxVal float64
	MinVal float64
}

// DailyAggRows returns all (val, count, sum_val, max_val, min_val) rows of a
// dimension across a day range. Ordering is left to the caller (vals are
// labels, not numbers). max/min are the extrema of the per-day extrema — the
// window extrema.
func (s *Store) DailyAggRows(startDayMs, endDayMs int64, dim string) ([]AggRow, error) {
	rows, err := s.db.Query(
		`SELECT val, SUM(count), COALESCE(SUM(sum_val), 0), COALESCE(MAX(max_val), 0), COALESCE(MIN(min_val), 0) FROM daily_agg
		 WHERE day >= ? AND day < ? AND dim = ? GROUP BY val`,
		startDayMs, endDayMs, dim,
	)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []AggRow{}
	for rows.Next() {
		var r AggRow
		if err := rows.Scan(&r.Val, &r.Count, &r.SumVal, &r.MaxVal, &r.MinVal); err != nil {
			return nil, err
		}
		out = append(out, r)
	}
	return out, rows.Err()
}

// KindTotals returns lifetime event counts per kind (survives raw pruning).
func (s *Store) KindTotals() (map[string]int64, error) {
	out := map[string]int64{}
	rows, err := s.db.Query(`SELECT val, SUM(count) FROM daily_agg WHERE dim = 'kind' GROUP BY val`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	for rows.Next() {
		var v string
		var c int64
		if err := rows.Scan(&v, &c); err != nil {
			return nil, err
		}
		out[v] = c
	}
	return out, rows.Err()
}

// PlayFreqBetween returns, per analyze_count value, how many install-days
// reached it in the window (the "plays per install per day" distribution).
func (s *Store) PlayFreqBetween(startDayMs, endDayMs int64) (map[int64]int64, error) {
	out := map[int64]int64{}
	rows, err := s.db.Query(
		`SELECT analyze_count, COUNT(*) FROM install_days
		 WHERE day >= ? AND day < ? AND analyze_count > 0 GROUP BY analyze_count`,
		startDayMs, endDayMs,
	)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	for rows.Next() {
		var c, n int64
		if err := rows.Scan(&c, &n); err != nil {
			return nil, err
		}
		out[c] = n
	}
	return out, rows.Err()
}

// DataStartDay returns the first day (epoch ms) present in the aggregates —
// the earliest date any statistic can be queried for. 0 when empty.
func (s *Store) DataStartDay() (int64, error) {
	var d int64
	err := s.db.QueryRow(`SELECT COALESCE(MIN(day), 0) FROM daily_agg`).Scan(&d)
	return d, err
}

func (s *Store) DeleteEventsBefore(cutoff int64) (int64, error) {
	res, err := s.db.Exec(`DELETE FROM events WHERE ts < ?`, cutoff)
	if err != nil {
		return 0, err
	}
	return res.RowsAffected()
}

func (s *Store) DeleteHoursBefore(cutoff int64) (int64, error) {
	res, err := s.db.Exec(`DELETE FROM install_hours WHERE hour < ?`, cutoff)
	if err != nil {
		return 0, err
	}
	return res.RowsAffected()
}

// MigrateReport summarizes one run of Migrate.
type MigrateReport struct {
	EventsScanned int64
	AggRows       int64
	InstallDays   int64
	InstallHours  int64
	DeletedEvents int64
	SizeBefore    int64
	SizeAfter     int64
}

// Migrate rebuilds every aggregate from the raw events table (destroy-and-
// rebuild: idempotent by construction, meant to run once with the server
// stopped). Then, if retentionDays > 0, prunes raw events older than that and
// VACUUMs so the file shrinks.
//
// Strategy: kind/install_days/install_hours come from SQL GROUP BY over the
// raw table (no per-row overhead); the JSON payload dimensions are swept once
// in Go (single pass, in-memory accumulation — only a few thousand unique
// keys regardless of event volume) and bulk-inserted afterwards.
func (s *Store) Migrate(retentionDays int) (*MigrateReport, error) {
	rep := &MigrateReport{}
	if st, err := os.Stat(s.dbPath); err == nil {
		rep.SizeBefore = st.Size()
	}

	for _, t := range []string{"daily_agg", "install_days", "install_hours"} {
		if _, err := s.db.Exec("DELETE FROM " + t); err != nil {
			return nil, err
		}
	}

	// Lifetime per-kind counters.
	if _, err := s.db.Exec(
		`INSERT INTO daily_agg (day, dim, val, count, sum_val)
		 SELECT ts/86400000*86400000, 'kind', kind, COUNT(*), 0 FROM events GROUP BY 1, 3`,
	); err != nil {
		return nil, err
	}
	// Per-install daily analyze counts.
	if _, err := s.db.Exec(
		`INSERT INTO install_days (install_id, day, analyze_count)
		 SELECT install_id, ts/86400000*86400000, COUNT(*) FROM events
		 WHERE kind = 'analyze' GROUP BY 1, 2`,
	); err != nil {
		return nil, err
	}
	// Per-install hourly presence (any kind).
	if _, err := s.db.Exec(
		`INSERT INTO install_hours (install_id, hour)
		 SELECT DISTINCT install_id, ts/3600000*3600000 FROM events`,
	); err != nil {
		return nil, err
	}

	// JSON dimension sweep: accumulate increments per (day, dim, val).
	type aggKey struct {
		day int64
		dim string
		val string
	}
	type aggVal struct {
		count  int64
		sumVal float64
		maxVal float64
		minVal float64
	}
	acc := make(map[aggKey]*aggVal)
	rows, err := s.db.Query(`SELECT ts, data FROM events WHERE kind = 'analyze'`)
	if err != nil {
		return nil, err
	}
	for rows.Next() {
		var ts int64
		var dataJSON string
		if err := rows.Scan(&ts, &dataJSON); err != nil {
			rows.Close()
			return nil, err
		}
		rep.EventsScanned++
		for _, inc := range AnalyzeAggIncs(ParseData(dataJSON), ts) {
			k := aggKey{inc.Day, inc.Dim, inc.Val}
			if v, ok := acc[k]; ok {
				v.count += inc.Count
				v.sumVal += inc.SumVal
				if inc.MaxVal > v.maxVal {
					v.maxVal = inc.MaxVal
				}
				if inc.MinVal < v.minVal {
					v.minVal = inc.MinVal
				}
			} else {
				acc[k] = &aggVal{inc.Count, inc.SumVal, inc.MaxVal, inc.MinVal}
			}
		}
	}
	if err := rows.Err(); err != nil {
		rows.Close()
		return nil, err
	}
	rows.Close()

	// Bulk upsert of the accumulated dimension rows.
	tx, err := s.db.Begin()
	if err != nil {
		return nil, err
	}
	for k, v := range acc {
		if _, err := tx.Exec(
			`INSERT INTO daily_agg (day, dim, val, count, sum_val, max_val, min_val) VALUES (?, ?, ?, ?, ?, ?, ?)
			 ON CONFLICT(day, dim, val) DO UPDATE SET
			   count = count + excluded.count, sum_val = sum_val + excluded.sum_val,
			   max_val = MAX(max_val, excluded.max_val), min_val = MIN(min_val, excluded.min_val)`,
			k.day, k.dim, k.val, v.count, v.sumVal, v.maxVal, v.minVal,
		); err != nil {
			tx.Rollback()
			return nil, err
		}
	}
	if err := tx.Commit(); err != nil {
		return nil, err
	}

	var aggCount, daysCount, hoursCount int64
	aggCount = countRows(s.db, `SELECT COUNT(*) FROM daily_agg`)
	daysCount = countRows(s.db, `SELECT COUNT(*) FROM install_days`)
	hoursCount = countRows(s.db, `SELECT COUNT(*) FROM install_hours`)
	rep.AggRows, rep.InstallDays, rep.InstallHours = aggCount, daysCount, hoursCount

	if retentionDays > 0 {
		cutoff := time.Now().UnixMilli() - int64(retentionDays)*DayMs
		rep.DeletedEvents, err = s.DeleteEventsBefore(cutoff)
		if err != nil {
			return nil, err
		}
	}

	if _, err := s.db.Exec("VACUUM"); err != nil {
		return nil, err
	}
	if st, err := os.Stat(s.dbPath); err == nil {
		rep.SizeAfter = st.Size()
	}
	return rep, nil
}

func countRows(db *sql.DB, query string) int64 {
	var n int64
	db.QueryRow(query).Scan(&n)
	return n
}

// SnapshotTo writes a consistent copy of the database to path via VACUUM INTO.
func (s *Store) SnapshotTo(path string) error {
	escaped := strings.ReplaceAll(path, "'", "''")
	_, err := s.db.Exec("VACUUM INTO '" + escaped + "'")
	return err
}
