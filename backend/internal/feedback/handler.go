package feedback

import (
	"context"
	"crypto/ed25519"
	"crypto/hmac"
	"crypto/sha256"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"math"
	"net"
	"net/http"
	"strconv"
	"strings"
	"sync"
	"time"

	"osumania-telemetry/internal/config"
	"osumania-telemetry/internal/spec"
	"osumania-telemetry/internal/store"
)

const (
	maxBodyBytes      = 8192
	allowedSkewSecond = 300
)

// Handler serves POST /api/v1/feedback.
type Handler struct {
	cfg      config.Config
	store    *store.FeedbackStore
	osu      *OSUAPI
	keys     map[string]ed25519.PublicKey
	dedupKey []byte
	ipLimit  *windowLimiter
	mapLimit *windowLimiter
}

// NewHandler builds the ingest handler.
func NewHandler(cfg config.Config, st *store.FeedbackStore, osu *OSUAPI) *Handler {
	keys := ParsePublicKeys(cfg.FeedbackPublicKeys)
	if !cfg.FeedbackAllowDevKey {
		delete(keys, "dev")
	}
	return &Handler{
		cfg:      cfg,
		store:    st,
		osu:      osu,
		keys:     keys,
		dedupKey: []byte(cfg.FeedbackDedupSecret),
		ipLimit:  newWindowLimiter(time.Hour, cfg.FeedbackRateLimitPerHour),
		mapLimit: newWindowLimiter(24*time.Hour, cfg.FeedbackMapDailyCap),
	}
}

