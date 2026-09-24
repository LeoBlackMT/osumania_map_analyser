// Malody V 判定档 + Pro + 倍率形式 → 转换器要用的 OD（`.mc → .osu`）。
//
// 表值由 `tools/malody-v-od-check/verify.py` 从窗口值复算（同一套 96% 等精度 σ* 方法学，
// 与 `tools/malody4-od-check/verify.py` 同形）；`--emit-js` 打印本文件的表体，
// `--check-js` 反查本文件的表值是否仍与复算一致（改错一个值即以非 0 退出）。
//
// ## 两条轴，不要混
// Malody V 的难度有**两个独立来源**，本文件只负责其中一个：
//   * 倍率（密度、反应时间）→ 由调用点写进管线的 `musicRate`，**不在这里**；
//   * 判定窗口（命中精度）→ 换算成等效 osu!mania OD，**就是这里**。
//
// ## 三组 Mod 与 Turbo 的区别（`MalodyV-判定窗口-6.7.22.md` §3）
// 判定器比较 `(this+44) × |delta|` 与窗口表值，其中 `this+44` 在**速度对象非空**（chart / replay
// 自带的精确调速，即 Turbo）时是 `1/playSpeed`，否则恒为 `1.0`：
//   * **Turbo**（精确调速）：窗口被补偿 ⇒ 同一倍率下窗口**更窄** ⇒ 更难；
//   * **Dash / Rush / Slow**（Mod 位变速）：`bnf::Init` 不读 Mod 位，窗口**保持原值**
//     ⇒ 与 NM 的 OD 相同，它们的难度差异全部由倍率承载。
//   所以本表里 DASH / RUSH / SLOW 三列与 NM 同值，而 TURBO 列更高 —— 这正是
//   「Turbo 1.2 与 Dash 1.2 必须可区分」的落点（两者倍率相同、OD 不同）。
//
// ## 输入约定
// `judge` 是**数值 0..4 ↔ A~E**（与游戏 `config.json` 的 `user_judge_level`、桥载荷、壳
// state 帧一致）；字母只在显示层转换。`pro === true` ⇒ 严格组，`false` ⇒ 常态组，
// **`null`/缺失 ⇒ 关闭动态 OD**（不得拿常态组冒充）。
//
// ## 输出
// `{ od, reason }`：`reason` 非空即表示"本次没有启用动态 OD"，调用点据此写状态行；
// 单一常量无法区分"Pro 未知"与"判定档取不到"两种情形，故用 `reason` 分别承载。
// OD 越界照数值返回、不夹断（仓库既有约定：`js/parser/judgeOdTable.js` 的 `OD_BOUNDS`）。

/** 转换器缺省 OD（`js/parser/mcToOsuConverter.js` 的 `CONVERT_OD`）：动态 OD 未启用时的取值。 */
const DEFAULT_OD = 9;

/** 判定档字母（显示层用；表键是数值 `0..4`）。 */
export const JUDGE_LETTERS = Object.freeze(["A", "B", "C", "D", "E"]);

/**
 * 常态组（Pro 关闭）等效 OD：5 档 × 4 倍率形式。
 * 由 `tools/malody-v-od-check/verify.py --emit-js` 生成，**不要手改**。
 */
const NORMAL_OD = Object.freeze({
    0: Object.freeze({ NM: -2.06, DASH: -2.06, RUSH: -2.06, SLOW: -2.06 }),
    1: Object.freeze({ NM: 1.38, DASH: 1.38, RUSH: 1.38, SLOW: 1.38 }),
    2: Object.freeze({ NM: 4.87, DASH: 4.87, RUSH: 4.87, SLOW: 4.87 }),
    3: Object.freeze({ NM: 7.72, DASH: 7.72, RUSH: 7.72, SLOW: 7.72 }),
    4: Object.freeze({ NM: 10.63, DASH: 10.63, RUSH: 10.63, SLOW: 10.63 }),
});

