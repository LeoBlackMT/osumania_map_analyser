class WebSocketManager {
    constructor(host) {
    this.host = host;
    this.sockets = {};
    // `sockets` 以 URL 为键（`sendCommand` 按 `/websocket/commands` 取用），但同一条 URL
    // 可能被**打开两次**（`settings.js:810` 与 `presets/tosuTransport.js:100` 各开一条
    // `/websocket/commands`）⇒ 后开的会覆盖槽位，被覆盖的那条永远不在 map 里。
    // 切端点时必须连它一起关（否则旧 host 的连接残留、命令投到旧端点），故另记
    // "曾创建过的全部连接"。
    this.allSockets = new Set();
    }

    setHost(host, reconnect = true) {
    const normalized = typeof host === "string" ? host.trim() : "";
    if (!normalized || normalized === this.host) {
            return false;
    }

    this.host = normalized;

    if (reconnect) {
            // 关闭**曾创建过的全部** socket：每条连接自己的 onclose 会按新 host 在 1s 后
            // 重连（`connect()` 每次都读 `this.host`），故不需要在这里重建。
            for (const socket of this.allSockets) {
        try {
                    socket.close();
        } catch {
                    // Ignore close errors and rely on reconnect loop.
        }
            }
            this.allSockets.clear();
    }

    return true;
    }

    createConnection(url, callback, filters) {
    let reconnectTimer = null;

    const connect = () => {
            const ws = new WebSocket(`ws://${this.host}${url}?l=${encodeURI(window.COUNTER_PATH)}`);
            this.sockets[url] = ws;
            this.allSockets.add(ws);

            ws.onopen = () => {
        if (reconnectTimer) clearTimeout(reconnectTimer);
        if (Array.isArray(filters)) {
                    ws.send(`applyFilters:${JSON.stringify(filters)}`);
        }
            };

            ws.onclose = () => {
        // 只清自己的槽位：被同 URL 的新连接覆盖过的旧 connection 关掉时不得把新连接
        // 从槽里删掉（`sendCommand` 会因此找不到命令通道）。
        if (this.sockets[url] === ws) {
                    delete this.sockets[url];
        }
        this.allSockets.delete(ws);
        reconnectTimer = setTimeout(connect, 1000);
            };

            ws.onmessage = (event) => {
        try {
                    const data = JSON.parse(event.data);
                    if (data?.error || data?.message?.error) return;
                    callback(data);
        } catch (error) {
                    console.log("[MESSAGE_ERROR]", error);
        }
            };
    };

    connect();
    }

    api_v2(callback, filters) {
    this.createConnection("/websocket/v2", callback, filters);
    }

    commands(callback) {
    this.createConnection("/websocket/commands", callback);
    }

    sendCommand(name, command, amountOfRetries = 1) {
    const that = this;

    if (!this.sockets["/websocket/commands"]) {
            // Bounded wait for the connection to open. This was an unbounded
            // 100ms retry loop: while the socket was down it spammed tosu with
            // getSettings requests, adding to the broadcast storm that slowed
            // the server to tens of seconds per request. Give up after ~2s —
            // callers have fallbacks (e.g. the presets HTTP pull).
            if (amountOfRetries <= 20) {
            setTimeout(() => {
        that.sendCommand(name, command, amountOfRetries + 1);
            }, 100);
            }
            return;
    }

    try {
            const payload = typeof command === "object" ? JSON.stringify(command) : command;
            this.sockets["/websocket/commands"].send(`${name}:${payload}`);
    } catch (error) {
            if (amountOfRetries <= 3) {
        setTimeout(() => {
                    that.sendCommand(name, command, amountOfRetries + 1);
        }, 1000);
        return;
            }
            console.error("[COMMAND_ERROR]", error);
    }
    }
}

export default WebSocketManager;