func (h *Handler) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	if r.URL.Path != "/api/v1/feedback" {
		http.NotFound(w, r)
		return
	}
	origin := r.Header.Get("Origin")
	if origin != "" && !corsAllowed(origin) {
		writeJSON(w, http.StatusForbidden, map[string]string{"error": "forbidden_origin"})
		return
	}
	if origin != "" {
		w.Header().Set("Access-Control-Allow-Origin", origin)
		w.Header().Set("Vary", "Origin")
	}
	if r.Method == http.MethodOptions {
		w.Header().Set("Access-Control-Allow-Methods", "POST, OPTIONS")
		w.Header().Set("Access-Control-Allow-Headers", "Content-Type")
		w.Header().Set("Access-Control-Max-Age", "600")
		w.WriteHeader(http.StatusNoContent)
		return
	}
	if r.Method != http.MethodPost {
		w.Header().Set("Allow", "POST, OPTIONS")
		writeJSON(w, http.StatusMethodNotAllowed, map[string]string{"error": "method_not_allowed"})
		return
	}

	now := time.Now()
	day := store.DayStartMs(now.UnixMilli())

	var env Envelope
	decoder := json.NewDecoder(http.MaxBytesReader(w, r.Body, maxBodyBytes))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&env); err != nil {
		h.reject(w, day, ReasonBadJSON, http.StatusBadRequest)
		return
	}

	pub, ok := h.keys[env.Auth.KeyID]
	if !ok || pub == nil {
		h.reject(w, day, ReasonBadSignature, http.StatusUnauthorized)
		return
	}
	signature, err := hex.DecodeString(strings.TrimSpace(env.Auth.Signature))
	if err != nil || len(signature) != ed25519.SignatureSize {
		h.reject(w, day, ReasonBadSignature, http.StatusUnauthorized)
		return
	}
	if !ed25519.Verify(pub, []byte(CanonicalSignString(env.Payload)), signature) {
		h.reject(w, day, ReasonBadSignature, http.StatusUnauthorized)
		return
	}

	if math.Abs(float64(now.Unix()-env.Payload.Timestamp)) > allowedSkewSecond {
		h.reject(w, day, ReasonStaleTimestamp, http.StatusUnprocessableEntity)
		return
	}
	if reason := ValidateStatic(env.Payload); reason != "" {
		h.reject(w, day, reason, http.StatusBadRequest)
		return
	}
	if err := h.store.InsertNonce(env.Auth.KeyID, env.Payload.Nonce, now.UnixMilli()); err != nil {
		if errors.Is(err, store.ErrNonceReused) {
			h.reject(w, day, ReasonReusedNonce, http.StatusConflict)
			return
		}
		h.reject(w, day, ReasonAPIUnavailable, http.StatusServiceUnavailable)
		return
	}

	// osu! API truth: bid exists, mode is mania, not a convert, checksum matches.
	var validation store.BeatmapValidation
	if h.cfg.FeedbackOSUValidation {
		ctx, cancel := context.WithTimeout(r.Context(), time.Duration(h.cfg.FeedbackOSUWaitMS*2)*time.Millisecond+5*time.Second)
		defer cancel()
		val, reason := h.osu.Validate(ctx, env.Payload.Bid, strings.ToLower(env.Payload.Checksum))
		if reason != "" {
			status := http.StatusUnprocessableEntity
			if reason == ReasonAPIUnavailable || reason == ReasonAPIRateLimited {
				status = http.StatusServiceUnavailable
			}
			h.reject(w, day, reason, status)
			return
		}
		validation = val
		if math.Abs(val.CS-float64(env.Payload.KeyCount)) > 0.01 {
			h.reject(w, day, ReasonBadKeyCount, http.StatusBadRequest)
			return
		}
		if math.Abs(val.OD-*env.Payload.OD) > 0.01 {
			h.reject(w, day, ReasonBadOD, http.StatusBadRequest)
			return
		}
	}

	ip := clientIP(r)
	if !h.ipLimit.allow(ip) {
		h.reject(w, day, ReasonRateLimited, http.StatusTooManyRequests)
		return
	}
	mapKey := fmt.Sprintf("%s|%d", ip, env.Payload.Bid)
	if !h.mapLimit.allow(mapKey) {
		h.reject(w, day, ReasonRateLimited, http.StatusTooManyRequests)
		return
	}

	dedupToken := h.dedupToken(env.Payload.InstallID, env.Payload.Bid)
	since := now.Add(-time.Duration(h.cfg.FeedbackDedupDays) * 24 * time.Hour).UnixMilli()
	duplicate, err := h.store.HasRecentDedup(dedupToken, env.Payload.Bid, since)
	if err != nil {
		h.reject(w, day, ReasonAPIUnavailable, http.StatusServiceUnavailable)
		return
	}
	if duplicate {
		h.store.RecordReject(day, ReasonDuplicate)
		w.WriteHeader(http.StatusNoContent)
		return
	}

	od := env.Payload.OD
	star := math.NaN()
	if h.cfg.FeedbackOSUValidation {
		star = validation.Star
	}
	actual, _ := spec.NormalizeActualAlgorithm(env.Payload.ActualAlgorithm)

	modsJSON, _ := json.Marshal(normalizeMods(env.Payload.Mods))
	event := store.FeedbackEvent{
		Bid:               env.Payload.Bid,
		Checksum:          strings.ToLower(env.Payload.Checksum),
		Rate:              1.0,
		Mods:              string(modsJSON),
		KeyCount:          env.Payload.KeyCount,
		LnRatio:           env.Payload.LnRatio,
		LnBucket:          lnBucket(env.Payload.LnRatio),
		Pattern:           env.Payload.Pattern,
		ModeTag:           env.Payload.ModeTag,
		OD:                sqlNullFloat(od),
		ODBucket:          odBucket(od),
		Star:              sqlNullFloat(finiteOrNil(star)),
		ActualAlgorithm:   actual,
		SelectedAlgorithm: env.Payload.SelectedAlgorithm,
		DisplayedVal:      env.Payload.DisplayedVal,
		Vote:              env.Payload.Vote,
		Weight:            env.Payload.Weight,
		WeightVersion:     env.Payload.WeightVersion,
		Target:            math.Round((env.Payload.DisplayedVal+env.Payload.Vote)*100) / 100,
		Passed:            true,
		Client:            env.Payload.Client,
		Classic:           env.Payload.Classic,
		PluginVersion:     env.Payload.PluginVersion,
		SettingsHash:      strings.ToLower(env.Payload.SettingsHash),
		DedupToken:        dedupToken,
		KeyID:             env.Auth.KeyID,
		Nonce:             env.Payload.Nonce,
		Timestamp:         env.Payload.Timestamp,
		Day:               day,
		CreatedAt:         now.UnixMilli(),
	}
	if err := h.store.InsertFeedback(event); err != nil {
		log.Printf("feedback: insert failed: %v", err)
		h.reject(w, day, ReasonAPIUnavailable, http.StatusServiceUnavailable)
		return
	}
	w.WriteHeader(http.StatusNoContent)
}

