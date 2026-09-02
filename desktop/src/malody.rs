// Malody 请求轮询（壳侧）——编辑器按钮触发的文件通道：
//
// 1) 编辑器插件 WriteFile 写请求——Malody 自动加谱面名前缀，实际文件为
//    {谱面目录}/<谱面base名>_mma_request.json（实测确认）。
// 2) 本模块扫描 {malodyRoot}/chart/**/*_mma_request.json（后缀匹配）→ 按标题
//    resolve .mc → 发 song 帧 → 页面分析 → result 帧 → 写回
//    <谱面base名>_mma_result.txt（编辑器 ReadFile('mma_result.txt') 同前缀读取）。
// 3) 壳写 chart 目录可能被拒（Steam ACL）→ 日志提示以管理员运行壳。
//
// 壳不做任何「自动捕获最新谱面」——只有编辑器按钮触发才分析。

use crate::server::{broadcast, Shared};
use crate::server::log::log_line;
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

/// Malody WriteFile 生成的请求文件名为 `<谱面base名>_mma_request.json`；
/// 结果文件应对应 `<谱面base名>_mma_result.txt`（编辑器 ReadFile('mma_result.txt')
/// 同样会被 Malody 加前缀）。
fn result_path_for(req: &std::path::Path) -> PathBuf {
    let name = req.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let stem = name.trim_end_matches("_mma_request.json");
    req.with_file_name(format!("{}_mma_result.txt", stem))
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

/// 按 title/artist/keys 在 chart 目录递归匹配 .mc（与 post.rs resolve 同语义，
/// 简化版：精确标题优先，子串兜底）。
fn resolve_mc(root: &std::path::Path, title: &str, artist: &str, keys: u64) -> Option<PathBuf> {
    let title_norm = title.trim().to_lowercase();
    let artist_norm = artist.trim().to_lowercase();
    let mut walk = vec![root.join("chart")];
    let mut exact: Option<PathBuf> = None;
    let mut sub: Option<PathBuf> = None;
    while let Some(dir) = walk.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("mc") {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            let t = v
                .pointer("/meta/song/title")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_lowercase();
            let a = v
                .pointer("/meta/song/artist")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_lowercase();
            let fname = path.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
            let is_exact_title = !title_norm.is_empty() && t == title_norm;
            let is_exact = is_exact_title && (artist_norm.is_empty() || a == artist_norm);
            let keys_ok = keys == 0
                || v.pointer("/meta/mode_ext/column")
                    .and_then(|x| x.as_u64())
                    .map(|c| c == keys)
                    .unwrap_or(true);
            if is_exact && keys_ok && exact.is_none() {
                exact = Some(path.clone());
            }
            if exact.is_none()
                && !title_norm.is_empty()
                && fname.contains(&title_norm)
                && keys_ok
                && sub.is_none()
            {
                sub = Some(path.clone());
            }
            if exact.is_none()
                && !title_norm.is_empty()
                && !t.is_empty()
                && (t.contains(&title_norm) || title_norm.contains(&t))
                && keys_ok
                && sub.is_none()
            {
                sub = Some(path.clone());
            }
        }
    }
    exact.or(sub)
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
                    log_line(&format!(
                        "malody request malformed ({}): 非 JSON",
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
                log_line(&format!(
                    "malody request: {} (title={} artist={} level={} keys={})",
                    req_path.display(),
                    title,
                    artist,
                    level,
                    keys
                ));

                // 1. resolve .mc 原文。
                let Some(mc_path) = resolve_mc(&root, title, artist, keys) else {
                    let target = result_path_for(&req_path);
                    if fs::write(&target, format!(
                        "MMA: 壳未在 chart 目录找到该谱面（title={} artist={}）\n",
                        title, artist
                    )).is_ok() {
                        log_line(&format!("malody result written to {}", target.display()));
                    } else {
                        log_line(&format!("malody result WRITE FAILED: {}（请以管理员运行壳）", target.display()));
                    }
                    continue;
                };
                let Ok(mc_text) = fs::read_to_string(&mc_path) else {
                    continue;
                };

                // 2. 发 song 帧（请求关联 requestId = r{seq}）。
                seq += 1;
                let request_id = format!("r{}", seq);
                let content_md5 = md5_hex(&mc_text);
                let identity = format!("mdy:{}:{}:{}:{}", title, level, keys, content_md5);
                let song = serde_json::json!({
                    "requestId": request_id,
                    "source": "malody",
                    "identity": identity,
                    "modData": { "speedRate": "1.0" },
                    "meta": { "title": title, "artist": artist, "version": level, "keys": keys, "devMsd8": [] },
                    "cover": null,
                    "rawText": mc_text,
                });
                broadcast(&shared, "song", Some(song));

                // 3. 等待页面 result 帧（30s 超时）。
                let (tx, rx) = mpsc::channel::<String>();
                shared.pending.lock().unwrap().insert(request_id.clone(), tx);
                match rx.recv_timeout(Duration::from_secs(30)) {
                    Ok(result_text) => {
                        if let Some(inbound) = crate::server::ws::parse_result_payload(&result_text) {
                            let errs = inbound.errors.unwrap_or_default();
                            let content = if errs.is_empty() {
                                let star_v = serde_json::from_str::<serde_json::Value>(&result_text)
                                    .ok()
                                    .and_then(|j| {
                                        j.get("payload")
                                            .cloned()
                                            .unwrap_or(j)
                                            .get("star")
                                            .cloned()
                                    });
                                let star_s = star_v
                                    .map(|s| s.to_string())
                                    .unwrap_or_else(|| "-".to_string());
                                let active = inbound
                                    .active_source
                                    .as_deref()
                                    .unwrap_or("");
                                format!(
                                    "MMA: 分析完成\nstar={}\npattern={}\nactiveSource={}",
                                    star_s,
                                    inbound.status_hint.as_deref().unwrap_or(""),
                                    active
                                )
                            } else {
                                format!("MMA: 分析失败：{}", errs.join("；"))
                            };
                            let target = result_path_for(&req_path);
                            if fs::write(&target, content).is_ok() {
                                log_line(&format!("malody result written to {}", target.display()));
                            } else {
                                // 权限不足（Steam 目录 ACL 常拒普通用户写）：提示管理员运行。
                                log_line(&format!(
                                    "malody result WRITE FAILED: {}（目录不可写——请以管理员运行壳）",
                                    target.display()
                                ));
                            }
                        }
                    }
                    Err(_) => {
                        let target = result_path_for(&req_path);
                        let _ = fs::write(&target, "MMA: 分析超时（30s）\n");
                    }
                }
            }
        }
    });
}
