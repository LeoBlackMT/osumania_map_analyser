// 壳内卡片主体的迷你 Tab 胶囊栏。
//
// 只做两件事：把静态按钮与点击回调绑定；按当前状态同步可见性/激活态。
// 交互刻意收窄为 click（左键）——不挂 wheel/touch/键盘/拖拽，避免在游戏或选歌时误触。

import { contentBarTabsEl, getActiveContentBar, state } from "./appContext.js";
import { isShellRuntime } from "./feedback/feedbackSpec.js";

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
        if (typeof onSelect === "function") {
            onSelect(button.dataset.contentTab);
        }
    });
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
        return;
    }

    const active = getActiveContentBar();
    for (const button of contentBarTabsEl.querySelectorAll("[data-content-tab]")) {
        const isActive = button.dataset.contentTab === active;
        button.classList.toggle("is-active", isActive);
        button.setAttribute("aria-pressed", isActive ? "true" : "false");
    }
}
