/// 查一个「类型 + 字段」的结构化查法（不写死类型名的完整拼写）。
#[derive(Clone, Copy, Debug)]
pub struct FieldLookup {
    /// 类型键必须全部包含的子串（all-of；全部按 canonical_type 规范化后比较）。
    pub type_all: &'static [&'static str],
    /// 类型键必须至少包含其一的子串（any-of；空 = 不约束）。
    pub type_any: &'static [&'static str],
    /// 字段名（逐字，不做大小写归一：错一个字母就该查不到）。
    pub field: &'static str,
}

impl FieldLookup {
    /// all-of 形态（最常用：一个类型子串 + 一个字段）。
    pub const fn new(type_all: &'static [&'static str], field: &'static str) -> FieldLookup {
        FieldLookup {
            type_all,
            type_any: &[],
            field,
        }
    }

    /// any-of 形态（派生类改名容忍；见 `type_any`）。
    pub const fn new_any(type_any: &'static [&'static str], field: &'static str) -> FieldLookup {
        FieldLookup {
            type_all: &[],
            type_any,
            field,
        }
    }

    /// 日志/降级标记用的短名（`类型子串#字段`）。
    pub fn label(&self) -> String {
        let mut parts: Vec<&str> = self.type_all.to_vec();
        parts.extend_from_slice(self.type_any);
        format!("{}#{}", parts.join("+"), self.field)
    }
}

/// 字段查找失败的原因（字面量，进 `degradedFields` 与日志）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LookupError {
    /// 没有任何类型键包含全部子串 ⇒ 该类型的字段本表没有。
    NoType(String),
    /// ≥2 个类型键命中 ⇒ 分不出实例化（拒绝，不猜）。
    Ambiguous(String, Vec<String>),
    /// 类型找到了但字段不在（改名/被裁掉）。
    NoField(String, String),
    /// 偏移不在合理域（对象头之后、且不超过 [`MAX_FIELD_OFFSET`]）。
    OutOfRange(String, i64),
}

impl std::fmt::Display for LookupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LookupError::NoType(label) => write!(f, "no-type:{label}"),
            LookupError::Ambiguous(label, keys) => {
                write!(f, "ambiguous:{label}({})", keys.join(","))
            }
            LookupError::NoField(key, field) => write!(f, "no-field:{key}.{field}"),
            LookupError::OutOfRange(label, offset) => {
                write!(f, "offset-out-of-range:{label}={offset}")
            }
        }
    }
}

/// 字段偏移的合理下界（x64 对象的前 8 字节是 MethodTable 指针 ⇒ 字段必在 `+0x08` 之后）。
pub const MIN_FIELD_OFFSET: i64 = 0x08;
/// 字段偏移的合理上界：真机最大字段（`OsuScreenStack` 的 `stack`）是 `0x320`；给 16 倍余量。
/// 超出即判"表坏了/查错了键"，不给假值。
pub const MAX_FIELD_OFFSET: i64 = 0x4000;

/// CLR 类型名规范化（读取侧的唯一规则，与生成器 tools/lazer-offsets-gen/names.rs 逐字同源）。
pub fn canonical_type(display: &str) -> String {
    let trimmed = display.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.contains("...") {
        return trimmed.split_whitespace().collect::<Vec<_>>().join("");
    }
    let mut parser = TypeParser {
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

struct TypeParser<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl<'a> TypeParser<'a> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ') | Some(b'\t')) {
            self.index += 1;
        }
    }

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
        if self.peek() == Some(b'[') && self.bytes.get(self.index + 1) == Some(&b']') {
            while self.peek() == Some(b'[') && self.bytes.get(self.index + 1) == Some(&b']') {
                name.push_str("[]");
                self.index += 2;
            }
            return name;
        }
        if self.peek() == Some(b'[') && self.bytes.get(self.index + 1) == Some(&b'[') {
            self.index += 2;
            let mut args: Vec<String> = Vec::new();
            loop {
                self.skip_ws();
                if self.peek() == Some(b'[') {
                    self.index += 1;
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
