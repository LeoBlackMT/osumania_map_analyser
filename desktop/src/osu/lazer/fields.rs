use crate::osu::offsets::FieldLookup;

// ---- 结构常数（来自我们自己的 P4/P4b 证据；不是偏移表的内容）----

pub const MARKER_PATTERN: &str = "01 01 00 00 00 00 80 44 00 00 40 44";
pub const ANCHOR_SITE_DELTA: i64 = 0x24;
pub const SITE_DELTAS: &[i64] = &[0x24, 0x28, 0x2c, 0x20, 0x30, 0x1c, 0x34];
pub const GAME_BASE_HOPS: &[(&str, u64)] = &[
    ("external_link_opener", 0x0),
    ("api_access", 0x218),
    ("game", 0x310),
];
pub const ANCHOR_KEY: &str = "gameBase";
pub const ARCH_X64: &str = "x64";
pub const ARCH_X86: &str = "x86";
pub const TABLE_DIR: &str = "lazer-offsets";
pub const ENV_TABLE: &str = "MMA_LAZER_OFFSETS";
pub const NEAREST_MAX_DISTANCE: u32 = 64;
pub const MARKER_HIT_LIMIT: usize = 16;
pub const MAX_RESOLUTION_LINES: usize = 16;
pub const FOLDER_DOT: &str = ".";
pub const FILES_DIR: &str = "files";
pub const MD5_HEX_LEN: usize = 32;
pub const STORE_KEY_HEX_LEN: usize = 64;
pub const GAP_MODS: &str = "play.mods:offsets-missing-ScoreInfo.ModsJson";
pub const GAP_PLAY_HITS: &str = "play.hits:offsets-missing-score-chain";
pub const GAP_RESULTS_HITS: &str = "resultsScreen.hits:offsets-missing-score-chain";
pub const GAP_BACKGROUND: &str = "files.background:offsets-missing-BeatmapSetInfo.Files";
pub const GAP_AUDIO: &str = "files.audio:offsets-missing-BeatmapSetInfo.Files";
pub const GAP_NO_BEATMAP: &str = "beatmap:none";
pub const SCREEN_STACK_MAX: i32 = 64;
pub const UNMAPPED_SCREEN_SUFFIX: &str = "not-in-observed-set";

pub const SCREEN_STATE_MAP: &[(&str, &str)] = &[
    ("osu.Game.Screens.Menu.MainMenu", "menu"),
    ("osu.Game.Screens.Select.SoloSongSelect", "selectPlay"),
    ("osu.Game.Screens.Select.SongSelect", "selectPlay"),
    ("osu.Game.Screens.Play.Player", "play"),
    ("osu.Game.Screens.Play.SoloPlayer", "play"),
    ("osu.Game.Screens.Play.ReplayPlayer", "play"),
    ("osu.Game.Screens.Play.PlayerLoader", "play"),
    ("osu.Game.Screens.Ranking.ResultsScreen", "resultScreen"),
    ("osu.Game.Screens.Ranking.SoloResultsScreen", "resultScreen"),
    ("osu.Game.Screens.Edit.Editor", "edit"),
    ("osu.Game.Screens.Edit.EditorLoader", "selectEdit"),
];

pub fn screen_state_for(type_name: &str) -> Option<&'static str> {
    SCREEN_STATE_MAP
        .iter()
        .find(|(screen, _)| *screen == type_name)
        .map(|(_, state)| *state)
}

pub const LIVE_TIME_MAX_MS: f64 = 86_400_000.0;

// ---- 表驱动的字段查法 ----

pub const GAME_TYPE_ANY: &[&str] = &["osu.Desktop.OsuGameDesktop", "osu.Game.OsuGameBase"];

pub const F_STORAGE: FieldLookup = FieldLookup::new_any(GAME_TYPE_ANY, "<Storage>k__BackingField");
pub const F_BEATMAP: FieldLookup = FieldLookup::new_any(GAME_TYPE_ANY, "<Beatmap>k__BackingField");
pub const F_SCREEN_STACK: FieldLookup =
    FieldLookup::new_any(GAME_TYPE_ANY, "<ScreenStack>k__BackingField");
pub const F_SELECTED_MODS: FieldLookup = FieldLookup::new_any(GAME_TYPE_ANY, "SelectedMods");

