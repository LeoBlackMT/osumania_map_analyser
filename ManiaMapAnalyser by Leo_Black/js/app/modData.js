function collectValues(value) {
    if (Array.isArray(value)) {
        return value.filter(Boolean);
    }
    if (value && typeof value === "object") {
        return Object.values(value).filter(Boolean);
    }
    return [];
}

function addCodesFromString(codes, value, sortedKnownModCodes) {
    if (typeof value !== "string" || value.trim().length === 0) {
        return;
    }
    const normalized = value.toUpperCase().replace(/[^A-Z]/g, "");
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

export function getModData(data, { sortedKnownModCodes, modBitFlagEntries }) {
    const client = String(data?.client || "").toLowerCase();

    const modsCandidates = [data?.play?.mods, data?.menu?.mods, data?.resultsScreen?.mods];

    for (const tourneyClient of collectValues(data?.tourney?.clients)) {
        modsCandidates.push(tourneyClient?.play?.mods);
    }
    for (const ipcClient of collectValues(data?.tourney?.ipcClients)) {
        modsCandidates.push(ipcClient?.gameplay?.mods);
    }

    const validMods = modsCandidates.filter(Boolean);

    const modCodes = new Set();
    const modArrays = [];
    for (const mods of validMods) {
        addCodesFromString(modCodes, mods?.name, sortedKnownModCodes);
        addCodesFromString(modCodes, mods?.str, sortedKnownModCodes);
        addCodesFromString(modCodes, mods?.acronym, sortedKnownModCodes);
        addCodesFromNumber(modCodes, mods?.number, modBitFlagEntries);
        addCodesFromNumber(modCodes, mods?.num, modBitFlagEntries);

        if (Array.isArray(mods?.array)) {
            modArrays.push(mods.array);
        }
        if (Array.isArray(mods)) {
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
            addCodesFromString(modCodes, modItem?.acronym, sortedKnownModCodes);
        }
    }

    let speedRate = 1.0;
    let odFlag = null;
    let cvtFlag = null;
    let daOverallDifficulty = null;
    let lazerSpeedChange = null;

    if (client === "lazer") {
        for (const arrayMods of modArrays) {
            for (const modItem of arrayMods) {
                if (!modItem || typeof modItem !== "object") {
                    continue;
                }

                const acronym = String(modItem?.acronym || "").toUpperCase();
                if (acronym) {
                    modCodes.add(acronym);
                }

                const speedChange = Number(modItem?.settings?.speed_change);
                if (Number.isFinite(speedChange) && speedChange > 0) {
                    lazerSpeedChange = speedChange;
                }

                if (acronym === "DA") {
                    const overallDifficulty = Number(modItem?.settings?.overall_difficulty);
                    if (Number.isFinite(overallDifficulty)) {
                        daOverallDifficulty = overallDifficulty;
                    }
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

    const modSignature = [
        Number(speedRate).toFixed(5),
        odFlag == null ? "none" : String(odFlag),
        cvtFlag == null ? "none" : String(cvtFlag),
        [...modCodes].sort().join("+"),
    ].join("|");

    return {
        client,
        speedRate,
        odFlag,
        cvtFlag,
        modSignature,
    };
}

export function extractCurrentSongTimeMs(data) {
    const liveTime = Number(data?.beatmap?.time?.live);
    if (Number.isFinite(liveTime)) {
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
        if (Number.isFinite(num)) {
            return num;
        }
    }

    return null;
}
