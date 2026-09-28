// stable 锚点台账（**唯一权威**）。
//
// ⚠️ 与参照实现的关系（DEC-02）：本表**不是**从 tosu 抄来的数值表。每一条的
// 签名/位移都是"待验证假设"经**本机真机**复现后的落档，`derivation` 写的是
// 为什么这串字节能定位该全局量（可独立复核），`evidence` 写的是**验证方法**
// （而不是某个 `temp/` 文件的路径）——`temp/` 交付后被删除，台账仍必须自解释。
//
// 本步只用**三枚**锚点（计划 §4 Step 5：两枚起步 + 菜单 mod 位掩码）：
// - `statusPtr`   → `GameState` 整数（→ `state.name`）
// - `baseAddr`    → Beatmap 对象（→ `beatmap.md5` 与身份字段）
// - `menuModsPtr` → 菜单 mod 位掩码（本步仅用于菜单态对照）
//
// 其余 13 枚锚点属 C2/D 步，不预先入库（`CLAUDE.md` §2：不做投机抽象）。

/// 一条锚点的台账记录。`offset` 是**匹配地址之上的有符号位移**（含负位移），
/// 由 `scan.rs` 在解析时施加**一次**；施加后得到的地址即"链的起点"。
#[derive(Clone, Copy, Debug)]
pub struct Anchor {
    /// 稳定键名（reason `signature-miss:<key>` 与日志都用它）。
    pub key: &'static str,
    /// IDA 风格签名（空格分隔十六进制字节，`??` = 单字节通配）。
    pub pattern: &'static str,
    /// 施加在匹配地址上的有符号位移。
    pub offset: i32,
    /// 为什么这串字节能定位该全局量（可独立复核的推导说明）。
    pub derivation: &'static str,
    /// 真机验证时的目标构建（`osu!.exe` MD5）。
    pub verified_build: &'static str,
    /// 真机验证时间。
    pub verified_at: &'static str,
    /// **验证方法**（不是文件路径）：怎么证明这条签名是它说的那个量。
    pub evidence: &'static str,
}

/// 被验证过的构建：`osu!.exe`（stable）MD5。
///
/// 事实（真机实测，非文档引用）：MD5 `f845ef10bf97c3260b818fc02e73e196`、
/// 文件大小 4541216 B、FileVersion `1.3.3.8`、PE machine `0x014C`（i386/32 位）、
/// 磁盘映像 mtime `2026-09-26T04:20:47Z`。锚点地址**每次进程启动都会变**（ASLR），
/// 所以本表只固化"签名 + 位移"，地址一律现场扫描得到。
pub const VERIFIED_BUILD_STABLE_MD5: &str = "f845ef10bf97c3260b818fc02e73e196";

/// 验证时间（本机时区）。
pub const VERIFIED_AT: &str = "2026-09-27T21:53+08:00";

/// 状态指针锚点：`status = read_pointer(statusPtr)`，即 `GameState` 整数。
///
/// 推导：`48 83 F8 04` 是 x86-64 的 `cmp rax, 4`（`REX.W + 83 /7 ib`），紧随的
/// `73 1E` 是 `jae rel8(+0x1E)`——"落在 `0..=4` 才继续、否则跳到别处"这种两分支形状，
/// 只出现在"用状态整数查表/分派"的代码里；`-0x4` 落回**该比较之前**，那里是被比较值
/// 的装载点，即 `GameState` 全局的间接地址所在位置（P1 实测：`-0x4` 处的 4 字节
/// `2C 69 93 03` 就是 statusPtr 自身，其解引用取值 2 = `play`）。
pub const STATUS_PTR: Anchor = Anchor {
    key: "statusPtr",
    pattern: "48 83 F8 04 73 1E",
    offset: -0x4,
    derivation: "`cmp rax,4` + `jae` 的两分支形状 → 状态整数分派点；负位移 -0x4 落回被比较值的装载点，即 GameState 全局的间接地址",
    verified_build: VERIFIED_BUILD_STABLE_MD5,
    verified_at: VERIFIED_AT,
    evidence: "真机：在 stable 进程上按 filter A（MEM_COMMIT & RW/RWX）与 filter B（任一可读已提交区）各扫一遍，命中数 1；resolved 地址解引用链路 read_pointer(statusPtr) 的取值与同时刻 tosu /json/v2 的 state.number 逐位相等（play=2 与 selectPlay=5 两次独立采样）",
};

