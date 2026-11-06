# 谱面卡片显示错图：请求身份与响应体不是同一快照

> 归类：知识与教训（learnings）。记录一次“卡片显示的谱面与 osu stable/lazer 实际选中的谱面不一致”的排查结论，重点是可复用的判断方法与踩坑点。
> 代码基线：`main`（3.0.0 之后，XDC1 合并后）。

## 结论

用户看到“卡片显示 A 图，游戏实际选中 B 图”，且标题、SR、LN%、Keys、Roxy 估值、轨道图一起错，指向的是**整份分析快照被写到错误的缓存键下**，不是 UI 渲染错位，也不是 `resultCache` 的 LRU 淘汰。真正的根因有三层：

1. **请求身份与响应体不是同一个原子快照**：`/files/beatmap/file` 按“请求时的当前谱面”供奉，页面在请求开始时快照 `identity`，却没有校验响应体 `.osu` 的 `[Metadata]` 是否属于这张图。请求期间用户切图，body 是 B，代码却按 A 的 identity 解析、渲染并写缓存；之后切回 A 就命中一份 B 的快照。
2. **换图不会立即作废在途分析**：`analysisRequestSeq` 只在新 `fetchBeatmapFile` 开始时自增，而换图后要等 200ms 防抖才发新请求；这段窗口里的旧请求仍能通过 stale 检查，拿到新图 body 并按旧 identity 提交。
3. **同 id 不同 hash 被判成同图**：旧 `isSameBeatmapIdentity` 在两边都有 `id` 时只比 id，忽略 hash/path，导致导入/替换同 id 谱面（hash 变了、id 没变）永远不重算。

另外，壳环境的 `fullChartTexts`（全谱原文 LRU）只按 identity 存字符串，没有内容校验，也不会被 `clearResultCache()` 清掉；错误原文可以跨缓存清空存活，扩大症状。

## 为什么不是“普通缓存淘汰”

`resultCache` 的每条快照是完整分析结果：`parsedInfo.metadata`、`rework`（star/lnRatio/columnCount/graph）、`patternReport`、`ettResult`、`interludeStar`、`ppMetrics`、`laneAccText`。LRU 淘汰只会让 `get(key)` miss，然后重算当前图；它不会把 B 的 metadata 搬到 A 的 key 上。能显示整张错图只有两种可能：**key 碰撞**，或**写入时身份错配**。这次两者都存在。

## 修复模式（可复用）

- **请求作用域内容键**：`contentKey = hash > 目录限定 path > id > meta`。缓存键与原文 LRU 都用请求开始时快照的 contentKey，不用可变的 `state.lastBeatmapIdentity`。
- **响应体校验**：fetch 回来后只读 `.osu [Metadata]`（不跑完整解析），比对 Artist/Title/Version/Creator/BeatmapID/BeatmapSetID；不一致就丢弃本次 body（不 remember、不渲染、不写缓存）。Artist/Title 要同时接受 `Artist`/`ArtistUnicode` 与 `Title`/`TitleUnicode`，因为 tosu 的 `beatmap.artist/title` 可能是显示值。
- **卡片状态代数**：换图/换 mod 时 `analysisEpoch++`，`isStaleRequest()` 同时比较 `requestSeq` 与 `epoch`。这比“只在新 fetch 开始时推进序号”可靠，因为防抖窗口里没有新 fetch。
- **弱帧合并**：同一张图的帧可能缺 hash/path/version/mapper；用上一帧补齐缺失段，只在 hash 或可比元数据冲突时判换图。不要退回“id-only 比较”，也不要让弱帧覆盖完整身份。
- **缓存快照身份校验**：快照写入 `contentKey`/`metaKey`，命中时再验一次；原文 LRU 存 `{text, metaKey}`，读取时 metaKey 不一致直接 miss。
- **清理路径统一**：`clearResultCache()`、关闭结果缓存、清卡时同时 `clearFullChartTexts()`，避免原文 LRU 跨失效存活。

