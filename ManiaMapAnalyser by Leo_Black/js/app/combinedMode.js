// 组合模式（Combined）的纯配置模型。
//
// 只被壳配置与设置页使用；不碰 DOM、不 import 其他模块。模块 token 复用
// contentBar 的规范值，数组顺序就是卡片上的显示顺序。enabled 为真时至少保留
// 一个模块；空数组/缺键一律回落默认四模块，避免与 contentBar=None 的语义重复。

export const COMBINED_MODE_MODULES = Object.freeze(["Pattern", "Etterna", "Graph", "ReworkPP"]);

export const COMBINED_MODE_LABELS = Object.freeze({
    Pattern: "Pattern",
    Etterna: "Etterna",
    Graph: "Graph",
    ReworkPP: "PP",
});

export function isCombinedModeModule(value) {
    if (typeof value !== "string") {
        return false;
    }
    const lowered = value.trim().toLowerCase();
    return COMBINED_MODE_MODULES.some((module) => module.toLowerCase() === lowered);
}

/**
 * 归一化 `mma-shell-config.json` 的 combinedMode 值。
 *
 * - `enabled` 只认严格的 `true`；
 * - `modules` 大小写归一、去重、保序、丢弃非法项；
 * - 缺失/非数组/归一化后为空 → 默认四模块（至少一个模块）。
 *
 * @param {unknown} value
 * @returns {{ enabled: boolean, modules: string[] }}
 */
export function normalizeCombinedMode(value) {
    const source = value && typeof value === "object" && !Array.isArray(value) ? value : {};
    const enabled = source.enabled === true;
    const modules = [];

    if (Array.isArray(source.modules)) {
        for (const item of source.modules) {
            if (typeof item !== "string") {
                continue;
            }
            const lowered = item.trim().toLowerCase();
            const matched = COMBINED_MODE_MODULES.find((module) => module.toLowerCase() === lowered);
            if (matched && !modules.includes(matched)) {
                modules.push(matched);
            }
        }
    }

    return {
        enabled,
        modules: modules.length > 0 ? modules : [...COMBINED_MODE_MODULES],
    };
}
