// Package feedback ingests opt-in RC difficulty ratings.
//
// The endpoint is deliberately strict: signature, timestamp, nonce, osu! API
// beatmap identity, mod/rate/pass rules, and dedup must all pass. A rejected
// request is never stored as a degraded row — only a per-day counter moves.
package feedback

import (
	"crypto/ed25519"
	"encoding/hex"
	"fmt"
	"math"
	"regexp"
	"sort"
	"strings"

	"osumania-telemetry/internal/spec"
)

// SignVersion is the first field of the canonical sign string.
const SignVersion = "v1"

// RC-only bounds.
const (
	RCLnMax       = 0.15
	WeightMin     = 0.40
	WeightMax     = 1.00
	WeightVersion = 3
	DisplayedMin  = -3.0
	DisplayedMax  = 25.0
)

// AllowedMods is the RC feedback whitelist. NM is the empty set.
var AllowedMods = map[string]bool{
	"HD":  true,
	"FI":  true,
	"FL":  true,
	"DT":  true,
	"NC":  true,
	"HT":  true,
	"DC":  true,
	"MR":  true,
	"SD":  true,
	"PF":  true,
	"SV2": true,
	"TD":  true,
	"CS":  true,
}

// AllowedRates are the speed multipliers accepted by the pipeline
// (NM/MR 1.0, DT/NC 1.5, HT/DC 0.75).
var AllowedRates = []float64{0.75, 1.0, 1.5}

func rateAllowed(rate float64) bool {
	for _, allowed := range AllowedRates {
		if rate == allowed {
			return true
		}
	}
	return false
}

var keyModPattern = regexp.MustCompile(`^[0-9]+K$`)
var checksumPattern = regexp.MustCompile(`^[0-9a-f]{32}$`)

// Reject reasons. They are stable identifiers shown to the client and counted
// in feedback_rejects.
const (
	ReasonBadJSON         = "bad_json"
	ReasonBadSignature    = "bad_signature"
	ReasonStaleTimestamp  = "stale_timestamp"
	ReasonReusedNonce     = "reused_nonce"
	ReasonBadBid          = "bad_bid"
	ReasonChecksumInvalid = "checksum_invalid"
	ReasonLookupFailed    = "lookup_failed"
	ReasonNotMania        = "not_mania"
	ReasonConvert         = "convert"
	ReasonNotRC           = "not_rc"
	ReasonBadKeyCount     = "bad_keycount"
	ReasonBadClient       = "bad_client"
	ReasonBadClassic      = "bad_classic"
	ReasonBadVersion      = "bad_version"
	ReasonBadRate         = "bad_rate"
	ReasonBannedMod       = "banned_mod"
	ReasonUnknownMod      = "unknown_mod"
	ReasonKeyMod          = "keymod"
	ReasonNotPassed       = "not_passed"
	ReasonBadAlgorithm    = "bad_algorithm"
	ReasonBadDisplayed    = "bad_displayed"
	ReasonBadWeight       = "bad_weight"
	ReasonBadVote         = "bad_vote"
	ReasonBadOD           = "bad_od"
	ReasonBadSettingsHash = "bad_settings_hash"
	ReasonDuplicate       = "duplicate"
	ReasonRateLimited     = "rate_limited"
	ReasonAPIRateLimited  = "api_rate_limited"
	ReasonAPIUnavailable  = "api_unavailable"
)

// Payload is the signed body sent by the desktop shell.
type Payload struct {
	Bid               int64    `json:"bid"`
	Checksum          string   `json:"checksum"`
	Rate              float64  `json:"rate"`
	Mods              []string `json:"mods"`
	KeyCount          int      `json:"key_count"`
	LnRatio           float64  `json:"ln_ratio"`
	Pattern           string   `json:"pattern"`
	ModeTag           string   `json:"mode_tag"`
	OD                *float64 `json:"od"`
	ActualAlgorithm   string   `json:"actual_algorithm"`
	SelectedAlgorithm string   `json:"selected_algorithm"`
	DisplayedVal      float64  `json:"displayed_val"`
	Vote              float64  `json:"vote"`
	Weight            float64  `json:"weight"`
	WeightVersion     int      `json:"weight_version"`
	Passed            bool     `json:"passed"`
	Client            string   `json:"client"`
	Classic           bool     `json:"classic"`
	PluginVersion     string   `json:"plugin_version"`
	SettingsHash      string   `json:"settings_hash"`
	InstallID         string   `json:"install_id"`
	Timestamp         int64    `json:"timestamp"`
	Nonce             string   `json:"nonce"`
}

// Auth carries the detached Ed25519 signature.
type Auth struct {
	KeyID     string `json:"key_id"`
	Signature string `json:"signature"`
}

// Envelope is the full request body.
type Envelope struct {
	Payload Payload `json:"payload"`
	Auth    Auth    `json:"auth"`
}

