// Package config loads runtime configuration from a local .env file and the
// process environment. Every setting has a sane default; the OBS backup block
// is optional (empty credentials disable it).
package config

import (
	"fmt"
	"os"
	"strconv"
	"strings"
)

type Config struct {
	Addr              string
	DBPath            string
	RawRetentionDays  int // raw events keep window (debug log only)
	HourRetentionDays int // install_hours rolling window
	ActiveMin         int // analyze events per day an install needs to count as active
	OnlineWindowMin   int
	RateLimitPerMin   int
	StatsCacheSeconds int

	OBSAccessKey string
	OBSSecretKey string
	OBSEndpoint  string
	OBSBucket    string
	DailyKeep    int
	MonthlyKeep  int

	// RC difficulty feedback (independent database + endpoint).
	FeedbackEnabled            bool
	FeedbackDB                 string
	FeedbackOSUValidation      bool
	FeedbackOSURatePerMin      int
	FeedbackOSURateBurst       int
	FeedbackOSUWaitMS          int
	FeedbackNegativeCacheDays  int
	FeedbackAPICooldownSeconds int
	OSUClientID                string
	OSUClientSecret            string
	FeedbackPublicKeys         string
	FeedbackAllowDevKey        bool
	FeedbackDedupSecret        string
	FeedbackAdminToken         string
	FeedbackRateLimitPerHour   int
	FeedbackMapDailyCap        int
	FeedbackDedupDays          int
	FeedbackRawRetentionDays   int
	FeedbackMinN               int
	FeedbackTrimRatio          float64
}

// Load reads ".env" (when present) and then the process environment, returning
// the merged configuration. Real environment variables take precedence over
// values from the .env file.
func Load() (Config, error) {
	values, err := readDotEnv(".env")
	if err != nil {
		return Config{}, err
	}
	for k, v := range values {
		if _, ok := os.LookupEnv(k); !ok {
			os.Setenv(k, v)
		}
	}

	cfg := Config{
		Addr:              envStr("MMA_TELEMETRY_ADDR", ":8080"),
		DBPath:            envStr("MMA_TELEMETRY_DB", "telemetry.db"),
		RawRetentionDays:  envInt("MMA_TELEMETRY_RAW_RETENTION_DAYS", 14),
		HourRetentionDays: envInt("MMA_TELEMETRY_HOUR_RETENTION_DAYS", 90),
		ActiveMin:         envInt("MMA_TELEMETRY_ACTIVE_MIN", 10),
		OnlineWindowMin:   envInt("MMA_TELEMETRY_ONLINE_WINDOW_MIN", 10),
		RateLimitPerMin:   envInt("MMA_TELEMETRY_RATE_LIMIT_PER_MIN", 120),
		StatsCacheSeconds: envInt("MMA_TELEMETRY_STATS_CACHE_SECONDS", 60),
		OBSAccessKey:      envStr("MMA_BACKUP_OBS_AK", ""),
		OBSSecretKey:      envStr("MMA_BACKUP_OBS_SK", ""),
		OBSEndpoint:       envStr("MMA_BACKUP_OBS_ENDPOINT", ""),
		OBSBucket:         envStr("MMA_BACKUP_OBS_BUCKET", ""),
		DailyKeep:         envInt("MMA_BACKUP_DAILY_KEEP", 30),
		MonthlyKeep:       envInt("MMA_BACKUP_MONTHLY_KEEP", 12),

		FeedbackEnabled:            envBool("FEEDBACK_ENABLED", false),
		FeedbackDB:                 envStr("MMA_FEEDBACK_DB", "feedback.db"),
		FeedbackOSUValidation:      envBool("FEEDBACK_OSU_VALIDATION", true),
		FeedbackOSURatePerMin:      envInt("FEEDBACK_OSU_RATE_PER_MIN", 50),
		FeedbackOSURateBurst:       envInt("FEEDBACK_OSU_RATE_BURST", 1),
		FeedbackOSUWaitMS:          envInt("FEEDBACK_OSU_WAIT_MS", 2000),
		FeedbackNegativeCacheDays:  envInt("FEEDBACK_NEGATIVE_CACHE_DAYS", 7),
		FeedbackAPICooldownSeconds: envInt("FEEDBACK_API_COOLDOWN_SECONDS", 60),
		OSUClientID:                envStr("OSU_CLIENT_ID", ""),
		OSUClientSecret:            envStr("OSU_CLIENT_SECRET", ""),
		FeedbackPublicKeys:         envStr("FEEDBACK_PUBLIC_KEYS", ""),
		FeedbackAllowDevKey:        envBool("FEEDBACK_ALLOW_DEV_KEY", false),
		FeedbackDedupSecret:        envStr("FEEDBACK_DEDUP_SECRET", ""),
		FeedbackAdminToken:         envStr("FEEDBACK_ADMIN_TOKEN", ""),
		FeedbackRateLimitPerHour:   envInt("FEEDBACK_RATE_LIMIT_PER_HOUR", 5),
		FeedbackMapDailyCap:        envInt("FEEDBACK_MAP_DAILY_CAP", 20),
		FeedbackDedupDays:          envInt("FEEDBACK_DEDUP_DAYS", 30),
		FeedbackRawRetentionDays:   envInt("FEEDBACK_RAW_RETENTION_DAYS", 180),
		FeedbackMinN:               envInt("FEEDBACK_MIN_N", 8),
		FeedbackTrimRatio:          envFloat("FEEDBACK_TRIM_RATIO", 0.10),
	}

	if cfg.Addr == "" {
		cfg.Addr = ":8080"
	}
	if cfg.OnlineWindowMin <= 0 {
		cfg.OnlineWindowMin = 10
	}
	if cfg.ActiveMin <= 0 {
		cfg.ActiveMin = 10
	}
	if cfg.RawRetentionDays < 0 {
		return Config{}, fmt.Errorf("MMA_TELEMETRY_RAW_RETENTION_DAYS must be >= 0")
	}
	if cfg.HourRetentionDays < 0 {
		return Config{}, fmt.Errorf("MMA_TELEMETRY_HOUR_RETENTION_DAYS must be >= 0")
	}
	if cfg.RateLimitPerMin < 0 {
		return Config{}, fmt.Errorf("MMA_TELEMETRY_RATE_LIMIT_PER_MIN must be >= 0")
	}
	if cfg.StatsCacheSeconds < 0 {
		return Config{}, fmt.Errorf("MMA_TELEMETRY_STATS_CACHE_SECONDS must be >= 0")
	}
	if cfg.FeedbackOSURatePerMin <= 0 || cfg.FeedbackOSURatePerMin > 60 {
		return Config{}, fmt.Errorf("FEEDBACK_OSU_RATE_PER_MIN must be within 1..60")
	}
	if cfg.FeedbackOSURateBurst <= 0 {
		cfg.FeedbackOSURateBurst = 1
	}
	if cfg.FeedbackOSUWaitMS < 0 {
		return Config{}, fmt.Errorf("FEEDBACK_OSU_WAIT_MS must be >= 0")
	}
	if cfg.FeedbackRateLimitPerHour < 0 || cfg.FeedbackMapDailyCap < 0 {
		return Config{}, fmt.Errorf("feedback rate limits must be >= 0")
	}
	if cfg.FeedbackDedupDays <= 0 {
		cfg.FeedbackDedupDays = 30
	}
	if cfg.FeedbackRawRetentionDays < 0 {
		return Config{}, fmt.Errorf("FEEDBACK_RAW_RETENTION_DAYS must be >= 0")
	}
	if cfg.FeedbackMinN <= 0 {
		cfg.FeedbackMinN = 8
	}
	if cfg.FeedbackTrimRatio < 0 || cfg.FeedbackTrimRatio >= 0.5 {
		return Config{}, fmt.Errorf("FEEDBACK_TRIM_RATIO must be within 0..0.5")
	}

	return cfg, nil
}

