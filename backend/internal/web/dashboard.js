// ManiaMapAnalyser — Usage Statistics dashboard logic.
// Loaded as a separate asset (go:embed).

"use strict";

// XSS policy: every client-controlled string is written via textContent only.
const PALETTE = ["#635bff", "#0d9c5f", "#f5a623", "#cf1e3b", "#06b6d4", "#a855f7", "#ec4899", "#0ea5e9"];
const CHIP_DAYS = [1, 7, 30, 90];
const REFRESH_MS = 60000;
// Display caps for histogram X-ranges (view-only: the server keeps the full
// bins, and the true extrema live in the second metrics row).
const DUR_DISPLAY_CAP_MS = 600;
const PLAY_DISPLAY_CAP = 500;

// ── Window state (shared by chips, custom range, URL, and the fetch) ──
let win = { days: 30, from: "", to: "" };
let dataStart = "";   // earliest date with data (from the server), "YYYY-MM-DD"
let retryDelayMs = 2000;
let refreshTimer = null;
let loading = false;

function queryString() {
  return win.from && win.to ? "from=" + win.from + "&to=" + win.to : "days=" + win.days;
}
function windowLabel() {
  return win.from && win.to ? win.from.slice(5) + " → " + win.to.slice(5) : win.days + "d";
}
function syncUrl() {
  history.replaceState(null, "", location.pathname + "?" + queryString());
}
function todayStr() {
  return new Date().toISOString().slice(0, 10); // "YYYY-MM-DD" (UTC)
}

// ── Local-time helpers ───────────────────────────────
// All stored times are UTC; the dashboard converts them to the visitor's
// timezone for display (tooltips always carry the tz label).
function tzLabel() {
  const off = -new Date().getTimezoneOffset(); // minutes east of UTC
  if (off === 0) return "UTC";
  const sign = off > 0 ? "+" : "-";
  const abs = Math.abs(off);
  const h = Math.floor(abs / 60), m = abs % 60;
  return "UTC" + sign + h + (m ? ":" + String(m).padStart(2, "0") : "");
}
// Local "YYYY-MM-DD HH:MM" for an epoch-ms timestamp.
function fmtLocalTime(epochMs) {
  const d = new Date(epochMs);
  const p = (n) => String(n).padStart(2, "0");
  return d.getFullYear() + "-" + p(d.getMonth() + 1) + "-" + p(d.getDate()) + " " + p(d.getHours()) + ":" + p(d.getMinutes());
}
// Local hour-of-day (minutes since local midnight) for a UTC hour index.
function localHourOfDay(utcIdx) {
  const offMin = -new Date().getTimezoneOffset();
  return (((utcIdx * 60 + offMin) % 1440) + 1440) % 1440;
}
// Short axis label: "14" (or "05:30" for fractional-offset zones).
function hourLabel(utcIdx) {
  const m = localHourOfDay(utcIdx);
  const p = (n) => String(n).padStart(2, "0");
  return m % 60 ? p(Math.floor(m / 60)) + ":" + p(m % 60) : p(Math.floor(m / 60));
}
// Tooltip time: "14:00 (UTC+8)".
function hourTooltip(utcIdx) {
  const m = localHourOfDay(utcIdx);
  const p = (n) => String(n).padStart(2, "0");
  return p(Math.floor(m / 60)) + ":" + p(m % 60) + " (" + tzLabel() + ")";
}

// ── Theme: default to system preference, persist choice. ──
(function initTheme() {
  let saved = null;
  try { saved = localStorage.getItem("mma-theme"); } catch {}
  const sysDark = window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches;
  const theme = saved || (sysDark ? "dark" : "light");
  document.documentElement.setAttribute("data-theme", theme);
})();
document.getElementById("themeBtn").addEventListener("click", () => {
  const cur = document.documentElement.getAttribute("data-theme") || "light";
  const next = cur === "dark" ? "light" : "dark";
  document.documentElement.setAttribute("data-theme", next);
  try { localStorage.setItem("mma-theme", next); } catch {}
});

// ── Tooltip ─────────────────────────────────────────
const tip = document.getElementById("tip");
function bindTip(node, text) {
  node.addEventListener("mouseenter", () => { tip.textContent = String(text); tip.style.display = "block"; });
  node.addEventListener("mousemove", (e) => { tip.style.left = (e.clientX + 14) + "px"; tip.style.top = (e.clientY + 14) + "px"; });
  node.addEventListener("mouseleave", () => { tip.style.display = "none"; });
}

// ── DOM helpers ─────────────────────────────────────
function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = String(text);
  return e;
}
function svgEl(tag, attrs) {
  const e = document.createElementNS("http://www.w3.org/2000/svg", tag);
  for (const k in attrs) e.setAttribute(k, String(attrs[k]));
  return e;
}
function emptyBox(host, msg) {
  host.textContent = "";
  host.appendChild(el("div", "empty", msg || "No data in range"));
}

