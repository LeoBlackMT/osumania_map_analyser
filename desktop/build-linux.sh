#!/usr/bin/env bash
# ManiaMapAnalyser desktop shell — Linux 打包（Windows 侧用 release.ps1）。
# 用法：desktop/build-linux.sh [--check-signing]
# 依赖：Tauri Linux 系统库（与 CI shell-build.yml 的 build-linux 一致）：
#   sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev \
#     libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev
# 产物：cargo build --release → release/ 下插件 tar.gz
#       （含 mma-shell（保留执行位）、插件目录与 bridges 安装素材；开发产物与插件源码不入包）。
#       release.ps1 会用 MMA_RELEASE_DIR 把 tar.gz 放进同一个 `release/{timestamp}-{version}/`。
#
# 反馈签名（必填）：MMA_FEEDBACK_SIGNING_SEED（64 hex）+ MMA_FEEDBACK_KEY_ID，
# 或 MMA_FEEDBACK_SEED_FILE 指向只含 seed 的本地文件（gitignore）。
# 本地测试可设 MMA_FEEDBACK_ALLOW_DEV=1 使用内置 dev seed（服务端需 FEEDBACK_ALLOW_DEV_KEY=true）。

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(dirname "$script_dir")"
plugin_dir="$root/ManiaMapAnalyser by Leo_Black"
# 输出目录：release.ps1 用 MMA_RELEASE_DIR 指定同一个时间戳目录（相对仓库根或绝对路径）；
# 单独运行本脚本时回落到 release/。
if [ -n "${MMA_RELEASE_DIR:-}" ]; then
    case "$MMA_RELEASE_DIR" in
        /*) out_dir="$MMA_RELEASE_DIR" ;;
        *) out_dir="$root/$MMA_RELEASE_DIR" ;;
    esac
else
    out_dir="$root/release"
fi

# --- 反馈签名 seed：只从环境变量或本地 seed 文件读取，绝不写进仓库/日志 ---
seed="${MMA_FEEDBACK_SIGNING_SEED:-}"
if [ -z "$seed" ] && [ -n "${MMA_FEEDBACK_SEED_FILE:-}" ]; then
    if [ ! -f "$MMA_FEEDBACK_SEED_FILE" ]; then
        echo "MMA_FEEDBACK_SEED_FILE not found: $MMA_FEEDBACK_SEED_FILE" >&2
        exit 1
    fi
    seed="$(tr -d '[:space:]' < "$MMA_FEEDBACK_SEED_FILE")"
fi
seed="$(printf '%s' "$seed" | tr -d '[:space:]')"

if [ -n "$seed" ]; then
    if ! printf '%s' "$seed" | grep -Eq '^[0-9a-fA-F]{64}$'; then
        echo "MMA_FEEDBACK_SIGNING_SEED must be 64 hex chars (32 bytes); got ${#seed} chars" >&2
        exit 1
    fi
    export MMA_FEEDBACK_SIGNING_SEED="$(printf '%s' "$seed" | tr 'A-F' 'a-f')"
    export MMA_FEEDBACK_KEY_ID="${MMA_FEEDBACK_KEY_ID:-release}"
    if ! printf '%s' "$MMA_FEEDBACK_KEY_ID" | grep -Eq '^[A-Za-z0-9_-]{1,32}$'; then
        echo "MMA_FEEDBACK_KEY_ID must be 1-32 chars of [A-Za-z0-9_-]" >&2
        exit 1
    fi
    echo "feedback signing: key_id=$MMA_FEEDBACK_KEY_ID"
    if command -v go >/dev/null 2>&1 && [ -f "$root/backend/cmd/feedback-keygen/main.go" ]; then
        pub="$(cd "$root/backend" && go run ./cmd/feedback-keygen -derive -public-only 2>/dev/null | tail -n 1 || true)"
        if [ -n "$pub" ]; then
            echo "feedback public key: $pub"
            echo "  -> server FEEDBACK_PUBLIC_KEYS must contain: ${MMA_FEEDBACK_KEY_ID}:${pub}"
        else
            echo "warning: could not derive the public key" >&2
        fi
    else
        echo "warning: go not found; derive the public key with: cd backend && go run ./cmd/feedback-keygen -derive -public-only" >&2
    fi
elif [ "${MMA_FEEDBACK_ALLOW_DEV:-0}" = "1" ]; then
    unset MMA_FEEDBACK_SIGNING_SEED MMA_FEEDBACK_KEY_ID
    echo "warning: feedback signing uses the dev seed (key_id=dev)" >&2
else
    echo "error: set MMA_FEEDBACK_SIGNING_SEED (64 hex) or MMA_FEEDBACK_SEED_FILE; use MMA_FEEDBACK_ALLOW_DEV=1 for a local test build" >&2
    exit 1
fi

if [ "${1:-}" = "--check-signing" ]; then
    echo "signing check passed"
    exit 0
fi

# 版本号以插件 metadata.txt 为唯一来源（与 release.ps1 同源，避免漂移）。
version="$(sed -n 's/^Version:[[:space:]]*//p' "$plugin_dir/metadata.txt" | head -n 1 | tr -d '\r')"
if [ -z "$version" ]; then
    echo "version not found in $plugin_dir/metadata.txt" >&2
    exit 1
