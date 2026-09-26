// 壳 state 帧 → 页面状态（M3/M5 最小集）。
//
// tosuOnline：壳模式在线数据面信号（M5 起驱动 osu 源启停与败方门控）。
// sources 存活位：供 Auto 决策（M5 sourceManager）与后续 UI（圆点源状态）。
//
// malody4 的字段各有唯一写入者（两个语义不得打架）：
//   - state 帧（applyShellState）→ malody4Playing / malody4Screen / malody4Reason / malody4Judge；
//   - selection 帧（applyMalody4Selection）→ malody4LastSeq / malody4LastSeenAt / malody4Alive。
// `state.malody4Alive` 由 selection 帧的新鲜度派生，**绝不被 30s 周期的 state 帧覆盖**。
//
// Malody V 选曲桥（契约 v4）同样由 state 帧驱动：`sources.malody` 的六个字段进页面状态，
// 并在**桥事件边沿 + 卡片归属为桥**时清空卡片（规则见下方 maybeClearBridgeCard）。

import { state } from "../appContext.js";
import { setStatus } from "../hud.js";
import { invokeCardClear } from "../analysis.js";
import { currentRoute, notifySourceEvent, reEvaluate } from "./sourceManager.js";

/** 心跳 2s + 容忍丢一拍 → 6s 未见 selection 帧即视为本源离场。 */
const MALODY4_ALIVE_WINDOW_MS = 6000;

/** 壳 `reason` 闭集里"这个身份键查过、重建尝试也用完仍解析不出来"的字面量。 */
const UNKNOWN_IDENTITY_REASON = "chart-unknown-identity";
/** 闭集里"刚查过、重建尝试还在进行中"的**临时**字面量（首次未命中即上报）。 */
const UNRESOLVED_CHART_REASON = "chart-unresolved";
/** 状态行里属于"谱面未解析"这类提示的文本集合（清理时只收自己写的这几条）。 */
const SPECTATOR_NOTICES = new Set();

/**
 * 状态行提示：卡片**为什么**停在上一张谱面上。
 *
 * 分两层，因为壳的判定本身分两层：首次未命中（约 0.2s）就报临时原因，重建重试用尽（约 10s）
 * 才报确定结论。只报后者会让用户先盯着毫无反应的卡片等十秒 —— 那正是实测反馈的问题。
 *
 * 文本不写 md5、不用术语：用户看到的现象就是"卡片没跟着换谱"，这里只说清两件事——这张谱
 * 为什么没换、卡片仍是上一张。卡片本身不隐藏、不门控，提示只是把沉默补上。
 * 两条文案都导出，供本地冒烟脚本引用（测试不重抄字面量，改文案时不会假失败）。
 */
export const UNRESOLVED_CHART_NOTICE =
    "Malody 4: looking up this chart in the local beatmap library — the card still shows the previous chart.";
export const UNKNOWN_IDENTITY_NOTICE =
    "Malody 4: this chart is not in the local beatmap library — the card still shows the previous chart.";
SPECTATOR_NOTICES.add(UNRESOLVED_CHART_NOTICE);
SPECTATOR_NOTICES.add(UNKNOWN_IDENTITY_NOTICE);

/**
 * 应用壳 state 帧（shell → 页）。
 * @param {object} payload state 帧 payload
 */
export function applyShellState(payload) {
    state.shellTosuOnline = Boolean(payload.tosuOnline);
    state.shellErrors = Array.isArray(payload.errors) ? payload.errors : [];
    const sources = payload.sources || {};
    state.etternaAlive = Boolean(sources.etterna && sources.etterna.alive);
    state.etternaPlaying = Boolean(sources.etterna && sources.etterna.playing);
    state.etternaPlayingExpireAt = sources.etterna ? sources.etterna.playingExpireAt : null;
    state.malodyAlive = Boolean(sources.malody && sources.malody.alive);
    applyMalodyBridgeFields(sources.malody || {});
    const malody4 = sources.malody4 || {};
    state.malody4Playing = Boolean(malody4.playing);
    state.malody4Screen = malody4.screen || null;
    state.malody4Reason = malody4.reason || null;
    state.malody4Judge = malody4.judge || null;
    // 注意：这里绝不写 state.malody4Alive —— 它是 selection 帧新鲜度的派生值，
    // 30s 周期帧写它会把心跳之间的在线状态冲成假离线。
    syncUnknownIdentityNotice();
    reEvaluate();
}

/** 页面侧已处理的桥事件序号（边沿基线）。 */
let lastMalodyEventSeq = null;

/**
 * 解析桥场景字段（契约 v4 的 `sources.malody` 六字段）并处理清空边沿。
 *
 * **v3 壳降级**：旧壳只发 `alive`，五个新字段一律 `undefined` ⇒ 整段跳过，
 * 页面绝不进入桥的场景/清空/判定路径（CONTRACT.md §11.8）。
 * `alive` 的存活语义由壳负责（stale ⇒ `playing=false` / `screen="none"`），页面不重推。
 *
 * @param {object} malody state 帧里的 `sources.malody`
 */
