import { parseOsuManiaFromText } from "../parser/patternOsuParser.js";
import { fromChart } from "./summary.js";

export function analyzePatternFromText(osuText, rate = 1.0) {
    void rate;
    const chart = parseOsuManiaFromText(osuText);
    const report = fromChart(chart);

    return {
    report,
    topFiveClusters: report.Clusters.slice(0, 5),
    };
}
