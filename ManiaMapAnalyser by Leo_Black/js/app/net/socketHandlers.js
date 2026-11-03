import {
    MOD_BIT_FLAG_ENTRIES,
    NOTE_END_MARGIN_MS,
    PAUSE_DETECT_EPSILON_MS,
    PAUSE_FALLBACK_THRESHOLD_MS,
    SONG_TIME_JUMP_THRESHOLD_MS,
    SORTED_KNOWN_MOD_CODES,
    socket,
    state,
} from "../core/appContext.js";
import { laneAccView } from "../ui/laneAcc/view.js";
import { sendLaneAccSync } from "../sources/bridgeClient.js";
import {
    extractCurrentSongTimeMs as extractCurrentSongTimeMsFromPayload,
    getModData as getModDataFromPayload,
} from "../../parser/modData.js";
import {
    isPlayStateName,
    isResultScreenStateName,
    normalizeClientStateName,
} from "../core/modeLogic.js";
import {
    addPauseMarker,
    refreshGraphTimeline,
    resetPauseRuntime,
} from "../graph/index.js";
import { updateCardPlayVisibility } from "../ui/hud.js";
import { scheduleRecompute } from "../core/scheduler.js";
import { getCounterPathForCommand, resetContentBarTabOverride } from "../core/settings.js";
import { applyCoverThemeForBeatmap } from "../ui/coverTheme.js";
import { updateLivePp } from "../live/livePp.js";
import { buildSongKey, resolveChangeKind } from "../ui/changeKind.js";
import { noteTelemetryActivity } from "./telemetry.js";
import { isOsuSuppressed } from "../sources/sourceManager.js";
import { notifySourceEvent } from "../sources/sourceManager.js";
import { noteOsuScanPayloadArrival } from "../sources/osuScanHint.js";
import { noteFeedbackFrame, resetFeedbackStrip } from "../feedback/feedbackStrip.js";
import { clearSection, isSameBeatmapIdentity } from "../graph/section.js";


function getModData(data) {
    return getModDataFromPayload(data, {
        sortedKnownModCodes: SORTED_KNOWN_MOD_CODES,
        modBitFlagEntries: MOD_BIT_FLAG_ENTRIES,
        fallbackClient: state.client,
        preferPlayMods: state.isInPlayState,
    });
}

function extractCurrentSongTimeMs(data) {
    return extractCurrentSongTimeMsFromPayload(data);
}

