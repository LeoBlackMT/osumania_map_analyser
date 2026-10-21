// Feedback store: a separate SQLite database (`feedback.db`) so RC ratings have
// their own schema, write lock, retention, and backup object.
//
// No install_id is ever stored: only the HMAC dedup token derived from it.
package store

import (
	"database/sql"
	"errors"
	"strings"
	"time"

	_ "modernc.org/sqlite"
)

// ErrFeedbackDuplicate means the same install rated the same map within the
// dedup window.
var ErrFeedbackDuplicate = errors.New("feedback duplicate")

// ErrNonceReused means the (key_id, nonce) pair was already accepted.
var ErrNonceReused = errors.New("nonce reused")

// FeedbackEvent is one accepted rating.
type FeedbackEvent struct {
	Bid               int64
	Checksum          string
	Rate              float64
	Mods              string // JSON array, sorted
	KeyCount          int
	LnRatio           float64
	LnBucket          string
	Pattern           string
	ModeTag           string
	TargetSide        string
	OD                sql.NullFloat64
	ODBucket          string
	Star              sql.NullFloat64
	ActualAlgorithm   string
	SelectedAlgorithm string
	DisplayedVal      float64
	Vote              float64
	Weight            float64
	WeightVersion     int
	Target            float64
	Passed            bool
	Client            string
	Classic           bool
	PluginVersion     string
	SettingsHash      string
	DedupToken        string
	KeyID             string
	Nonce             string
	Timestamp         int64
	Day               int64
	CreatedAt         int64
}

// BeatmapValidation is the cached osu! API result for one beatmap.
type BeatmapValidation struct {
	Bid       int64
	Checksum  string
	Mode      int
	Convert   bool
	CS        float64
	OD        float64
	Star      float64
	CheckedAt int64
}

// FeedbackStore owns the feedback database handle.
type FeedbackStore struct {
	db     *sql.DB
	dbPath string
}

