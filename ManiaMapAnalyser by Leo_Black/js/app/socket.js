const COMMAND_RETRY_DELAY_MS = 100;
const MAX_COMMAND_ATTEMPTS = 20;
const RECONNECT_DELAY_MS = 1000;

function getCounterLocation() {
    if (typeof window === "undefined") {
        return "";
    }

    if (typeof window.COUNTER_PATH === "string" && window.COUNTER_PATH.trim()) {
        return window.COUNTER_PATH.trim();
    }

    return `${window.location?.pathname || "/"}${window.location?.search || ""}`;
}

function getWebSocketProtocol() {
    return typeof window !== "undefined" && window.location?.protocol === "https:"
        ? "wss"
        : "ws";
}

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
            let connection;
            try {
                const location = encodeURIComponent(getCounterLocation());
                connection = new WebSocket(`${getWebSocketProtocol()}://${this.host}${url}?l=${location}`);
            } catch (error) {
                console.error("[CONNECTION_ERROR]", error);
                reconnectTimer = setTimeout(connect, RECONNECT_DELAY_MS);
                return;
            }
            this.sockets[url] = connection;

            connection.onopen = () => {
                if (reconnectTimer) clearTimeout(reconnectTimer);
                if (Array.isArray(filters)) {
                    connection.send(`applyFilters:${JSON.stringify(filters)}`);
                }
            };

            connection.onclose = () => {
                if (this.sockets[url] === connection) {
                    delete this.sockets[url];
                }
                reconnectTimer = setTimeout(connect, RECONNECT_DELAY_MS);
            };

            connection.onmessage = (event) => {
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

    sendCommand(name, command, attempt = 1) {
        const commandSocket = this.sockets["/websocket/commands"];
        if (!commandSocket || commandSocket.readyState !== 1) {
            if (attempt < MAX_COMMAND_ATTEMPTS) {
                setTimeout(() => {
                    this.sendCommand(name, command, attempt + 1);
                }, COMMAND_RETRY_DELAY_MS);
            } else {
                console.error(`[COMMAND_ERROR] ${name}: command socket unavailable`);
            }
            return false;
        }

        try {
            const payload = typeof command === "object" ? JSON.stringify(command) : command;
            commandSocket.send(`${name}:${payload}`);
            return true;
        } catch (error) {
            if (attempt < MAX_COMMAND_ATTEMPTS) {
                setTimeout(() => {
                    this.sendCommand(name, command, attempt + 1);
                }, COMMAND_RETRY_DELAY_MS);
                return false;
            }
            console.error("[COMMAND_ERROR]", error);
            return false;
        }
    }
}

export default WebSocketManager;
