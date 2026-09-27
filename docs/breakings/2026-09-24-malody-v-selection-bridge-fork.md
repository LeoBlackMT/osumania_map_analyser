# 2026-09-24 — Malody V 选曲桥改为自建 fork（契约 v5）

> 破坏性变更记录。格式：改了什么 / 为什么 / 影响范围 / 兼容性 / 验证方式。

## 1. 改了什么

| 面 | 之前 | 现在 |
|---|---|---|
| 游戏侧插件 | 上游 `MalodyInsightBridge.dll`（GUID `local.malody.insight.selection`） | **本仓库自建 fork** `MMAMalodySelection.dll`（GUID `local.mma.malody.selection`），源码在 `bridges/malody/bepinex/plugin/` |
| 插件能力 | 8 字段载荷，无判定档 / Pro / Turbo | **11 字段**：新增 `judge_level` / `pro_judge` / `turbo` |
| 游戏内叠加界面 | 有（overlay / 字体光栅化 / 布局等 14 个文件约 2294 行） | **已整块移除**，插件只剩数据通道（859 行） |
| 壳桥契约 | v4 | **v5**：`song` 帧与 `sources.malody` 都新增 `pro`/`turbo`，`winScale` 可为 `null` |
| 页面接受区间 | `[3,4]` | **`[3,5]`** |
| 转换器 OD | Malody V 一律默认 OD 9 | **动态 OD**：判定档 × Pro（严格/常态组）× 倍率，经 `js/app/sources/odResolver.js` |
| 缓存键 | 5 段 | **最多 7 段**：新增窗口缩放段与 Pro 段 |
| 游戏内 cfg | 安装器创建 `local.malody.insight.selection.cfg` 并写 `[Overlay] Enabled = false` | **安装器不再写任何 cfg**；`local.mma.malody.selection.cfg` 由 BepInEx 首次加载时自行生成 |

## 2. 为什么

原来的通道有三个绕不过去的问题：

1. **只读文件、且判定/速率拿不到**——Malody V 的 Lua 通道只能写文件且强制加谱面名前缀，判定档与倍率都拿不到，于是 OD 只能写死 9、星数不随 Mod 变化。
2. **上游插件是"编辑器 + 叠加界面"的形态**，我们只需要数据桥，却在游戏里多渲染一层界面；而且它的载荷不含判定档/Pro/Turbo，无法支撑"Pro = 严格组"这条换算。
3. **"Pro 是否开启"是本项目唯一能区分两套判定窗口的信息**，只有游戏内插件能读到（`config.json` 没有对应键）。

因此改为自带一份**最小数据桥** fork：保留全部数据挂点，删掉全部叠加界面，并补上三个新字段。**Pro 的读取链**经过实机验证：面板记录 `PanelJudge.del.proJudge` → 与面板无关的播放设置记录 `Malody.Play.boq.get_bcgh` → 具名控件；**设置文件永不作为 Pro 来源**。

## 3. 影响范围

- **Malody V 源的星数会变**：OD 从写死 9 变为动态（判定档 C/常态组 ≈ 4.87；Pro 严格组 ≈ 8.08；Turbo 1.2 ≈ 7.75）。这是本次变更的主要可见结果。
- **需要重新跑一次安装器**（见下"兼容性"）。装过上游插件的用户还必须**自行删除**旧的 `MalodyInsightBridge.dll`——安装器只报告、绝不替用户删除（两份插件共存会双 Hook）。
- **其余三个源（osu!、Etterna、Malody 4.3.7）不受影响**，代码路径未改。
- 判定窗口与倍率的语义分两条轴：**倍率**进管线的 `musicRate`（密度与反应时间），**窗口**经等效 OD 表达——两者独立、都不重复计算。

## 4. 兼容性

- **旧插件 + 新壳**：可用。新字段是 `#[serde(default)]` 容错的，缺字段不拒收；此时 `pro`/`turbo` 为 `null`，页面**关闭动态 OD 并在状态行说明原因**（不会静默按常态组冒充）。
- **新插件 + 旧壳（v4）**：页面按字段**逐个存在性**降级，不按版本号分支；新字段被忽略，卡片仍工作但没有 Pro/Turbo 语义。
- **页面契约区间**：`3 <= contract <= 5` 一律接受，`2` 与 `6` 进终态。**壳与页面两处常量必须同步升版**——只升一侧会让 hello 越界，`bridgeOnline()` 变假，除了数据帧不通，`sendControl()` 也会一起失效（拖动把手与置顶/穿透/关闭快捷键）。
- **历史文档豁免**：`docs/breakings/` 下本次之前的文档描述的是 **fork 之前**的形态（例如 4.3.7 相关的旧说明提到"Malody V 绝不传 OD 参数"），保留原样作为历史记录，不代表当前实现。

## 5. 验证方式

