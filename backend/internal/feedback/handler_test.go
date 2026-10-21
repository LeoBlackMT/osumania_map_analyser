package feedback

import (
	"bytes"
	"crypto/ed25519"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"osumania-telemetry/internal/config"
	"osumania-telemetry/internal/store"
)

func basePayload() Payload {
	od := 8.5
	return Payload{
		Bid:               4182931,
		Checksum:          "7c9e6679742540de944bbed07fc1f90a",
		Rate:              1.0,
		Mods:              []string{"MR"},
		KeyCount:          4,
		LnRatio:           0.015,
		Pattern:           "Jacks",
		ModeTag:           "RC",
		OD:                &od,
		ActualAlgorithm:   "Roxy",
		SelectedAlgorithm: "Mixed",
		DisplayedVal:      12.2,
		Vote:              0.5,
		Weight:            0.995,
		WeightVersion:     3,
		Passed:            true,
		Client:            "stable",
		Classic:           true,
		PluginVersion:     "2.2.0",
		SettingsHash:      "a1b2c3d4",
		InstallID:         "6f1c2a9e6f1c2a9e6f1c2a9e6f1c2a9e",
		Timestamp:         time.Now().Unix(),
		Nonce:             "a4f19c08e21b7d33",
	}
}

func TestCanonicalSignStringMatchesShell(t *testing.T) {
	payload := basePayload()
	payload.Timestamp = 1760000000
	got := CanonicalSignString(payload)
	want := "v1|4182931|7c9e6679742540de944bbed07fc1f90a|1.000|MR|4|0.0150|Jacks|RC|8.50|Roxy|Mixed|12.20|0.5|0.995|3|1|stable|1|2.2.0|a1b2c3d4|6f1c2a9e6f1c2a9e6f1c2a9e6f1c2a9e|1760000000|a4f19c08e21b7d33"
	if got != want {
		t.Fatalf("sign string mismatch:\n got %q\nwant %q", got, want)
	}

	payload.TargetSide = "LN"
	v2 := CanonicalSignString(payload)
	if !strings.HasPrefix(v2, "v2|") || !strings.HasSuffix(v2, "|LN") {
		t.Fatalf("v2 sign string = %q", v2)
	}
}

func TestValidateStatic(t *testing.T) {
	cases := []struct {
		name   string
		mutate func(*Payload)
		want   string
	}{
		{"ok", func(p *Payload) {}, ""},
		{"bad bid", func(p *Payload) { p.Bid = 0 }, ReasonBadBid},
		{"bad checksum", func(p *Payload) { p.Checksum = "xyz" }, ReasonChecksumInvalid},
		{"bad rate", func(p *Payload) { p.Rate = 1.1 }, ReasonBadRate},
		{"banned mod", func(p *Payload) { p.Mods = []string{"CL"} }, ReasonUnknownMod},
		{"keymod", func(p *Payload) { p.Mods = []string{"7K"} }, ReasonKeyMod},
		{"not rc", func(p *Payload) { p.ModeTag = "LN" }, ReasonNotRC},
		{"ln ratio", func(p *Payload) { p.LnRatio = 0.2 }, ReasonNotRC},
		{"keycount", func(p *Payload) { p.KeyCount = 5 }, ReasonBadKeyCount},
		{"6k ln", func(p *Payload) { p.KeyCount = 6; p.ModeTag = "LN"; p.LnRatio = 0.8; p.TargetSide = "LN" }, ""},
		{"6k no side", func(p *Payload) { p.KeyCount = 6; p.ModeTag = "LN"; p.LnRatio = 0.8 }, ReasonBadSide},
		{"6k bad mode", func(p *Payload) { p.KeyCount = 6; p.ModeTag = "XX"; p.LnRatio = 0.8; p.TargetSide = "RC" }, ReasonBadMode},
		{"4k wrong side", func(p *Payload) { p.TargetSide = "LN" }, ReasonBadSide},
		{"client", func(p *Payload) { p.Client = "stable-x" }, ReasonBadClient},
		{"version", func(p *Payload) { p.PluginVersion = "2.2.0-dev" }, ReasonBadVersion},
		{"failed", func(p *Payload) { p.Passed = false }, ReasonNotPassed},
		{"algorithm", func(p *Payload) { p.ActualAlgorithm = "Mixed" }, ReasonBadAlgorithm},
		{"displayed", func(p *Payload) { p.DisplayedVal = 99 }, ReasonBadDisplayed},
		{"weight", func(p *Payload) { p.Weight = 0.39 }, ReasonBadWeight},
		{"vote", func(p *Payload) { p.Vote = 1.5 }, ReasonBadVote},
		{"settings", func(p *Payload) { p.SettingsHash = "zz" }, ReasonBadSettingsHash},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			payload := basePayload()
			tc.mutate(&payload)
			if got := ValidateStatic(payload); got != tc.want {
				t.Fatalf("got %q want %q", got, tc.want)
			}
		})
	}
}