// ── Compact number formatter ────────────────────────
function fmtCompact(n) {
  n = Number(n) || 0;
  if (n >= 1e9) return (n / 1e9).toFixed(1).replace(/\.0$/, "") + "B";
  if (n >= 1e6) return Math.round(n / 1e5) / 10 + "M";
  if (n >= 1e4) return Math.round(n / 1e3) + "k";
  return String(n);
}

// ── Delta arrow ─────────────────────────────────────
function deltaSpan(delta) {
  const s = el("span", "delta");
  if (delta == null || !isFinite(delta)) { s.className = "delta flat"; s.textContent = "—"; return s; }
  if (delta > 0) { s.className = "delta up"; s.textContent = "▲" + fmtCompact(delta); }
  else if (delta < 0) { s.className = "delta down"; s.textContent = "▼" + fmtCompact(-delta); }
  else { s.className = "delta flat"; s.textContent = "–"; }
  bindTip(s, String(delta));
  return s;
}

// ── Stat card ────────────────────────────────────────
function card(k, v, opts) {
  const d = el("div", "card");
  const vWrap = el("div", "v");
  const vs = el("span", "", fmtCompact(v));
  bindTip(vs, String(v));
  vWrap.appendChild(vs);
  if (opts && opts.delta != null) vWrap.appendChild(deltaSpan(opts.delta));
  d.appendChild(vWrap);
  d.appendChild(el("div", "k", k));
  return d;
}

function renderCards(s) {
  const trend = s.activeTrend || [];
  const t = trend[trend.length - 1], y = trend[trend.length - 2];
  const todayDelta = (t && y) ? t.active - y.active : null;
  const weekDelta = (s.weekActive != null && s.weekActivePrev != null) ? s.weekActive - s.weekActivePrev : null;

  const ov = document.getElementById("overview");
  ov.textContent = "";
  ov.appendChild(card("Installs", s.totalInstalls));
  ov.appendChild(card("Online now", s.onlineNow));
  ov.appendChild(card("Active today", s.todayActive, { delta: todayDelta }));
  ov.appendChild(card("Active 7d", s.weekActive, { delta: weekDelta }));
  ov.appendChild(card("New today", s.newToday));
  ov.appendChild(card("New 7d", s.newWeek));
  ov.appendChild(card("Active 30d", s.monthActive));
  ov.appendChild(card("Events", s.totalEvents));
}

// ── Horizontal bar list ──────────────────────────────
function renderHBar(id, items, cap) {
  const c = document.getElementById(id);
  c.textContent = "";
  if (!items.length) { emptyBox(c); return; }
  const limit = (cap && items.length > cap) ? cap : items.length;
  let total = 0;
  for (const it of items) total += it.count;
  const bars = el("div", "bars");
  for (let i = 0; i < limit; i++) {
    const it = items[i];
    const row = el("div", "bar-row");
    const label = el("span", "bar-label", it.key);
    const track = el("div", "bar-track");
    const fill = el("div", "bar-fill");
    const pct = total > 0 ? Math.round(it.count / total * 100) : 0;
    fill.style.width = pct + "%";
    fill.style.animationDelay = (i * 30) + "ms";
    bindTip(row, it.key + " — " + it.count + " (" + pct + "%)");
    track.appendChild(fill);
    row.appendChild(label);
    row.appendChild(track);
    const cnt = el("span", "bar-count");
    const cntNum = el("span", "num", fmtCompact(it.count));
    bindTip(cntNum, String(it.count));
    cnt.appendChild(cntNum);
    cnt.appendChild(el("span", "pct", pct + "%"));
    row.appendChild(cnt);
    bars.appendChild(row);
  }
  c.appendChild(bars);
  if (items.length > limit) {
    const btn = el("button", "more-btn", "Show all " + items.length);
    btn.addEventListener("click", () => {
      bars.textContent = "";
      for (let i = 0; i < items.length; i++) {
        const it = items[i];
        const row = el("div", "bar-row");
        const label = el("span", "bar-label", it.key);
        const track = el("div", "bar-track");
        const fill = el("div", "bar-fill");
        const pct = total > 0 ? Math.round(it.count / total * 100) : 0;
        fill.style.width = pct + "%";
        bindTip(row, it.key + " — " + it.count + " (" + pct + "%)");
        track.appendChild(fill);
        row.appendChild(label);
        row.appendChild(track);
        const cnt = el("span", "bar-count");
        cnt.appendChild(el("span", "num", fmtCompact(it.count)));
        row.appendChild(cnt);
        bars.appendChild(row);
      }
      btn.remove();
    });
    c.appendChild(btn);
  }
}

