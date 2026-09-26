# 预设系统（Preset System）

> 本文档说明预设系统的功能、架构与使用方法。目标读者：开发者与 AI。
> 对应的用户文档见 [docs/settings.md](../settings.md) 的「预设（Presets）」部分。

## 功能说明

预设系统允许用户一键应用或保存整套插件配置。与早期版本（硬编码于 `js/app/presets.js`）不同，当前实现是完全**自拓展**的：设置 schema 来自 `settings.json` 本身，新增设置项无需修改任何预设代码。

- **内置预设**（只读，SYSTEM 分类）：存放于**插件目录内**的 `presets/*.json`（清单 + 每预设一个文件），每个预设只含**覆盖子集**（相对默认值有差异的键）。应用时仅覆盖这些键，未覆盖的设置保留当前值。
- **Default 预设**：不落盘（SYSTEM 分类首行），应用时由 `settings.json` 的 `value` 字段动态生成全量出厂快照（与 config.js defaults 对齐）。
- **预设结构**：`{id, name, description, version, settings}`——`id` 由 name 自动生成（slug，可省略）；`name` 限英文字母/数字/`_`/`-`（≤40 字符，系统槽 "Custom N" 豁免）；`version` 为纯数字（越大越新）。
- **自定义预设**：用户在 presets.html 管理器中创建，支持**部分快照**（编辑时取消勾选的字段不纳入，应用时保留原状态）。
- **LastSavedPreset 自动跟随**（Auto 模式）：未锚定自定义预设时，用户在 tosu 设置页的手动修改自动存入该容器；它是跟随标记，**不是**可应用的快照。
- **锚定行为**：选中自定义预设后手动修改 → 自动覆盖该预设；选中内置预设后手动修改 → 修改进入 LastSavedPreset。
- **固定锚定槽**：Custom1 / Custom2 / Custom3 首次加载自动创建（物化当前配置），不可重命名/删除。
- **导出/导入**：单个预设、当前编辑态或全库导出为 json 文件（`mma-preset` / `mma-preset-collection` 格式 v2，含 metadata；导入兼容 v1 旧格式）。

## 使用方法

### tosu 设置页（dashboard）

- `preset` 设置项（options，位于 Links 分组之后）选项固定为：Default + 10 个内置预设 + Custom1-3 + LastSavedPreset。
- 选择任一预设立即应用并写回 tosu；选择 LastSavedPreset 进入跟随模式。
- **自定义预设（任意命名）不进 tosu 下拉**，统一在 presets.html 管理。

### 预设管理器（浏览器，`presets.html`）

- 打开 `http://<host>:<port>/<插件目录名>/presets.html`（默认 `http://localhost:24050/<插件目录名>/presets.html`）。
- **顶部操作栏**（独立、sticky）：New（清空编辑器，需确认）、Save（保存/覆盖，需确认）、Apply Checked（应用勾选字段，需确认）、Export Current（导出当前编辑态）、Export All、Import、Guide（新窗口打开 [预设新手教程](../presets-guide.md)）。
- **左侧列表**（分类）：SYSTEM 分类 = Default（首行，动态出厂快照）+ 10 个内置预设 + LastSavedPreset（只读行）；My Presets = 自定义预设（Custom1-3 固定槽仅 Edit/Apply/Export）。每行按钮：Edit（加载到编辑区并高亮）、Apply（一键应用，需确认）、自定义行另有 Rename/Delete/Export。
- **右侧编辑区**：Preset Info 面板（Name/Description/Version 可编辑，ID 自动生成只读）+ 自动生成的设置表单（来自 settings.json，header 分组；`preset`/`presetStorage` 系统设置排除不显示，`wsEndpoint` 默认不勾选）。每行左侧复选框（勾选 = 纳入预设）与右侧布尔设置控件尺寸一致。
- **滚动**：页面所有可滚动区域（左侧列表、右侧表单）使用自定义细滚动条（配色随主题，圆角）。
- **反馈**：所有操作结果通过右上角 toast 通知（自动消失）；破坏性操作（应用/删除/覆盖/导入/清空）均弹窗二次确认。

## 架构

### 模块划分（`js/app/presets/`）

