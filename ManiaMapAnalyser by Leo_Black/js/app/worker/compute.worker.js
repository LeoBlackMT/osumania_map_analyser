/**
 * Compute Worker — runs all estimator computation off the main thread.
 *
 * Receives:  { id, osuText, options }
 * Returns:   { id, result } or { id, error }
 *
 * The `id` field is echoed back for request matching in the manager.
 */

import { runSunnyEstimatorFromText } from "../../estimator/sunnyEstimator.js";
import { runDanielEstimatorFromText } from "../../estimator/danielEstimator.js";
import { runAzusaEstimatorFromText } from "../../estimator/azusaEstimator.js";
import { runRoxyEstimatorFromText } from "../../estimator/roxyEstimator.js";
import { runMixedEstimatorFromText } from "../../estimator/mixedEstimator.js";

const ESTIMATORS = {
    Sunny: "Sunny",
    Daniel: "Daniel",
    Azusa: "Azusa",
    Roxy: "Roxy",
    Mixed: "Mixed",
};

self.onmessage = (event) => {
    const { id, osuText, options } = event.data || {};
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
        } else if (estimator === ESTIMATORS.Mixed) {
            result = runMixedEstimatorFromText(osuText, options);
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
