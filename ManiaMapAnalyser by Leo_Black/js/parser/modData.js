function collectValues(value) {
    if (Array.isArray(value)) {
        return value.filter(Boolean);
    }
    if (value && typeof value === "object") {
        return Object.values(value).filter(Boolean);
    }
    return [];
}

function collectPlayModsCandidates(data) {
    const candidates = [data?.play?.mods];

    for (const tourneyClient of collectValues(data?.tourney?.clients)) {
        candidates.push(tourneyClient?.play?.mods);
    }
    for (const ipcClient of collectValues(data?.tourney?.ipcClients)) {
        candidates.push(ipcClient?.gameplay?.mods);
    }

    return candidates;
}

function collectMenuModsCandidates(data) {
    return [data?.menu?.mods];
}

function collectResultsModsCandidates(data) {
    return [data?.resultsScreen?.mods];
}

function addCodesFromString(codes, value, sortedKnownModCodes) {
    if (typeof value !== "string" || value.trim().length === 0) {
        return;
    }
    // Keep digits: "SV2" (ScoreV2) must survive normalization, same as the
    // lazer acronym path which preserves them verbatim.
    const normalized = value.toUpperCase().replace(/[^A-Z0-9]/g, "");
    let index = 0;
    while (index < normalized.length) {
        let matched = false;
        for (const code of sortedKnownModCodes) {
            if (normalized.startsWith(code, index)) {
                codes.add(code);
                index += code.length;
                matched = true;
                break;
            }
        }
        if (!matched) {
            index += 1;
        }
    }
}

function addCodesFromNumber(codes, value, modBitFlagEntries) {
    const number = Number(value);
    if (!Number.isFinite(number)) {
        return;
    }
    for (const [code, bit] of modBitFlagEntries) {
        if ((number & bit) !== 0) {
            codes.add(code);
        }
    }
}

