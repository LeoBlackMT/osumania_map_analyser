// 壳配置（mma-shell-config.json）的页面侧读取器。
//
// 只服务壳窗口（24061 或 24050#mma-shell）：
//   - 启动时 GET 24061/shell-config 拿初始 combinedMode；
//   - 运行中壳把配置变更作为现有 `settings` 帧广播（payload 就是壳配置全文），
//     由 isShellConfigPayload() 辨别后直接应用，不额外轮询。
// 浏览器/OBS 页面不会调用这里（调用方先判 isShellRuntime）。

import { isShellRuntime } from "./feedback/feedbackSpec.js";

export const SHELL_CONFIG_URL = "http://127.0.0.1:24061/shell-config";

/**
 * GET /shell-config → config 对象；失败/超时/非 2xx/形状不对 → null。
 * 壳未启动时静默失败，页面保持默认 compact 行为。
 *
 * @param {number} [timeoutMs]
 * @returns {Promise<object|null>}
 */
export async function fetchShellConfig(timeoutMs = 1200) {
    if (!isShellRuntime()) {
        return null;
    }

    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), timeoutMs);
    try {
        const response = await fetch(SHELL_CONFIG_URL, {
            cache: "no-store",
            signal: controller.signal,
        });
        if (!response.ok) {
            return null;
        }
        const body = await response.json();
        return body && typeof body.config === "object" && body.config !== null
            ? body.config
            : null;
    } catch {
        return null;
    } finally {
        clearTimeout(timer);
    }
}

/**
 * `settings` 帧 payload 是否是壳配置（而不是插件设置）。
 *
 * 壳配置始终带 combinedMode，并至少带 hotkeys/gameClient/logLevel 之一；
 * 插件设置是 uniqueID→value 的对象或数组，不会命中这些键。
 *
 * @param {unknown} payload
 * @returns {boolean}
 */
export function isShellConfigPayload(payload) {
    if (!payload || typeof payload !== "object" || Array.isArray(payload)) {
        return false;
    }
    const hasCombinedMode = payload.combinedMode
        && typeof payload.combinedMode === "object"
        && !Array.isArray(payload.combinedMode);
    const hasShellMarker = ["hotkeys", "gameClient", "logLevel"].some((key) =>
        Object.prototype.hasOwnProperty.call(payload, key),
    );
    return Boolean(hasCombinedMode && hasShellMarker);
}

/**
 * 把一个壳配置 patch POST 到 /shell-config（预设应用 combinedMode 时持久化用）。
 * fire-and-forget：失败只返回 false，不抛错——运行时效果已由
 * applyCombinedModeSetting 生效，落盘失败不该打断预设应用。
 * 非壳环境直接 false；24050#mma-shell 跨源 POST 无 CORS 时同样静默失败。
 *
 * @param {object} patch
 * @returns {Promise<boolean>}
 */
export async function postShellConfigPatch(patch) {
    if (!isShellRuntime() || !patch || typeof patch !== "object") {
        return false;
    }
    try {
        const response = await fetch(SHELL_CONFIG_URL, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(patch),
            cache: "no-store",
        });
        return response.ok;
    } catch {
        return false;
    }
}
