import assert from "node:assert/strict";
import fs from "node:fs";

import { calculateClusteredPatterns } from "../ManiaMapAnalyser by Leo_Black/js/patterns/clustering.js";
import { NoteType as PatternNoteType } from "../ManiaMapAnalyser by Leo_Black/js/patterns/chart.js";
import { parseOsuManiaFromText } from "../ManiaMapAnalyser by Leo_Black/js/parser/patternOsuParser.js";
import { analyzePatternFromText } from "../ManiaMapAnalyser by Leo_Black/js/patterns/service.js";
import { buildInterludeRows } from "../ManiaMapAnalyser by Leo_Black/js/interlude/chartBuilder.js";
import { NoteType as InterludeNoteType } from "../ManiaMapAnalyser by Leo_Black/js/interlude/types.js";
import { parseBenchmarkCsv, toNumberOrNull } from "../docs/assets/js/csv.js";
import { computeSummary } from "../docs/assets/js/stats.js";

function beatmap({ keys = 4, objects, mode = 3 }) {
    return `osu file format v14
[General]
Mode:${mode}
[Difficulty]
CircleSize:${keys}
OverallDifficulty:8
[TimingPoints]
0,500,4,2,1,60,1,0
[HitObjects]
${objects}`;
}

function hold(time, endTime, x = 64) {
    return `${x},192,${time},128,0,${endTime}:0:0:0:0:`;
}

function rice(time, x = 64) {
    return `${x},192,${time},1,0,0:0:0:0:`;
}

{
    const pattern = (Start, End) => ({
        Pattern: "Stream",
        SpecificType: null,
        Mixed: false,
        Start,
        End,
        MsPerBeat: 100,
    });
    const [cluster] = calculateClusteredPatterns([pattern(0, 10), pattern(5, 15)]);
    assert.equal(cluster.Amount, 15, "overlapping pattern intervals must be unioned");
}

{
    const tailHead = parseOsuManiaFromText(beatmap({
        objects: `${hold(0, 100)}\n${hold(100, 200)}`,
    }));
    assert.equal(tailHead.Notes.find((row) => row.Time === 100).Data[0], PatternNoteType.HOLDTAIL_HOLDHEAD);

    const tailRice = parseOsuManiaFromText(beatmap({
        objects: `${hold(0, 100)}\n${rice(100)}`,
    }));
    assert.equal(tailRice.Notes.find((row) => row.Time === 100).Data[0], PatternNoteType.HOLDTAIL_NORMAL);
    assert.doesNotThrow(() => analyzePatternFromText(beatmap({
        objects: `${hold(0, 100)}\n${hold(100, 200)}`,
    })));
}

{
    const tenKey = parseOsuManiaFromText(beatmap({ keys: 0, objects: rice(0, 25) }));
    assert.equal(tenKey.Keys, 10);
    assert.equal(tenKey.Notes[0].Data.length, 10);
    assert.throws(
        () => parseOsuManiaFromText(beatmap({ mode: 0, objects: rice(0) })),
        /not mania/i,
    );
}

{
    const tailHead = await buildInterludeRows(beatmap({
        objects: `${hold(0, 100)}\n${hold(100, 200)}`,
    }));
    assert.equal(tailHead.rows.find((row) => row.time === 100).data[0], InterludeNoteType.HOLDTAIL_HOLDHEAD);

    const tailRice = await buildInterludeRows(beatmap({
        objects: `${hold(0, 100)}\n${rice(100)}`,
    }));
    assert.equal(tailRice.rows.find((row) => row.time === 100).data[0], InterludeNoteType.HOLDTAIL_NORMAL);
}

{
    assert.equal(toNumberOrNull(""), null);
    const legacyText = fs.readFileSync(new URL("../docs/data/Legacy.csv", import.meta.url), "utf8");
    const legacy = parseBenchmarkCsv(legacyText);
    const first = legacy.rows[0];
    assert.equal(first.delta, first.expected - first.got);
    assert.equal(first.deltaAbs, Math.abs(first.expected - first.got));

    const summary = computeSummary([{
        name: "direction",
        expected: 5,
        got: 1,
        delta: -4,
        deltaAbs: 4,
        pattern: "test",
        subPattern: "test",
    }]);
    assert.equal(summary.metrics.bias, 4);
}

