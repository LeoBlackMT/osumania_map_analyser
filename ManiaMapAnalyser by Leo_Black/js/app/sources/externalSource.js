// 外部谱面源（Etterna/Malody）接入：壳 song 帧 → 转换 → state 注入 → recompute。
//
// 与 socketHandlers 同形地写 state（lastBeatmapIdentity/modSignature/speedRate 等），
// 缓存键/覆盖检查/写门全部自然收敛；转换在主线程（缓存命中短路后不会执行）。
// 转换失败 → 直接经 result 帧回执（errors → 壳 500），不进渲染。

import { state } from "../appContext.js";
import { scheduleRecompute } from "../scheduler.js";
import { convertSmSscToOsuText } from "../../parser/smSscToOsuConverter.js";
import { convertMcToOsuText } from "../../parser/mcToOsuConverter.js";
import { sendResult, sendDiag } from "./bridgeClient.js";
import { notifySourceEvent, routeAllowsExternal } from "./sourceManager.js";

function looksLikeOsu(text) {
    return typeof text === "string"
        && text.includes("[HitObjects]")
        && (text.includes("\n") || text.includes("\r"));
}

function sniffSmFormat(text) {
    return /#NOTEDATA/i.test(text) ? "ssc" : "sm";
}

/**
 * Etterna 谱面可能是 .sm/.ssc 容器内嵌 osu 文本（[HitObjects] 直通）。
 * 统一入口：osu 直通，否则按格式转换。
 * @param {string} rawText 源谱面原文
 * @param {string|null} difficulty 桥上报的难度名（同图多难度时选对应块）
 */
function convertEtternaText(rawText, difficulty = null) {
    if (looksLikeOsu(rawText)) {
        return rawText;
    }
    return convertSmSscToOsuText({
        text: rawText,
        format: sniffSmFormat(rawText),
        difficulty: difficulty || null,
    }).osuText;
}

/**
 * 处理壳 song 帧。路由预检（M5 的 sourceManager 接管前：仅强制锁定校验）。
 * @param {object} payload song 帧 payload
 */
export function handleSongFrame(payload) {
    const requestId = payload.requestId || null;
    const source = payload.source;
    // 诊断：页面收到 song 帧即向壳回日志（确认 WS 双向通）。
    try {
        sendDiag(`page got song: source=${source} req=${requestId} rawLen=${(payload.rawText || "").length}`);
    } catch {
        // 诊断失败静默
    }
    const locked = state.externalSourceLocked;
    const fail = (errors) => {
        if (requestId) {
            sendResult({
                requestId,
                statusHint: "routing-reject",
                activeSource: locked || "",
                errors,
            });
        }
    };

    if (!routeAllowsExternal(source)) {
        fail([`路由不可用：当前活跃源为 ${locked || "osu"}`]);
        return;
    }

    // 转换接线（主线程；osu 直通）。同图多难度：用桥上报的 difficulty 选对应块。
    let osuText = payload.rawText;
    try {
        if (source === "etterna") {
            const diff = payload.meta && payload.meta.difficulty ? String(payload.meta.difficulty) : null;
            osuText = convertEtternaText(osuText, diff);
        } else if (source === "malody") {
            osuText = looksLikeOsu(osuText) ? osuText : convertMcToOsuText(osuText).osuText;
        } else {
            throw new Error(`未知数据源：${source}`);
        }
    } catch (e) {
        if (requestId) {
            sendResult({
                requestId,
                statusHint: "analysis-failed",
                activeSource: source,
                errors: [String((e && e.message) || e)],
            });
        }
        return;
    }

    const mod = payload.modData || {};
    // state 注入（与 socketHandlers 写字段同形）：无 OD/cvt 修改时用 null
    // （与基线一致；"none" 字符串会被估算器 parseFloat 成 NaN）。
    state.lastBeatmapIdentity = String(payload.identity || "");
    state.pendingSourceText = osuText;
    state.pendingSourceRequestId = requestId;
    state.pendingSourceActive = source;
    state.speedRate = Number(mod.speedRate) || 1;
    // odFlag/cvtFlag：仅接受真实修改值；"none"/空/null 一律归一为 null
    // （"none" 会被估算器 parseFloat 成 NaN → star 全链 NaN——历史事故）。
    const normFlag = (v) => (v == null || v === "" || v === "none" ? null : v);
    state.odFlag = normFlag(mod.odFlag);
    state.cvtFlag = normFlag(mod.cvtFlag);
    // 外部源 modSignature 直构（不走 modData 派生、与 client 无关）；
    // 签名文本保持 "none" 稳定（缓存键用，与 state 数值语义分离）。
    state.modSignature = `${state.speedRate.toFixed(5)}|${mod.odFlag || "none"}|${mod.cvtFlag || "none"}|${mod.classic || 0}`;
    state.externalSourceActive = source;
    // osu 文本直通标记：osu 谱无 Etterna MSD 语义 → 主体不选 Etterna（回退 Pattern）。
    state.externalSourceOsuLike = looksLikeOsu(payload.rawText);
    notifySourceEvent(source);

    scheduleRecompute("external source song", false);
}