class WebSocketManager {
    constructor(host) {
    this.host = host;
    this.sockets = {};
    }

    setHost(host, reconnect = true) {
    const normalized = typeof host === "string" ? host.trim() : "";
    if (!normalized || normalized === this.host) {
            return false;
    }

    this.host = normalized;

    if (reconnect) {
            for (const socket of Object.values(this.sockets)) {
        try {
                    socket.close();
        } catch {
                    // Ignore close errors and rely on reconnect loop.
        }
            }
    }

    return true;
    }

    createConnection(url, callback, filters) {
    let reconnectTimer = null;

    const connect = () => {
            this.sockets[url] = new WebSocket(`ws://${this.host}${url}?l=${encodeURI(window.COUNTER_PATH)}`);

            this.sockets[url].onopen = () => {
        if (reconnectTimer) clearTimeout(reconnectTimer);
        if (Array.isArray(filters)) {
                    this.sockets[url].send(`applyFilters:${JSON.stringify(filters)}`);
        }
            };

            this.sockets[url].onclose = () => {
        delete this.sockets[url];
        reconnectTimer = setTimeout(connect, 1000);
            };

            this.sockets[url].onmessage = (event) => {
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
            setTimeout(() => {
        that.sendCommand(name, command, amountOfRetries + 1);
            }, 100);
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
