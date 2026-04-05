export const APP_CONFIG = {
    endpoint: "http://localhost:24050/files/beatmap/file",
    socketHost: "localhost:24050",

    options: {
        contentBar: ["None", "Auto", "Pattern", "Etterna", "Graph"],
        srText: ["Auto", "ReworkSR", "MSD", "Pattern"],
        diffText: ["None", "Graph", "Difficulty", "MSD", "Pattern", "ReworkSR"],
        estimatorAlgorithm: ["Sunny", "Daniel"],
        modeTag: ["RC", "LN", "HB", "Mix", "SV"],
    },

    starStops: {
        background: [
            [0.0, "#aaaaaa"],
            [0.9, "#4bb3fd"],
            [2.0, "#4fffd5"],
            [3.0, "#d3f557"],
            [4.0, "#fda265"],
            [5.0, "#f94d79"],
            [6.0, "#b64cc1"],
            [7.0, "#5654ca"],
            [8.0, "#14117d"],
            [9.0, "#000000"],
            [9999.9, "#000000"],
        ],
        text: [
            [0.0, "#000000"],
            [6.49, "#000000"],
            [6.5, "#ffd966"],
            [8.9, "#ffd966"],
            [9.0, "#f6f05c"],
            [10.0, "#ff7a69"],
            [11.0, "#e74a95"],
            [12.0, "#9a57ce"],
            [12.39, "#6563de"],
            [9999.9, "#6563de"],
        ],
    },

    etterna: {
        maxSkillValue: 41.0,
        vibroJackspeedRatioThreshold: 0.93,
    },

    graph: {
        viewboxWidth: 260,
        viewboxHeight: 86,
        paddingX: 6,
        paddingTop: 8,
        paddingBottom: 6,
        resampleIntervalMs: 100,
        loadingSampleCount: 20,
        loadingWaveCycles: 2.5,
        loadingWaveAmplitude: 7,
        loadingBaseOffset: 22,
        pauseLineColor: "#FF3B3B",
        pauseLineWidth: 2,
    },

    timing: {
        songTimeJumpThresholdMs: 2000,
        noteEndMarginMs: 500,
        socketRecalcLazyDelayMs: 200,
        settingsCommandTimeoutMs: 1500,
        pauseDetectEpsilonMs: 0,
    },

    defaults: {
        wsEndpoint: "localhost:24050",
        autoMode: false,
        useDanielAlgorithm: false,
        estimatorAlgorithm: "Sunny",
        pauseDetectionEnabled: true,
        enableEtternaRainbowBars: true,
        vibroDetection: true,
        disableVibroDetection: false,
        svDetection: false,
        showModeTagCapsule: true,
        enableNumericDifficulty: false,
        hideCardDuringPlay: false,
        contentBar: "Pattern",
        srText: "ReworkSR",
        diffText: "Difficulty",
        debugUseAmount: false,
    },

    mods: {
        knownCodes: ["DA", "NC", "DT", "HT", "HR", "EZ", "DC", "IN", "HO", "MR"],
        bitFlags: {
            EZ: 2,
            HR: 16,
            DT: 64,
            HT: 256,
            NC: 512,
        },
    },
};