/// Beatmap 对象锚点：`beatmap = read_pointer(baseAddr - 0xC)`。
///
/// 推导：`F8 01 74 04 83 65` 里的 `F8 01` 是某条指令的尾部立即数（`F8 01` = 504），
/// 其后 `74 04` = `je +4`、`83 65 ??` = `and dword [ebp+disp8], imm8`——这是 osu! 主循环
/// 里"按标志位决定是否清空当前谱面"的形状；该 `and` 之后的 `8B CE 8B 55`（`mov ecx,esi`
/// / `mov edx,[ebp-…]`）紧接着就是"装载 Beatmap 对象"的调用序列，`baseAddr` 本身即
/// 谱面数据的间接地址，`-0xC` 偏移落到上一字段（对象指针槽）上。
pub const BASE_ADDR: Anchor = Anchor {
    key: "baseAddr",
    pattern: "F8 01 74 04 83 65",
    offset: 0,
    derivation: "主循环里 `je` + `and dword [ebp+disp8], imm8` 的谱面清理分支；其后紧跟 `mov ecx,esi / mov edx,[ebp-…]` 的 Beatmap 装载序列，baseAddr 即谱面数据间接地址",
    verified_build: VERIFIED_BUILD_STABLE_MD5,
    verified_at: VERIFIED_AT,
    evidence: "真机：两过滤器下各 1 次命中；`read_pointer(baseAddr-0xC)` 得到的对象 +0x6C 处的 C# 字符串等于同时刻 tosu 的 beatmap.checksum，且该 32 位十六进制串在磁盘上等于 `Songs\\<folder>\\<filename>` 的 .osu 文件 MD5（两张不同谱面各一次）",
};

/// 菜单 mod 位掩码锚点：`mods_mask = read_pointer(menuModsPtr)`。
///
/// 推导：`C8 FF ?? ?? ?? ?? ??` 是 `enter`/`int 3` 之后的填充形状（`C8 FF` 起的一串
/// 立即数），紧随 `81 0D ?? ?? ?? ?? ?? 08 00 00` = `or dword [addr], 0x800`——对某个
/// **全局 dword** 做 `or imm32`（`0x800` 是 mania 的键位/模式标志位），该 `or` 的目标
/// 就是菜单态 mod 位掩码所在的全局槽；`+0x9` 落到 `81 0D` 之后的 4 字节地址立即数。
pub const MENU_MODS_PTR: Anchor = Anchor {
    key: "menuModsPtr",
    pattern: "C8 FF ?? ?? ?? ?? ?? 81 0D ?? ?? ?? ?? ?? 08 00 00",
    offset: 0x9,
    derivation: "`81 0D <addr> <imm32>` = `or dword [addr], 0x800` 的形状 → 该全局 dword 即菜单态 mod 位掩码；+0x9 落在 `81 0D` 之后的地址立即数上",
    verified_build: VERIFIED_BUILD_STABLE_MD5,
    verified_at: VERIFIED_AT,
    evidence: "真机：两过滤器下各 1 次命中；`read_pointer(menuModsPtr)` 在 play 态读出 0（= NM），与同时刻 tosu `menu.mods` 为 null 的语义一致。**menu 态选 mod 后非零**的一侧仍未观测（P1-notes F8），故本步只把该掩码用于菜单/选歌态对照，并标注这一未关闭项",
};

/// 本步扫描的锚点（顺序即扫描顺序；每枚只取首个命中）。
///
/// `settingsClassAddr` 是**第四枚**（B3 新增，服务 `folders.songs`）：它**不是**必得项
/// （见 `BEST_EFFORT_KEYS`），命中不了只是该字段降级，不影响发布。
pub const ANCHORS: &[Anchor] = &[STATUS_PTR, BASE_ADDR, MENU_MODS_PTR, SETTINGS_CLASS_ADDR];

