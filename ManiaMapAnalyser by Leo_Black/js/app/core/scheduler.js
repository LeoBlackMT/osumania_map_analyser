import { SOCKET_RECALC_LAZY_DELAY_MS, state } from "./appContext.js";

let recomputeHandler = null;

export function setRecomputeHandler(handler) {
    recomputeHandler = typeof handler === "function" ? handler : null;
}

export function scheduleRecompute(reason, useLazyDelay) {
    if (state.recalcTimerId != null) {
        clearTimeout(state.recalcTimerId);
        state.recalcTimerId = null;
    }

    const run = () => {
        state.recalcTimerId = null;
        if (recomputeHandler) {
            recomputeHandler(reason);
        }
    };

    if (useLazyDelay) {
        state.recalcTimerId = setTimeout(run, SOCKET_RECALC_LAZY_DELAY_MS);
    } else {
        run();
    }
}
