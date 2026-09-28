// `osu` 模块门面 + 轮询线程（Wave B2：stable 单字段真机读取的**最小**版本）。
//
// 本步只发布两枚字段（`state.name` + `beatmap.md5`）以及它们所依赖的两条链，接口极简：
// - `spawn_osu_reader()`：从 `server::start` 与 `osu_compat` 并列启动（**不改 state 帧、
//   不改契约版本**——那是 Step 9；因此这里不往 `Shared` 加字段，状态只在本线程的 `Mutex` 里）。
// - `Reader::latest()`：最新快照 + 原因字符串（后续步骤把它接进 state 帧 / 影子比对）。
//
// 节奏（计划 §3.4 的常数）：附着失败 2s 重试；正常 250ms 一 tick；硬错误即 detach，
// 下次 tick 重新发现进程（进程重启会换 PID，`select_target()` 每次重新解析）。
//
// 线程模型：单线程循环 + 一个 dev-only 的 tosu WS 客户端线程（`compare.rs`）。
// 所有句柄都在 `win::Target` 的 `Drop` 里关闭——`detach` 就是 `target = None`。

pub mod beatmap_file;
pub mod compare;
pub mod keys;
pub mod model;
pub mod packet;
pub mod patterns;
pub mod scan;
pub mod shadow;
pub mod win;

use crate::osu::model::{Client, Reason, Snapshot};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// 附着失败后的重试间隔。
pub const ATTACH_RETRY: Duration = Duration::from_secs(2);
/// 兜底路径（PID 扫描）失败后的重试间隔：一次全量扫描是秒级开销，不能 2 秒来一次。
pub const SWEEP_RETRY: Duration = Duration::from_secs(30);
/// 正常 tick。
pub const TICK: Duration = Duration::from_millis(250);
/// 锚点扫描失败后的重试间隔（扫描要 ~8s，不能用 250ms 去撞）。
pub const SCAN_RETRY: Duration = Duration::from_secs(5);
/// 区域过滤：先用参考过滤器（RW/RWX），两枚锚点都没命中再退化到宽过滤器。
pub const FILTER_READY: &[u32] = &[win::FILTER_RW, win::FILTER_READABLE];
/// 单进程区域数上限（防某个进程挖出一个病态长的 VAD 列表把扫描拖死）。
const REGION_LIMIT: usize = 65_536;
/// 单锚点的命中列表上限：同一签名可能出现多次（P1 的 `configurationAddr` 有 2 处），
/// 上限给 16 是为了"同形状代码"不再把真正的那一处挤掉，同时不让扫描无界累积。
const ANCHOR_HIT_LIMIT: usize = 16;

/// 读取线程对外可见的状态。
#[derive(Clone, Debug, Default)]
pub struct ReaderState {
    /// 最新快照（三枚锚点齐了才有 `beatmap.*`；`state_name` 只要 `statusPtr` 可用就有）。
    pub snapshot: Option<Snapshot>,
    /// 不可用原因（`Some` = 未发布；字面量见 `model::Reason::as_str`）。
    pub reason: Option<String>,
    pub pid: Option<u32>,
    pub image_path: Option<String>,
    pub client: Option<String>,
    /// 锚点定址结果（诊断/证据用）。
    pub anchors: Option<AnchorAddrs>,
    pub scan_ms: Option<u128>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AnchorAddrs {
    pub status_ptr: u32,
    pub base_addr: u32,
    pub menu_mods_ptr: u32,
    /// B3 新增（best-effort；未命中 = 0）。
    pub settings_class_addr: u32,
}

/// 读取线程的句柄（`spawn_osu_reader` 返回；可 clone 给消费者）。
#[derive(Clone)]
pub struct Reader {
    inner: Arc<Mutex<ReaderState>>,
}

impl Reader {
    pub fn latest(&self) -> ReaderState {
        self.inner.lock().unwrap().clone()
    }

    /// 最新快照（`None` = 未附着/未定址/被 detach）。
    pub fn snapshot(&self) -> Option<Snapshot> {
        self.inner.lock().unwrap().snapshot.clone()
    }

