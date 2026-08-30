// Malody skin 状态写入冒烟：POST → result(malody) → mma_state.txt 内容校验；
// result(etterna)/errors 不写。预备：mma-shell 已启动（MMA_SKIP_TOSU_PROBE=1、
// MMA_MALODY_ROOT=含 mma.txt 哨兵皮肤的 fake 根）。
// 运行：node desktop/scripts/smoke-malody-skin.mjs <fakeMalodyRoot>

const BASE = "http://127.0.0.1:24061";
const POST_URL = "http://127.0.0.1:24060";
const root = process.argv[2];
if (!root) {
    console.error("usage: node smoke-malody-skin.mjs <fakeMalodyRoot>");
    process.exit(1);
}
const { readFileSync, existsSync } = await import("node:fs");
const { join } = await import("node:path");
const chartText = JSON.stringify({
    meta: { mode: 0, mode_ext: { column: 4 } },
    time: [{ bpm: 120, beat: [0, 0, 1] }],
    note: [{ type: 0, column: 0, beat: [0, 0, 1] }],
});

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

function wsNext(ws, type, timeoutMs = 8000) {
    return new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error(`frame ${type} timeout`)), timeoutMs);
        const onMsg = (ev) => {
            let f;
            try {
                f = JSON.parse(ev.data);
            } catch {
                return;
            }
            if (f.type === type) {
                clearTimeout(timer);
                ws.removeEventListener("message", onMsg);
                resolve(f);
            }
        };
        ws.addEventListener("message", onMsg);
    });
}

async function postChart(title) {
    const res = await fetch(POST_URL, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ meta: { title, keys: 4 }, chartText }),
    });
    return res;
}

const stateFile = join(root, "skin", "MySkin", "mma_state.txt");

try {
    const ws = new WebSocket("ws://127.0.0.1:24061/ws");
    await new Promise((res, rej) => {
        ws.addEventListener("open", res);
        ws.addEventListener("error", () => rej(new Error("ws error")));
    });
    await wsNext(ws, "hello");

    // 1. result(malody, success) → 写入
    const p1 = postChart("Skin Test 1");
    const song1 = await wsNext(ws, "song");
    ws.send(JSON.stringify({
        v: 1, type: "result", seq: 1,
        payload: { requestId: song1.payload.requestId, statusHint: "success", activeSource: "malody", errors: [], star: 7.32, pattern: "RC", updatedAt: 1234567890 },
    }));
    assert((await p1).status === 200, "malody success -> POST 200");
    await new Promise((r) => setTimeout(r, 300)); // 等原子写
    const content1 = existsSync(stateFile) ? readFileSync(stateFile, "utf-8") : "";
    assert(content1.includes("star=7.32") && content1.includes("pattern=RC"),
        `mma_state.txt 含 star/pattern (${content1.trim().split("\n").slice(0, 2).join(" / ")})`);
    assert(content1.includes("client=malody"), "client=malody");

    // 2. result(etterna, success) → 不覆盖（写门 activeSource=malody）
    const p2 = postChart("Skin Test 2");
    const song2 = await wsNext(ws, "song");
    ws.send(JSON.stringify({
        v: 1, type: "result", seq: 2,
        payload: { requestId: song2.payload.requestId, statusHint: "success", activeSource: "etterna", errors: [], star: 7.32, pattern: "RC", updatedAt: 2222222222 },
    }));
    assert((await p2).status === 200, "etterna success -> POST 200（结果仍应答）");
    await new Promise((r) => setTimeout(r, 300));
    const content2 = readFileSync(stateFile, "utf-8");
    assert(content2.includes("client=malody"), "etterna 分析不覆盖 skin 文件");

    // 3. result(malody, errors) → 不写
    const p3 = postChart("Skin Test 3");
    const song3 = await wsNext(ws, "song");
    ws.send(JSON.stringify({
        v: 1, type: "result", seq: 3,
        payload: { requestId: song3.payload.requestId, statusHint: "analysis-failed", activeSource: "malody", errors: ["boom"], updatedAt: 3333333333 },
    }));
    assert((await p3).status === 500, "malody errors -> POST 500");
    await new Promise((r) => setTimeout(r, 300));
    const content3 = readFileSync(stateFile, "utf-8");
    assert(!content3.includes("3333333333"), "errors 非空不写 skin 文件");

    // 4. 无哨兵皮肤目录不写
    const p4 = postChart("Skin Test 4");
    const song4 = await wsNext(ws, "song");
    ws.send(JSON.stringify({
        v: 1, type: "result", seq: 4,
        payload: { requestId: song4.payload.requestId, statusHint: "success", activeSource: "malody", errors: [], star: 9.9, updatedAt: 4444444444 },
    }));
    assert((await p4).status === 200, "success -> POST 200");
    await new Promise((r) => setTimeout(r, 300));
    const noSentinel = join(root, "skin", "OtherSkin", "mma_state.txt");
    assert(!existsSync(noSentinel), "无哨兵皮肤目录不写入");

    ws.close();
    console.log(`\nSMOKE-MALODY-SKIN: ${passed}/${passed + failed} PASS`);
    process.exit(failed > 0 ? 1 : 0);
} catch (e) {
    console.error(`SMOKE-MALODY-SKIN FAIL: ${e.message}`);
    process.exit(1);
}