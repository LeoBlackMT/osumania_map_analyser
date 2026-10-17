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

/** 结算门槛。 */
export const ACC_MIN = 92;
export const ACC_MAX = 99;
export const TOTAL_HITS_MIN = 200;
export const COMPLETION_MIN = 0.85;
export const MISS_RATE_MAX = 0.05;

/** weight 曲线：w = floor + (1-floor) * exp(-(acc-center)^2 / (2 sigma^2))。 */
export const WEIGHT_VERSION = 1;
export const WEIGHT_CENTER = 96.5;
export const WEIGHT_SIGMA = 2.0;
export const WEIGHT_FLOOR = 0.35;
export const WEIGHT_MIN = 0.4;
export const WEIGHT_MAX = 1.0;

/** mod 白名单（NM 用空集合表示）。其余全部视为不允许。 */
export const ALLOWED_MOD_CODES = Object.freeze(["MR", "SD", "PF", "SV2", "TD", "CS"]);

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