    /// 最新原因字符串（`None` = 正常发布）。
    pub fn reason(&self) -> Option<String> {
        self.inner.lock().unwrap().reason.clone()
    }
}

/// 启动读取线程（`server::start` 调用）。返回句柄；线程自身永不 panic 退出。
pub fn spawn_osu_reader() -> Reader {
    let reader = Reader {
        inner: Arc::new(Mutex::new(ReaderState::default())),
    };
    let handle = reader.clone();
    thread::spawn(move || run(handle));
    reader
}

/// 进程级读取句柄：`server::start` 注入一次。本步没有 state 帧消费点（Step 9 接），
/// 但证据/诊断入口需要一个与 `spawn` 调用点解耦的稳定读取入口。
pub static INSTANCE: std::sync::OnceLock<Reader> = std::sync::OnceLock::new();

/// 便捷读取入口（未启动时返回 `None`）。
pub fn instance() -> Option<Reader> {
    INSTANCE.get().cloned()
}

/// 主循环：发现 → 附着 → 定址 → 每 tick 发布。
#[cfg(windows)]
fn run(reader: Reader) {
    let mut attached: Option<win::Target> = None;
    let mut anchors: Option<patterns::AnchorTable> = None;
    let mut last_scan_fail: Option<std::time::Instant> = None;
    let mut last_reason: Option<Reason> = None;

    // dev-only 对拍装置：tosu 载荷进 Mutex，本线程每 tick 落一条 JSONL。
    let compare_on = compare::enabled();
    let feed = if compare_on {
        let feed = Arc::new(compare::TosuFeed::default());
        compare::spawn_tosu_client(feed.clone());
        Some(feed)
    } else {
        None
    };
    let mut writer = compare_on.then(|| compare::SampleWriter::new(compare::samples_path()));
    // 对拍节奏（默认 100 ms；`MMA_OSU_COMPARE_INTERVAL_MS` 覆盖）。关闭对拍时退回 `TICK`。
    let compare_interval = if compare_on {
        compare::interval()
    } else {
        TICK
    };
    if compare_on {
        eprintln!(
            "[osu] compare: interval={}ms out={}",
            compare_interval.as_millis(),
            compare::samples_path().display()
        );
    }
    // `.osu` 解析缓存：key = `beatmap.checksum`（同一张图只解析一次；换图/换 md5 才重读）。
    let mut osu_cache: Option<OsuFileCache> = None;

    loop {
        // ① 附着（进程重启/被 detach 后自动重新发现）。
        if attached.is_none() {
            match win::select_target() {
                Ok(target) => {
                    let state = AnchorAddrs::default();
                    publish(
                        &reader,
                        None,
                        None,
                        Some(target.pid),
                        Some(target.image_path.to_string_lossy().to_string()),
                        Some(state),
                        None,
                    );
                    eprintln!(
                        "[osu] attached pid={} bitness=0x{:04X} module_base=0x{:08X} path={}",
                        target.pid,
                        target.bitness,
                        target.module_base,
                        target.image_path.display()
                    );
                    attached = Some(target);
                    anchors = None;
                }
                Err(reason) => {
                    let detail = reason.as_str();
                    if last_reason.as_ref().map(Reason::as_str) != Some(detail.clone()) {
                        if reason != Reason::ProcessNotFound {
                            eprintln!("[osu] no target: {detail}");
                        } else {
                            eprintln!(
                                "[osu] no target: process-not-found (discovery={})",
                                if win::last_discovery_used_sweep() {
                                    "pid-sweep"
                                } else {
                                    "toolhelp32"
                                }
                            );
                        }
                        last_reason = Some(reason.clone());
                    }
                    publish(&reader, None, Some(detail), None, None, None, None);
                    // 兜底扫描（秒级）失败后拉长重试，避免每 2 秒全量重扫。
                    thread::sleep(if win::last_discovery_used_sweep() {
                        SWEEP_RETRY
                    } else {
                        ATTACH_RETRY
                    });
                    continue;
                }
            }
        }

        // ② 锚点定址（每进程一次；失败按 SCAN_RETRY 重试）。
        if anchors.is_none() {
            let retry_due = last_scan_fail
                .map(|at| at.elapsed() >= SCAN_RETRY)
                .unwrap_or(true);
            if retry_due {
                let target = attached.as_ref().expect("attached");
                match resolve_anchors(target) {
                    Ok((table, elapsed_ms)) => {
                        eprintln!(
                            "[osu] anchors resolved in {}ms: statusPtr=0x{:08X} baseAddr=0x{:08X} menuModsPtr=0x{:08X} settingsClassAddr=0x{:08X}",
                            elapsed_ms,
                            table.status_ptr.unwrap_or(0),
                            table.base_addr.unwrap_or(0),
                            table.menu_mods_ptr.unwrap_or(0),
                            table.settings_class_addr.unwrap_or(0)
                        );
                        anchors = Some(table);
                        last_scan_fail = None;
                    }
                    Err(reason) => {
                        let detail = reason.as_str();
                        eprintln!("[osu] anchor scan failed: {detail}");
                        last_scan_fail = Some(std::time::Instant::now());
                        last_reason = Some(reason.clone());
                        publish(&reader, None, Some(detail), None, None, None, None);
                        // 扫描失败不立刻 detach（可能是过滤器不匹配而不是目标变了）；
                        // 但连续失败时把 target 丢掉重来（进程可能已经被替换）。
                        attached = None;
                        thread::sleep(ATTACH_RETRY);
                        continue;
                    }
                }
            } else {
                thread::sleep(TICK);
                continue;
            }
        }

        // ③ 每 tick 读一次（读失败即 detach → 下一轮重新发现）。
        {
            let target = attached.as_ref().expect("attached");
            let table = anchors.as_ref().expect("anchors");
            match read_snapshot(target, table) {
                Ok((snapshot, degraded_reason)) => {
                    last_reason = None;
                    let mut state = snapshot.clone();
                    if let Some(field) = degraded_reason.as_ref() {
                        state.degraded_fields.push(field.clone());
                    }
                    // `.osu` 解析（C6）：按 checksum 缓存；只有换图时才真的读盘。
                    attach_beatmap_file(&mut state, &mut osu_cache);
                    publish(
                        &reader,
                        Some(state.clone()),
                        None,
                        Some(target.pid),
                        Some(target.image_path.to_string_lossy().to_string()),
                        Some(AnchorAddrs {
                            status_ptr: table.status_ptr.unwrap_or(0),
                            base_addr: table.base_addr.unwrap_or(0),
                            menu_mods_ptr: table.menu_mods_ptr.unwrap_or(0),
                            settings_class_addr: table.settings_class_addr.unwrap_or(0),
                        }),
                        None,
                    );
                    if let Some(writer) = writer.as_mut() {
                        let tosu = feed.as_ref().and_then(|f| f.latest());
                        let record = compare::record(&state, None, tosu.as_ref());
                        writer.write(&record);
                    }
                }
                Err(reason) => {
                    let detail = reason.as_str();
                    eprintln!("[osu] read failed, detaching: {detail}");
                    last_reason = Some(reason.clone());
                    publish(&reader, None, Some(detail), None, None, None, None);
                    attached = None;
                    anchors = None;
                    osu_cache = None;
                    if let Some(writer) = writer.as_mut() {
                        let tosu = feed.as_ref().and_then(|f| f.latest());
                        let record = compare::record(
                            &Snapshot::default(),
                            Some(reason.as_str()),
                            tosu.as_ref(),
                        );
                        writer.write(&record);
                    }
                }
            }
        }
        thread::sleep(compare_interval);
    }
}

#[cfg(not(windows))]
fn run(reader: Reader) {
    publish(
        &reader,
        None,
        Some(Reason::ReadError.as_str()),
        None,
        None,
        None,
        None,
    );
    loop {
        thread::sleep(ATTACH_RETRY);
    }
}

fn publish(
    reader: &Reader,
    snapshot: Option<Snapshot>,
    reason: Option<String>,
    pid: Option<u32>,
    image_path: Option<String>,
    anchors: Option<AnchorAddrs>,
    scan_ms: Option<u128>,
) {
    let mut state = reader.inner.lock().unwrap();
    state.snapshot = snapshot;
    state.reason = reason;
    state.pid = pid.or(state.pid);
    state.image_path = image_path.or(state.image_path.clone());
    state.anchors = anchors.or(state.anchors);
    state.scan_ms = scan_ms.or(state.scan_ms);
    state.client = Some(Client::Stable.as_str().to_string());
}

/// 锚点定址：先参考过滤器（RW/RWX），两枚关键锚点缺任一就用宽过滤器重扫。
///
/// 每次重扫都要遍历 ~1200–2200 个区域并读 0.6–1.3 GB（P1 实测 ~8s / ~21s），
/// 所以只在"两枚关键锚点（statusPtr/baseAddr）缺一个"时才退化——菜单 mod 掩码
/// 或 songs 链（B3 的 best-effort 锚点）缺席不值得把扫描时间翻三倍。
#[cfg(windows)]
fn resolve_anchors(target: &win::Target) -> Result<(patterns::AnchorTable, u128), Reason> {
    use std::time::Instant;
    let started = Instant::now();
    let mut table = patterns::AnchorTable::default();
    // 扫描顺序固定为循序表（statusPtr → baseAddr → menuModsPtr → settingsClassAddr），但
    // **先算便宜的键**：`statusPtr` 的每候选自证只花 1 次解引用 + 1 次读，可以用来把
    // "同形状代码"的假阳性直接筛掉；其余自证要读字符串/指针，留到后面。
    let mut order: Vec<&'static str> = vec![
        "statusPtr",
        "baseAddr",
        "menuModsPtr",
        // best-effort（B3）：缺席只降级 `folders.songs`，不进 `missing()`。
        "settingsClassAddr",
    ];
    let mut degraded: Vec<&'static str> = Vec::new();

