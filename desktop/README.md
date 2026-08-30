# ManiaMapAnalyser desktop shell

Tauri v2 桌面壳（Windows/Linux）：加载同一插件页面（在线 = tosu 插件页 /
离线 = 本壳 24061 静态服务），并作为本地聚合桥：

| 能力 | 端口/路径 | 说明 |
|---|---|---|
| Malody 编辑器 POST | 24060 | `{meta, chartText}` → requestId → song 帧 → 页面分析 → result 应答（200/500/504，见 `docs/CONTRACT.md`） |
| 静态服务（离线） | 24061 `/` | 服务插件目录（exe 相对路径探测） |
| WS | 24061 `/ws` | hello/state/song/settings/result/ping 帧（六型职责见 CONTRACT.md） |
| 设置 | 24061 `/settings` | 离线：GET 初始 / POST 变更（壳自有 JSON） |
| 封面 | 24061 `/cover/...` | 白名单具体文件（图片扩展名，同帧下发 URL） |
| Etterna 轮询 | — | 2Hz 轮询 `Save/LeosMmaBridge.txt`/`LeosMmaGameplay.txt`（M3 接入） |
| tosu 探测 | - | `tosu.env` 逐级向上探测（2–3 层）→ `GET {ip}:{port}/` 健康检查（30s 周期） |

窗口：透明/置顶（Win 穿透尽力，Linux 不做）；`WebviewUrl::External` 指向
tosu 插件页（在线）或 `http://127.0.0.1:24061/`（离线）。

构建与运行（开发态）：

```
cargo run --release
```

开发环境变量：

- `MMA_PLUGIN_DIR`：覆盖插件目录解析（缺省：exe 相对路径探测 `ManiaMapAnalyser
  by Leo_Black`）
- `MMA_SKIP_TOSU_PROBE=1`：跳过 tosu.env 探测（强制离线模式）

协议细节见 `docs/CONTRACT.md`（契约版本 1）。