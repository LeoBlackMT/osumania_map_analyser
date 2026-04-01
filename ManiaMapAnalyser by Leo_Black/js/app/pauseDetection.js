export function computePauseTransition({
    previousTimeMs,
    currentTimeMs,
    isPaused,
    jumpThresholdMs,
    noteEndMarginMs,
    timelineStartMs,
    timelineEndMs,
    epsilonMs = 0,
}) {
    const prev = Number(previousTimeMs);
    const now = Number(currentTimeMs);
    const threshold = Number(jumpThresholdMs);
    const margin = Number(noteEndMarginMs);
    const epsilon = Math.max(0, Number(epsilonMs) || 0);

    if (!Number.isFinite(prev) || !Number.isFinite(now) || !Number.isFinite(threshold) || threshold <= 0) {
        return {
            jumped: false,
            atEnd: false,
            sameTime: false,
            nextPaused: Boolean(isPaused),
            shouldAddMarker: false,
            shouldClearMarkers: false,
            frozenInterpMs: null,
            pauseTimeMs: 0,
        };
    }

    const hasEnd = Number.isFinite(timelineEndMs);
    const atEnd = hasEnd && now >= (Number(timelineEndMs) - (Number.isFinite(margin) ? margin : 0));
    const hasStart = Number.isFinite(timelineStartMs);
    const beforeStart = hasStart && now < Number(timelineStartMs);

    const timeDelta = now - prev;
    const jumped = Math.abs(timeDelta) > threshold && !(timeDelta > 0 && timeDelta < threshold);
    const sameTime = Math.abs(timeDelta) <= epsilon;

    let nextPaused = Boolean(isPaused);
    let shouldAddMarker = false;
    let shouldClearMarkers = false;
    let frozenInterpMs = null;
    let pauseTimeMs = 0;

    if (jumped && !atEnd && !beforeStart) {
        nextPaused = false;
        shouldClearMarkers = true;
    } else if (sameTime && !atEnd && !beforeStart) {
        if (!nextPaused) {
            nextPaused = true;
            shouldAddMarker = true;
            frozenInterpMs = now;
            pauseTimeMs = now;
        }
    } else if (nextPaused) {
        nextPaused = false;
    }

    return {
        jumped,
        atEnd,
        sameTime,
        nextPaused,
        shouldAddMarker,
        shouldClearMarkers,
        frozenInterpMs,
        pauseTimeMs,
    };
}