## 踩坑点

- **不要把 `files.beatmap` 当路径**：它只是裸文件名；`directPath.beatmapFile` 才是 `folder\filename`。不同目录的同名 `.osu` 会拼出相同 `path:` 身份。
- **不要把背景图文件名当目录**：`directPath.beatmapBackground` 是 `folder\background.jpg`，用它当 mapset 目录会把同一 mapset 的不同难度误判为换歌。
- **不要把 `beatmap.mapper` 和 `.osu Creator` 的 Unicode/空值问题当成必然匹配**：只做“两边都有值才比较”，避免缺字段误杀。
- **stale 守卫要覆盖每个 await**：fetch、worker、Companella、LaneAcc 补抓、finally 都要判；只判开头等于没判。
- **结果缓存正确性不只靠 key**：key 相同但快照内容/元信息不符时，仍要拒绝复用；缓存版本前缀在键语义变化时必须 bump。

## 验证方法

- 纯函数测试：同 id 不同 hash、同图弱帧合并、同名不同目录、A body 对 B frame、Unicode Artist/Title、meta-only 退化身份。
- DOM 桩竞态测试：请求 A 未返回时切到 B，A 的响应必须因 epoch 变化而作废；请求 A 但端点返回 B body，必须拒绝且不写原文/结果缓存；真实 `.osu` 样本必须通过校验并进入管线。
- 手工验收：shell/tosu 两端各做一次“快速切图”“导入/替换同 id 谱面”“A→B→A 来回切”，确认卡片标题与游戏实际选中一致；壳环境额外确认清缓存后不再从原文 LRU 取到旧图。

## 参考

- `ManiaMapAnalyser by Leo_Black/js/app/core/beatmapIdentity.js`
- `ManiaMapAnalyser by Leo_Black/js/app/core/analysis.js`（`fetchBeatmapFile` / `isStaleRequest` / 写门）
- `ManiaMapAnalyser by Leo_Black/js/app/net/socketHandlers.js`（`applyBeatmapState`）
- `ManiaMapAnalyser by Leo_Black/js/app/graph/section.js`（`fullChartTexts`）
- `docs/pipeline/result-cache.md`、`docs/pipeline/analysis-pipeline.md`

---

# Beatmap card shows the wrong map: request identity and response body are not the same snapshot

> Category: knowledge & lessons learned. This records the investigation of "the card shows a different beatmap than the one actually selected in osu stable/lazer", focusing on reusable reasoning and pitfalls.
> Baseline: `main` (after 3.0.0, after the XDC1 merge).

## Conclusion

When the card shows map A while the game has map B selected, and the title, SR, LN%, Keys, Roxy estimate and lane graph are all from A, the cause is a **whole analysis snapshot written under the wrong cache key** — not a UI rendering offset and not `resultCache` LRU eviction. There are three layers:

1. **Request identity and response body are not an atomic snapshot**: `/files/beatmap/file` serves whatever map is current when the request is handled. The page snapshots `identity` before fetching but never verifies that the returned `.osu` `[Metadata]` belongs to it. If the user switches during the request, the body is B while the code parses/renders/caches it as A; switching back to A later hits a snapshot of B.
2. **A beatmap change does not immediately invalidate in-flight analysis**: `analysisRequestSeq` only advances when a new `fetchBeatmapFile` starts, and a map change waits for the 200ms debounce before starting one. During that window the old request still passes the stale check and can commit the new map's body under the old identity.
3. **Same id, different hash is treated as the same map**: the old `isSameBeatmapIdentity` compared only the id when both sides had one, ignoring hash/path. Importing or replacing a map with the same online id but different file content therefore never triggered a recompute.

In the shell, `fullChartTexts` (the full-chart text LRU) also stored plain strings keyed by identity without content validation, and `clearResultCache()` did not clear it; a wrong text could survive a cache clear.

