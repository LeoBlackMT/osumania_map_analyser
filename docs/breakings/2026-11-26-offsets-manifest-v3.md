# 偏移表远端更新与 manifest schema 3 / Offset remote update and manifest schema 3

- 日期 / Date: 2026-11-26
- 分支 / Branch: `fix/offsets-remote-update`
- 影响面 / Scope: 桌面壳 offsets 远端更新、`desktop/offsets/manifest.json`、stable/ppysb/lazer 偏移表、维护工具；不涉及页面帧契约与插件版本号。

## 修改内容（What changed）

- `desktop/offsets/manifest.json` 从 schema 2 升到 schema 3，新增 `manifest_signature`：对去掉该字段后的 canonical JSON 签名，绑定 `updated_at`、表列表与每张表的元数据。
- 每张 manifest 表新增单调 `revision`；stable/ppysb 的 revision 写入表 JSON 内，lazer 的 revision 由 `lazer_version` 推导（`2026.1117` → `20261117`）。
- 远端更新门从字符串版本比较改为 revision 比较；stable/ppysb 不再使用哈希式 `version` 做大小判断。
- `POST /offsets/update` 增加 manifest schema/signature 校验、client 白名单、文件名安全校验、LF 规范、下载超时/重试/大小上限、更新锁、唯一临时文件与逐表错误状态（`ok` / `partial` / `error` / `busy`）。
- `/offsets/status` 返回实际找到的最新 lazer 表及 stable/ppysb 来源、revision；设置页更新按钮在请求期间禁用，并显示失败表名/阶段。
- 新增离线 `tools/offsets-tool`（`sign` / `verify`）；CI 在 Linux job 执行 `verify`。
- `cargo test --lib` 的 offsets 测试改为只读校验 manifest 签名、表 SHA-256/Ed25519、revision 与 manifest 覆盖完整性；不再在 `cargo test` 时覆写 `desktop/offsets/manifest.json`。
- `tools/stable-offsets-gen` 生成 stable/ppysb 表时写入当前 UTC 日期 revision，支持 `--revision` 覆盖。
- 所有 offsets JSON 与 manifest 强制 LF（`.gitattributes`）。

## 修改原因（Why）

- 旧 manifest 的表签名在 Windows 工作区按 CRLF 生成，而 GitHub raw 提供 LF，导致真实下载路径验签失败；测试“自签自验”无法发现。
- stable/ppysb 的 `version` 是构建哈希/`ppysb-latest`，`version_is_newer` 会把它归一成 0，远端更新永远不会生效。
- 旧更新逻辑对下载、验签、schema、写盘失败全部静默 `continue`，最后仍返回 `ok`，用户与维护者都无法判断更新是否真的发生。
- 旧 offsets 测试会以 2 张旧表覆盖当前 4 张表 manifest；且 `client`/`filename` 来自远端且未校验，存在路径拼接风险。
- manifest 只签表内容、不签元数据；新增 manifest 级签名以绑定 `updated_at`、表列表与元数据。

## 影响范围（Scope）

- 受影响：桌面壳 v3.0.0+ 的 offsets 远端更新、`desktop/offsets/*` 的 schema/签名、`tools/stable-offsets-gen`、`tools/offsets-tool`、设置页 offsets 状态/更新反馈。
- 不受影响：页面帧契约（仍为 v12）、插件版本号（仍为 3.0.3）、反馈签名、后端 API、非 offsets 的壳功能。
- 兼容性：旧壳（v3.0.0–v3.0.3）会忽略 manifest 的未知 `revision` / `manifest_signature` 字段，仍按旧逻辑读取；新 manifest 的表级签名仍使用现有公钥。本次没有更换签名密钥。旧壳的 stable/ppysb 版本门仍是旧实现（本次修复只对新壳生效）；如需旧壳行为完全一致，请随下个壳版本发布。

## 兼容策略（Compat）

- manifest 保持 `schema_version` 3；新壳要求 `manifest_signature`，旧壳不检查 schema 版本。
- 表 JSON 对旧壳是增量字段（`revision`），serde 默认忽略未知字段；旧壳仍可读取表内容并用表级签名验证。
- 远端更新失败时保留本地/随包表，不删除现有缓存；`offline_fallback` 仅用于 manifest 网络不可达。
- 回滚：恢复旧 `manifest.json` 与旧 `desktop/src/server/http.rs`/`remote.rs` 即可；回滚后 stable/ppysb 的旧版本门问题会重新出现。

## 验证方式（Verification）

