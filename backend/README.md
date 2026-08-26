# osumania-telemetry

一个自包含的小型 Go 服务：收集 [osumania_map_analyser](https://github.com/LeoBlackMT/osumania_map_analyser) tosu 插件的**匿名使用统计**，并提供**公开聚合看板**。

没有用户系统、没有登录、没有任何个人可识别信息。插件只上报匿名安装 ID（`localStorage` 中的随机 UUID）与聚合分析元数据；服务端只存储与展示聚合结果。

> English version: [README_EN.md](README_EN.md)。

## 目录

- [功能](#功能)
- [隐私](#隐私)
- [架构](#架构)
- [目录结构](#目录结构)
- [快速开始](#快速开始)
- [配置](#配置)
- [HTTP 接口](#http-接口)
- [限流](#限流)
- [数据保留](#数据保留)
- [部署（Linux）](#部署linux)
- [开发](#开发)

## 功能

- 在 `POST /api/v1/event` 接收事件（`boot`、`heartbeat`、`analyze`）。
- 写入单个 SQLite 文件：**写路径全聚合**——每个事件在同一事务里落原始日志 + 更新聚合表（`daily_agg` / `install_days` / `install_hours`）。
- 在 `GET /api/v1/stats` 提供聚合统计（只读小表，毫秒级，与事件总量无关；窗口 1d/7d/30d/90d + 自定义日期范围）。
- 在 `/` 渲染公开看板（无图表库，纯内联 CSS/SVG；1d 窗口显示小时粒度趋势）。
- 原始事件日志保留 `MMA_TELEMETRY_RAW_RETENTION_DAYS`（默认 14 天）后自动清理；所有统计口径由永久聚合表承载，不受保留期影响。
- 可选：把数据库快照备份到华为云 OBS（每日 ×30 + 每月 ×12）。
- 一次性重建命令：`telemetry-server -migrate`（停服状态下从原始事件清空重建全部聚合，幂等）。

## 隐私

**采集**（每个 `analyze` 事件）：匿名安装 ID、事件类型、服务器 UTC 时间戳、插件版本、所选/实际运行算法、键数（4/6/7K）、mod 与变速、模式标签（HB/RC/LN/Mix/SV）、估算星数、LN 比例、键型占比、分析耗时。

**永不采集/存储**：用户名、玩家 id、分数/acc、谱面 md5/标题、IP 地址（连哈希都不存）、UA、操作系统、时区。采集端在服务端做字段白名单过滤，其余一律丢弃。

看板与 `/api/v1/stats` 只暴露**聚合结果**——绝不暴露单个安装 ID、单条事件或 IP。

## 架构

```
插件(浏览器) ──POST /api/v1/event──▶ 内存限流 ──▶ 字段白名单 ──▶ SQLite
                                                                   │
公开看板 ◀── /api/v1/stats(60s缓存) ◀── 聚合查询 ◀─────────────────┘
```

## 目录结构

```
backend/
  cmd/server/main.go            入口 + 装配 + 保留期清理协程
  internal/config/              .env 加载与校验
  internal/ratelimit/           内存固定窗口限流器
  internal/store/               SQLite schema 与查询
  internal/telemetry/           POST /api/v1/event 处理器
  internal/analytics/           聚合与分布统计
  internal/web/                 / 看板 + /api/v1/stats（含内嵌 HTML）
  internal/backup/              可选华为云 OBS 快照
```

## 快速开始

需要 Go 1.22+。

```bash
cd backend
cp .env.example .env      # 按需修改（用默认值也能跑）
go build -o telemetry-server ./cmd/server
./telemetry-server
```

然后打开 <http://localhost:8080/> 查看看板，或用 curl 发一条测试事件：

```bash
curl -d '{"id":"00000000-0000-4000-8000-000000000000","kind":"boot","version":"1.7.4"}' \
     http://localhost:8080/api/v1/event -i
# 期望: HTTP/1.1 204 No Content
```

## 配置

配置从 `.env` 文件读取（真实环境变量可覆盖）。参见 [`.env.example`](.env.example)。主要变量：

| 变量 | 默认值 | 说明 |
| --- | --- | --- |
| `MMA_TELEMETRY_ADDR` | `:8080` | 监听地址（反代后面绑定 `127.0.0.1:8080`） |
| `MMA_TELEMETRY_DB` | `telemetry.db` | SQLite 路径 |
| `MMA_TELEMETRY_RAW_RETENTION_DAYS` | `14` | 原始事件日志保留天数（调试/重建源；不影响统计口径） |
| `MMA_TELEMETRY_ACTIVE_MIN` | `10` | 「活跃」= 当日 ≥N 次 analyze |
| `MMA_TELEMETRY_HOUR_RETENTION_DAYS` | `90` | `install_hours` 滚动窗口（24h 分布/小时趋势上限） |
| `MMA_TELEMETRY_ONLINE_WINDOW_MIN` | `10` | 「在线」= last_seen 在 N 分钟内 |
| `MMA_TELEMETRY_RATE_LIMIT_PER_MIN` | `120` | 采集请求限速（次/分/IP，0 关闭） |
| `MMA_TELEMETRY_STATS_CACHE_SECONDS` | `60` | `/api/v1/stats` 聚合缓存秒数 |
| `MMA_BACKUP_OBS_*` | 空 | 华为云 OBS 备份凭据（空 = 禁用） |

## HTTP 接口

### `POST /api/v1/event`

请求体（最大 16 KB）：

```json
{
  "id": "<uuid>",
  "kind": "boot | heartbeat | analyze",
  "version": "1.7.4",
  "data": { "algorithm": "Mixed", "keycount": 4, "...": "仅白名单字段" }
}
```

返回 `204 No Content`。请求体错误返回 `400`，方法错误返回 `405`，被限流返回 `429`。`data` 对象在服务端只保留以下键：`algorithm`、`actualAlgorithm`、`keycount`、`mods`、`speedRate`、`mode`、`star`、`lnRatio`、`typeBreakdown`、`durationMs`、`numericDifficulty`（可选，标准数值化难度 .0=mid，见 `docs/features/telemetry.md`）。

对 `POST`/`OPTIONS` 开放 CORS（插件运行在 `http://localhost:24050`）。

### `GET /api/v1/stats`

返回看板使用的聚合 JSON。公开，带缓存。

### `GET /`

公开看板（HTML）。

## 限流

固定窗口限流器，按客户端 IP 计键，**只存内存**（绝不持久化或写日志）。它信任 `X-Forwarded-For`，因为预期部署方式是服务绑定回环地址、前面放反代——**不要把端口直接暴露公网**，否则限流可被伪造绕过。

## 数据保留

三档独立保留（见 `data-model.md`）：

- **原始事件 `events`**：`MMA_TELEMETRY_RAW_RETENTION_DAYS`（默认 14）——纯调试日志，删了不影响任何统计；
- **小时表 `install_hours`**：`MMA_TELEMETRY_HOUR_RETENTION_DAYS`（默认 90）——24h 分布/小时趋势的窗口上限；
- **聚合表 `daily_agg` / `install_days` / `installs`**：永久——所有看板统计的事实来源。

后台循环每 24h 清理一次；「活跃」口径 = 当日 ≥ `MMA_TELEMETRY_ACTIVE_MIN`（默认 10）次 analyze，由永久表承载，与清理策略完全解耦。聚合按窗口缓存 60s。

## 部署（Linux）

在任何机器上交叉编译（无需 Docker、无 cgo）。`-X main.version=` 注入后端版本（`/api/v1/stats` 返回 `serverVersion`、看板 footer 显示、启动日志打印）；不注入则为 `dev`：

```bash
cd backend
CGO_ENABLED=0 GOOS=linux GOARCH=amd64 go build -trimpath -ldflags="-X main.version=1.1.2 -s -w" -o bin/telemetry-server ./cmd/server
```

在服务器上：

1. 上传二进制、创建专用用户、放置 `.env`（权限 `600`）。
2. 安装 systemd unit（见下）并 `systemctl enable --now osumania-telemetry`。
3. 前面放反代（Caddy 自动 HTTPS）：

```caddyfile
mma-stats.leoblack.top {
    reverse_proxy 127.0.0.1:8080
}
```

4. 防火墙只放行 80/443，服务绑定 `127.0.0.1:8080`。

systemd unit 示例：

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

### 可选：OBS 备份

创建**私有**华为云 OBS 桶，创建**IAM 子用户**并只授该桶的写/删/列权限，然后把 `MMA_BACKUP_OBS_AK/SK/ENDPOINT/BUCKET` 填入 `.env`。服务每日快照数据库（保留 30 份）、每月归档（保留 12 份）。备份失败只记日志，绝不影响服务。

## 开发

- Go 1.22+，除两个依赖外纯标准库：`modernc.org/sqlite`（纯 Go SQLite，无 cgo）与 `huaweicloud-sdk-go-obs`（华为云 OBS SDK，纯 Go）。
- 所有 SQL 使用参数化查询；看板所有动态值通过 `textContent` 渲染（防 XSS）。
- 提交前 `go build ./...` 与 `go vet ./...` 应通过。
