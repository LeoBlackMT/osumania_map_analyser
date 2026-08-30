// 壳桥冒烟（desktop，契约 v1）：WS 帧 + 24060 POST 全链路。
// 前置：mma-shell 已启动（离线模式：MMA_SKIP_TOSU_PROBE=1）。
// 运行：node desktop/scripts/smoke-bridge.mjs

const BASE = "http://127.0.0.1:24061";
const POST_URL = "http://127.0.0.1:24060";

let passed = 0;
let failed = 0;
function assert(cond, msg) {
    if (cond) {
        passed++;
        console.log(`PASS: ${msg}`);
    } else {
        failed++;
        console.error(`FAIL: ${msg}`);
    }
}

function wsConnectWithTimeout(ms = 5000) {
    return new Promise((resolve, reject) => {
        const ws = new WebSocket("ws://127.0.0.1:24061/ws");
        const timer = setTimeout(() => reject(new Error("ws connect timeout")), ms);
        ws.addEventListener("open", () => {
            clearTimeout(timer);
            resolve(ws);
        });
        ws.addEventListener("error", () => {
            clearTimeout(timer);
            reject(new Error("ws error"));
        });
    });
}

function nextFrame(ws, type, timeoutMs = 8000) {
    return new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
            cleanup();
            reject(new Error(`frame ${type} timeout`));
        }, timeoutMs);
        const onMsg = (ev) => {
            let frame;
            try {
                frame = JSON.parse(ev.data);
            } catch {
                return;
            }
            if (frame.type === type) {
                cleanup();
                resolve(frame);
            }
        };
        const cleanup = () => {
            clearTimeout(timer);
            ws.removeEventListener("message", onMsg);
        };
        ws.addEventListener("message", onMsg);
    });
}

function post(payload) {
    return fetch(POST_URL, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: typeof payload === "string" ? payload : JSON.stringify(payload),
    });
}

async function main() {
    // 0. 基础 HTTP
    try {
        const index = await fetch(`${BASE}/`);
        assert(index.status === 200, `GET / -> 200 (${(await index.text()).length} bytes)`);
    } catch (e) {
        console.error("STEP0 GET / FAULT:", e.name, e.message, e.cause?.message ?? "");
        throw e;
    }
    const settings0 = await fetch(`${BASE}/settings`);
    assert(settings0.status === 200, "GET /settings -> 200");
    const bad = await post({ nope: true });
    assert(bad.status === 400, `POST bad payload -> 400 (got ${bad.status})`);
    const huge = await post({
        meta: { title: "huge", keys: 4 },
        chartText: "x".repeat(5 * 1024 * 1024 + 1),
    });
    assert(huge.status === 504, `POST >5MB -> 504 PAYLOAD_TOO_LARGE (got ${huge.status})`);

    // 1. WS hello（离线模式 tosuOnline=false）
    const ws = await wsConnectWithTimeout();
    const hello = await nextFrame(ws, "hello");
    assert(hello.payload && hello.payload.contract === 1, `hello.contract === 1 (got ${hello?.payload?.contract})`);
    assert(hello.payload.tosuOnline === false, `hello.tosuOnline === false (offline)`);

    // 2. POST → song 帧；页面不回 result → 30s 超时 504（超时语义验证）。
    const chartText = JSON.stringify({
        meta: { mode: 0, mode_ext: { column: 4 } },
        time: [{ bpm: 120, beat: [0, 0, 1] }],
        note: [{ type: 0, column: 0, beat: [0, 0, 1] }],
    });
    const timeoutPost = post({
        meta: { title: "Smoke Timeout", artist: "t", level: "Lv.5", keys: 4 },
        chartText,
    });
    const song = await nextFrame(ws, "song");
    assert(song.payload && song.payload.requestId && song.payload.source === "malody", "song 帧带 requestId/source=malody");
    assert(song.payload.identity.startsWith("mdy:"), `identity=mdy:... (${song.payload.identity})`);
    assert(song.payload.modData.speedRate === "1.0", "modData.speedRate=1.0");
    const timeoutResp = await timeoutPost;
    const timeoutBody = await timeoutResp.text();
    assert(timeoutResp.status === 504 && timeoutBody.includes("分析超时"),
        `不回 result -> 30s 超时 504 TIMEOUT (got ${timeoutResp.status}: ${timeoutBody})`);

    // 3. 页面回 result success → POST 200
    const okPost = post({
        meta: { title: "Smoke Test 2", keys: 4 },
        chartText,
    });
    const song2 = await nextFrame(ws, "song");
    ws.send(JSON.stringify({
        v: 1,
        type: "result",
        seq: 1,
        payload: {
            requestId: song2.payload.requestId,
            statusHint: "success",
            activeSource: "malody",
            errors: [],
        },
    }));
    const okResp = await okPost;
    assert(okResp.status === 200, `result success -> POST 200 (got ${okResp.status})`);

    // 4. 页面回 routing-reject → 504 SOURCE_NOT_ACTIVE
    const rejectPost = post({
        meta: { title: "Smoke Test 3", keys: 4 },
        chartText,
    });
    const song3 = await nextFrame(ws, "song");
    ws.send(JSON.stringify({
        v: 1,
        type: "result",
        seq: 2,
        payload: {
            requestId: song3.payload.requestId,
            statusHint: "routing-reject",
            activeSource: "osu",
            errors: ["路由不可用：当前活跃源为 osu"],
        },
    }));
    const rejectResp = await rejectPost;
    const rejectBody = await rejectResp.text();
    assert(rejectResp.status === 504 && rejectBody.includes("路由不可用"),
        `routing-reject -> 504 SOURCE_NOT_ACTIVE (got ${rejectResp.status}: ${rejectBody})`);

    // 5. 离线设置 POST/GET 往返
    const setResp = await fetch(`${BASE}/settings`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ gameClient: "Etterna" }),
    });
    assert(setResp.status === 200, "POST /settings (offline) -> 200");
    const settings1 = await (await fetch(`${BASE}/settings`)).json();
    assert(settings1.gameClient === "Etterna", `GET /settings 回读 gameClient (got ${JSON.stringify(settings1)})`);

    // 6. 不在白名单的 cover → 404
    const cover = await fetch(`${BASE}/cover/nope.png`);
    assert(cover.status === 404, `cover 白名单外 -> 404 (got ${cover.status})`);

    // 7. 目录穿越防护
    const trav = await fetch(`${BASE}/../Cargo.toml`);
    assert(trav.status === 404, `目录穿越 -> 404 (got ${trav.status})`);

    ws.close();
    console.log(`\nSMOKE: ${passed}/${passed + failed} PASS`);
    process.exit(failed > 0 ? 1 : 0);
}

main().catch((e) => {
    console.error(`SMOKE FAIL: ${e.message}`);
    process.exit(1);
});