const feedbackSchema = `
CREATE TABLE IF NOT EXISTS feedback_events (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	bid INTEGER NOT NULL,
	checksum TEXT NOT NULL,
	rate REAL NOT NULL,
	mods TEXT NOT NULL,
	key_count INTEGER NOT NULL,
	ln_ratio REAL NOT NULL,
	ln_bucket TEXT NOT NULL,
	pattern TEXT NOT NULL,
	mode_tag TEXT NOT NULL,
	target_side TEXT NOT NULL DEFAULT 'RC',
	od REAL,
	od_bucket TEXT,
	star REAL,
	actual_algorithm TEXT NOT NULL,
	selected_algorithm TEXT NOT NULL,
	displayed_val REAL NOT NULL,
	vote REAL NOT NULL,
	weight REAL NOT NULL,
	weight_version INTEGER NOT NULL,
	target REAL NOT NULL,
	passed INTEGER NOT NULL,
	client TEXT NOT NULL,
	classic INTEGER NOT NULL,
	plugin_version TEXT NOT NULL,
	settings_hash TEXT,
	dedup_token TEXT NOT NULL,
	key_id TEXT NOT NULL,
	nonce TEXT NOT NULL,
	timestamp INTEGER NOT NULL,
	day INTEGER NOT NULL,
	created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_fb_group ON feedback_events(bid, checksum, rate, actual_algorithm);
CREATE INDEX IF NOT EXISTS idx_fb_dedup ON feedback_events(dedup_token, bid, rate, created_at);
CREATE INDEX IF NOT EXISTS idx_fb_feature ON feedback_events(day, mode_tag, pattern, key_count, ln_bucket);
CREATE INDEX IF NOT EXISTS idx_fb_time ON feedback_events(created_at);

CREATE TABLE IF NOT EXISTS feedback_map_agg (
	bid INTEGER NOT NULL,
	checksum TEXT NOT NULL,
	rate REAL NOT NULL,
	actual_algorithm TEXT NOT NULL,
	mods TEXT NOT NULL,
	key_count INTEGER NOT NULL,
	pattern TEXT NOT NULL,
	ln_bucket TEXT NOT NULL,
	target_side TEXT NOT NULL DEFAULT 'RC',
	n INTEGER NOT NULL,
	weight_sum REAL NOT NULL,
	distinct_installs INTEGER NOT NULL,
	target_median REAL NOT NULL,
	target_q25 REAL NOT NULL,
	target_q75 REAL NOT NULL,
	updated_at INTEGER NOT NULL,
	PRIMARY KEY (bid, checksum, rate, actual_algorithm, mods)
);

CREATE TABLE IF NOT EXISTS feedback_feature_daily (
	day INTEGER NOT NULL,
	mode_tag TEXT NOT NULL,
	pattern TEXT NOT NULL,
	key_count INTEGER NOT NULL,
	ln_bucket TEXT NOT NULL,
	od_bucket TEXT NOT NULL,
	actual_algorithm TEXT NOT NULL,
	n INTEGER NOT NULL,
	weight_sum REAL NOT NULL,
	vote_sum REAL NOT NULL,
	target_weighted_sum REAL NOT NULL,
	target_min REAL NOT NULL,
	target_max REAL NOT NULL,
	updated_at INTEGER NOT NULL,
	PRIMARY KEY (day, mode_tag, pattern, key_count, ln_bucket, od_bucket, actual_algorithm)
);

CREATE TABLE IF NOT EXISTS feedback_rejects (
	day INTEGER NOT NULL,
	reason TEXT NOT NULL,
	count INTEGER NOT NULL DEFAULT 0,
	PRIMARY KEY (day, reason)
);

CREATE TABLE IF NOT EXISTS beatmap_validation (
	bid INTEGER PRIMARY KEY,
	checksum TEXT NOT NULL,
	mode INTEGER NOT NULL,
	convert INTEGER NOT NULL,
	cs REAL,
	od REAL,
	star REAL,
	checked_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS beatmap_validation_negative (
	bid INTEGER PRIMARY KEY,
	status TEXT NOT NULL,
	checked_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS feedback_nonces (
	key_id TEXT NOT NULL,
	nonce TEXT NOT NULL,
	created_at INTEGER NOT NULL,
	PRIMARY KEY (key_id, nonce)
);
`

// OpenFeedback opens (and migrates) the feedback database.
func OpenFeedback(path string) (*FeedbackStore, error) {
	db, err := sql.Open("sqlite", path)
	if err != nil {
		return nil, err
	}
	db.SetMaxOpenConns(1)
	for _, pragma := range []string{
		"PRAGMA journal_mode=WAL",
		"PRAGMA synchronous=NORMAL",
		"PRAGMA busy_timeout=5000",
	} {
		if _, err := db.Exec(pragma); err != nil {
			db.Close()
			return nil, err
		}
	}
	if _, err := db.Exec(feedbackSchema); err != nil {
		db.Close()
		return nil, err
	}
	// Existing feedback.db files predate target_side. SQLite has no IF NOT EXISTS
	// for ADD COLUMN; duplicate-column errors are the only expected failure here.
	if _, err := db.Exec(`ALTER TABLE feedback_events ADD COLUMN target_side TEXT NOT NULL DEFAULT 'RC'`); err != nil &&
		!strings.Contains(strings.ToLower(err.Error()), "duplicate column") {
		db.Close()
		return nil, err
	}
	if _, err := db.Exec(`ALTER TABLE feedback_map_agg ADD COLUMN target_side TEXT NOT NULL DEFAULT 'RC'`); err != nil &&
		!strings.Contains(strings.ToLower(err.Error()), "duplicate column") {
		db.Close()
		return nil, err
	}
	if _, err := db.Exec(`CREATE INDEX IF NOT EXISTS idx_fb_dedup_side ON feedback_events(dedup_token, bid, rate, target_side, created_at)`); err != nil {
		db.Close()
		return nil, err
	}
	return &FeedbackStore{db: db, dbPath: path}, nil
}

func (s *FeedbackStore) Close() error { return s.db.Close() }