| 模块 | 职责 |
| --- | --- |
| `index.js` | 入口：副作用导入，`initPresets()` 自初始化（被 `main.js` 引用，恰好一次） |
| `schema.js` | **自拓展核心**：fetch `settings.json`，按命名约定 `apply{Key}Setting` 在 settings.js 导出中动态查找并注册 applier；`getterFor` 读取当前用户值；`buildDefaultSnapshot` 从 `value` 字段生成出厂快照 |
| `core.js` | 应用逻辑：设置流处理、echo 防护、写回、自定义预设 CRUD、Auto 跟随、init；re-export 拆分模块的 API |
| `snapshot.js` | 快照纯函数：`snapshotOf`/`stripSystemKeys`/`hasKeyChanged`/`captureCurrentSettings`/`applySnapshot`（无模块状态依赖） |
| `builtin.js` | 内置预设加载（`presets/*.json`）：清单 + 逐文件解析，自包含缓存 |
| `form.js` | presets.html 表单渲染（自拓展设置表单、复选框、值控件），经 `createForm(api)` 注入状态 |
| `storage.js` | 持久化：`presetStorage`（tosu 设置项，单一权威存储，v2 结构 `{v, lastWritten, presets}`）读写、写回去重队列、系统键剥离 |
| `io.js` | 导出/导入（Blob 下载 / file input 上传，格式校验） |
| `manager.js` | presets.html 页面 UI：布局、动作栏、列表 CRUD、导入导出接线 |

### 自拓展机制（schema.js）

- **applier 动态注册**：遍历 `settings.json`，对每个 uniqueID 生成 `apply{首字母大写}Setting` 并在 settings.js 的模块命名空间中查找；找到即注册，找不到（header/button/`preset`/`presetStorage`）自动跳过。个别命名例外：`enablePauseDetection → applyPauseDetectionSetting`。
- **getter 动态生成**：默认 `state[key]` 直读；例外表覆盖 `user*` 三件套（contentBar/srText/diffText）、`pauseDetectionEnabled`、`vibroDetection`。
- **recompute/cache 键集**：`SETTING_RECOMPUTE_KEYS` / `SETTING_CACHE_KEYS` 由 `settings.js` 导出（settings.js 的监听器已数据化为 `SETTING_HANDLERS` 表），预设模块直接 import，单一来源。
- **效果**：新增设置 = 改 settings.json（uniqueID/value）+ settings.js（parse/apply 各一）两处，预设系统（表单、快照、应用、重算、缓存）自动跟随。

### 数据流

```
tosu WebSocket 设置广播
  → core.js 自己的 /websocket/commands 连接（socket.commands(handleSettingsPacket)）
  → 首包：记录基线 lastValues + 从 presetStorage 载入自定义预设库 + 恢复激活预设
  → 后续包快照 diff（排除 wsEndpoint / presetStorage）：
      ├─ preset 值变化（picker 移动）且非写回 echo → 应用该预设
      ├─ 其他键变化（手动修改）且非写回 echo → 自动保存（锚定自定义预设则覆盖它，否则写 LastSavedPreset）
      └─ presetStorage 变化 → 同步库（跨页面）
  → applySnapshot：逐键调 applier，按 SETTING_RECOMPUTE_KEYS / SETTING_CACHE_KEYS
    决定 scheduleRecompute / clearResultCache（部分快照：缺键跳过 = 保留当前值）
  → 写回：POST /api/counters/settings/<folder>（浏览器页面 localhost/127.0.0.1；CEF overlay 只读）
```

### 持久化（presetStorage）

- 自定义预设库与写回去重队列（lastWritten）序列化后存入 tosu 设置项 `presetStorage`（text，位于 Debug Options 分组，values.json 中）。**不使用浏览器存储**：localhost 与 127.0.0.1 是不同 origin，localStorage 会按 origin 隔离导致数据不一致——单一 tosu 存储经广播/HTTP GET 到达所有页面（含游戏内 overlay），天然跨 origin 一致。
- 页面加载时优先从设置广播读取 store；广播未到时通过 `GET /api/counters/settings/<folder>` 直接拉取（origin 无关）；两者都失败（tosu 离线）则以空库启动。旧 v1 格式（裸数组）自动兼容读取（含旧 "Auto" 容器改名 "LastSavedPreset"）。
- `presetStorage` 的写回不参与 preset 写回去重（库变更总是写）。

### echo 防护与跨页面协调

- 写回去重：`shouldWriteBack` 对比最近写回快照，重复不写；`markWritten` 入队（深度 3）。
- 自动保存节流：`recentlyWritten` 1.5s 窗口，防止滞后页面把预设应用 echo 误判为手动修改。
- 跨页面/跨 origin 同步：全部通过 tosu 设置广播（presetStorage + lastWritten 随每次写回一并下发），任意页面改动所有页面即时一致。

## 离线（壳）模式（桌面壳设置窗口）

非 tosu 用户在桌面壳的**设置窗口**里管理预设（页面 `settings.html`，由壳的本机 24061 提供）。这条路径不依赖 tosu，也不依赖 `window.COUNTER_PATH`（该全局在 24061 页恒为 `undefined`——它由 tosu 注入），靠的是 **transport 注入点 + 壳的 `/settings` 端点**。

### transport 注入点（`core.js` + 两个实现）

