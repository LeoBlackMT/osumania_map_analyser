// sourceManager：gameClient 路由（Auto 决策表 L1–L4+L3'）+ 源圆点 + osu 门控咨询。
//
// L1 游玩态抢占：osu=isInPlayState（raw 豁免照读）、etterna=playing 标志、
//   malody4=壳 state 帧的 playing 位（screen==3 && fresh）、
//   malody=壳 state 帧的 playing 位（桥 screen==playing 且桥存活；alive 门控防止游戏关闭后路由钉死）；
// L2 新鲜事件窗口（60s）：osu=换谱/换 mod/改 rate（identity/modSignature 变化）、
//   etterna=桥 select/gameplay 写入（song 帧到达）、malody4=song 帧/选曲记录
//   （hidden 与心跳不续约）、malody=POST/song 帧；
//   malody 桥通道的"新鲜事件"由**壳侧**判定 = 内容四元组 `(path, rate_text, screen, judge)` 变化：
//   心跳与重复观察既不产生事件、也不续期窗口，页面侧**不得**新增任何续期逻辑
//   （窗口过期即让位给 osu/Etterna，这是矩阵 #18 的判据）；
// L3 hold 与抢占：续约只作用于当前持有源；他源新鲜事件在无游玩态时可抢占；
//   当前源窗口过期 → 按固定优先级 osu>Etterna>Malody 4>Malody 选窗口内第一源；
// L3' 存活回窗：无窗口内源时，tosu 在线（壳 state 帧）→ osu 回窗（菜单态有效）；
// L4 全离线：无窗口内源且无存活 → 无源（灰空心圆点）。
//
// 卡片归属（`state.cardOwner`）：置位点 = externalSource.handleSongFrame（带 screen 的桥帧）；
//   复位点 = 本文件的 notifySourceEvent（任何非 "malody-bridge" 的源事件）。
//   shellState 的桥清空规则按它门控（归属不是桥 ⇒ 不清空别的源的卡片）。
//
// 败方门控：activeSource≠osu 且壳模式 → socketHandlers 经本模块咨询后
// 挂起 osu 的 identity/mod 写与 recompute（信号部分照常应用），并缓冲最后
// 一条 tosu 包，切回 osu 时先回放再 recompute（缓存键对齐）。

import { state } from "../appContext.js";

const FRESH_WINDOW_MS = 60000;
const DEBOUNCE_MS = 200;
const PRIORITY = ["osu", "etterna", "malody4", "malody"];
const LABELS = { osu: "osu!", etterna: "Etterna", malody4: "Malody 4", malody: "Malody" };
// 各源品牌色（osu! 粉 / Etterna 紫 / Malody 4 亮青 / Malody 蓝），与遥测 dashboard 的 PALETTE 无关。
const DOT_COLORS = { osu: "#ff66aa", etterna: "#a855f7", malody4: "#22d3ee", malody: "#3b82f6" };

let debounceTimer = 0;
let activeSource = null; // 最近一次已应用的路由结果（null=无源）
let onApplied = null; // 应用回调（socketHandlers 门控包装注册）

export function setActiveSourceListener(cb) {
    onApplied = cb || null;
}

// ── 事件输入 ──

/**
 * osu / etterna / malody4 / malody 新鲜事件（L2）。
 * 同时是卡片归属的**复位点**：任何非桥源的事件都表示卡片不再属于桥通道
 * （osu 经 socketHandlers、Etterna 与 Malody Lua 通道都从这里过），
 * shellState 的清空门控据此放行/拦截。
 *
 * 同一处也复位 `analysisRate`：它是「倍率语义只在桥通道内有效」的载体
 * （Mod 派生速率 ⇒ 1.0，难度由 OD 表达）。它只在 `externalSource.handleSongFrame`
 * 的桥帧分支里被赋值，而清空路径（`clearSourceCard()` → `resetSourceContext()`）
 * **只在桥的 `other` 边沿触发**——若从桥通道切到 osu 时没有该边沿，残留的 1.0
 * 会让 osu 的 DT/HT 被当成 1.0 参与估星（静默错误）。把复位放在这里，可同时覆盖
 * osu / Etterna / Lua 三条路径，且 `notifySourceEvent()` 是同步调用，调用方随后的
 * 重算管线读到的已经是复位后的值。
 */
export function notifySourceEvent(source) {
    const now = Date.now();
    state.sourceEvents = state.sourceEvents || {};
    state.sourceEvents[source] = now;
    if (source !== "malody-bridge") {
        state.cardOwner = source;
        // 桥帧自己的 source 是 "malody"，但它随后会把 analysisRate 重新写对
        // （handleSongFrame 里赋值在 notifySourceEvent 之后），故此处不会误清。
        state.analysisRate = null;
    }
    scheduleApply();
}

/** 壳 state 帧到达（tosuOnline / alive 变化）→ 触发重评估。 */
export function reEvaluate() {
    scheduleApply();
}

/** 立即取当前路由（不等待 debounce；供门控包装即时咨询）。 */
export function currentRoute() {
    if (hasForceClient()) {
        return normalizeClient(state.gameClient);
    }
    return decide();
}

function hasForceClient() {
    const c = String(state.gameClient || "Auto");
    return c !== "Auto" && c !== "";
}