// FeedbackReady reports whether the feedback endpoint can accept traffic.
// Missing keys keep the route disabled instead of accepting unverifiable data.
func (c Config) FeedbackReady() bool {
	if !c.FeedbackEnabled {
		return false
	}
	if strings.TrimSpace(c.FeedbackDedupSecret) == "" {
		return false
	}
	if strings.TrimSpace(c.FeedbackPublicKeys) == "" {
		return false
	}
	if c.FeedbackOSUValidation && (c.OSUClientID == "" || c.OSUClientSecret == "") {
		return false
	}
	return true
}

// BackupEnabled reports whether all four OBS credentials are configured.
func (c Config) BackupEnabled() bool {
	return c.OBSAccessKey != "" && c.OBSSecretKey != "" && c.OBSEndpoint != "" && c.OBSBucket != ""
}

func envStr(key, fallback string) string {
	if v, ok := os.LookupEnv(key); ok && strings.TrimSpace(v) != "" {
		return strings.TrimSpace(v)
	}
	return fallback
}

func envInt(key string, fallback int) int {
	v, ok := os.LookupEnv(key)
	if !ok || strings.TrimSpace(v) == "" {
		return fallback
	}
	n, err := strconv.Atoi(strings.TrimSpace(v))
	if err != nil {
		return fallback
	}
	return n
}

func envBool(key string, fallback bool) bool {
	v, ok := os.LookupEnv(key)
	if !ok || strings.TrimSpace(v) == "" {
		return fallback
	}
	switch strings.ToLower(strings.TrimSpace(v)) {
	case "1", "true", "yes", "on":
		return true
	case "0", "false", "no", "off":
		return false
	default:
		return fallback
	}
}

func envFloat(key string, fallback float64) float64 {
	v, ok := os.LookupEnv(key)
	if !ok || strings.TrimSpace(v) == "" {
		return fallback
	}
	f, err := strconv.ParseFloat(strings.TrimSpace(v), 64)
	if err != nil {
		return fallback
	}
	return f
}

// readDotEnv parses a minimal KEY=VALUE file. Lines starting with "#" are
// comments; values may be optionally quoted. Returns an empty map when the
// file does not exist.
func readDotEnv(path string) (map[string]string, error) {
	out := map[string]string{}
	data, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return out, nil
		}
		return nil, err
	}
	for _, line := range strings.Split(string(data), "\n") {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		key, value, ok := strings.Cut(line, "=")
		if !ok {
			continue
		}
		key = strings.TrimSpace(key)
		value = strings.TrimSpace(value)
		value = strings.Trim(value, `"'`)
		if key != "" {
			out[key] = value
		}
	}
	return out, nil
}