function updateSongTimeState(data) {
    const beatmapTime = data?.beatmap?.time;
    const liveTimeMs = extractCurrentSongTimeMs(data);

    const speedRate = Number.isFinite(state.speedRate) && state.speedRate > 0 ? state.speedRate : 1;

    // 谱面时间线的更新不依赖 live 时间：早期实现在无 live 时直接返回，
    // 会连带丢掉 firstObject/lastObject，使图表 x 轴窗口永远按"未知时间线"渲染。
    const firstObjectMs = Number(beatmapTime?.firstObject);
    const lastObjectMs = Number(beatmapTime?.lastObject);
    const previousSongStartMs = state.songStartMs;
    state.songStartMs = Number.isFinite(firstObjectMs) ? firstObjectMs / speedRate : null;
    state.songEndMs = Number.isFinite(lastObjectMs) ? lastObjectMs / speedRate : null;

    // 图表 x 轴窗口按渲染时刻的 songStartMs 裁剪；tosu 常在首次渲染后才补全
    // firstObject（选歌阶段为 0），此处补一次重建，避免前奏空档 + 进度线偏移
    // 残留整首歌。未变化时 refreshGraphTimeline 立即返回。
    if (state.songStartMs !== previousSongStartMs) {
        refreshGraphTimeline();
    }

    if (!Number.isFinite(liveTimeMs)) {
        return;
    }

    const scaledLiveTimeMs = liveTimeMs / speedRate;

    if (state.pauseDetectionEnabled && state.isInPlayState && state.pauseMarkerTimes.length > 0) {
        let earliestPauseTimeMs = Number.POSITIVE_INFINITY;
        for (const markerTime of state.pauseMarkerTimes) {
            if (Number.isFinite(markerTime) && markerTime < earliestPauseTimeMs) {
                earliestPauseTimeMs = markerTime;
            }
        }
        if (Number.isFinite(earliestPauseTimeMs) && (scaledLiveTimeMs + PAUSE_DETECT_EPSILON_MS) < earliestPauseTimeMs) {
            resetPauseRuntime(true);
        }
    }

    const now = performance.now();
    const previousTime = state.songTimeMs;

    if (!state.hasSongTimeSample) {
        state.hasSongTimeSample = true;
        state.prevSongTimeMs = scaledLiveTimeMs;
        state.prevSongTimeReceiveTs = now;
        state.songTimeMs = scaledLiveTimeMs;
        state.songTimeReceiveTs = now;
        state.songTimeLastMoveAt = now;
        state.frozenInterpMs = scaledLiveTimeMs;
        return;
    }

    if (scaledLiveTimeMs > previousTime + PAUSE_DETECT_EPSILON_MS) {
        state.playbackAdvanced = true;
    }

    if (state.pauseDetectionEnabled && state.isInPlayState) {
        // 初始值必须继承当前暂停态：原生 paused 的防抖等待期间不能把已有暂停解除。
        let gamePaused = state.isPaused;

        if (typeof data?.game?.paused === "boolean") {
            // 优先层：数据源有原生/推导标志（官方 tosu 或桌面壳 native 24062）。
            // Stable 的 `paused` 是“两次读取时间相同”的推导值，开局加载阶段
            // 常连续出现相同时间；必须等谱面真正前进过并持续 500ms，才认定为暂停。
            const nativePaused = data.game.paused;
            if (!state.playbackAdvanced) {
                gamePaused = false;
                state.pauseFreezeStartRealMs = 0;
            } else if (nativePaused) {
                if (!state.pauseFreezeStartRealMs) {
                    state.pauseFreezeStartRealMs = now;
                } else if ((now - state.pauseFreezeStartRealMs) >= PAUSE_FALLBACK_THRESHOLD_MS) {
                    gamePaused = true;
                }
            } else {
                state.pauseFreezeStartRealMs = 0;
                gamePaused = false;
            }
        } else {
            // 兜底层：数据源无 game.paused 字段时（旧版 tosu 或缺失字段的数据源），以 500ms 谱面时间停滞推测
            const isTimeFrozen = Math.abs(scaledLiveTimeMs - previousTime) <= PAUSE_DETECT_EPSILON_MS;
            if (isTimeFrozen && !state.isPaused) {
                if (!state.pauseFreezeStartRealMs) {
                    state.pauseFreezeStartRealMs = now;
                } else if ((now - state.pauseFreezeStartRealMs) >= PAUSE_FALLBACK_THRESHOLD_MS) {
                    gamePaused = true;
                }
            } else if (!isTimeFrozen) {
                state.pauseFreezeStartRealMs = 0;
                gamePaused = false;
            } else if (state.isPaused) {
                // 已在暂停态且时间未前进，维持暂停
                gamePaused = true;
            }
        }

        if (gamePaused && !state.isPaused) {
            // 仅在谱面时间线内记录暂停标记（开头之前 / 末尾缓冲带内不记）。
            const hasTimelineEnd = Number.isFinite(state.songEndMs);
            const hasTimelineStart = Number.isFinite(state.songStartMs);
            const atTimelineEnd = hasTimelineEnd && scaledLiveTimeMs >= (state.songEndMs - NOTE_END_MARGIN_MS);
            const beforeTimelineStart = hasTimelineStart && scaledLiveTimeMs < state.songStartMs;
            if (!atTimelineEnd && !beforeTimelineStart) {
                addPauseMarker(scaledLiveTimeMs);
                state.pauseTimeMs = scaledLiveTimeMs;
                state.frozenInterpMs = scaledLiveTimeMs;
            }
            state.isPaused = true;
        } else if (!gamePaused && state.isPaused) {
            state.isPaused = false;
            state.pauseTimeMs = 0;
            state.pauseFreezeStartRealMs = 0;
        }
    } else {
        state.isPaused = false;
        state.pauseTimeMs = 0;
        state.pauseFreezeStartRealMs = 0;
        state.frozenInterpMs = state.songTimeMs;
    }

    // 原生读取器按 150ms 推包，但谱面时钟的更新粒度更粗：连续两包可能给出
    // 完全相同的 live 时间。旧写法把「重复样本」也当成新锚点（delta=0），
    // 图表插值速率随即变 0，下一包再跳一大步 → 游标 10→11→10→11 抖动。
    // 只有时钟真的前进/回跳时才滚动基准；重复样本继续沿用上一次正向速率。
    const movedForward = scaledLiveTimeMs > previousTime;
    const jumped = Math.abs(scaledLiveTimeMs - previousTime) > SONG_TIME_JUMP_THRESHOLD_MS;
    if (movedForward || jumped) {
        state.prevSongTimeMs = previousTime;
        state.prevSongTimeReceiveTs = state.songTimeReceiveTs;
        state.songTimeMs = scaledLiveTimeMs;
        state.songTimeReceiveTs = now;
        state.songTimeLastMoveAt = now;
    }
    // 重复样本（时钟粒度粗于推包频率）不刷新锚点：让图表继续按上一次
    // 正向速率外推，直到下一包真正前进；静止超过窗口后由 graph 层冻结。

    if (jumped) {
        state.prevSongTimeMs = state.songTimeMs;
        state.prevSongTimeReceiveTs = state.songTimeReceiveTs;
    }
}