/// `settingsClassAddr`：osu! 的 `ConfigManager`/设置对象锚点。
///
/// 推导（**B3 在真机上按字节复核后定案**）：`83 E0 20` 是 `and eax, 0x20`——对一个刚
/// 装载的全局 dword 做单比特掩码（`0x20` 是 osu! 的"界面可见"位），紧随 `85 C0`
/// （`test eax, eax`）+ `7E 2F`（`jle +0x2F`）就是"该位为真才走某条分支"的形状。
/// 真机运行期字节流（B3 实测，`match = 0x0A2313B1`，窗口从 `match - 8` 起）：
/// ```text
/// E0 20 | 85 C0 | 7E 2F | A1 78 3B 9C 04 | 85 C0 | 74 06 | 0F B6 40 0C | EB 02 | 33 C0 | 85 C0 | 74 13 | C6 05 1D 67 87 …
/// ```
/// 即：`and eax,0x20` → `test eax,eax` → `jle` 跳过整个分支体；分支体内**第一条**就是
/// `A1 <addr>`（`mov eax,[0x049C3B78]`），紧接着又是 `test eax,eax` / `je +6` /
/// `movzx eax, byte [eax+0xC]` —— 这正是"设置对象 +0xC 是 showInterface 字节"的形状
/// （P1 语义表里那条自证，也是本锚点的结构自证判据）。`A1` 的操作码在 **match + 7**、
/// 其 4 字节小端地址立即数在 **match + 8**（对齐复核：`7E 2F` 是 2 字节、`A1` 是 1 字节）。
///
/// ⚠️ 台账演进（三次修正都留档，避免下次再走回头路）：
/// - **P1 探针台账 `offset = 0`**（`match_addr == final_addr`）：探针是"取原始命中地址直接
///   当槽"的近似，当时靠后续语义自证通过；B3 用结构自证复现时**不成立**
///   （`0x0A2313A3` 解引用不出对象）。
/// - **v1 `offset = -0x5`**（"`A1` 紧邻匹配点之前"的假设）：被真机字节流直接证伪
///   （匹配点前是 `8B 86 5C 01 00 00` = `mov eax,[esi+0x15C]`，没有 `A1`）。
/// - **v2 `offset = +0x5`**（漏算 `7E 2F` 的 2 字节）：真机实测该位移落进 `A1` 的地址
///   立即数里、读出来是**错位**的字节流（`read-error`），被拒绝判据当场拦下。
/// - **定案 `offset = +0x8`**：`match + 8` 处 4 字节 = `78 3B 9C 04` ⇒ 槽地址
///   `0x049C3B78`（该槽地址落在 filter A 区域内），其解引用即设置对象
///   （+0x0 为 MethodTable/vtable、+0xC 为 showInterface 字节）。
///
/// ⚠️ **未关闭项（B3 如实记录）**：把 `settingsClassAddr` 当"指针槽"解引用后，两种链
/// 解释都读不出 `Songs`（`[对象+0x8]` 得到的值指向映像内，`+0xB8` 一跳随即 `read-error`；
/// 另一次实测该链第三跳得到 `0x00000002`）。**锚点命中与候选自证成立，语义链未成立** ⇒
/// 本字段在 B3 里走 `degraded` 路径（`folders.songs` 不出现在载荷里，对拍记
/// `skipped:songs-chain:unavailable`），**不阻塞**；该链的语义须在 Step 10（lazer 偏移
/// 生成器一并处理"结构证明"）里用 IL 差分重做，不得凭当前假设发布该字段。
///
/// 用途（B3）：`[[[settingsClassAddr+0x8]+0xB8]+0x4]` 是 osu! cfg 的 **`BeatmapDirectory`
/// 值**（本机观测 = `"Songs"`，**是名字不是路径**）；相邻槽位是 `"BeatmapDirectory"` 这个
/// 键名本身，故该链同时自证"读对了槽"。绝对目录由调用方按参照实现的规则合成：
/// `join(dirname(exe), 值)` 存在则用它，否则用值本身（P1-notes F2 / P2 menu-chain 实测
/// `D:\Games\osu!` + `Songs` == tosu 的 `folders.songs`）。
pub const SETTINGS_CLASS_ADDR: Anchor = Anchor {
    key: "settingsClassAddr",
    pattern: "83 E0 20 85 C0 7E 2F",
    offset: 0x8,
    derivation: "`and eax, 0x20` + `test eax, eax` + `jle` 的「单比特门控分支」形状；分支体首条 `A1 <addr>`（`mov eax, [addr]`）的操作码在 match+7、4 字节地址立即数在 match+8 ⇒ +0x8 即设置类全局对象的指针槽（真机字节流 `… 7E 2F A1 78 3B 9C 04 85 C0 74 06 0F B6 40 0C …` 直接给出）",
    verified_build: VERIFIED_BUILD_STABLE_MD5,
    verified_at: VERIFIED_AT,
    evidence: "真机（P1 主轮次 + B3 复现，不同进程实例）：filter A（MEM_COMMIT & RW/RWX）命中 **1** 次；+0x8 处 4 字节 = `78 3B 9C 04` ⇒ 槽 = 0x049C3B78（槽地址本身在 filter A 区域内）。候选自证：`read_pointer(槽)` 得到设置对象（4 字节对齐、≥64 KiB、落在可读区）、其 +0x0 的 MethodTable/vtable 同样是可读区里的对齐非空指针、且 +0xC 的字节 ∈ {0,1}（配置布尔）——三条同时成立才采纳。语义自证两条：① 设置对象 +0xC 的字节在 play 态 = 1，与同时刻 tosu `settings.interfaceVisible` = true 一致（真机字节流里 `0F B6 40 0C` = `movzx eax, byte [eax+0xC]` 正是读该字节）；② `[[设置对象+0xB8]+0x4]` 的 C# 字符串 = `Songs`（32 位字符串约定 len@+0x04/chars@+0x08，另一约定读出不可信值），相邻槽位 = `BeatmapDirectory`（键名），且 dirname(exe) + `\\` + 值 与同时刻 tosu `folders.songs` 逐字节相等（P2 的 menu-chain 对拍行）",
};

