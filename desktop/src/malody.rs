// Malody 轮询（壳侧）——已按用户拍板收敛为最小职责：
//
// 1) 编辑器按钮触发（MMA Analyze 插件）：POST 24060 → 壳生成 song 帧 → 前端分析。
//    壳不做任何「自动捕获最新谱面」——mtime 轮询/启动捕获均会误触发
//    （用户：一打开 shell 就触发，应当在游戏内点击按钮才分析）。
// 2) 本模块仅维护 chart 索引（供 resolve 通道按标题找 .mc 原文），
//    索引维护 30s 增量一次，不产生 song 帧、不触发分析。

use crate::server::Shared;
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

pub fn spawn_malody_poller(shared: std::sync::Arc<Shared>) {
    thread::spawn(move || {
        // chart 内容 md5 索引（path -> hash）与 mtime 缓存（增量维护）
        let mut index: HashMap<PathBuf, String> = HashMap::new();
        let mut index_mtime: HashMap<PathBuf, SystemTime> = HashMap::new();
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
        }
    });
}