package spec

import (
	"testing"
)

func TestIsOfficialVersion(t *testing.T) {
	valid := []string{"2.1.0", "v2.1.0", "2.0.0", "1.7.4", "1.8", "0.9.1"}
	for _, v := range valid {
		if !IsOfficialVersion(v) {
			t.Errorf("expected %q to be official, got false", v)
		}
	}

	invalid := []string{
		"", "unknown", "dev", "2.1.0-dev", "2.0.0-test", "dirty", "v2.1.0-debug", "local-build",
		"alpha", "beta.1", "1", "1.2.3.4",
	}
	for _, v := range invalid {
		if IsOfficialVersion(v) {
			t.Errorf("expected %q to be unofficial, got true", v)
		}
	}
}

func TestAlgorithms(t *testing.T) {
	for alg := range AllowedAlgorithms {
		if !IsAllowedAlgorithm(alg) {
			t.Errorf("expected %q to be allowed, got false", alg)
		}
	}
	for _, bad := range []string{"BabyDan", "aleju03", "Rework", "LowDiff7K", "Invalid"} {
		if IsAllowedAlgorithm(bad) {
			t.Errorf("expected %q to be rejected, got true", bad)
		}
	}
}

func TestNormalizeActualAlgorithm(t *testing.T) {
	aliasExpectations := map[string]string{
		"Azusa+Companella": "Azusa",
		"Roxy+XDC1":        "Roxy",
		"XDC1+Azusa":       "Azusa",
	}
	for alias, want := range aliasExpectations {
		norm, ok := NormalizeActualAlgorithm(alias)
		if !ok || norm != want {
			t.Errorf("expected %s to map to %s, got %q, %v", alias, want, norm, ok)
		}
	}

	for alg := range AllowedActualAlgorithms {
		norm, ok := NormalizeActualAlgorithm(alg)
		if !ok || norm != alg {
			t.Errorf("expected %q to be accepted, got %q, %v", alg, norm, ok)
		}
	}

	for _, bad := range []string{"BabyRice", "BabyDan", "Mixed", "LowDiff7K"} {
		if _, ok := NormalizeActualAlgorithm(bad); ok {
			t.Errorf("expected %q to be rejected, got true", bad)
		}
	}
}