type testEnv struct {
	handler *Handler
	store   *store.FeedbackStore
	priv    ed25519.PrivateKey
	keyID   string
}

func newTestEnv(t *testing.T) *testEnv {
	t.Helper()
	dir := t.TempDir()
	st, err := store.OpenFeedback(filepath.Join(dir, "feedback.db"))
	if err != nil {
		t.Fatalf("open feedback store: %v", err)
	}
	t.Cleanup(func() { _ = st.Close() })

	pub, priv, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatalf("key: %v", err)
	}
	cfg := config.Config{
		FeedbackEnabled:          true,
		FeedbackOSUValidation:    false,
		FeedbackPublicKeys:       "test:" + hex.EncodeToString(pub),
		FeedbackDedupSecret:      "test-secret-value",
		FeedbackRateLimitPerHour: 100,
		FeedbackMapDailyCap:      100,
		FeedbackDedupDays:        30,
		FeedbackMinN:             8,
		FeedbackTrimRatio:        0.1,
	}
	return &testEnv{handler: NewHandler(cfg, st, nil), store: st, priv: priv, keyID: "test"}
}

func (e *testEnv) post(t *testing.T, payload Payload, signatureOverride []byte) *httptest.ResponseRecorder {
	t.Helper()
	body := Envelope{
		Payload: payload,
		Auth: Auth{
			KeyID:     e.keyID,
			Signature: hex.EncodeToString(ed25519.Sign(e.priv, []byte(CanonicalSignString(payload)))),
		},
	}
	if signatureOverride != nil {
		body.Auth.Signature = hex.EncodeToString(signatureOverride)
	}
	encoded, err := json.Marshal(body)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	req := httptest.NewRequest(http.MethodPost, "/api/v1/feedback", bytes.NewReader(encoded))
	req.Header.Set("Content-Type", "application/json")
	rec := httptest.NewRecorder()
	e.handler.ServeHTTP(rec, req)
	return rec
}

func (e *testEnv) count(t *testing.T) int {
	t.Helper()
	var n int
	if err := e.store.DB().QueryRow("SELECT COUNT(*) FROM feedback_events").Scan(&n); err != nil {
		t.Fatalf("count: %v", err)
	}
	return n
}

func TestHandlerAcceptsAndDeduplicates(t *testing.T) {
	env := newTestEnv(t)
	rec := env.post(t, basePayload(), nil)
	if rec.Code != http.StatusNoContent {
		t.Fatalf("accept status = %d body=%s", rec.Code, rec.Body.String())
	}
	if env.count(t) != 1 {
		t.Fatalf("expected 1 row")
	}
	// Same install + map inside the dedup window → 204 but no second row.
	second := basePayload()
	second.Nonce = "bbbbbbbbbbbbbbbb"
	rec = env.post(t, second, nil)
	if rec.Code != http.StatusNoContent {
		t.Fatalf("duplicate status = %d", rec.Code)
	}
	if env.count(t) != 1 {
		t.Fatalf("duplicate must not insert")
	}
}