{
    class FakeWorker {
        static instances = [];

        constructor() {
            this.listeners = { message: [], error: [] };
            this.terminated = false;
            FakeWorker.instances.push(this);
        }

        addEventListener(type, handler) {
            this.listeners[type].push(handler);
        }

        removeEventListener(type, handler) {
            this.listeners[type] = this.listeners[type].filter((entry) => entry !== handler);
        }

        postMessage(message) {
            this.lastMessage = message;
        }

        terminate() {
            this.terminated = true;
        }

        emit(type, data) {
            for (const handler of [...this.listeners[type]]) {
                handler(type === "message" ? { data } : data);
            }
        }
    }

    globalThis.Worker = FakeWorker;
    const { runInWorker } = await import("../ManiaMapAnalyser by Leo_Black/js/app/worker/manager.js?regression");
    const firstPromise = runInWorker("first", {});
    const firstOutcome = firstPromise.then(
        () => null,
        (error) => error,
    );
    const secondPromise = runInWorker("second", {});

    assert.equal(FakeWorker.instances.length, 2);
    assert.equal(FakeWorker.instances[0].terminated, true);
    assert.equal((await firstOutcome).name, "AbortError");

    const current = FakeWorker.instances[1];
    current.emit("message", {
        id: current.lastMessage.id,
        result: { request: "second" },
    });
    assert.deepEqual(await secondPromise, { request: "second" });
    assert.equal(current.listeners.message.length, 0);
    assert.equal(current.listeners.error.length, 0);

    const crashPromise = runInWorker("crash", {}).then(
        () => null,
        (error) => error,
    );
    const crashingWorker = FakeWorker.instances.at(-1);
    crashingWorker.emit("error", { message: "boom" });
    assert.match((await crashPromise).message, /boom/);
    assert.equal(crashingWorker.terminated, true);

    const recoveryPromise = runInWorker("recovery", {});
    const recoveryWorker = FakeWorker.instances.at(-1);
    recoveryWorker.emit("message", {
        id: recoveryWorker.lastMessage.id,
        result: { recovered: true },
    });
    assert.deepEqual(await recoveryPromise, { recovered: true });
}

{
    const originalWindow = globalThis.window;
    const originalWebSocket = globalThis.WebSocket;
    const originalSetTimeout = globalThis.setTimeout;
    const originalConsoleError = console.error;
    const createdUrls = [];

    class FakeWebSocket {
        constructor(url) {
            this.url = url;
            this.readyState = 0;
            createdUrls.push(url);
        }
    }

    globalThis.window = {
        COUNTER_PATH: "/folder name/counter?preview=1&mode=test",
        location: { protocol: "http:", pathname: "/fallback", search: "" },
    };
    globalThis.WebSocket = FakeWebSocket;
    const WebSocketManager = (await import("../ManiaMapAnalyser by Leo_Black/js/app/socket.js?regression")).default;
    const manager = new WebSocketManager("localhost:24050");
    manager.api_v2(() => {});
    assert.match(
        createdUrls[0],
        /l=%2Ffolder%20name%2Fcounter%3Fpreview%3D1%26mode%3Dtest$/,
        "counter location must be encoded as one query value",
    );

    let scheduledRetries = 0;
    let loggedErrors = 0;
    globalThis.setTimeout = (callback) => {
        scheduledRetries += 1;
        callback();
        return scheduledRetries;
    };
    console.error = () => {
        loggedErrors += 1;
    };
    assert.equal(manager.sendCommand("getSettings", "counter"), false);
    assert.equal(scheduledRetries, 19, "command retries must be bounded");
    assert.equal(loggedErrors, 1, "retry exhaustion should be reported once");

    let sentPayload = "";
    manager.sockets["/websocket/commands"] = {
        readyState: 1,
        send(payload) {
            sentPayload = payload;
        },
    };
    assert.equal(manager.sendCommand("getSettings", "counter"), true);
    assert.equal(sentPayload, "getSettings:counter");

    globalThis.window = originalWindow;
    globalThis.WebSocket = originalWebSocket;
    globalThis.setTimeout = originalSetTimeout;
    console.error = originalConsoleError;
}

console.log("regressions: ok");
