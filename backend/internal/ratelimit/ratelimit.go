// Package ratelimit implements a fixed-window, per-key rate limiter kept
// entirely in memory. IPs are never written to disk or logs.
package ratelimit

import (
	"sync"
	"time"
)

type window struct {
	start time.Time
	count int
}

type Limiter struct {
	mu      sync.Mutex
	limit   int
	windows map[string]*window
}

// New returns a limiter allowing `perMinute` requests per key per minute.
// A limit <= 0 disables rate limiting (Allow always returns true).
func New(perMinute int) *Limiter {
	l := &Limiter{
		limit:   perMinute,
		windows: make(map[string]*window),
	}
	go l.cleanup()
	return l
}

func (l *Limiter) Allow(key string) bool {
	if l.limit <= 0 {
		return true
	}

	l.mu.Lock()
	defer l.mu.Unlock()

	now := time.Now()
	w := l.windows[key]
	if w == nil || now.Sub(w.start) >= time.Minute {
		l.windows[key] = &window{start: now, count: 1}
		return true
	}
	if w.count >= l.limit {
		return false
	}
	w.count++
	return true
}

func (l *Limiter) cleanup() {
	for range time.Tick(time.Minute) {
		l.mu.Lock()
		cutoff := time.Now().Add(-2 * time.Minute)
		for k, w := range l.windows {
			if w.start.Before(cutoff) {
				delete(l.windows, k)
			}
		}
		l.mu.Unlock()
	}
}
