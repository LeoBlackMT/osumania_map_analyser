// modColumnRemap.js —— osu!mania RD/MR 的列变换（DOM-free，Node/浏览器/Worker 共用）。
//
// RD：复刻 Lazer ManiaModRandom 与 osu!stable mania Random 的列置换；两者
//     RNG 都来自 .NET seeded System.Random，但置换构造不同，不能互相替代。
// MR：ManiaModMirror 的 K-1-column 镜像。
//
// 本模块只做“谱面列 → 新列”的纯计算与 .osu 文本重写，不读 state、不碰 DOM。

import { xToColumn } from "./noteColumn.js";

/** .NET seeded System.Random（Knuth subtractive generator）。 */
class CSharpRandom {
    constructor(seed) {
        seed |= 0;
        const subtraction = seed === -2147483648 ? 2147483647 : Math.abs(seed);
        let mj = (161803398 - subtraction) | 0;
        this.seedArray = new Array(56).fill(0);
        this.seedArray[55] = mj;
        let mk = 1;
        let ii = 0;
        for (let i = 1; i < 55; i += 1) {
            ii += 21;
            if (ii >= 55) ii -= 55;
            this.seedArray[ii] = mk;
            mk = (mj - mk) | 0;
            if (mk < 0) mk += 2147483647;
            mj = this.seedArray[ii];
        }
        for (let k = 1; k < 5; k += 1) {
            for (let i = 1; i < 56; i += 1) {
                let n = i + 30;
                if (n >= 55) n -= 55;
                this.seedArray[i] = (this.seedArray[i] - this.seedArray[1 + n]) | 0;
                if (this.seedArray[i] < 0) this.seedArray[i] += 2147483647;
            }
        }
        this.inext = 0;
        this.inextp = 21;
    }

    next() {
        let a = this.inext + 1;
        if (a >= 56) a = 1;
        let b = this.inextp + 1;
        if (b >= 56) b = 1;
        let value = this.seedArray[a] - this.seedArray[b];
        if (value === 2147483647) value -= 1;
        if (value < 0) value += 2147483647;
        this.seedArray[a] = value;
        this.inext = a;
        this.inextp = b;
        return value;
    }

    /** 对应 .NET Random.Next(0, maxExclusive)。 */
    nextRange(maxExclusive) {
        const max = Number(maxExclusive);
        if (!Number.isInteger(max) || max <= 0) return 0;
        return Math.floor(this.next() * (1.0 / 2147483647) * max);
    }
}

/** Lazer ManiaModRandom：OrderBy(_ => rng.Next())。返回 oldColumn -> newColumn。 */
export function maniaRandomColumnMap(keyCount, seed) {
    const count = Number(keyCount);
    if (!Number.isInteger(count) || count <= 0 || count > 32) {
        return null;
    }
    const rng = new CSharpRandom(seed | 0);
    return Array.from({ length: count }, (_, column) => ({ column, key: rng.next() }))
        .sort((a, b) => a.key - b.key || a.column - b.column)
        .map((entry) => entry.column);
}

/**
 * Stable mania Random：逐个列 Random.Next(0,K) 直到抽到未使用列。
 * 6K/8K/10K 等“大于 4 且偶数键”的 dual-stage 谱面，首列不消耗随机数：
 *   SpecialStyle == Right(2) -> K-1；否则 -> 0。
 * 返回 oldColumn -> newColumn。
 */
export function maniaStableRandomColumnMap(keyCount, seed, specialStyle = 0) {
    const count = Number(keyCount);
    if (!Number.isInteger(count) || count <= 0 || count > 32) {
        return null;
    }
    const style = Number(specialStyle) || 0;
    const rng = new CSharpRandom(seed | 0);
    const used = new Set();
    const map = new Array(count);
    for (let i = 0; i < count; i += 1) {
        if (i === 0 && count > 4 && count % 2 === 0) {
            map[0] = style === 2 ? count - 1 : 0;
            used.add(map[0]);
            continue;
        }
        let next;
        do {
            next = rng.nextRange(count);
        } while (used.has(next));
        used.add(next);
        map[i] = next;
    }
    return map;
}

/** ManiaModMirror：oldColumn -> K-1-oldColumn。 */
export function mirrorColumnMap(keyCount) {
    const count = Number(keyCount);
    if (!Number.isInteger(count) || count <= 0 || count > 32) {
        return null;
    }
    return Array.from({ length: count }, (_, column) => count - 1 - column);
}

/** 复合两个 old -> new 映射：先 first，再 second。 */
export function composeColumnMaps(first, second) {
    if (!Array.isArray(first) || !Array.isArray(second) || first.length !== second.length) {
        return null;
    }
    return first.map((column) => second[column]);
}

export function isIdentityColumnMap(map) {
    return Array.isArray(map) && map.every((column, index) => column === index);
}

/** .osu 的 CircleSize；0 按 parser 约定视为 10K。 */
export function readOsuKeyCount(osuText) {
    const match = String(osuText ?? "").match(/^CircleSize\s*:\s*(\d+)/m);
    if (!match) return 0;
    const count = Number(match[1]);
    if (!Number.isInteger(count) || count < 0 || count > 32) return 0;
    return count === 0 ? 10 : count;
}