    for (index, mask) in FILTER_READY.iter().enumerate() {
        let regions = win::walk_regions(target.handle(), *mask, REGION_LIMIT);
        let mut stats = scan::ScanStats {
            regions: regions.len(),
            ..Default::default()
        };
        for key in &mut order {
            if table.get(key).is_some() {
                continue;
            }
            let Some(anchor) = patterns::ANCHORS.iter().find(|a| a.key == *key) else {
                continue;
            };
            let pattern = scan::Pattern::parse(anchor.pattern)
                .map_err(|_| Reason::SignatureMiss(anchor.key))?;
            // 同一签名可能出现多次（P1 实测 `configurationAddr` 有 2 处，其中一处只是同形状
            // 代码），所以取列表再**逐个自证**：只有解引用后落在结构上说得通的那一个才算。
            let candidates = scan::find_in_regions(
                target.handle(),
                &regions,
                &pattern,
                anchor.offset,
                ANCHOR_HIT_LIMIT,
                &mut stats,
            );
            let chosen = candidates
                .iter()
                .copied()
                .find(|addr| anchor_proves_out(target, *key, *addr, &regions));
            eprintln!(
                "[osu] scan {} -> {} hit(s) {} chosen={:?}",
                anchor.key,
                candidates.len(),
                candidates
                    .iter()
                    .map(|a| format!("0x{a:08X}"))
                    .collect::<Vec<_>>()
                    .join(" "),
                chosen.map(|a| format!("0x{a:08X}"))
            );
            match chosen {
                Some(addr) => table.set(anchor.key, addr),
                None => {
                    if !degraded.contains(key) {
                        degraded.push(key);
                    }
                }
            }
        }
        eprintln!(
            "[osu] scan filter#{} regions={} chunks_ok={} chunks_failed={} bytes={} elapsed={}ms unresolved={:?}",
            index,
            stats.regions,
            stats.chunks_ok,
            stats.chunks_failed,
            stats.bytes_read,
            started.elapsed().as_millis(),
            degraded
        );
        if table.status_ptr.is_some() && table.base_addr.is_some() {
            break;
        }
    }
    match table.missing() {
        Some(key) => Err(Reason::SignatureMiss(key)),
        None => Ok((table, started.elapsed().as_millis())),
    }
}

