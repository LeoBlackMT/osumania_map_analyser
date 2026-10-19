// Difficulty-feedback statistics page. Vanilla JS, no dependencies.
// Data: GET /api/v1/feedback/summary?days=N (accepted votes only, groups N>=5).
(() => {
  "use strict";

  const $ = (id) => document.getElementById(id);
  const DEFAULT_DAYS = 30;
  let days = DEFAULT_DAYS;

  // Theme: same storage key as the usage dashboard so the two tabs stay in sync.
  (function initTheme() {
    let saved = null;
    try { saved = localStorage.getItem("mma-theme"); } catch { /* private mode */ }
    const systemDark = window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches;
    document.documentElement.setAttribute("data-theme", saved || (systemDark ? "dark" : "light"));
  })();

  $("themeBtn").addEventListener("click", () => {
    const current = document.documentElement.getAttribute("data-theme") || "light";
    const next = current === "dark" ? "light" : "dark";
    document.documentElement.setAttribute("data-theme", next);
    try { localStorage.setItem("mma-theme", next); } catch { /* ignore */ }
  });

  const el = (tag, className, text) => {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (text != null) node.textContent = text;
    return node;
  };

  const signed = (value) => {
    const number = Number(value);
    if (!Number.isFinite(number)) return "–";
    const absolute = Math.abs(number).toFixed(2);
    if (Math.abs(number) < 0.005) return "±0.00";
    return (number > 0 ? "+" : "−") + absolute;
  };

  function renderCards(host, entries) {
    host.innerHTML = "";
    for (const [value, label] of entries) {
      const card = el("div", "card");
      card.appendChild(el("div", "v", value));
      card.appendChild(el("div", "k", label));
      host.appendChild(card);
    }
  }

  /** Horizontal bar rows (same structure as the usage dashboard). */
  function renderBars(host, rows) {
    host.innerHTML = "";
    if (!rows.length) {
      host.appendChild(el("div", "empty", "No data in range"));
      return;
    }
    const max = Math.max(...rows.map((row) => row.value), 1);
    for (const row of rows) {
      const wrapper = el("div", "bar-row");
      wrapper.appendChild(el("div", "bar-label", row.label));
      const track = el("div", "bar-track");
      const fill = el("div", "bar-fill");
      fill.style.width = `${Math.max(2, (row.value / max) * 100)}%`;
      track.appendChild(fill);
      wrapper.appendChild(track);
      const count = el("div", "bar-count");
      count.appendChild(el("span", "num", row.num));
      if (row.extra) count.appendChild(el("span", "pct", row.extra));
      wrapper.appendChild(count);
      host.appendChild(wrapper);
    }
  }

  function renderDaily(points) {
    const host = $("daily");
    host.innerHTML = "";
    $("dailyFrom").textContent = points.length ? points[0].day : "–";
    $("dailyTo").textContent = points.length ? points[points.length - 1].day : "–";
    if (!points.length) {
      host.appendChild(el("div", "empty", "No data in range"));
      return;
    }
    const last30 = points.slice(-30);
    const max = Math.max(...last30.map((point) => point.votes), 1);
    for (const point of last30) {
      const bar = el("div", "dbar");
      bar.style.height = `${Math.max(2, (point.votes / max) * 100)}%`;
      bar.title = `${point.day} · ${point.votes} vote${point.votes === 1 ? "" : "s"}`;
      host.appendChild(bar);
    }
  }

  function render(data) {
    const totals = data.totals || {};
    renderCards($("overview"), [
      [Number(totals.votes || 0).toLocaleString(), "Accepted votes"],
      [Number(totals.maps || 0).toLocaleString(), "Maps rated"],
      [Number(totals.days || 0).toLocaleString(), "Days with votes"],
      [signed(totals.mean_vote), "Mean vote (dan)"],
      [signed(totals.mean_residual), "Mean residual (dan)"],
    ]);

    renderDaily(Array.isArray(data.daily) ? data.daily : []);

    const voteRows = (data.votes || []).map((bucket) => ({
      label: bucket.vote > 0 ? `+${bucket.vote}` : String(bucket.vote),
      value: Number(bucket.count) || 0,
      num: Number(bucket.count).toLocaleString(),
    }));
    renderBars($("votes"), voteRows);

    const algorithmRows = (data.algorithms || []).map((group) => ({
      label: group.name || "–",
      value: Number(group.n) || 0,
      num: Number(group.n).toLocaleString(),
      extra: `Δ ${signed(group.mean_residual)}`,
    }));
    renderBars($("algorithms"), algorithmRows);

    const keyRows = (data.key_counts || []).map((group) => ({
      label: `${group.name}K`,
      value: Number(group.n) || 0,
      num: Number(group.n).toLocaleString(),
      extra: `${Number(group.maps).toLocaleString()} maps`,
    }));
    renderBars($("keycounts"), keyRows);
  }

  async function load() {
    $("windowLabel").textContent = `${days}d`;
    try {
      const response = await fetch(`/api/v1/feedback/summary?days=${days}`, { cache: "no-store" });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      const data = await response.json();
      render(data);
      $("updated").textContent = "updated " + new Date().toLocaleTimeString();
    } catch (error) {
      $("updated").textContent = "unavailable (" + error.message + ")";
      for (const id of ["overview", "daily", "votes", "algorithms", "keycounts"]) {
        const host = $(id);
        if (host) {
          host.innerHTML = "";
          host.appendChild(el("div", "empty", "Feedback statistics are not available on this build yet."));
        }
      }
    }
  }

  $("range").addEventListener("click", (event) => {
    const button = event.target.closest("button[data-days]");
    if (!button) return;
    const next = Number(button.dataset.days);
    if (!Number.isFinite(next) || next === days) return;
    days = next;
    $("range").querySelectorAll("button").forEach((item) => item.classList.toggle("active", item === button));
    load();
  });

  load();
})();
