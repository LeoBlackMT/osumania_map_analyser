// beatmapIdentity.js — 谱面身份与 .osu 内容校验（纯函数，无 DOM / window）。
//
// 背景：`/files/beatmap/file` 供奉的是**请求那一刻**的当前谱面；WS 帧里的 identity
// 与 HTTP body 不是同一个原子快照。请求开始后用户切图，body 可能是另一张图，
// 若仍按请求开始时的 identity 解析/渲染/写缓存，整份快照（标题/SR/图表/LaneAcc）
// 都会被写到错误的缓存键下并跨切图存活。
//
// 这里只提供两件事：
//   1. 结构化身份：id/hash/path/meta 分开保存，能合并弱帧、能识别“同 id 换文件”；
//   2. 内容校验：从 .osu 的 [Metadata] 读回 Artist/Title/Version/Creator/BeatmapID，
//      与请求身份比对，不一致就拒绝这份 body。

function normalizeText(value) {
    if (value == null) return "";
    return String(value).trim();
}

function normalizeHash(value) {
    return normalizeText(value).toLowerCase();
}

function normalizePath(value) {
    let normalized = normalizeText(value).replace(/\\/g, "/");
    if (!normalized) return "";
    normalized = normalized.replace(/\/+/g, "/").toLowerCase();
    while (normalized.startsWith("./")) {
        normalized = normalized.slice(2);
    }
    return normalized;
}

function hasMetaKey(metaKey) {
    return Boolean(metaKey);
}

function normMetaValue(value) {
    const text = normalizeText(value);
    // NFC：不同来源可能一个给组合字符、一个给预组合字符，避免把同一标题判成冲突。
    return (typeof text.normalize === "function" ? text.normalize("NFC") : text).toLowerCase();
}

/** 两边都有值的元数据字段是否冲突（缺字段不算冲突，避免弱帧误判）。 */
function metaConflicts(previous, next) {
    for (const key of ["artist", "title"]) {
        const previousValues = [previous.meta[key], previous.meta[`${key}Unicode`]]
            .map(normMetaValue).filter(Boolean);
        const nextValues = [next.meta[key], next.meta[`${key}Unicode`]]
            .map(normMetaValue).filter(Boolean);
        if (previousValues.length > 0 && nextValues.length > 0
            && !previousValues.some((value) => nextValues.includes(value))) {
            return true;
        }
    }
    for (const key of ["version", "mapper"]) {
        const previousValue = normMetaValue(previous.meta[key]);
        const nextValue = normMetaValue(next.meta[key]);
        if (previousValue && nextValue && previousValue !== nextValue) {
            return true;
        }
    }
    return false;
}

/** 两边至少有一个共同可比的元数据字段。 */
function hasSharedMeta(previous, next) {
    return ["artist", "title", "version", "mapper"].some(
        (key) => previous.meta[key] && next.meta[key],
    );
}

/**
 * 构造结构化身份。
 *
 * - `identity` 仍是展示/选区复位用的 `id|hash|path` 组合串（全缺时 `meta:`）；
 * - `contentKey` 是缓存/原文 LRU 的键：hash > folder-qualified path > id > meta。
 *   它比 identity 更窄：只要文件内容不变，弱帧不会让键抖动。
 */
