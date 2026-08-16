# 预设系统（Preset System）

> 本文档说明预设系统的功能、架构与使用方法。目标读者：开发者与 AI。
> 对应的用户文档见 [docs/settings.md](../settings.md) 的「预设（Presets）」部分。

## 功能说明

预设系统允许用户一键应用或保存整套插件配置。与早期版本（硬编码于 `js/app/presets.js`）不同，当前实现是完全**自拓展**的：设置 schema 来自 `settings.json` 本身，新增设置项无需修改任何预设代码。

- **内置预设**（只读）：存放于 `presets/*.json`（清单 + 每预设一个文件），每个预设只含**覆盖子集**（相对默认值有差异的键）。应用时仅覆盖这些键，未覆盖的设置保留当前值。
- **Default 预设**：不落盘，应用时由 `settings.json` 的 `value` 字段动态生成全量出厂快照。
- **自定义预设**：用户在 presets.html 管理器中创建，支持**部分快照**（编辑时取消勾选的字段不纳入，应用时保留原状态）。
- **Last Saved Preset 自动跟随**（Auto 模式）：未锚定自定义预设时，用户在 tosu 设置页的手动修改自动存入该容器；它是跟随标记，**不是**可应用的快照。
- **锚定行为**：选中自定义预设后手动修改 → 自动覆盖该预设；选中内置预设后手动修改 → 修改进入 Last Saved Preset。
- **固定锚定槽**：Custom 1 / Custom 2 / Custom 3 首次加载自动创建（物化当前配置），不可重命名/删除。
- **导出/导入**：单个预设或全库导出为 json 文件，可导入分享（`mma-preset` / `mma-preset-collection` 格式，带版本号与校验）。

## 使用方法

### tosu 设置页（dashboard）

- `preset` 设置项（options，位于 Links 分组之后）选项固定为：Default + 11 个内置预设 + Custom 1-3 + Last Saved Preset。
- 选择任一预设立即应用并写回 tosu；选择 Last Saved Preset 进入跟随模式。
- **自定义预设（任意命名）不进 tosu 下拉**，统一在 presets.html 管理。

### 预设管理器（浏览器，`presets.html`）

- 打开 `http://<host>:<port>/<插件目录名>/presets.html`（默认 `http://localhost:24050/<插件目录名>/presets.html`）。
- 左侧：预设列表（My Presets：Apply/Edit/Rename/Delete/Export；System：Apply；Last Saved Preset 只读）。
- 右侧：**自动生成**的设置表单（来自 settings.json，header 分组、checkbox/options/text/color/number 控件），每项前有**复选框**控制是否纳入快照。
- 顶栏动作：**Save as Preset**（勾选字段存为新预设）、**Apply Checked**（勾选字段应用并写回 tosu）、**Use Current Settings**（表单恢复默认值，随后由 tosu 广播刷新为当前值）、**Export All**、**Import**。
- 表单值随 tosu 设置广播自动同步（正在聚焦的控件除外）。

## 架构

### 模块划分（`js/app/presets/`）

| 模块 | 职责 |
| --- | --- |
| `index.js` | 入口：副作用导入，`initPresets()` 自初始化（被 `main.js` 引用，恰好一次） |
| `schema.js` | **自拓展核心**：fetch `settings.json`，按命名约定 `apply{Key}Setting` 在 settings.js 导出中动态查找并注册 applier；`getterFor` 读取当前用户值；`buildDefaultSnapshot` 从 `value` 字段生成出厂快照 |
| `core.js` | 应用逻辑：快照应用（部分语义）、设置流处理、echo 防护、写回、自定义预设 CRUD、Auto 跟随 |
| `storage.js` | 持久化：`presetStorage`（tosu 设置项）读写、localStorage 缓存、旧库迁移、写回去重队列 |
| `io.js` | 导出/导入（Blob 下载 / file input 上传，格式校验） |
| `manager.js` | presets.html 页面 UI：自动生成表单、复选框、列表 CRUD、导入导出 |

### 自拓展机制（schema.js）

