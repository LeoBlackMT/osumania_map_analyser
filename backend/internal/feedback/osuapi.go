package feedback

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"osumania-telemetry/internal/store"
)

// osu! API hard limit is 60 requests/minute. The client keeps a safety budget
// (`ratePerMin`, default 50) plus a burst of 1, and never queues work: a
// request that cannot get a token in time is rejected, never postponed.
type OSUAPI struct {
	client   *http.Client
	id       string
	secret   string
	store    *store.FeedbackStore
	negative time.Duration
	cooldown time.Duration
	wait     time.Duration

	mu           sync.Mutex
	token        string
	tokenExpiry  time.Time
	coolingUntil time.Time
	flights      map[int64]*flight
	tokens       chan struct{}

	requests    atomic.Int64
	cacheHits   atomic.Int64
	rateLimited atomic.Int64
	api429      atomic.Int64
}

type flight struct {
	done chan struct{}
	val  store.BeatmapValidation
	why  string
}

type beatmapResponse struct {
	ID               int64   `json:"id"`
	Checksum         string  `json:"checksum"`
	Mode             string  `json:"mode"`
	ModeInt          int     `json:"mode_int"`
	Convert          bool    `json:"convert"`
	CS               float64 `json:"cs"`
	Accuracy         float64 `json:"accuracy"`
	DifficultyRating float64 `json:"difficulty_rating"`
	DeletedAt        *string `json:"deleted_at"`
}

// NewOSUAPI builds the client and starts the token refill ticker.
func NewOSUAPI(id, secret string, st *store.FeedbackStore, ratePerMin, burst, waitMS, negativeDays, cooldownSeconds int) *OSUAPI {
	if burst < 1 {
		burst = 1
	}
	if ratePerMin < 1 {
		ratePerMin = 50
	}
	api := &OSUAPI{
		client:   &http.Client{Timeout: 10 * time.Second},
		id:       id,
		secret:   secret,
		store:    st,
		negative: time.Duration(negativeDays) * 24 * time.Hour,
		cooldown: time.Duration(cooldownSeconds) * time.Second,
		wait:     time.Duration(waitMS) * time.Millisecond,
		flights:  make(map[int64]*flight),
		tokens:   make(chan struct{}, burst),
	}
	for i := 0; i < burst; i++ {
		api.tokens <- struct{}{}
	}
	interval := time.Minute / time.Duration(ratePerMin)
	go func() {
		ticker := time.NewTicker(interval)
		defer ticker.Stop()
		for range ticker.C {
			select {
			case api.tokens <- struct{}{}:
			default:
			}
		}
	}()
	return api
}

// Metrics returns the counters shown on the admin page.
func (o *OSUAPI) Metrics() map[string]int64 {
	return map[string]int64{
		"requests":     o.requests.Load(),
		"cache_hits":   o.cacheHits.Load(),
		"rate_limited": o.rateLimited.Load(),
		"http_429":     o.api429.Load(),
	}
}

// Validate returns the cached/verified beatmap and a reject reason ("" = ok).
func (o *OSUAPI) Validate(ctx context.Context, bid int64, checksum string) (store.BeatmapValidation, string) {
	now := time.Now()
	if cached, ok, err := o.store.GetBeatmapValidation(bid); err == nil && ok {
		if now.Sub(time.UnixMilli(cached.CheckedAt)) < 30*24*time.Hour {
			o.cacheHits.Add(1)
			return cached, checkCached(cached, checksum)
		}
	}
	if status, at, ok, err := o.store.GetNegative(bid); err == nil && ok {
		if now.Sub(time.UnixMilli(at)) < o.negative {
			o.cacheHits.Add(1)
			switch status {
			case "not_mania":
				return store.BeatmapValidation{}, ReasonNotMania
			case "convert":
				return store.BeatmapValidation{}, ReasonConvert
			default:
				return store.BeatmapValidation{}, ReasonLookupFailed
			}
		}
	}

	o.mu.Lock()
	if now.Before(o.coolingUntil) {
		o.mu.Unlock()
		o.rateLimited.Add(1)
		return store.BeatmapValidation{}, ReasonAPIRateLimited
	}
	if f, ok := o.flights[bid]; ok {
		o.mu.Unlock()
		select {
		case <-f.done:
			return f.val, f.why
		case <-ctx.Done():
			return store.BeatmapValidation{}, ReasonAPIUnavailable
		}
	}
	f := &flight{done: make(chan struct{})}
	o.flights[bid] = f
	o.mu.Unlock()

	val, why := o.fetch(ctx, bid, checksum)
	f.val, f.why = val, why
	close(f.done)
	o.mu.Lock()
	delete(o.flights, bid)
	o.mu.Unlock()
	return val, why
}

func checkCached(cached store.BeatmapValidation, checksum string) string {
	if cached.Mode != 3 {
		return ReasonNotMania
	}
	if cached.Convert {
		return ReasonConvert
	}
	if !strings.EqualFold(cached.Checksum, checksum) {
		return ReasonChecksumInvalid
	}
	return ""
}