export function createBeatmapIdentityFrame({ id = "", hash = "", path = "", set = "", meta = {}, rawIdentity = "" } = {}) {
    const cleanId = normalizeText(id);
    const cleanHash = normalizeHash(hash);
    const cleanPath = normalizePath(path);
    const cleanSet = normalizeText(set);
    const cleanMeta = {
        artist: normalizeText(meta.artist),
        artistUnicode: normalizeText(meta.artistUnicode),
        title: normalizeText(meta.title),
        titleUnicode: normalizeText(meta.titleUnicode),
        version: normalizeText(meta.version),
        mapper: normalizeText(meta.mapper),
    };
    // metaKey 保留 artist/title/version/mapper，用于 meta: 降级身份与内容校验。
    // 全空时必须是空串而不是 "::"——否则任意两张无 meta 的 map 都会被判成同图。
    const metaParts = [cleanMeta.artist, cleanMeta.title, cleanMeta.version, cleanMeta.mapper];
    const metaKey = metaParts.some(Boolean) ? metaParts.join("::").toLowerCase() : "";

    const identityParts = [];
    if (cleanId) identityParts.push(`id:${cleanId}`);
    if (cleanHash) identityParts.push(`hash:${cleanHash}`);
    if (cleanPath) identityParts.push(`path:${cleanPath}`);
    if (identityParts.length === 0 && hasMetaKey(metaKey)) {
        identityParts.push(`meta:${metaKey}`);
    }
    const identity = identityParts.join("|") || normalizeText(rawIdentity);

    const contentKey = cleanHash
        ? `hash:${cleanHash}`
        : cleanPath
            ? `path:${cleanPath}`
            : cleanId
                ? `id:${cleanId}`
                : hasMetaKey(metaKey)
                    ? `meta:${metaKey}`
                    : identity;

    return {
        identity,
        contentKey,
        id: cleanId,
        hash: cleanHash,
        path: cleanPath,
        set: cleanSet,
        metaKey,
        meta: cleanMeta,
    };
}

/** 从已有的 identity 字符串反解结构化身份（外部源 / 兼容旧状态）。 */
export function parseBeatmapIdentityFrame(identity = "") {
    const raw = normalizeText(identity);
    const parts = raw.split("|");
    const pick = (prefix) => {
        const part = parts.find((entry) => entry.startsWith(prefix));
        return part ? part.slice(prefix.length) : "";
    };
    const metaRaw = pick("meta:");
    const metaParts = metaRaw ? metaRaw.split("::") : [];
    return createBeatmapIdentityFrame({
        id: pick("id:"),
        hash: pick("hash:"),
        path: pick("path:"),
        meta: {
            artist: metaParts[0] || "",
            title: metaParts[1] || "",
            version: metaParts[2] || "",
            mapper: metaParts[3] || "",
        },
        rawIdentity: raw,
    });
}

/**
 * 两次结构化身份是否仍是同一张图。
 *
 * 规则：
 * - 两边都有 id：id 必须相同；任何一边有 hash 时 hash 冲突即不同图；
 *   两边都有值的元数据字段冲突也算不同图（防 tosu 用旧 id 配新 metadata）。
 * - 否则两边都有 hash：hash 必须相同（元数据冲突同样判不同图）。
 * - 一边有稳定段、另一边只有 meta：元数据不冲突且有共同字段 ⇒ 同图（弱帧）。
 * - 只有 meta：元数据不冲突且有共同字段 ⇒ 同图；否则退回整串比较。
 */
export function isSameBeatmapIdentityFrames(previous, next) {
    if (!previous || !next) {
        return false;
    }
    if (previous.id && next.id) {
        if (previous.id !== next.id) return false;
        if (previous.hash && next.hash && previous.hash !== next.hash) return false;
        return !metaConflicts(previous, next);
    }
    if (previous.hash && next.hash) {
        if (previous.hash !== next.hash) return false;
        return !metaConflicts(previous, next);
    }
    const previousStable = Boolean(previous.id || previous.hash);
    const nextStable = Boolean(next.id || next.hash);
    if (previousStable !== nextStable) {
        return !metaConflicts(previous, next) && hasSharedMeta(previous, next);
    }
    if (previousStable && nextStable) {
        if (!metaConflicts(previous, next) && hasSharedMeta(previous, next)) {
            return true;
        }
        return previous.identity !== "" && previous.identity === next.identity;
    }
    if (!metaConflicts(previous, next) && hasSharedMeta(previous, next)) {
        return true;
    }
    return previous.identity !== "" && previous.identity === next.identity;
}

/** 字符串版（外部源等只有 identity 字符串的调用方）。 */
export function isSameBeatmapIdentity(previous, next) {
    const a = parseBeatmapIdentityFrame(previous);
    const b = parseBeatmapIdentityFrame(next);
    return isSameBeatmapIdentityFrames(a, b);
}

/**
 * 同图弱帧合并：next 缺的段用 previous 补，避免「弱帧覆盖完整身份」导致
 * 缓存键/原文 LRU 反复失效；next 带新 hash/path 时升级为更完整的身份。
 * 只在 `isSameBeatmapIdentityFrames(previous, next) === true` 时调用。
 */