/** 严格组（Pro 开启）等效 OD。同表同源。 */
const STRICT_OD = Object.freeze({
    0: Object.freeze({ NM: 1.05, DASH: 1.05, RUSH: 1.05, SLOW: 1.05 }),
    1: Object.freeze({ NM: 4.52, DASH: 4.52, RUSH: 4.52, SLOW: 4.52 }),
    2: Object.freeze({ NM: 8.08, DASH: 8.08, RUSH: 8.08, SLOW: 8.08 }),
    3: Object.freeze({ NM: 11.00, DASH: 11.00, RUSH: 11.00, SLOW: 11.00 }),
    4: Object.freeze({ NM: 13.96, DASH: 13.96, RUSH: 13.96, SLOW: 13.96 }),
});

/**
 * Turbo 的等效 OD（窗口被 `1/playSpeed` 补偿）。倍率不同 ⇒ 压缩量不同，
 * 故与上两表分开列；倍率不属于本文件职责，这里只存"该倍率下 Turbo 的 OD"。
 */
const NORMAL_TURBO_OD = Object.freeze({ 1.2: 7.75 });
const STRICT_TURBO_OD = Object.freeze({ 1.2: 10.36 });

/** 倍率与名义值的匹配容差（与壳侧 `RATE_TOLERANCE` 同口径）。 */
const RATE_TOLERANCE = 0.005;

/** 动态 OD 关闭的原因文案（调用点据此写状态行，绝不静默降级）。 */
export const OD_DISABLED_REASON = Object.freeze({
    proUnknown: "Pro 状态未知（插件未上报）——请打开一次 JUDGE 面板",
    judgeUnknown: "判定档取不到",
});

/** 倍率是否命中某个名义值，命中则返回对应的表键。 */
function rateKey(speedRate) {
    for (const [key, nominal] of [["DASH", 1.2], ["RUSH", 1.5], ["SLOW", 0.8]]) {
        if (Math.abs(speedRate - nominal) <= RATE_TOLERANCE) return key;
    }
    return "NM";
}

/**
 * 判定档 + Pro + 倍率形式 ⇒ 转换器 OD 与关闭原因。
 *
 * @param {number|null|undefined} judge 判定档数值 0..4（0=A … 4=E）；null/越界 = 未采集到
 * @param {boolean|null|undefined} pro `true` = 严格组，`false` = 常态组，null/缺失 = 未知
 * @param {number|null|undefined} speedRate 当局真实倍率（取 Mod 表键；Turbo 另用于取补偿后的 OD）
 * @param {boolean|null|undefined} turbo 是否 Turbo（精确调速）；true ⇒ 窗口被 `1/playSpeed` 补偿
 * @returns {{od: number, reason: string|null}} `reason` 非空 = 本次关闭了动态 OD
 */
export function resolveOd(judge, pro, speedRate, turbo) {
    // 判定档：非整数 / 越界 = 未采集到（`Number(null)` 会静默变成 0 = A 档，必须显式挡掉）。
    const judgeNumber = Number.isInteger(judge) && judge >= 0 && judge <= 4 ? judge : null;
    if (judgeNumber === null) {
        return { od: DEFAULT_OD, reason: OD_DISABLED_REASON.judgeUnknown };
    }
    // Pro 未知 ⇒ 关闭动态 OD，**不得按常态组冒充**（那正是"静默用错组"）。
    if (pro !== true && pro !== false) {
        return { od: DEFAULT_OD, reason: OD_DISABLED_REASON.proUnknown };
    }
    const strict = pro === true;

    if (turbo === true) {
        const table = strict ? STRICT_TURBO_OD : NORMAL_TURBO_OD;
        const rate = Number(speedRate);
        for (const [key, od] of Object.entries(table)) {
            if (Math.abs(rate - Number(key)) <= RATE_TOLERANCE) return { od, reason: null };
        }
        // 该倍率下的 Turbo 补偿量未建表 ⇒ 关闭而不是拿别的倍率的值冒充。
        return { od: DEFAULT_OD, reason: OD_DISABLED_REASON.judgeUnknown };
    }

    const table = strict ? STRICT_OD : NORMAL_OD;
    return { od: table[judgeNumber][rateKey(Number(speedRate))], reason: null };
}
