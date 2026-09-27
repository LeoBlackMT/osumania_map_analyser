/**
 * `settings.json` `type: "button"` entries as working links (settings.html).
 *
 * tosu renders these entries as buttons in its own settings dashboard; this page
 * used to drop them (settingsForm.js only renders keys with an applier, so every
 * button was skipped) and the Guide / Preset guide / Issue / Benchmark / Debug /
 * Presets-page links were lost. They are links, not settings, so they live in a
 * compact row of their own instead of inside the form.
 *
 * Clicking never navigates this window: the click is handled here and the URL is
 * opened through `openExternalLink()` (see ../externalLink.js), which falls back
 * to the clipboard + `onCopied()` when the shell webview blocks `window.open`.
 */

import { openExternalLink } from "../externalLink.js";

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
        const buttons = (entries || []).filter((entry) => entry && entry.type === "button" && entry.uniqueID);
        if (buttons.length === 0) {
            return;
        }

        const group = document.createElement("div");
        group.className = "settings-links";

        const title = document.createElement("h3");
        title.className = "settings-group-title";
        title.textContent = "Links";
        group.appendChild(title);

        const row = document.createElement("div");
        row.className = "settings-links-row";
        for (const entry of buttons) {
            row.appendChild(buildLink(entry));
        }
        group.appendChild(row);
        root.appendChild(group);
    }

    function buildLink(entry) {
        const url = urlFor(entry);
        const link = document.createElement("a");
        link.className = "settings-link";
        link.dataset.settingsLink = entry.uniqueID;
        link.textContent = entry.title || entry.text || entry.uniqueID;
        if (url) {
            link.href = url;
            link.title = url;
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
