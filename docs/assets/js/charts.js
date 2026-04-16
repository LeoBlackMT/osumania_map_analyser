import { BAND_META, BAND_ORDER } from "./stats.js";

const BAND_COLORS = {
    exact: "#5ee8bd",
    close: "#7ad1ff",
    moderate: "#f9bb5d",
    miss: "#ff6f7d",
};

const diagonalPlugin = {
    id: "benchmarkDiagonalReference",
    afterDraw(chart, _args, options) {
        if (!options || options.enabled !== true) {
            return;
        }

        const x = chart.scales.x;
        const y = chart.scales.y;
        if (!x || !y) {
            return;
        }

        const min = Math.max(x.min, y.min);
        const max = Math.min(x.max, y.max);

        const ctx = chart.ctx;
        ctx.save();
        ctx.strokeStyle = "rgba(159, 176, 218, 0.9)";
        ctx.setLineDash([6, 6]);
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.moveTo(x.getPixelForValue(min), y.getPixelForValue(min));
        ctx.lineTo(x.getPixelForValue(max), y.getPixelForValue(max));
        ctx.stroke();
        ctx.restore();
    },
};

let pluginRegistered = false;

function ensurePluginRegistered(ChartRef) {
    if (!pluginRegistered) {
        ChartRef.register(diagonalPlugin);
        pluginRegistered = true;
    }
}

function alphaColor(hex, alphaHex) {
    return `${hex}${alphaHex}`;
}

export class BenchmarkCharts {
    constructor(canvasIds) {
        this.canvasIds = canvasIds;
        this.instances = [];

        this.ChartRef = globalThis.Chart;
        if (!this.ChartRef) {
            throw new Error("Chart.js is not loaded");
        }

        ensurePluginRegistered(this.ChartRef);
        this.applyGlobalDefaults();
    }

    applyGlobalDefaults() {
        this.ChartRef.defaults.color = "#9fb0da";
        this.ChartRef.defaults.borderColor = "rgba(153, 180, 255, 0.18)";
        this.ChartRef.defaults.font.family = "Outfit, Segoe UI, sans-serif";
        this.ChartRef.defaults.font.size = 12;
        this.ChartRef.defaults.maintainAspectRatio = false;
    }

    destroy() {
        while (this.instances.length > 0) {
            const chart = this.instances.pop();
            chart.destroy();
        }
    }

    render(summary, compareSummary = null) {
        this.destroy();

        this.renderAccuracyBreakdown(summary);
        this.renderScatter(summary);
        this.renderDeltaDistribution(summary);
        this.renderTrend(summary);
        this.renderPattern(summary);

        if (this.hasCanvas(this.canvasIds.subPattern)) {
            this.renderSubPattern(summary);
        }

        if (this.hasCanvas(this.canvasIds.headToHead)) {
            this.renderHeadToHead(compareSummary);
        }
    }

    register(chart) {
        this.instances.push(chart);
        return chart;
    }

    getCanvas(id) {
        const element = document.getElementById(id);
        if (!element) {
            throw new Error(`Canvas not found: ${id}`);
        }
        return element;
    }

    hasCanvas(id) {
        if (!id) {
            return false;
        }
        return Boolean(document.getElementById(id));
    }

    renderAccuracyBreakdown(summary) {
        const labels = BAND_ORDER.map((key) => `${BAND_META[key].label}`);
        const values = BAND_ORDER.map((key) => summary.bandCounts[key] || 0);

        const chart = new this.ChartRef(this.getCanvas(this.canvasIds.accuracy), {
            type: "doughnut",
            data: {
                labels,
                datasets: [{
                    data: values,
                    backgroundColor: BAND_ORDER.map((key) => BAND_COLORS[key]),
                    borderColor: "rgba(10, 15, 28, 0.95)",
                    borderWidth: 2,
                }],
            },
            options: {
                plugins: {
                    legend: {
                        position: "bottom",
                        labels: {
                            color: "#eef3ff",
                        },
                    },
                    tooltip: {
                        callbacks: {
                            label: (context) => {
                                const count = Number(context.raw || 0);
                                const total = summary.validRows || 1;
                                const rate = ((count / total) * 100).toFixed(1);
                                return ` ${context.label}: ${count} (${rate}%)`;
                            },
                        },
                    },
                },
            },
        });

        this.register(chart);
    }

