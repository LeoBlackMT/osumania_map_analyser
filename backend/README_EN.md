# osumania-telemetry

A small, self-contained Go server that collects **anonymous usage statistics** from the [osumania_map_analyser](https://github.com/LeoBlackMT/osumania_map_analyser) tosu plugin and exposes a **public aggregate dashboard**.

No user accounts, no login, no personally identifiable information. The plugin sends an anonymous install id (a random UUID stored in `localStorage`) plus aggregate analysis metadata; the server stores and displays only aggregates.

> 中文说明见 [README.md](README.md)。

## Table of contents

- [What it does](#what-it-does)
- [Privacy](#privacy)
- [Architecture](#architecture)
- [Directory layout](#directory-layout)
- [Quick start](#quick-start)
- [Configuration](#configuration)
- [HTTP API](#http-api)
- [Rate limiting](#rate-limiting)
- [Data retention](#data-retention)
- [Deployment (Linux)](#deployment-linux)
- [Development](#development)

## What it does

- Ingests events at `POST /api/v1/event` (`boot`, `heartbeat`, `analyze`).
- Stores them in a single SQLite file (`installs` + `events`).
- Computes aggregate statistics and serves them at `GET /api/v1/stats`.
- Renders a public dashboard at `/` (no chart library — inline SVG/CSS only).
- Automatically deletes events older than `MMA_TELEMETRY_RETENTION_DAYS`.
- Optionally snapshots the database to Huawei Cloud OBS (daily ×30 + monthly ×12).

## Privacy

**Collected** (per `analyze` event): anonymous install id, event kind, server-side UTC timestamp, plugin version, selected/actual estimator algorithm, key count (4/6/7K), mods and speed rate, mode tag (HB/RC/LN/Mix/SV), estimated star rating, LN ratio, key-type breakdown, and analysis duration.

**Never collected or stored**: usernames, player ids, scores/acc, beatmap md5/title, IP addresses (not even a hash), user agent, OS, or timezone. The ingest handler applies a server-side whitelist and drops anything else.

The dashboard and `/api/v1/stats` expose **aggregates only** — never an individual install id, event, or IP.

## Architecture

```
plugin (browser) ──POST /api/v1/event──▶ rate limit ──▶ whitelist ──▶ SQLite
                                                                    │
public dashboard ◀── /api/v1/stats (60s cache) ◀── aggregate queries ◀─┘
```

## Directory layout

```
backend/
  cmd/server/main.go            entrypoint + wiring + retention loop
  internal/config/              .env loading + validation
  internal/ratelimit/           in-memory fixed-window limiter
  internal/store/               SQLite schema + queries
  internal/telemetry/           POST /api/v1/event handler
  internal/analytics/           aggregation + distributions
  internal/web/                 / dashboard + /api/v1/stats (+ embedded HTML)
  internal/backup/              optional Huawei OBS snapshots
```

## Quick start

Requires Go 1.22+.

```bash
cd backend
cp .env.example .env      # edit as needed (works with defaults too)
go build -o telemetry-server ./cmd/server
./telemetry-server
```

Then open <http://localhost:8080/> for the dashboard, or post a test event:

```bash
curl -d '{"id":"00000000-0000-4000-8000-000000000000","kind":"boot","version":"1.7.4"}' \
     http://localhost:8080/api/v1/event -i
# expect: HTTP/1.1 204 No Content
```

## Configuration

Configuration is read from a `.env` file (and can be overridden by real environment variables). See [`.env.example`](.env.example). Key variables:

| Variable | Default | Meaning |
| --- | --- | --- |
| `MMA_TELEMETRY_ADDR` | `:8080` | Listen address (bind `127.0.0.1:8080` behind a proxy) |
| `MMA_TELEMETRY_DB` | `telemetry.db` | SQLite path |
| `MMA_TELEMETRY_RETENTION_DAYS` | `365` | Event retention |
| `MMA_TELEMETRY_ONLINE_WINDOW_MIN` | `10` | "Online" = last_seen within N minutes |
| `MMA_TELEMETRY_RATE_LIMIT_PER_MIN` | `120` | Ingest requests/min/IP (0 disables) |
| `MMA_TELEMETRY_STATS_CACHE_SECONDS` | `60` | `/api/v1/stats` aggregate cache |
| `MMA_BACKUP_OBS_*` | empty | Huawei OBS backup credentials (empty = disabled) |

## HTTP API

### `POST /api/v1/event`

Body (max 16 KB):

```json
{
  "id": "<uuid>",
  "kind": "boot | heartbeat | analyze",
  "version": "1.7.4",
  "data": { "algorithm": "Mixed", "keycount": 4, "...": "whitelisted fields only" }
}
```

Returns `204 No Content`. `400` for a bad body, `405` for wrong method, `429` when rate limited. The `data` object is filtered server-side to these keys only: `algorithm`, `actualAlgorithm`, `keycount`, `mods`, `speedRate`, `mode`, `star`, `lnRatio`, `typeBreakdown`, `durationMs`.

CORS is enabled for `POST`/`OPTIONS` (the plugin runs on `http://localhost:24050`).

### `GET /api/v1/stats`

Returns the aggregate JSON used by the dashboard. Public, cached.

### `GET /`

The public dashboard (HTML).

## Rate limiting

A fixed-window limiter keyed by client IP, kept **in memory only** (never persisted or logged). It trusts `X-Forwarded-For` because the intended deployment binds the server to loopback behind a reverse proxy — **do not expose the port directly**, or rate limiting can be spoofed.

## Data retention

A background loop deletes `events` older than `MMA_TELEMETRY_RETENTION_DAYS` (default 365). The `installs` table is tiny and kept. Aggregates are computed on demand and cached for 60 s.

## Deployment (Linux)

Cross-compile from any machine (no Docker, no cgo). `-X main.version=` injects the backend version (returned as `serverVersion` in `/api/v1/stats`, shown in the dashboard footer and the startup log); without it the build reports `dev`:

```bash
cd backend
CGO_ENABLED=0 GOOS=linux GOARCH=amd64 go build -trimpath -ldflags="-X main.version=1.0.0 -s -w" -o bin/telemetry-server ./cmd/server
```

On the server:

1. Upload the binary, create a dedicated user, and place `.env` (mode `600`).
2. Install a systemd unit (see below) and `systemctl enable --now osumania-telemetry`.
3. Put a reverse proxy in front (Caddy for automatic HTTPS):

```caddyfile
mma-stats.leoblack.top {
    reverse_proxy 127.0.0.1:8080
}
```

4. Open only 80/443 in the firewall. Bind the service to `127.0.0.1:8080`.

Example systemd unit:

```ini
[Unit]
Description=osumania-telemetry
After=network.target

[Service]
User=osumatelemetry
EnvironmentFile=/etc/osumania-telemetry/.env
ExecStart=/usr/local/bin/telemetry-server
Restart=always
NoNewPrivileges=yes
ProtectSystem=strict

[Install]
WantedBy=multi-user.target
```

### Optional OBS backup

Create a **private** Huawei Cloud OBS bucket, create an **IAM sub-user** with only write/delete/list permission on that bucket, then fill `MMA_BACKUP_OBS_AK/SK/ENDPOINT/BUCKET` in `.env`. The server snapshots the DB daily (kept 30) and archives monthly (kept 12). Backup failures only log — they never affect serving.

## Development

- Go 1.22+, pure standard library except two dependencies: `modernc.org/sqlite` (pure-Go SQLite, no cgo) and `huaweicloud-sdk-go-obs` (Huawei OBS SDK, pure Go).
- All SQL uses parameterized queries; the dashboard renders all dynamic values via `textContent` (XSS-safe).
- `go build ./...` and `go vet ./...` should pass before committing.
