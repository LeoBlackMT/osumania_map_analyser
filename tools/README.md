# tools/ — 开发工具集 / Development Tools

> 本目录存放仓库的辅助开发脚本，不参与插件运行时（插件运行不依赖 Node 等任何外部运行时）。
> This folder holds helper scripts for repository maintenance. They are NOT part of the plugin runtime (the plugin itself never requires Node or any external runtime).

## patch-minaclac-msd-cap.mjs（MSD 上限突破补丁）

### 这个脚本是做什么的（What it does）

MinaCalc（Etterna 的官方难度计算器）内部把技能值上限（SSR cap）钳制在 **40.0**，导致超高难谱面（高速叠键 / 高密度）的 MSD 结果被压平在 40 左右。本脚本把该上限从 40.0 提升到 **100.0**，让 MSD 可以突破 42 并继续显示真实难度。

MinaCalc (Etterna's official difficulty calculator) clamps its internal skill cap (SSR cap) at **40.0**, flattening MSD results for ultra-hard charts (fast jacks / extreme density). This script raises the cap from 40.0 to **100.0** so MSD can break past 42 and show true difficulty.

### 原理（How it works）

- WASM 字节码中 `f32.const 40.0` 编码为 `43 00 00 20 42`（IEEE754 单精度小端）。
- `f32.const 100.0` 编码为 `43 00 00 c8 42`。
- 脚本扫描 `ManiaMapAnalyser by Leo_Black/js/ett/versions/` 下的所有 `.wasm`，把每一处 `43 00 00 20 42` 等长替换为 `43 00 00 c8 42`。
- 这是 5 字节 → 5 字节的等长替换，不改变 wasm 的 section 偏移、函数表与导入导出表，结构完全安全。

- In WASM bytecode, `f32.const 40.0` is encoded as `43 00 00 20 42` (IEEE754 little-endian single precision).
- `f32.const 100.0` is encoded as `43 00 00 c8 42`.
- The script scans every `.wasm` under `ManiaMapAnalyser by Leo_Black/js/ett/versions/` and replaces each `43 00 00 20 42` with `43 00 00 c8 42`.
- The replacement is equal-length (5 bytes → 5 bytes), so section offsets, function tables and import/export tables stay intact — structurally safe.

### 使用方法（Usage）

```bash
node tools/patch-minaclac-msd-cap.mjs
```

- 从仓库根目录或任意位置运行均可（路径按脚本自身位置解析）。
- 脚本是幂等的：某个文件里已没有 40.0 常量时会提示并跳过。
- 修改前会先把原始二进制复制一份到仓库的本地备份目录（该目录不入库）。
- 仓库内的 `.wasm` 由 git 跟踪，原始字节随时可以从 git 历史恢复。

- Run from the repository root or anywhere (paths resolve relative to the script itself).
- The script is idempotent: files with no remaining 40.0 constant are reported and skipped.
- Before modifying, a copy of each original binary is saved to the repository's local backup folder (never committed).
- The shipped `.wasm` files are git-tracked, so pristine bytes can always be restored from git history.

### 何时需要重新执行（When to re-run）

向 `ManiaMapAnalyser by Leo_Black/js/ett/versions/` 添加新的 MinaCalc 版本（新 `.wasm`）之后，请重新运行本脚本，确保新版本同样突破上限。

After adding a new MinaCalc version (a new `.wasm`) to `ManiaMapAnalyser by Leo_Black/js/ett/versions/`, re-run this script so the new version gets the cap lift too.

### 注意事项（Notes）

- **`minaclac-68.0-unofficial.wasm` 不参与 patch**：该版本中的 40.0 常量并非技能上限（实测其技能值本就可超过 40），patch 会改变普通谱面的输出，因此保持原样。
- **上层 JS 无需改动**：`calc.js` 的 `mapOutputValues` 直接透传 wasm 输出的 8 个技能值，上限突破后数值自然更高。
- **修改 wasm 字节后**，请同步递增 `ManiaMapAnalyser by Leo_Black/js/ett/constants.js` 中的 `WASM_ASSET_VERSION`（浏览器缓存失效用），避免用户加载到旧的 wasm。

- **`minaclac-68.0-unofficial.wasm` is NOT patched**: its 40.0 constants are not a skill cap (its skill values already exceed 40 in practice), and patching it would alter normal-map outputs.
- **No JS glue changes needed**: `calc.js`'s `mapOutputValues` passes through the 8 skill values from wasm directly; higher outputs flow through naturally.
- **After changing wasm bytes**, bump `WASM_ASSET_VERSION` in `ManiaMapAnalyser by Leo_Black/js/ett/constants.js` (used for browser cache busting) so users do not load a stale wasm.
