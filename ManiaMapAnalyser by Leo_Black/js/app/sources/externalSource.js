// 外部谱面源（Etterna/Malody）接入：壳 song 帧 → 转换 → state 注入 → recompute。
//
// 与 socketHandlers 同形地写 state（lastBeatmapIdentity/modSignature/speedRate 等），
// 缓存键/覆盖检查/写门全部自然收敛；转换在主线程（缓存命中短路后不会执行）。
// 转换失败 → 直接经 result 帧回执（errors → 壳 500），不进渲染。

import { state } from "../appContext.js";
import { scheduleRecompute } from "../scheduler.js";
import { convertSmSscToOsuText } from "../../parser/smSscToOsuConverter.js";
import { convertMcToOsuText } from "../../parser/mcToOsuConverter.js";
import { sendResult } from "./bridgeClient.js";
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
 * 处理壳 song 帧。路由预检（M5 的 sourceManager 接管前：仅强制锁定校验）。
 * @param {object} payload song 帧 payload
 */
export function handleSongFrame(payload) {
    const requestId = payload.requestId || null;
    const source = payload.source;
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

    // 转换接线（主线程；.osu 直通）
    let osuText = payload.rawText;
    try {
        if (!looksLikeOsu(osuText)) {
            if (source === "etterna") {
                osuText = convertSmSscToOsuText({
                    text: osuText,
                    format: sniffSmFormat(osuText),
                }).osuText;
            } else if (source === "malody") {
                osuText = convertMcToOsuText(osuText).osuText;
            } else {
                throw new Error(`未知数据源：${source}`);
            }
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
    // state 注入（与 socketHandlers 写字段同形）
    state.lastBeatmapIdentity = String(payload.identity || "");
    state.pendingSourceText = osuText;
    state.pendingSourceRequestId = requestId;
    state.pendingSourceActive = source;
    state.speedRate = Number(mod.speedRate) || 1;
    state.odFlag = String(mod.odFlag || "none");
    state.cvtFlag = String(mod.cvtFlag || "none");
    // 外部源 modSignature 直构（不走 modData 派生、与 client 无关）
    state.modSignature = `${state.speedRate.toFixed(5)}|${state.odFlag}|${state.cvtFlag}|${mod.classic || 0}`;
    state.externalSourceActive = source;
    notifySourceEvent(source);

    scheduleRecompute("external source song", false);
}