/**
 * Compute Worker — runs all estimator computation off the main thread.
 *
 * Receives:  { id, osuText, options }
 * Returns:   { id, result } or { id, error }
 *
 * The `id` field is echoed back for request matching in the manager.
 */

import { runAnalysisPipeline } from "../../pipeline/runAnalysisPipeline.js";
import { runSunnyEstimatorFromText } from "../../estimator/sunnyEstimator.js";
import { runDanielEstimatorFromText } from "../../estimator/danielEstimator.js";
import { runAzusaEstimatorFromText } from "../../estimator/azusaEstimator.js";
import { runRoxyEstimatorFromText } from "../../estimator/roxyEstimator.js";

const ESTIMATORS = { Sunny: "Sunny", Daniel: "Daniel", Azusa: "Azusa", Roxy: "Roxy" };

self.onmessage = (event) => {
    const data = event.data || {};

    // pipeline 消息：整段分析（解析/分派/归一化/SunnyWindow/派生/Interlude/Pattern/Ett/Companella 二次 Ett）
    // 一次往返。runAnalysisPipeline 是异步的（ett WASM + interlude），结果经 then 回传。
    if (data.type === "pipeline") {
        const { id, input } = data;
        if (!id || !input || !input.rawText) {
            self.postMessage({ id, error: "Missing pipeline input" });
            return;
        }
        runAnalysisPipeline(input)
            .then((result) => {
                self.postMessage({ id, result }, []);
            })
            .catch((err) => {
                self.postMessage({ id, error: err?.message || String(err) });
            });
        return;
    }

    // 原 4 估算器消息（保留不动）。
    const { id, osuText, options } = data;
    if (!osuText || !id) {
        self.postMessage({ id, error: "Missing osuText or id" });
        return;
    }

    const estimator = String(options?.estimatorAlgorithm || "Sunny").trim();

    try {
        let result = null;
        let actualEstimatorAlgorithm = estimator;

        if (estimator === "Daniel") {
            result = runDanielEstimatorFromText(osuText, options);
        } else if (estimator === "Azusa") {
            const azusaOpts = {
                ...options,
                forceSunnyReferenceHo: options?.forceSunnyReferenceHo ?? true,
            };
            result = runAzusaEstimatorFromText(osuText, azusaOpts);
            if (!isValidResult(result)) {
                result = runSunnyEstimatorFromText(osuText, options);
                actualEstimatorAlgorithm = "Sunny";
            }
        } else if (estimator === ESTIMATORS.Roxy) {
            result = runRoxyEstimatorFromText(osuText, options);
            if (!isValidResult(result)) {
                result = runSunnyEstimatorFromText(osuText, options);
                actualEstimatorAlgorithm = "Sunny";
            }
        } else {
            result = runSunnyEstimatorFromText(osuText, options);
            actualEstimatorAlgorithm = "Sunny";
        }

        if (result && typeof result === "object") {
            result = { ...result, actualEstimatorAlgorithm };
        }
        self.postMessage({ id, result }, []);
    } catch (err) {
        self.postMessage({ id, error: err?.message || String(err) });
    }
};

function isValidResult(r) {
    return Boolean(r)
        && Number.isFinite(r.star)
        && Number.isFinite(r.numericDifficulty)
        && typeof r.estDiff === "string";
}