func (h *Handler) reject(w http.ResponseWriter, day int64, reason string, status int) {
	h.store.RecordReject(day, reason)
	writeJSON(w, status, map[string]string{"error": reason})
}

func (h *Handler) dedupToken(installID string, bid int64) string {
	mac := hmac.New(sha256.New, h.dedupKey)
	mac.Write([]byte(installID))
	mac.Write([]byte("|"))
	mac.Write([]byte(strconv.FormatInt(bid, 10)))
	mac.Write([]byte("|1.000"))
	return hex.EncodeToString(mac.Sum(nil))
}

func writeJSON(w http.ResponseWriter, status int, body any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(body)
}

func corsAllowed(origin string) bool {
	switch strings.ToLower(strings.TrimSpace(origin)) {
	case "http://127.0.0.1:24061", "http://localhost:24061",
		"http://127.0.0.1:24050", "http://localhost:24050",
		"http://tauri.localhost":
		return true
	}
	return false
}

func clientIP(r *http.Request) string {
	host, _, err := net.SplitHostPort(r.RemoteAddr)
	if err != nil {
		return r.RemoteAddr
	}
	return host
}

func normalizeMods(mods []string) []string {
	out := make([]string, 0, len(mods))
	for _, mod := range mods {
		out = append(out, strings.ToUpper(strings.TrimSpace(mod)))
	}
	if len(out) == 0 {
		return []string{}
	}
	return out
}

func lnBucket(ratio float64) string {
	if ratio <= 0 {
		return "0.00"
	}
	bucket := math.Floor(ratio/0.05) * 0.05
	if bucket > RCLnMax {
		bucket = RCLnMax
	}
	return fmt.Sprintf("%.2f", bucket)
}

func odBucket(od *float64) string {
	if od == nil || math.IsNaN(*od) {
		return "unknown"
	}
	return fmt.Sprintf("%.1f", math.Floor(*od*2)/2)
}

func finiteOrNil(v float64) *float64 {
	if math.IsNaN(v) || math.IsInf(v, 0) {
		return nil
	}
	return &v
}

func sqlNullFloat(v *float64) sql.NullFloat64 {
	if v == nil {
		return sql.NullFloat64{}
	}
	return sql.NullFloat64{Float64: *v, Valid: true}
}

// windowLimiter is a fixed-window in-memory limiter (never persisted).
type windowLimiter struct {
	mu     sync.Mutex
	window time.Duration
	limit  int
	hits   map[string]windowHit
}

type windowHit struct {
	start time.Time
	count int
}

func newWindowLimiter(window time.Duration, limit int) *windowLimiter {
	l := &windowLimiter{window: window, limit: limit, hits: make(map[string]windowHit)}
	go func() {
		for range time.Tick(window) {
			l.mu.Lock()
			cutoff := time.Now().Add(-l.window)
			for key, hit := range l.hits {
				if hit.start.Before(cutoff) {
					delete(l.hits, key)
				}
			}
			l.mu.Unlock()
		}
	}()
	return l
}

func (l *windowLimiter) allow(key string) bool {
	if l.limit <= 0 {
		return true
	}
	l.mu.Lock()
	defer l.mu.Unlock()
	now := time.Now()
	hit := l.hits[key]
	if hit.start.IsZero() || now.Sub(hit.start) >= l.window {
		l.hits[key] = windowHit{start: now, count: 1}
		return true
	}
	if hit.count >= l.limit {
		return false
	}
	hit.count++
	l.hits[key] = hit
	return true
}
