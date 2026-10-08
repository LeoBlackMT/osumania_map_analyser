// 壳原生端点上"抓谱面文件失败"的**静默重试**策略（Step 9g）。
//
// 背景（用户报的那条）：游玩结束进入结算界面时，卡片会显示 `Request failed with 404`，
// 约 2 s 后触发重算又恢复正常。404 来自壳的 24062（读线程正处于**身份保持/停帧**窗口，
// `/files/beatmap/file` 没有可供奉的当前谱面），而页面把状态码原样渲染上了状态行。
//
// 这里只做一件事：对**壳原生端点**的瞬态失败重试**一次**（约 1 s）。理由：
// 该端点在这一刻没有数据 ≠ 这张谱面的数据不存在——读线程几十秒内就会恢复
// （`IDENTITY_HOLD_GRACE` / `RECOVERY_CLEAN_FRAMES` 两条窗口都在秒级）。
//
// **tosu 通道（无壳 / `mode:"tosu"`）逐字节不变**：`state.runtimeOsuHost` 未生效时
// `shouldRetryNativeBeatmapFetch` 恒为 `false` ⇒ 不重试、错误照旧原样抛出。
//
// 模块是 DOM 无关的（可在 Node 里用桩测试；`osuScanHintRules.js` 的同一先例）。

/** 静默重试的间隔（毫秒）。 */
export const NATIVE_FETCH_RETRY_DELAY_MS = 1000;

/**
 * 值得重试的 HTTP 状态闭集：
 * - `404`：读线程没有"当前谱面"（未附着 / 身份保持 / 停帧）——本策略要治的就是它；
 * - `5xx`：读盘/服务端瞬态（例如 `.osu` 正在被替换）。
 * 明确**不**重试：`403`（Host 门禁）、`405`（方法）——那是语义错误，重试只是白等。
 */
export const NATIVE_FETCH_RETRY_STATUSES = Object.freeze([404, 500, 502, 503, 504]);

/**
 * 纯判据：这一次失败值得静默重试吗？
 *
 * @param {{native: boolean, ok: boolean, status?: number}} input
 *   `native` = 本次请求打的是壳原生端点（`isRuntimeOsuOverrideActive()`）；
 *   `ok` = `response.ok`；`status` = `response.status`。
 * @returns {boolean}
 */
export function shouldRetryNativeBeatmapFetch({ native, ok, status }) {
    if (!native || ok) {
        return false;
    }
    return NATIVE_FETCH_RETRY_STATUSES.includes(Number(status));
}

/**
 * 抓一次谱面文本的 `Response`；需要时静默重试一次（间隔 [`NATIVE_FETCH_RETRY_DELAY_MS`]）。
 *
 * 只负责"拿到 Response"：`ok` 判定与错误文案仍由调用方（`analysis.js`）负责，
 * 因此重试耗尽后**错误路径与改动前逐字相同**。
 *
 * @param {string} url 谱面文件端点
 * @param {{native: boolean, isStale: () => boolean}} options
 *   `isStale` = 调用方的请求序号守卫（重试前后各判一次：被取代的请求不重试、不返回）。
 * @param {{fetchImpl?: Function, sleep?: Function}} [deps] 测试缝（默认 = 全局 `fetch` + `setTimeout`）
 * @returns {Promise<Response|null>} `null` = 请求已被取代（stale）
 */
export async function fetchBeatmapTextWithRetry(url, { native, isStale }, deps = {}) {
    const request = deps.fetchImpl
        || ((input) => fetch(input, { method: "GET", cache: "no-store" }));
    const sleep = deps.sleep
        || ((ms) => new Promise((resolve) => setTimeout(resolve, ms)));
    let response = null;
    try {
        response = await request(url);
    } catch (err) {
        if (!native) {
            throw err;
        }
        await sleep(NATIVE_FETCH_RETRY_DELAY_MS);
        if (isStale()) {
            return null;
        }
        response = await request(url);
    }
    if (isStale()) {
        return null;
    }
    if (shouldRetryNativeBeatmapFetch({
        native,
        ok: Boolean(response && response.ok),
        status: response && response.status,
    })) {
        await sleep(NATIVE_FETCH_RETRY_DELAY_MS);
        if (isStale()) {
            return null;
        }
        response = await request(url);
        if (isStale()) {
            return null;
        }
    }
    return response;
}