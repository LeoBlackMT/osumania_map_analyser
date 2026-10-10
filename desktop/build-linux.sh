#!/usr/bin/env bash
# ManiaMapAnalyser desktop shell — Linux 打包（Windows 侧用 release.ps1）。
# 用法：desktop/build-linux.sh
# 依赖：Tauri Linux 系统库（与 CI shell-build.yml 的 build-linux 一致）：
#   sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev \
#     libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev
# 产物：cargo build --release → release/ 下插件 tar.gz
#       （含 mma-shell（保留执行位）、插件目录与 bridges 安装素材；开发产物与插件源码不入包）。

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(dirname "$script_dir")"
plugin_dir="$root/ManiaMapAnalyser by Leo_Black"
out_dir="$root/release"

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
exe="$script_dir/target/release/mma-shell"
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
cp -r "$plugin_dir" "$stage/"
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

tar -czf "$tarball" -C "$stage" .

echo "released: $tarball"