export function getModData(data, {
    sortedKnownModCodes,
    modBitFlagEntries,
    fallbackClient,
    preferPlayMods = false,
    prefer = null,
}) {
    const payloadClient = String(data?.client || "").toLowerCase();
    const client = payloadClient || String(fallbackClient || "").toLowerCase();

    const playModsCandidates = collectPlayModsCandidates(data);
    const menuModsCandidates = collectMenuModsCandidates(data);
    const resultsModsCandidates = collectResultsModsCandidates(data);

    // prefer:
    // - play：只信本局 play.mods；
    // - results：只信该成绩自己的 resultsScreen.mods（缺载荷时按显式 NM）；
    // - select：选歌界面菜单 mods 优先；lazer 原生载荷把当前 SelectedMods 放在
    //   play.mods（menu.mods 恒 null），所以菜单缺载荷时回退 play.mods；
    // - null / 其他：保持旧的并集行为（debug 面板等）。
    let selectedModsCandidates;
    if (prefer === "play" || preferPlayMods) {
        selectedModsCandidates = playModsCandidates;
    } else if (prefer === "results") {
        const resultsMods = data?.resultsScreen?.mods;
        selectedModsCandidates = [resultsMods === undefined || resultsMods === null ? [] : resultsMods];
    } else if (prefer === "select") {
        const menuMods = data?.menu?.mods;
        selectedModsCandidates = [menuMods === undefined || menuMods === null ? data?.play?.mods : menuMods];
    } else {
        selectedModsCandidates = [...playModsCandidates, ...menuModsCandidates, ...resultsModsCandidates];
    }

    const validMods = selectedModsCandidates.filter((mods) => mods !== undefined && mods !== null);
    const hasModPayload = validMods.length > 0;

    const modCodes = new Set();
    const modArrays = [];
    let hasModInfo = false;
    let hasExplicitNoMod = false;
    for (const mods of validMods) {
        const nameText = typeof mods?.name === "string" ? mods.name.trim() : "";
        const strText = typeof mods?.str === "string" ? mods.str.trim() : "";
        const acronymText = typeof mods?.acronym === "string" ? mods.acronym.trim() : "";
        const numberValue = Number(mods?.number);
        const numValue = Number(mods?.num);

        if (nameText.length > 0) {
            hasModInfo = true;
        }
        if (strText.length > 0) {
            hasModInfo = true;
        }
        if (acronymText.length > 0) {
            hasModInfo = true;
        }
        if (Number.isFinite(numberValue) || Number.isFinite(numValue)) {
            hasModInfo = true;
        }

        if (/^(NM|NOMOD|NONE)$/i.test(nameText)
            || /^(NM|NOMOD|NONE)$/i.test(strText)
            || /^(NM|NOMOD|NONE)$/i.test(acronymText)
            || (Number.isFinite(numberValue) && numberValue === 0)
            || (Number.isFinite(numValue) && numValue === 0)) {
            hasExplicitNoMod = true;
        }

        addCodesFromString(modCodes, mods?.name, sortedKnownModCodes);
        addCodesFromString(modCodes, mods?.str, sortedKnownModCodes);
        addCodesFromString(modCodes, mods?.acronym, sortedKnownModCodes);
        addCodesFromNumber(modCodes, mods?.number, modBitFlagEntries);
        addCodesFromNumber(modCodes, mods?.num, modBitFlagEntries);

        if (Array.isArray(mods?.array)) {
            if (mods.array.length > 0) {
                hasModInfo = true;
            } else {
                hasExplicitNoMod = true;
            }
            modArrays.push(mods.array);
        }
        if (Array.isArray(mods)) {
            if (mods.length > 0) {
                hasModInfo = true;
            } else {
                hasExplicitNoMod = true;
            }
            modArrays.push(mods);
        }
    }

    for (const arrayMods of modArrays) {
        for (const modItem of arrayMods) {
            if (!modItem) {
                continue;
            }
            if (typeof modItem === "string") {
                addCodesFromString(modCodes, modItem, sortedKnownModCodes);
                continue;
            }
            addCodesFromString(
                modCodes,
                modItem?.acronym || modItem?.code || modItem?.shortName || modItem?.name,
                sortedKnownModCodes,
            );
        }
    }

    let speedRate = 1.0;
    let odFlag = null;
    let cvtFlag = null;
    let daOverallDifficulty = null;
    let lazerSpeedChange = null;
    let randomSeed = null;

    const parseSeed = (value) => {
        if (value === undefined || value === null || value === "") {
            return null;
        }
        const seed = Number(value);
        return Number.isInteger(seed) && seed >= -2147483648 && seed <= 2147483647
            ? seed
            : null;
    };

    if (client === "lazer") {
        for (const arrayMods of modArrays) {
            for (const modItem of arrayMods) {
                if (!modItem || typeof modItem !== "object") {
                    continue;
                }

                const acronym = String(
                    modItem?.acronym || modItem?.code || modItem?.shortName || modItem?.name || "",
                ).toUpperCase();
                if (acronym) {
                    modCodes.add(acronym);
                }

                const speedChange = Number(
                    modItem?.settings?.speed_change ?? modItem?.speed_change ?? modItem?.rate,
                );
                if (Number.isFinite(speedChange) && speedChange > 0) {
                    lazerSpeedChange = speedChange;
                }

                if (acronym === "DA") {
                    const overallDifficulty = Number(
                        modItem?.settings?.overall_difficulty ?? modItem?.overall_difficulty,
                    );
                    if (Number.isFinite(overallDifficulty)) {
                        daOverallDifficulty = overallDifficulty;
                    }
                }

                if (acronym === "RD") {
                    const seed = parseSeed(modItem?.settings?.seed ?? modItem?.seed);
                    if (seed != null) {
                        randomSeed = seed;
                    }
                }
            }
        }
    } else if (client === "stable") {
        // stable/ppysb 的 seed 由壳从 Score.Seed（play/results）读出，放在
        // play.seed / resultsScreen.seed；只有 RD 真正出现时才消费，
        // 否则 NM 局每次变化的 seed 会让 modSignature 抖动。
        if (modCodes.has("RD")) {
            // 历史结算只信该成绩自己的 resultsScreen.seed，绝不回退上一局 play.seed。
            const stableSeeds = prefer === "results"
                ? [data?.resultsScreen?.seed]
                : (prefer === "select"
                    ? [data?.menu?.seed, data?.play?.seed]
                    : (prefer === "play"
                        ? [data?.play?.seed]
                        : [data?.play?.seed, data?.menu?.seed, data?.resultsScreen?.seed]));
            for (const value of stableSeeds) {
                const seed = parseSeed(value);
                if (seed != null) {
                    randomSeed = seed;
                    break;
                }
            }
        }
    }

    if (client === "lazer" && Number.isFinite(lazerSpeedChange) && lazerSpeedChange > 0) {
        speedRate = lazerSpeedChange;
    } else if (modCodes.has("NC") || modCodes.has("DT")) {
        speedRate = 1.5;
    } else if (modCodes.has("HT") || modCodes.has("DC")) {
        speedRate = 0.75;
    }

    if (client === "lazer" && Number.isFinite(daOverallDifficulty)) {
        odFlag = daOverallDifficulty;
    } else if (modCodes.has("HR")) {
        odFlag = "HR";
    } else if (modCodes.has("EZ")) {
        odFlag = "EZ";
    }

    if (client === "lazer") {
        if (modCodes.has("IN")) {
            cvtFlag = "IN";
        } else if (modCodes.has("HO")) {
            cvtFlag = "HO";
        }
    }

    const hasRelevantModInfo = modCodes.size > 0
        || Number.isFinite(lazerSpeedChange)
        || Number.isFinite(daOverallDifficulty)
        || randomSeed != null
        || cvtFlag != null
        || odFlag != null
        || Math.abs(Number(speedRate) - 1.0) > 1e-6;

    const hasExplicitNoModSignal = hasExplicitNoMod
        && !hasRelevantModInfo;

    // SV2 模组存在 → ScoreV2 计分 → 非 Classic（stable 导入 lazer 的成绩可带 CL+SV2 → 非 Classic）
    const classic = !modCodes.has("SV2")
        && (client !== "lazer" || modCodes.has("CL"));

    const mirror = modCodes.has("MR");
    const randomEngine = randomSeed == null
        ? "none"
        : (client === "stable" ? "stable" : "lazer");

    // Only include calculation-relevant dimensions in signature.
    // 第 5 段起：RD seed / RD 引擎 / MR。引擎必须进键——stable 与 Lazer
    // 对同一 (K, seed) 的置换不同，不能互相命中缓存。
    const modSignature = [
        Number(speedRate).toFixed(5),
        odFlag == null ? "none" : String(odFlag),
        cvtFlag == null ? "none" : String(cvtFlag),
        classic ? "1" : "0",
        randomSeed == null ? "none" : String(randomSeed),
        randomEngine,
        mirror ? "1" : "0",
    ].join("|");

    return {
        client,
        speedRate,
        odFlag,
        cvtFlag,
        modSignature,
        classic,
        modCodes: [...modCodes].sort(),
        randomSeed,
        randomEngine,
        mirror,
        hasModPayload,
        hasModInfo: hasRelevantModInfo,
        hasExplicitNoMod: hasExplicitNoModSignal,
    };
}

export function extractCurrentSongTimeMs(data) {
    const liveTime = Number(data?.beatmap?.time?.live);

    // 选歌帧常见 beatmap.time.live=0（字段存在但尚未播放），此时不能
    // 用它遮住 menu.bm.time.current；否则 Graph 进度在选歌界面永远停在 0。
    if (Number.isFinite(liveTime) && liveTime > 0) {
        return liveTime;
    }

    const candidates = [
        data?.beatmap?.time?.current,
        data?.menu?.bm?.time?.current,
        data?.play?.time?.current,
        data?.resultsScreen?.time?.current,
    ];

    for (const value of candidates) {
        const num = Number(value);
        if (Number.isFinite(num) && num > 0) {
            return num;
        }
    }

    // 保留合法的 0（真正的播放起点）；全无有效时间才返回 null。
    if (Number.isFinite(liveTime)) {
        return liveTime;
    }
    return null;
}