    renderScatter(summary) {
        const points = BAND_ORDER.flatMap((key) => summary.scatterByBand[key] || []);
        const values = points.flatMap((row) => [row.expected, row.got]);
        const min = values.length ? Math.min(...values) - 0.6 : 0;
        const max = values.length ? Math.max(...values) + 0.6 : 10;

        const datasets = BAND_ORDER.map((key) => ({
            label: BAND_META[key].label,
            data: (summary.scatterByBand[key] || []).map((row) => ({
                x: row.expected,
                y: row.got,
                row,
            })),
            backgroundColor: alphaColor(BAND_COLORS[key], "c8"),
            borderColor: BAND_COLORS[key],
            borderWidth: 0,
            pointRadius: 4,
            pointHoverRadius: 6,
        }));

        const chart = new this.ChartRef(this.getCanvas(this.canvasIds.scatter), {
            type: "scatter",
            data: {
                datasets,
            },
            options: {
                plugins: {
                    benchmarkDiagonalReference: {
                        enabled: true,
                    },
                    legend: {
                        labels: {
                            color: "#eef3ff",
                        },
                    },
                    tooltip: {
                        callbacks: {
                            title: (items) => {
                                const names = [...new Set(
                                    (items || [])
                                        .map((item) => item?.raw?.row?.name)
                                        .filter((name) => Boolean(name)),
                                )];

                                if (!names.length) {
                                    return "Unknown";
                                }

                                if (names.length === 1) {
                                    return names[0];
                                }

                                return `${names.length} maps overlapped`;
                            },
                            label: (item) => {
                                const row = item.raw.row;
                                return `${row.name} | expected ${row.expected.toFixed(2)} | got ${row.got.toFixed(2)} | delta ${row.delta.toFixed(2)} | pattern ${row.pattern || "-"}`;
                            },
                        },
                    },
                },
                scales: {
                    x: {
                        min,
                        max,
                        title: {
                            display: true,
                            text: "Expected",
                        },
                    },
                    y: {
                        min,
                        max,
                        title: {
                            display: true,
                            text: "Got",
                        },
                    },
                },
            },
        });

        this.register(chart);
    }

    renderDeltaDistribution(summary) {
        const chart = new this.ChartRef(this.getCanvas(this.canvasIds.deltaDistribution), {
            type: "bar",
            data: {
                labels: summary.deltaHistogram.labels,
                datasets: [{
                    label: "Rows",
                    data: summary.deltaHistogram.counts,
                    backgroundColor: "rgba(128, 179, 255, 0.55)",
                    borderColor: "rgba(128, 179, 255, 0.9)",
                    borderWidth: 1,
                }],
            },
            options: {
                plugins: {
                    legend: {
                        display: false,
                    },
                },
                scales: {
                    x: {
                        ticks: {
                            maxRotation: 0,
                            minRotation: 0,
                            autoSkip: true,
                            maxTicksLimit: 12,
                        },
                    },
                    y: {
                        beginAtZero: true,
                    },
                },
            },
        });

        this.register(chart);
    }

    renderTrend(summary) {
        const rows = summary.trendRows;
        const labels = rows.map((_row, index) => `${index + 1}`);

        const chart = new this.ChartRef(this.getCanvas(this.canvasIds.trend), {
            type: "line",
            data: {
                labels,
                datasets: [
                    {
                        label: "Expected",
                        data: rows.map((row) => row.expected),
                        borderColor: "#7ad1ff",
                        backgroundColor: "rgba(122, 209, 255, 0.12)",
                        borderWidth: 2,
                        pointRadius: 0,
                        tension: 0.26,
                    },
                    {
                        label: "Got",
                        data: rows.map((row) => row.got),
                        borderColor: "#5ee8bd",
                        backgroundColor: "rgba(94, 232, 189, 0.12)",
                        borderWidth: 2,
                        pointRadius: 0,
                        tension: 0.26,
                    },
                ],
            },
            options: {
                interaction: {
                    mode: "index",
                    intersect: false,
                },
                plugins: {
                    legend: {
                        labels: {
                            color: "#eef3ff",
                        },
                    },
                    tooltip: {
                        callbacks: {
                            title: (items) => {
                                const index = Number(items[0]?.dataIndex || 0);
                                return rows[index]?.name || "Row";
                            },
                        },
                    },
                },
                scales: {
                    x: {
                        ticks: {
                            maxTicksLimit: 14,
                        },
                        title: {
                            display: true,
                            text: "Row index (sorted by expected)",
                        },
                    },
                    y: {
                        title: {
                            display: true,
                            text: "Difficulty value",
                        },
                    },
                },
            },
        });

        this.register(chart);
    }

