// Malody 请求轮询（壳侧）——编辑器按钮触发的文件通道：
//
// 1) 编辑器插件 WriteFile 写请求——Malody 自动加谱面名前缀，实际文件为
//    {谱面目录}/<谱面base名>_mma_request.json（实测确认）。
// 2) 本模块扫描 {malodyRoot}/chart/**/*_mma_request.json（两级目录 mtime 快筛）
//    → 谱面本体 = 同目录 <base>.mc|.osu（base 精确锁定，与命名/格式无关）
//    → 发 song 帧 → 页面分析 → 卡片展示（壳即展示端，不回写 txt 到游戏内）。
// 3) 处理完删除 request 文件（防残留污染：切难度时旧 request 不得重处理）；
//    删除失败（ACL）→ 日志提示，handled 集合兜底防同 mtime 重复。
//
// 壳不做任何「自动捕获最新谱面」——只有编辑器按钮触发才分析。

use crate::server::{broadcast, Shared};
use crate::server::log::{log_at, log_line};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime};

const POLL_INTERVAL: Duration = Duration::from_secs(1);

fn md5_hex(input: &str) -> String {
    use md5::{Digest, Md5};
    let mut hasher = Md5::new();
    hasher.update(input.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

/// Malody WriteFile 生成的请求文件名为 `<谱面base名>_mma_request.json`。
/// 谱面本体 = 同目录下的 `<谱面base名>.mc`（或 .osu——Malody 也可读 osu 谱）。
/// 用 base 精确定位谱面——与真实谱面命名（title/artist 不规整）无关。
fn chart_path_for(req: &std::path::Path) -> Option<PathBuf> {
    let name = req.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let base = name.trim_end_matches("_mma_request.json");
    if base.is_empty() {
        return None;
    }
    let dir = req.parent()?;
    for ext in ["mc", "osu"] {
        let p = dir.join(format!("{}.{}", base, ext));
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// 增量扫描：维护目录 mtime 缓存，只对「mtime 变化的目录」深入。
/// Malody 结构 chart/<song>/<difficulty>/；request 写在 <difficulty>/ 下。
/// Windows 目录 mtime 只在「直接子项」变化时更新——写文件到 <difficulty>/
/// 只更新 <difficulty>/ 的 mtime，不会向上传播到 chart/<song>/。因此快筛必须
/// 覆盖两层：chart/ 直接子目录（song 层）+ 每个 song 的直接子目录（difficulty
/// 层，通常 _song_XXXX/N 形态）。song 层数量数百、difficulty 层数千——
/// 每轮 ~几千次 stat（<100ms），每 2s 一次可接受。
fn scan_requests(
    root: &std::path::Path,
    dir_cache: &mut HashMap<PathBuf, SystemTime>,
) -> Vec<(PathBuf, SystemTime)> {
    let mut out = Vec::new();
    let chart = root.join("chart");
    let mut changed_leaves: Vec<PathBuf> = Vec::new();

    let stat_mt = |p: &std::path::Path| fs::metadata(p).and_then(|m| m.modified()).ok();

    // 1. song 层：chart/ 直接子目录（mtime 对比）。
    let Ok(entries) = fs::read_dir(&chart) else {
        return out;
    };
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        if !ft.is_dir() {
            continue;
        }
        let path = entry.path();
        let mt = stat_mt(&path);
        let cached = dir_cache.get(&path).copied();
        dir_cache.insert(path.clone(), mt.unwrap_or(SystemTime::UNIX_EPOCH));
        if mt.is_some() && cached == mt {
            // song 目录未变：其直接子目录（difficulty 层）可能有变——继续检查。
        }
        // 2. difficulty 层：song 的直接子目录 mtime 对比。
        let Ok(sub) = fs::read_dir(&path) else { continue };
        for sub_entry in sub.flatten() {
            let Ok(sft) = sub_entry.file_type() else { continue };
            if !sft.is_dir() {
                continue;
            }
            let dpath = sub_entry.path();
            let dmt = stat_mt(&dpath);
            let dcached = dir_cache.get(&dpath).copied();
            dir_cache.insert(dpath.clone(), dmt.unwrap_or(SystemTime::UNIX_EPOCH));
            if dmt.is_some() && dcached == dmt {
                continue; // difficulty 目录未变
            }
            changed_leaves.push(dpath);
        }
    }

    // 3. 深入所有变化 difficulty 目录找 *_mma_request.json（含子目录递归）。
    let mut stack: Vec<PathBuf> = changed_leaves;
    while let Some(dir) = stack.pop() {
        let Ok(sub) = fs::read_dir(&dir) else { continue };
        for entry in sub.flatten() {
            let path = entry.path();
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() {
                stack.push(path);
                continue;
            }
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if !name.ends_with("_mma_request.json") {
                continue;
            }
            if let Ok(meta) = fs::metadata(&path) {
                if let Ok(mt) = meta.modified() {
                    out.push((path, mt));
                }
            }
        }
    }
    out
}

pub fn spawn_malody_poller(shared: std::sync::Arc<Shared>) {
    thread::spawn(move || {
        // 已处理的请求签名（path -> mtime），避免重复触发。
        let mut handled: HashMap<PathBuf, SystemTime> = HashMap::new();
        // 目录 mtime 缓存（分层快筛，避免每轮全量 stat 数万文件）。
        let mut dir_cache: HashMap<PathBuf, SystemTime> = HashMap::new();
        let mut seq = 0u64;

        loop {
            thread::sleep(POLL_INTERVAL);
            let Some(root) = crate::server::malody_root(&shared) else {
                continue;
            };
            for (req_path, mt) in scan_requests(&root, &mut dir_cache) {
                if handled.get(&req_path).copied() == Some(mt) {
                    continue; // 已处理
                }
                handled.insert(req_path.clone(), mt);
                let Ok(text) = fs::read_to_string(&req_path) else {
                    continue;
                };
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
                    log_at("debug", &format!("malody request malformed ({}): 非 JSON",
                        req_path.display()
                    ));
                    continue;
                };
                if v.get("action").and_then(|x| x.as_str()) != Some("analyze") {
                    continue; // 非本插件的请求
                }
                let title = v.get("title").and_then(|x| x.as_str()).unwrap_or("");
                let artist = v.get("artist").and_then(|x| x.as_str()).unwrap_or("");
                let level = v.get("level").and_then(|x| x.as_str()).unwrap_or("");
                let keys = v.get("keys").and_then(|x| x.as_u64()).unwrap_or(0);
                log_at("debug", &format!(
                    "malody request: {} (title={} artist={} level={} keys={})",
                    req_path.display(),
                    title,
                    artist,
                    level,
                    keys
                ));

                // 1. 谱面本体 = 同目录 `<base>.mc|.osu`（Malody 生成的 base 精确锁定，
                //    与命名规整度/格式无关）。
                let Some(chart_path) = chart_path_for(&req_path) else {
                    log_at("debug", &format!("malody chart not found beside request: {}（请确认谱面已保存为 .mc/.osu）",
                        req_path.display()
                    ));
                    let _ = fs::remove_file(&req_path);
                    continue;
                };
                let Ok(chart_text) = fs::read_to_string(&chart_path) else {
                    let _ = fs::remove_file(&req_path);
                    continue;
                };

                // 2. 发 song 帧（请求关联 requestId = r{seq}；页面分析后卡片展示——
                //    壳就是展示端，不回写 txt 到游戏内，见问题 6 拍板）。
                seq += 1;
                let request_id = format!("r{}", seq);
                let content_md5 = md5_hex(&chart_text);
                let identity = format!("mdy:{}:{}:{}:{}", title, level, keys, content_md5);
                let song = serde_json::json!({
                    "requestId": request_id,
                    "source": "malody",
                    "identity": identity,
                    "modData": { "speedRate": "1.0" },
                    "meta": { "title": title, "artist": artist, "version": level, "keys": keys, "devMsd8": [] },
                    "cover": null,
                    "rawText": chart_text,
                });
                broadcast(&shared, "song", Some(song));

                // 3. 等待页面 result 帧（仅确认分析完成/失败，供日志；不回写游戏内）。
                let (tx, rx) = mpsc::channel::<String>();
                shared.pending.lock().unwrap().insert(request_id.clone(), tx);
                let got_result = match rx.recv_timeout(Duration::from_secs(30)) {
                    Ok(result_text) => {
                        match crate::server::ws::parse_result_payload(&result_text) {
                            Some(inbound) => {
                                let errs = inbound.errors.unwrap_or_default();
                                if errs.is_empty() {
                                    log_at("debug", "malody analysis ok (shown on card)");
                                } else {
                                    log_line(&format!(
                                        "malody analysis failed: {}",
                                        errs.join("；").chars().take(300).collect::<String>()
                                    ));
                                }
                                true
                            }
                            None => false,
                        }
                    }
                    Err(_) => {
                        log_at("debug", "malody analysis timed out (no page yet?)");
                        false
                    }
                };

                // 4. 页面成功分析后才删 request。超时（页面未就绪/未连接——开壳即
                //    自动触发时 song 帧可能丢失，造成首次 NoData）→ 保留 request 并
                //    移出 handled，页面就绪后下轮重试。
                if got_result {
                    if fs::remove_file(&req_path).is_ok() {
                        log_at("debug", "malody request removed after processing");
                    } else {
                        log_line(&format!(
                            "malody request REMOVE FAILED: {}（残留可能造成重复分析；目录不可写时请以管理员运行壳）",
                            req_path.display()
                        ));
                    }
                } else {
                    handled.remove(&req_path);
                    log_at("debug", "malody request kept for retry (page not ready)");
                }
            }
        }
    });
}
