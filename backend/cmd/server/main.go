// osumania-telemetry is a small, self-contained Go server that collects
// anonymous usage statistics from the osu!mania plugin and exposes a public
// aggregate dashboard.
package main

import (
	"log"
	"net/http"
	"time"

	"osumania-telemetry/internal/backup"
	"osumania-telemetry/internal/config"
	"osumania-telemetry/internal/ratelimit"
	"osumania-telemetry/internal/store"
	"osumania-telemetry/internal/telemetry"
	"osumania-telemetry/internal/web"
)

func main() {
	cfg, err := config.Load()
	if err != nil {
		log.Fatalf("config: %v", err)
	}

	st, err := store.Open(cfg.DBPath)
	if err != nil {
		log.Fatalf("store: %v", err)
	}
	defer st.Close()

	limiter := ratelimit.New(cfg.RateLimitPerMin)
	eventHandler := telemetry.NewHandler(st, limiter)
	webHandler := web.NewHandler(st, cfg.OnlineWindowMin, cfg.StatsCacheSeconds)

	go retentionLoop(st, cfg.RetentionDays)

	bm, err := backup.New(cfg, st)
	if err != nil {
		log.Fatalf("backup: %v", err)
	}
	if bm.Enabled() {
		go bm.Run()
	} else {
		log.Printf("backup: disabled (OBS credentials not set)")
	}

	mux := http.NewServeMux()
	mux.Handle("/api/v1/event", eventHandler)
	mux.Handle("/", webHandler)

	server := &http.Server{
		Addr:              cfg.Addr,
		Handler:           mux,
		ReadHeaderTimeout: 10 * time.Second,
		ReadTimeout:       15 * time.Second,
		IdleTimeout:       60 * time.Second,
	}

	log.Printf("osumania-telemetry listening on %s", cfg.Addr)
	if err := server.ListenAndServe(); err != nil && err != http.ErrServerClosed {
		log.Fatalf("server: %v", err)
	}
}

func retentionLoop(st *store.Store, retentionDays int) {
	if retentionDays <= 0 {
		log.Printf("retention: disabled (keep events forever)")
		return
	}
	run := func() {
		cutoff := time.Now().Add(-time.Duration(retentionDays) * 24 * time.Hour).UnixMilli()
		if n, err := st.DeleteEventsBefore(cutoff); err != nil {
			log.Printf("retention: %v", err)
		} else if n > 0 {
			log.Printf("retention: deleted %d events older than %d days", n, retentionDays)
		}
	}
	run()
	for range time.Tick(24 * time.Hour) {
		run()
	}
}