// ── Vertical bar chart with Y-axis ticks ─────────────
function renderVChart(id, items, xEvery) {
  const c = document.getElementById(id);
  c.textContent = "";
  if (!items.length) { emptyBox(c); return; }
  const W = 560, H = 170, ML = 40, MB = 22, PAD = 6;
  const max = Math.max.apply(null, items.map((i) => i.count).concat([1]));
  const plotW = W - ML - PAD;
  const plotH = H - MB - PAD;
  const bw = plotW / items.length;
  const step = xEvery || Math.max(1, Math.ceil(items.length / 12));

  const svg = svgEl("svg", { viewBox: "0 0 " + W + " " + H, width: "100%" });

  [0, 1 / 3, 2 / 3, 1].forEach((f) => {
    const gy = (PAD + (1 - f) * plotH).toFixed(1);
    svg.appendChild(svgEl("line", { x1: ML, y1: gy, x2: W - PAD, y2: gy, class: "grid" }));
    const tx = svgEl("text", { x: ML - 6, y: (parseFloat(gy) + 3).toFixed(1), "text-anchor": "end" });
    tx.textContent = fmtCompact(Math.round(max * f));
    bindTip(tx, String(Math.round(max * f)));
    svg.appendChild(tx);
  });

  items.forEach((it, i) => {
    const h = Math.max(1, (it.count / max) * plotH);
    const x = ML + i * bw + bw * 0.2;
    const w = bw * 0.6;
    const rect = svgEl("rect", {
      x: x.toFixed(1), y: (H - MB - h).toFixed(1),
      width: w.toFixed(1), height: h.toFixed(1), rx: 1.5,
      fill: "#635bff", class: "grow",
    });
    rect.style.animationDelay = (i * 18) + "ms";
    bindTip(rect, it.tip || (it.label + " — " + it.count));
    svg.appendChild(rect);
    if (i % step === 0 || i === items.length - 1) {
      const tx = svgEl("text", { x: (x + w / 2).toFixed(1), y: H - 6, "text-anchor": "middle" });
      tx.textContent = it.label;
      svg.appendChild(tx);
    }
  });
  c.appendChild(svg);
}

// ── Single-series line chart with crosshair snap-hover ─────────────
// items: [{label, count, tip}] — used for the 24h distribution and the
// hourly trend (windows of one day).
function renderLine(id, items) {
  const c = document.getElementById(id);
  c.textContent = "";
  if (!items.length) { emptyBox(c); return; }
  const W = 560, H = 170, ML = 40, MB = 22, PAD = 6;
  const max = Math.max.apply(null, items.map((i) => i.count).concat([1]));
  const n = items.length;
  const x = (i) => ML + (i * (W - ML - PAD)) / Math.max(n - 1, 1);
  const y = (v) => PAD + (1 - v / max) * (H - MB - PAD);

  const svg = svgEl("svg", { viewBox: "0 0 " + W + " " + H, width: "100%" });
  const lg = svgEl("linearGradient", { id: "lineAreaGrad", x1: "0", y1: "0", x2: "0", y2: "1" });
  lg.appendChild(svgEl("stop", { offset: "0%", "stop-color": "#635bff", "stop-opacity": "0.18" }));
  lg.appendChild(svgEl("stop", { offset: "100%", "stop-color": "#635bff", "stop-opacity": "0" }));
  svg.appendChild(lg);

  [0, 1 / 3, 2 / 3, 1].forEach((f) => {
    const gy = (PAD + (1 - f) * (H - MB - PAD)).toFixed(1);
    svg.appendChild(svgEl("line", { x1: ML, y1: gy, x2: W - PAD, y2: gy, class: "grid" }));
    const tx = svgEl("text", { x: ML - 6, y: (parseFloat(gy) + 3).toFixed(1), "text-anchor": "end" });
    tx.textContent = fmtCompact(Math.round(max * f));
    bindTip(tx, String(Math.round(max * f)));
    svg.appendChild(tx);
  });

  const pts = items.map((it, i) => [x(i), y(it.count)]);
  const linePath = smoothPath(pts);
  const area = linePath + " L" + x(n - 1).toFixed(1) + "," + (H - MB).toFixed(1) + " L" + x(0).toFixed(1) + "," + (H - MB).toFixed(1) + " Z";
  svg.appendChild(svgEl("path", { d: area, fill: "url(#lineAreaGrad)" }));
  svg.appendChild(svgEl("path", { d: linePath, fill: "none", stroke: "#635bff", "stroke-width": 1.8, "stroke-linejoin": "round", class: "draw" }));

  const hover = svgEl("g", { display: "none" });
  const guide = svgEl("line", { class: "hover-guide" });
  guide.setAttribute("y1", PAD); guide.setAttribute("y2", H - MB);
  const halo = svgEl("circle", { class: "hover-halo", r: 10 });
  const dot = svgEl("circle", { class: "hover-dot", r: 4 });
  hover.appendChild(guide); hover.appendChild(halo); hover.appendChild(dot);
  svg.appendChild(hover);

  function snap(clientX) {
    const rect = svg.getBoundingClientRect();
    const sx = (clientX - rect.left) / rect.width * W;
    let best = 0, bestD = Infinity;
    for (let i = 0; i < n; i++) {
      const d = Math.abs(x(i) - sx);
      if (d < bestD) { bestD = d; best = i; }
    }
    return best;
  }
  function showHover(idx, clientX, clientY) {
    const it = items[idx];
    const xi = x(idx), yi = y(it.count);
    guide.setAttribute("x1", xi); guide.setAttribute("x2", xi);
    halo.setAttribute("cx", xi); halo.setAttribute("cy", yi);
    dot.setAttribute("cx", xi); dot.setAttribute("cy", yi);
    hover.setAttribute("display", "block");
    tip.textContent = it.tip;
    tip.style.display = "block";
    tip.style.left = (clientX + 14) + "px";
    tip.style.top = (clientY + 14) + "px";
  }
  function hideHover() { hover.setAttribute("display", "none"); tip.style.display = "none"; }

  svg.addEventListener("mousemove", (e) => showHover(snap(e.clientX), e.clientX, e.clientY));
  svg.addEventListener("mouseleave", hideHover);

  const step = Math.max(1, Math.ceil(n / 12));
  for (let i = 0; i < n; i += step) {
    const tx = svgEl("text", { x: x(i).toFixed(1), y: H - 6, "text-anchor": "middle" });
    tx.textContent = items[i].label;
    svg.appendChild(tx);
  }
  c.appendChild(svg);
}