/** .osu 的 SpecialStyle；0=None / 1=Left / 2=Right。 */
export function readOsuSpecialStyle(osuText) {
    const match = String(osuText ?? "").match(/^SpecialStyle\s*:\s*(\d+)/m);
    if (!match) return 0;
    const style = Number(match[1]);
    return Number.isInteger(style) && style >= 0 && style <= 2 ? style : 0;
}

/**
 * 构建 oldColumn -> newColumn。
 * - engine: "lazer" | "stable"；缺省/未知按 lazer。
 * - seed: RD seed；null/非整数时不做 RD（仅 MR 仍可）。
 * - mirror: 是否在 RD 后镜像。
 * 没有任何变换时返回 null。
 */
export function buildColumnMap(keyCount, {
    engine = "lazer",
    seed = null,
    mirror = false,
    specialStyle = 0,
} = {}) {
    const count = Number(keyCount);
    if (!Number.isInteger(count) || count <= 0 || count > 32) return null;
    const randomSeed = seed == null || seed === "" ? NaN : Number(seed);
    const hasRandom = Number.isInteger(randomSeed);
    if (!hasRandom && !mirror) return null;

    let map = Array.from({ length: count }, (_, column) => column);
    if (hasRandom) {
        const randomMap = engine === "stable"
            ? maniaStableRandomColumnMap(count, randomSeed, specialStyle)
            : maniaRandomColumnMap(count, randomSeed);
        if (!randomMap) return null;
        map = randomMap;
    }
    if (mirror) {
        const mirrorMap = mirrorColumnMap(count);
        if (!mirrorMap) return null;
        map = composeColumnMaps(map, mirrorMap);
    }
    return isIdentityColumnMap(map) ? null : map;
}

/**
 * 重写 .osu `[HitObjects]` 每行的第一个字段 x。
 * 列映射按 `xToColumn(x,K)` 识别旧列；写回 `floor((newColumn+0.5)*512/K)`。
 * 不改动任何其它行/字段。
 */
export function remapOsuTextColumns(osuText, columnMap) {
    const text = String(osuText ?? "");
    const keyCount = readOsuKeyCount(text);
    if (!keyCount || !Array.isArray(columnMap) || columnMap.length < keyCount) {
        return { text, changed: 0, keyCount };
    }
    if (isIdentityColumnMap(columnMap)) {
        return { text, changed: 0, keyCount };
    }

    const eol = text.includes("\r\n") ? "\r\n" : "\n";
    const lines = text.split(/\r?\n/);
    let inHitObjects = false;
    let changed = 0;
    for (let i = 0; i < lines.length; i += 1) {
        const line = lines[i];
        if (line.trim() === "[HitObjects]") {
            inHitObjects = true;
            continue;
        }
        if (!inHitObjects || line.trim() === "" || line.trimStart().startsWith("//")) {
            continue;
        }
        const parts = line.split(",");
        if (parts.length < 5) continue;
        const oldColumn = xToColumn(Number.parseInt(parts[0], 10), keyCount);
        const newColumn = Number(columnMap[oldColumn]);
        if (!Number.isInteger(newColumn) || newColumn < 0 || newColumn >= keyCount) {
            continue;
        }
        const x = Math.floor(((newColumn + 0.5) * 512) / keyCount);
        const next = String(x);
        if (parts[0] !== next) {
            parts[0] = next;
            lines[i] = parts.join(",");
            changed += 1;
        }
    }
    return { text: lines.join(eol), changed, keyCount };
}

/** 便捷入口：按 transform 构建映射并重写文本。 */
export function applyColumnTransformToOsuText(osuText, transform) {
    if (!transform) return String(osuText ?? "");
    const keyCount = readOsuKeyCount(osuText);
    if (!keyCount) return String(osuText ?? "");
    const map = buildColumnMap(keyCount, {
        ...transform,
        specialStyle: transform.specialStyle ?? readOsuSpecialStyle(osuText),
    });
    if (!map) return String(osuText ?? "");
    return remapOsuTextColumns(osuText, map).text;
}

/**
 * 从页面状态字段生成列变换描述。
 * - MR 在选歌即可用；
 * - RD 只有拿到实际 seed 才应用；自定义 seed 在选歌可用，默认 null 等
 *   play 帧的 ScoreInfo.ModsJson。
 */
export function columnTransformFromState({ modCodes, randomSeed, client } = {}) {
    const mods = Array.isArray(modCodes) ? modCodes : [];
    const hasRD = mods.some((mod) => /^RD$/i.test(String(mod)));
    const hasMR = mods.some((mod) => /^MR$/i.test(String(mod)));
    if (!hasRD && !hasMR) {
        return null;
    }

    const clientText = String(client || "").toLowerCase();
    const rawSeed = randomSeed;
    const seed = rawSeed == null || rawSeed === "" ? NaN : Number(rawSeed);
    const hasSeed = Number.isInteger(seed)
        && seed >= -2147483648
        && seed <= 2147483647;

    if (hasRD && !hasSeed) {
        return hasMR ? { mirror: true } : null;
    }
    if (hasRD && clientText && clientText !== "lazer" && clientText !== "stable") {
        return hasMR ? { mirror: true } : null;
    }
    return {
        engine: clientText === "stable" ? "stable" : "lazer",
        seed: hasRD ? seed : null,
        mirror: hasMR,
    };
}
