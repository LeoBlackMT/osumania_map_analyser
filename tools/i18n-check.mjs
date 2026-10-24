#!/usr/bin/env node
// tools/i18n-check.mjs
// 检查壳内页面的中文词典覆盖率：
//   1) settings.json 的每个设置项/分组标题是否有 zh-CN 译文（描述有则查）；
//   2) 源码里 t()/tf() 与 HTML/JS 的 data-i18n 用到的键是否在词典里（缺失 = 回落英文，可接受但应记录）；
//   3) 词典里多余的键（设置项被删/改名后残留）。
// 用法：node tools/i18n-check.mjs
import fs from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";

const ROOT = path.resolve(import.meta.dirname, "..");
const PLUGIN = path.join(ROOT, "ManiaMapAnalyser by Leo_Black");
const SCAN = [
    "js/app/settingsPage/index.js",
    "js/app/settingsPage/settingsForm.js",
    "js/app/settingsPage/shellConfigPanel.js",
    "js/app/settingsPage/settingsLinks.js",
    "js/app/presets/manager.js",
    "js/app/presets/form.js",
];
const SCAN_HTML = ["settings.html"];

const settings = JSON.parse(await fs.readFile(path.join(PLUGIN, "settings.json"), "utf8"));
const dict = (await import(pathToFileURL(path.join(PLUGIN, "js/app/i18n/zh-CN.js")).href)).default;

const missing = [];
const expected = new Set();

for (const entry of settings) {
    const key = entry.uniqueID;
    if (!key || entry.type === "button") {
        continue;
    }
    const titleKey = "settings." + key + ".title";
    expected.add(titleKey);
    if (!dict[titleKey]) {
        missing.push(titleKey);
    }
    if (entry.description) {
        const descKey = "settings." + key + ".description";
        expected.add(descKey);
        if (!dict[descKey]) {
            missing.push(descKey);
        }
    }
}

const used = new Set();
const texts = [];
for (const rel of [...SCAN, ...SCAN_HTML]) {
    texts.push(await fs.readFile(path.join(PLUGIN, rel), "utf8"));
}

for (const text of texts) {
    for (const match of text.matchAll(/\b(?:t|tf)\(\s*"([^"]+)"/g)) {
        used.add(match[1]);
    }
    // data-i18n 既出现在 settings.html，也出现在 manager.js 的 innerHTML 片段里。
    for (const match of text.matchAll(/data-i18n(?:-title|-placeholder|-aria)?="([^"]+)"/g)) {
        used.add(match[1]);
    }
    for (const match of text.matchAll(/data-mma-title-key="([^"]+)"/g)) {
        used.add(match[1]);
    }
    // 语言下拉的选项键通过 labelKey 传递。
    for (const match of text.matchAll(/labelKey:\s*"([^"]+)"/g)) {
        used.add(match[1]);
    }
    if (/t\(\s*`option\./.test(text)) {
        used.add("option.<dynamic>");
    }
}

// 动态键族：源码里出现 t(`<前缀>${...}`) 时，整族都算「已使用」。
const DYNAMIC_PREFIXES = ["option.", "ui.presets.action.", "ui.shell.field.", "ui.shell.hotkey."];
const dynamicFamilies = DYNAMIC_PREFIXES.filter((prefix) => (
    texts.some((text) => text.includes("t(`" + prefix + "${"))
));
const isUsed = (key) => used.has(key) || dynamicFamilies.some((prefix) => key.startsWith(prefix));
const usedMissing = [...used].filter((key) => !key.includes("<dynamic>") && !dict[key]);
const extra = Object.keys(dict).filter((key) => !expected.has(key) && !isUsed(key));

console.log("settings entries checked: " + expected.size);
console.log("dictionary keys: " + Object.keys(dict).length);
console.log("missing for settings.json: " + missing.length);
for (const key of missing) {
    console.log("  - " + key);
}
console.log("t() keys without a translation (fall back to English): " + usedMissing.length);
for (const key of usedMissing) {
    console.log("  - " + key);
}
console.log("unused dictionary keys: " + extra.length);
for (const key of extra.slice(0, 40)) {
    console.log("  - " + key);
}
process.exitCode = missing.length === 0 && usedMissing.length === 0 ? 0 : 1;
