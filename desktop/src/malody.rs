// Malody 自动分析轮询（壳侧）——三通道合一：
//
// 1) replay 触发（游戏内游玩）：每局游玩结束 Malody 写
//    replay/{YYYYMMDD_HHMM}_{chart内容MD5}.mrv；轮询新 hash → 按 MD5 从
//    chart/** 索引定位谱面 → song 帧 → 前端分析（本机已验证 53/53 命中）。
// 2) chart mtime（编辑器保存）：.{fe}: 最新修改 .mc 变化即触发。
// 3) 启动即捕获当前最新谱面分析一次。
//
// 边界（如实标注）：游玩中 Malody 不写谱面文件 → 结果在游玩结束（回放落盘）
// 后展示；游玩中实时不可能（无游戏内写通道）。皮肤/编辑器插件为可选通道。

use crate::server::{broadcast, log_line, Shared};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, SystemTime};

const POLL_INTERVAL: Duration = Duration::from_secs(2);

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

fn chart_meta(text: &str) -> (String, u32) {
    let value = serde_json::from_str::<serde_json::Value>(text).unwrap_or(serde_json::Value::Null);
    let title = value
        .pointer("/meta/song/title")
        .and_then(|x| x.as_str())
        .unwrap_or("untitled")
        .to_string();
    let keys = value
        .pointer("/meta/mode_ext/column")
        .and_then(|x| x.as_u64())
        .unwrap_or(4) as u32;
    (title, keys)
}

pub fn spawn_malody_poller(shared: std::sync::Arc<Shared>) {
    log_line("malody poller started");
    thread::spawn(move || {
        // chart 内容 md5 索引（path -> hash）与 mtime 缓存（增量维护）
        let mut index: HashMap<PathBuf, String> = HashMap::new();
        let mut index_mtime: HashMap<PathBuf, SystemTime> = HashMap::new();
        let mut last_chart: Option<(PathBuf, SystemTime)> = None;
        let mut seq = 0u64;
        let mut tick = 0u64;

        loop {
            thread::sleep(POLL_INTERVAL);
            tick += 1;
            let Some(root) = crate::server::malody_root(&shared) else {
                continue;
            };

            // ── 索引维护（降频：每 15 轮 ≈30s 全量增量）──
            if tick % 15 == 0 {
                let mut walk = vec![root.join("chart")];
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
                        if let Ok(meta) = fs::metadata(&path) {
                            if let Ok(mt) = meta.modified() {
                                if index_mtime.get(&path).copied() != Some(mt) {
                                    if let Ok(text) = fs::read_to_string(&path) {
                                        index.insert(path.clone(), md5_hex(&text));
                                        index_mtime.insert(path.clone(), mt);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // ── chart mtime（编辑器保存 / 启动捕获）──
            // replay 通道已按用户否决移除（游玩结束回放触发不满足「游玩前得知」）。
            let mut best: Option<(PathBuf, SystemTime)> = None;
            let mut walk2 = vec![root.join("chart")];
            while let Some(dir) = walk2.pop() {
                let Ok(entries) = fs::read_dir(&dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        walk2.push(path);
                        continue;
                    }
                    if path.extension().and_then(|e| e.to_str()) != Some("mc") {
                        continue;
                    }
                    if let Ok(meta) = fs::metadata(&path) {
                        if let Ok(mt) = meta.modified() {
                            if best.as_ref().map(|(_, b)| mt > *b).unwrap_or(true) {
                                best = Some((path, mt));
                            }
                        }
                    }
                }
            }
            if let Some((path, mt)) = best {
                let changed = last_chart
                    .as_ref()
                    .map(|(lp, lm)| lp != &path || lm != &mt)
                    .unwrap_or(true);
                last_chart = Some((path.clone(), mt));
                if changed {
                    if let Ok(text) = fs::read_to_string(&path) {
                        let (title, keys) = chart_meta(&text);
                        seq += 1;
                        let content_md5 = md5_hex(&text);
                        let identity = format!("mdy:chart:{}:{}:{}", title, keys, content_md5);
                        log_line(&format!(
                            "malody chart -> song frame: {} ({} bytes)",
                            path.display(),
                            text.len()
                        ));
                        let song = serde_json::json!({
                            "requestId": format!("c{}", seq),
                            "source": "malody",
                            "identity": identity,
                            "modData": { "speedRate": "1.0" },
                            "meta": { "title": title, "keys": keys },
                            "cover": null,
                            "rawText": text,
                        });
                        broadcast(&shared, "song", Some(song));
                    }
                }
            }
        }
    });
}