function normalizeClient(value) {
    const lower = String(value || "").toLowerCase();
    if (lower === "osu!" || lower === "osu") return "osu";
    if (lower.startsWith("ett")) return "etterna";
    // 精确匹配必须在 startsWith("mal") 之前：否则前缀分支会把 malody4 折叠成 malody，
    // 强制源通道静默失效（Malody 4 与 Malody V 是两个独立源）。
    if (lower === "malody4" || lower === "malody 4" || lower === "malody-4") return "malody4";
    if (lower.startsWith("mal")) return "malody";
    return null;
}

function decide() {
    // L1
    if (state.isInPlayState) return "osu";
    // Malody 选曲桥的游玩态：`alive` 门控防止游戏关闭后路由被钉死在 Malody
    // （壳在桥静默时会把 playing 置假，这里的 alive 是第二道保险）。
    if (state.malodyPlaying && state.malodyAlive) return "malody";
    if (state.etternaPlaying) return "etterna";
    if (state.malody4Playing) return "malody4";
    // L2/L3：窗口 + hold/抢占
    const now = Date.now();
    const events = state.sourceEvents || {};
    const inWindow = (s) => events[s] && now - events[s] <= FRESH_WINDOW_MS;
    const previous = state.sourceEvents ? lastRoute() : null;
    if (previous && inWindow(previous)) {
        // L3 hold：当前源窗口续约；但他源新鲜事件在无游玩态时可抢占（优先级序）。
        for (const s of PRIORITY) {
            if (s !== previous && inWindow(s)) {
                return s;
            }
        }
        return previous;
    }
    for (const s of PRIORITY) {
        if (inWindow(s)) return s; // 窗口过期后按优先级重选
    }
    // L3' 存活回窗：tosu 在线（壳模式）→ osu（菜单态持续推送视为存活）
    if (state.shellTosuOnline && state.externalBridgeAvailable) {
        return "osu";
    }
    // L4
    return null;
}

/** 最近一次已应用的源（供 hold 续约）。 */
function lastRoute() {
    if (state.lastSourceRoute && inPriority(state.lastSourceRoute)) {
        return state.lastSourceRoute;
    }
    return "osu";
}

function inPriority(s) {
    return PRIORITY.includes(s);
}

// ── 应用（debounce + 旧结果保留）──

function scheduleApply() {
    clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
        const next = currentRoute();
        if (next === activeSource) {
            syncDot(next);
            return;
        }
        const prev = activeSource;
        activeSource = next;
        state.lastSourceRoute = next;
        state.activeSource = next;
        syncDot(next);
        if (onApplied && prev !== next) {
            onApplied(next, prev);
        }
    }, DEBOUNCE_MS);
}

/** 外部源 song 帧是否可路由（强制指定时只放行该源；Auto 接受并入窗）。 */
export function routeAllowsExternal(source) {
    if (hasForceClient()) {
        return normalizeClient(state.gameClient) === source;
    }
    return true;
}

// ── osu 门控咨询 ──

/** osu 的 beatmap 状态应用是否应挂起（败方门控）。
 * 仅当「壳桥在线且 tosu 离线」时 osu 才可能被外部源压制——浏览器模式
 * （无壳）或壳在线模式（tosu 存活）下 osu 恒为主数据面，绝不挂起。
 * ⚠️ 端口守卫：必须限定在壳离线页（24061）。用户打开正常 tosu 浏览器页
 * （24050）时壳也可能在跑（externalBridgeAvailable=true、tosu 未运行），
 * 此时 osu 是页面唯一数据面，绝不能挂起——否则换图/mod 全部被缓冲吞掉。 */
export function isOsuSuppressed() {
    if (!state.externalBridgeAvailable || state.shellTosuOnline) {
        return false;
    }
    if (typeof window === "undefined" || !window.location) {
        return false;
    }
    if (String(window.location.port) !== "24061") {
        return false; // 浏览器 tosu 页：osu 恒为主数据面
    }
    return currentRoute() !== null && currentRoute() !== "osu";
}

// ── 圆点 ──

function dotElement() {
    if (typeof document === "undefined") return null;
    return document.getElementById("mma-source-dot");
}

function syncDot(source) {
    const dot = dotElement();
    if (!dot) return;
    // 空心 = 无来源；osu!/Etterna/Malody 4/Malody 各一实心色（粉/紫/青/蓝）。
    if (!source) {
        dot.className = "mma-source-dot off";
        dot.style.background = ""; // 清除内联色，让 .off 的空心样式生效
        dot.title = "无数据源";
        return;
    }
    dot.className = "mma-source-dot on";
    dot.style.background = DOT_COLORS[source] || "#888";
    const followState = source === "malody4"
        ? "精确（谱面级跟随）"
        : source === "malody"
            ? "选曲/游玩/结算精确（游戏内桥）；编辑器通道为回退"
            : source === "etterna"
                ? "精确（桥文件跟随）"
                : "精确（tosu）";
    dot.title = `数据源：${LABELS[source]}（${followState}）`;
}

/** 初始化（main 挂载）。 */
export function initSourceManager(handlers) {
    if (handlers && handlers.onApplied) {
        setActiveSourceListener(handlers.onApplied);
    }
    scheduleApply();
}