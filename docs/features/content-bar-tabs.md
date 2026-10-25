# 卡片 Tab 胶囊栏与组合模式（Combined）

> 目标读者：AI / 开发者｜语言：中文｜基线：`main` @ 79e4e45（插件版本 2.1.0，契约 v7）
> 本文描述桌面壳专属的卡片主体交互；浏览器/OBS（tosu 24050）不启用其中任何一项。

## 1. 功能范围

- **Tab 胶囊栏**：壳窗口内、卡片主体顶部的 4 枚微型胶囊（Pattern / Etterna / Graph / PP）。左键单击临时切换当前谱面的展示模块；不写设置、不进预设、不写 localStorage。
- **组合模式（Combined）**：原 `Full` 的替代。由 `mma-shell-config.json` 的 `combinedMode` 控制，在卡片中同时显示多个模块，并按配置顺序排列。入口在设置页 `Shell Configuration`。
- **tosu 侧**：`contentBar` 枚举只保留 `None / Auto / Pattern / Etterna / Graph / ReworkPP`；旧值 `"Full"` 静默降级为 `"Auto"`。旧的 `Full` 不再能把多模块塞回 tosu 叠加层。

## 2. 可见性规则

Tab 栏仅在以下全部条件成立时可见：

1. `isShellRuntime()` 为真：端口 `24061`，或 URL hash 为 `#mma-shell`（`osuTransport: "tosu"` 时壳加载 24050 页面）；
2. `state.combinedModeEnabled === false`；
3. `getActiveContentBar() !== "None"`。

因此：

- tosu/OBS 页面（24050，无 hash）永不显示；
- `contentBar=None` 不显示（无临时巡检入口）；
- Combined 开启时不显示；
- 浏览器页面即使壳进程正在运行，也不会显示。

## 3. 状态模型

`js/app/appContext.js`：

```js
tabContentBar: null,          // 会话级 Tab 覆盖；null = 跟随 contentBar
tabForceRecompute: false,     // 本次 Tab 点击是否强制跳过结果缓存
combinedModeEnabled: false,   // 仅壳配置可置真
combinedModeModules: [...COMBINED_MODE_MODULES],
```

优先级：

```js
getActiveContentBar() = state.tabContentBar || state.effectiveContentBar || state.contentBar;

contentBarShows(section) =
  state.combinedModeEnabled ? state.combinedModeModules.includes(section)
                            : getActiveContentBar() === section;
```

不变量：

- `combinedModeEnabled` 为真时 `tabContentBar` 必须为 `null`；
- `combinedModeModules` 至少一个模块（空数组归一化默认四模块）；
- `tabForceRecompute` 在 `fetchBeatmapFile` 入口读取后立即清零。

## 4. 点击语义

`js/app/contentBarTabs.js` 只注册一个 `click` 监听（不挂 `wheel` / `touchstart` / `pointerdown` / 键盘 / 拖拽）：

```js
contentBarTabsEl.addEventListener("click", (event) => {
    if (event.button !== 0) return;
    const button = event.target.closest("[data-content-tab]");
    if (button) onSelect(button.dataset.contentTab);
});
```

`js/app/settings.js` 的 `setTabContentBar(module)`：

1. 非壳或 Combined 开启 → 拒绝；
2. 计算 `isActive = getActiveContentBar() === module`；
3. 写 `state.tabContentBar = module`；
4. `state.tabForceRecompute = isActive`；
5. 立即更新可见性/高度过渡；
6. `scheduleRecompute("content bar tab", false)`。

### 4.1 只计算一个展示模块

`needComputed` 只按 `contentBarShows` 打开当前 Tab 的模块。Tab 巡检期间 `isAutoDisplayEnabledNow()` 返回 false，避免默认 Auto 用户顺带计算 Pattern 并触发 auto profile 的二次派发。

**无法回避的依赖**（不是预取）：`Mixed`/`Companella` 估算器需要 Ett/Companella 数据；`useSvDetection`/`vibroDetection` 需要 Pattern；头部 `srText=MSD|InterludeSR|ReworkPP` 与 `diffText=Graph` 需要对应值。这些按既有管线计算。

### 4.2 点击已激活 Tab = 手动重算

`isActive` 为真时 `tabForceRecompute = true`；`fetchBeatmapFile` 本次跳过结果缓存（`!forceRecompute` 才读缓存），真正重跑当前模块。若用户只想要“重新走流程但允许缓存命中”，删除这一条即可，当前需求明确要求重算。

