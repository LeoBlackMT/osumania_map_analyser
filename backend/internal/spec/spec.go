// Package spec defines the authoritative whitelist schemas, validation rules,
// and bounds for anonymous telemetry data.
package spec

import (
	"regexp"
	"strings"
)

// AllowedAlgorithms defines all user-selectable estimator algorithms.
var AllowedAlgorithms = map[string]bool{
	"Mixed":      true,
	"Sunny":      true,
	"Azusa":      true,
	"Daniel":     true,
	"Roxy":       true,
	"Companella": true,
}

// AllowedActualAlgorithms defines all underlying compute algorithms.
// Mixed is a routing meta-estimator, so it is never an actual algorithm.
var AllowedActualAlgorithms = map[string]bool{
	"Sunny":      true,
	"Daniel":     true,
	"Azusa":      true,
	"Roxy":       true,
	"Companella": true,
}

// CapsuleAliases maps historical display capsule text to canonical actual algorithm names.
var CapsuleAliases = map[string]string{
	"Azusa+Companella": "Azusa",
}

// MaxComputeDurationMs is the upper bound for valid map analysis duration (30 seconds).
// Any analyze event with durationMs exceeding this threshold is discarded entirely
// from duration metrics to prevent debugging/breakpoint pauses from skewing statistics.
const MaxComputeDurationMs = 30000

// semverPattern validates standard production release semantic versions.
var semverPattern = regexp.MustCompile(`^v?[0-9]+\.[0-9]+(?:\.[0-9]+)?$`)

// IsOfficialVersion reports whether a version string matches standard production semver
// and contains no development, testing, or modified markers.
func IsOfficialVersion(v string) bool {
	v = strings.TrimSpace(v)
	if v == "" || v == "unknown" {
		return false
	}
	lower := strings.ToLower(v)
	if strings.Contains(lower, "dev") ||
		strings.Contains(lower, "test") ||
		strings.Contains(lower, "dirty") ||
		strings.Contains(lower, "debug") ||
		strings.Contains(lower, "local") {
		return false
	}
	return semverPattern.MatchString(v)
}

// NormalizeActualAlgorithm maps display capsule aliases and validates against the actual algorithm whitelist.
func NormalizeActualAlgorithm(val string) (string, bool) {
	val = strings.TrimSpace(val)
	if canonical, ok := CapsuleAliases[val]; ok {
		val = canonical
	}
	if AllowedActualAlgorithms[val] {
		return val, true
	}
	return "", false
}

// IsAllowedAlgorithm checks whether the given algorithm is in the allowed user algorithm whitelist.
func IsAllowedAlgorithm(val string) bool {
	return AllowedAlgorithms[strings.TrimSpace(val)]
}
