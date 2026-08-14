// Package backup snapshots the SQLite database to Huawei Cloud OBS: a daily
// backup (kept 30) plus a monthly archive (kept 12). It is entirely optional —
// when any OBS credential is empty the manager is a no-op and failures only log.
package backup

import (
	"log"
	"os"
	"sort"
	"time"

	"github.com/huaweicloud/huaweicloud-sdk-go-obs/obs"

	"osumania-telemetry/internal/config"
	"osumania-telemetry/internal/store"
)

type Manager struct {
	cfg    config.Config
	store  *store.Store
	client *obs.ObsClient
}

func New(cfg config.Config, st *store.Store) (*Manager, error) {
	if !cfg.BackupEnabled() {
		return &Manager{cfg: cfg, store: st}, nil
	}
	client, err := obs.New(cfg.OBSAccessKey, cfg.OBSSecretKey, cfg.OBSEndpoint)
	if err != nil {
		return nil, err
	}
	return &Manager{cfg: cfg, store: st, client: client}, nil
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

	if err := m.backupTo("daily/telemetry-" + now.Format("20060102") + ".db"); err != nil {
		log.Printf("backup: daily snapshot failed: %v", err)
	} else if err := m.prune("daily/", m.cfg.DailyKeep); err != nil {
		log.Printf("backup: prune daily failed: %v", err)
	}

	if now.Day() == 1 {
		if err := m.backupTo("monthly/telemetry-" + now.Format("2006-01") + ".db"); err != nil {
			log.Printf("backup: monthly snapshot failed: %v", err)
		} else if err := m.prune("monthly/", m.cfg.MonthlyKeep); err != nil {
			log.Printf("backup: prune monthly failed: %v", err)
		}
	}
}

func (m *Manager) backupTo(key string) error {
	tmp, err := os.CreateTemp("", "telemetry-backup-*.db")
	if err != nil {
		return err
	}
	tmpPath := tmp.Name()
	tmp.Close()
	os.Remove(tmpPath) // VACUUM INTO must create the target itself.

	if err := m.store.SnapshotTo(tmpPath); err != nil {
		return err
	}
	defer os.Remove(tmpPath)

	input := &obs.PutFileInput{}
	input.Bucket = m.cfg.OBSBucket
	input.Key = key
	input.SourceFile = tmpPath
	_, err = m.client.PutFile(input)
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