/** 信号段：游玩态/songTime/livePP（败方门控豁免照读，供 Auto 路由回 osu）。 */
export function applySignalState(data) {
    noteTelemetryActivity();
    let laneAccEnteredPlay = false;
    let laneAccEnteredResult = false;
        const normalizedClientStateName = normalizeClientStateName(data?.state?.name);
        if (normalizedClientStateName) {
            const wasInPlayState = state.isInPlayState;
            const wasResultScreen = state.clientStateName === "resultscreen";
            const nextInPlayState = isPlayStateName(normalizedClientStateName);
            const nextIsResultScreen = isResultScreenStateName(normalizedClientStateName);
            const enteredPlayState = !wasInPlayState && nextInPlayState;
            const leftPlayState = wasInPlayState && !nextInPlayState;

            state.clientStateName = normalizedClientStateName;
            state.isInPlayState = nextInPlayState;

            // 结算屏来源：只有"刚从 play 态离开"进入的结算屏才是刚打完的一局。
            // 从选曲/本地排行榜直接打开的结算屏（查看历史成绩）没有 play 前态 ⇒
            // 不允许反馈；页面启动时已在结算屏（壳中途重启）也判为不确定。
            if (nextIsResultScreen && !wasResultScreen) {
                state.resultsFromPlay = leftPlayState;
                laneAccEnteredResult = true;
                // 结算屏不保留选区（D2）：复位后由重算把卡片恢复成整图口径。
                clearSection("results", { recompute: true });
            } else if (!nextIsResultScreen) {
                state.resultsFromPlay = false;
            }

            updateCardPlayVisibility();

            if (enteredPlayState) {
                // 游玩态优先保障实时指标/进度线：选区退出并恢复整图分析。
                laneAccEnteredPlay = true;
                clearSection("play", { recompute: true });
            }

            if (leftPlayState && !nextIsResultScreen) {
                laneAccView.finishSession();
            }

            if (enteredPlayState || (leftPlayState && !nextIsResultScreen)) {
                resetPauseRuntime(true);
            } else if (leftPlayState) {
                resetPauseRuntime(false);
            }
        }

        const modData = getModData(data);
        if (modData.client) {
            state.client = modData.client;
        }
        if (modData.hasModPayload) {
            state.modCodes = modData.modCodes || [];
            state.classicMod = Boolean(modData.classic);
        }

        // LaneAcc 的 play/结算边沿放在 mods 之后：窗口用本局的 speedRate/OD。
        if (laneAccEnteredPlay) {
            laneAccView.startSession();
            sendLaneAccSync();
        }
        if (laneAccEnteredResult) {
            laneAccView.finishSession();
        }

        updateSongTimeState(data);

        // 每消息实时 PP：内部自带 early-return 守卫（成本极低），必须在
        // beatmap 守卫之前，保证 play/resultScreen 状态变化也走此路径。
        updateLivePp(data);
}