- `cargo test --lib`：495 passed，0 failed，1 ignored；包含 manifest 签名、表签名、revision 与覆盖完整性检查。
- `cargo check --release`：通过。
- `cargo run --manifest-path tools/offsets-tool/Cargo.toml -- verify --root desktop/offsets`：5/5 表通过。
- 用 `file://` manifest + 临时 APPDATA 的端到端测试：首次更新 2 张表，第二次 0 updated、全部 current；临时测试文件已删除。
- `node --check`（settings 页与 i18n）与 `node tools/i18n-check.mjs`：通过。
- `tools/stable-offsets-gen` 使用 `rustc` 编译通过。

---

# English

## What changed

- `desktop/offsets/manifest.json` moves from schema 2 to schema 3 and gains `manifest_signature`, a signature over the canonical JSON without that field, binding `updated_at`, the table list, and per-table metadata.
- Every manifest table gains a monotonic `revision`; stable/ppysb revisions live inside the table JSON, while lazer revisions are derived from `lazer_version` (`2026.1117` → `20261117`).
- The remote update gate switches from string version comparison to revision comparison; stable/ppysb no longer compare their hash-like `version` strings.
- `POST /offsets/update` now validates manifest schema/signature, uses a client allowlist and safe-filename checks, enforces LF, applies download timeouts/retries/size limits, serializes updates, uses unique temp files, and returns per-table `ok` / `partial` / `error` / `busy` statuses.
- `/offsets/status` reports the latest lazer table actually found plus stable/ppysb source and revision; the settings update button is disabled while a request is in flight and displays failing filenames/stages.
- New offline `tools/offsets-tool` (`sign` / `verify`); the Linux CI job runs `verify`.
- The `cargo test --lib` offsets test is now read-only and verifies manifest signature, table SHA-256/Ed25519, revisions, and manifest coverage; it no longer overwrites `desktop/offsets/manifest.json` during `cargo test`.
- `tools/stable-offsets-gen` writes the current UTC date as the stable/ppysb revision and supports `--revision` overrides.
- All offsets JSON and the manifest are forced to LF (`.gitattributes`).

## Why

- The old manifest table signatures were generated from CRLF bytes on Windows while GitHub raw serves LF, so the real download path failed verification; the self-sign/self-verify test could not catch it.
- stable/ppysb `version` values are build hashes or `ppysb-latest`; `version_is_newer` collapses them to 0, so remote updates never applied.
- The old update loop silently `continue`d on download/signature/schema/write failures and still returned `ok`, leaving users and maintainers unable to tell whether an update happened.
- The old offsets test overwrote the current 4-table manifest with a 2-table list; `client`/`filename` came from the remote manifest without validation and were joined into filesystem paths.
- The manifest signed only table bytes, not metadata; a manifest-level signature now binds `updated_at`, the table list, and metadata.

## Scope

- Affected: desktop shell v3.0.0+ offsets remote updates, `desktop/offsets/*` schema/signatures, `tools/stable-offsets-gen`, `tools/offsets-tool`, and the settings offsets status/update feedback.
- Unaffected: page frame contract (still v12), plugin version (still 3.0.3), feedback signing, backend API, and non-offsets shell features.
- Compatibility: old shells (v3.0.0–v3.0.3) ignore the unknown `revision` / `manifest_signature` manifest fields and keep their old behavior; table-level signatures still use the existing public key. No signing-key rotation was performed in this change. The old stable/ppysb version gate remains in old shells; only new shells receive this fix.

## Compat

- The manifest stays at `schema_version` 3; new shells require `manifest_signature`, while old shells do not check the schema version.
- `revision` is an additive table field for old shells (serde ignores unknown fields); old shells can still read table content and verify table signatures.
- Failed remote updates keep the local/bundled tables; `offline_fallback` is only used when the manifest host is unreachable.
- Rollback: restore the previous `manifest.json` and the previous `desktop/src/server/http.rs` / `remote.rs`; the old stable/ppysb version-gate bug will return.

## Verification

- `cargo test --lib`: 495 passed, 0 failed, 1 ignored, including manifest signature, table signatures, revisions, and coverage checks.
- `cargo check --release`: passed.
- `cargo run --manifest-path tools/offsets-tool/Cargo.toml -- verify --root desktop/offsets`: 5/5 tables passed.
- End-to-end `file://` manifest test with a temporary APPDATA: first run updated 2 tables; second run updated 0 and reported all current; the temporary test file was removed.
- `node --check` (settings page and i18n) and `node tools/i18n-check.mjs`: passed.
- `tools/stable-offsets-gen`: compiled with `rustc`.