/// 逐候选自证（L1 结构校验）：只有"解引用后说得通"的候选才被采纳。
///
/// 为什么不是"取首个命中"：同一签名可能出现多次（同形状代码），而 P1 的探针取首个并全部
/// 命中，是在**另一份进程实例**（ASLR 不同、且当时映像路径不同）上发生的。本步在真机上把
/// **原始命中地址与各候选位移**逐个打印核对（`evidence/B2-stable-minimal/diag-probe-offsets.txt`），
/// 结论：`statusPtr` 必须 `-0x4`、`menuModsPtr` 必须 `+0x9`——两处的原始命中地址读出来都
/// 不是指针，位移之后才是指针。因此判据写成**可自证的结构**（不依赖 tosu，离线同样成立）：
/// - `statusPtr`：`read_pointer(addr)` 落在小整数状态域（`0..=15`，覆盖观测到的 0/2/5/7）
/// - `baseAddr`：`read_pointer(addr - 0xC)` 是非空指针，且落在可读区域里
/// - `menuModsPtr`：`read_pointer(addr)` 的**第一跳**落在可读区域里（掩码本体是什么值
///   由 L2 不变量管，不在这里判——真机实测该掩码此时是 `0x20000000`（ScoreV2 位），
///   只有"已知位"的白名单会误判成假阳性）
///
/// 校验失败 ⇒ 该键进 `unresolved`，由调用方给出 `signature-miss:<key>`。
///
/// B3 新增 `settingsClassAddr` 的判据（比其余三枚更严，因为它的位移取自一条 `A1 <addr>`
/// 的立即数，错位 1–4 字节时**残字节仍可能被当成合法指针**）：
/// ① `read_pointer(addr)` 得到一个 **4 字节对齐**、`>= 0x10000`（排除小整数/空指针）的对象；
/// ② 该对象落在可读区；③ 对象 `+0x0` 的 MethodTable/vtable 也是 4 字节对齐、非空、落在可读区。
pub fn anchor_proves_out(
    target: &win::Target,
    key: &str,
    addr: u32,
    regions: &[scan::Region],
) -> bool {
    match key {
        "statusPtr" => win::read_pointer(target, addr)
            .map(|value| value <= STATE_INDEX_MAX)
            .unwrap_or(false),
        "baseAddr" => win::read_pointer(target, addr.wrapping_sub(0xC))
            .map(|object| object != 0 && region_contains(regions, object))
            .unwrap_or(false),
        "menuModsPtr" => win::read_u32(target, addr)
            .map(|pointer| pointer != 0 && region_contains(regions, pointer))
            .unwrap_or(false),
        "settingsClassAddr" => match win::read_pointer(target, addr) {
            Ok(object) => {
                if let Some(why) = plausible_object(regions, object) {
                    eprintln!(
                        "[osu] diag {key} candidate=0x{addr:08X} reject=object object=0x{object:08X} why={why}"
                    );
                    return false;
                }
                match win::read_u32(target, object) {
                    Ok(vtable) => {
                        if let Some(why) = plausible_object(regions, vtable) {
                            eprintln!(
                                "[osu] diag {key} candidate=0x{addr:08X} reject=vtable object=0x{object:08X} vtable=0x{vtable:08X} why={why}"
                            );
                            return false;
                        }
                        true
                    }
                    Err(reason) => {
                        eprintln!(
                            "[osu] diag {key} candidate=0x{addr:08X} reject=vtable-read object=0x{object:08X} reason={}",
                            reason.as_str()
                        );
                        false
                    }
                }
            }
            Err(reason) => {
                eprintln!(
                    "[osu] diag {key} candidate=0x{addr:08X} reject=object-read reason={}",
                    reason.as_str()
                );
                false
            }
        },
        _ => false,
    }
}

