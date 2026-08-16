# 预设系统（Preset System）

> 本文档说明预设系统的功能、架构与使用方法。目标读者：开发者与 AI。
> 对应的用户文档见 [docs/settings.md](../settings.md) 的「预设（Preset）」部分。

## 功能说明

预设系统允许用户一键应用或保存整套插件配置（快照覆盖 32 项设置），由 `js/app/presets.js`（约 1690 行）实现。

- **12 个系统预设**（只读，定义于 `PRESET_DEFS`，每个 = `APP_CONFIG.defaults` 全量 + 覆盖）：
  Default / mini / For osu Player / For Etterna Player / For Interlude Player / Pattern Focus /
  Full Overview / Vibro Player / Jack Player / The Limit Does Not Exist / Daniel-like / Wild Dan (WIP)。
- **自定义预设**：用户保存的完整配置快照，数量不限（受浏览器 localStorage 容量限制）。
- **Last Saved Preset 自动跟随**（Auto 模式）：未锚定自定义预设时，用户在 tosu 设置页的手动修改自动存入该容器；它是一个跟随标记，**不是**可应用的快照（系统保留名，不可改名/删除）。
- **锚定行为**：选中自定义预设后手动修改 → 自动覆盖该预设内容；选中内置预设后手动修改 → 修改内容进入 Last Saved Preset（绝不覆盖系统预设）。
- **固定锚定槽**：Custom 1 / Custom 2 / Custom 3 首次加载自动创建，不可重命名/删除；在 dashboard 下拉选择它们时才按当前配置物化。

## 使用方法

### tosu 设置页（dashboard）

- `settings.json` 新增 `preset` 设置项（options 类型，位于 Links 分组之后、Modules 分组之前）。
- 选择任一预设 → 立即应用（覆盖下方所有设置）并写回 tosu。
- 选择 **Last Saved Preset** → 进入跟随模式，后续手动修改自动保存到其中。

### 预设管理器（浏览器，`?edit=1`）

- 打开 `http://localhost:24050/<插件目录名>/index.html?edit=1`，页面右侧出现 Presets 面板。
- 面板能力：**Apply**（应用）、**Rename**（重命名）、**Delete**（删除，需 confirm）、底部 **Save current**（把当前配置保存为新预设；同名即覆盖）。
- 面板分两组：My Presets（用户预设；Custom 1-3 固定槽仅可 Apply）与 System（12 个系统预设 + 只读的 Last Saved Preset）。

## 架构

### 模块定位

- 完全自包含：不改 `settings.js` / `appContext.js` / `config.js` / `index.html` 的既有逻辑。
- 仓库改动仅四处：`settings.json` 新增 `preset` 设置项与 `hPresets` 分组、`js/app/presets.js`（新文件）、`styles/presets.css`（管理器样式，动态注入）、`js/app/main.js` 一行副作用导入（模块加载即自初始化）。

### 数据流

```
tosu WebSocket 设置广播
  → presets.js 自己的 /websocket/commands 连接（socket.commands(handleSettingsPacket)）
  → extractSettingsPayload / snapshotOf 解析全量快照 + extractPresetValue 读 preset 选项值
  → 首包：记录基线 lastValues + 恢复激活预设（localStorage）
  → 后续包快照 diff（hasKeyChanged，排除 wsEndpoint）：
      ├─ preset 值变化（picker 移动）且非写回 echo → 应用该预设
      │    ├─ 内置/自定义：applyPresetByName（未知名回退 Default）
      │    └─ Last Saved Preset：跟随模式（有手动修改则先覆盖容器）
      ├─ 其他键变化（手动修改）且非写回 echo → 自动保存
      │    ├─ 锚定自定义预设 → 覆盖该预设并保持锚定
      │    └─ 否则 → 写入 Last Saved Preset
      └─ 写回 echo（与最近写回记录匹配）→ 忽略
  → applySnapshot：逐键调 PRESET_APPLIERS 的 apply* 函数，按 RECOMPUTE_KEYS/CACHE_KEYS
    决定 scheduleRecompute / clearResultCache
  → writeBackToTosu：POST /api/counters/settings/<folder>（仅 127.0.0.1 页面）
```

### 快照 schema（35 键）

- `PRESET_APPLIERS`：schema 键 → `settings.js` 的 apply 函数（32 项；`wsEndpoint` 也映射，但系统预设快照故意不含它）。
- `PRESET_STATE_GETTERS`：键 → 从 `state` 读取当前用户值（`captureCurrentSettings()` 用于保存当前配置）。
- `RECOMPUTE_KEYS`（20 键）与 `CACHE_KEYS`（14 键）：与 `settings.js` 的 recomputeNeeded / clearResultCache 键集保持同步（两处代码有注释互相提醒）。
  - 注意：`CACHE_KEYS` 比 `settings.js` 的缓存失效集合多 `debugUseAmount`、`display6kLevel` 两项——main 已证明二者为 display-only（不真正影响输出），多清缓存是保守行为，无正确性问题。
- `wsEndpoint` 语义：连接参数（如局域网地址），应用系统预设不得改变它（避免断 socket）；自定义预设仍会捕获它（应用时写回，可能触发重连——已知取舍）。

### 存储（localStorage）

| 键 | 内容 |
| --- | --- |
| `mma.presets.custom.v1` | 自定义预设数组 `[{id, name, settings, createdAt, updatedAt}]` |
| `mma.presets.active.v1` | 当前激活预设名 |
| `mma.presets.lastWritten.v1` | 写回去重队列（深度 3，每条含 presetName + 快照 + 时间戳；兼容旧单条格式） |

### echo 防护与跨页面协调

- **写回去重**：`shouldWriteBack` 对比最近写回快照，重复不写；`markWritten` 每次写回入队。
- **节流**：`recentlyWritten` 对自动保存类写回做 1.5s 节流——延迟 echo 可能被滞后页面误判为手动修改，从而写回 Last Saved Preset 并让所有页面的 picker 跳变；显式写回（应用预设、用户编辑）不受节流，避免 tosu 侧 values.json 陈旧。
- **origin 限制**：仅 `127.0.0.1` 页面写回；游戏内 overlay 从 `localhost` 加载（不同 origin，无共享 localStorage），一律只读，防止跨页面写回循环。
- **跨页面同步**：`storage` 事件监听 `custom.v1` / `active.v1` / `lastWritten.v1`，任意页面增删改预设后其他页面即时刷新。
- **旧数据迁移**：旧版 "Auto" 容器名自动迁移为 "Last Saved Preset"。

## 注意事项

1. 预设名（`PRESET_DEFS[].name`）必须与 `settings.json` 的 `preset` 选项枚举一致，否则 dashboard 选择后 `applyPresetByName` 找不到而回退 Default。
2. 新增/修改设置项时，若其影响计算结果，需同步检查 `PRESET_APPLIERS` / `RECOMPUTE_KEYS` / `CACHE_KEYS` 是否需要纳入（当前预设快照未覆盖 `enableTelemetry` 等新增设置，属正常设计）。
3. `settings.json` 中 preset 相关文案必须全英文（CLAUDE.md 要求），用户可见文案保持直白。
4. 系统预设的 `pauseDetectionThreshold` 为字符串（如 `"500"`），与 `state.pauseDetectionThresholdMs`（数字）在 GETTERS 中互转。
