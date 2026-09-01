# 桌面壳（desktop shell）技术文档

> 面向 AI 的技术文档（实现细节、检测逻辑、契约、构建）。**人类使用教程**见 [docs/shell-guide.md](../shell-guide.md)（安装/窗口操控/桥接线/故障排查）。多数据源整体架构见 [multi-source.md](multi-source.md)。

## 1. 定位与架构

桌面壳是可选的 Tauri v2 桌面窗口，同时是三大协作方：

1. **宿主窗口**：加载插件页（在线= tosu 插件页；离线=壳自身 24061 静态服务），提供置顶/无边框/透明形态，可覆盖在游戏（含全屏）之上。
2. **本地聚合桥**（无 tosu 环境时的数据面）：
   - `24060` HTTP POST——Malody 编辑器分析入口（经 WS 中转到页面）；
   - `24061` HTTP——插件页静态服务（离线模式）、`/settings`（离线设置）、`/cover/`（封面白名单）；
   - `24061/ws` WS——帧通道（hello/state/song/settings/result/control + ping），页面与壳双向；
   - Malody skin 状态文件写入（`mma_state.txt`，契约 §9）。
3. **Etterna 数据源轮询器**：2Hz 轮询桥文件（`Save/MmaBridge.txt` / `Save/MmaGameplay.txt`），解析后广播 song/state 帧。

模块：`desktop/src/{main,server,config,frames,etterna}.rs`；契约 `desktop/docs/CONTRACT.md`（v2）。

## 2. 启动流程（main.rs + config.rs）

```
plugin_dir() 解析（env MMA_PLUGIN_DIR 覆盖 → exe 上溯 0..=3 层找
  含 index.html 的 "ManiaMapAnalyser by Leo_Black" → 兜底相对路径）
probe_tosu_env()（exe 目录向上 ≤3 层找 tosu.env；MMA_SKIP_TOSU_PROBE 跳过）
  ├─ 命中且 tosu_online()（TCP connect 2s）→ url = http://{ip}:{port}/{插件目录 %20}/
  └─ 未命中/离线 → url = http://127.0.0.1:24061/
server::start（24060/24061 双监听 + 30s 定时帧 + etterna poller）
window.navigate(url)
```

目录检测（如实标注）：

- **插件目录**：`MMA_PLUGIN_DIR` 环境变量 > exe 目录逐级上溯 0–3 层，每层拼 `ManiaMapAnalyser by Leo_Black` 并校验 `index.html` 存在；都不中则用相对路径。发布形态（exe 与插件目录同层）上溯 0 层即命中；开发形态（target/debug）上溯 2 层命中仓库根。
- **tosu.env**：从 exe 所在目录开始向上（含当前层）最多 3 层；解析 `SERVER_PORT`（默认 24050）与 `SERVER_IP`（默认 127.0.0.1）；根目录 = tosu.env 所在目录（用于 `settings/{插件名}.json` 只读读取）。
- **Etterna/Malody 根**：`MMA_ETTERNA_ROOT` / `MMA_MALODY_ROOT` 环境变量 > **独立配置 `mma-shell.json`**（exe 旁，`{gameClient, etternaRoot, malodyRoot}`，可经 `/settings` POST 写入或直接编辑，30s 周期检测变化后重载并推送 settings 帧）> tosu 在线只读。无 tosu 用户无需下载 tosu 即可配置游戏路径。

## 3. 契约 v2 帧

| 帧 | 方向 | 载荷要点 |
| --- | --- | --- |
| hello | 壳→页 | `{contract: 2, shell}`；契约不匹配=终态（页面停止重连并提示） |
| state | 壳→页 | tosuOnline/errors/sources{etterna{alive,playing,playingExpireAt},malody{alive}} |
| song | 壳→页 | requestId/source/identity/modData{rate,...}/cover/rawText |
| settings | 双向 | 离线设置 JSON（在线只读不推） |
| result | 页→壳 | requestId/statusHint/errors/activeSource/star/pattern/updatedAt |
| control | 页→壳 | `{action: alwaysOnTop\|close\|clickThrough, value: bool}`（窗口操控；快捷键已全局化，此帧供未来 UI） |
| ping | 双向 | 15s keepalive |

## 4. 窗口操控（v2 起）

无边框（decorations:false）、透明、置顶（alwaysOnTop:true）为默认形态；`resizable:true`——**拖拽边缘改窗口尺寸**；**整窗移动**用页面顶部 `data-tauri-drag-region` 拖动把手（22px 发光条，中间 `⋮⋮` 提示）；页面缩放走 WebView 原生（`Ctrl+滚轮` / `Ctrl+=` / `Ctrl+-`）。

**全局快捷键**（tauri-plugin-global-shortcut，启动时注册，与页面焦点/连接无关，点击穿透时同样生效）：默认 `Ctrl+Shift+T` 置顶开关、`Ctrl+Shift+C` 点击穿透开关（`set_ignore_cursor_events`）、`Ctrl+Q` 关闭；**可经 `mma-shell.json` 的 `hotkeys` 覆盖**（避免与系统冲突），注册成败写入日志（`shortcut FAILED` 即冲突）。窗口位置/尺寸/置顶/穿透状态持久化到 `mma-shell-state.json`（exe 旁），启动恢复、切换与关闭时保存。Windows 透明白闪已由 `noRedirectionBitmap` 配置缓解（如仍有白闪属已知抖动）。

## 4b. 配置、日志与路径容错

- **mma-shell.json**（exe 旁，首启自动生成骨架）：`gameClient`/`etternaRoot`/`malodyRoot`/`hotkeys`/`logLevel`；损坏或非法字段自动回落默认并警告（不崩溃）。
- **路径容错**：根路径字段经 `normalize_path` 归一化——`\` 与 `/` 混用、尾部斜杠均处理（用户写 `D:\Games\Etterna` 或 `D:/Games/Etterna` 均可；JSON 内单反斜杠是非法转义，文档已提示用 `/` 或 `\\`）。
- **日志**：`mma-shell-{日序号}.log`（exe 旁）按日轮转、保留 7 个，每行 `[unix秒] [级别] 消息`；`log_level()` 按 `mma-shell.json logLevel` 过滤（debug/info/warn/error/off）。

## 5. 构建与发布

- 开发：`cargo build`（debug 保留控制台输出）。
- 发布：`cargo build --release`；`desktop/release.ps1` 将 exe 拷入插件目录并打 zip；**release 为 Windows GUI 子系统**（`#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`）——正式交付无命令行窗口。
- CI：`.github/workflows/shell-build.yml`——仅当 **main 分支 `desktop/**` 变更**时构建 **Windows**（release 产物上传 artifact）；`workflow_dispatch` 可手动触发。**Linux 构建已移除**：Etterna 无官方 Linux 版、Malody V 无 Linux 版，无对应受众。

## 6. 已知限制与待办

离线设置持久化已实现（`mma-shell.json` + 页面 `/settings` 应用，在线时仍以 tosu 为准只读）。外部源封面消费（壳 cover URL 已下发）为待办。真机验证项：Etterna 主题桥真实写入、Malody 编辑器 DoRequest 签名与自动化读谱（resolve 通道按标题扫 chart 目录；DoRequest 真机签名待验）、PlayMeta 字段、皮肤目录可写性；窗口穿透与透明目视。浏览器模式（无壳）不受影响：osu! 单源，control/result no-op。来源指示器：空心=无源；osu! 蓝 / Etterna 绿 / Malody 橙实心。