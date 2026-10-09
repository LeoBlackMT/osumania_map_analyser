// malody4::runtime - 后台 poller 轮询、索引构建线程、进程附着与 selection/song 帧分发

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Instant, SystemTime};

use crate::frames::MAX_PAYLOAD_BYTES;
use crate::malody4::anchor::{self, AnchorError, IdentityKey, MemoryProbe};
use crate::malody4::config::{self, GameConfig};
use crate::malody4::gamelog::{self, SceneTracker};
use crate::malody4::library::{self, ChartMeta, LibraryEntry};
use crate::malody4::model::{Screen, Selection, UnavailableReason};
use crate::malody4::selection::{Action, Availability, SelectionState};
use crate::malody4::settings::{
    chain_disagreement_warning, effective_settings, file_cross_check_warning, settings_log,
    EffectiveSettings, SettingsLogKey, SettingsOrigin,
};
use crate::server::log::log_at;
use crate::server::{broadcast, Shared};

use super::*;

/// 索引构建线程：`idx.lib` 的唯一写者；根目录由 poller 经 channel 送达。
pub fn index_build_loop(idx: Arc<IndexShared>, root_rx: mpsc::Receiver<PathBuf>) {
    let mut root: Option<PathBuf> = None;
    let mut built_root: Option<PathBuf> = None;
    let mut last_rescan: Option<Instant> = None;
    loop {
        match root_rx.recv_timeout(POLL_INTERVAL) {
            Ok(new_root) => root = Some(new_root),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            // poller 线程结束（进程收尾）→ 本线程收工
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        while let Ok(new_root) = root_rx.try_recv() {
            root = Some(new_root);
        }
        let requested = idx.rebuild_requested.swap(false, Ordering::Relaxed);
        let Some(dir) = root.clone() else { continue };
        // 首次拿到根目录、或根目录换了（旧索引整体作废）→ 立即建一次
        let stale = built_root.as_deref() != Some(dir.as_path());
        let rescan_due = last_rescan
            .map(|at| at.elapsed() >= INDEX_RESCAN)
            .unwrap_or(true);
        // 到点且没有别的重建理由 → 只读元数据地判定"树变了没有"（绝不在这里哈希）
        let scanned = rescan_due && !stale && !requested;
        let changed = scanned && library::rescan_change(&dir, &index_fingerprint(&idx)).is_some();
        if rescan_due {
            last_rescan = Some(Instant::now());
        }
        if !requested && !stale && !changed {
            if scanned {
                // 每个重扫周期一条（60s），不是每 tick 一条：库没变就不该有重建噪声
                log_at(
                    "debug",
                    &format!(
                        "malody4 index rescan: root={} unchanged (metadata fingerprint identical) — rebuild skipped",
                        dir.display()
                    ),
                );
            }
            continue;
        }
        let started = Instant::now();
        let rebuilt = library::rebuild(&dir);
        let stats = rebuilt.stats.clone();
        *idx.lib.lock().unwrap() = rebuilt;
        let generation = idx.generation.fetch_add(1, Ordering::Relaxed) + 1;
        // 树确实变了（周期重扫检出 / 根目录换了）→ 放开此前判定"不可解析"的身份键：
        // 它们可能正是这次新增的文件。miss 触发的重建不在此列（见 `content_revision`）。
        if changed || stale {
            idx.content_revision.fetch_add(1, Ordering::Relaxed);
        }
        // stats 全字段：用于区分"库里没有这张谱"与"被过滤掉了"。
        // info 级（默认 logLevel=info 可见）：这是判断索引是否建成、收了多少张的唯一现场证据。
        log_at(
            "info",
            &format!(
                "malody4 index built: root={} indexed={} skipped_non_key={} skipped_osu_not_mania={} skipped_osu_no_keys={} skipped_junk={} skipped_ext={} skipped_unparsed={} duplicate_md5={} elapsed_ms={} generation={}",
                dir.display(),
                stats.indexed,
                stats.skipped_non_key,
                stats.skipped_osu_not_mania,
                stats.skipped_osu_no_keys,
                stats.skipped_junk,
                stats.skipped_ext,
                stats.skipped_unparsed,
                stats.duplicate_md5,
                started.elapsed().as_millis(),
                generation
            ),
        );
        built_root = Some(dir);
    }
}

/// 已 attach 的目标：`Target` 持进程句柄（基址 / 映像路径 / 磁盘文件大小的来源），
/// `Attachment` 持读取句柄。任一次读取失败 → 整体丢弃（两者 `Drop` 各自关闭句柄）。
pub struct Attached {
    pub target: anchor::Target,
    pub attachment: anchor::Attachment,
}

impl Attached {
    pub fn process_exe(&self) -> &Path {
        &self.target.exe_path
    }
}

/// 日志尾随状态：`(文件路径, 已读字节偏移, 半行残留)`。
pub struct LogTail {
    pub path: Option<PathBuf>,
    pub offset: u64,
    pub partial: String,
}

impl LogTail {
    pub fn new() -> Self {
        LogTail {
            path: None,
            offset: 0,
            partial: String::new(),
        }
    }

    /// 尾随最新日志文件：新文件 → 从 0 读起；旧文件被删且无后继 → 保留最后已知 `screen`
    /// （`playing` 由 `fresh(PLAYING_FRESH)` 自然失鲜）。
    pub fn follow(&mut self, dir: &Path, tracker: &mut SceneTracker) {
        let Some(latest) = gamelog::latest_log(dir) else {
            return;
        };
        if self.path.as_deref() != Some(latest.as_path()) {
            self.path = Some(latest.clone());
            self.offset = 0;
            self.partial.clear();
        }
        self.read_new(&latest, tracker);
    }

    /// 按字节读新增内容（**绝不整文件 read_to_string**），半行留到下一 tick 再拼。
    pub fn read_new(&mut self, path: &Path, tracker: &mut SceneTracker) {
        let Ok(len) = fs::metadata(path).map(|meta| meta.len()) else {
            return;
        };
        if len < self.offset {
            // 文件被截断 / 同名替换：从头重读
            self.offset = 0;
            self.partial.clear();
        }
        if len == self.offset {
            return;
        }
        let Ok(mut file) = fs::File::open(path) else {
            return;
        };
        if file.seek(SeekFrom::Start(self.offset)).is_err() {
            return;
        }
        let mut buf = Vec::new();
        if file.read_to_end(&mut buf).is_err() {
            return;
        }
        self.offset += buf.len() as u64;
        let mut combined = std::mem::take(&mut self.partial);
        combined.push_str(&String::from_utf8_lossy(&buf));
        let mut lines: Vec<&str> = combined.split('\n').collect();
        let tail = lines.pop().unwrap_or_default().to_string();
        for line in lines {
            tracker.apply_line(line.trim_end_matches('\r'));
        }
        self.partial = tail;
    }
}

/// `config.json` 的解析缓存：**按字节内容**判定是否变化，`(mtime, 长度)` 只作廉价提示。
#[derive(Default)]
pub struct ConfigCache {
    pub sig: Option<(SystemTime, u64)>,
    /// 上次**接受**的原始字节（内容判据的基准；`None` = 文件不存在或读不出来）。
    pub bytes: Option<Vec<u8>>,
    pub value: Option<GameConfig>,
    pub loaded: bool,
}

impl ConfigCache {
    pub fn read(&mut self, path: &Path) -> Option<&GameConfig> {
        let sig = fs::metadata(path)
            .ok()
            .and_then(|meta| meta.modified().ok().map(|mtime| (mtime, meta.len())));
        let bytes = fs::read(path).ok();
        // 快速路径：签名与字节都没变 → 不重新解析（`bytes` 为 `None` 时同样成立）
        if self.loaded && self.sig == sig && self.bytes.as_deref() == bytes.as_deref() {
            return self.value.as_ref();
        }
        self.sig = sig;
        self.loaded = true;
        // 与 `config::read` 同语义：非 UTF-8 视同读不出来
        let parsed = bytes
            .as_deref()
            .and_then(|raw| std::str::from_utf8(raw).ok())
            .and_then(config::parse);
        self.bytes = bytes;
        if self.value != parsed {
            self.value = parsed;
        }
        self.value.as_ref()
    }
}

/// 按"原因"分别节流的日志簿（同一原因在 `ACTION_LOG_THROTTLE` 内只记一条）。
#[derive(Default)]
pub struct ReasonLog {
    pub last: HashMap<String, Instant>,
}

impl ReasonLog {
    pub fn due(&mut self, reason: &str, now: Instant) -> bool {
        // "process-not-found" 是未运行 Malody 4 时的常态，绝不在整个会话中每 30s 刷屏。
        // 每个附着周期（从启动或重连重置起）只打一条。
        if reason == "process-not-found" {
            if self.last.contains_key(reason) {
                return false;
            }
            self.last.insert(reason.to_string(), now);
            return true;
        }
        match self.last.get(reason) {
            Some(at) if now.duration_since(*at) < ACTION_LOG_THROTTLE => false,
            _ => {
                self.last.insert(reason.to_string(), now);
                true
            }
        }
    }
}

/// 一个身份键的重建尝试簿（纯逻辑、无 IO；住在 `Runtime` 里）。
#[derive(Debug, Default)]
pub struct MissRebuildTracker {
    /// md5 → 已经用掉的重建次数。
    pub used: HashMap<String, u32>,
    /// 已判定本会话不可解析的 md5。
    pub unresolvable: HashSet<String>,
}

impl MissRebuildTracker {
    /// 该 md5 是否已判定不可解析（判定后调用方直接跳过：不请求、不记日志）。
    pub fn is_unresolvable(&self, md5: &str) -> bool {
        self.unresolvable.contains(md5)
    }

    /// 该 md5 的对外不可用原因（**两档**）：
    pub fn unavailable_reason(&self, md5: &str) -> UnavailableReason {
        if self.is_unresolvable(md5) {
            UnavailableReason::ChartUnknownIdentity
        } else {
            UnavailableReason::ChartUnresolved
        }
    }

    /// 已用掉的重建次数（诊断与单测）。
    pub fn attempts(&self, md5: &str) -> u32 {
        self.used.get(md5).copied().unwrap_or(0)
    }

    /// 记一次重建尝试；预算用完返回 `false`，并就此把该 md5 记为不可解析。
    pub fn spend(&mut self, md5: &str) -> bool {
        let used = self.used.entry(md5.to_string()).or_insert(0);
        if *used >= MISS_REBUILD_ATTEMPTS {
            self.unresolvable.insert(md5.to_string());
            return false;
        }
        *used += 1;
        true
    }

    /// 库**真的**变了（周期重扫检出变化 / 根目录换了）→ 清空预算与放弃记录。
    pub fn reset(&mut self) {
        self.used.clear();
        self.unresolvable.clear();
    }
}

/// 判定"本会话不可解析"那一刻的 **info** 日志文案（纯函数，便于单测逐字钉住）。
pub fn unresolvable_chart_log(md5: &str, generation: u64, indexed: usize) -> String {
    format!(
        "malody4 chart cannot be identified from the local library: md5={md5} (generation={generation}, indexed={indexed}) — {MISS_REBUILD_ATTEMPTS} rebuild attempts exhausted, no further rebuilds for this identity; the previous chart stays on screen"
    )
}

/// poller 的运行时状态（私有；`IndexShared` 是它与构建线程之间的共享）。
pub struct Runtime {
    pub idx: Arc<IndexShared>,
    pub root_tx: mpsc::Sender<PathBuf>,
    pub sent_root: Option<PathBuf>,
    pub attached: Option<Attached>,
    pub attach_error: Option<UnavailableReason>,
    pub last_attach_try: Option<Instant>,
    pub tail: LogTail,
    pub tracker: SceneTracker,
    pub game_config: ConfigCache,
    /// 连续软锚点读取失败的计数（硬失败不计数、直接 detach）。
    pub soft_reads: SoftReadTolerance,
    pub selection: SelectionState,
    pub dispatch: SongDispatchState,
    pub reason_log: ReasonLog,
    pub last_rebuild_request: Option<Instant>,
    /// 每个身份键的 miss 重试预算（用完即本会话不再为它重建）。
    pub miss_tracker: MissRebuildTracker,
    /// 上次同步过的 `IndexShared::content_revision`（变大 = 库真的变了 → 放开不可解析记录）。
    pub seen_content_revision: u64,
    pub last_miss: Option<(String, u64)>,
    pub last_hit: Option<(String, String)>,
    pub last_selection_log: Option<(String, String, f64, &'static str)>,
    /// 上一 tick 写进 `shared.malody4.reason` 的值（心跳之间的 tick 保持不变，避免闪）。
    pub reason: String,
    /// FAIR 判定模组的一次性提示是否已发（进程内只发一次）。
    pub fair_logged: bool,
    /// 附着成功时刻 = "内存 vs config.json"交叉校验窗口的起点（未附着为 `None`）。
    pub attached_at: Option<Instant>,
    /// 交叉校验是否已经做过（每个附着窗口只做一次，无论结论如何）。
    pub settings_cross_checked: bool,
    /// `S` / `P` 连续不一致的拍数（一致或读不到就清零）。
    pub chain_disagree: u32,
    /// 两条链持续不一致的告警是否已记（**进程内只记一次**）。
    pub chain_disagree_logged: bool,
    /// 上一次记过的有效值（来源 / 判定 / 速率）：只有变化时才再记 info。
    pub last_settings: Option<SettingsLogKey>,
    pub song_seq: u64,
}

impl Runtime {
    pub fn new(idx: Arc<IndexShared>, root_tx: mpsc::Sender<PathBuf>) -> Self {
        let seen_content_revision = idx.content_revision.load(Ordering::Relaxed);
        Runtime {
            idx,
            root_tx,
            sent_root: None,
            attached: None,
            attach_error: None,
            last_attach_try: None,
            tail: LogTail::new(),
            tracker: SceneTracker::new(),
            game_config: ConfigCache::default(),
            soft_reads: SoftReadTolerance::new(),
            selection: SelectionState::new(),
            dispatch: SongDispatchState::default(),
            reason_log: ReasonLog::default(),
            last_rebuild_request: None,
            miss_tracker: MissRebuildTracker::default(),
            seen_content_revision,
            last_miss: None,
            last_hit: None,
            last_selection_log: None,
            reason: String::new(),
            fair_logged: false,
            attached_at: None,
            settings_cross_checked: false,
            chain_disagree: 0,
            chain_disagree_logged: false,
            last_settings: None,
            song_seq: 0,
        }
    }

    /// 一个 tick。
    pub fn tick(&mut self, shared: &Shared) {
        let now = Instant::now();

        // ① 惰性 attach：未附着时每 ATTACH_RETRY 试一次；各失败原因分别节流记 warn。
        self.ensure_attached(now);

        // ② 根目录解析链（process_exe → env → 壳配置 → tosu 设置 → 启发候选）。
        let process_exe = self
            .attached
            .as_ref()
            .map(|attached| attached.process_exe().to_path_buf());
        let root = crate::server::malody4_root(shared, process_exe.as_deref());
        if let Some(root) = root.as_ref() {
            // 解析结果进 Shared 缓存：24061 侧（`/shell-config` 的 `resolved.malody4Root`）
            // 拿不到 `process_exe`，只能从这里取"实际使用的目录"。
            crate::server::set_malody4_root(shared, root.clone());
            // 根目录变化时才通知构建线程（构建线程只做构建，不做解析）
            if self.sent_root.as_deref() != Some(root.as_path()) {
                let _ = self.root_tx.send(root.clone());
                self.sent_root = Some(root.clone());
            }
        }

        // ③ 索引侧同步：周期重扫（60s）已下沉到构建线程——指纹扫描要在那里跟 `idx.lib` 比对，
        //    且主循环绝不遍历文件系统。这里只把"库真的变了"的修订号同步过来，放开此前判定
        //    不可解析的身份键（它们可能正是这次新增的文件）。
        self.sync_content_revision();

        // ④ 场景：尾随最新日志（目录枚举与字节读取分别在 gamelog / LogTail 里）。
        let screen = match root.as_ref() {
            Some(root) => {
                self.tail.follow(&root.join("log"), &mut self.tracker);
                self.tracker.screen()
            }
            None => Screen::Other,
        };
        let playing = screen == Screen::Playing && self.tracker.fresh(PLAYING_FRESH);

        // ⑤ 锚点：只有根目录已知且已 attach 时才读内存。
        let mut key: Option<IdentityKey> = None;
        let mut read_ok = false;
        let mut blocker: Option<UnavailableReason> = None;
        if root.is_none() {
            blocker = Some(UnavailableReason::RootNotConfigured);
        } else if let Some(attached) = self.attached.as_ref() {
            match read_action(
                &mut self.soft_reads,
                anchor::read_identity_classified(&attached.attachment),
            ) {
                ReadAction::Healthy(identity) => {
                    key = identity;
                    read_ok = true;
                }
                ReadAction::Tolerated => read_ok = true,
                ReadAction::Detach(err) => blocker = Some(self.detach(err, now)),
            }
        } else {
            blocker = Some(
                self.attach_error
                    .clone()
                    .unwrap_or(UnavailableReason::ProcessNotFound),
            );
        }

        // ⑥ 索引 lookup：未就绪 → NoLibrary；miss → 置重建请求 + 记日志
        let mut entry: Option<LibraryEntry> = None;
        if blocker.is_none() {
            if let Some(identity) = key.as_ref() {
                if !index_ready(&self.idx) {
                    blocker = Some(UnavailableReason::NoLibrary);
                } else {
                    entry = lookup(&self.idx, &identity.md5);
                    match entry.as_ref() {
                        Some(found) => self.log_hit(&identity.md5, &found.path),
                        None => {
                            self.request_miss_rebuild(&identity.md5, now);
                            blocker = Some(self.miss_tracker.unavailable_reason(&identity.md5));
                        }
                    }
                }
            }
        }

        // ⑦ 判定档 / 变速位：**进程内存优先**，config.json 兜底。
        let file_config = match root.as_ref() {
            Some(root) => self.game_config.read(&root.join("config.json")).cloned(),
            None => None,
        };
        let probe = self.read_memory_settings(now);
        self.note_chain_disagreement(&probe);
        let effective = effective_settings(probe.published(), file_config.as_ref());
        self.note_file_cross_check(&effective, file_config.as_ref(), now);
        self.log_settings_change(&effective);
        let rate = effective.rate;
        let judge = effective.judge;
        if fair_judge_mod(effective.user_mods) && !self.fair_logged {
            self.fair_logged = true;
            log_at("warn", FAIR_JUDGE_WARNING);
        }

        // ⑧ selection 状态机
        let version = entry
            .as_ref()
            .and_then(|found| meta_of(&self.idx, &found.path))
            .map(|meta| meta.version)
            .unwrap_or_default();
        let availability = match blocker.clone() {
            Some(reason) => Availability::Unavailable(reason),
            None => Availability::Ready,
        };
        let action = self.selection.fold(
            key,
            entry.as_ref(),
            screen,
            &version,
            rate,
            availability,
            now,
        );

        if judge.is_none() && matches!(action, Action::Emit(_)) && self.reason_log.due("judge-unknown", now)
        {
            log_at(
                "warn",
                "malody4: judge level unknown (not readable from game memory and config.json missing or unparsable) — song frame withheld this tick",
            );
        }

        // ⑨ 分派：selection 帧必发；song 帧只在签名变化 / 出现更大连接 id 时发。
        let conn = max_conn_id(shared);
        for plan in dispatch_plan(&action, &mut self.dispatch, conn, judge) {
            match plan {
                Dispatch::SelectionFrame => self.send_selection(shared, &action),
                Dispatch::SongFrame => self.send_song(shared, entry.as_ref(), rate, judge),
                Dispatch::None => {}
            }
        }

        // ⑩ 状态更新：先在独立作用域里改完并 drop guard，再广播 state 帧。
        let alive = root.is_some() && read_ok;
        let reason = wire_reason(blocker.as_ref(), &action, &self.reason);
        let next = Malody4Status {
            alive,
            screen: screen.as_str().to_string(),
            playing,
            reason: reason.clone(),
            judge,
        };
        let due = {
            let mut status = shared.malody4.lock().unwrap();
            let due = state_push_due(&status, &next);
            *status = next;
            due
        };
        self.reason = reason;
        if due {
            broadcast(shared, "state", Some(crate::server::state_frame(shared)));
        }
    }

    /// 未附着时每 `ATTACH_RETRY` 重试一次 `find_target()` + `open()`。
    pub fn ensure_attached(&mut self, now: Instant) {
        if self.attached.is_some() {
            return;
        }
        if let Some(at) = self.last_attach_try {
            if now.duration_since(at) < ATTACH_RETRY {
                return;
            }
        }
        self.last_attach_try = Some(now);
        let error = match anchor::find_target() {
            Ok(target) => match anchor::open(&target) {
                Ok(attachment) => {
                    log_at(
                        "debug",
                        &format!(
                            "malody4 attached: pid={} base=0x{:08X} exe={}",
                            target.pid,
                            target.base_u32,
                            target.exe_path.display()
                        ),
                    );
                    self.attached = Some(Attached { target, attachment });
                    self.attach_error = None;
                    self.reason_log.last.clear();
                    self.attached_at = Some(now);
                    self.settings_cross_checked = false;
                    self.chain_disagree = 0;
                    return;
                }
                Err(err) => unavailable_from_anchor(err),
            },
            Err(err) => unavailable_from_anchor(err),
        };
        self.attach_error = Some(error.clone());
        if self.reason_log.due(&error.as_str(), now) {
            log_at(
                "warn",
                &format!(
                    "malody4 attach failed: {} (retrying every {}s)",
                    error.as_str(),
                    ATTACH_RETRY.as_secs()
                ),
            );
        }
    }

    /// 丢弃已附着目标。
    pub fn detach(&mut self, err: AnchorError, now: Instant) -> UnavailableReason {
        let reason = unavailable_from_anchor(err);
        self.attached = None;
        self.attach_error = Some(reason.clone());
        self.last_attach_try = Some(now);
        self.soft_reads.reset();
        self.attached_at = None;
        self.chain_disagree = 0;
        if self.reason_log.due(&reason.as_str(), now) {
            log_at(
                "warn",
                &format!(
                    "malody4 detached: {} (retrying every {}s)",
                    reason.as_str(),
                    ATTACH_RETRY.as_secs()
                ),
            );
        }
        reason
    }

    /// 读一次内存设置。
    pub fn read_memory_settings(&mut self, now: Instant) -> MemoryProbe {
        let Some(attached) = self.attached.as_ref() else {
            return MemoryProbe::default();
        };
        match anchor::read_settings(&attached.attachment) {
            Ok(probe) => probe,
            Err(err) => {
                if self.reason_log.due("settings-read", now) {
                    log_at(
                        "debug",
                        &format!("malody4 settings read failed: {err:?} — using config.json this tick"),
                    );
                }
                MemoryProbe::default()
            }
        }
    }

    pub fn note_chain_disagreement(&mut self, probe: &MemoryProbe) {
        match probe.disagreement() {
            Some((user, play)) => {
                self.chain_disagree = self.chain_disagree.saturating_add(1);
                if self.chain_disagree >= CHAIN_DISAGREE_POLLS && !self.chain_disagree_logged {
                    self.chain_disagree_logged = true;
                    log_at(
                        "warn",
                        &chain_disagreement_warning(user, play, self.chain_disagree),
                    );
                }
            }
            None => self.chain_disagree = 0,
        }
    }

    pub fn note_file_cross_check(
        &mut self,
        effective: &EffectiveSettings,
        file: Option<&GameConfig>,
        now: Instant,
    ) -> bool {
        if self.settings_cross_checked {
            return false;
        }
        let Some(attached_at) = self.attached_at else {
            return false;
        };
        if now.duration_since(attached_at) > SETTINGS_CROSS_CHECK {
            self.settings_cross_checked = true;
            return false;
        }
        let (Some(file), SettingsOrigin::Memory) = (file, effective.source) else {
            return false;
        };
        self.settings_cross_checked = true;
        if file.judge_letter() != effective.judge
            || (file.mod_flags().speed_rate() - effective.rate).abs() >= RATE_EPS
        {
            log_at("warn", &file_cross_check_warning(effective, file));
            return true;
        }
        false
    }

    pub fn log_settings_change(&mut self, effective: &EffectiveSettings) -> bool {
        let key = SettingsLogKey {
            source: effective.source,
            judge: effective.judge,
            rate: effective.rate,
        };
        if self.last_settings.as_ref().map(|last| last.matches(&key)) == Some(true) {
            return false;
        }
        log_at("info", &settings_log(effective));
        self.last_settings = Some(key);
        true
    }

    pub fn log_hit(&mut self, md5: &str, path: &Path) {
        let hit = (md5.to_string(), path.display().to_string());
        if self.last_hit.as_ref() != Some(&hit) {
            log_at(
                "info",
                &format!("malody4 anchor hit md5={} -> {}", hit.0, hit.1),
            );
            self.last_hit = Some(hit);
        }
    }

    pub fn sync_content_revision(&mut self) {
        let revision = self.idx.content_revision.load(Ordering::Relaxed);
        if revision != self.seen_content_revision {
            self.seen_content_revision = revision;
            self.miss_tracker.reset();
        }
    }

    pub fn request_miss_rebuild(&mut self, md5: &str, now: Instant) {
        if self.miss_tracker.is_unresolvable(md5) {
            return;
        }
        if self
            .last_rebuild_request
            .map(|at| now.duration_since(at) < INDEX_REBUILD_THROTTLE)
            .unwrap_or(false)
        {
            return;
        }
        let generation = self.idx.generation.load(Ordering::Relaxed);
        if !self.miss_tracker.spend(md5) {
            log_at(
                "info",
                &unresolvable_chart_log(md5, generation, indexed_count(&self.idx)),
            );
            return;
        }
        request_rebuild(&self.idx);
        self.last_rebuild_request = Some(now);
        if self.last_miss.as_ref() != Some(&(md5.to_string(), generation)) {
            log_at(
                "info",
                &format!(
                    "malody4 index miss md5={md5} (generation={generation}, indexed={}) — rebuild requested",
                    indexed_count(&self.idx)
                ),
            );
            self.last_miss = Some((md5.to_string(), generation));
        }
    }

    pub fn send_selection(&mut self, shared: &Shared, action: &Action) {
        let record = match action {
            Action::Emit(selection) => selection.clone(),
            Action::Hidden(_) => Selection::hidden(self.selection.sequence(), "hidden"),
            Action::None => return,
        };
        let log_key = (
            record.event.clone(),
            record.path.clone(),
            record.speed_rate,
            record.screen,
        );
        if self.last_selection_log.as_ref() != Some(&log_key) {
            log_at(
                "debug",
                &format!(
                    "malody4 selection: path=\"{}\" speed_rate={:.5} screen={} event={} sequence={}",
                    record.path, record.speed_rate, record.screen, record.event, record.sequence
                ),
            );
            self.last_selection_log = Some(log_key);
        }
        let payload = serde_json::to_value(&record).unwrap_or(serde_json::Value::Null);
        broadcast(shared, "malody4_selection", Some(payload));
    }

    pub fn send_song(
        &mut self,
        shared: &Shared,
        entry: Option<&LibraryEntry>,
        rate: f64,
        judge: Option<char>,
    ) {
        let (Some(entry), Some(judge)) = (entry, judge) else {
            return;
        };
        let Some(meta) = meta_of(&self.idx, &entry.path) else {
            if self.reason_log.due("meta-missing", Instant::now()) {
                log_at(
                    "warn",
                    &format!("malody4 index meta missing for {}", entry.path.display()),
                );
            }
            return;
        };
        let seq = self.song_seq + 1;
        let Some(song) = build_song_frame(shared, entry, &meta, rate, judge, seq) else {
            return;
        };
        self.song_seq = seq;
        log_at(
            "debug",
            &format!(
                "malody4 song frame: identity=mdy4:{} requestId=n{} path={} rate={:.5} judge={}",
                entry.md5,
                seq,
                entry.path.display(),
                rate,
                judge
            ),
        );
        broadcast(shared, "song", Some(song));
    }
}

/// 启动 Malody 4 源 poller：单线程 200ms 轮转睡眠 + 一个索引构建线程。
pub fn spawn_poller(shared: Arc<Shared>) {
    thread::spawn(move || {
        let idx = Arc::new(IndexShared::new());
        let idx_build = idx.clone();
        let (root_tx, root_rx) = mpsc::channel::<PathBuf>();
        thread::spawn(move || index_build_loop(idx_build, root_rx));
        let mut runtime = Runtime::new(idx, root_tx);
        loop {
            thread::sleep(POLL_INTERVAL);
            runtime.tick(&shared);
        }
    });
}

/// song 帧（与 MalodyV 通道同形 + `meta.judge`）。
pub fn build_song_frame(
    shared: &Shared,
    entry: &LibraryEntry,
    meta: &ChartMeta,
    rate: f64,
    judge: char,
    seq: u64,
) -> Option<serde_json::Value> {
    let raw_text = match fs::read_to_string(&entry.path) {
        Ok(text) => text,
        Err(err) => {
            log_at(
                "warn",
                &format!(
                    "malody4 chart read failed: {} ({err})",
                    entry.path.display()
                ),
            );
            return None;
        }
    };
    if raw_text.len() > MAX_PAYLOAD_BYTES {
        shared.shell_errors.lock().unwrap().push(format!(
            "Malody 4 chart too large, skipped: {} (>5MB)",
            entry.path.display()
        ));
        return None;
    }
    Some(serde_json::json!({
        "requestId": format!("n{}", seq),
        "source": "malody4",
        "identity": format!("mdy4:{}", entry.md5),
        "modData": { "speedRate": format!("{:.5}", rate) },
        "meta": {
            "title": meta.title,
            "artist": meta.artist,
            "version": meta.version,
            "keys": meta.keys,
            "devMsd8": [],
            "judge": judge,
        },
        "cover": null,
        "rawText": raw_text,
    }))
}

/// 当前 WS 客户端里最大的连接 id（加锁时间极短，不持锁做任何其他事）。
pub fn max_conn_id(shared: &Shared) -> Option<u64> {
    shared.sinks.lock().unwrap().iter().map(|(id, _)| *id).max()
}
