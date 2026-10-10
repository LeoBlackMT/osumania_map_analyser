/**
 * English localization dictionary for ManiaMapAnalyser settings page.
 * Acts as the reference schema for all translation keys.
 */

export default {
    localeName: "English",
    nav: {
        shellConfig: "Shell Configuration",
        cardSettings: "Card Settings",
        presetsManager: "Presets Manager"
    },
    status: {
        loading: "Loading…",
        shellUnavailable: "Cannot reach the shell (24061)",
        shellOnline: "tosu is connected — settings are read-only here.",
        shellOnlinePresets: "tosu is connected — settings are read-only here. Manage presets from the Presets page of your tosu instance.",
        originNotice: "Open this page from the desktop shell: http://127.0.0.1:24061/settings.html",
        saving: "Saving…",
        saved: "Saved.",
        ready: "Ready",
        saveFailed: "Save failed: ",
        unreadableNotice: "Config file on disk is unreadable or malformed (HTTP 400); changes disabled.",
        refreshingPaths: "Refreshing the adopted paths…",
        tosuDashboard: "tosu dashboard:",
        cardSettingsSub: "The desktop shell keeps these values in mma-settings.json while tosu is offline; the overlay picks up every change immediately."
    },
    headers: {
        hLinks: "Links",
        hPresets: "Presets",
        hModules: "Modules Customization",
        hTheme: "Theme & Effects",
        hFunctions: "Functionality Options",
        hNetwork: "Network Configuration",
        hDebug: "Debug Options"
    },
    shell: {
        title: "Shell Configuration",
        subtitle: "Only the desktop shell reads these values (mma-shell-config.json, next to mma-shell.exe).",
        gameClient: {
            title: "Game client",
            description: "Client mma-shell attaches to. Auto chooses between running osu! (native memory reader), Etterna, Malody V and Malody 4.3.7; choosing a specific client disables auto-detection."
        },
        roots: {
            etternaRoot: {
                title: "Etterna root",
                description: "Folder that holds the Etterna install. Empty = let the shell detect it."
            },
            malodyRoot: {
                title: "Malody V root",
                description: "Folder that holds the Malody V install. Empty = let the shell detect it."
            },
            malody4Root: {
                title: "Malody 4.3.7 root",
                description: "Folder that holds the Malody 4.3.7 install. Empty = let the shell detect it."
            }
        },
        hotkeys: {
            title: "Global hotkeys",
            description: "Format: modifier+modifier+key (e.g. Ctrl+Shift+T). Blank disables the shortcut.",
            topmost: "Toggle topmost",
            clickThrough: "Toggle click-through",
            close: "Close overlay",
            settings: "Open settings window"
        },
        logLevel: {
            title: "Log level",
            description: "Log verbosity of mma-shell.log (written to logs/ alongside the executable)."
        },
        window: {
            title: "Window position & size",
            description: "Current overlay window position and size. Set on launch; saving applies immediately.",
            width: "Width",
            height: "Height",
            x: "X position",
            y: "Y position"
        },
        offsets: {
            title: "Memory Offsets Management",
            description: "State of runtime memory layout tables for osu! (lazer & stable).",
            stableLayout: "Stable Layout: ",
            lazerLayout: "Lazer Layout: ",
            actionsTitle: "Offsets Actions",
            actionsDesc: "Update or generate memory layout tables for osu! clients.",
            liveGenerator: "Live Generator",
            checkRemote: "Check Remote Updates",
            genReadyTitle: "Extract offsets from running osu! with zero .NET SDK",
            genDisabledTitle: "gen.exe not detected",
            updateTitle: "Verify Ed25519 signed manifest and sync updated tables",
            generatingMsg: "Scanning running osu! and generating offsets…",
            updatingMsg: "Checking remote manifest and verifying signatures…",
            genSuccess: "Offsets generated and verified successfully.",
            updateSuccessZero: "Offsets are up to date.",
            updateSuccessMulti: "Updated {count} table(s) successfully."
        },
        shadow: {
            title: "Shadow Diagnostics (Native vs Tosu)",
            descEmpty: "Waiting for compare frames… (play a beatmap in osu! while both native reader and tosu are active)",
            descPopulated: "{rate}% match rate ({matches} matched, {mismatches} mismatched of {total} frames)",
            resetBtn: "Reset Shadow Metrics"
        }
    },
    cardSettings: {
        GuideButton: {
            title: "Settings Guide",
            description: "Go to the GitHub repository for settings usage instructions and more information."
        },
        PresetGuideButton: {
            title: "Presets Guide",
            description: "Go to the GitHub repository for presets usage instructions and more information."
        },
        IssueButton: {
            title: "Report Issue",
            description: "Report any bugs or suggest features on GitHub."
        },
        BenchmarkButton: {
            title: "Benchmark Results",
            description: "View estimator algorithm's benchmark results and performance data."
        },
        preset: {
            title: "Preset",
            description: "Quickly apply built-in presets that overwrite the settings below. Choose LastSavedPreset to follow your manual changes."
        },
        PresetButton: {
            title: "Presets Manage Page",
            description: "Go to the presets.html page to manage and share custom presets."
        },
        contentBar: {
            title: "Card Body Content",
            description: "Select what to show in the card body. Full shows Pattern, Etterna and Graph together.",
            options: {
                "None": "None",
                "Auto": "Auto",
                "Pattern": "Pattern",
                "Etterna": "Etterna",
                "Graph": "Graph",
                "ReworkPP": "Rework PP",
                "Full": "Full"
            }
        },
        srText: {
            title: "Top-left Capsule Text",
            description: "Select what to show in the top-left capsule.",
            options: {
                "Auto": "Auto",
                "ReworkSR": "Rework SR",
                "ReworkPP": "Rework PP",
                "InterludeSR": "Interlude SR",
                "MSD": "MSD",
                "Pattern": "Pattern"
            }
        },
        diffText: {
            title: "Top-right Content",
            description: "Select what to show at top-right of the card.",
            options: {
                "None": "None",
                "Graph": "Graph",
                "Difficulty": "Difficulty",
                "MSD": "MSD",
                "Pattern": "Pattern",
                "ReworkSR": "Rework SR",
                "InterludeSR": "InterludeSR"
            }
        },
        showModeTagCapsule: {
            title: "Map Tag Capsule",
            description: "Show the beatmap's tag capsule (HB/RC/LN/Mix/SV) at bottom-left."
        },
        enableOsuTheme: {
            title: "osu!Lazer Card Theme",
            description: "Use the osu!Lazer-style themed card."
        },
        useOsuFont: {
            title: "osu! Font",
            description: "Use the Osu Torus font on the analysis card only."
        },
        enableFloatingTriangles: {
            title: "Floating Triangles Animation",
            description: "[Requires Lazer Card Theme] Show the animated triangles drifting up behind the card."
        },
        enableCoverArt: {
            title: "Cover Art Background",
            description: "[Requires Lazer Card Theme] Lay the beatmap's background behind the content blocks. Suggested to set a custom background color if disabled."
        },
        customBackgroundColor: {
            title: "Custom Background Color",
            description: "[Requires Lazer Card Theme] Use a custom color instead of sampling the beatmap's background. Set to pure black (#000000) to disable."
        },
        enableEtternaRainbowBars: {
            title: "Rainbow Bars",
            description: "Enable multi-color bars in Etterna mode. Suggested to disable if Lazer theme is enabled."
        },
        enableStatusMarquee: {
            title: "Metadata Marquee",
            description: "Enable horizontal marquee scrolling for long metadata text."
        },
        enableNumericDifficulty: {
            title: "Numeric Difficulty",
            description: "Show numeric difficulty values in RC algorithms."
        },
        enableLNDifficulty: {
            title: "LN Star Rating",
            description: "[Requires Improve Sunny LN Estimation] Show Rework Star Rating values in LN algorithms."
        },
        reverseCardExtendDirection: {
            title: "Reverse Card Extension",
            description: "Anchor card bottom and extend upwards when card needs to extend."
        },
        cardVisibility: {
            title: "Card Visibility",
            description: "Control when the analysis card is shown.",
            options: {
                "DuringPlay": "DuringPlay",
                "OutsidePlay": "OutsidePlay",
                "Always": "Always"
            }
        },
        cardOpacity: {
            title: "Card Opacity",
            description: "Set overall card opacity."
        },
        cardBgBlur: {
            title: "Content Background Blur",
            description: "Gaussian blur strength for the cover art behind Pattern, Etterna, and Graph blocks. Requires Cover Art Theme.",
            options: {
                "Off": "Off",
                "4px": "4px",
                "8px": "8px",
                "12px": "12px",
                "16px": "16px",
                "20px": "20px"
            }
        },
        cardRadius: {
            title: "Card Radius",
            description: "Set card corner roundness.",
            options: {
                "Small": "Small",
                "Medium": "Medium",
                "Large": "Large"
            }
        },
        enableUpdateCheck: {
            title: "Enable Update Check",
            description: "Check latest GitHub release once per day. The star icon is shown only when a newer release is found."
        },
        enableResultCache: {
            title: "Result Cache",
            description: "Cache analysis results to show instantly when revisiting a beatmap. Disable if you suspect stale results."
        },
        enablePauseDetection: {
            title: "Pause Detection",
            description: "Recommended: Count pauses and draw pause markers on the graph when playing."
        },
        VibroDetection: {
            title: "Vibro Detection",
            description: "Recommended: Detect vibro maps and apply vibro fallback handling when needed."
        },
        useSvDetection: {
            title: "SV Detection",
            description: "Enable SV-based mode tag/category detection. When enabled, maps with significant speed changes will be tagged as SV."
        },
        display6kLevel: {
            title: "Show 6K Constant Rating",
            description: "Display the constant rating overlay on the star capsule for 6K beatmaps."
        },
        extendedEstimationRange: {
            title: "Extended Estimation Range",
            description: "Only effective with the Sunny estimator on some situations. Estimates change and extended tiers' accuracy is not guaranteed."
        },
        forceSunnyWindow: {
            title: "Improve Sunny LN Estimation",
            description: "Improve Sunny LN Estimation by removing rice parts of beatmap before analyze."
        },
        enableAnalyzeLN: {
            title: "Analyze LN Parts",
            description: "[Requires Improve Sunny LN Estimation] Analyze the percentage of LN/HB/Mix/RC parts in beatmap."
        },
        estimatorAlgorithm: {
            title: "Estimator Algorithm",
            description: "Select estimator algorithm.",
            options: {
                "Mixed": "Mixed",
                "Azusa": "Azusa",
                "Roxy": "Roxy",
                "Sunny": "Sunny",
                "Daniel": "Daniel",
                "Companella": "Companella"
            }
        },
        etternaVersion: {
            title: "Global Etterna Version",
            description: "Select Etterna's MinaCalc version used by MSD and related metrics."
        },
        companellaEtternaVersion: {
            title: "Companella Etterna Version",
            description: "Select Etterna's MinaCalc version specifically used for Companella Estimator."
        },
        wsEndpoint: {
            title: "WebSocket Endpoint",
            description: "Set host:port for both websocket and beatmap HTTP endpoint."
        },
        enableTelemetry: {
            title: "Anonymous Usage Statistics",
            description: "Send anonymous usage statistics (no username, no beatmap info, no IP) to the project's telemetry server. You can turn this off anytime."
        },
        DebugButton: {
            title: "Debug Page",
            description: "Go to the debug.html page to view and manage debug information."
        },
        debugUseAmount: {
            title: "Use Amount For Category",
            description: "Sort categories by amount and use the top pattern amount for category fallback."
        },
        azusaSunnyReferenceHo: {
            title: "Azusa Sunny Reference Force HO",
            description: "When enabled, Azusa always calls Sunny reference with cvtFlag=HO to remove LN inflation from partial LN maps."
        },
        enableAlwaysShowLNDifficulty: {
            title: "Always Show LN Difficulty",
            description: "Always show LN Difficulty even when LN% is too low or LN is too simple."
        },
        presetStorage: {
            title: "Preset Storage (Internal)",
            description: "Internal storage for custom presets (JSON). Do not edit unless you know what you are doing."
        }
    },
    presets: {
        title: "Presets Manager",
        systemCategory: "System Presets",
        customCategory: "My Presets",
        createBtn: "New Preset",
        importBtn: "Import",
        exportAllBtn: "Export All",
        applyBtn: "Apply Preset",
        saveBtn: "Save Changes",
        deleteBtn: "Delete Preset",
        exportBtn: "Export Preset",
        nameLabel: "Preset Name",
        descLabel: "Description",
        versionLabel: "Version",
        fieldsIncludedLabel: "Included Settings Fields"
    }
};