## Why ordinary cache eviction is not the explanation

Each `resultCache` entry is a complete snapshot: `parsedInfo.metadata`, `rework` (star/lnRatio/columnCount/graph), `patternReport`, `ettResult`, `interludeStar`, `ppMetrics`, `laneAccText`. Eviction only causes a miss and a recompute of the current map; it cannot move B's metadata onto A's key. A whole wrong card requires either **key collision** or **identity mismatch at write time**. Both existed here.

## Reusable fix pattern

- **Request-scoped content key**: `contentKey = hash > folder-qualified path > id > meta`. Cache keys and the text LRU use the key snapshotted at request start, never the mutable `state.lastBeatmapIdentity`.
- **Response body validation**: after fetching, read only `.osu [Metadata]` (no full parse) and compare Artist/Title/Version/Creator/BeatmapID/BeatmapSetID. On mismatch discard the body entirely (no remember, no render, no cache write). Accept both `Artist`/`ArtistUnicode` and `Title`/`TitleUnicode`, because tosu's `beatmap.artist/title` may be display values.
- **Card-state epoch**: increment `analysisEpoch` on beatmap/mod changes and compare both `requestSeq` and `epoch` in `isStaleRequest()`. This covers the debounce window where no new fetch has started yet.
- **Weak-frame merge**: frames of the same map may be missing hash/path/version/mapper; fill missing parts from the previous frame and only treat hash or comparable metadata conflicts as a map change. Never fall back to id-only comparison, and never let a weak frame overwrite a complete identity.
- **Snapshot identity validation**: store `contentKey`/`metaKey` in result snapshots and re-check on hit; store `{text, metaKey}` in the text LRU and miss when metaKey differs.
- **Unified invalidation**: `clearResultCache()`, disabling the result cache, and card clears must also call `clearFullChartTexts()`.

## Pitfalls

- **`files.beatmap` is not a path**: it is a bare filename; `directPath.beatmapFile` is `folder\filename`. Same-named `.osu` files in different folders otherwise collide on `path:`.
- **Do not use the background filename as a folder**: `directPath.beatmapBackground` is `folder\background.jpg`; using it as the mapset folder misclassifies difficulty switches as song changes.
- **Mapper/creator and Unicode fields may be missing**: only compare fields that exist on both sides; do not reject a valid body just because one side lacks a field.
- **Stale guards must cover every await**: fetch, worker, Companella, the LaneAcc fallback fetch, and `finally` all need the check; checking only at the start is equivalent to no check.
- **Result cache correctness is not only about the key**: reject a snapshot whose content/metadata does not match even if the key matches; bump the cache version prefix when key semantics change.

## Verification

- Pure-function tests: same id/different hash, weak-frame merge, same filename in different folders, A body against B frame, Unicode Artist/Title, meta-only degraded identity.
- DOM-stub race tests: switching to B while A's request is pending must invalidate A via epoch; a B body returned for an A request must be rejected without writing the text/result cache; a real `.osu` sample must pass validation and enter the pipeline.
- Manual acceptance: on both shell and tosu, run "rapid switching", "import/replace a map with the same id", and "A→B→A"; confirm the card title matches the game selection. In the shell, also confirm a cache clear cannot return an old text from the text LRU.

## References

- `ManiaMapAnalyser by Leo_Black/js/app/core/beatmapIdentity.js`
- `ManiaMapAnalyser by Leo_Black/js/app/core/analysis.js` (`fetchBeatmapFile` / `isStaleRequest` / write gate)
- `ManiaMapAnalyser by Leo_Black/js/app/net/socketHandlers.js` (`applyBeatmapState`)
- `ManiaMapAnalyser by Leo_Black/js/app/graph/section.js` (`fullChartTexts`)
- `docs/pipeline/result-cache.md`, `docs/pipeline/analysis-pipeline.md`