function applyMalodyBridgeFields(malody) {
    const hasBridgeFields = malody.transport !== undefined
        || malody.screen !== undefined
        || malody.playing !== undefined
        || malody.eventSeq !== undefined
        || malody.judge !== undefined;
    if (!hasBridgeFields) {
        return; // 旧壳（v3）：桥字段缺省，整段不进入
    }
    state.malodyTransport = malody.transport ?? null;
    state.malodyScreen = malody.screen ?? null;
    state.malodyPlaying = Boolean(malody.playing);
    state.malodyEventSeq = Number(malody.eventSeq) || 0;
    state.malodyJudge = malody.judge ?? null;
    // 桥静默（transport 不再是 bridge）⇒ 桥注入、但尚未被分析取走的谱面文本作废。
    // "桥静默而无 other 事件"这条路径不会走下面的清空边沿；不作废的话，下一次 osu 触发的
    // 缓存未命中会拿上一张 Malody 谱面的文本去分析，并把快照写到 osu 的缓存键下。
    // 只作废**桥通道自己的**待分析文本（归属仍是桥）：Lua 通道有自己的 60s 存活窗口，不受影响。
    if (malody.transport !== "bridge" && state.cardOwner === "malody-bridge") {
        state.pendingSourceText = null;
        state.pendingSourceRequestId = null;
        state.pendingSourceActive = null;
    }
    maybeClearBridgeCard();
}

/**
 * 清空规则（计划 Step 4 第 3 项）：**边沿触发 + 归属门控**，载体 = state 帧（唯一）。
 *
 * 为什么不能"`screen === "other"` 就清"：`other` 是**稳态**（主菜单/编辑器/回放浏览，
 * 上游 `SceneLifecycle.cs:27-31` 把编辑器也归入 `Other`）。桥在 `other` 态每 2s 心跳、
 * 壳每 30s 必发 state 帧 ⇒ 无条件清空会每 30s 擦一次 osu/Etterna 的卡片，也会擦掉
 * Lua 编辑器通道（MMA Analyze）产出的卡片 —— 那是本计划保留的回退路径。
 *
 * - 边沿：只认壳侧 `eventSeq`（只在真实事件 +1）的变化，不比较 `screen` 字符串
 *   （字符串比较无法正确处理 `other → selection → other` 的连续两次边沿，
 *   也会在 WS 重连/壳重启后"首帧无历史可比"时误判）；
 * - 归属：只有"当前卡片来自桥"（`state.cardOwner === "malody-bridge"`）才清 ——
 *   置位点 = `externalSource.handleSongFrame`，复位点 = `sourceManager.notifySourceEvent`。
 */
function maybeClearBridgeCard() {
    const seq = state.malodyEventSeq;
    const changed = seq !== lastMalodyEventSeq;
    lastMalodyEventSeq = seq;
    if (!changed || state.malodyScreen !== "other") {
        return; // 非边沿（心跳/30s 定时帧）或非 other（selection/playing/result/none）⇒ 不清
    }
    if (state.cardOwner !== "malody-bridge") {
        return; // 归属门控：卡片是 osu/Etterna/Lua 通道出的，绝不代它清空
    }
    invokeCardClear();
}

/**
 * 两层"谱面未解析"提示 → 状态行（去重与清理都只看 `state.statusText`，不另设标志位）。
 *
 * - 只在**当前路由就是 malody4** 时提示：这时卡面正是 malody4 在供数据，提示才不会张冠李戴；
 * - `chart-unresolved`（临时，首次未命中）与 `chart-unknown-identity`（确定，重试用尽）各自
 *   对应一句话；已经是该句 → 什么都不做（state 帧按原因变化推送，不再等 30s）；
 * - 原因消失/换路由（壳清空：换到可解析的谱面、游戏空闲）→ **只收自己写的那几条**：当前状态若
 *   已被分析流程改写（`analysis.js` 的 "Loading beatmap file…" 或元信息行），那是更新的权威，
 *   绝不覆盖。提示因而自我清除：换一张能解析的谱面时状态行会被分析流程重写。
 */
function syncUnknownIdentityNotice() {
    let notice = null;
    if (currentRoute() === "malody4") {
        if (state.malody4Reason === UNRESOLVED_CHART_REASON) {
            notice = UNRESOLVED_CHART_NOTICE;
        } else if (state.malody4Reason === UNKNOWN_IDENTITY_REASON) {
            notice = UNKNOWN_IDENTITY_NOTICE;
        }
    }
    if (notice) {
        if (state.statusText !== notice) {
            setStatus(notice, "error");
        }
        return;
    }
    if (SPECTATOR_NOTICES.has(state.statusText)) {
        setStatus("", "ok");
    }
}

/**
 * 应用壳 `malody4_selection` 帧（Malody 4 选曲记录）。
 *
 * `state.malody4Alive` 是**诊断位 + 圆点/日志**用的值：`decide()` 不读它，路由只由 L1
 * `malody4Playing` 与 L2 事件窗口决定（避免下一位维护者以为它是死字段）。
 * @param {object} payload selection 帧 payload
 */
export function applyMalody4Selection(payload) {
    state.malody4LastSeq = payload.sequence != null ? payload.sequence : null;
    state.malody4LastSeenAt = Date.now();
    state.malody4Alive = Date.now() - state.malody4LastSeenAt <= MALODY4_ALIVE_WINDOW_MS;
    // L2 续约：只有"真的选中了谱面"的帧才算新鲜事件。hidden 帧（path 为空）与心跳必须
    // 排除，否则壳只要在跑（hidden 心跳永不停止）就会把路由永久钉在 malody4，饿死 osu/Etterna。
    if (payload.path && payload.event !== "heartbeat") {
        notifySourceEvent("malody4");
    }
}