function renderHourBars(id, counts) {
  const items = (counts || []).map((v, i) => ({
    label: hourLabel(i), // local hour-of-day, tz in tooltip
    count: v || 0,
    tip: hourTooltip(i) + " — " + (v || 0) + " installs",
  }));
  renderLine(id, items);
}

function renderValueBars(id, buckets) {
  const items = (buckets || []).map((b) => ({
    label: b.key,
    count: b.count,
    tip: b.key + " → " + b.count,
  }));
  renderVChart(id, items, 0);
}

// ── Smooth cubic bezier path through points ─────────────
// points: [[x, y], ...] — returns SVG path d string. Control points are
// clamped to the data y-range: the /6 Catmull-Rom tangents overshoot below
// zero between sharply falling points (the dip under the axis people saw
// between 16:00-17:00), and a bezier stays inside the convex hull of its
// control points, so clamping removes the dip entirely.
function smoothPath(points) {
  if (points.length < 2) return "";
  let minY = Infinity, maxY = -Infinity;
  for (const p of points) {
    if (p[1] < minY) minY = p[1];
    if (p[1] > maxY) maxY = p[1];
  }
  const cl = (v) => Math.max(minY, Math.min(maxY, v));
  let d = "M" + points[0][0].toFixed(1) + "," + points[0][1].toFixed(1);
  for (let i = 0; i < points.length - 1; i++) {
    const p0 = points[Math.max(i - 1, 0)];
    const p1 = points[i];
    const p2 = points[i + 1];
    const p3 = points[Math.min(i + 2, points.length - 1)];
    const cp1x = p1[0] + (p2[0] - p0[0]) / 6;
    const cp1y = cl(p1[1] + (p2[1] - p0[1]) / 6);
    const cp2x = p2[0] - (p3[0] - p1[0]) / 6;
    const cp2y = cl(p2[1] - (p3[1] - p1[1]) / 6);
    d += "C" + cp1x.toFixed(1) + "," + cp1y.toFixed(1) + " " + cp2x.toFixed(1) + "," + cp2y.toFixed(1) + " " + p2[0].toFixed(1) + "," + p2[1].toFixed(1);
  }
  return d;
}