/// 指针合理性（32 位用户态最小判据）：4 字节对齐、非空、≥ 64 KiB、落在已枚举的可读区。
/// 返回 `Some(原因)` = 不合理。
pub fn plausible_object(regions: &[scan::Region], pointer: u32) -> Option<&'static str> {
    if pointer == 0 {
        return Some("null");
    }
    if !patterns::is_aligned(pointer) {
        return Some("unaligned");
    }
    if pointer < 0x1_0000 {
        return Some("too-low");
    }
    if !region_contains(regions, pointer) {
        return Some("not-in-region");
    }
    None
}

/// 状态整数上界：tosu 的 `GameState` 枚举取到 7（`resultScreen`），留一倍余量给未观测值。
/// 注意这里只是"候选自证"的粗筛，**不是** §3.4 I-01 的名字表（观测集外仍发布 `""`）。
const STATE_INDEX_MAX: u32 = 15;

fn region_contains(regions: &[scan::Region], addr: u32) -> bool {
    let addr = addr as usize;
    regions
        .iter()
        .any(|r| addr >= r.base && addr < r.base + r.size)
}

/// 单次采样：先读状态（只要 `statusPtr` 就够），再读谱面链。
///
/// 返回 `(快照, 降级字段)`：`degraded_fields` 非空 = "仍发布但某字段缺失/降级"
/// （计划 §3.4：`degraded` ≠ `unhealthy`）。**指针链硬失败**（对象指针读不出来）
/// 才返回 `Err` → 调用方 detach。
#[cfg(windows)]
fn read_snapshot(
    target: &win::Target,
    table: &patterns::AnchorTable,
) -> Result<(Snapshot, Option<String>), Reason> {
    let mut snapshot = Snapshot {
        client: Some(Client::Stable),
        pid: target.pid,
        ..Default::default()
    };
    let mut degraded: Option<String> = None;

    if let Some(status_ptr) = table.status_ptr {
        match win::read_pointer(target, status_ptr) {
            Ok(raw) => {
                let index = raw as i32;
                snapshot.state_number = Some(index);
                let name = model::state_name_for(index);
                snapshot.state_name = Some(name.to_string());
                // I-01：观测集外 → 发布 "" 并上报降级，**不**判 unhealthy。
                if name.is_empty() {
                    degraded = Some("state.name".to_string());
                }
            }
            Err(reason) => return Err(reason),
        }
    }

    if let Some(base_addr) = table.base_addr {
        let beatmap_addr = base_addr.wrapping_sub(0xC);
        let object = win::read_pointer(target, beatmap_addr)?;
        snapshot.beatmap_object = Some(object);
        if object != 0 {
            // 每个字段独立降级：读不出来的那个进 degraded_fields，其余照发。
            let mut read_string = |offset: u32, field: &'static str| -> Option<String> {
                let slot = object.wrapping_add(offset);
                let ptr = match win::read_u32(target, slot) {
                    Ok(p) => p,
                    Err(_) => {
                        if degraded.is_none() {
                            degraded = Some(field.to_string());
                        }
                        return None;
                    }
                };
                match win::read_csharp_string(target, ptr) {
                    Ok(text) => Some(text),
                    Err(_) => {
                        if degraded.is_none() {
                            degraded = Some(field.to_string());
                        }
                        None
                    }
                }
            };
            snapshot.checksum = read_string(0x6C, "beatmap.md5");
            snapshot.filename = read_string(0x90, "files.beatmap");
            snapshot.folder = read_string(0x78, "folders.beatmap");
            snapshot.version = read_string(0xAC, "beatmap.version");
            snapshot.artist = read_string(0x18, "beatmap.artist");
            snapshot.title = read_string(0x24, "beatmap.title");
            snapshot.mapper = read_string(0x7C, "beatmap.mapper");
            snapshot.map_id = win::read_i32(target, object.wrapping_add(0xC8)).ok();
            snapshot.set_id = win::read_i32(target, object.wrapping_add(0xCC)).ok();
            snapshot.map_id_bits = win::read_u32(target, object.wrapping_add(0xC8)).ok();
        }
    }

    if let Some(menu_mods_ptr) = table.menu_mods_ptr {
        snapshot.menu_mods_mask = win::read_pointer(target, menu_mods_ptr).ok();
    }

    // `folders.songs` 链（B3 新增；best-effort，失败只降级不判 unhealthy）：
    // `[[[settingsClassAddr+0x8]+0xB8]+0x4]` → C# 字符串 = osu! cfg 的 `BeatmapDirectory`
    // **值**（本机观测 `"Songs"`，名字不是路径）。
    if let Some(settings_class_addr) = table.settings_class_addr {
        snapshot.songs_cfg_value = read_songs_cfg_value(target, settings_class_addr);
        if snapshot.songs_cfg_value.is_none() {
            degraded.get_or_insert_with(|| "folders.songs".to_string());
        }
        // 绝对目录 = `join(dirname(exe), 值)` 存在则用它，否则用值本身（镜像 tosu 的规则）。
        snapshot.songs_folder = snapshot
            .songs_cfg_value
            .as_deref()
            .map(|value| resolve_songs_folder(&target.image_path, value));
    }
    // songs 链未走通时的**回退链**（B3 实测：cfg 链的语义尚未关闭，见 `patterns.rs` 的
    // 未关闭项）。回退顺序固定，且**只在路径真的存在时**才采纳——这样"换图时路径前缀
    // 不变"（`folder\filename` 才是身份的关键部分，前缀只是读盘用）：
    // ① `dirname(exe)\Songs`（stable 默认安装形态，本机命中）② `dirname(exe)\..\Songs`
    // （游戏与数据分离）③ `dirname(exe)\Data\Songs`（旧安装形态）。
    // 三处都不存在 ⇒ 保持 `None` ⇒ 对拍记 `skipped:osu-file-not-parsed`（**不猜**）。
    if snapshot.songs_folder.is_none() {
        snapshot.songs_folder = fallback_songs_folder(&target.image_path);
        if snapshot.songs_folder.is_some() {
            degraded.get_or_insert_with(|| "folders.songs(fallback-path)".to_string());
        }
    }

    Ok((snapshot, degraded))
}