/// 解析结果：`key` → 已施加 `offset` 的"链起点"地址（必须 4 字节对齐才可能是指针槽）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AnchorTable {
    pub status_ptr: Option<u32>,
    pub base_addr: Option<u32>,
    pub menu_mods_ptr: Option<u32>,
    /// B3 新增：`folders.songs` 链的起点（**best-effort**，缺失不算失败）。
    pub settings_class_addr: Option<u32>,
}

/// **best-effort** 锚点：缺席只降级对应字段，不进 `missing()`、不判 `unhealthy`。
pub const BEST_EFFORT_KEYS: &[&str] = &["settingsClassAddr"];

impl AnchorTable {
    /// 必需锚点齐了才能发布（少一枚 = 该字段无来源 ⇒ reason `signature-miss:<key>`）。
    /// `settingsClassAddr` 不在其中（见 `BEST_EFFORT_KEYS`）。
    pub fn missing(&self) -> Option<&'static str> {
        if self.status_ptr.is_none() {
            return Some(STATUS_PTR.key);
        }
        if self.base_addr.is_none() {
            return Some(BASE_ADDR.key);
        }
        if self.menu_mods_ptr.is_none() {
            return Some(MENU_MODS_PTR.key);
        }
        None
    }

    pub fn get(&self, key: &str) -> Option<u32> {
        match key {
            "statusPtr" => self.status_ptr,
            "baseAddr" => self.base_addr,
            "menuModsPtr" => self.menu_mods_ptr,
            "settingsClassAddr" => self.settings_class_addr,
            _ => None,
        }
    }

    pub fn set(&mut self, key: &str, addr: u32) {
        match key {
            "statusPtr" => self.status_ptr = Some(addr),
            "baseAddr" => self.base_addr = Some(addr),
            "menuModsPtr" => self.menu_mods_ptr = Some(addr),
            "settingsClassAddr" => self.settings_class_addr = Some(addr),
            _ => {}
        }
    }
}

/// 对齐检查：指针槽必然 4 字节对齐。不对齐 = 命中是假阳性（L1 结构校验的第一道）。
pub fn is_aligned(addr: u32) -> bool {
    addr % 4 == 0
}

#[cfg(test)]
#[path = "../../tests-local/osu_patterns.rs"]
mod tests_patterns;
