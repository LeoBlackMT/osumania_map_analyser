// js/laneAcc/index.js —— Mania LaneAcc 独立计分与回放解析模型
//
// 纯逻辑领域模型：包含 ScoreV1 状态机累加器、ReplayFrame 采样帧解析、
// 谱面按键分轨对象构建与命中窗口换算、准确度格式化。
// 零 DOM 依赖，三个运行时（浏览器/桌面壳/Node）皆可直接调用。

export {
    createAccumulator,
    normalizeSpaceMode,
    HIT_WINDOWS_MANIA_V1,
    KEY_MAX,
    KEY_MIN,
} from "./accumulator.js";

export {
    framesToSv1Events,
    sampleFramesAtPlayback,
    sortReplayFrames,
} from "./rawFrames.js";

export {
    buildChart,
    handGroups,
    HAND_SPLITS,
} from "./chart.js";

export {
    formatAcc,
    formatMiss,
} from "./format.js";