func TestHandlerAllowsBothSides(t *testing.T) {
	env := newTestEnv(t)

	rc := basePayload()
	rc.KeyCount = 6
	rc.ModeTag = "RC"
	rc.LnRatio = 0.10
	rc.TargetSide = "RC"
	rc.DisplayedVal = 12.2
	if rec := env.post(t, rc, nil); rec.Code != http.StatusNoContent {
		t.Fatalf("RC status = %d body=%s", rec.Code, rec.Body.String())
	}

	ln := basePayload()
	ln.KeyCount = 6
	ln.ModeTag = "LN"
	ln.LnRatio = 0.80
	ln.TargetSide = "LN"
	ln.DisplayedVal = 12.2
	ln.Nonce = "bbbbbbbbbbbbbbbb"
	if rec := env.post(t, ln, nil); rec.Code != http.StatusNoContent {
		t.Fatalf("LN status = %d body=%s", rec.Code, rec.Body.String())
	}
	if env.count(t) != 2 {
		t.Fatalf("expected RC and LN rows, got %d", env.count(t))
	}

	// Same side again is still deduplicated.
	lnAgain := ln
	lnAgain.Nonce = "cccccccccccccccc"
	if rec := env.post(t, lnAgain, nil); rec.Code != http.StatusNoContent {
		t.Fatalf("LN duplicate status = %d", rec.Code)
	}
	if env.count(t) != 2 {
		t.Fatalf("duplicate LN must not insert")
	}
	if err := RefreshAll(env.store, 1, 0); err != nil {
		t.Fatalf("refresh aggregates: %v", err)
	}
	var aggRows int
	if err := env.store.DB().QueryRow(`SELECT COUNT(*) FROM feedback_map_agg`).Scan(&aggRows); err != nil {
		t.Fatalf("count agg rows: %v", err)
	}
	if aggRows != 2 {
		t.Fatalf("expected RC and LN aggregate rows, got %d", aggRows)
	}
}

func TestHandlerRejects(t *testing.T) {
	env := newTestEnv(t)

	bad := make([]byte, ed25519.SignatureSize)
	bad[0] = 1
	if rec := env.post(t, basePayload(), bad); rec.Code != http.StatusUnauthorized {
		t.Fatalf("bad signature status = %d", rec.Code)
	}

	stale := basePayload()
	stale.Timestamp = time.Now().Add(-time.Hour).Unix()
	if rec := env.post(t, stale, nil); rec.Code != http.StatusUnprocessableEntity {
		t.Fatalf("stale status = %d", rec.Code)
	}

	mod := basePayload()
	mod.Mods = []string{"CL"}
	if rec := env.post(t, mod, nil); rec.Code != http.StatusBadRequest {
		t.Fatalf("banned mod status = %d", rec.Code)
	}

	rate := basePayload()
	rate.Rate = 1.25
	if rec := env.post(t, rate, nil); rec.Code != http.StatusBadRequest {
		t.Fatalf("rate status = %d", rec.Code)
	}

	if env.count(t) != 0 {
		t.Fatalf("rejections must not insert")
	}

	var rejects int
	if err := env.store.DB().QueryRow("SELECT COALESCE(SUM(count),0) FROM feedback_rejects").Scan(&rejects); err != nil {
		t.Fatalf("rejects: %v", err)
	}
	if rejects < 4 {
		t.Fatalf("expected reject counters, got %d", rejects)
	}
}

func TestHandlerRejectsReusedNonce(t *testing.T) {
	env := newTestEnv(t)
	first := basePayload()
	if rec := env.post(t, first, nil); rec.Code != http.StatusNoContent {
		t.Fatalf("first status = %d", rec.Code)
	}
	// Different install id → not a dedup hit, but the nonce must still fail.
	second := basePayload()
	second.InstallID = "11111111111111111111111111111111"
	if rec := env.post(t, second, nil); rec.Code != http.StatusConflict {
		t.Fatalf("nonce reuse status = %d", rec.Code)
	}
}

func TestTrimmedWeightedMedian(t *testing.T) {
	samples := []sample{
		{target: 1, weight: 1}, {target: 2, weight: 1}, {target: 3, weight: 1},
		{target: 4, weight: 1}, {target: 100, weight: 1},
	}
	got := trimmedWeightedMedian(samples, 0.2)
	if got != 3 {
		t.Fatalf("trimmed median = %v, want 3", got)
	}
	if q := quantile([]float64{1, 2, 3, 4}, 0.5); q != 2.5 {
		t.Fatalf("quantile = %v", q)
	}
}
