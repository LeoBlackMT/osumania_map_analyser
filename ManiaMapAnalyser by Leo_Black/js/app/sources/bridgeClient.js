// 壳桥客户端（浏览器专属）：探测 24061 WS、帧解析、重连、hello 复位、result 发送。
//
// 浏览器模式（壳未启动）：连接失败 → 自动周期性重试，外部源不可用（osu 单源）。
// 契约版本不匹配：呈现终态（state.shellContractMismatch）并停止重连。

import { state } from "../appContext.js";

const BRIDGE_WS_URL = "ws://127.0.0.1:24061/ws";
const RECONNECT_DELAY_MS = 3000;
/** 页面实现的契约版本（= 发送帧的信封 `v`）。 */
const CONTRACT_VERSION = 5;
/**
 * 页面接受的最低壳契约版本（兼容矩阵见 CONTRACT.md §11.8）。
 *
 * **v3 兼容路径**：v3 壳仍然可用 —— 它的 `state.sources.malody` 只有 `alive`，
 * 桥通道的 `song` 帧也没有 `screen`/`judge`/`winScale`。页面据此退回"Lua 通道 + 旧形态"：
 * 场景/清空/判定三段逻辑在字段缺省时一律不进入（见 shellState.js / externalSource.js）。
 * **v4** 壳有六个 `sources.malody` 字段、`winScale` 恒为数值；**v5** 起 `pro`/`turbo`
 * 随帧下发、`winScale` 可为 `null`（未知）。页面按字段**逐个存在性**降级，不按版本号分支。
 * 只有越界（< 3 或 > 5）才算不匹配。
 *
 * ⚠️ 这个常量必须与壳侧 `desktop/src/frames.rs::CONTRACT_VERSION` 同步：壳升而页面不升，
 * `hello` 会被判为越界 ⇒ `contractOk` 为假 ⇒ `bridgeOnline()` 为假 ⇒ 不只数据帧不通，
 * `sendControl()` 早在首行就返回，**窗口拖动把手与置顶/穿透/关闭快捷键会一起静默失效**。
 */
const MIN_ACCEPTED_CONTRACT = 3;

let socket = null;
let reconnectTimer = 0;
let stopped = false;
let contractOk = false;
let seq = 0;

export function isBridgeConnected() {
    return socket !== null && socket.readyState === WebSocket.OPEN;
}

export function bridgeOnline() {
    return isBridgeConnected() && contractOk;
}

function syncState() {
    state.externalBridgeAvailable = bridgeOnline();
    state.shellContractMismatch = bridgeOnline() === false && stopped;
    // 壳模式 class：贴边布局等仅壳窗口生效（浏览器模式不变）。
    if (typeof document !== "undefined" && document.documentElement) {
        document.documentElement.classList.toggle("shell-mode", bridgeOnline());
    }
}

export function sendResult(payload) {
    if (!bridgeOnline()) {
        return;
    }
    seq += 1;
    const frame = JSON.stringify({ v: CONTRACT_VERSION, type: "result", seq, payload });
    try {
        socket.send(frame);
    } catch {
        // 遥测式静默失败
    }
}

/** 发送窗口控制帧（契约 v2 control；供未来 UI 按钮使用，快捷键走壳全局注册）。 */
export function sendControl(action, value) {
    if (!bridgeOnline()) {
        return;
    }
    seq += 1;
    const frame = JSON.stringify({
        v: CONTRACT_VERSION,
        type: "control",
        seq,
        payload: { action, value },
    });
    try {
        socket.send(frame);
    } catch {
        // 静默失败
    }
}

/** 诊断通道：页面 → 壳（打日志，不参与协议）。 */
export function sendDiag(message) {
    if (!bridgeOnline()) {
        return;
    }
    seq += 1;
    const frame = JSON.stringify({
        v: CONTRACT_VERSION,
        type: "diag",
        seq,
        payload: { message },
    });
    try {
        socket.send(frame);
    } catch {
        // 静默失败
    }
}

/** 初始化壳桥（handlers: {onHello, onState, onSong, onSettings, onMalody4Selection}）。 */
export function initBridgeClient(handlers = {}) {
    if (typeof WebSocket === "undefined") {
        return; // 非浏览器环境（benchmark 等）
    }
    stopped = false;
    connect(handlers);
}

function connect(handlers) {
    if (stopped) {
        return;
    }
    try {
        socket = new WebSocket(BRIDGE_WS_URL);
    } catch {
        scheduleReconnect(handlers);
        return;
    }
    socket.addEventListener("open", syncState);
    socket.addEventListener("message", (ev) => {
        let frame;
        try {
            frame = JSON.parse(ev.data);
        } catch {
            return;
        }
        handleFrame(frame, handlers);
    });
    socket.addEventListener("close", () => {
        socket = null;
        contractOk = false;
        syncState();
        scheduleReconnect(handlers);
    });
    socket.addEventListener("error", () => {
        try {
            socket.close();
        } catch {
            // noop
        }
    });
}

function scheduleReconnect(handlers) {
    if (stopped) {
        return;
    }
    clearTimeout(reconnectTimer);
    reconnectTimer = setTimeout(() => connect(handlers), RECONNECT_DELAY_MS);
}

function handleFrame(frame, handlers) {
    const payload = frame.payload || {};
    switch (frame.type) {
        case "hello": {
            // 接受区间 [MIN_ACCEPTED_CONTRACT, CONTRACT_VERSION]：v4 全功能；v3 接受但页面
            // 不进入桥的场景/清空/判定路径（字段缺省）。仅越界才进终态。
            contractOk = Number.isInteger(payload.contract)
                && payload.contract >= MIN_ACCEPTED_CONTRACT
                && payload.contract <= CONTRACT_VERSION;
            if (!contractOk) {
                // 契约不匹配：终态提示并停止重连（防无限握手循环）。
                stopped = true;
                syncState();
                console.warn(`mma shell: contract mismatch (got ${payload.contract}, expected ${MIN_ACCEPTED_CONTRACT}..${CONTRACT_VERSION})`);
                return;
            }
            syncState();
            if (handlers.onHello) {
                handlers.onHello(payload);
            }
            break;
        }
        case "state":
            if (handlers.onState) {
                handlers.onState(payload);
            }
            break;
        case "song":
            if (handlers.onSong) {
                handlers.onSong(payload);
            }
            break;
        case "malody4_selection":
            if (handlers.onMalody4Selection) {
                handlers.onMalody4Selection(payload);
            }
            break;
        case "settings":
            if (handlers.onSettings) {
                handlers.onSettings(payload);
            }
            break;
        default:
            break; // ping 等：连接存活即足够
    }
}