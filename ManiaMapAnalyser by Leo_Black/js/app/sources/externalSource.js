// 外部谱面源（Etterna/Malody V/Malody 4）接入：壳 song 帧 → 转换 → state 注入 → recompute。
//
// 与 socketHandlers 同形地写 state（lastBeatmapIdentity/modSignature/speedRate 等），
// 缓存键/覆盖检查/写门全部自然收敛；转换在主线程（缓存命中短路后不会执行）。
// 转换失败 → 直接经 result 帧回执（errors → 壳 500），不进渲染。
//
// 转换按源分流：etterna → sm/ssc；malody（Malody V）→ 桥通道带判定与窗口因子，
// OD 经 odResolver.resolveOd() 解出后传给转换器（Lua 通道没有判定 ⇒ 用转换器默认 OD）；
// malody4（Malody 4.3.7）→ 判定档 × 速率经 judgeOdTable 解出等效 OD 后传给转换器，
// 判定字母同时进 modSignature 第 5 段（换判定必须重算，不得命中旧快照）。
//
// Malody V 选曲桥（契约 v4）的 song 帧额外携带 `screen` / `judge` / `winScale`：
// - 场景：本文件只做正常场景（`selection` / `playing` / `result`），**不再负责清空**——
//   清空 = shellState 的 `other` 边沿 + 归属门控；`playing` 额外立刻置 `state.malodyPlaying`
//   （L1 即时生效，不必等 30s state 帧）；`other` 理论上收不到，收到则忽略并 sendDiag；
// - 判定与速率：本函数是 `state.malodyJudge` / `state.odOverride` / `state.analysisRate` 的
//   **唯一写入者**（判定与桥事件异步，只放 state 帧会有竞态）；
// - 归属：带 `screen` 的帧把 `state.cardOwner` 置为 `"malody-bridge"`（必须在 notifySourceEvent 之后）。

import { state } from "../appContext.js";
import { scheduleRecompute } from "../scheduler.js";
import { convertSmSscToOsuText } from "../../parser/smSscToOsuConverter.js";
import { convertMcToOsuText } from "../../parser/mcToOsuConverter.js";
import { computeOd } from "../../parser/judgeOdTable.js";
import { resolveOd } from "./odResolver.js";
import { sendResult, sendDiag } from "./bridgeClient.js";
import { notifySourceEvent, routeAllowsExternal } from "./sourceManager.js";
import { setStatus } from "../hud.js";

function looksLikeOsu(text) {
    return typeof text === "string"
        && text.includes("[HitObjects]")
        && (text.includes("\n") || text.includes("\r"));
}

/** 上一次写入状态行的"动态 OD 关闭原因"（null = 已启用）。只在变化时写，避免刷屏。 */
let lastOdNotice = null;
/** 当前状态行是否正是本模块写的"动态 OD 未启用"提示（用于收拾自己写的那一条）。 */
let odNoticeShown = false;

