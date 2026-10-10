/**
 * `settings.json` `type: "button"` entries as working links (settings.html).
 *
 * tosu renders these entries as buttons in its own settings dashboard; this page
 * used to drop them (settingsForm.js only renders keys with an applier, so every
 * button was skipped) and the Guide / Preset guide / Issue / Benchmark links
 * were lost. They are links, not settings, so they live in a compact row of
 * their own instead of inside the form.
 *
 * Clicking never navigates this window: the click is handled here and the URL is
 * opened through `openExternalLink()` (see ../externalLink.js), which falls back
 * to the clipboard + `onCopied()` when the shell webview blocks `window.open`.
 */

import { openExternalLink } from "../externalLink.js";
import { t, translateEntry } from "../i18n/index.js";

/**
 * Link ids this page does not offer (the sibling exclusion of `EXCLUDED_KEYS`
 * in settingsForm.js, for the button row instead of the form): both targets are
 * tosu-hosted pages under `http://localhost:24050/ManiaMapAnalyser.by.Leo_Black/`
 * that do not exist while the page is served by the desktop shell, so the links
 * could only ever land on a dead address. `settings.json` keeps all six entries
 * for tosu's own settings dashboard.
 *
 * Nothing is lost by dropping `PresetButton`: while tosu is online the read-only
 * notice still points at the tosu Presets page (settingsPage/index.js
 * `presetsPageUrl()`, which reads that same entry).
 */
const EXCLUDED_LINK_IDS = new Set(["PresetButton", "DebugButton"]);

/**
 * @param {object} options
 * @param {HTMLElement} options.root container the links row is rendered into.
 * @param {Array<object>} options.entries settings.json entries.
 * @param {(entry: object) => string|null} options.urlFor entry → URL (the page
 *        rewrites PresetButton's host:port to the configured wsEndpoint).
 * @param {() => void} [options.onCopied] called when a click fell back to the
 *        clipboard (show the shared notice).
 */
export function createSettingsLinks({ root, entries, urlFor, onCopied }) {
    function render() {
        root.textContent = "";
        const buttons = (entries || []).filter((entry) => entry && entry.type === "button"
            && entry.uniqueID && !EXCLUDED_LINK_IDS.has(entry.uniqueID));
        if (buttons.length === 0) {
            return;
        }

        const group = document.createElement("div");
        group.className = "settings-links";

        const title = document.createElement("h3");
        title.className = "settings-group-title";
        title.textContent = t("headers.hLinks", "Links");
        group.appendChild(title);

        const row = document.createElement("div");
        row.className = "settings-links-row";
        for (const entry of buttons) {
            row.appendChild(buildLink(entry));
        }
        group.appendChild(row);
        root.appendChild(group);
    }

    function buildLink(rawEntry) {
        const trans = translateEntry(rawEntry);
        const url = urlFor(rawEntry);
        const link = document.createElement("a");
        link.className = "settings-link";
        link.dataset.settingsLink = rawEntry.uniqueID;
        link.textContent = trans.title || rawEntry.title || rawEntry.text || rawEntry.uniqueID;
        if (url) {
            link.href = url;
            link.title = trans.description || url;
        }
        link.addEventListener("click", (event) => {
            if (event && typeof event.preventDefault === "function") {
                event.preventDefault(); // never navigate the settings window away
            }
            const target = urlFor(entry);
            if (!target) {
                return;
            }
            openExternalLink(target).then((opened) => {
                if (!opened && onCopied) {
                    onCopied();
                }
            });
        });
        return link;
    }

    return { render };
}
