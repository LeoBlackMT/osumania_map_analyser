// sourceManager：gameClient 路由（Auto 决策表 L1–L4+L3'）+ 源圆点 + osu 门控咨询。
//
// L1 游玩态抢占：osu=isInPlayState（raw 豁免照读）、etterna=playing 标志、
//   malody=无游玩态信号；
// L2 新鲜事件窗口（60s）：osu=换谱/换 mod/改 rate（identity/modSignature 变化）、
//   etterna=桥 select/gameplay 写入（song 帧到达）、malody=POST/song 帧；
// L3 hold 与抢占：续约只作用于当前持有源；他源新鲜事件在无游玩态时可抢占；
//   当前源窗口过期 → 按固定优先级 osu>Etterna>Malody 选窗口内第一源；
// L3' 存活回窗：无窗口内源时，tosu 在线（壳 state 帧）→ osu 回窗（菜单态有效）；
// L4 全离线：无窗口内源且无存活 → 无源（灰空心圆点）。
//
// 败方门控：activeSource≠osu 且壳模式 → socketHandlers 经本模块咨询后
// 挂起 osu 的 identity/mod 写与 recompute（信号部分照常应用），并缓冲最后
// 一条 tosu 包，切回 osu 时先回放再 recompute（缓存键对齐）。

import { state } from "../appContext.js";

const FRESH_WINDOW_MS = 60000;
const DEBOUNCE_MS = 200;
const PRIORITY = ["osu", "etterna", "malody"];
const LABELS = { osu: "osu!", etterna: "Etterna", malody: "Malody" };
// 与遥测 Client 饼图段色一致（dashboard PALETTE 前 3 色）。
const DOT_COLORS = { osu: "#635bff", etterna: "#0d9c5f", malody: "#f5a623" };

let debounceTimer = 0;
let activeSource = null; // 最近一次已应用的路由结果（null=无源）
let onApplied = null; // 应用回调（socketHandlers 门控包装注册）

export function setActiveSourceListener(cb) {
    onApplied = cb || null;
}

// ── 事件输入 ──

/** osu / etterna / malody 新鲜事件（L2）。 */
export function notifySourceEvent(source) {
    const now = Date.now();
    state.sourceEvents = state.sourceEvents || {};
    state.sourceEvents[source] = now;
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
    if (lower.startsWith("mal")) return "malody";
    return null;
}

function decide() {
    // L1
    if (state.isInPlayState) return "osu";
    if (state.etternaPlaying) return "etterna";
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
    // 未应用过：以外部源/单源猜测
    if (state.externalSourceActive) return state.externalSourceActive;
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

/** osu 的 beatmap 状态应用是否应挂起（败方门控）。 */
export function isOsuSuppressed() {
    return (state.externalBridgeAvailable || state.externalSourceActive)
        && currentRoute() !== null
        && currentRoute() !== "osu";
}

// ── 圆点 ──

function dotElement() {
    if (typeof document === "undefined") return null;
    return document.getElementById("mma-source-dot");
}

function syncDot(source) {
    const dot = dotElement();
    if (!dot) return;
    // 空心语义：仅「无任何源」或「osu 源」（osu 数据面始终在 tosu，非
    // shell 外部源；Etterna/Malody 才用实心色标识）。
    if (!source || source === "osu") {
        dot.className = "mma-source-dot off";
        dot.title = source === "osu" ? "数据源：osu!" : "无数据源";
        return;
    }
    dot.className = "mma-source-dot on";
    dot.style.background = DOT_COLORS[source] || "#888";
    const followState = source === "malody"
        ? "编辑器/web post 精确；游玩受限"
        : "精确（桥文件跟随）";
    dot.title = `数据源：${LABELS[source]}（${followState}）`;
}

/** 初始化（main 挂载）。 */
export function initSourceManager(handlers) {
    if (handlers && handlers.onApplied) {
        setActiveSourceListener(handlers.onApplied);
    }
    scheduleApply();
}