// ── Line chart with crosshair snap-hover (two series: active · new) ──
function renderTrend(id, trend) {
  const c = document.getElementById(id);
  c.textContent = "";
  if (!trend || !trend.length) { emptyBox(c); return; }
  const W = 560, H = 170, ML = 40, MB = 22, PAD = 6;
  const max = Math.max.apply(null, trend.map((d) => Math.max(d.active, d.new)).concat([1]));
  const n = trend.length;
  const x = (i) => ML + (i * (W - ML - PAD)) / Math.max(n - 1, 1);
  const y = (v) => PAD + (1 - v / max) * (H - MB - PAD);

  const svg = svgEl("svg", { viewBox: "0 0 " + W + " " + H, width: "100%" });
  const lg = svgEl("linearGradient", { id: "areaGrad", x1: "0", y1: "0", x2: "0", y2: "1" });
  lg.appendChild(svgEl("stop", { offset: "0%", "stop-color": "#635bff", "stop-opacity": "0.12" }));
  lg.appendChild(svgEl("stop", { offset: "100%", "stop-color": "#635bff", "stop-opacity": "0" }));
  svg.appendChild(lg);

  [0, 1 / 3, 2 / 3, 1].forEach((f) => {
    const gy = (PAD + (1 - f) * (H - MB - PAD)).toFixed(1);
    svg.appendChild(svgEl("line", { x1: ML, y1: gy, x2: W - PAD, y2: gy, class: "grid" }));
    const tx = svgEl("text", { x: ML - 6, y: (parseFloat(gy) + 3).toFixed(1), "text-anchor": "end" });
    tx.textContent = fmtCompact(Math.round(max * f));
    bindTip(tx, String(Math.round(max * f)));
    svg.appendChild(tx);
  });

  const actPts = trend.map((d, i) => [x(i), y(d.active)]);
  const newPts = trend.map((d, i) => [x(i), y(d.new)]);
  const actPath = smoothPath(actPts);
  const newPath = smoothPath(newPts);
  const area = actPath + " L" + x(n - 1).toFixed(1) + "," + (H - MB).toFixed(1) + " L" + x(0).toFixed(1) + "," + (H - MB).toFixed(1) + " Z";
  svg.appendChild(svgEl("path", { d: area, fill: "url(#areaGrad)" }));
  svg.appendChild(svgEl("path", { d: actPath, fill: "none", stroke: "#635bff", "stroke-width": 1.8, "stroke-linejoin": "round", class: "draw" }));
  svg.appendChild(svgEl("path", { d: newPath, fill: "none", stroke: "#f5a623", "stroke-width": 1.2, "stroke-dasharray": "3 3", class: "draw" }));

  const hover = svgEl("g", { display: "none" });
  const guide = svgEl("line", { class: "hover-guide" });
  guide.setAttribute("y1", PAD); guide.setAttribute("y2", H - MB);
  const halo = svgEl("circle", { class: "hover-halo", r: 10 });
  const dot = svgEl("circle", { class: "hover-dot", r: 4 });
  hover.appendChild(guide); hover.appendChild(halo); hover.appendChild(dot);
  svg.appendChild(hover);

  function snap(clientX) {
    const rect = svg.getBoundingClientRect();
    const sx = (clientX - rect.left) / rect.width * W;
    let best = 0, bestD = Infinity;
    for (let i = 0; i < n; i++) {
      const d = Math.abs(x(i) - sx);
      if (d < bestD) { bestD = d; best = i; }
    }
    return best;
  }
  function showHover(idx, clientX, clientY) {
    const d = trend[idx];
    const xi = x(idx), yi = y(d.active);
    guide.setAttribute("x1", xi); guide.setAttribute("x2", xi);
    halo.setAttribute("cx", xi); halo.setAttribute("cy", yi);
    dot.setAttribute("cx", xi); dot.setAttribute("cy", yi);
    hover.setAttribute("display", "block");
    tip.innerHTML = "";
    const date = el("div", "tt-date", d.date || "");
    if (date.textContent) tip.appendChild(date);
    const r1 = el("div", "tt-row");
    r1.appendChild(el("span", "sw")); r1.firstChild.style.background = "#635bff";
    r1.appendChild(el("span", "lbl", "active"));
    r1.appendChild(el("span", "v", String(d.active)));
    tip.appendChild(r1);
    const r2 = el("div", "tt-row");
    r2.appendChild(el("span", "sw")); r2.firstChild.style.background = "#f5a623";
    r2.appendChild(el("span", "lbl", "new"));
    r2.appendChild(el("span", "v", String(d.new)));
    tip.appendChild(r2);
    tip.style.display = "block";
    tip.style.left = (clientX + 14) + "px";
    tip.style.top = (clientY + 14) + "px";
  }
  function hideHover() { hover.setAttribute("display", "none"); tip.style.display = "none"; }

  svg.addEventListener("mousemove", (e) => showHover(snap(e.clientX), e.clientX, e.clientY));
  svg.addEventListener("mouseleave", hideHover);

  const step = Math.max(1, Math.ceil(n / 6));
  for (let i = 0; i < n; i += step) {
    const tx = svgEl("text", { x: x(i).toFixed(1), y: H - 6, "text-anchor": "middle" });
    tx.textContent = trend[i].date ? trend[i].date.slice(5) : "";
    svg.appendChild(tx);
  }
  c.appendChild(svg);
}

// Trend panel: hourly (window <= 1 day) or daily (active · new).
function renderTrendSection(s) {
  const title = document.getElementById("trendTitle");
  const sub = document.getElementById("trendSub");
  const hours = s.activeTrendHourly;
  if (hours && hours.length) {
    title.textContent = "Active per hour";
    sub.textContent = "active";
    const items = hours.map((h) => {
      const d = new Date(h.hour);
      return {
        label: String(d.getHours()).padStart(2, "0"), // just the hour on the axis
        count: h.active,
        tip: fmtLocalTime(h.hour) + " (" + tzLabel() + ") — " + h.active + " active",
      };
    });
    renderLine("trend", items);
  } else {
    title.textContent = "Active per day";
    sub.textContent = "active · new";
    renderTrend("trend", s.activeTrend || []);
  }
}

