// Package store - clean.go provides one-shot cleanup utilities to remove
// historical contaminated dimensions and extreme debug outliers from daily_agg
// and installs tables.
package store

import (
	"fmt"
	"strings"

	"osumania-telemetry/internal/spec"
)

type CleanReport struct {
	DeletedAlgRows    int64
	MergedCapsuleRows int64
	DeletedActualRows int64
	DeletedDurRows    int64
	DeletedInstalls   int64
}

// CleanLegacyData purges legacy contaminated records and debug anomalies from the database.
func (s *Store) CleanLegacyData() (*CleanReport, error) {
	tx, err := s.db.Begin()
	if err != nil {
		return nil, err
	}
	defer tx.Rollback()

	rep := &CleanReport{}

	// 1. Delete unauthorized alg rows
	algPlaceholders := make([]string, 0, len(spec.AllowedAlgorithms))
	algArgs := make([]interface{}, 0, len(spec.AllowedAlgorithms))
	for k := range spec.AllowedAlgorithms {
		algPlaceholders = append(algPlaceholders, "?")
		algArgs = append(algArgs, k)
	}
	delAlgQuery := fmt.Sprintf(
		"DELETE FROM daily_agg WHERE dim = 'alg' AND val NOT IN (%s)",
		strings.Join(algPlaceholders, ","),
	)
	res, err := tx.Exec(delAlgQuery, algArgs...)
	if err != nil {
		return nil, fmt.Errorf("clean alg: %w", err)
	}
	rep.DeletedAlgRows, _ = res.RowsAffected()

	// 2. Merge display capsule "Azusa+Companella" into "Azusa" in daily_agg
	rows, err := tx.Query(`SELECT day, count, sum_val, max_val, min_val FROM daily_agg WHERE dim = 'actual' AND val = 'Azusa+Companella'`)
	if err == nil {
		type aggRow struct {
			day    int64
			count  int64
			sumVal float64
			maxVal float64
			minVal float64
		}
		var capsuleRows []aggRow
		for rows.Next() {
			var r aggRow
			if err := rows.Scan(&r.day, &r.count, &r.sumVal, &r.maxVal, &r.minVal); err == nil {
				capsuleRows = append(capsuleRows, r)
			}
		}
		rows.Close()

		for _, r := range capsuleRows {
			if _, err := tx.Exec(
				`INSERT INTO daily_agg (day, dim, val, count, sum_val, max_val, min_val) VALUES (?, 'actual', 'Azusa', ?, ?, ?, ?)
				 ON CONFLICT(day, dim, val) DO UPDATE SET
				   count = count + excluded.count, sum_val = sum_val + excluded.sum_val,
				   max_val = MAX(max_val, excluded.max_val), min_val = MIN(min_val, excluded.min_val)`,
				r.day, r.count, r.sumVal, r.maxVal, r.minVal,
			); err == nil {
				rep.MergedCapsuleRows++
			}
		}
		tx.Exec(`DELETE FROM daily_agg WHERE dim = 'actual' AND val = 'Azusa+Companella'`)
	}

	// 3. Delete unauthorized actual rows
	actualPlaceholders := make([]string, 0, len(spec.AllowedActualAlgorithms))
	actualArgs := make([]interface{}, 0, len(spec.AllowedActualAlgorithms))
	for k := range spec.AllowedActualAlgorithms {
		actualPlaceholders = append(actualPlaceholders, "?")
		actualArgs = append(actualArgs, k)
	}
	delActualQuery := fmt.Sprintf(
		"DELETE FROM daily_agg WHERE dim = 'actual' AND val NOT IN (%s)",
		strings.Join(actualPlaceholders, ","),
	)
	res, err = tx.Exec(delActualQuery, actualArgs...)
	if err != nil {
		return nil, fmt.Errorf("clean actual: %w", err)
	}
	rep.DeletedActualRows, _ = res.RowsAffected()

	// 4. Delete duration rows with debug anomaly values (> 30s)
	res, err = tx.Exec(`DELETE FROM daily_agg WHERE dim = 'dur' AND max_val > ?`, spec.MaxComputeDurationMs)
	if err != nil {
		return nil, fmt.Errorf("clean dur: %w", err)
	}
	rep.DeletedDurRows, _ = res.RowsAffected()

	// 5. Purge installs with unknown or dev/test versions
	res, err = tx.Exec(`DELETE FROM installs WHERE version = '' OR version = 'unknown' OR version LIKE '%dev%' OR version LIKE '%test%' OR version LIKE '%dirty%' OR version LIKE '%debug%'`)
	if err != nil {
		return nil, fmt.Errorf("clean installs: %w", err)
	}
	rep.DeletedInstalls, _ = res.RowsAffected()

	if err := tx.Commit(); err != nil {
		return nil, err
	}

	_, _ = s.db.Exec("VACUUM")
	return rep, nil
}
