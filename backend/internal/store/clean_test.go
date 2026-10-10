package store

import (
	"path/filepath"
	"testing"
)

func TestCleanLegacyData(t *testing.T) {
	dbPath := filepath.Join(t.TempDir(), "test_clean.db")
	st, err := Open(dbPath)
	if err != nil {
		t.Fatalf("Open: %v", err)
	}
	defer st.Close()

	// 1. Insert dirty algorithm and extreme duration rows
	day := int64(1700000000000)
	tx, err := st.db.Begin()
	if err != nil {
		t.Fatalf("Begin: %v", err)
	}
	// Valid
	tx.Exec(`INSERT INTO daily_agg (day, dim, val, count, sum_val, max_val, min_val) VALUES (?, 'alg', 'Mixed', 10, 0, 0, 0)`, day)
	tx.Exec(`INSERT INTO daily_agg (day, dim, val, count, sum_val, max_val, min_val) VALUES (?, 'actual', 'Sunny', 5, 0, 0, 0)`, day)
	tx.Exec(`INSERT INTO daily_agg (day, dim, val, count, sum_val, max_val, min_val) VALUES (?, 'actual', 'Azusa', 5, 0, 0, 0)`, day)
	tx.Exec(`INSERT INTO daily_agg (day, dim, val, count, sum_val, max_val, min_val) VALUES (?, 'dur', '100', 5, 500, 120, 90)`, day)

	// Dirty
	tx.Exec(`INSERT INTO daily_agg (day, dim, val, count, sum_val, max_val, min_val) VALUES (?, 'alg', 'BabyDan', 3, 0, 0, 0)`, day)
	tx.Exec(`INSERT INTO daily_agg (day, dim, val, count, sum_val, max_val, min_val) VALUES (?, 'actual', 'BabyRice', 8, 0, 0, 0)`, day)
	tx.Exec(`INSERT INTO daily_agg (day, dim, val, count, sum_val, max_val, min_val) VALUES (?, 'actual', 'Azusa+Companella', 2, 0, 0, 0)`, day)
	tx.Exec(`INSERT INTO daily_agg (day, dim, val, count, sum_val, max_val, min_val) VALUES (?, 'dur', '5000+', 1, 41619785, 41619785, 41619785)`, day)

	// Installs
	tx.Exec(`INSERT INTO installs (id, first_seen, last_seen, version) VALUES ('inst-1', 1, 1, '2.1.0')`)
	tx.Exec(`INSERT INTO installs (id, first_seen, last_seen, version) VALUES ('inst-2', 1, 1, 'unknown')`)
	tx.Exec(`INSERT INTO installs (id, first_seen, last_seen, version) VALUES ('inst-3', 1, 1, '2.1.0-dev')`)
	if err := tx.Commit(); err != nil {
		t.Fatalf("Commit: %v", err)
	}

	rep, err := st.CleanLegacyData()
	if err != nil {
		t.Fatalf("CleanLegacyData: %v", err)
	}

	if rep.DeletedAlgRows != 1 {
		t.Errorf("expected 1 deleted alg row, got %d", rep.DeletedAlgRows)
	}
	if rep.DeletedActualRows != 1 {
		t.Errorf("expected 1 deleted actual row (BabyRice), got %d", rep.DeletedActualRows)
	}
	if rep.MergedCapsuleRows != 1 {
		t.Errorf("expected 1 merged capsule row, got %d", rep.MergedCapsuleRows)
	}
	if rep.DeletedDurRows != 1 {
		t.Errorf("expected 1 deleted dur anomaly row, got %d", rep.DeletedDurRows)
	}
	if rep.DeletedInstalls != 2 {
		t.Errorf("expected 2 deleted installs, got %d", rep.DeletedInstalls)
	}

	// Verify Azusa count increased from 5 to 7
	var azusaCount int64
	st.db.QueryRow(`SELECT count FROM daily_agg WHERE dim = 'actual' AND val = 'Azusa'`).Scan(&azusaCount)
	if azusaCount != 7 {
		t.Errorf("expected Azusa count to be 7, got %d", azusaCount)
	}

	// Verify dirty rows no longer exist
	var babyRiceCount int64
	st.db.QueryRow(`SELECT COUNT(*) FROM daily_agg WHERE val IN ('BabyDan', 'BabyRice', 'Azusa+Companella')`).Scan(&babyRiceCount)
	if babyRiceCount != 0 {
		t.Errorf("expected 0 dirty rows left, got %d", babyRiceCount)
	}
}
