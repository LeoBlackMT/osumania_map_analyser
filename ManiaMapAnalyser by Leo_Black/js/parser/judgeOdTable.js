// Malody 4.3.7（PC 端）判定档 × 速率 → 等效 osu!mania OD 表。
//
// 数据来源：PC 端 malody.exe 4.3.7 反汇编出的 Key 判定窗口，经「96% 准确率等精度
// （σ*）」求解得到的等效 OD——窗口数值、方法与复算校验见
// `tools/malody4-od-check/verify.py`（本文件不复制任何窗口数值，避免两处表）。
// 方法学与越界理由见 `.omo/plans/malody4-native-source.md` §2 #23/#24/#24b/#25。
//
// 注意：Malody V（6.7.x）是另一个客户端、用的是更宽的一套判定窗口，
// 因此**不能共用本表**（要用它的窗口值另算一张）。
//
// 表驱动：不做插值、不含任何拟合系数。误差分布或准确率口径一旦改动，整表重算。

const RATE_EPS = 1e-5;   // 速率容差（与壳侧 speedRate 匹配同口径）
const JUDGE_LETTERS = ["A", "B", "C", "D", "E"];
const FALLBACK_JUDGE = "C";   // 判定缺失/非法时回落标准档 C（旧写死值 9 的邻近档）
const FALLBACK_RATE = "NM";

// 表值（列序恒为 NM / DASH 1.2 / RUSH 1.5 / SLOW 0.8）：
//        NM      DASH    RUSH     SLOW
export const JUDGE_OD = Object.freeze({
    A: Object.freeze({ NM: 1.05, DASH: 4.66, RUSH: 8.16, SLOW: -4.56 }),
    B: Object.freeze({ NM: 4.52, DASH: 7.47, RUSH: 10.33, SLOW: -0.06 }),
    C: Object.freeze({ NM: 8.08, DASH: 10.36, RUSH: 12.58, SLOW: 4.56 }),
    D: Object.freeze({ NM: 11.00, DASH: 12.74, RUSH: 14.47, SLOW: 8.33 }),
    E: Object.freeze({ NM: 13.96, DASH: 15.19, RUSH: 16.42, SLOW: 12.10 }),
});

// 越界边界：负端不夹断；上界 21.3 是因为 Sunny 族用
// 0.3*sqrt((64.5 - ceil(3*od))/500) 且无定义域保护——od > 21.3̄ 时根号内为负 → NaN，
// 写成 21.4 会把边界本身放进 NaN 区。
export const OD_BOUNDS = Object.freeze({ lo: -5, hi: 21.3 });

// 速率 → 列名：非 1.2/1.5/0.8（含 1.0、null、NaN）一律视为 NM。
function resolveRateName(speedRate) {
    const rate = Number(speedRate);
    if (Math.abs(rate - 1.2) < RATE_EPS) {
        return "DASH";
    }
    if (Math.abs(rate - 1.5) < RATE_EPS) {
        return "RUSH";
    }
    if (Math.abs(rate - 0.8) < RATE_EPS) {
        return "SLOW";
    }
    return FALLBACK_RATE;
}

/**
 * 判定档 + 速率 → 等效 OD。
 * @param {string} judgeLetter 判定档字母 A~E（大小写不敏感；缺失/非法回落 C）
 * @param {number} speedRate 速率（1.2=DASH / 1.5=RUSH / 0.8=SLOW，其余=NM）
 * @returns {{od: number, judge: string, rateName: string, known: boolean}}
 *          known 仅在判定字母被识别时为 true
 */
export function computeOd(judgeLetter, speedRate) {
    const letter = typeof judgeLetter === "string" ? judgeLetter.trim().toUpperCase() : "";
    const known = JUDGE_LETTERS.includes(letter);
    const judge = known ? letter : FALLBACK_JUDGE;
    const rateName = resolveRateName(speedRate);
    return { od: JUDGE_OD[judge][rateName], judge, rateName, known };
}