/// songs 目录的回退解析（只读盘存在性判断；见 `read_snapshot` 的注释）。
pub fn fallback_songs_folder(image_path: &std::path::Path) -> Option<String> {
    let dir = image_path.parent()?;
    for candidate in [
        dir.join("Songs"),
        dir.join("..").join("Songs"),
        dir.join("Data").join("Songs"),
    ] {
        if candidate.is_dir() {
            return Some(candidate.to_string_lossy().to_string());
        }
    }
    None
}

/// `[[[settingsClassAddr+0x8]+0xB8]+0x4]` → C# 字符串（32 位约定）。
///
/// 三段链全部走 `wrapping_add`（**不**用 `+`：地址溢出在 debug 构建会 panic，而这里
/// 只是"读不到就降级"的路径）。返回 `None` = 任意一跳失败。
///
/// ⚠️ **本步未能在真机上把这条链走通**（见 `patterns.rs::SETTINGS_CLASS_ADDR` 的未关闭项）：
/// 锚点本身能命中并通过候选自证，但 `[对象+0x8]` 那一跳读出来的是指向映像内的值，
/// `+0xB8` 一跳随即 `read-error`；从"锚点当槽"与"锚点当对象"两种解释都失败。
/// 该字段因此在 B3 里恒为 `degraded`（`folders.songs` 不出现在载荷里，对拍记
/// `skipped:songs-chain:unavailable`），**不影响其余字段**。
#[cfg(windows)]
fn read_songs_cfg_value(target: &win::Target, settings_class_addr: u32) -> Option<String> {
    let first = win::read_pointer(target, settings_class_addr.wrapping_add(0x8)).ok()?;
    if first == 0 {
        return None;
    }
    let second = win::read_u32(target, first.wrapping_add(0xB8)).ok()?;
    if second == 0 {
        return None;
    }
    let slot = win::read_u32(target, second.wrapping_add(0x4)).ok()?;
    win::read_csharp_string(target, slot).ok()
}