- **插件离线断言**：`bridges/malody/bepinex/plugin/tests/`（206 条，本地不入库，含 11 字段键名、读取链三级、降级两组、日志文本）。
- **构建**：`pwsh -File bridges/malody/bepinex/plugin/build.ps1 -BepInExDir <…> -GameInteropDir <…>`，要求 0 error / 0 warning，并把产物复制到 `plugin/MMAMalodySelection.dll`（安装器读这一份）。
- **壳**：`cd desktop; cargo test`（169 passed / 0 failed）；真壳端到端 `temp/t3-build/acs-live.mjs`（51/51）。
- **OD 换算表可复算**：`python tools/malody-v-od-check/verify.py --check-js`（自检残差 ≤ 0.05 ms、Pro 恒高于非 Pro、同倍率 TURBO 恒高于 DASH；**改错页面表里任一个值即以非 0 退出**）。端到端方向判据见 `.omo/evidence/malody-v-selection-bridge-fork/t7b-od-end-to-end.txt`。
- **冒烟**：`smoke-malody-bridge.mjs`（82/82）、`smoke-bridge.mjs`（20/20）、`smoke-etterna.mjs`（11/11）。
- **安装器**：fake root 上的安装/卸载/篡改/上游共存四条路径。
- **实机**：`BepInEx\LogOutput.log` 出现 `Selection bridge ready`，且不打开任何面板时日志即有 `pro_judge=… tier=play-record`。

# 2026-09-24 — Malody V selection bridge moved to a self-built fork (contract v5)

## 1. What changed

The game-side plugin is now **this repository's own fork** `MMAMalodySelection.dll` (GUID `local.mma.malody.selection`, sources in `bridges/malody/bepinex/plugin/`) instead of the upstream `MalodyInsightBridge.dll`; the payload grew from 8 to **11 fields** (`judge_level` / `pro_judge` / `turbo`); the in-game overlay layer (~2294 lines across 14 files) was deleted, leaving the data bridge only; the shell contract went **v4 → v5** (`song` and `sources.malody` both carry `pro`/`turbo`, `winScale` may be `null`); the page now accepts `[3,5]`; Malody V gained **dynamic OD** (judge level × Pro × rate via `js/app/sources/odResolver.js`) where it previously always used OD 9; the cache key grew to at most **7 segments**; and the installer **no longer writes any cfg** (BepInEx generates `local.mma.malody.selection.cfg` itself).

## 2. Why

The old channel could not carry the information this feature needs. The Lua file channel cannot see the judge level or the rate, so OD was hard-coded to 9 and star ratings never moved with mods. The upstream plugin is an "editor plus overlay" build when only the data bridge is wanted, and its payload has no judge/Pro/Turbo. **Pro is the only signal that distinguishes the two judge-window groups and it exists only inside the game process**, so an in-game plugin is required. The fork keeps every data hook, drops the whole overlay, and adds the three fields; Pro is read through a verified chain (panel record → the panel-independent play-settings record `Malody.Play.boq.get_bcgh` → a named control), and never from the settings file.

## 3. Impact

Malody V star ratings change (OD is now dynamic: judge C with Pro off ≈ 4.87, Pro on ≈ 8.08, Turbo 1.2 ≈ 7.75). **Users must re-run the installer**, and anyone who had the upstream plugin installed must **delete `MalodyInsightBridge.dll` themselves** — the installer only reports it, because co-installing both double-hooks the game. osu!, Etterna and Malody 4.3.7 are unaffected. Rate and judge windows remain two independent axes: the rate goes into the pipeline's `musicRate`, the window enters through the equivalent OD, and neither is counted twice.

## 4. Compatibility

Old plugin with a new shell works (the new fields are `serde(default)`, so a missing field is never rejected; `pro`/`turbo` become `null` and the page turns dynamic OD off with a reason instead of silently assuming the normal group). A new plugin with an old v4 shell degrades per field, not per version number. The page accepts contracts `3..5`; `2` and `6` are terminal. **The shell and page constants must be bumped together** — bumping only one side pushes `hello` out of range, `bridgeOnline()` turns false and `sendControl()` dies with it (drag handle plus the topmost / click-through / close shortcuts). Documents under `docs/breakings/` dated before this one describe the pre-fork shape and are kept as history.

## 5. How to verify

Plugin offline assertions (`plugin/tests/`, 206 checks, local-only); `build.ps1` must report 0 errors / 0 warnings and stage `plugin/MMAMalodySelection.dll`; `cargo test` in `desktop/` (169 passed); live shell acceptance (`51/51`); the OD table is recomputed and checked by `python tools/malody-v-od-check/verify.py --check-js` (residual ≤ 0.05 ms, Pro above non-Pro, TURBO above DASH at equal rate; a single wrong stored value exits non-zero); the smoke scripts (82/82, 20/20, 11/11); the installer's four fake-root paths; and a real run whose `BepInEx\LogOutput.log` shows `Selection bridge ready` and a `pro_judge=… tier=play-record` line **without opening any panel**.
