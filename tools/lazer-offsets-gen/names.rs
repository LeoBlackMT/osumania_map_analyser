// names.rs —— CLR 类型名的规范化（生成器内部唯一的一份规则）
//
// SOS 打印的类型名是 CLR 的 "assembly-qualified 显示名"，例如：
//   osu.Framework.Bindables.NonNullableBindable`1[[osu.Game.Beatmaps.WorkingBeatmap, osu.Game]]
//   ..., osu.Framework]]                     ← 列宽不足时的**中段截断**形态
//   osu.Game.Beatmaps.WorkingBeatmapCache+BeatmapManagerWorkingBeatmap   ← 嵌套类型用 `+`
//
// 表里的类型键必须是**可复现**的字符串，所以这里定义唯一一份规范化规则：
// - 泛型实例化：`Name`1[[Arg, Asm],[Arg2, Asm2]]` → `` Name`1<Arg,Arg2> ``（去掉程序集限定与空白）
// - 数组后缀原样保留：`X[]`
// - 嵌套类型保留 `+`
//
// IL 侧的对照键用的是**开放泛型定义**（元数据里就只有那个名字）：把规范化结果里的
// 第一个 `<` 之后全部去掉即可（[`open_generic`]）。这条规则写在 README 的判据表里。

/// 规范化一个 SOS/CLR 显示名。
///
/// 被 SOS 列宽截断的形态（含 `...`）**不做结构解析**：去掉空白后原样返回
/// （截断串里只剩尾巴，解析出来的是残片；类型比较走"按引用/值类型分类"那条规则）。
pub fn canonical_type(display: &str) -> String {
    let trimmed = display.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if is_truncated(trimmed) {
        return trimmed.split_whitespace().collect::<Vec<_>>().join("");
    }
    let mut parser = Parser {
        bytes: trimmed.as_bytes(),
        index: 0,
    };
    let mut out = parser.parse_type();
    parser.skip_ws();
    out = out.trim().to_string();
    if out.is_empty() {
        out = trimmed.split_whitespace().collect::<Vec<_>>().join("");
    }
    out
}

/// 开放泛型定义键：`Ns.Bindable`1<A>` → `Ns.Bindable`1`。
pub fn open_generic(canonical: &str) -> &str {
    match canonical.find('<') {
        Some(index) => &canonical[..index],
        None => canonical,
    }
}

/// 显示名是否是被 SOS 列宽截断的（含 `...`）。
pub fn is_truncated(display: &str) -> bool {
    display.contains("...")
}

struct Parser<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ') | Some(b'\t')) {
            self.index += 1;
        }
    }

    /// 解析一个类型（可能带泛型参数与数组后缀）。
    fn parse_type(&mut self) -> String {
        self.skip_ws();
        let start = self.index;
        while let Some(c) = self.peek() {
            if c == b'[' || c == b']' || c == b',' {
                break;
            }
            self.index += 1;
        }
        let mut name: String = String::from_utf8_lossy(&self.bytes[start..self.index])
            .trim()
            .to_string();
        // 数组后缀：`[]`（可能多重）。
        if self.peek() == Some(b'[') && self.bytes.get(self.index + 1) == Some(&b']') {
            while self.peek() == Some(b'[') && self.bytes.get(self.index + 1) == Some(&b']') {
                name.push_str("[]");
                self.index += 2;
            }
            return name;
        }
        // 泛型实例化：`[[` 开始。
        if self.peek() == Some(b'[') && self.bytes.get(self.index + 1) == Some(&b'[') {
            self.index += 2;
            let mut args: Vec<String> = Vec::new();
            loop {
                self.skip_ws();
                if self.peek() == Some(b'[') {
                    self.index += 1; // 参数自身的 `[`
                }
                let arg = self.parse_type();
                args.push(arg);
                self.skip_to_close();
                self.skip_ws();
                if self.peek() == Some(b',') {
                    self.index += 1;
                    continue;
                }
                break;
            }
            while self.peek() == Some(b']') {
                self.index += 1;
            }
            // 泛型实例化也可以带数组后缀：`List`1[[...]][]`。
            if !args.is_empty() {
                name = format!("{name}<{}>", args.join(","));
            }
            while self.peek() == Some(b'[') && self.bytes.get(self.index + 1) == Some(&b']') {
                name.push_str("[]");
                self.index += 2;
            }
            return name;
        }
        name
    }

    /// 跳过当前参数的剩余部分（程序集限定）直到本级的 `]`，并吃掉它。
    fn skip_to_close(&mut self) {
        let mut depth = 0usize;
        while let Some(c) = self.peek() {
            match c {
                b'[' => {
                    depth += 1;
                    self.index += 1;
                }
                b']' => {
                    if depth == 0 {
                        self.index += 1;
                        return;
                    }
                    depth -= 1;
                    self.index += 1;
                }
                _ => self.index += 1,
            }
        }
    }
}