/// 镜像参照实现的 `folders.songs` 规则（**只读盘，不写**）：
/// `join(dirname(osu!.exe), cfg 值)` 存在 ⇒ 用它；否则用 cfg 值本身（P1-notes F2；
/// 本机实测 `D:\Games\osu!` + `Songs` == tosu 的 `folders.songs`）。
pub fn resolve_songs_folder(image_path: &std::path::Path, value: &str) -> String {
    let candidate = match image_path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join(value),
        _ => std::path::PathBuf::from(value),
    };
    if candidate.is_dir() {
        candidate.to_string_lossy().to_string()
    } else {
        value.to_string()
    }
}

/// `.osu` 解析（C6）：按 `beatmap.checksum` 缓存，命中即复用（**成功与失败都缓存**——
/// 否则一张坏图会让本线程以对拍节奏反复读盘）。
///
/// 路径 = `songs_folder \ folder \ filename`（stable 的三段拼接，照 §3.3 的"读盘"约定）。
/// 任何一步失败都只在证据里留 `beatmap_file_error`，**不影响发布**（C6 是对拍项，不是门）。
fn attach_beatmap_file(snapshot: &mut Snapshot, cache: &mut Option<OsuFileCache>) {
    snapshot.beatmap_file = None;
    snapshot.beatmap_file_error = None;
    snapshot.beatmap_file_mismatches.clear();
    let (Some(songs), Some(folder), Some(filename), Some(checksum)) = (
        snapshot.songs_folder.as_deref(),
        snapshot.folder.as_deref(),
        snapshot.filename.as_deref(),
        snapshot.checksum.as_deref(),
    ) else {
        snapshot.beatmap_file_error = Some("songs-or-path-unavailable".to_string());
        return;
    };
    let path = std::path::Path::new(songs).join(folder).join(filename);
    snapshot.beatmap_file_path = Some(path.to_string_lossy().to_string());

    if cache.as_ref().map(|c| c.checksum.as_str()) != Some(checksum) {
        let loaded = match std::fs::read(&path) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(text) => Ok(beatmap_file::parse(&text)),
                Err(e) => Err(format!("not-utf8: {e}")),
            },
            Err(e) => Err(format!("read: {e}")),
        };
        *cache = Some(OsuFileCache {
            checksum: checksum.to_string(),
            path: path.to_string_lossy().to_string(),
            loaded,
        });
    }
    let entry = cache.as_ref().expect("cache filled above");
    match entry.loaded.as_ref() {
        Ok(file) => {
            snapshot.beatmap_file_mismatches = mismatches_for(file, snapshot);
            snapshot.beatmap_file = Some(file.clone());
        }
        Err(error) => snapshot.beatmap_file_error = Some(error.clone()),
    }
}

