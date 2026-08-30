// Etterna 桥轮询冒烟：fake Etterna 根 + 桥文件 → song 帧 / state / cover 白名单。
// 前置：mma-shell 已启动（MMA_ETTERNA_ROOT=fake 根、MMA_SKIP_TOSU_PROBE=1）。
// 运行：node desktop/scripts/smoke-etterna.mjs <fakeEtternaRoot>

const BASE = "http://127.0.0.1:24061";
const fakeRoot = process.argv[2];
if (!fakeRoot) {
    console.error("usage: node smoke-etterna.mjs <fakeEtternaRoot>");
    process.exit(1);
}
const { readFileSync, existsSync } = await import("node:fs");
const { join } = await import("node:path");

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

const ws = new Promise((resolve, reject) => {
    const w = new WebSocket("ws://127.0.0.1:24061/ws");
    const t = setTimeout(() => reject(new Error("ws timeout")), 5000);
    w.addEventListener("open", () => {
        clearTimeout(t);
        resolve(w);
    });
    w.addEventListener("error", () => reject(new Error("ws error")));
}).then((w) => {
    const waiters = [];
    const listener = (ev) => {
        let frame;
        try {
            frame = JSON.parse(ev.data);
        } catch {
            return;
        }
        for (let i = waiters.length - 1; i >= 0; i--) {
            if (waiters[i].type === frame.type) {
                waiters[i].resolve(frame);
                waiters.splice(i, 1);
            }
        }
    };
    w.addEventListener("message", listener);
    return {
        w,
        next(type, timeoutMs = 8000) {
            return new Promise((resolve, reject) => {
                const timer = setTimeout(() => {
                    const idx = waiters.findIndex((x) => x.type === type);
                    if (idx >= 0) waiters.splice(idx, 1);
                    reject(new Error(`frame ${type} timeout`));
                }, timeoutMs);
                waiters.push({ type, resolve: (f) => { clearTimeout(timer); resolve(f); } });
            });
        },
    };
});

try {
    // 连接确认（hello）后，模拟游戏写入桥文件 → 触发轮询 song 帧。
    const wsConn = await ws;
    await wsConn.next("hello");
    const { writeFileSync } = await import("node:fs");
    const saveDir = join(fakeRoot, "Save");
    writeFileSync(join(saveDir, "LeosMmaBridge.txt"),
        "title=Simple Test\nartist=Tester\nsong_dir=Songs/Demo/\nstep_file=Test.sm\n" +
        "difficulty=Hard\nmeter=8\nrate=1.5\ncover=Songs/Demo/coolbg.png\n" +
        "msd_1=10.0\nmsd_2=0\nmsd_3=0\nmsd_4=0\nmsd_5=0\nmsd_6=0\nmsd_7=0\nmsd_8=0\n", "ascii");
    writeFileSync(join(saveDir, "LeosMmaGameplay.txt"),
        "playing=1\nmusic_seconds=3.2\ntotal_seconds=120\nrate=1.5\n", "ascii");

    // 桥文件应触发 song 帧（identity=ett:...）
    const songBefore = await wsConn.next("song");
    const payload = songBefore.payload;
    assert(payload.source === "etterna", `source=etterna`);
    assert(payload.identity.startsWith("ett:"), `identity=ett:... (${payload.identity})`);
    assert(payload.modData.speedRate === "1.5", `speedRate=1.5 (got ${payload.modData.speedRate})`);
    assert(payload.meta.devMsd8.length === 8, "meta.devMsd8 8 项（开发对照）");
    assert(payload.rawText.includes("#TITLE"), `rawText 为 .sm 原文`);
    assert(payload.cover && payload.cover.url.startsWith("/cover/"), `cover URL: ${payload.cover?.url}`);

    // cover 白名单（绝对路径）可访问
    const cover = await fetch(`${BASE}${payload.cover.url}`);
    assert(cover.status === 200, `cover GET 200 (got ${cover.status})`);
    const notAllowed = await fetch(`${BASE}/cover/C:/Windows/win.ini`);
    assert(notAllowed.status === 404, `未白名单 cover -> 404`);

    // state：etterna.alive/playing（玩游戏写 gameplay）
    const state = await wsConn.next("state", 20000);
    const et = state.payload.sources.etterna;
    assert(et.alive === true, `state etterna.alive=true`);
    assert(et.playing === true, `state etterna.playing=true`);
    assert(typeof et.playingExpireAt === "number" && et.playingExpireAt > 0,
        `playingExpireAt 已外推 (${et.playingExpireAt})`);
    wsConn.w.close();
    console.log(`\nSMOKE-ETTERNA: ${passed}/${passed + failed} PASS`);
    process.exit(failed > 0 ? 1 : 0);
} catch (e) {
    console.error(`SMOKE-ETTERNA FAIL: ${e.message}`);
    process.exit(1);
}