function setOdNoticeShown(shown) {
    odNoticeShown = shown;
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
    // 场景语义（契约 v4 §1）：只有 Malody 选曲桥通道的 song 帧携带 screen/judge/winScale。
    // 缺省即"不是桥帧"（Lua 通道、Etterna、Malody 4）⇒ 按 selection 走既有流程，
    // 桥的判定/速率/归属三段路径一律不进入。
    const bridgeFrame = payload.screen !== undefined;
    const screen = bridgeFrame ? String(payload.screen) : "selection";
    // 诊断：页面收到 song 帧即向壳回日志（确认 WS 双向通）。
    try {
        sendDiag(`page got song: source=${source} req=${requestId} screen=${screen} rawLen=${(payload.rawText || "").length}`);
    } catch {
        // 诊断失败静默
    }
    if (screen === "other") {
        // 壳在 other 稳态只广播 state 帧（不发 song 帧，CONTRACT §11.5）；收到即来源异常（旧壳）。
        // 忽略并留痕，不做任何注入——清空由 state 帧的 eventSeq 边沿负责，绝不在这里清。
        try {
            sendDiag(`page ignored song frame with screen=other: source=${source} req=${requestId}`);
        } catch {
            // 诊断失败静默
        }
        return;
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

    if (screen === "playing") {
        // 游玩中立刻置位：L1 门控立即成立，不必等 30s state 帧。
        // 存活语义仍由壳负责（桥静默 ⇒ 壳下一帧就把 playing 置假），页面不重推。
        state.malodyPlaying = true;
    }

    // 转换接线（主线程；osu 直通）。同图多难度：用桥上报的 difficulty 选对应块。
    let osuText = payload.rawText;
    // 判定档字母（仅 malody4 的 `meta.judge` 提供）归一化为大写；未知/缺失为 "?"。
    // 它进 modSignature 第 5 段：判定不在键里 → 换判定会命中旧快照（旧星数配新 OD，静默错误）。
    let judge = "?";
    // 桥通道的判定（数值 0..4，A~E 只在显示层转换）与窗口缩放因子；缺省/越界 = 未采集到。
    let judgeNumber = null;
    let winScale = 1;
    try {
        if (source === "etterna") {
            const diff = payload.meta && payload.meta.difficulty ? String(payload.meta.difficulty) : null;
            osuText = convertEtternaText(osuText, diff);
        } else if (source === "malody4") {
            // 判定档 × 速率 → 等效 OD（PC 表）；未知判定回落 C 档（8.08）并报 known=false。
            // 注意：mod 的绑定在本函数下方，这里直接用 payload.modData，避免 use-before-init。
            const judgeLetter = payload.meta ? payload.meta.judge : null;
            const { od, judge: normalized, known } = computeOd(
                judgeLetter,
                Number((payload.modData || {}).speedRate) || 1
            );
            judge = known ? normalized : "?";
            osuText = looksLikeOsu(osuText)
                ? osuText
                : convertMcToOsuText(osuText, { overallDifficulty: od }).osuText;
            if (!known) {
                sendDiag(`malody4 judge unknown -> OD fallback ${od}`);
            }
        } else if (source === "malody") {
            // 桥通道：判定（数值 0..4 ↔ A~E）与窗口缩放因子随 song 帧下发（判定与桥事件异步，
            // 只放 state 帧会有 ordering 竞态）。数值越界/非整数一律视为"未采集到"，不猜。
            // ⚠️ `judge === null` 必须显式当"未采集到"：壳只在 `screen=playing` 期间发布判定，
            // selection/result/other 阶段恒为 `null`，而 `Number(null)` 会静默变成 0（= A 档）
            // —— 那正是用户裁定禁止的"拿旧值/默认值冒充当局判定"。
            const rawJudge = bridgeFrame && payload.judge !== null && payload.judge !== undefined
                ? Number(payload.judge)
                : NaN;
            judgeNumber = Number.isInteger(rawJudge) && rawJudge >= 0 && rawJudge <= 4 ? rawJudge : null;
            winScale = bridgeFrame ? Number(payload.winScale) || 1 : 1;
            // OD 换算：判定档 × Pro（严格/常态组）× 倍率形式（Turbo 补偿）三者的函数。
            // `pro`/`turbo` 只对桥帧有意义；任一为未知 ⇒ 折算关闭并给出原因（绝不按常态组冒充）。
            const { od, reason: odReason } = resolveOd(
                judgeNumber,
                bridgeFrame ? payload.pro : null,
                Number((payload.modData || {}).speedRate) || 1,
                bridgeFrame ? payload.turbo : null
            );
            // 唯一写入者（页面侧）：判定档 + 转换器 OD 输入口 + 关闭原因（状态行据此提示）。
            state.malodyJudge = judgeNumber;
            state.odOverride = odReason === null && bridgeFrame ? od : null;
            state.odDisabledReason = odReason;
            // 状态行明示"本次没启用动态 OD 及原因" —— fail-visible：绝不静默按常态组算出星数。
            // 只在原因变化时写，避免逐帧刷屏；原因消失时由分析流程自己重写状态行。
            if (odReason !== lastOdNotice) {
                lastOdNotice = odReason;
                if (odReason !== null) {
                    setStatus(`动态 OD 未启用：${odReason}`, "error");
                    setOdNoticeShown(true);
                } else if (odNoticeShown) {
                    setStatus("", "ok");
                    setOdNoticeShown(false);
                }
            }
            // Lua 通道（非桥帧）没有判定 ⇒ odOverride 为 null ⇒ 转换器用默认 OD，
            // 与基线逐字节一致（不能传 9：那会写成 "9.00"）。
            osuText = looksLikeOsu(osuText)
                ? osuText
                : convertMcToOsuText(osuText, { overallDifficulty: state.odOverride }).osuText;
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
    // ⚠️ `state.analysisRate` 的赋值在下面的 `notifySourceEvent()` **之后**：该函数内含
    // "非桥源 ⇒ analysisRate 复位" 的规则（与 cardOwner 同一处，覆盖 osu/Etterna/Lua 三条路径），
    // 放在此处会被它立刻清掉。
    // odFlag/cvtFlag：仅接受真实修改值；"none"/空/null 一律归一为 null
    // （"none" 会被估算器 parseFloat 成 NaN → star 全链 NaN——历史事故）。
    const normFlag = (v) => (v == null || v === "" || v === "none" ? null : v);
    state.odFlag = normFlag(mod.odFlag);
    state.cvtFlag = normFlag(mod.cvtFlag);
    // 外部源 modSignature 直构（不走 modData 派生、与 client 无关）；
    // 签名文本保持 "none" 稳定（缓存键用，与 state 数值语义分离）；
    // 第 5 段 = 判定（malody4 = 判定档字母 A~E；桥通道 = 判定档数值 0..4 / "?"）；
    // 第 6 段 = 桥通道的窗口缩放因子。**判定与窗口必须同时进键**：Turbo 1.2 与 Dash 1.2 的
    // speedRate 相同但 OD 不同，只加判定维度仍会让这两者互相命中缓存（Context §3f）。
    const judgeSegment = bridgeFrame ? `judge${judgeNumber == null ? "?" : judgeNumber}` : judge;
    const winSegment = bridgeFrame ? `|win${winScale}` : "";
    // 第 7 段 = Pro（严格组 / 常态组 / 未知）。**必须进键**：Pro 在**同一判定档与同一倍率**下
    // 改变换算出的 OD，不进键就会在勾/去勾 Pro 后命中旧快照 —— 卡片星数纹丝不动，
    // 而那正是"Pro 决定用哪组窗口"这件事在用户侧唯一的可见效果。
    const bridgePro = bridgeFrame ? payload.pro : undefined;
    const proSegment = bridgeFrame
        ? `|pro${bridgePro === true ? 1 : bridgePro === false ? 0 : "?"}`
        : "";
    state.modSignature = `${state.speedRate.toFixed(5)}|${mod.odFlag || "none"}|${mod.cvtFlag || "none"}|${mod.classic || 0}|${judgeSegment}${winSegment}${proSegment}`;
    state.externalSourceActive = source;
    // osu 文本直通标记：osu 谱无 Etterna MSD 语义 → 主体不选 Etterna（回退 Pattern）。
    state.externalSourceOsuLike = looksLikeOsu(payload.rawText);
    notifySourceEvent(source);
    // 归属置位（计划 Step 4 第 3 项，位置钉死）：**必须在 `notifySourceEvent` 之后**——
    // 复位规则在 notifySourceEvent 内（`source !== "malody-bridge"` ⇒ `cardOwner = source`）会先把
    // 它写成 "malody"；放在前面会被立刻覆盖，shellState 的归属门控永不生效（清空恒为 0 次）。
    // 判据 = 本帧带 `screen`（只有选曲桥通道带这三个字段，契约 §1）。
    if (bridgeFrame) {
        state.cardOwner = "malody-bridge";
    }
    // 进管线的速率（Context §3f，唯一写入者 = 本函数）：**桥帧一律用真实倍率**，与 osu 的
    // DT/HT 同口径——倍率提高密度、压缩反应时间，这件事与 Mod 是否同时改判定窗口无关。
    // ⚠️ 这里曾写成 `winScale < 1 ? 1 : state.speedRate`，把 `winScale`（一个**倒数**）当成
    // "是否 Mod 派生速率"的布尔用，实测后果：Dash/Rush 被压成 1.0（三者结果完全相同），
    // Slow 走另一分支拿到 0.8 却也只因为巧合。Turbo 反而是对的（它 `winScale = null`）。
    // 判定窗口那一侧的难度由 OD 表达（`resolveOd` 的职责），**不从倍率里再挤一次**。
    // 非桥帧（osu / Etterna / Malody 4 / Lua 通道）⇒ null，回落 `state.speedRate`（既有行为）。
    // `state.speedRate` 本身不变：仍是真实倍率，供 UI 显示、modSignature 与遥测使用。
    // 位置在 `notifySourceEvent()` 之后（它会把非桥源的 analysisRate 复位），见上方注释。
    state.analysisRate = bridgeFrame ? state.speedRate : null;

    scheduleRecompute("external source song", false);
}