/// 同 checksum ⇒ 同一张图：缓存 `.osu` 的解析结果（或失败原因）。
struct OsuFileCache {
    checksum: String,
    /// 诊断用（换图时日志里能看到读的是哪个文件）。
    #[allow(dead_code)]
    path: String,
    loaded: Result<beatmap_file::BeatmapFile, String>,
}

/// 头字段交叉校验（不一致的字段名；空 = 全等或无法比较）。
fn mismatches_for(file: &beatmap_file::BeatmapFile, snapshot: &Snapshot) -> Vec<String> {
    file.header_mismatches(
        snapshot.title.as_deref(),
        snapshot.artist.as_deref(),
        snapshot.mapper.as_deref(),
        snapshot.version.as_deref(),
        snapshot.map_id,
        snapshot.set_id,
    )
    .into_iter()
    .map(|field| field.to_string())
    .collect()
}

#[cfg(not(windows))]
fn read_songs_cfg_value(_target: &win::Target, _addr: u32) -> Option<String> {
    None
}

// ---- 非 Windows 桩（同 `malody4` 约定：同名同签名，行为为空）----

#[cfg(not(windows))]
fn resolve_anchors(_target: &win::Target) -> Result<(patterns::AnchorTable, u128), Reason> {
    Err(Reason::ReadError)
}

#[cfg(not(windows))]
fn read_snapshot(
    _target: &win::Target,
    _table: &patterns::AnchorTable,
) -> Result<(Snapshot, Option<String>), Reason> {
    Err(Reason::ReadError)
}

/// 便于外部（后续步骤的服务端装配）拿到"当前是否发布了快照"的布尔。
pub fn healthy(state: &ReaderState) -> bool {
    state.snapshot.is_some() && state.reason.is_none()
}

/// 诊断行：一行文本说明当前状态（日志/证据用）。
pub fn describe(state: &ReaderState) -> String {
    match (&state.snapshot, &state.reason) {
        (Some(snapshot), None) => {
            let keys = keys::derive(snapshot);
            format!(
                "pid={:?} state={:?} md5={:?} identity={}",
                state.pid, snapshot.state_name, snapshot.checksum, keys.identity
            )
        }
        (_, Some(reason)) => format!("unavailable: {reason}"),
        _ => "unavailable: <no snapshot>".to_string(),
    }
}

/// 供后续步骤使用的便捷入口：快照 + 原因。
pub fn latest_pair(reader: &Reader) -> (Option<Snapshot>, Option<String>) {
    let state = reader.latest();
    (state.snapshot, state.reason)
}

/// 证据/日志用的 JSON 摘要（不含任何游戏内存地址之外的隐私内容）。
pub fn state_json(reader: &Reader) -> Value {
    let state = reader.latest();
    let keys = state
        .snapshot
        .as_ref()
        .map(keys::derive)
        .unwrap_or_default();
    serde_json::json!({
        "pid": state.pid,
        "client": state.client,
        "reason": state.reason,
        "state_name": state.snapshot.as_ref().and_then(|s| s.state_name.clone()),
        "checksum": state.snapshot.as_ref().and_then(|s| s.checksum.clone()),
        "identity": keys.identity,
        "mod_signature": keys.mod_signature,
        // B3 新增：songs 链（原始 cfg 值 + 解析出的绝对目录）与 C6 的两个值。
        "songs_cfg_value": state.snapshot.as_ref().and_then(|s| s.songs_cfg_value.clone()),
        "songs_folder": state.snapshot.as_ref().and_then(|s| s.songs_folder.clone()),
        "first_object": state.snapshot.as_ref().and_then(|s| s.first_object()),
        "last_object": state.snapshot.as_ref().and_then(|s| s.last_object()),
        "degraded_fields": state.snapshot.as_ref().map(|s| s.degraded_fields.clone()).unwrap_or_default(),
    })
}

#[cfg(test)]
#[path = "../../tests-local/osu_beatmap_corpus.rs"]
mod tests_beatmap_corpus;
#[cfg(test)]
#[path = "../../tests-local/osu_beatmap_file.rs"]
mod tests_beatmap_file;
#[cfg(test)]
#[path = "../../tests-local/osu_keys.rs"]
mod tests_keys;
#[cfg(test)]
#[path = "../../tests-local/osu_model.rs"]
mod tests_model;
#[cfg(test)]
#[path = "../../tests-local/osu_scan.rs"]
mod tests_scan;
#[cfg(test)]
#[path = "../../tests-local/osu_shadow.rs"]
mod tests_shadow;