// ── Donut + legend ───────────────────────────────────
function renderDonut(id, items, maxSlices) {
  const c = document.getElementById(id);
  c.textContent = "";
  if (!items.length) { emptyBox(c); return; }
  const MAX = maxSlices || 6;
  let slices = items;
  if (items.length > MAX) {
    const top = items.slice(0, MAX - 1);
    const other = items.slice(MAX - 1).reduce((s, it) => s + it.count, 0);
    slices = top.concat([{ key: "Other", count: other }]);
  }
  const total = slices.reduce((s, it) => s + it.count, 0) || 1;
  const svg = svgEl("svg", { viewBox: "0 0 42 42", width: "108", height: "108", "aria-hidden": "true" });
  let angle = 0;
  slices.forEach((it, i) => {
    const frac = it.count / total;
    const circle = svgEl("circle", {
      cx: 21, cy: 21, r: 15.915, fill: "none", "stroke-width": 4,
      stroke: PALETTE[i % PALETTE.length],
      "stroke-dasharray": (frac * 100).toFixed(2) + " " + (100 - frac * 100).toFixed(2),
      "stroke-dashoffset": (-angle * 100).toFixed(2),
    });
    bindTip(circle, it.key + " — " + it.count + " (" + Math.round((it.count / total) * 100) + "%)");
    svg.appendChild(circle);
    angle += frac;
  });
  const center = svgEl("text", {
    x: 21, y: 21, "text-anchor": "middle", "dominant-baseline": "central",
    "font-size": "5.4", "font-weight": "600",
  });
  center.textContent = fmtCompact(total);
  bindTip(center, String(total));
  svg.appendChild(center);

  const legend = el("div", "legend");
  slices.forEach((it, i) => {
    const row = el("div", "legend-row");
    const sw = el("span", "sw");
    sw.style.background = PALETTE[i % PALETTE.length];
    row.appendChild(sw);
    row.appendChild(el("span", "lk", it.key));
    const lv = el("span", "lv");
    const lvNum = el("span", "num", fmtCompact(it.count));
    bindTip(lvNum, String(it.count));
    lv.appendChild(lvNum);
    lv.appendChild(el("span", "pct", Math.round((it.count / total) * 100) + "%"));
    row.appendChild(lv);
    legend.appendChild(row);
  });
  const wrap = el("div", "donut-wrap");
  wrap.appendChild(svg);
  wrap.appendChild(legend);
  c.appendChild(wrap);
}

// ── Bottom metric chips ──────────────────────────────
function renderMetrics(s) {
  const c = document.getElementById("metrics");
  c.textContent = "";
  const wEl = document.getElementById("windowLabel");
  if (wEl) wEl.textContent = windowLabel();
  // "Avg daily" is meaningless for a single-day window — show a dash.
  const singleDay = s.windowFrom && s.windowFrom === s.windowTo;
  const avgDaily = singleDay ? "—" : fmtCompact(s.avgDailyActive);
  const avgDailyRaw = singleDay ? "—" : String(s.avgDailyActive);
  const hour = s.peakOnlineHour != null ? " · " + hourLabel(s.peakOnlineHour) : "";
  const peakRaw = s.peakOnline ? String(s.peakOnline) + " · " + hourTooltip(s.peakOnlineHour) : "—";
  const items = [
    ["Avg ★", s.avgStar ? s.avgStar.toFixed(2) : "—", s.avgStar ? s.avgStar.toFixed(2) : "—"],
    ["Avg LN%", s.avgLnRatio ? (s.avgLnRatio * 100).toFixed(1) + "%" : "—", s.avgLnRatio ? (s.avgLnRatio * 100).toFixed(1) + "%" : "—"],
    ["Peak online", s.peakOnline ? fmtCompact(s.peakOnline) + hour : "—", peakRaw],
    ["Avg daily", avgDaily, avgDailyRaw],
    ["Analyzes", fmtCompact(s.analyzeCount), String(s.analyzeCount)],
    ["Dur. avg", (s.durationStats && s.durationStats.avgMs ? s.durationStats.avgMs : 0) + " ms", (s.durationStats && s.durationStats.avgMs ? s.durationStats.avgMs : 0) + " ms"],
    ["Dur. p50", (s.durationStats ? s.durationStats.p50Ms : 0) + " ms", (s.durationStats ? s.durationStats.p50Ms : 0) + " ms"],
    ["Dur. p90", (s.durationStats ? s.durationStats.p90Ms : 0) + " ms", (s.durationStats ? s.durationStats.p90Ms : 0) + " ms"],
  ];
  for (const [k, v, raw] of items) {
    const chip = el("div", "chip");
    const cv = el("div", "cv", v);
    if (raw !== v) bindTip(cv, raw);
    chip.appendChild(cv);
    chip.appendChild(el("div", "ck", k));
    c.appendChild(chip);
  }

  // Second row: window extrema — four metrics × (min/max) = 8 chips.
  const cm = document.getElementById("metricsMax");
  if (cm) {
    cm.textContent = "";
    const ds = s.durationStats || {};
    const durLive = s.analyzeCount > 0;
    const starLive = !!s.maxStar;
    const numLive = s.maxNumeric != null && (s.maxNumeric !== 0 || !!s.minNumeric);
    const playLive = !!s.playMax;
    const pairs = [
      ["Dur.", durLive ? ds.minMs + " ms" : "—", durLive ? String(ds.minMs) : "—", durLive ? ds.maxMs + " ms" : "—", durLive ? String(ds.maxMs) : "—"],
      ["★", starLive ? s.minStar.toFixed(2) : "—", starLive ? s.minStar.toFixed(2) : "—", starLive ? s.maxStar.toFixed(2) : "—", starLive ? s.maxStar.toFixed(2) : "—"],
      ["Numeric", numLive ? s.minNumeric.toFixed(2) : "—", numLive ? s.minNumeric.toFixed(2) : "—", numLive ? s.maxNumeric.toFixed(2) : "—", numLive ? s.maxNumeric.toFixed(2) : "—"],
      ["Plays/day", playLive ? String(s.playMin) : "—", playLive ? String(s.playMin) : "—", playLive ? String(s.playMax) : "—", playLive ? String(s.playMax) : "—"],
    ];
    for (const [name, minV, minRaw, maxV, maxRaw] of pairs) {
      const mk = (v, raw, label) => {
        const chip = el("div", "chip");
        const cv = el("div", "cv", v);
        if (raw !== v) bindTip(cv, raw);
        chip.appendChild(cv);
        chip.appendChild(el("div", "ck", label));
        cm.appendChild(chip);
      };
      mk(minV, minRaw, name + " min");
      mk(maxV, maxRaw, name + " max");
    }
  }
}

