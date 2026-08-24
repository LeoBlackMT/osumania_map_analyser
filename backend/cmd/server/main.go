// osumania-telemetry is a small, self-contained Go server that collects
// anonymous usage statistics from the osu!mania plugin and exposes a public
// aggregate dashboard.
//
// The dashboard only reads write-path aggregates (daily_agg / install_days /
// install_hours); the raw events table is a bounded debug log (~14 days).
//
// One-shot maintenance command (run with the service stopped):
//
//	telemetry-server -migrate
//
// rebuilds every aggregate from the raw events, prunes raw events older than
// the retention window, and VACUUMs. Idempotent by construction.
package main

import (
	"log"
	"net/http"
	"os"
	"time"

	"osumania-telemetry/internal/backup"
	"osumania-telemetry/internal/config"
	"osumania-telemetry/internal/ratelimit"
	"osumania-telemetry/internal/store"
	"osumania-telemetry/internal/telemetry"
	"osumania-telemetry/internal/web"
)

// version is injected at build time via
// `-ldflags "-X main.version=<tag>"`; local/dev builds report "dev".
var version = "dev"

func main() {
	if len(os.Args) > 1 && os.Args[1] == "-migrate" {
		runMigrate()
		return
	}

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
	webHandler := web.NewHandler(st, cfg.OnlineWindowMin, cfg.ActiveMin, cfg.StatsCacheSeconds, version)

	go rawRetentionLoop(st, cfg.RawRetentionDays)
	go hourRetentionLoop(st, cfg.HourRetentionDays)

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

	log.Printf("osumania-telemetry %s listening on %s (active = %d analyzes/day)", version, cfg.Addr, cfg.ActiveMin)
	if err := server.ListenAndServe(); err != nil && err != http.ErrServerClosed {
		log.Fatalf("server: %v", err)
	}
}

// runMigrate rebuilds the aggregates and exits. Intended to run once with the
// service stopped; see the package comment.
func runMigrate() {
	cfg, err := config.Load()
	if err != nil {
		log.Fatalf("config: %v", err)
	}
	st, err := store.Open(cfg.DBPath)
	if err != nil {
		log.Fatalf("store: %v", err)
	}
	defer st.Close()

	log.Printf("migrate: rebuilding aggregates from %s ...", cfg.DBPath)
	rep, err := st.Migrate(cfg.RawRetentionDays)
	if err != nil {
		log.Fatalf("migrate: %v", err)
	}
	log.Printf("migrate: scanned %d events -> %d agg rows, %d install-days, %d install-hours",
		rep.EventsScanned, rep.AggRows, rep.InstallDays, rep.InstallHours)
	log.Printf("migrate: deleted %d raw events older than %d days", rep.DeletedEvents, cfg.RawRetentionDays)
	log.Printf("migrate: db size %d -> %d bytes", rep.SizeBefore, rep.SizeAfter)
	log.Printf("migrate: done (aggregates are complete; starting the server is safe)")
}

// rawRetentionLoop prunes raw events (the debug log) after RawRetentionDays.
// Aggregates are unaffected — they are the product; raw events are not.
func rawRetentionLoop(st *store.Store, retentionDays int) {
	if retentionDays <= 0 {
		log.Printf("raw retention: disabled (keep raw events forever)")
		return
	}
	run := func() {
		cutoff := time.Now().Add(-time.Duration(retentionDays) * 24 * time.Hour).UnixMilli()
		if n, err := st.DeleteEventsBefore(cutoff); err != nil {
			log.Printf("raw retention: %v", err)
		} else if n > 0 {
			log.Printf("raw retention: deleted %d raw events older than %d days", n, retentionDays)
		}
	}
	run()
	for range time.Tick(24 * time.Hour) {
		run()
	}
}

// hourRetentionLoop bounds the install_hours table (hourly presence) to a
// rolling window. The 24h-of-day distribution and the hourly trend for short
// windows read it; with the default 90 days the dashboard's max window is
// fully covered.
func hourRetentionLoop(st *store.Store, retentionDays int) {
	if retentionDays <= 0 {
		log.Printf("hour retention: disabled")
		return
	}
	run := func() {
		cutoff := time.Now().Add(-time.Duration(retentionDays) * 24 * time.Hour).UnixMilli()
		if n, err := st.DeleteHoursBefore(cutoff); err != nil {
			log.Printf("hour retention: %v", err)
		} else if n > 0 {
			log.Printf("hour retention: deleted %d install-hour rows older than %d days", n, retentionDays)
		}
	}
	run()
	for range time.Tick(24 * time.Hour) {
		run()
	}
}