// DB exposes the handle for snapshots (backup).
func (s *FeedbackStore) DB() *sql.DB { return s.db }

// SnapshotTo writes a consistent copy via VACUUM INTO.
func (s *FeedbackStore) SnapshotTo(path string) error {
	escaped := strings.ReplaceAll(path, "'", "''")
	_, err := s.db.Exec("VACUUM INTO '" + escaped + "'")
	return err
}

// Path reports the database file path.
func (s *FeedbackStore) Path() string { return s.dbPath }

// InsertNonce records a nonce; returns ErrNonceReused when present.
func (s *FeedbackStore) InsertNonce(keyID, nonce string, now int64) error {
	_, err := s.db.Exec(
		"INSERT INTO feedback_nonces(key_id, nonce, created_at) VALUES(?,?,?)",
		keyID, nonce, now,
	)
	if err != nil {
		if isUniqueViolation(err) {
			return ErrNonceReused
		}
		return err
	}
	return nil
}

func isUniqueViolation(err error) bool {
	if err == nil {
		return false
	}
	text := err.Error()
	return strings.Contains(text, "UNIQUE constraint failed") || strings.Contains(text, "constraint failed: UNIQUE")
}

// HasRecentDedup reports whether the token already rated this map/rate/side inside the window.
func (s *FeedbackStore) HasRecentDedup(token string, bid int64, rate float64, targetSide string, since int64) (bool, error) {
	var one int
	err := s.db.QueryRow(
		"SELECT 1 FROM feedback_events WHERE dedup_token=? AND bid=? AND rate=? AND target_side=? AND created_at>=? LIMIT 1",
		token, bid, rate, targetSide, since,
	).Scan(&one)
	if errors.Is(err, sql.ErrNoRows) {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	return true, nil
}

// InsertFeedback writes one accepted event.
func (s *FeedbackStore) InsertFeedback(ev FeedbackEvent) error {
	_, err := s.db.Exec(`
INSERT INTO feedback_events(
	bid, checksum, rate, mods, key_count, ln_ratio, ln_bucket, pattern, mode_tag, target_side,
	od, od_bucket, star, actual_algorithm, selected_algorithm, displayed_val, vote,
	weight, weight_version, target, passed, client, classic, plugin_version,
	settings_hash, dedup_token, key_id, nonce, timestamp, day, created_at
) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)`,
		ev.Bid, ev.Checksum, ev.Rate, ev.Mods, ev.KeyCount, ev.LnRatio, ev.LnBucket,
		ev.Pattern, ev.ModeTag, ev.TargetSide, ev.OD, ev.ODBucket, ev.Star, ev.ActualAlgorithm,
		ev.SelectedAlgorithm, ev.DisplayedVal, ev.Vote, ev.Weight, ev.WeightVersion,
		ev.Target, boolInt(ev.Passed), ev.Client, boolInt(ev.Classic), ev.PluginVersion,
		ev.SettingsHash, ev.DedupToken, ev.KeyID, ev.Nonce, ev.Timestamp, ev.Day, ev.CreatedAt,
	)
	return err
}

func boolInt(v bool) int {
	if v {
		return 1
	}
	return 0
}

// RecordReject increments the per-day rejection counter.
func (s *FeedbackStore) RecordReject(day int64, reason string) {
	_, _ = s.db.Exec(`
INSERT INTO feedback_rejects(day, reason, count) VALUES(?,?,1)
ON CONFLICT(day, reason) DO UPDATE SET count = count + 1`, day, reason)
}

// GetBeatmapValidation returns a cached osu! API result.
func (s *FeedbackStore) GetBeatmapValidation(bid int64) (BeatmapValidation, bool, error) {
	var v BeatmapValidation
	var convert int
	err := s.db.QueryRow(`
SELECT bid, checksum, mode, convert, COALESCE(cs,0), COALESCE(od,0), COALESCE(star,0), checked_at
FROM beatmap_validation WHERE bid=?`, bid,
	).Scan(&v.Bid, &v.Checksum, &v.Mode, &convert, &v.CS, &v.OD, &v.Star, &v.CheckedAt)
	if errors.Is(err, sql.ErrNoRows) {
		return BeatmapValidation{}, false, nil
	}
	if err != nil {
		return BeatmapValidation{}, false, err
	}
	v.Convert = convert != 0
	return v, true, nil
}

// PutBeatmapValidation caches a positive result.
func (s *FeedbackStore) PutBeatmapValidation(v BeatmapValidation) error {
	_, err := s.db.Exec(`
INSERT INTO beatmap_validation(bid, checksum, mode, convert, cs, od, star, checked_at)
VALUES(?,?,?,?,?,?,?,?)
ON CONFLICT(bid) DO UPDATE SET checksum=excluded.checksum, mode=excluded.mode,
	convert=excluded.convert, cs=excluded.cs, od=excluded.od, star=excluded.star,
	checked_at=excluded.checked_at`,
		v.Bid, v.Checksum, v.Mode, boolInt(v.Convert), v.CS, v.OD, v.Star, v.CheckedAt,
	)
	return err
}

// GetNegative returns the cached negative status and its timestamp.
func (s *FeedbackStore) GetNegative(bid int64) (string, int64, bool, error) {
	var status string
	var at int64
	err := s.db.QueryRow(
		"SELECT status, checked_at FROM beatmap_validation_negative WHERE bid=?", bid,
	).Scan(&status, &at)
	if errors.Is(err, sql.ErrNoRows) {
		return "", 0, false, nil
	}
	if err != nil {
		return "", 0, false, err
	}
	return status, at, true, nil
}

// PutNegative caches a negative result.
func (s *FeedbackStore) PutNegative(bid int64, status string, at int64) error {
	_, err := s.db.Exec(`
INSERT INTO beatmap_validation_negative(bid, status, checked_at) VALUES(?,?,?)
ON CONFLICT(bid) DO UPDATE SET status=excluded.status, checked_at=excluded.checked_at`,
		bid, status, at,
	)
	return err
}

// Prune deletes expired nonces, old raw events, and stale agg rows.
//
// ⚠️ `now` 是**毫秒**（与 `created_at`/`checked_at` 一致）：所有窗口都必须先换成
// 毫秒再相减。历史实现写成 `now - days*86400`（秒），导致 180 天保留实际只有
// 4.3 小时、nonce 去重只有 0.6 秒——线上会每天静默丢光反馈事件。
func (s *FeedbackStore) Prune(now int64, rawRetentionDays int, nonceMinutes int) (int64, error) {
	const dayMs = int64(24 * 60 * 60 * 1000)
	const minuteMs = int64(60 * 1000)
	var deleted int64
	res, err := s.db.Exec("DELETE FROM feedback_nonces WHERE created_at < ?", now-int64(nonceMinutes)*minuteMs)
	if err != nil {
		return 0, err
	}
	n, _ := res.RowsAffected()
	deleted += n
	if rawRetentionDays > 0 {
		res, err = s.db.Exec("DELETE FROM feedback_events WHERE created_at < ?",
			now-int64(rawRetentionDays)*dayMs)
		if err != nil {
			return deleted, err
		}
		n, _ = res.RowsAffected()
		deleted += n
		res, err = s.db.Exec("DELETE FROM beatmap_validation WHERE checked_at < ?", now-90*dayMs)
		if err != nil {
			return deleted, err
		}
		n, _ = res.RowsAffected()
		deleted += n
		res, err = s.db.Exec("DELETE FROM beatmap_validation_negative WHERE checked_at < ?", now-30*dayMs)
		if err != nil {
			return deleted, err
		}
		n, _ = res.RowsAffected()
		deleted += n
	}
	return deleted, nil
}

// DayStartMs truncates an epoch-millisecond timestamp to its UTC day.
func DayStartMs(ms int64) int64 {
	return ms / DayMs * DayMs
}

// NowMs is a small helper for callers.
func NowMs() int64 { return time.Now().UnixMilli() }