// ── Accessibility: chart summaries (rendered content stays untouched) ──
function labelChart(id, text) {
  const node = document.getElementById(id);
  if (!node) return;
  node.setAttribute("role", "img");
  node.setAttribute("aria-label", text);
}

// ── Fetch + dispatch ─────────────────────────────────
function render(s) {
  const wl = windowLabel();
  document.getElementById("updated").textContent = "updated " + new Date().toLocaleString() + " (" + tzLabel() + ")";
  document.getElementById("serverVer").textContent = "v" + (s.serverVersion || "?");
  document.getElementById("hourRange").textContent = tzLabel() + " · " + wl;

  renderCards(s);
  renderHourBars("onlineByHour", s.onlineByHour || []);
  renderTrendSection(s);
  renderValueBars("starHistogram", s.starHistogram || []);
  renderValueBars("lnRatioHistogram", s.lnRatioHistogram || []);
  renderValueBars("numericHistogram", s.numericHistogram || [], "numeric");
  renderHBar("keycounts", s.keycounts || []);
  renderDonut("modes", s.modes || []);
  renderDonut("mods", s.mods || []);
  renderDonut("versions", s.versions || []);
  renderHBar("algorithms", s.algorithms || [], 8);
  renderHBar("actualAlgorithms", s.actualAlgorithms || [], 8);
  // Duration histogram: view capped at 600ms (the dense sub-second range
  // stays readable; bins beyond it and the true max live in the chips).
  renderValueBars("durationHistogram", (s.durationHistogram || []).filter((b) => (b.value || 0) < DUR_DISPLAY_CAP_MS));
  // Plays per day: continuous 10-count bins, view capped at 500 (server-side
  // binning continues past 500 — nothing is truncated).
  renderValueBars("playFreq", (s.playFreq || []).filter((b) => (b.value || 0) <= PLAY_DISPLAY_CAP));
  renderMetrics(s);

  // Chart summaries for screen readers (values also live in tooltips/legend).
  labelChart("onlineByHour", "Active installs by hour of day, " + wl);
  labelChart("trend", document.getElementById("trendTitle").textContent + ", " + wl);
  labelChart("starHistogram", "Star rating histogram, " + wl);
  labelChart("lnRatioHistogram", "LN ratio histogram, " + wl);
  labelChart("numericHistogram", "Numeric difficulty histogram, " + wl);
  labelChart("durationHistogram", "Analysis duration histogram, " + wl);
  labelChart("playFreq", "Plays per install per day, " + wl);
}