// CanonicalSignString must match the shell byte for byte.
func CanonicalSignString(p Payload) string {
	mods := "NM"
	if len(p.Mods) > 0 {
		upper := make([]string, 0, len(p.Mods))
		for _, mod := range p.Mods {
			upper = append(upper, strings.ToUpper(mod))
		}
		sort.Strings(upper)
		mods = strings.Join(upper, ",")
	}
	od := "-"
	if p.OD != nil {
		od = fmt.Sprintf("%.2f", *p.OD)
	}
	passed := 0
	if p.Passed {
		passed = 1
	}
	classic := 0
	if p.Classic {
		classic = 1
	}
	return fmt.Sprintf(
		"%s|%d|%s|%.3f|%s|%d|%.4f|%s|%s|%s|%s|%s|%.2f|%.1f|%.3f|%d|%d|%s|%d|%s|%s|%s|%d|%s",
		SignVersion, p.Bid, p.Checksum, p.Rate, mods, p.KeyCount, p.LnRatio, p.Pattern, p.ModeTag,
		od, p.ActualAlgorithm, p.SelectedAlgorithm, p.DisplayedVal, p.Vote, p.Weight,
		p.WeightVersion, passed, p.Client, classic, p.PluginVersion, p.SettingsHash,
		p.InstallID, p.Timestamp, p.Nonce,
	)
}

// ParsePublicKeys reads `keyid:hex,keyid2:hex`.
func ParsePublicKeys(text string) map[string]ed25519.PublicKey {
	out := make(map[string]ed25519.PublicKey)
	for _, part := range strings.Split(text, ",") {
		part = strings.TrimSpace(part)
		keyID, value, ok := strings.Cut(part, ":")
		if !ok {
			continue
		}
		raw, err := hex.DecodeString(strings.TrimSpace(value))
		if err != nil || len(raw) != ed25519.PublicKeySize {
			continue
		}
		out[strings.TrimSpace(keyID)] = ed25519.PublicKey(raw)
	}
	return out
}

// ValidateStatic checks every rule that does not need the osu! API.
// The returned reason is empty when the payload is acceptable.
func ValidateStatic(p Payload) string {
	if p.Bid <= 0 {
		return ReasonBadBid
	}
	if !checksumPattern.MatchString(strings.ToLower(p.Checksum)) {
		return ReasonChecksumInvalid
	}
	if !rateAllowed(p.Rate) {
		return ReasonBadRate
	}
	if len(p.Mods) > 8 {
		return ReasonBannedMod
	}
	for _, mod := range p.Mods {
		code := strings.ToUpper(strings.TrimSpace(mod))
		if keyModPattern.MatchString(code) {
			return ReasonKeyMod
		}
		if !AllowedMods[code] {
			return ReasonUnknownMod
		}
	}
	if p.ModeTag != "RC" {
		return ReasonNotRC
	}
	if !isFinite(p.LnRatio) || p.LnRatio < 0 || p.LnRatio > RCLnMax {
		return ReasonNotRC
	}
	if p.KeyCount != 4 && p.KeyCount != 6 && p.KeyCount != 7 {
		return ReasonBadKeyCount
	}
	if p.Client != "stable" && p.Client != "lazer" {
		return ReasonBadClient
	}
	if !spec.IsOfficialVersion(p.PluginVersion) {
		return ReasonBadVersion
	}
	if !p.Passed {
		return ReasonNotPassed
	}
	if !spec.IsAllowedAlgorithm(p.SelectedAlgorithm) {
		return ReasonBadAlgorithm
	}
	if _, ok := spec.NormalizeActualAlgorithm(p.ActualAlgorithm); !ok {
		return ReasonBadAlgorithm
	}
	if !isFinite(p.DisplayedVal) || p.DisplayedVal < DisplayedMin || p.DisplayedVal > DisplayedMax {
		return ReasonBadDisplayed
	}
	if p.WeightVersion != WeightVersion || !isFinite(p.Weight) || p.Weight < WeightMin || p.Weight > WeightMax {
		return ReasonBadWeight
	}
	if !isFinite(p.Vote) || p.Vote < -1 || p.Vote > 1 || math.Abs(p.Vote*2-math.Round(p.Vote*2)) > 1e-9 {
		return ReasonBadVote
	}
	// OD 是签名的客观字段，必须由壳从本地 .osu 解析后提供；缺失即拒绝，
	// 绝不用 osu! API 的值回填（否则 OD 校验形同虚设）。
	if p.OD == nil || !isFinite(*p.OD) || *p.OD < 0 || *p.OD > 10 {
		return ReasonBadOD
	}
	if len(p.SettingsHash) != 8 || !isHex(p.SettingsHash) {
		return ReasonBadSettingsHash
	}
	if len(p.InstallID) != 32 || !isHex(p.InstallID) {
		return ReasonBadJSON
	}
	if len(p.Pattern) == 0 || len(p.Pattern) > 32 {
		return ReasonBadJSON
	}
	return ""
}

func isFinite(v float64) bool {
	return !math.IsNaN(v) && !math.IsInf(v, 0)
}

func isHex(text string) bool {
	for _, c := range text {
		if !((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F')) {
			return false
		}
	}
	return len(text) > 0
}
