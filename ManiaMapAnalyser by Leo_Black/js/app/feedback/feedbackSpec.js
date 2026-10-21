// RC 难度反馈的常量与判定阈值（纯数据，Node 可测）。
//
// 只服务桌面壳页面：浏览器 24050 页 / OBS 源没有壳标记，永不加载反馈条。

/** 壳在导航 URL 里追加的 hash 标记；页面据此确认"我在壳窗口里"。 */
export const SHELL_MARKER = "mma-shell";

/** 五档投票值：负数 = 比显示值简单，正数 = 比显示值难。 */
export const FEEDBACK_VOTES = Object.freeze([-1, -0.5, 0, 0.5, 1]);

/** RC-only：LN 占比上限与不允许的模式标签。 */
export const RC_LN_MAX = 0.15;
export const NON_RC_MODE_TAGS = Object.freeze(["LN", "Mix", "HB"]);

/** 6K/7K 的反馈范围：所有算法支持的类型都可以问，4K 仍只走 RC。 */
export const SUPPORTED_FEEDBACK_KEYS = Object.freeze([4, 6, 7]);
export const ALLOWED_MODE_TAGS_6K7K = Object.freeze(["RC", "LN", "Mix", "HB", "SV"]);
export const ALLOWED_TARGET_SIDES = Object.freeze(["RC", "LN"]);

/** 结算门槛。 */
export const ACC_MIN = 90;
export const ACC_MAX = 100;
export const TOTAL_HITS_MIN = 200;
export const COMPLETION_MIN = 0.85;

/** weight 曲线（v3）：峰值在 acc=96，左右不对称的高斯。
 *  w = 0.30 + 0.70 * exp(-0.5*((acc-96)/σ)^2)，σ = 3.2（acc≤96）/ 4.0（acc>96）。
 *  低 acc 快速下降（90% ≈ 0.42），96% = 1.0，100% 回落到 ≈0.73。 */
export const WEIGHT_VERSION = 3;
export const WEIGHT_BASE = 0.3;
export const WEIGHT_SPAN = 0.7;
export const WEIGHT_PEAK = 96;
export const WEIGHT_SIGMA_LEFT = 3.2;
export const WEIGHT_SIGMA_RIGHT = 4.0;
export const WEIGHT_MIN = 0.4;
export const WEIGHT_MAX = 1.0;

/** mod 白名单（NM 用空集合表示）。其余全部视为不允许。 */
export const ALLOWED_MOD_CODES = Object.freeze([
    "HD", "FI", "FL", "DT", "NC", "HT", "DC",
    "MR", "SD", "PF", "SV2", "TD", "CS",
]);

/** 允许的倍速：NM/MR 类 1.0，DT/NC 1.5，HT/DC 0.75。 */
export const ALLOWED_RATES = Object.freeze([0.75, 1.0, 1.5]);

/** 本地去重：同一谱面同一倍速 30 天只问一次。 */
export const DEDUP_KEY_PREFIX = "mma.feedback.v1.";
export const DEDUP_TTL_MS = 30 * 24 * 60 * 60 * 1000;

/** 反馈条时序。 */
export const STRIP_DELAY_MS = 1000;
export const STRIP_TIMEOUT_MS = 12000;
export const STRIP_THANKS_MS = 800;

/** 后端接收端；本地联调可用 window.__MMA_FEEDBACK_ENDPOINT 覆盖。 */
export const DEFAULT_FEEDBACK_ENDPOINT = "https://mma-stats.leoblack.top/api/v1/feedback";

export function feedbackEndpoint() {
    const override = typeof window !== "undefined" ? window.__MMA_FEEDBACK_ENDPOINT : null;
    return typeof override === "string" && override.trim().length > 0
        ? override.trim()
        : DEFAULT_FEEDBACK_ENDPOINT;
}

/** 壳窗口判定：端口 24061，或壳在导航 URL 上追加的 hash 标记（tosu 在线时页面由 tosu 提供）。 */
export function isShellRuntime() {
    if (typeof window === "undefined" || !window.location) return false;
    if (String(window.location.port) === "24061") return true;
    return String(window.location.hash || "").replace(/^#/, "") === SHELL_MARKER;
}