function load() {
  if (loading) return;
  loading = true;
  const status = document.getElementById("updated");
  status.textContent = "updating…";

  fetch("/api/v1/stats?" + queryString())
    .then((r) => r.json().catch(() => null).then((j) => ({ ok: r.ok, j })))
    .then(({ ok, j }) => {
      if (!ok) throw new Error((j && j.error) || "request failed");
      return j;
    })
    .then((s) => {
      retryDelayMs = 2000;
      dataStart = s.dataStart || "";
      syncDateLimits();
      render(s);
      loading = false;
    })
    .catch((err) => {
      // Keep the previous content; retry with backoff.
      status.textContent = "update failed — retrying in " + Math.round(retryDelayMs / 1000) + "s (" + err.message + ")";
      const delay = retryDelayMs;
      retryDelayMs = Math.min(retryDelayMs * 2, 60000);
      loading = false;
      setTimeout(load, delay);
    });
}

// ── Window controls ──────────────────────────────────
const fromInput = document.getElementById("fromInput");
const toInput = document.getElementById("toInput");
const customBox = document.getElementById("customBox");
const customPopup = document.getElementById("customPopup");
const customBtn = document.getElementById("customBtn");
const customErr = document.getElementById("customErr");

function showCustomErr(msg) {
  customErr.textContent = msg || "";
}
// Keep pickers sane: end >= start (and start <= end) while editing, plus the
// dataStart floor and the "today" ceiling.
function syncDateLimits() {
  const today = todayStr();
  toInput.max = today;
  if (dataStart) fromInput.min = dataStart;
  if (fromInput.value) toInput.min = fromInput.value;
  if (toInput.value) fromInput.max = toInput.value;
}
function isPopupOpen() {
  return !customPopup.hidden;
}
function openPopup() {
  showCustomErr("");
  syncDateLimits();
  customPopup.hidden = false;
  void customPopup.offsetHeight; // force reflow so the enter transition plays
  customPopup.classList.add("open");
  customBtn.setAttribute("aria-expanded", "true");
  fromInput.focus();
}
function closePopup() {
  customPopup.classList.remove("open");
  customBtn.setAttribute("aria-expanded", "false");
  // Match the CSS transition duration before hiding (animation on the way out).
  setTimeout(() => { customPopup.hidden = true; }, 180);
}
function syncControls() {
  const custom = Boolean(win.from && win.to);
  document.querySelectorAll("#range button").forEach((b) => {
    b.classList.toggle("active", !custom && win.days === parseInt(b.dataset.days, 10));
  });
  customBtn.classList.toggle("active", custom);
  customBox.classList.toggle("active", custom);
  fromInput.value = win.from || "";
  toInput.value = win.to || "";
  syncDateLimits();
}
function applyWin(next, close) {
  win = next;
  showCustomErr("");
  syncUrl();
  syncControls();
  if (close) closePopup();
  load();
}

(function initWinFromUrl() {
  const p = new URLSearchParams(location.search);
  const f = p.get("from"), t = p.get("to");
  if (f && t) {
    win = { days: 30, from: f, to: t };
  } else {
    const d = parseInt(p.get("days") || "30", 10);
    if (CHIP_DAYS.indexOf(d) >= 0) win.days = d;
  }
})();

// Chips are always clickable: while a custom range is active they act as the
// way back to the quick windows (no disabled state, nothing to get stuck in).
document.getElementById("range").addEventListener("click", (e) => {
  const btn = e.target.closest("button[data-days]");
  if (!btn) return;
  applyWin({ days: parseInt(btn.dataset.days, 10), from: "", to: "" }, true);
});

customBtn.addEventListener("click", () => {
  if (isPopupOpen()) closePopup();
  else openPopup();
});
// Clicking outside the popup closes it.
document.addEventListener("click", (e) => {
  if (isPopupOpen() && !customBox.contains(e.target)) closePopup();
});
// Esc closes the popup.
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && isPopupOpen()) closePopup();
});
// Live min/max linkage between the two date fields.
fromInput.addEventListener("change", syncDateLimits);
toInput.addEventListener("change", syncDateLimits);

document.getElementById("applyBtn").addEventListener("click", () => {
  const f = fromInput.value, t = toInput.value;
  if (!f || !t) { showCustomErr("Choose both dates"); return; }
  if (f > t) { showCustomErr("From must not be after To"); return; }
  if (dataStart && f < dataStart) { showCustomErr("Earliest date is " + dataStart); return; }
  if (t > todayStr()) { showCustomErr("To must not be in the future"); return; }
  applyWin({ days: 30, from: f, to: t }, true);
});

// Cancel discards the edits in the open popup (an active custom range stays
// until a chip or a new range replaces it).
document.getElementById("cancelBtn").addEventListener("click", () => {
  showCustomErr("");
  syncControls(); // restore the applied values into the fields
  closePopup();
});

// ── Auto-refresh (paused while the tab is hidden) ────
function startRefresh() {
  if (!refreshTimer) refreshTimer = setInterval(load, REFRESH_MS);
}
function stopRefresh() {
  if (refreshTimer) { clearInterval(refreshTimer); refreshTimer = null; }
}
document.addEventListener("visibilitychange", () => {
  if (document.hidden) { stopRefresh(); }
  else { load(); startRefresh(); }
});

syncControls();
load();
startRefresh();