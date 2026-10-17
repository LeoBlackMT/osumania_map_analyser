// Package backup snapshots SQLite databases to Huawei Cloud OBS.
//
// A pass uploads a matched set — `telemetry-*.db`, `feedback-*.db` (when the
// feedback store exists), then `manifest-*.json` last. The manifest is the
// commit marker: without it the pair is incomplete. Pruning is per prefix so
// adding the second database does not halve the retention window.
package backup

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"io"
	"log"
	"os"
	"sort"
	"time"

	"github.com/huaweicloud/huaweicloud-sdk-go-obs/obs"

	"osumania-telemetry/internal/config"
	"osumania-telemetry/internal/store"
)

type Manager struct {
	cfg      config.Config
	store    *store.Store
	feedback *store.FeedbackStore
	version  string
	client   *obs.ObsClient
}

func New(cfg config.Config, st *store.Store, fb *store.FeedbackStore, version string) (*Manager, error) {
	if !cfg.BackupEnabled() {
		return &Manager{cfg: cfg, store: st, feedback: fb, version: version}, nil
	}
	client, err := obs.New(cfg.OBSAccessKey, cfg.OBSSecretKey, cfg.OBSEndpoint)
	if err != nil {
		return nil, err
	}
	return &Manager{cfg: cfg, store: st, feedback: fb, version: version, client: client}, nil
}

func (m *Manager) Enabled() bool { return m.client != nil }

// Run performs one backup pass immediately, then repeats daily at 03:00 UTC.
func (m *Manager) Run() {
	if m.client == nil {
		return
	}
	m.pass()
	for {
		time.Sleep(time.Until(nextRun()))
		m.pass()
	}
}

func (m *Manager) pass() {
	now := time.Now().UTC()
	m.backupSet("daily", now.Format("20060102"), m.cfg.DailyKeep)
	if now.Day() == 1 {
		m.backupSet("monthly", now.Format("2006-01"), m.cfg.MonthlyKeep)
	}
}

type manifestEntry struct {
	SHA256 string `json:"sha256"`
	Size   int64  `json:"size"`
}

type manifest struct {
	CreatedAt time.Time      `json:"created_at"`
	Version   string         `json:"version"`
	Telemetry *manifestEntry `json:"telemetry"`
	Feedback  *manifestEntry `json:"feedback,omitempty"`
}

// backupSet uploads one matched set. Any failure before the manifest aborts
// the set and skips pruning, so the previous complete set stays intact.
func (m *Manager) backupSet(prefix, stamp string, keep int) {
	telemetry, err := snapshot(m.store.SnapshotTo)
	if err != nil {
		log.Printf("backup: telemetry snapshot failed: %v", err)
		return
	}
	defer os.Remove(telemetry.path)
	if err := m.put(prefix+"/telemetry-"+stamp+".db", telemetry.path); err != nil {
		log.Printf("backup: telemetry upload failed: %v", err)
		return
	}

	doc := manifest{CreatedAt: time.Now().UTC(), Version: m.version, Telemetry: telemetry.entry}

	if m.feedback != nil {
		fb, err := snapshot(m.feedback.SnapshotTo)
		if err != nil {
			log.Printf("backup: feedback snapshot failed: %v", err)
			return
		}
		defer os.Remove(fb.path)
		if err := m.put(prefix+"/feedback-"+stamp+".db", fb.path); err != nil {
			log.Printf("backup: feedback upload failed: %v", err)
			return
		}
		doc.Feedback = fb.entry
	}

	body, err := json.MarshalIndent(doc, "", "  ")
	if err != nil {
		log.Printf("backup: manifest encode failed: %v", err)
		return
	}
	if err := m.putBytes(prefix+"/manifest-"+stamp+".json", body); err != nil {
		log.Printf("backup: manifest upload failed: %v", err)
		return
	}

	for _, item := range []struct {
		prefix string
		keep   int
	}{
		{prefix + "/telemetry-", keep},
		{prefix + "/feedback-", keep},
		{prefix + "/manifest-", keep},
	} {
		if err := m.prune(item.prefix, item.keep); err != nil {
			log.Printf("backup: prune %s failed: %v", item.prefix, err)
		}
	}
}

type snapshotFile struct {
	path  string
	entry *manifestEntry
}

func snapshot(take func(string) error) (snapshotFile, error) {
	tmp, err := os.CreateTemp("", "mma-backup-*.db")
	if err != nil {
		return snapshotFile{}, err
	}
	path := tmp.Name()
	tmp.Close()
	os.Remove(path) // VACUUM INTO must create the target itself.

	if err := take(path); err != nil {
		os.Remove(path)
		return snapshotFile{}, err
	}
	info, err := os.Stat(path)
	if err != nil {
		os.Remove(path)
		return snapshotFile{}, err
	}
	file, err := os.Open(path)
	if err != nil {
		os.Remove(path)
		return snapshotFile{}, err
	}
	defer file.Close()
	hasher := sha256.New()
	if _, err := io.Copy(hasher, file); err != nil {
		os.Remove(path)
		return snapshotFile{}, err
	}
	return snapshotFile{
		path: path,
		entry: &manifestEntry{
			SHA256: hex.EncodeToString(hasher.Sum(nil)),
			Size:   info.Size(),
		},
	}, nil
}

func (m *Manager) put(key, path string) error {
	input := &obs.PutFileInput{}
	input.Bucket = m.cfg.OBSBucket
	input.Key = key
	input.SourceFile = path
	_, err := m.client.PutFile(input)
	return err
}

func (m *Manager) putBytes(key string, body []byte) error {
	input := &obs.PutObjectInput{}
	input.Bucket = m.cfg.OBSBucket
	input.Key = key
	input.Body = bytes.NewReader(body)
	_, err := m.client.PutObject(input)
	return err
}

func (m *Manager) prune(prefix string, keep int) error {
	if keep <= 0 {
		return nil
	}
	input := &obs.ListObjectsInput{}
	input.Bucket = m.cfg.OBSBucket
	input.Prefix = prefix
	output, err := m.client.ListObjects(input)
	if err != nil {
		return err
	}

	keys := make([]string, 0, len(output.Contents))
	for _, c := range output.Contents {
		keys = append(keys, c.Key)
	}
	sort.Strings(keys)
	if len(keys) <= keep {
		return nil
	}

	for _, k := range keys[:len(keys)-keep] {
		del := &obs.DeleteObjectInput{}
		del.Bucket = m.cfg.OBSBucket
		del.Key = k
		if _, err := m.client.DeleteObject(del); err != nil {
			log.Printf("backup: delete %s failed: %v", k, err)
		}
	}
	return nil
}

// nextRun returns the next 03:00 UTC instant.
func nextRun() time.Time {
	now := time.Now().UTC()
	next := time.Date(now.Year(), now.Month(), now.Day(), 3, 0, 0, 0, time.UTC)
	if !next.After(now) {
		next = next.AddDate(0, 0, 1)
	}
	return next
}
