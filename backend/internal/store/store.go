// Package store owns the SQLite persistence layer (installs + events).
package store

import (
	"database/sql"
	"strings"

	_ "modernc.org/sqlite"
)

type Store struct {
	db *sql.DB
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
CREATE INDEX IF NOT EXISTS idx_events_install_ts ON events(install_id, ts);
CREATE INDEX IF NOT EXISTS idx_events_kind_ts ON events(kind, ts);
CREATE INDEX IF NOT EXISTS idx_events_ts ON events(ts);
`

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
	if _, err := db.Exec(schema); err != nil {
		db.Close()
		return nil, err
	}
	return &Store{db: db}, nil
}

func (s *Store) Close() error { return s.db.Close() }

// RecordEvent upserts the install row and appends one event. `dataJSON` must
// already be a valid JSON object string.
func (s *Store) RecordEvent(installID, kind, version, dataJSON string, ts int64) error {
	if _, err := s.db.Exec(
		`INSERT INTO installs (id, first_seen, last_seen, version)
		 VALUES (?, ?, ?, ?)
		 ON CONFLICT(id) DO UPDATE SET last_seen=excluded.last_seen, version=excluded.version`,
		installID, ts, ts, version,
	); err != nil {
		return err
	}
	_, err := s.db.Exec(
		`INSERT INTO events (install_id, kind, ts, data) VALUES (?, ?, ?, ?)`,
		installID, kind, ts, dataJSON,
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

func (s *Store) CountActiveSince(since int64) (int64, error) {
	var n int64
	err := s.db.QueryRow(`SELECT COUNT(DISTINCT install_id) FROM events WHERE ts >= ?`, since).Scan(&n)
	return n, err
}

func (s *Store) CountEvents() (int64, error) {
	var n int64
	err := s.db.QueryRow(`SELECT COUNT(*) FROM events`).Scan(&n)
	return n, err
}

func (s *Store) CountAnalyzeSince(since int64) (int64, error) {
	var n int64
	err := s.db.QueryRow(`SELECT COUNT(*) FROM events WHERE kind='analyze' AND ts >= ?`, since).Scan(&n)
	return n, err
}

// OnlineByHour returns, for each hour-of-day (0-23, UTC), how many distinct
// installs had an event in that hour across the whole `since` window.
func (s *Store) OnlineByHour(since int64) ([24]int64, error) {
	var out [24]int64
	rows, err := s.db.Query(
		`SELECT (ts/3600000) % 24 AS h, COUNT(DISTINCT install_id) FROM events WHERE ts >= ? GROUP BY h`,
		since,
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

// ActivePerDay returns distinct installs per UTC day (keyed by day epoch).
func (s *Store) ActivePerDay(since int64) (map[int64]int64, error) {
	return s.dayCounts(
		`SELECT ts/86400000 AS d, COUNT(DISTINCT install_id) FROM events WHERE ts >= ? GROUP BY d`,
		since,
	)
}

// NewPerDay returns new installs per UTC day (keyed by day epoch).
func (s *Store) NewPerDay(since int64) (map[int64]int64, error) {
	return s.dayCounts(
		`SELECT first_seen/86400000 AS d, COUNT(*) FROM installs WHERE first_seen >= ? GROUP BY d`,
		since,
	)
}

func (s *Store) dayCounts(query string, since int64) (map[int64]int64, error) {
	out := map[int64]int64{}
	rows, err := s.db.Query(query, since)
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

// VersionCounts returns install counts grouped by plugin version.
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

// ScanAnalyzeData calls fn for each analyze event's data JSON within range.
func (s *Store) ScanAnalyzeData(since int64, fn func(dataJSON string) error) error {
	rows, err := s.db.Query(`SELECT data FROM events WHERE kind='analyze' AND ts >= ?`, since)
	if err != nil {
		return err
	}
	defer rows.Close()
	for rows.Next() {
		var data string
		if err := rows.Scan(&data); err != nil {
			return err
		}
		if err := fn(data); err != nil {
			return err
		}
	}
	return rows.Err()
}

func (s *Store) DeleteEventsBefore(cutoff int64) (int64, error) {
	res, err := s.db.Exec(`DELETE FROM events WHERE ts < ?`, cutoff)
	if err != nil {
		return 0, err
	}
	return res.RowsAffected()
}

// SnapshotTo writes a consistent copy of the database to path via VACUUM INTO.
func (s *Store) SnapshotTo(path string) error {
	escaped := strings.ReplaceAll(path, "'", "''")
	_, err := s.db.Exec("VACUUM INTO '" + escaped + "'")
	return err
}