fi

if [ ! -f "$plugin_dir/index.html" ]; then
    echo "plugin dir not found: $plugin_dir" >&2
    exit 1
fi

(cd "$script_dir" && cargo build --release)
exe="${CARGO_TARGET_DIR:-$script_dir/target}/release/mma-shell"
if [ ! -f "$exe" ]; then
    echo "release binary missing: $exe" >&2
    exit 1
fi

mkdir -p "$out_dir"
tarball="$out_dir/ManiaMapAnalyser-by-Leo_Black-v$version-with-shell-linux.tar.gz"
rm -f "$tarball"

# 打包：插件目录 + mma-shell + bridges（桥安装素材；二进制保留执行位）。
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
# 与 Windows zip 相同的目录结构：**插件内容直接放在包根**（`index.html` 与
# `mma-shell` 同级），再放 bridges/ 与 offsets/。壳的 plugin_dir 规则 1
# （"exe 同级有 index.html"）因此命中；历史包多套了一层
# `ManiaMapAnalyser by Leo_Black/`，与 Windows 布局不一致。
cp -a "$plugin_dir/." "$stage/"
cp -r "$root/bridges" "$stage/bridges"
if [ -d "$root/desktop/offsets" ]; then
    cp -r "$root/desktop/offsets" "$stage/offsets"
fi
install -m 0755 "$exe" "$stage/mma-shell"

# ⚠️ 开发产物一律不入包：NuGet 包缓存（`bepinex/.tools/nuget-packages`，约 97 MB）、
# 本地断言工程（`plugin/tests/`）与 `plugin/{bin,obj}/`。`.gitignore` 只管 git、不管拷贝。
# 与 release.ps1 的 robocopy /XD 同一份规则：按目录名排除，任意层级生效。
find "$stage/bridges" -type d \( -name .tools -o -name bin -o -name obj -o -name tests \) -prune -exec rm -rf {} +

# `bepinex/plugin/` 是插件的**开发树**（5 个 .cs、.csproj、NuGet.Config、build.ps1）。
# 安装器只读 MMAMalodySelection.dll（install-bridge.ps1 的 $BridgeDllRel）；LICENSE 是该 DLL
# 的 MIT 归属声明、NOTICES.md 是它的来源与复现说明，两者必须随二进制分发。其余文件对用户
# 没有任何用途，只留这三份（与 release.ps1 的 $pluginShip 同一份清单）。
find "$stage/bridges/malody/bepinex/plugin" -maxdepth 1 -type f \
    ! -name 'MMAMalodySelection.dll' ! -name 'LICENSE' ! -name 'NOTICES.md' -delete

# 兜底断言：安装素材必须随包分发。加载器与 Unity 参考程序集都是 gitignore 的本地资产，
# 插件 DLL 由 build.ps1 落盘——缺任何一个，安装器都会拒绝安装。
for rel in \
    bridges/malody/bepinex/loader/bepinex-il2cpp-788.zip \
    bridges/malody/bepinex/unity-libs/2022.3.62.zip \
    bridges/malody/bepinex/plugin/MMAMalodySelection.dll ; do
    if [ ! -f "$stage/$rel" ]; then
        echo "release package is missing a required asset: $rel" >&2
        exit 1
    fi
done

# 包内目录结构与 Windows zip 保持一致：没有外层包装目录，条目直接是
# `ManiaMapAnalyser by Leo_Black/…`、`bridges/…`、`mma-shell`、`offsets/…`。
# （`tar -C "$stage" .` 会写成 `./…` 并附带一个 `./` 条目，部分解压工具会把它
# 当成隐藏根目录，和 zip 不一致。）`find -printf %P` 去掉 `./` 前缀，且用 NUL
# 分隔，路径里的空格不会把文件名拆坏。
( cd "$stage" && find . -mindepth 1 -maxdepth 1 -printf '%P\0' | tar --null -czf "$tarball" -T - )

echo "released: $tarball"