export function mergeBeatmapIdentityFrames(previous, next) {
    if (!previous) {
        return next;
    }
    return createBeatmapIdentityFrame({
        id: next.id || previous.id,
        hash: next.hash || previous.hash,
        path: next.path || previous.path,
        set: next.set || previous.set,
        meta: {
            artist: next.meta.artist || previous.meta.artist,
            artistUnicode: next.meta.artistUnicode || previous.meta.artistUnicode,
            title: next.meta.title || previous.meta.title,
            titleUnicode: next.meta.titleUnicode || previous.meta.titleUnicode,
            version: next.meta.version || previous.meta.version,
            mapper: next.meta.mapper || previous.meta.mapper,
        },
        rawIdentity: next.identity || previous.identity,
    });
}

/** 只读 [Metadata] 段（不跑完整 OsuFileParser，失败路径代价极低）。 */
export function parseOsuMetadataHeader(osuText) {
    const metadata = {};
    const lines = String(osuText || "").split(/\r?\n/);
    let inMetadata = false;
    for (const rawLine of lines) {
        const line = rawLine.trim();
        if (!line) continue;
        if (line.startsWith("[") && line.endsWith("]")) {
            inMetadata = line === "[Metadata]";
            if (!inMetadata && Object.keys(metadata).length > 0) {
                break;
            }
            continue;
        }
        if (!inMetadata) continue;
        const splitIndex = line.indexOf(":");
        if (splitIndex <= 0) continue;
        metadata[line.slice(0, splitIndex).trim()] = line.slice(splitIndex + 1).trim();
    }
    return metadata;
}

/**
 * 校验一份 .osu body 是否属于请求身份。
 *
 * 返回 `{ ok, checked }`：
 * - `ok=false`：至少一个可比字段冲突（或请求有元数据但 body 完全读不到）；
 * - `checked=false`：没有任何可比字段（例如外部源文本），调用方按“无法校验”处理。
 *
 * 只做“有冲突才拒绝”，不对缺失字段过度推断：部分谱面/客户端会缺 mapper，
 * 不能因此把正确的 body 丢掉。
 */
export function beatmapTextMatchesFrame(osuText, frame) {
    const metadata = parseOsuMetadataHeader(osuText);
    const expected = frame && frame.meta ? frame.meta : {};
    let compared = 0;
    let missing = 0;
    let mismatch = false;

    // Artist/Title 在 tosu 里可能给 `artist`（显示值/Unicode）而不是 `.osu` 的
    // `Artist`（原始 ASCII）；两边都接受 Unicode 变体，避免把正确 body 误杀。
    const compareMulti = (expectedValues, bodyValues) => {
        const wants = expectedValues.map(normMetaValue).filter(Boolean);
        if (wants.length === 0) {
            return;
        }
        const gots = bodyValues.map(normMetaValue).filter(Boolean);
        if (gots.length === 0) {
            missing += 1;
            return;
        }
        compared += 1;
        if (!wants.some((want) => gots.includes(want))) {
            mismatch = true;
        }
    };

    compareMulti([expected.artist, expected.artistUnicode], [metadata.Artist, metadata.ArtistUnicode]);
    compareMulti([expected.title, expected.titleUnicode], [metadata.Title, metadata.TitleUnicode]);
    compareMulti([expected.version], [metadata.Version]);
    compareMulti([expected.mapper], [metadata.Creator]);

    const expectedId = normalizeText(frame && frame.id);
    const bodyId = normalizeText(metadata.BeatmapID);
    if (expectedId && bodyId && bodyId !== "0") {
        compared += 1;
        if (bodyId !== expectedId) {
            mismatch = true;
        }
    }

    const expectedSet = normalizeText(frame && frame.set);
    const bodySet = normalizeText(metadata.BeatmapSetID);
    if (expectedSet && bodySet && bodySet !== "0" && bodySet !== "-1") {
        compared += 1;
        if (bodySet !== expectedSet) {
            mismatch = true;
        }
    }

    if (mismatch) {
        return { ok: false, checked: true };
    }
    if (compared > 0) {
        return { ok: true, checked: true };
    }
    if (missing > 0) {
        return { ok: false, checked: false };
    }
    return { ok: true, checked: false };
}
