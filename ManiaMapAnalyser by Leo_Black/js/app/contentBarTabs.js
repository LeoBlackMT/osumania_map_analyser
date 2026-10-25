// 壳内卡片主体的迷你 Tab 胶囊栏。
//
// 只做三件事：把静态按钮与点击回调绑定；按当前状态同步可见性/激活态；
// 卡片闲置（无 hover/操作）超过 TAB_IDLE_FADE_MS 后把胶囊栏淡化至隐藏，
// 任何指针/焦点活动都会立即恢复。交互刻意收窄为 click（左键）——不挂
// wheel/touch/键盘/拖拽，避免在游戏或选歌时误触。

import { contentBarTabsEl, getActiveContentBar, mainCardEl, state } from "./appContext.js";
import { isShellRuntime } from "./feedback/feedbackSpec.js";

/** 卡片无 hover/操作多久后淡化胶囊栏。 */
const TAB_IDLE_FADE_MS = 3000;

let idleFadeTimerId = 0;

function clearIdleFadeTimer() {
    if (idleFadeTimerId) {
        clearTimeout(idleFadeTimerId);
        idleFadeTimerId = 0;
    }
}

function idleFadeSchedule() {
    clearIdleFadeTimer();
    idleFadeTimerId = setTimeout(() => {
        idleFadeTimerId = 0;
        if (contentBarTabsEl && !contentBarTabsEl.hidden) {
            contentBarTabsEl.classList.add("is-idle");
        }
    }, TAB_IDLE_FADE_MS);
}

/** 胶囊栏可见时的活动信号：立即恢复不透明，并重新起闲置计时。 */
function markTabsActivity() {
    if (!contentBarTabsEl || contentBarTabsEl.hidden) {
        return;
    }
    contentBarTabsEl.classList.remove("is-idle");
    idleFadeSchedule();
}

/**
 * @param {{ onSelect?: (module: string) => void }} [options]
 */
export function initContentBarTabs({ onSelect } = {}) {
    if (!contentBarTabsEl || contentBarTabsEl.dataset.bound === "1") {
        return;
    }
    contentBarTabsEl.dataset.bound = "1";
    contentBarTabsEl.addEventListener("click", (event) => {
        if (event.button !== 0) {
            return;
        }
        const button = event.target.closest("[data-content-tab]");
        if (!button) {
            return;
        }
        markTabsActivity();
        if (typeof onSelect === "function") {
            onSelect(button.dataset.contentTab);
        }
    });
    if (!isShellRuntime()) {
        return;
    }
    // 闲置淡化的活动信号：卡片上的任何指针活动 + Tab 栏获得键盘焦点。
    // 只在壳内挂（胶囊栏也只在壳内可见），浏览器/OBS 页面零监听。
    if (mainCardEl) {
        for (const type of ["pointerenter", "pointermove", "pointerdown"]) {
            mainCardEl.addEventListener(type, markTabsActivity, { passive: true });
        }
    }
    contentBarTabsEl.addEventListener("focusin", markTabsActivity);
}

/** 同步 Tab 栏可见性与激活态；非壳/Combined/None 时隐藏。 */
export function syncContentBarTabs() {
    if (!contentBarTabsEl) {
        return;
    }

    const visible = isShellRuntime()
        && !state.combinedModeEnabled
        && getActiveContentBar() !== "None";
    contentBarTabsEl.hidden = !visible;
    if (!visible) {
        clearIdleFadeTimer();
        contentBarTabsEl.classList.remove("is-idle");
        return;
    }
    idleFadeSchedule();

    const active = getActiveContentBar();
    for (const button of contentBarTabsEl.querySelectorAll("[data-content-tab]")) {
        const isActive = button.dataset.contentTab === active;
        button.classList.toggle("is-active", isActive);
        button.setAttribute("aria-pressed", isActive ? "true" : "false");
    }
}
