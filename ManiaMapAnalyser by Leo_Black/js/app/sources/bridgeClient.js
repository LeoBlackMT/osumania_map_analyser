// 壳桥客户端（浏览器专属）：探测 24061 WS、帧解析、重连、hello 复位、result 发送。
//
// 浏览器模式（壳未启动）：连接失败 → 自动周期性重试，外部源不可用（osu 单源）。
// 契约版本不匹配：呈现终态（state.shellContractMismatch）并停止重连。

import { state } from "../appContext.js";

const BRIDGE_WS_URL = "ws://127.0.0.1:24061/ws";
const RECONNECT_DELAY_MS = 3000;
const CONTRACT_VERSION = 2;

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

/** 初始化壳桥（handlers: {onHello, onState, onSong, onSettings}）。 */
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
            contractOk = payload.contract === CONTRACT_VERSION;
            if (!contractOk) {
                // 契约不匹配：终态提示并停止重连（防无限握手循环）。
                stopped = true;
                syncState();
                console.warn(`mma shell: contract mismatch (got ${payload.contract}, expected ${CONTRACT_VERSION})`);
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
        case "settings":
            if (handlers.onSettings) {
                handlers.onSettings(payload);
            }
            break;
        default:
            break; // ping 等：连接存活即足够
    }
}