- **applier 动态注册**：遍历 `settings.json`，对每个 uniqueID 生成 `apply{首字母大写}Setting` 并在 settings.js 的模块命名空间中查找；找到即注册，找不到（header/button/`preset`/`presetStorage`）自动跳过。个别命名例外：`enablePauseDetection → applyPauseDetectionSetting`。
- **getter 动态生成**：默认 `state[key]` 直读；例外表覆盖 `user*` 三件套（contentBar/srText/diffText）、`pauseDetectionEnabled`、`pauseDetectionThresholdMs`（字符串化）、`vibroDetection`。
- **recompute/cache 键集**：`SETTING_RECOMPUTE_KEYS` / `SETTING_CACHE_KEYS` 由 `settings.js` 导出（settings.js 的监听器已数据化为 `SETTING_HANDLERS` 表），预设模块直接 import，单一来源。
- **效果**：新增设置 = 改 settings.json（uniqueID/value）+ settings.js（parse/apply 各一）两处，预设系统（表单、快照、应用、重算、缓存）自动跟随。

### 数据流

```
tosu WebSocket 设置广播
  → core.js 自己的 /websocket/commands 连接（socket.commands(handleSettingsPacket)）
  → 首包：记录基线 lastValues + 从 presetStorage 载入自定义预设库 + 恢复激活预设
  → 后续包快照 diff（排除 wsEndpoint / presetStorage）：
      ├─ preset 值变化（picker 移动）且非写回 echo → 应用该预设
      ├─ 其他键变化（手动修改）且非写回 echo → 自动保存（锚定自定义预设则覆盖它，否则写 Last Saved Preset）
      └─ presetStorage 变化 → 同步库（跨页面）
  → applySnapshot：逐键调 applier，按 SETTING_RECOMPUTE_KEYS / SETTING_CACHE_KEYS
    决定 scheduleRecompute / clearResultCache（部分快照：缺键跳过 = 保留当前值）
  → 写回：POST /api/counters/settings/<folder>（仅 127.0.0.1 页面；localhost overlay 只读）
```

### 持久化（presetStorage）

- 自定义预设库序列化后存入 tosu 设置项 `presetStorage`（text，位于 Debug Options 分组，values.json 中）。
- 随实例设置转移（换设备拷贝 tosu settings 目录即可）、随 getSettings 广播到达**所有页面**（包括游戏内 overlay，解决 127.0.0.1/localhost origin 隔离问题）、插件更新/清浏览器缓存不丢。
- localStorage（`mma.presets.custom.v1`）仅作缓存；首次加载检测到旧库自动迁移（含旧 "Auto" 容器改名 "Last Saved Preset"）。
- `presetStorage` 的写回不参与 preset 写回去重（库变更总是写）。

### echo 防护与跨页面协调

- 写回去重：`shouldWriteBack` 对比最近写回快照，重复不写；`markWritten` 入队（深度 3）。
- 自动保存节流：`recentlyWritten` 1.5s 窗口，防止滞后页面把预设应用 echo 误判为手动修改。
- 跨页面同步：presetStorage 广播（主）+ localStorage `storage` 事件（缓存/激活名）。

## 注意事项

1. 内置预设名（`presets/index.json` 的 `name`）必须与 `settings.json` 的 `preset` options 一致（不一致时 dashboard 选择后 `applyPresetByName` 找不到 → 回退 Default）。
2. 新增设置时：settings.js 的 `SETTING_HANDLERS` 加一行（parse/apply 对）并同步 `SETTING_RECOMPUTE_KEYS`/`SETTING_CACHE_KEYS` 集合；`schema.js` 无需改动。
3. `preset` 与 `presetStorage` 两个 uniqueID 是系统保留键：前者是预设选择器（不参与快照应用），后者是预设库存储（不参与手动修改判定）。
4. 系统预设的 `pauseDetectionThreshold` 为字符串（如 `"500"`），与 `state.pauseDetectionThresholdMs`（数字）在 getter 中互转。
5. 部分快照语义：应用任意预设只覆盖快照中存在的键；"Default" 是全量出厂快照（唯一全量预设）。
