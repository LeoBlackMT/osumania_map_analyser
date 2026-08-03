// qa-graph-cache.mjs — Task 9 browser QA for lru-cache-graph-progress.
// Zero-dependency: node built-ins only (http, crypto, fs, assert, child_process).
// Runs against a real Chromium via CDP (--remote-debugging-port), no npm packages.
//
//   node docs/qa/qa-graph-cache.mjs
//
// Architecture:
//   - static server  :8787 serves the repo root (plugin page must load over http,
//                      file:// breaks ES module CORS)
//   - tosu mock      :24050 serves /files/beatmap/file (real .osu content) and
//                      answers the plugin's two WebSocket endpoints
//                      (/websocket/v2 = api_v2 pushes, /websocket/commands =
//                      getSettings) with a minimal RFC6455 implementation
//   - chromium headless driven over CDP (evaluate / screenshot)
//
// Mock contract (must match socketHandlers.js / modData.js / settings.js):
//   api_v2 payload fields used:
//     state.name                         -> play state (menu = not playing)
//     client                             -> "stable" (no lazer speed/OD quirks)
//     menu.mods.name                     -> mod code ("NM"/"DT") -> modSignature
//     beatmap.id / beatmap.set           -> numeric, truncated
//     beatmap.md5                        -> lowercase
//     files.beatmap                      -> normalized lowercase path
//     directPath.beatmapBackground       -> songKey "dir:" part
//     beatmap.time.firstObject/lastObject-> song range (for cursor clamping)
//     beatmap.time.live                  -> music_time ms (song time pushes)
//   getSettings command response: full settings array [{uniqueID, value}]
//     (same shape tosu pushes; plugin applyIf only touches present keys)

import http from "node:http";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import assert from "node:assert";
import { execFile } from "node:child_process";

// ---------------------------------------------------------------------------
// Paths & constants
// ---------------------------------------------------------------------------
const REPO = path.resolve(import.meta.dirname, "../..");
const PLUGIN = path.join(REPO, "ManiaMapAnalyser by Leo_Black");
const SETTINGS_FILE = path.join(PLUGIN, "settings.json");
const EVIDENCE_DIR = path.join(REPO, ".omo", "evidence");
const HIBACHI_FILE = path.join(REPO, "docs", "file", "speed", "HIBACHI.osu");
const LN_MAP_FILE = path.join(REPO, "docs", "file", "ln", "L4.8TS.osu");

const STATIC_PORT = 8787;
const TOSU_PORT = 24050;
const CDP_PORT = 9223;
const PAGE_URL = `http://localhost:${STATIC_PORT}/ManiaMapAnalyser%20by%20Leo_Black/index.html`;