### 4.3 复位

- 换歌/换难度：`socketHandlers` 与 `sources/externalSource.js` 检测 `isSameBeatmapIdentity` 为 false → `resetContentBarTabOverride()`；
- `cvtFlag`（IN/HO，含 Malody Holds/Taps）变化：同上；
- 任何 `applyContentBarSetting()`：清空覆盖；
- Combined 开启：清空覆盖；
- `resetContentBarTabOverride()` 同时清 `tabForceRecompute`。

## 5. 组合模式配置

`mma-shell-config.json`：

```json
"combinedMode": {
  "enabled": false,
  "modules": ["Pattern", "Etterna", "Graph", "ReworkPP"]
}
```

- 数组顺序即卡片显示顺序；token 只认四个规范值（大小写不敏感）；
- 缺失/非数组/空数组/全非法 → 默认四模块；
- UI 禁止取消最后一个已勾选模块；
- 设置页写入整对象 `{enabled, modules}`，避免 `merge_json` 递归合并残留旧数组；
- 旧 `contentBar: "Full"` 只按 §1 降级为 Auto；组合模式使用独立的新键 `combinedMode`。

## 6. 传输链路

```
启动：main.initialize()
  → initShellConfig() → GET http://127.0.0.1:24061/shell-config
        （仅 isShellRuntime；失败/1200ms 超时 → combinedMode disabled）
  → loadSettings()

运行中：壳 POST /shell-config 或 mma-shell-config.json mtime 变化
  → server/mod.rs broadcast("settings", shellConfig)
  → 页面 onSettings: isShellConfigPayload(payload) ?
        applyCombinedModeSetting(payload.combinedMode) : applySettingsPayload(payload)
```

- `GET /shell-config` 对 24050/24061 允许的 loopback Origin 返回 ACAO（与 `/feedback` 同白名单），供 `24050#mma-shell` 跨源读取；
- 不新增 WS 帧类型、不改 `CONTRACT_VERSION`（保持 7）；
- 旧壳无 `combinedMode` → 页面默认 disabled；新壳旧页忽略该键。

## 7. CSS / DOM

- Tab 栏是 `.main-card` 的网格项，`order: 0`；不包裹、不移动已有模块；
- 4 枚胶囊高 18px，`display:flex`、`overflow:visible`，不出现原生滚动条；
- Combined 只写 `style.order`：每个模块的分隔条与容器使用同一 `order`，等值时保持文档顺序（分隔条在前）；
- 未勾选模块 `hidden = true` 并清空 `order`；
- `.main-card.bars-combined .combined-separator[hidden] { display:none }` 兜底；
- `contentBar=None` / Combined 开启 / 非壳页面：Tab 栏 `hidden`。

## 8. 已知限制

- **重复切换成本**：结果缓存每个 key 只存一份快照；A→B→A 可能每次都要重新分析 A。这是“只算一种”的代价，不做四模块预取、不做多快照。
- **估算器依赖**：如使用 Mixed/Companella，Ett 等前置仍会计算；Tab 只限制“展示模块”，不改变估算器语义。
- **`contentBar=None`**：没有 Tab 入口；改回其他 `contentBar` 值后恢复。
- **外部源 osu-like**：Etterna Tab 仍可能显示 `No data`（该源没有 MSD 语义），这是既有能力边界，不是 Tab 引入的回归。
- **关闭结果缓存**：每次 Tab 点击都会真正重算；这是 `enableResultCache=false` 的既有代价。

## 9. 验收清单

- [ ] 非壳页面（24050 无 hash）Tab 隐藏，`/shell-config` 零请求；
- [ ] 24061 壳页 Tab 可见，激活态与 `contentBar`/Auto 一致；
- [ ] `contentBar=None` 时 Tab 隐藏；
- [ ] 左键点击只切换一个模块；滚轮/右键/中键/触摸不切换；
- [ ] 点击已激活 Tab 触发一次强制重算（跳过缓存一次）；
- [ ] 换歌/换难度/IN·HO 后 Tab 复位；
- [ ] Combined 开启时 Tab 隐藏，模块按配置顺序渲染，分隔条成对移动；
- [ ] 空 `modules` 归一化默认四模块；设置页最后一个复选框禁用；
- [ ] 卡片高度自然生长，无内嵌滚动条；
- [ ] 旧 `contentBar: "Full"` 解析为 `"Auto"`。