pub const F_STORAGE_BASE_PATH: FieldLookup = FieldLookup::new(
    &["osu.Game.IO.OsuStorage"],
    "<BasePath>k__BackingField",
);
pub const F_BEATMAP_BINDABLE_VALUE: FieldLookup = FieldLookup::new(
    &[
        "osu.Framework.Bindables.NonNullableBindable`1",
        "osu.Game.Beatmaps.WorkingBeatmap",
    ],
    "value",
);
pub const F_WORKING_BEATMAP_INFO: FieldLookup = FieldLookup::new(
    &["osu.Game.Beatmaps.WorkingBeatmapCache+BeatmapManagerWorkingBeatmap"],
    "BeatmapInfo",
);
pub const F_BEATMAP_INFO_MD5: FieldLookup = FieldLookup::new(
    &["osu.Game.Beatmaps.BeatmapInfo"],
    "<MD5Hash>k__BackingField",
);
pub const F_BEATMAP_INFO_HASH: FieldLookup = FieldLookup::new(
    &["osu.Game.Beatmaps.BeatmapInfo"],
    "<Hash>k__BackingField",
);
pub const F_BEATMAP_INFO_ONLINE_ID: FieldLookup = FieldLookup::new(
    &["osu.Game.Beatmaps.BeatmapInfo"],
    "<OnlineID>k__BackingField",
);
pub const F_BEATMAP_INFO_DIFFICULTY_NAME: FieldLookup = FieldLookup::new(
    &["osu.Game.Beatmaps.BeatmapInfo"],
    "<DifficultyName>k__BackingField",
);
pub const F_BEATMAP_INFO_METADATA: FieldLookup = FieldLookup::new(
    &["osu.Game.Beatmaps.BeatmapInfo"],
    "<Metadata>k__BackingField",
);
pub const F_BEATMAP_INFO_SET: FieldLookup = FieldLookup::new(
    &["osu.Game.Beatmaps.BeatmapInfo"],
    "<BeatmapSet>k__BackingField",
);
pub const F_SET_ONLINE_ID: FieldLookup = FieldLookup::new(
    &["osu.Game.Beatmaps.BeatmapSetInfo"],
    "<OnlineID>k__BackingField",
);
pub const F_METADATA_TITLE: FieldLookup =
    FieldLookup::new(&["osu.Game.Beatmaps.BeatmapMetadata"], "<Title>k__BackingField");
pub const F_METADATA_ARTIST: FieldLookup =
    FieldLookup::new(&["osu.Game.Beatmaps.BeatmapMetadata"], "<Artist>k__BackingField");
pub const F_METADATA_AUTHOR: FieldLookup =
    FieldLookup::new(&["osu.Game.Beatmaps.BeatmapMetadata"], "<Author>k__BackingField");
pub const F_REALM_USERNAME: FieldLookup =
    FieldLookup::new(&["osu.Game.Models.RealmUser"], "<Username>k__BackingField");
pub const F_SCREEN_STACK_LIST: FieldLookup =
    FieldLookup::new(&["osu.Game.Screens.OsuScreenStack"], "stack");
pub const F_SCREEN_STACK_ARRAY: FieldLookup = FieldLookup::new(
    &["System.Collections.Generic.Stack`1", "osu.Framework.Screens.IScreen"],
    "_array",
);
pub const F_SCREEN_STACK_SIZE: FieldLookup = FieldLookup::new(
    &["System.Collections.Generic.Stack`1", "osu.Framework.Screens.IScreen"],
    "_size",
);
pub const F_BEATMAP_CLOCK: FieldLookup =
    FieldLookup::new_any(GAME_TYPE_ANY, "beatmapClock");
pub const F_BEATMAP_TRACK_CLOCK: FieldLookup = FieldLookup::new(
    &["osu.Game.Beatmaps.FramedBeatmapClock"],
    "interpolatedTrack",
);
pub const F_BEATMAP_CLOCK_TIME: FieldLookup = FieldLookup::new(
    &["osu.Framework.Timing.InterpolatingFramedClock"],
    "<CurrentTime>k__BackingField",
);
pub const F_STRING_LENGTH: FieldLookup = FieldLookup::new(&["System.String"], "_stringLength");
pub const F_STRING_CHARS: FieldLookup = FieldLookup::new(&["System.String"], "_firstChar");

pub const TABLE_BACKED_FIELDS: &[&str] = &[
    "state.number",
    "state.name",
    "beatmap.id",
    "beatmap.set",
    "beatmap.md5",
    "beatmap.version",
    "beatmap.artist",
    "beatmap.title",
    "beatmap.mapper",
    "beatmap.time.live",
    "files.beatmap",
    "files.background",
    "files.audio",
    "folders.songs",
    "directPath.beatmapFile",
    "menu.mods",
    "play.mods",
    "resultsScreen.mods",
    "play.hits",
    "resultsScreen.hits",
];

pub fn missing_table_degraded_fields() -> Vec<String> {
    TABLE_BACKED_FIELDS
        .iter()
        .map(|field| (*field).to_string())
        .collect()
}
