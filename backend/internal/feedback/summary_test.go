package feedback

import (
	"path/filepath"
	"testing"
	"time"

	"osumania-telemetry/internal/store"
)

// newSummaryStore opens a throwaway feedback DB for summary tests.
func newSummaryStore(t *testing.T) *store.FeedbackStore {
	t.Helper()
	st, err := store.OpenFeedback(filepath.Join(t.TempDir(), "feedback.db"))
	if err != nil {
		t.Fatalf("open feedback store: %v", err)
	}
	t.Cleanup(func() { _ = st.Close() })
	return st
}

func insertVote(t *testing.T, st *store.FeedbackStore, bid int64, algorithm string, vote, weight, target, displayed float64) {
	t.Helper()
	now := time.Now()
	day := store.DayStartMs(now.UnixMilli())
	event := store.FeedbackEvent{
		Bid:             bid,
		Checksum:        "7c9e6679742540de944bbed07fc1f90a",
		Rate:            1.5,
		Mods:            `["DT"]`,
		KeyCount:        4,
		Pattern:         "Jacks",
		ModeTag:         "RC",
		ActualAlgorithm: algorithm,
		DisplayedVal:    displayed,
		Vote:            vote,
		Weight:          weight,
		WeightVersion:   3,
		Target:          target,
		Passed:          true,
		Client:          "lazer",
		DedupToken:      "token-" + algorithm + "-" + time.Now().Format("150405.000000"),
		Day:             day,
		CreatedAt:       now.UnixMilli(),
		Timestamp:       now.Unix(),
	}
	if err := st.InsertFeedback(event); err != nil {
		t.Fatalf("insert: %v", err)
	}
}

func TestSummaryAggregatesAndHidesSmallGroups(t *testing.T) {
	st := newSummaryStore(t)
	// Roxy: 6 votes across 3 maps, all "harder" (+0.5) → residual +0.5.
	for i := int64(0); i < 6; i++ {
		insertVote(t, st, 100+i%3, "Roxy", 0.5, 1.0, 12.5, 12.0)
	}
	// Azusa: only 2 votes (below N>=5) → must not appear.
	for i := int64(0); i < 2; i++ {
		insertVote(t, st, 200+i, "Azusa", -0.5, 1.0, 11.5, 12.0)
	}

	summary, err := BuildSummary(st, 30, time.Now())
	if err != nil {
		t.Fatalf("summary: %v", err)
	}
	if summary.Totals.Votes != 8 {
		t.Fatalf("votes = %d, want 8", summary.Totals.Votes)
	}
	if summary.Totals.Maps != 5 {
		t.Fatalf("maps = %d, want 5", summary.Totals.Maps)
	}
	if len(summary.Daily) != 1 || summary.Daily[0].Votes != 8 {
		t.Fatalf("daily = %+v, want one day with 8", summary.Daily)
	}
	if len(summary.Algorithms) != 1 || summary.Algorithms[0].Name != "Roxy" {
		t.Fatalf("algorithms = %+v, want only Roxy (Azusa is below the threshold)", summary.Algorithms)
	}
	if summary.Algorithms[0].N != 6 || summary.Algorithms[0].Maps != 3 {
		t.Fatalf("roxy bucket = %+v", summary.Algorithms[0])
	}
	if got := summary.Algorithms[0].MeanResidual; got < 0.49 || got > 0.51 {
		t.Fatalf("roxy residual = %v, want ≈ +0.5", got)
	}
	if summary.Totals.MeanResidual < 0.2 || summary.Totals.MeanResidual > 0.4 {
		t.Fatalf("overall residual = %v, want between Azusa and Roxy", summary.Totals.MeanResidual)
	}
	// 2 votes at -0.5 and 6 at +0.5 → mean vote = (3 - 1) / 8 = 0.25.
	if summary.Totals.MeanVote < 0.24 || summary.Totals.MeanVote > 0.26 {
		t.Fatalf("mean vote = %v, want 0.25", summary.Totals.MeanVote)
	}
}

func TestSummaryRespectsTheWindow(t *testing.T) {
	st := newSummaryStore(t)
	insertVote(t, st, 1, "Roxy", 0.0, 1.0, 10.0, 10.0)

	// A vote from 60 days ago must not appear in a 30-day window.
	old := store.FeedbackEvent{
		Bid: 2, Checksum: "7c9e6679742540de944bbed07fc1f90a", Rate: 1,
		ActualAlgorithm: "Roxy", DisplayedVal: 10, Vote: 1, Weight: 1, WeightVersion: 3,
		Target: 11, Passed: true, Client: "lazer", DedupToken: "old",
		Day: store.DayStartMs(time.Now().AddDate(0, 0, -60).UnixMilli()),
		CreatedAt: time.Now().AddDate(0, 0, -60).UnixMilli(),
	}
	if err := st.InsertFeedback(old); err != nil {
		t.Fatalf("insert old: %v", err)
	}

	summary, err := BuildSummary(st, 30, time.Now())
	if err != nil {
		t.Fatalf("summary: %v", err)
	}
	if summary.Totals.Votes != 1 {
		t.Fatalf("votes in 30d = %d, want 1", summary.Totals.Votes)
	}
}