const CHROME_CANDIDATES = [
    "C:\\Users\\Leo_BlackLT\\AppData\\Local\\ms-playwright\\chromium-1223\\chrome-win64\\chrome.exe",
    "C:\\Users\\Leo_BlackLT\\AppData\\Local\\ms-playwright\\chromium-1208\\chrome-win64\\chrome.exe",
    "C:\\Users\\Leo_BlackLT\\AppData\\Local\\ms-playwright\\chromium-1223\\chrome-win\\chrome.exe",
];
const CHROME = CHROME_CANDIDATES.find((p) => fs.existsSync(p));
if (!CHROME) {
    console.error("[FATAL] no chromium binary found in ms-playwright cache");
    process.exit(1);
}
for (const f of [HIBACHI_FILE, LN_MAP_FILE, SETTINGS_FILE]) {
    if (!fs.existsSync(f)) {
        console.error(`[FATAL] missing fixture: ${f}`);
        process.exit(1);
    }
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// ---------------------------------------------------------------------------
// Logging / evidence
// ---------------------------------------------------------------------------
const RESULTS_FILE = path.join(EVIDENCE_DIR, "task-9-results.txt");
const log = (line) => {
    const text = `[${new Date().toISOString()}] ${line}`;
    console.log(text);
    fs.appendFileSync(RESULTS_FILE, text + "\n");
};
const fail = (line) => {
    const text = `[${new Date().toISOString()}] [FAIL] ${line}`;
    console.error(text);
    fs.appendFileSync(RESULTS_FILE, text + "\n");
};

// ---------------------------------------------------------------------------
// Minimal RFC6455 WebSocket (client -> server frames are masked; we unmask)
// ---------------------------------------------------------------------------
function encodeWsFrame(opcode, payload) {
    const data = Buffer.isBuffer(payload) ? payload : Buffer.from(String(payload));
    const len = data.length;
    let header;
    if (len < 126) {
        header = Buffer.from([0x80 | opcode, len]);
    } else if (len < 65536) {
        header = Buffer.alloc(4);
        header[0] = 0x80 | opcode;
        header[1] = 126;
        header.writeUInt16BE(len, 2);
    } else {
        header = Buffer.alloc(10);
        header[0] = 0x80 | opcode;
        header[1] = 127;
        header.writeBigUInt64BE(BigInt(len), 2);
    }
    return Buffer.concat([header, data]);
}

class WsConnection {
    constructor(socket) {
        this.socket = socket;
        this.buffer = Buffer.alloc(0);
        this.onMessage = null;
        this.onClose = null;
        socket.on("data", (chunk) => this._onData(chunk));
        socket.on("close", () => this.onClose && this.onClose());
        socket.on("error", () => {});
    }
    _onData(chunk) {
        this.buffer = Buffer.concat([this.buffer, chunk]);
        for (;;) {
            const frame = this._parseFrame();
            if (!frame) return;
            if (frame.opcode === 8) {
                this.close();
                return;
            }
            if (frame.opcode === 9) {
                this.socket.write(encodeWsFrame(10, frame.payload));
                continue;
            }
            if ((frame.opcode === 1 || frame.opcode === 2) && this.onMessage) {
                this.onMessage(frame.payload.toString());
            }
        }
    }
    _parseFrame() {
        const b = this.buffer;
        if (b.length < 2) return null;
        const opcode = b[0] & 0x0f;
        let len = b[1] & 0x7f;
        let offset = 2;
        if (len === 126) {
            if (b.length < 4) return null;
            len = b.readUInt16BE(2);
            offset = 4;
        } else if (len === 127) {
            if (b.length < 10) return null;
            len = Number(b.readBigUInt64BE(2));
            offset = 10;
        }
        const masked = (b[1] & 0x80) !== 0;
        let maskKey = null;
        if (masked) {
            if (b.length < offset + 4) return null;
            maskKey = b.subarray(offset, offset + 4);
            offset += 4;
        }
        if (b.length < offset + len) return null;
        let payload = b.subarray(offset, offset + len);
        if (masked) {
            const unmasked = Buffer.alloc(len);
            for (let i = 0; i < len; i++) unmasked[i] = payload[i] ^ maskKey[i & 3];
            payload = unmasked;
        }
        this.buffer = b.subarray(offset + len);
        return { opcode, payload };
    }
    send(text) {
        try {
            this.socket.write(encodeWsFrame(1, text));
        } catch {
            /* socket dying */
        }
    }
    close() {
        try {
            this.socket.write(encodeWsFrame(8, Buffer.alloc(0)));
        } catch {
            /* ignore */
        }
        try {
            this.socket.end();
        } catch {
            /* ignore */
        }
    }
}

// ---------------------------------------------------------------------------
// tosu mock (port 24050): beatmap file HTTP + two WS endpoints
// ---------------------------------------------------------------------------
const mock = {
    fetchCount: 0,
    currentMap: null,
    currentMapFile: null,
    currentMod: "NM",
    stateName: "menu",
    v2Clients: new Set(),
    commandClients: new Set(),
    settings: [], // [{uniqueID, value}] — full profile, mutated by pushSettings
};

function pushV2(payload) {
    const text = JSON.stringify(payload);
    for (const c of mock.v2Clients) c.send(text);
}
function pushCommands(text) {
    for (const c of mock.commandClients) c.send(text);
}
function pushSettings(entries) {
    for (const entry of entries) {
        const found = mock.settings.find((s) => s.uniqueID === entry.uniqueID);
        if (found) found.value = entry.value;
    }
    pushCommands(JSON.stringify(mock.settings));
}

const CORS = {
    "Access-Control-Allow-Origin": "*",
    "Access-Control-Allow-Headers": "*",
};

const tosuServer = http.createServer((req, res) => {
    const pathname = req.url.split("?")[0];
    if (pathname === "/files/beatmap/file") {
        mock.fetchCount += 1;
        const content = fs.readFileSync(mock.currentMapFile);
        res.writeHead(200, { "Content-Type": "application/octet-stream", "Cache-Control": "no-store", ...CORS });
        res.end(content);
        return;
    }
    res.writeHead(404, CORS);
    res.end("not found");
});
tosuServer.on("upgrade", (req, socket) => {
    const url = req.url.split("?")[0];
    if (!url.startsWith("/websocket/")) {
        socket.destroy();
        return;
    }
    const accept = crypto
        .createHash("sha1")
        .update(req.headers["sec-websocket-key"] + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11")
        .digest("base64");
    socket.write(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n"
        + `Sec-WebSocket-Accept: ${accept}\r\n\r\n`
    );
    const conn = new WsConnection(socket);
    if (url.startsWith("/websocket/v2")) {
        mock.v2Clients.add(conn);
    } else {
        mock.commandClients.add(conn);
    }
    conn.onMessage = (text) => {
        if (text.startsWith("getSettings:")) {
            conn.send(JSON.stringify(mock.settings));
        }
        // "applyFilters:..." and everything else: no-op
    };
    conn.onClose = () => {
        mock.v2Clients.delete(conn);
        mock.commandClients.delete(conn);
    };
});

// ---------------------------------------------------------------------------
// Static server (port 8787) — repo root
// ---------------------------------------------------------------------------
const MIME = {
    ".html": "text/html; charset=utf-8",
    ".js": "text/javascript; charset=utf-8",
    ".mjs": "text/javascript; charset=utf-8",
    ".css": "text/css; charset=utf-8",
    ".wasm": "application/wasm",
    ".json": "application/json; charset=utf-8",
    ".png": "image/png",
    ".jpg": "image/jpeg",
    ".svg": "image/svg+xml",
    ".osu": "text/plain; charset=utf-8",
    ".txt": "text/plain; charset=utf-8",
    ".md": "text/plain; charset=utf-8",
};
const staticServer = http.createServer((req, res) => {
    try {
        let pathname = decodeURIComponent(req.url.split("?")[0]);
        if (pathname.endsWith("/")) pathname += "index.html";
        const filePath = path.normalize(path.join(REPO, pathname));
        if (!filePath.startsWith(REPO)) {
            res.writeHead(403);
            res.end("forbidden");
            return;
        }
        const content = fs.readFileSync(filePath);
        res.writeHead(200, { "Content-Type": MIME[path.extname(filePath).toLowerCase()] || "application/octet-stream" });
        res.end(content);
    } catch {
        res.writeHead(404);
        res.end("not found");
    }
});

// ---------------------------------------------------------------------------
// api_v2 payload builder (contract mirrors socketHandlers.js)
// ---------------------------------------------------------------------------
const MAPS = {
    hibachi: {
        id: 901234, set: 90234, md5: "a1b2c3d4e5f60718293a4b5c6d7e8f90",
        folder: "hibachi", file: "HIBACHI.osu",
        artist: "Kobaryo", title: "HIBACHI", version: "speed", creator: "mock",
        firstObject: 1000, lastObject: 210000,
        filePath: HIBACHI_FILE,
    },
    l4_8ts: {
        id: 901235, set: 90235, md5: "b2c3d4e5f60718293a4b5c6d7e8f901a",
        folder: "ln", file: "L4.8TS.osu",
        artist: "Various", title: "L4.8TS", version: "3rd Soar", creator: "mock",
        firstObject: 0, lastObject: 200000,
        filePath: LN_MAP_FILE,
    },
};

function buildPayload(map, modsName, liveMs, stateName) {
    const mods = modsName === "NM" ? { name: "NM", settings: [] } : { name: modsName, settings: [] };
    const payload = {
        gameplay: { gameMode: 3, name: "osu" },
        client: "stable",
        menu: {
            bm: {
                id: map.id, set: map.set, md5: map.md5,
                path: { folder: map.folder, file: map.file },
                time: { firstObject: map.firstObject, lastObject: map.lastObject },
                artist: map.artist, title: map.title, version: map.version, creator: map.creator,
            },
            mods,
            state: { name: "menu" },
        },
        files: { beatmap: `${map.folder}/${map.file}` },
        directPath: {
            beatmapFile: `${map.folder}/${map.file}`,
            beatmapBackground: `${map.folder}/bg.jpg`,
            audioFile: `${map.folder}/audio.mp3`,
        },
        folders: { beatmap: map.folder },
        // "menu" hides the card (updateCardPlayVisibility) — correct for analysis.
        // pushState("playing") temporarily reveals it for screenshots.
        state: { name: stateName || "menu" },
        beatmap: {
            id: map.id, set: map.set, md5: map.md5,
            time: { firstObject: map.firstObject, lastObject: map.lastObject },
            artist: map.artist, title: map.title, version: map.version, creator: map.creator,
        },
    };
    if (Number.isFinite(liveMs)) payload.beatmap.time.live = liveMs;
    return payload;
}

function changeMap(mapKey, modsName = "NM") {
    const map = MAPS[mapKey];
    mock.currentMap = map;
    mock.currentMapFile = map.filePath;
    mock.currentMod = modsName;
    mock.stateName = "menu";
    pushV2(buildPayload(map, modsName, null, "menu"));
}
function pushTime(ms) {
    pushV2(buildPayload(mock.currentMap, mock.currentMod, ms, mock.stateName));
}
// Reveal the card for screenshots: identical map+mod, different client state.
// Same identity/modSignature -> no recompute; in play state mods come from
// data.play.mods (absent -> no mod payload -> signature unchanged).
function pushState(name) {
    mock.stateName = name;
    pushV2(buildPayload(mock.currentMap, mock.currentMod, null, name));
}

// ---------------------------------------------------------------------------
// Minimal CDP client over Node's built-in WebSocket
// ---------------------------------------------------------------------------
class CdpClient {
    constructor(ws) {
        this.ws = ws;
        this.seq = 0;
        this.pending = new Map();
        this.events = [];
        this.onEvent = null;
        ws.onmessage = (ev) => {
            const msg = JSON.parse(ev.data);
            if (msg.id == null) {
                this.events.push(msg);
                if (this.onEvent) this.onEvent(msg.method, msg.params);
                return;
            }
            const p = this.pending.get(msg.id);
            if (!p) return;
            this.pending.delete(msg.id);
            if (msg.error) p.reject(new Error(`CDP ${msg.error.message}`));
            else p.resolve(msg.result);
        };
    }
    send(method, params = {}) {
        const id = ++this.seq;
        return new Promise((resolve, reject) => {
            this.pending.set(id, { resolve, reject });
            this.ws.send(JSON.stringify({ id, method, params }));
        });
    }
    close() {
        try {
            this.ws.close();
        } catch {
            /* ignore */
        }
    }
}

async function connectCdp(wsUrl) {
    return new Promise((resolve, reject) => {
        const ws = new WebSocket(wsUrl);
        ws.onopen = () => resolve(new CdpClient(ws));
        ws.onerror = () => reject(new Error(`cannot connect CDP ${wsUrl}`));
    });
}

async function evalJs(client, expression) {
    const r = await client.send("Runtime.evaluate", {
        expression,
        returnByValue: true,
        awaitPromise: true,
    });
    if (r.exceptionDetails) {
        const d = r.exceptionDetails.exception?.description || r.exceptionDetails.text;
        throw new Error(`page eval error: ${d}`);
    }
    return r.result?.value;
}

const allClients = [];

async function newPage(url) {
    for (let i = 0; i < 20; i++) {
        try {
            const res = await fetch(`http://127.0.0.1:${CDP_PORT}/json/new?about:blank`, { method: "PUT" });
            const target = await res.json();
            if (!target.webSocketDebuggerUrl) throw new Error("no ws url");
            const client = await connectCdp(target.webSocketDebuggerUrl);
            allClients.push(client);
            await client.send("Page.enable");
            await client.send("Runtime.enable");
            await client.send("Page.navigate", { url });
            await waitFor(async () => (await evalJs(client, "document.readyState")) === "complete", 20000, 100, "page readyState");
            return { client, targetId: target.id };
        } catch (e) {
            await sleep(500);
            if (i === 19) throw e;
        }
    }
    throw new Error("newPage failed");
}

async function closePage(targetId) {
    try {
        await fetch(`http://127.0.0.1:${CDP_PORT}/json/close/${targetId}`);
    } catch {
        /* already gone */
    }
}

async function waitFor(fn, timeoutMs, intervalMs, label) {
    const start = Date.now();
    for (;;) {
        try {
            const v = await fn();
            if (v) return v;
        } catch {
            /* keep polling */
        }
        if (Date.now() - start > timeoutMs) throw new Error(`timeout waiting for ${label}`);
        await sleep(intervalMs);
    }
}

async function waitBootAnalysis(client) {
    const status = () => evalJs(client, "document.getElementById('status').textContent");
    try {
        await waitFor(async () => !(await status()).startsWith("Waiting to load beatmap file"), 30000, 100, "boot analysis start");
    } catch (e) {
        const diag = await evalJs(
            client,
            `(() => {
                const bad = performance.getEntriesByType('resource')
                    .filter(r => r.responseStatus >= 400)
                    .map(r => r.responseStatus + ' ' + r.name.slice(0, 120));
                return JSON.stringify({
                    status: document.getElementById('status')?.textContent,
                    version: window.__MMA_VERSION,
                    scripts: [...document.scripts].map(s => s.src),
                    badResources: bad,
                }, null, 1);
            })()`
        );
        const exceptions = client.events
            .filter((m) => m.method === "Runtime.exceptionThrown")
            .map((m) => {
                const d = m.params?.exceptionDetails;
                return d?.exception?.description || d?.text || JSON.stringify(m.params);
            });
        const consoleErrs = client.events
            .filter((m) => m.method === "Runtime.consoleAPICalled" && m.params?.type === "error")
            .map((m) => m.params.args.map((a) => a.value ?? a.description ?? "").join(" "));
        throw new Error(`boot did not start: ${diag}\npageExceptions: ${exceptions.join("\n")}\nconsoleErrors: ${consoleErrs.join("\n")}`);
    }
    await waitAnalysis(client);
}

// Wait for a full analysis cycle: status must enter "Loading beatmap file..."
// and leave it. Never misses a cycle even if it passes between polls.
async function waitAnalysis(client, timeoutMs = 120000) {
    const status = () => evalJs(client, "document.getElementById('status').textContent");
    let sawLoading = false;
    const start = Date.now();
    for (;;) {
        const s = await status();
        if (s.startsWith("Loading beatmap file")) sawLoading = true;
        if (sawLoading && !s.startsWith("Loading beatmap file")) {
            await sleep(200);
            return s;
        }
        if (Date.now() - start > timeoutMs) {
            throw new Error(`waitAnalysis timeout (last status: ${s})`);
        }
        await sleep(40);
    }
}

async function screenshotCard(client, filePath) {
    // Backgrounded tabs don't composite — activate before capturing.
    await client.send("Page.bringToFront");
    await sleep(300);
    // clip uses PAGE coordinates; getBoundingClientRect is viewport-relative
    const rect = await evalJs(
        client,
        `(() => { const el = document.querySelector('.main-card'); if (!el) return null; const r = el.getBoundingClientRect(); return { x: r.x + window.scrollX, y: r.y + window.scrollY, width: r.width, height: r.height }; })()`
    );
    assert(rect && rect.width > 10, `card not visible for screenshot`);
    const r = await client.send("Page.captureScreenshot", {
        format: "png",
        clip: { x: rect.x, y: rect.y, width: rect.width, height: rect.height, scale: 1 },
    });
    fs.writeFileSync(filePath, Buffer.from(r.data, "base64"));
    return filePath;
}

// ---------------------------------------------------------------------------
// Settings profile: settings.json baseline + QA overrides
// ---------------------------------------------------------------------------
function loadSettingsProfile() {
    const raw = JSON.parse(fs.readFileSync(SETTINGS_FILE, "utf8"));
    const profile = raw
        .filter((s) => s.type !== "header" && s.type !== "button")
        .map((s) => ({ uniqueID: s.uniqueID, value: s.value }));
    const set = (id, v) => {
        const found = profile.find((s) => s.uniqueID === id);
        if (found) found.value = v;
    };
    set("enableUpdateCheck", false);        // no GitHub calls in QA
    set("contentBar", "Pattern");           // deterministic pattern block
    set("estimatorAlgorithm", "Sunny");     // fast worker estimator
    set("diffText", "Difficulty");
    set("enableResultCache", true);
    set("enablePauseDetection", false);     // not exercised; avoids freeze artifacts
    set("wsEndpoint", "localhost:24050");
    // Headless-safe visuals: backdrop-filter + cover art + triangles break
    // compositing in headless chrome (blank captures).
    set("cardBgBlur", "Off");
    set("enableCoverArt", false);
    set("enableFloatingTriangles", false);
    return profile;
}

// ---------------------------------------------------------------------------
// Scenario helpers
// ---------------------------------------------------------------------------
// The star capsule animates toward its value (display.js animateNumericCapsuleValue,
// ~600ms ease-out). Reading mid-animation yields partial values, so wait for two
// identical reads before capturing/comparing.
async function waitStarSettled(client) {
    await waitFor(async () => {
        const a = await evalJs(client, "document.getElementById('rework-star').textContent");
        await sleep(150);
        const b = await evalJs(client, "document.getElementById('rework-star').textContent");
        return a !== "-" && a === b;
    }, 5000, 100, "star animation settle");
}

async function grabDom(client) {
    await waitStarSettled(client);
    const v = await evalJs(
        client,
        `(() => ({
            star: document.getElementById('rework-star').textContent,
            diff: document.getElementById('rework-diff').textContent,
            fillD: document.getElementById('rework-diff-graph-fill').getAttribute('d'),
            clusters: document.getElementById('pattern-clusters').innerHTML,
            meta: document.getElementById('rework-meta').innerHTML,
            caption: document.getElementById('est-diff-caption').textContent,
        }))()`
    );
    assert(v.star !== "-", `analysis did not produce a star value (status visible: ${v.star})`);
    return v;
}

const graphProbe = (client) => evalJs(
    client,
    `(() => {
        const wrap = document.getElementById('rework-diff-graph-wrap');
        const fill = document.getElementById('rework-diff-graph-fill');
        const err = document.getElementById('rework-diff-graph-error');
        return { wrapHidden: wrap.hidden, fillD: fill.getAttribute('d'), errHidden: err.hidden };
    })()`
);

const clipProbe = (client) => evalJs(
    client,
    `(() => {
        const h = document.getElementById('rework-diff-graph-play-clip-rect');
        const hc = document.getElementById('rework-diff-graph-cursor');
        const b = document.getElementById('body-graph-play-clip-rect');
        return {
            headerW: Number.parseFloat(h.getAttribute('width')) || 0,
            bodyW: Number.parseFloat(b.getAttribute('width')) || 0,
            cursorX: Number.parseFloat(hc.getAttribute('x1')) || 0,
            cursorHidden: hc.hidden,
        };
    })()`
);

const assertStatusOk = (status) => {
    assert(
        !status.startsWith("[Error]") && !status.startsWith("Failed"),
        `analysis ended in error: ${status}`
    );
};

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------
async function main() {
    fs.mkdirSync(EVIDENCE_DIR, { recursive: true });
    fs.writeFileSync(RESULTS_FILE, `Task 9 QA — lru-cache-graph-progress (result cache + graph split)\n`);
    mock.settings = loadSettingsProfile();
    mock.currentMap = MAPS.hibachi;      // boot's initial-load fetch serves HIBACHI
    mock.currentMapFile = MAPS.hibachi.filePath;

    // 1. servers
    await new Promise((resolve, reject) => {
        staticServer.once("error", reject);
        staticServer.listen(STATIC_PORT, "127.0.0.1", resolve);
    });
    await new Promise((resolve, reject) => {
        tosuServer.once("error", reject);
        tosuServer.listen(TOSU_PORT, "127.0.0.1", resolve);
    });
    log(`static server on :${STATIC_PORT}, tosu mock on :${TOSU_PORT}`);

    // 2. chromium
    const profileDir = fs.mkdtempSync(path.join(os.tmpdir(), "qa-chrome-"));
    const chromeProc = execFile(CHROME, [
        "--headless=new",
        `--remote-debugging-port=${CDP_PORT}`,
        "--remote-allow-origins=*",
        `--user-data-dir=${profileDir}`,
        "--no-first-run", "--no-default-browser-check",
        "--use-gl=angle", "--use-angle=swiftshader",
        "--disable-background-networking", "--disable-extensions",
        "--window-size=1280,800",
        "about:blank",
    ]);
    await waitFor(
        async () => {
            try {
                const r = await fetch(`http://127.0.0.1:${CDP_PORT}/json/version`);
                return r.ok;
            } catch {
                return false;
            }
        },
        30000, 200, "chrome CDP endpoint"
    );
    log(`chromium up (${path.basename(CHROME)})`);

    let pageA = null;
    let pageATargetId = null;
    const passed = [];
    const scenarioLog = (name, detail) => {
        log(`[PASS] 场景: ${name} ${detail ? "— " + detail : ""}`);
        passed.push(name);
    };

    try {
        // =================================================================
        // Page A: cache ON (default)
        // =================================================================
        log("--- opening page A (enableResultCache=true) ---");
        const pageADesc = await newPage(PAGE_URL);
        pageA = pageADesc.client;
        pageATargetId = pageADesc.targetId;
        await waitBootAnalysis(pageA);
        const bootCount = mock.fetchCount;
        log(`boot done (fetchCount=${bootCount})`);

        // ============ Scenario 1: cache hit skips fetch ============
        {
            log("场景1: 缓存命中跳过 fetch");
            changeMap("hibachi", "NM");
            const s1 = await waitAnalysis(pageA);
            assertStatusOk(s1);
            assert(mock.fetchCount === bootCount + 1,
                `first MAP_CHANGED should fetch exactly once: got ${mock.fetchCount - bootCount}`);

            changeMap("hibachi", "NM"); // identical re-push: socket layer dedupes
            await sleep(1200);
            assert(mock.fetchCount === bootCount + 1,
                `identical re-push must not refetch: got ${mock.fetchCount - bootCount}`);

            // Real cache-hit path: mod round trip (NM -> DT -> NM)
            changeMap("hibachi", "DT");
            const s2 = await waitAnalysis(pageA);
            assertStatusOk(s2);
            assert(mock.fetchCount === bootCount + 2, `DT analysis should fetch: ${mock.fetchCount - bootCount}`);
            changeMap("hibachi", "NM");
            const s3 = await waitAnalysis(pageA);
            assertStatusOk(s3);
            assert(mock.fetchCount === bootCount + 2,
                `NM re-analysis must be served from cache (no fetch): ${mock.fetchCount - bootCount}`);
            scenarioLog("1 缓存命中跳过 fetch",
                `two identical MAP_CHANGED → 1 fetch; NM→DT→NM round trip → cache hit, still ${mock.fetchCount - bootCount} fetches`);
        }

        // ============ Scenario 2: cache on/off DOM consistency ============
        {
            log("场景2: 缓存开/关 DOM 一致性");
            const aDom = await grabDom(pageA); // captured from the cached run above

            mock.settings.find((s) => s.uniqueID === "enableResultCache").value = false;
            log("--- opening page B (enableResultCache=false) ---");
            const pageBDesc = await newPage(PAGE_URL);
            const pageB = pageBDesc.client;
            await waitBootAnalysis(pageB);
            changeMap("hibachi", "NM");
            const s4 = await waitAnalysis(pageB);
            assertStatusOk(s4);
            const bDom = await grabDom(pageB);
            try {
                assert.deepStrictEqual(aDom, bDom, "cached DOM must byte-match fresh-computed DOM");
            } catch (e) {
                const keys = [...new Set([...Object.keys(aDom), ...Object.keys(bDom)])];
                const diffs = keys
                    .filter((k) => aDom[k] !== bDom[k])
                    .map((k) => `${k}: ${JSON.stringify(aDom[k]).slice(0, 60)} vs ${JSON.stringify(bDom[k]).slice(0, 60)}`);
                e.message += `\nDIFFS: ${diffs.join("\n")}`;
                throw e;
            }
            await closePage(pageBDesc.targetId);
            mock.settings.find((s) => s.uniqueID === "enableResultCache").value = true;
            scenarioLog("2 缓存开/关 DOM 一致性",
                `star=${aDom.star} diff=${aDom.diff} fillD=${JSON.stringify(aDom.fillD)} clusters=${aDom.clusters.length}B — byte equal`);
        }

        // ============ Scenario 3: estimator change invalidates cache ============
        {
            log("场景3: 设置变更失效缓存");
            const before = mock.fetchCount;
            pushSettings([{ uniqueID: "estimatorAlgorithm", value: "Daniel" }]);
            const s5 = await waitAnalysis(pageA);
            assertStatusOk(s5);
            assert(mock.fetchCount === before + 1,
                `estimator change must recompute with a fetch: got ${mock.fetchCount - before}`);
            scenarioLog("3 设置变更失效缓存", `estimatorAlgorithm=Daniel → fetch +1 (${before} → ${mock.fetchCount})`);
        }

        // ============ Scenario 4: graph coverage check ============
        {
            log("场景4: Graph 覆盖检查");
            const before = mock.fetchCount;
            pushSettings([
                { uniqueID: "estimatorAlgorithm", value: "Sunny" },
                { uniqueID: "diffText", value: "Graph" },
            ]);
            const s6 = await waitAnalysis(pageA);
            assertStatusOk(s6);
            await waitFor(async () => (await graphProbe(pageA)).fillD.length > 20, 5000, 100, "graph fill rendered");
            const g = await graphProbe(pageA);
            assert(!g.wrapHidden, "header graph wrap must be visible in diffText=Graph");
            assert(g.fillD.length > 20, "graph fill d must be non-empty");
            assert(g.errHidden, "graph error must stay hidden");
            assert(mock.fetchCount === before + 1, `graph coverage change must recompute: ${mock.fetchCount - before}`);
            scenarioLog("4 Graph 覆盖检查", `wrap visible, fill d ${g.fillD.length} chars, error hidden, fetch +1`);
        }

        // ============ Scenario 5: RC estimator → Sunny fallback + cache hit ============
        {
            log("场景5: 回退 Sunny + 二次命中");
            // NOTE: current code has NO LN gate in Azusa (rcLnRatioLimit exists only in
            // Roxy). Roxy rejects L4.8TS (100% LN > 0.18) and analysis.js falls back to
            // Sunny — this is the mechanism the plan's "Azusa 回退 Sunny" describes.
            pushSettings([
                { uniqueID: "estimatorAlgorithm", value: "Roxy" },
                { uniqueID: "diffText", value: "Difficulty" },
            ]);
            await waitAnalysis(pageA);
            changeMap("l4_8ts", "NM");
            const s7 = await waitAnalysis(pageA);
            assertStatusOk(s7);
            const caption1 = await evalJs(pageA, "document.getElementById('est-diff-caption').textContent");
            assert(caption1.includes("[Sunny]"),
                `high-LN map rejected by Roxy must fall back to Sunny in caption: "${caption1}"`);
            await waitStarSettled(pageA);
            const star1 = await evalJs(pageA, "document.getElementById('rework-star').textContent");

            // Second pass over the LN map: cache hit (via HIBACHI in between).
            // Both hops must be served from cache — 0 fetches proves the LN-map
            // re-analysis hit the Roxy-keyed entry written in the first pass.
            const before = mock.fetchCount;
            changeMap("hibachi", "NM");
            await waitAnalysis(pageA);
            changeMap("l4_8ts", "NM");
            const s8 = await waitAnalysis(pageA);
            assertStatusOk(s8);
            assert(mock.fetchCount === before,
                `cache-hit round trip must not fetch: got ${mock.fetchCount - before}`);
            await waitStarSettled(pageA);
            const star2 = await evalJs(pageA, "document.getElementById('rework-star').textContent");
            const caption2 = await evalJs(pageA, "document.getElementById('est-diff-caption').textContent");
            assert(star2 === star1, `cached star must equal fresh star: ${star1} vs ${star2}`);
            assert(caption2.includes("[Sunny]"), `cached caption must still say [Sunny]: "${caption2}"`);
            scenarioLog("5 回退 Sunny + 二次命中",
                `Roxy 拒绝高LN(L4.8TS)→Sunny: caption "${caption1}", star ${star1}, re-analysis 0 fetches`);
        }

        // ============ Scenario 6: graph split clip / cursor / clear ============
        {
            log("场景6: graph 分裂 clip/游标/清理");
            pushSettings([
                { uniqueID: "diffText", value: "Graph" },
                { uniqueID: "contentBar", value: "Graph" },
            ]);
            await waitAnalysis(pageA);
            changeMap("hibachi", "NM");
            const s9 = await waitAnalysis(pageA);
            assertStatusOk(s9);
            await waitFor(async () => (await graphProbe(pageA)).fillD.length > 20, 5000, 100, "graph fill rendered");

            // (b) unplayed: no music_time yet → clip width 0
            let p = await clipProbe(pageA);
            assert(p.headerW === 0 && p.bodyW === 0 && p.cursorHidden,
                `unplayed graph must have zero clip: header=${p.headerW} body=${p.bodyW}`);

            // (a) music_time=5000 → clip width tracks cursor x1
            pushTime(5000);
            await sleep(200);
            p = await clipProbe(pageA);
            assert(Math.abs(p.headerW - p.cursorX) <= 0.5,
                `clip width must match cursor x: width=${p.headerW} x1=${p.cursorX}`);
            const playedW = p.headerW;

            // (c) frozen song time → width stable
            pushTime(5000); // second sample locks interpolation rate to 0
            await sleep(200);
            const w1 = (await clipProbe(pageA)).headerW;
            await sleep(200);
            const w2 = (await clipProbe(pageA)).headerW;
            assert(w1 === w2, `frozen song time must freeze clip: ${w1} vs ${w2}`);

            // (d) clearDiffGraph via leaving Graph mode
            pushSettings([
                { uniqueID: "diffText", value: "Difficulty" },
                { uniqueID: "contentBar", value: "Pattern" },
            ]);
            await waitAnalysis(pageA);
            p = await clipProbe(pageA);
            assert(p.headerW === 0 && p.bodyW === 0,
                `clearDiffGraph must zero clips: header=${p.headerW} body=${p.bodyW}`);

            // back to Graph for screenshots
            pushSettings([
                { uniqueID: "diffText", value: "Graph" },
                { uniqueID: "contentBar", value: "Graph" },
            ]);
            const s10 = await waitAnalysis(pageA);
            assertStatusOk(s10);
            await waitFor(async () => (await graphProbe(pageA)).fillD.length > 20, 5000, 100, "graph fill rendered");
            scenarioLog("6 graph 分裂 clip/游标/清理",
                `unplayed 0/0; t=5000 width≈x1 (${playedW}); frozen stable (${w1}==${w2}); cleared 0/0`);

            // (e) screenshots at t=5000 and t=20000.
            // Card is hidden in "menu" (updateCardPlayVisibility) — switch to
            // "playing" so the card is visible in the captures.
            pushState("playing");
            await waitFor(
                async () => !(await evalJs(
                    pageA,
                    `document.querySelector('.main-card').classList.contains('card-hidden-by-play')`
                )),
                5000, 200, "card revealed by playing state"
            );
            await sleep(1200); // card fade-in transition (~0.5s)
            pushTime(5000);
            await sleep(150);
            pushTime(5000);
            await sleep(400);
            const shot1 = await screenshotCard(pageA, path.join(EVIDENCE_DIR, "task-9-split-header.png"));
            pushTime(20000);
            await sleep(150);
            pushTime(20000);
            await sleep(400);
            const shot2 = await screenshotCard(pageA, path.join(EVIDENCE_DIR, "task-9-split-body.png"));
            pushState("menu");
            await sleep(200);
            log(`[PASS] 场景: 6e 截图 — ${path.basename(shot1)} / ${path.basename(shot2)}`);
        }

        // ============ Scenario 7: cache off → never write cache ============
        {
            log("场景7: 开关关闭不写缓存");
            pushSettings([{ uniqueID: "enableResultCache", value: false }]);
            const before = mock.fetchCount;
            changeMap("hibachi", "DT");
            await waitAnalysis(pageA);
            const afterDt = mock.fetchCount;
            assert(afterDt === before + 1, `DT analysis must fetch: ${afterDt - before}`);
            changeMap("hibachi", "NM");
            await waitAnalysis(pageA);
            const afterNm = mock.fetchCount;
            assert(afterNm === afterDt + 1,
                `NM re-analysis must fetch again (cache disabled): ${afterNm - afterDt}`);
            scenarioLog("7 开关关闭不写缓存", `DT fetch +1, NM re-fetch +1 (${before} → ${afterNm})`);
        }
    } finally {
        // cleanup
        try {
            if (pageATargetId) await closePage(pageATargetId);
        } catch { /* ignore */ }
        for (const c of allClients) c.close();
        try { chromeProc.kill(); } catch { /* ignore */ }
        try { fs.rmSync(profileDir, { recursive: true, force: true }); } catch { /* ignore */ }
        try { staticServer.close(); } catch { /* ignore */ }
        try { tosuServer.close(); } catch { /* ignore */ }
        for (const c of [...mock.v2Clients, ...mock.commandClients]) c.close();
    }

    return passed;
}

// ---------------------------------------------------------------------------
// Evidence: mock contract summary
// ---------------------------------------------------------------------------
function writeContract() {
    const lines = [
        "Task 9 QA — mock contract summary (qa-graph-cache.mjs)",
        "= api_v2 payload fields the plugin reads (socketHandlers.js/modData.js) =",
        "  state.name            -> play state (menu → isInPlayState=false, pause detection off)",
        "  client                -> 'stable' (no lazer speed_change/DA handling)",
        "  menu.mods.name        -> mod code; 'NM' or 'DT'; drives speedRate & modSignature",
        "  beatmap.id            -> numeric identity part 'id:<n>'",
        "  beatmap.md5           -> hash identity part 'hash:<lowercase>'",
        "  files.beatmap         -> path identity part 'path:<lowercase, /-normalized>'",
        "  directPath.beatmapBackground / folders.beatmap -> songKey 'dir:' part",
        "  beatmap.set           -> songKey 'set:' part",
        "  beatmap.time.firstObject / lastObject -> songStartMs/songEndMs (cursor clamping)",
        "  beatmap.time.live     -> music_time (song time pushes; absent on map changes)",
        "  * identity MUST include id/hash/path (meta: fallback keys are skipped by cache)",
        "= WebSocket endpoints =",
        "  ws://host:24050/websocket/v2?l=...      api_v2 pushes (server→client)",
        "  ws://host:24050/websocket/commands?l=... getSettings request/response",
        "  getSettings response: full settings array [{uniqueID, value}]",
        "  (plugin sends 'getSettings:<counterPath>'; server replies with the array)",
        "= HTTP =",
        "  GET /files/beatmap/file → content of the CURRENT mock beatmap (no query params!)",
        "  GET /files/beatmap/background?ts=...    → 404 (cover theme falls back)",
        "  ./settings.json fetched by loadSettings() as baseline — served by static server",
        "= DOM ids asserted =",
        "  #rework-star #rework-diff #est-diff-caption #pattern-clusters #rework-meta",
        "  #rework-diff-graph-wrap #rework-diff-graph-fill #rework-diff-graph-error",
        "  #rework-diff-graph-play-clip-rect #rework-diff-graph-cursor",
        "  #body-graph-play-clip-rect",
        "= status cycle =",
        "  #status: 'Loading beatmap file (reason)...' → metadata line (done) / '[Error] ...'",
        "= notes / pitfalls found =",
        "  - identical re-push of the same map+mod is DEDUPED in socketHandlers.js",
        "    (hasStateMismatch early-return) — cache hits need a mod/map round trip",
        "  - page boot always fetches once (initial load, empty identity → no cache read);",
        "    the getSettings estimator change at boot can add a second fetch (aborted)",
        "  - clip width & cursor x1 are set from the same x value in updateGraphCursor()",
        "  - single music_time sample leaves interpolation rate=1 (time drifts);",
        "    a second sample with equal time locks rate=0 (frozen) — used for pause test",
    ];
    const file = path.join(EVIDENCE_DIR, "task-9-mock-contract.txt");
    fs.writeFileSync(file, lines.join("\n") + "\n");
    return file;
}

// ---------------------------------------------------------------------------
// Learnings append (idempotent: strips previous task-9 sections first)
// ---------------------------------------------------------------------------
function appendLearnings(passedCount, failed) {
    const file = path.join(REPO, ".omo", "notepads", "lru-cache-graph-progress", "learnings.md");
    let content = fs.readFileSync(file, "utf8");
    const marker = "## 2026-08-03"; // task 9 sections start with the date header
    const idx = content.indexOf(`\n## 2026-08-03`);
    if (idx > 0) content = content.slice(0, idx);
    const stamp = new Date().toISOString().replace("T", " ").slice(0, 19);
    const lines = [
        "",
        `## ${stamp} — Task 9: 浏览器端 QA（docs/qa/qa-graph-cache.mjs，零依赖）`,
        `- 结果: ${passedCount}/7 场景通过` + (failed ? `，失败: ${failed}` : ""),
        "- 方式: node 内置 http 静态服务器(8787) + tosu mock(24050, 手写 RFC6455 WS 服务端) + headless chromium(ms-playwright 缓存) 经 CDP 驱动 —— 无 npm 依赖、无 package.json、无 playwright 库（系统里根本没有 playwright npm 包，只有浏览器二进制）。",
        "- mock 契约字段清单见 .omo/evidence/task-9-mock-contract.txt；证据: .omo/evidence/task-9-results.txt + task-9-split-header/body.png（t=5000 / t=20000，分裂填充可见）。",
        "- 发现并修复 2 个真实插件 bug（浏览器构建在任务 5 后已损坏，QA 无法运行前必须先修）:",
        "  1) appContext.js 缺 parseEnableResultCacheValue 再导出（settings.js import 它）→ 模块图加载即崩。settingsParser 工厂里有，destructure 列表漏了 → 补一行。",
        "  2) analysis.js 缓存快照直接存 patternReport/mergedClusters，其 cluster 对象带 format()/Importance 方法，structuredClone 抛错 → 任何开启缓存的普通分析都失败。修法: put 前 jsonSafe 投影（渲染只读普通字段）。",
        "- 坑1: 相同 map+mod 重复推送被 socketHandlers 的 hasStateMismatch 提前 return 去重——缓存命中无法用重复 MAP_CHANGED 触发；用 NM→DT→NM 或 换图→换回 的回路验证真实命中。",
        "- 坑2: 页面 boot 自带 initial-load fetch（空 identity 不读缓存）；getSettings 到达后的 estimator 变更还会再触发一次 lazy recompute（abort 前者）——fetch 断言必须用增量而非绝对值。",
        "- 坑3: 单次 music_time 样本后插值速率=1（时间漂移），暂停/冻结测试需推两次相同 t 锁 rate=0。",
        "- 坑4: enableResultCache 不在 loadSettings 的 applySettingsFrom 列表（settings.json baseline 管不到它），必须走 getSettings 命令通道控制。",
        "- 坑5: 每次 map change 插件都会重发 getSettings，mock 的 settings profile 必须与手动推送保持同步，否则下一条响应会把设置改回去。",
        "- 坑6: 星值胶囊有 ~600ms 数字动画（animateNumericCapsuleValue），立即读取会拿到中途值——先等两次读取一致。",
        "- 坑7: menu 状态下 updateCardPlayVisibility 强制隐藏整张卡（visibility 规则），截图前必须推 state=playing 亮卡；且后续 pushTime 的 payload 会带上旧 state 名把卡又藏回去（mock 需维护当前 stateName）。",
        "- 坑8: 卡片入场有 opacity 过渡（~0.5s），截图要在过渡完成后；CDP captureScreenshot 的 clip 是页面坐标（要加 scrollX/Y）；headless 下 backdrop-filter/cover-art/浮窗动画可能不合成（QA profile 关掉 cardBgBlur/enableCoverArt/enableFloatingTriangles）。",
        "- 坑9: 场景5 的“Azusa 拒绝高 LN”前提过时——当前代码 azusaEstimator 没有 LN 门（rcLnRatioLimit 只在 Roxy），Azusa 只拒绝非 4K/过短。改用 Roxy+L4.8TS（真拒绝→Sunny 回退）验证同一机制。",
        "- 坑10: worker (Sunny/Daniel/Roxy) + Etterna WASM 在无头 chrome 正常；.wasm 需 application/wasm MIME；Windows 上 CDP 需 --remote-allow-origins=*。",
    ];
    fs.writeFileSync(file, content + lines.join("\n") + "\n");
}

const failedScenarios = [];
process.on("unhandledRejection", (e) => {
    fail(`unhandled rejection: ${e && e.stack ? e.stack : e}`);
    appendLearnings(0, "unhandledRejection");
    process.exitCode = 1;
});

try {
    const passed = await main();
    writeContract();
    appendLearnings(passed.length, null);
    log(`SUMMARY: ${passed.length}/7 scenarios passed, exit 0`);
    process.exitCode = 0;
} catch (e) {
    fail(e && e.stack ? e.stack : String(e));
    try { writeContract(); } catch { /* ignore */ }
    appendLearnings(0, e && e.message ? e.message : String(e));
    process.exitCode = 1;
}
