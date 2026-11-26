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

/// Lazer 顶层屏幕在 LaneAcc 会话生命周期里的分类。
///
/// 与 `SCREEN_STATE_MAP`（面向 tosu 兼容状态名，将 `PlayerLoader` 映射为 `"play"`）刻意解耦：
/// `PlayerLoader` 不是 `Player`，绝不能拿它解引用 `ScoreProcessor`/`Score`；同时它是新局与
/// 重试（Retry）的必经屏幕，用来在进入新 `Player` 前清掉上一局滞留在壳里的事件与按键帧。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LazerPlayScreen {
    Player,
    PlayerLoader,
    Results,
    #[default]
    Other,
}

pub fn classify_lazer_screen(type_name: &str, mapped: Option<&str>) -> LazerPlayScreen {
    if type_name.ends_with("PlayerLoader") {
        return LazerPlayScreen::PlayerLoader;
    }
    match mapped.or_else(|| screen_state_for(type_name)) {
        Some("play") => LazerPlayScreen::Player,
        Some("resultScreen") => LazerPlayScreen::Results,
        _ if type_name.ends_with("ResultsScreen") => LazerPlayScreen::Results,
        _ => LazerPlayScreen::Other,
    }
}

/// 追踪 Lazer 屏幕与选图切换，决定何时清理壳内缓存的 LaneAcc 日志。
///
/// - 选歌/菜单 → `PlayerLoader`（新开图或重进同一首歌）：第 1 拍立刻清理，防止 `laneacc_sync`
///   在 `Player` 创建前把上一首/上一局的残存数据灌回页面；
/// - 从游玩/结算退入 Loader 时，退出与重试的屏幕相同；保留记录直至看见下一次 Player，
///   避免用加载时间猜测方向，把中途退出的一局擦掉。
/// - 选歌/菜单（`Other`）中切换到不同谱面：立刻清理上一首歌残留；
/// - `Results`（结算屏）：永不清理，保留结算终态与 F5 刷新恢复。
#[derive(Clone, Debug, Default)]
pub struct LazerSessionResetTracker {
    last_screen: LazerPlayScreen,
    last_played_beatmap: Option<String>,
    has_session_data: bool,
}

impl LazerSessionResetTracker {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn observe(&mut self, screen: LazerPlayScreen, beatmap_id: Option<&str>) -> bool {
        let previous = self.last_screen;
        self.last_screen = screen;
        let beatmap_id = beatmap_id.filter(|id| !id.is_empty());
        let clear = self.has_session_data && match screen {
            LazerPlayScreen::PlayerLoader => previous == LazerPlayScreen::Other,
            LazerPlayScreen::Player => previous == LazerPlayScreen::PlayerLoader,
            LazerPlayScreen::Other => self.last_played_beatmap.as_deref().zip(beatmap_id)
                .is_some_and(|(old, new)| old != new),
            LazerPlayScreen::Results => false,
        };
        if clear {
            self.has_session_data = false;
            self.last_played_beatmap = None;
        }
        if screen == LazerPlayScreen::Player {
            self.has_session_data = true;
            if let Some(id) = beatmap_id {
                if self.last_played_beatmap.as_deref() != Some(id) {
                    self.last_played_beatmap = Some(id.to_owned());
                }
            }
        }
        clear
    }
}

#[cfg(test)]
mod screen_tracker_tests {
    use super::*;

    #[test]
    fn classifies_players_loaders_and_results() {
        assert_eq!(classify_lazer_screen("osu.Game.Screens.Play.SoloPlayer", None), LazerPlayScreen::Player);
        assert_eq!(classify_lazer_screen("osu.Game.Screens.Play.ReplayPlayer", None), LazerPlayScreen::Player);
        assert_eq!(classify_lazer_screen("osu.Game.Screens.Play.PlayerLoader", None), LazerPlayScreen::PlayerLoader);
        assert_eq!(
            classify_lazer_screen("osu.Game.Screens.Select.SoloSongSelect+PlayerLoader", None),
            LazerPlayScreen::PlayerLoader
        );
        assert_eq!(
            classify_lazer_screen("osu.Game.Screens.Ranking.SoloResultsScreen", None),
            LazerPlayScreen::Results
        );
        assert_eq!(
            classify_lazer_screen("osu.Game.Screens.Select.SoloSongSelect", None),
            LazerPlayScreen::Other
        );
    }

    #[test]
    fn loader_direction_not_duration_distinguishes_retry_from_exit() {
        let mut tracker = LazerSessionResetTracker::default();
        assert!(!tracker.observe(LazerPlayScreen::Player, Some("map-a")));
        // Even a slow exit via the loader retains the completed run.
        assert!(!tracker.observe(LazerPlayScreen::PlayerLoader, Some("map-a")));
        assert!(!tracker.observe(LazerPlayScreen::PlayerLoader, Some("map-a")));
        assert!(!tracker.observe(LazerPlayScreen::Other, Some("map-a")));
        // Re-entering the same map from SongSelect clears immediately on the first loader tick.
        assert!(tracker.observe(LazerPlayScreen::PlayerLoader, Some("map-a")));
        assert!(!tracker.observe(LazerPlayScreen::PlayerLoader, Some("map-a")));
        assert!(!tracker.observe(LazerPlayScreen::Player, Some("map-a")));
        // A retry is confirmed when the loader enters the next Player.
        assert!(!tracker.observe(LazerPlayScreen::PlayerLoader, Some("map-a")));
        assert!(!tracker.observe(LazerPlayScreen::PlayerLoader, Some("map-a")));
        assert!(tracker.observe(LazerPlayScreen::Player, Some("map-a")));
    }

    #[test]
    fn song_switch_on_select_clears_but_results_preserves() {
        let mut tracker = LazerSessionResetTracker::default();
        assert!(!tracker.observe(LazerPlayScreen::Player, Some("map-a")));
        assert!(!tracker.observe(LazerPlayScreen::Results, Some("map-a")));
        assert!(!tracker.observe(LazerPlayScreen::Other, Some("map-a")));
        assert!(tracker.observe(LazerPlayScreen::Other, Some("map-b")));
        // Once cleared, entering map-b's loader does not emit another clear.
        assert!(!tracker.observe(LazerPlayScreen::PlayerLoader, Some("map-b")));
    }
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
