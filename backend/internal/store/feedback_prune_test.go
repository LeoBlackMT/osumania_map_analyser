package store

import (
	"path/filepath"
	"testing"
	"time"
)

// Regression: Prune's windows are all in milliseconds. A historical unit mix
// (now_ms - days*86400) trimmed 180 days down to ~4.3 hours and nonces down to
// 0.6 seconds, silently deleting accepted feedback and breaking replay
// protection.
func TestPruneUsesMillisecondWindows(t *testing.T) {
	st, err := OpenFeedback(filepath.Join(t.TempDir(), "feedback.db"))
	if err != nil {
		t.Fatalf("open: %v", err)
	}
	defer st.Close()

	now := time.Now().UnixMilli()
	day := int64(24 * 60 * 60 * 1000)
	insert := func(bid int64, createdAt int64) {
		t.Helper()
		err := st.InsertFeedback(FeedbackEvent{
			Bid:             bid,
			Checksum:        "7c9e6679742540de944bbed07fc1f90a",
			Rate:            1,
			ActualAlgorithm: "Roxy",
			DisplayedVal:    10,
			Vote:            0,
			Weight:          1,
			WeightVersion:   3,
			Target:          10,
			Passed:          true,
			Client:          "lazer",
			DedupToken:      "token",
			Day:             createdAt,
			CreatedAt:       createdAt,
		})
		if err != nil {
			t.Fatalf("insert %d: %v", bid, err)
		}
	}
	insert(1, now-1*day)   // within 180 days → keep
	insert(2, now-200*day) // older → delete
	insert(3, now-4*60*60*1000) // 4 hours old: the old bug deleted this

	for _, nonce := range []struct {
		id string
		at int64
	}{
		{"fresh", now - 5*60*1000},   // 5 min → keep (10 min window)
		{"stale", now - 30*60*1000},  // 30 min → delete
	} {
		if err := st.InsertNonce("dev", nonce.id, nonce.at); err != nil {
			t.Fatalf("nonce %s: %v", nonce.id, err)
		}
	}

	if _, err := st.Prune(now, 180, 10); err != nil {
		t.Fatalf("prune: %v", err)
	}

	var events int
	if err := st.DB().QueryRow("SELECT COUNT(*) FROM feedback_events").Scan(&events); err != nil {
		t.Fatalf("count events: %v", err)
	}
	if events != 2 {
		t.Fatalf("events kept = %d, want 2 (1 day + 4 hours old)", events)
	}
	var kept4h int
	if err := st.DB().QueryRow("SELECT COUNT(*) FROM feedback_events WHERE bid=3").Scan(&kept4h); err != nil {
		t.Fatalf("count bid=3: %v", err)
	}
	if kept4h != 1 {
		t.Fatalf("4-hour-old event was pruned; the 180-day window is shrinking again")
	}
	var nonces int
	if err := st.DB().QueryRow("SELECT COUNT(*) FROM feedback_nonces").Scan(&nonces); err != nil {
		t.Fatalf("count nonces: %v", err)
	}
	if nonces != 1 {
		t.Fatalf("nonces kept = %d, want 1 (5 min old; 10 min window)", nonces)
	}
}