func (o *OSUAPI) fetch(ctx context.Context, bid int64, checksum string) (store.BeatmapValidation, string) {
	if !o.acquire(ctx) {
		o.rateLimited.Add(1)
		return store.BeatmapValidation{}, ReasonAPIRateLimited
	}
	bm, status, err := o.getBeatmap(ctx, bid)
	if err != nil {
		if status == http.StatusTooManyRequests {
			o.api429.Add(1)
			o.mu.Lock()
			o.coolingUntil = time.Now().Add(o.cooldown)
			o.mu.Unlock()
			return store.BeatmapValidation{}, ReasonAPIRateLimited
		}
		if status == http.StatusNotFound {
			_ = o.store.PutNegative(bid, "not_found", time.Now().UnixMilli())
			return store.BeatmapValidation{}, ReasonLookupFailed
		}
		return store.BeatmapValidation{}, ReasonAPIUnavailable
	}
	if bm.ModeInt != 3 {
		_ = o.store.PutNegative(bid, "not_mania", time.Now().UnixMilli())
		return store.BeatmapValidation{}, ReasonNotMania
	}
	if bm.Convert {
		_ = o.store.PutNegative(bid, "convert", time.Now().UnixMilli())
		return store.BeatmapValidation{}, ReasonConvert
	}
	if !strings.EqualFold(bm.Checksum, checksum) {
		// The local .osu may be an older revision; ask by checksum before rejecting.
		if looked, ok := o.lookupChecksum(ctx, checksum); ok && looked.ID == bid {
			bm = looked
		} else {
			return store.BeatmapValidation{}, ReasonChecksumInvalid
		}
	}
	val := store.BeatmapValidation{
		Bid:       bid,
		Checksum:  strings.ToLower(bm.Checksum),
		Mode:      bm.ModeInt,
		Convert:   bm.Convert,
		CS:        bm.CS,
		OD:        bm.Accuracy,
		Star:      bm.DifficultyRating,
		CheckedAt: time.Now().UnixMilli(),
	}
	_ = o.store.PutBeatmapValidation(val)
	return val, ""
}

func (o *OSUAPI) acquire(ctx context.Context) bool {
	if o.wait <= 0 {
		select {
		case <-o.tokens:
			return true
		default:
			return false
		}
	}
	timer := time.NewTimer(o.wait)
	defer timer.Stop()
	select {
	case <-o.tokens:
		return true
	case <-timer.C:
		return false
	case <-ctx.Done():
		return false
	}
}

func (o *OSUAPI) accessToken(ctx context.Context) (string, error) {
	o.mu.Lock()
	if o.token != "" && time.Now().Before(o.tokenExpiry) {
		token := o.token
		o.mu.Unlock()
		return token, nil
	}
	o.mu.Unlock()

	form := url.Values{
		"client_id":     {o.id},
		"client_secret": {o.secret},
		"grant_type":    {"client_credentials"},
		"scope":         {"public"},
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, "https://osu.ppy.sh/oauth/token",
		strings.NewReader(form.Encode()))
	if err != nil {
		return "", err
	}
	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	req.Header.Set("Accept", "application/json")
	res, err := o.client.Do(req)
	if err != nil {
		return "", err
	}
	defer res.Body.Close()
	if res.StatusCode != http.StatusOK {
		return "", fmt.Errorf("token status %d", res.StatusCode)
	}
	var body struct {
		AccessToken string `json:"access_token"`
		ExpiresIn   int    `json:"expires_in"`
	}
	if err := json.NewDecoder(io.LimitReader(res.Body, 1<<20)).Decode(&body); err != nil {
		return "", err
	}
	if body.AccessToken == "" {
		return "", errors.New("empty access token")
	}
	ttl := time.Duration(body.ExpiresIn)*time.Second - time.Minute
	if ttl <= 0 {
		ttl = time.Minute
	}
	o.mu.Lock()
	o.token = body.AccessToken
	o.tokenExpiry = time.Now().Add(ttl)
	o.mu.Unlock()
	return body.AccessToken, nil
}

func (o *OSUAPI) getBeatmap(ctx context.Context, bid int64) (beatmapResponse, int, error) {
	return o.getJSON(ctx, fmt.Sprintf("https://osu.ppy.sh/api/v2/beatmaps/%d", bid))
}

func (o *OSUAPI) lookupChecksum(ctx context.Context, checksum string) (beatmapResponse, bool) {
	bm, status, err := o.getJSON(ctx,
		"https://osu.ppy.sh/api/v2/beatmaps/lookup?checksum="+url.QueryEscape(checksum))
	if err != nil || status != http.StatusOK {
		return beatmapResponse{}, false
	}
	return bm, true
}

func (o *OSUAPI) getJSON(ctx context.Context, endpoint string) (beatmapResponse, int, error) {
	token, err := o.accessToken(ctx)
	if err != nil {
		return beatmapResponse{}, 0, err
	}
	var lastStatus int
	var lastErr error
	for attempt := 0; attempt < 3; attempt++ {
		if attempt > 0 {
			select {
			case <-time.After(time.Duration(attempt) * time.Second):
			case <-ctx.Done():
				return beatmapResponse{}, 0, ctx.Err()
			}
		}
		req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
		if err != nil {
			return beatmapResponse{}, 0, err
		}
		req.Header.Set("Authorization", "Bearer "+token)
		req.Header.Set("Accept", "application/json")
		o.requests.Add(1)
		res, err := o.client.Do(req)
		if err != nil {
			lastErr = err
			continue
		}
		body, readErr := io.ReadAll(io.LimitReader(res.Body, 1<<20))
		res.Body.Close()
		if readErr != nil {
			lastErr = readErr
			continue
		}
		lastStatus = res.StatusCode
		if res.StatusCode == http.StatusOK {
			var bm beatmapResponse
			if err := json.Unmarshal(body, &bm); err != nil {
				return beatmapResponse{}, res.StatusCode, err
			}
			return bm, res.StatusCode, nil
		}
		lastErr = fmt.Errorf("osu api status %d", res.StatusCode)
		if res.StatusCode != http.StatusTooManyRequests && res.StatusCode < 500 {
			return beatmapResponse{}, res.StatusCode, lastErr
		}
	}
	return beatmapResponse{}, lastStatus, lastErr
}