/** 状态段：谱面身份 / mod 应用与换图触发（败方门控挂起的部分）。 */
export function applyBeatmapState(data) {
        // ⚠️ 必须在此重取 modData：拆分 setupSocketListener 时该 const 留在
        // applySignalState 段，函数作用域隔离——此处直接引用会抛
        // ReferenceError: modData is not defined，换图/换 mod 的 recompute
        // 永远执行不到（socket.js try/catch 吞掉，仅控制台 MESSAGE_ERROR）。
        const modData = getModData(data);
        const beatmap = data?.beatmap;
        if (!beatmap) return;

        const normalizeText = (value) => {
            if (value == null) return "";
            return String(value).trim();
        };

        const normalizePathText = (value) => {
            const normalized = normalizeText(value).replace(/\\/g, "/");
            if (!normalized) return "";
            return normalized.replace(/\/+/g, "/").toLowerCase();
        };

        const normalizeNumberText = (value) => {
            const num = Number(value);
            if (!Number.isFinite(num) || num <= 0) {
                return "";
            }
            return String(Math.trunc(num));
        };

        const beatmapId = normalizeNumberText(beatmap?.id);
        const beatmapHash = normalizeText(beatmap?.md5 || beatmap?.checksum).toLowerCase();
        const beatmapPath = normalizePathText(data?.files?.beatmap || data?.directPath?.beatmapFile);
        const beatmapTitleKey = [
            normalizeText(beatmap?.artist),
            normalizeText(beatmap?.title),
            normalizeText(beatmap?.version),
            normalizeText(beatmap?.mapper),
        ].join("::").toLowerCase();

        // 曲（mapset）单位的标识：不含 version/难度名，也不含 md5/id/path，
        // 这样同一 mapset 内切换难度时 songKey 保持不变，可据此区分
        // "换歌" 与 "换难度"。来源单一优先 set > dir > meta——
        // 避免 api_v2 各状态字段集合波动导致 key 变化（误判为换歌）。
        const beatmapSetId = normalizeNumberText(beatmap?.set || beatmap?.setId || beatmap?.beatmapSetId);
        const beatmapFolderPath = (() => {
            const folder = normalizePathText(data?.directPath?.beatmapBackground
                || data?.directPath?.audioFile
                || data?.folders?.beatmap);
            if (folder) return folder;
            if (!beatmapPath) return "";
            // 退而求其次：取谱面文件所在目录作为 mapset 归属。
            const lastSlash = beatmapPath.lastIndexOf("/");
            return lastSlash > 0 ? beatmapPath.slice(0, lastSlash) : beatmapPath;
        })();
        const songMetaKey = [
            normalizeText(beatmap?.artist),
            normalizeText(beatmap?.title),
            normalizeText(beatmap?.mapper),
        ].join("::").toLowerCase();
        const nextSongKey = buildSongKey({ beatmapSetId, beatmapFolderPath, songMetaKey });

        const previousBeatmapIdentity = state.lastBeatmapIdentity || "";
        const previousModSignature = state.modSignature || "";
        const previousSongKey = state.lastSongKey || "";

        const identityParts = [];
        if (beatmapId) {
            identityParts.push(`id:${beatmapId}`);
        }
        if (beatmapHash) {
            identityParts.push(`hash:${beatmapHash}`);
        }
        if (beatmapPath) {
            identityParts.push(`path:${beatmapPath}`);
        }

        const hasMetadataIdentity = beatmapTitleKey.replace(/[:]/g, "").length > 0;
        if (identityParts.length === 0 && hasMetadataIdentity) {
            identityParts.push(`meta:${beatmapTitleKey}`);
        }

        const nextBeatmapIdentity = identityParts.join("|");
        if (!nextBeatmapIdentity) return;

        // 反馈条只认最近一次解析出的谱面身份：结算帧通常不带 beatmap，必须留档。
        if (beatmapId) state.currentBeatmapId = Number(beatmapId);
        if (/^[0-9a-f]{32}$/.test(beatmapHash)) state.currentBeatmapChecksum = beatmapHash;
        const objectsTotal = Number(beatmap?.stats?.objects?.total);
        if (Number.isFinite(objectsTotal) && objectsTotal > 0) {
            state.currentObjectsTotal = objectsTotal;
        }

        // api_v2 packets can be partial. Only apply incoming mod state when
        // mod payload is explicitly present; otherwise keep current state.
        const shouldApplyModState = !previousModSignature
            || (modData.hasModPayload && (modData.hasModInfo || modData.hasExplicitNoMod));
        const nextModSignature = shouldApplyModState
            ? modData.modSignature
            : previousModSignature;

        // 只比稳定段（id 优先，其次 hash）：原生/部分包会在个别帧缺
        // `checksum`/path。旧实现把这种「同一张图的弱帧」当成新图，每个包都
        // 清选区、重置 LaneAcc 并重排 200ms 重算，防抖永远不触发（表现：
        // 切图不重算、Graph 双击闪一下退出）。
        const identityChanged = !isSameBeatmapIdentity(previousBeatmapIdentity, nextBeatmapIdentity);
        const hasStateMismatch = identityChanged
            || nextModSignature !== previousModSignature;
        if (!hasStateMismatch) return;
        // Step 9f：**带谱面身份、且真的改动了卡片**的载荷 ⇒ 壳相位提示（若有）撤掉。
        // 判据刻意收窄（两道门）：① 无 `beatmap` 的空帧、身份没变的重复帧都不是"真载荷"
        // ——Step 9e 把钩子挂在每条 v2 帧上（含空帧），提示因此被自己的清理规则打掉、且同一
        // 相位值不再复活（真机现象：只闪 ~300 ms；tosu 的 v2 是 ≈10 帧/s 的持续流）；
        // ② 只有页面真的在读壳的原生端点时才算（在 `noteOsuScanPayloadArrival` 里判）。
        noteOsuScanPayloadArrival();
        notifySourceEvent("osu");

        const previousCvtFlag = state.cvtFlag;
        if (shouldApplyModState) {
            state.speedRate = modData.speedRate;
            state.odFlag = modData.odFlag;
            state.cvtFlag = modData.cvtFlag;
            state.modSignature = nextModSignature;
        }
        const cvtChanged = state.cvtFlag !== previousCvtFlag;

        // 区分本次变化的类型，供渲染层选择对应的入场动画（song/difficulty/mod）。
        state.pendingChangeKind = resolveChangeKind({
            previousBeatmapIdentity,
            nextBeatmapIdentity,
            previousSongKey,
            nextSongKey,
        });

        // 切图（谱面身份变化）→ 复位选区；仅改 mod（含 DT/HT 速率）不复位：
        // 选区按物理时间保存，速率变化时遮罩不动、切片区间不变（见 graphSection 注释）。
        if (identityChanged) {
            try {
                clearSection("beatmap", { recompute: false });
            } catch (error) {
                console.warn("mma: section reset failed", error);
            }
            // LaneAcc 不能等新谱面分析完成才清：否则切图/跨图包时结果屏会继续显示上一张图。
            // 空身份（partial/结算帧）不清，避免把已保留的终态误清。
            // ⚠️ 这里的 DOM 复位绝不能反过来中断身份写入与重算调度：一旦
            // reset 抛错（曾因 popover 丢失抛 null.hidden），socket 回调的
            // try/catch 会吞掉后半段，表现为「切图永远只显示第一张图」。
            if (nextBeatmapIdentity) {
                try {
                    laneAccView.reset();
                } catch (error) {
                    console.warn("mma: laneAcc reset failed", error);
                }
            }
        }
        if (identityChanged || cvtChanged) {
            try {
                resetContentBarTabOverride();
            } catch (error) {
                console.warn("mma: content bar reset failed", error);
            }
        }

        // 同一张图的弱帧（缺 hash/path）不覆盖更完整的旧身份：缓存键、
        // 原文 LRU 与反馈上下文都按这个字符串索引，来回切换会让它们全部失效。
        const adoptedIdentity = (!previousBeatmapIdentity || identityChanged)
            ? nextBeatmapIdentity
            : previousBeatmapIdentity;
        state.lastBeatmapIdentity = adoptedIdentity;
        // 换图/换 mod：旧分析结果不再对应当前显示值，反馈条立即作废。
        state.feedbackContext = null;
        resetFeedbackStrip();
        // 空 key（partial 包）不污染历史，避免后续完整包比较被污染。
        if (nextSongKey) state.lastSongKey = nextSongKey;
        state.lastBeatmapIdentitySource = identityParts.length > 1
            ? "composite"
            : (identityParts[0]?.split(":")[0] || "");
        // 页面自身的 tosu 数据面收到了可用载荷（壳离线门控据此放行，见 analysis.js）。
        state.tosuDataSeen = true;

        // 仅在谱面本身（非单纯改 mod）发生变化时，重新取封面主色刷新主题。
        // 取色异步进行、失败自动退默认，绝不阻塞分析流程。
        if (state.enableCoverArt && identityChanged) {
            applyCoverThemeForBeatmap(adoptedIdentity).catch(() => {});
        }
        const key = `${adoptedIdentity}|${nextModSignature}`;
        resetPauseRuntime(true);
        state.lastBeatmapKey = key;

        socket.sendCommand("getSettings", getCounterPathForCommand());

        scheduleRecompute("beatmap/mod changed", true);
}

// ── 败方门控（sourceManager 咨询）+ 缓冲回放 ──

let bufferedTosuData = null;

/** 切回 osu 时回放挂起期间最后一条 tosu 状态（缓存键对齐，命中即无额外分析）。 */
export function resumeBufferedOsuState() {
    if (bufferedTosuData) {
        applyBeatmapState(bufferedTosuData);
        bufferedTosuData = null;
    }
}

export function setupSocketListener() {
    socket.api_v2((data) => {
        applySignalState(data);
        if (isOsuSuppressed()) {
            bufferedTosuData = data;
            return;
        }
        applyBeatmapState(data);
        // 反馈条：每帧一次（内部有早退与一次性 token），必须在谱面身份更新之后。
        noteFeedbackFrame(data);
        if (bufferedTosuData) {
            bufferedTosuData = null;
        }
    });
}