    renderPattern(summary) {
        const topPatterns = summary.patternRows.slice(0, 12).reverse();

        const chart = new this.ChartRef(this.getCanvas(this.canvasIds.pattern), {
            type: "bar",
            data: {
                labels: topPatterns.map((row) => row.pattern),
                datasets: [{
                    label: "MAE",
                    data: topPatterns.map((row) => Number(row.mae.toFixed(4))),
                    backgroundColor: "rgba(249, 187, 93, 0.55)",
                    borderColor: "rgba(249, 187, 93, 0.95)",
                    borderWidth: 1,
                }],
            },
            options: {
                indexAxis: "y",
                plugins: {
                    legend: {
                        display: false,
                    },
                    tooltip: {
                        callbacks: {
                            label: (context) => {
                                const row = topPatterns[context.dataIndex];
                                return ` MAE ${Number(context.raw).toFixed(3)} | bias ${row.bias.toFixed(3)} | n=${row.count}`;
                            },
                        },
                    },
                },
                scales: {
                    x: {
                        beginAtZero: true,
                        title: {
                            display: true,
                            text: "MAE",
                        },
                    },
                },
            },
        });

        this.register(chart);
    }

    renderSubPattern(summary) {
        const topSubPatterns = (summary.subPatternRows || []).slice(0, 12).reverse();

        const chart = new this.ChartRef(this.getCanvas(this.canvasIds.subPattern), {
            type: "bar",
            data: {
                labels: topSubPatterns.map((row) => row.subPattern),
                datasets: [{
                    label: "MAE",
                    data: topSubPatterns.map((row) => Number(row.mae.toFixed(4))),
                    backgroundColor: "rgba(94, 232, 189, 0.55)",
                    borderColor: "rgba(94, 232, 189, 0.95)",
                    borderWidth: 1,
                }],
            },
            options: {
                indexAxis: "y",
                plugins: {
                    legend: {
                        display: false,
                    },
                    tooltip: {
                        callbacks: {
                            label: (context) => {
                                const row = topSubPatterns[context.dataIndex];
                                return ` MAE ${Number(context.raw).toFixed(3)} | bias ${row.bias.toFixed(3)} | n=${row.count}`;
                            },
                        },
                    },
                },
                scales: {
                    x: {
                        beginAtZero: true,
                        title: {
                            display: true,
                            text: "MAE",
                        },
                    },
                },
            },
        });

        this.register(chart);
    }

    renderHeadToHead(compareSummary) {
        const points = compareSummary?.points || [];
        const values = points.flatMap((point) => [point.x, point.y]);
        const min = values.length ? Math.max(0, Math.min(...values) - 0.2) : 0;
        const max = values.length ? Math.max(...values) + 0.2 : 5;

        const chart = new this.ChartRef(this.getCanvas(this.canvasIds.headToHead), {
            type: "scatter",
            data: {
                datasets: [{
                    label: "Rows",
                    data: points,
                    backgroundColor: "rgba(128, 179, 255, 0.78)",
                    borderColor: "rgba(128, 179, 255, 0.95)",
                    borderWidth: 0,
                    pointRadius: 4,
                    pointHoverRadius: 6,
                }],
            },
            options: {
                plugins: {
                    benchmarkDiagonalReference: {
                        enabled: true,
                    },
                    legend: {
                        display: false,
                    },
                    tooltip: {
                        callbacks: {
                            title: (items) => {
                                const names = [...new Set(
                                    (items || [])
                                        .map((item) => item?.raw?.name)
                                        .filter((name) => Boolean(name)),
                                )];

                                if (!names.length) {
                                    return "Unknown";
                                }

                                if (names.length === 1) {
                                    return names[0];
                                }

                                return `${names.length} maps overlapped`;
                            },
                            label: (item) => {
                                const point = item.raw;
                                return `${point.name} | base |delta| ${Number(point.x).toFixed(2)} | compare |delta| ${Number(point.y).toFixed(2)} | pattern ${point.pattern || "-"}`;
                            },
                        },
                    },
                },
                scales: {
                    x: {
                        min,
                        max,
                        title: {
                            display: true,
                            text: "Base |Delta|",
                        },
                    },
                    y: {
                        min,
                        max,
                        title: {
                            display: true,
                            text: "Compare |Delta|",
                        },
                    },
                },
            },
        });

        this.register(chart);
    }
}
