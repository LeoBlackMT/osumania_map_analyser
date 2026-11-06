// js/estimator/proj7kEstimator.js
// Proj7k —— proj7k 7K 难度引擎的估算器（共享 DOM-free 纯函数模块；Node 与浏览器同一实现）。
//
// 定位：独立的 7K 物理模型（手指耦合 + 手部负荷 + 阈值求解），不依赖 Sunny 的星数。
//   引擎本体见 proj7k/engine.js（与上游 Python 引擎逐位对齐，120 张 JinJin Dan 标杆谱最大差 4e-15 星）；
//   星数 → 段位：上游 dan_table.json 的 15 档 JinJin Dan 段位星数（4 个 RC 技法池的档位中位数），
//   相邻两档之间在 ln(star) 上线性插值得到连续 level（0 = Regular 0 … 14 = Regular Stellium），
//   再按 Polaris 同一套 tier 偏移（low −0.4 … high +0.4）取子档标签。
//
// 作用域：**7K 专用**。其他键数与 cvt（IN/HO）改图旗标回退 Sunny（actualEstimatorAlgorithm = "Sunny"）。
//   - star：保留 Sunny 原始 sr（星数胶囊口径不变，同 Polaris）；proj7k 自己的星数在 debug.proj7kStars。
//   - estDiff：单一 Regular 标签。引擎只有一条段位刻度（LN 技法池与 RC 共用同一阶梯，且设计上有意把
//     LN 读得比 RC 低），因此不另给 LN 半标签。
//   - numericDifficulty：连续 level（表范围 [−0.5, 14.5] 外为 null，标签用 `<` / `>` 边界语义）。
// 上游：https://github.com/ka1z07/proj7k，算法文档见 docs/proj7k_algorithm.md。

import { runSunnyEstimatorFromText } from "./sunnyEstimator.js";
import { OsuFileParser } from "../parser/osuFileParser.js";
import { evaluateNotes, SKILLS } from "./proj7k/engine.js";

// 上游 src/proj7k/dan_table.json（引擎 915e5af；ADR-0056 起技法层重做，但总星数与段位表未变）
export const PROJ7K_DAN_STARS = Object.freeze([
    ["0", 3.46],
    ["1", 3.72],
    ["2", 4.4],
    ["3", 4.76],
    ["4", 5.25],
    ["5", 5.64],
    ["6", 5.99],
    ["7", 6.28],
    ["8", 6.87],
    ["9", 7.35],
    ["10", 7.76],
    ["Gamma", 8.63],
    ["Azimuth", 9.64],
    ["Zenith", 10.88],
    ["Stellium", 12.28],
]);

const LEVEL_MIN = -0.5;
const LEVEL_MAX = PROJ7K_DAN_STARS.length - 1 + 0.5;

/** star → 连续 level：相邻两档在 ln(star) 上线性插值，两端按首/末段斜率外推。 */
export function proj7kStarToLevel(star) {
    const s = Number(star);
    if (!(s > 0)) return -Infinity;
    const x = Math.log(s);
    const xs = PROJ7K_DAN_STARS.map(([, v]) => Math.log(v));
    const last = xs.length - 1;
    let i = 0;
    if (x >= xs[last]) i = last - 1;
    else if (x > xs[0]) while (xs[i + 1] <= x) i += 1;
    return i + (x - xs[i]) / (xs[i + 1] - xs[i]);
}

function tierOf(offset) {
    if (offset < -0.3) return "low";
    if (offset < -0.1) return "mid/low";
    if (offset <= 0.1) return "mid";
    if (offset <= 0.3) return "mid/high";
    return "high";
}

/** 连续 level → "Regular N tier"（表外为 "< Regular 0 low" / "> Regular Stellium high"）。 */
export function proj7kLevelToLabel(level) {
    const names = PROJ7K_DAN_STARS.map(([name]) => name);
    if (!(level >= LEVEL_MIN)) return `< Regular ${names[0]} low`;
    if (level > LEVEL_MAX) return `> Regular ${names[names.length - 1]} high`;
    const n = Math.min(names.length - 1, Math.max(0, Math.round(level)));
    return `Regular ${names[n]} ${tierOf(level - n)}`;
}

function notesFromParsed(data, rate) {
    const notes = [];
    const { columns, noteStarts, noteEnds, noteTypes } = data;
    for (let i = 0; i < columns.length; i += 1) {
        const head = noteStarts[i] / rate / 1000;
        const isLn = (noteTypes[i] & 128) !== 0;
        const tail = isLn ? noteEnds[i] / rate / 1000 : null;
        notes.push([columns[i], head, tail]);
    }
    return notes;
}

const round = (x, digits) => (Number.isFinite(x) ? Number(x.toFixed(digits)) : null);

/**
 * 运行 Proj7k 估算器。
 *
 * @param {string} osuText  .osu 文本
 * @param {object} options  与其他估算器同构（speedRate/odFlag/cvtFlag/withGraph/…），可选 precomputedSunnyResult
 * @param {object|null} parsed  可选的已 process() OsuFileParser 实例
 * @returns {object} Sunny 结果 + proj7k 的 estDiff / numericDifficulty；
 *   `actualEstimatorAlgorithm` 为 "Proj7k"（7K 生效）或 "Sunny"（回退）
 */
export function runProj7kEstimatorFromText(osuText, options = {}, parsed = null) {
    const sunny = options.precomputedSunnyResult || runSunnyEstimatorFromText(osuText, options, parsed);
    const fallback = () => ({ ...sunny, actualEstimatorAlgorithm: "Sunny" });

    const cvtText = String(options.cvtFlag ?? "").toUpperCase();
    if (cvtText.includes("IN") || cvtText.includes("HO")) return fallback();

    let parser = parsed;
    if (!parser || typeof parser.getParsedData !== "function") {
        parser = new OsuFileParser(osuText);
        parser.process();
    }
    const data = parser.getParsedData();
    if (Number(data.columnCount) !== 7 || !data.columns || data.columns.length === 0) return fallback();

    const rate = Number(options.speedRate ?? 1.0) || 1.0;
    let profile;
    try {
        profile = evaluateNotes(notesFromParsed(data, rate));
    } catch {
        return fallback();
    }
    if (!Number.isFinite(profile.totalStars)) return fallback();

    const level = proj7kStarToLevel(profile.totalStars);
    const inRange = level >= LEVEL_MIN && level <= LEVEL_MAX;
    const skills = {};
    for (const k of SKILLS) {
        const s = profile.skills[k];
        skills[k] = { stars: round(s.stars, 3), level: round(s.level, 3), coverage: round(s.coverage, 4), dominance: round(s.dominance, 4) };
    }

    return {
        ...sunny,
        estDiff: proj7kLevelToLabel(level),
        numericDifficulty: inRange ? round(level, 2) : null,
        numericDifficultyHint: null,
        actualEstimatorAlgorithm: "Proj7k",
        debug: {
            rate,
            proj7kStars: round(profile.totalStars, 4),
            proj7kD: round(profile.totalD, 4),
            level: Number.isFinite(level) ? round(level, 4) : null,
            thumbHand: profile.thumbHand,
            dominantSkill: profile.dominantSkill,
            dominanceMargin: round(profile.dominanceMargin, 4),
            skills,
        },
    };
}
