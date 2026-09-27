/**
 * External link opening shared by the settings page and the presets panel.
 *
 * Inside the desktop shell's Tauri webview a plain `<a target="_blank">` click is
 * swallowed (no system browser, no new window), so every external link goes
 * through here:
 *   1. `window.open(url, "_blank", "noopener")` — the normal browser path;
 *   2. when that returns null/undefined (the webview blocked it) the URL is
 *      copied to the clipboard instead, and the caller shows
 *      `LINK_COPIED_NOTICE` to the user.
 *
 * The page itself is never navigated away and nothing throws: a missing or
 * denied clipboard API leaves the URL selected/copyable by hand.
 */

export const LINK_COPIED_NOTICE = "Link copied — open it in your browser.";

/**
 * Opens one link outside the page.
 * @param {string} url absolute URL.
 * @returns {Promise<boolean>} true = a window was opened; false = copied instead
 *          (or the URL was empty).
 */
export async function openExternalLink(url) {
    const target = String(url ?? "").trim();
    if (!target) {
        return false;
    }
    let opened = null;
    try {
        if (typeof window !== "undefined" && typeof window.open === "function") {
            opened = window.open(target, "_blank", "noopener");
        }
    } catch {
        opened = null; // blocked by the webview — fall through to the clipboard
    }
    if (opened) {
        return true;
    }
    await copyToClipboard(target);
    return false;
}

/** Best-effort clipboard write; every failure is silent by design. */
async function copyToClipboard(text) {
    try {
        if (typeof navigator !== "undefined"
            && navigator.clipboard
            && typeof navigator.clipboard.writeText === "function") {
            await navigator.clipboard.writeText(text);
        }
    } catch {
        // Clipboard API unavailable/denied: the notice still tells the user what to do.
    }
}
