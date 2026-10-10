// common::throttle - 统一去重与日志节流门控组件

use std::time::{Duration, Instant};

/// 基于值的变化去重门：只有当新值与上次不同时才判定为有效（due）。
/// 常用于心跳高频刷新下，仅在结论或状态变更时输出日志或广播。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangeDedupe<T: PartialEq + Clone> {
    last: Option<T>,
}

impl<T: PartialEq + Clone> ChangeDedupe<T> {
    pub fn new() -> Self {
        Self { last: None }
    }

    /// 检查新值是否发生变更。若变更，则记录并返回 true；否则返回 false。
    pub fn check_changed(&mut self, current: &T) -> bool {
        if self.last.as_ref() == Some(current) {
            return false;
        }
        self.last = Some(current.clone());
        true
    }

    /// 获取上一次记录的值。
    pub fn last(&self) -> Option<&T> {
        self.last.as_ref()
    }

    /// 重置状态。
    pub fn reset(&mut self) {
        self.last = None;
    }
}

/// 基于字符串的日志节流闸门（兼容 LogGate 语义）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogGate {
    last: Option<String>,
}

impl LogGate {
    pub fn new() -> Self {
        Self { last: None }
    }

    /// 当输入字符串与上一次不同时返回 true，否则返回 false。
    pub fn due(&mut self, line: &str) -> bool {
        if self.last.as_deref() == Some(line) {
            return false;
        }
        self.last = Some(line.to_string());
        true
    }

    pub fn last_line(&self) -> Option<&str> {
        self.last.as_deref()
    }
}

/// 带有时间窗口节流的告警闸门：同一原因在窗口期内不重复上报。
#[derive(Debug, Clone)]
pub struct TimedThrottle {
    last_tag: String,
    last_logged: Instant,
    window: Duration,
}

impl TimedThrottle {
    pub fn new(window: Duration) -> Self {
        Self {
            last_tag: String::new(),
            // 初始化为一个很早的瞬间，确保首次调用能立即触发
            last_logged: Instant::now() - window,
            window,
        }
    }

    /// 判定是否应当放行。
    pub fn due(&mut self, tag: &str) -> bool {
        let now = Instant::now();
        if tag == self.last_tag && now.duration_since(self.last_logged) < self.window {
            return false;
        }
        self.last_tag = tag.to_string();
        self.last_logged = now;
        true
    }
}
