// js/app/i18n/en-US.js
// 英文覆盖表：只放"源码里的英文不是最终英文"的键。
// settings.json 里给 tosu 看的标题是「Settings Guide | 设置说明」这种双语形式，
// 英文界面下只显示英文部分。
export default {
    "settings.GuideButton.title": "Settings Guide",
    "settings.PresetGuideButton.title": "Presets Guide",
    "ui.shell.laneAcc.title": "LaneAcc",
    "ui.shell.laneAcc.space.title": "Odd-key middle lane",
    "ui.shell.laneAcc.space.description": "Determines the hand grouping for the middle column in odd-key layouts for LaneAcc.",
    "ui.shell.laneAcc.space.independent": "Excluded",
    "ui.shell.laneAcc.space.left": "Join left hand",
    "ui.shell.laneAcc.space.right": "Join right hand",
    "ui.shell.offsets.networkUnavailable": "Network unavailable or remote host unreachable; keeping existing local offsets.",
    "ui.shell.offsets.manifestUnreadable": "The remote manifest is unreadable or malformed; keeping existing local offsets.",
    "ui.shell.offsets.updateSummary": "Offsets check complete: {updated} table(s) updated, {skipped} already current.",
    "ui.shell.offsets.genNotFound": "gen.exe was not found next to mma-shell.exe or in the dev tree.",
    "ui.shell.offsets.genFailed": "The generator failed or the game process is not running.",
    "ui.shell.offsets.genSpawnFailed": "Could not start gen.exe.",
    "ui.shell.offsets.genProcessNotRunning": "The osu! (lazer) process is not running.",
    "ui.shell.offsets.genAccessDenied": "Could not read the osu! process; run as administrator.",
    "ui.shell.offsets.genArchMismatch": "A 32-bit osu! process was detected; lazer must be 64-bit.",
    "ui.shell.offsets.genLazerDirMissing": "Could not locate the osu!lazer install directory.",
    "ui.shell.offsets.genAnchorNotFound": "Could not find the lazer anchor pattern in game memory.",
    "ui.shell.offsets.genValidationFailed": "A candidate GameBase was found, but live validation (MD5Hash / ScreenStack / VTable) failed.",
    "ui.shell.offsets.genIlMetadataFailed": "Could not read lazer IL metadata.",
    "ui.shell.offsets.genUnsupportedOs": "This feature is only supported on Windows.",
    "ui.shell.offsets.generatedMissingScore": "Offsets generated and verified, but the score section required by LaneAcc/LivePP is missing. Enter a play/replay and generate again.",
    "ui.shell.offsets.generatedMissingResults": "Offsets generated and verified, but the results section required by result-screen feedback is missing. Stay on the result screen and generate again.",
    "ui.shell.offsets.generatedMissingBoth": "Offsets generated and verified, but this run only captured the base fields. Enter a play/replay and generate once, then stay on the result screen and generate again to fill score/results.",
    "ui.shell.offsets.generatedSections": "Offsets generated and verified ({sections}).",
    "ui.shell.offsets.workflowHint": "Generate twice: ① in play/replay; ② stay on the result screen and generate again.",
};