- `core.js` 导出 `setPresetTransport(t)` / `getPresetTransport()`；模块级 `transport` 默认 = `createTosuTransport()`（`presets/tosuTransport.js`，把旧逻辑逐字搬成一个实现：tosu `/api/counters/settings/<folder>` 读写 + `COUNTER_PATH` 依赖）。
- 设置页在 `initPresets()` **之前**调用 `setPresetTransport(createShellPresetTransport())`（`js/app/settingsPage/shellTransport.js`）。它是**薄适配器**：组合框架无关的 `presets/shellTransport.js`（B1/B2 的唯一实现）与 bridge `settings` 帧的 pull-on-notify，不重复实现写语义。
- 接口（`core.js` 的三个 I/O 函数改为委托，`initPresets()` 的事件源按 `transport.mode` 双路——`"tosu"` 仍走 `socket.commands`，`"shell"` 走 `transport.subscribe`）：`{mode, isAvailable(), readStore(), writeLibrary(serialized), writeBack(values), subscribe(handler), requestInitial()}`。
- **B1（全量合并基点）**：壳侧所有写 = `{...base, ...patchObject}`，`base` = **最近一次成功 GET** 的响应体；从未成功 GET（`base === null`）时**拒写**——不发请求、`writeLibrary()` 返回 `false`、不推进持久化指纹，并回调失败。壳侧据此做请求键优先的读-改-写，所以"全量 body + 可能过期的 base"也不会丢键。
- **B2（成功以 2xx 为准 + 失败可见 + 有界重试）**：`writeLibrary` 的同步布尔沿用旧语义（已发出即 `true`），但收到非 2xx / 网络错误时置 `writeOk = false`——它**只影响下一次的返回值**（`core.js` 因此不推进指纹），**不影响发送**：每次调用都照常发请求，一次失败不会把后续写全部短路；2s 后**有界重试一次**（把期间新攒的 patch 一起带上），任何 2xx（含该次重试）复位 `writeOk = true`，重试再失败不排下一次；失败经 `onWriteError` 交给页面状态条显示。

### 两库隔离

- tosu 模式与壳模式的预设库是**两份独立的 `presetStorage`**：tosu 侧在 tosu 设置文件（`settings/<插件目录名>.json`）里，壳侧在 exe 旁的 `mma-settings.json` 里。两者互不同步、互不覆盖——切换环境（例如装上 tosu）后看到的是那一侧自己的库，不会"把壳里的预设搬到 tosu"。
- 因此壳页没有"tosu 在线时改预设"这条路径：在线时预设区整体不可用（只显示只读提示 + tosu Presets 页面地址，`manager.js` 那个模块根本不会被 import）。

### 无 LastSavedPreset 行

`manager.js` 只在**非设置页**（`document.documentElement.dataset.mmaPage !== "settings"`，即 `presets.html`）渲染 LastSavedPreset 行（`OFFLINE_SCOPE` 守卫；`presets/index.json` 内的内置预设与 Default 行不受影响）。LastSavedPreset 是 **tosu 设置页手动修改**的跟随标记，壳页没有 tosu 设置广播这条来源，渲染它只会得到一个永远不动的空行。

### pull-on-notify 投递

壳是"有变化才推"的（来源切换 / 本地文件变化 / 离线 POST），推的是帧通知；设置页收到 bridge `settings` 帧后**重新 GET `/settings`**（pull-on-notify），与上次投递不同才向 transport 的全部订阅者投递一个 tosu 形状的包 `{command:"getSettings", message: values}`——`core.js` 因此复用它的"首包基线"路径（内含 `presetStorage` 与 `lastWritten`，跨页面/跨刷新一致）。在线（`state.shellTosuOnline === true`）时只刷新 `base`、**不投递**（页面处于只读，库不需要更新）。

### 设置窗口的预设编辑器保留勾选列

设置窗口的预设区**整体复用** `manager.js`/`form.js`，因此编辑器的"勾选要包含的项"列（include checkbox）与 `presets.html` **一致地保留**。这是**预设编辑器**（部分快照）的语义，不是设置面板的语义——设置面板（`settingsPage/settingsForm.js`）才是无勾选列、改一项即提交。两者不要混为一谈。

## 注意事项

1. 内置预设名（`presets/index.json` 的 `name`）必须与 `settings.json` 的 `preset` options 一致（不一致时 dashboard 选择后 `applyPresetByName` 找不到 → 回退 Default）。
2. 新增设置时：settings.js 的 `SETTING_HANDLERS` 加一行（parse/apply 对）并同步 `SETTING_RECOMPUTE_KEYS`/`SETTING_CACHE_KEYS` 集合；`schema.js` 无需改动。
3. `preset` 与 `presetStorage` 两个 uniqueID 是系统保留键：前者是预设选择器（不参与快照应用），后者是预设库存储（不参与手动修改判定）。
4. 部分快照语义：应用任意预设只覆盖快照中存在的键；"Default" 是全量出厂快照（唯一全量预设）。
