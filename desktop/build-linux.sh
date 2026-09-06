#!/usr/bin/env bash
# ManiaMapAnalyser desktop shell — Linux 打包（Windows 侧用 desktop/release.ps1）。
# 用法：desktop/build-linux.sh
# 依赖：Tauri Linux 系统库（与 CI shell-build.yml 的 build-linux 一致）：
#   sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev \
#     libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev
# 产物：cargo build --release → release/ 下插件 tar.gz
#       （含 mma-shell（保留执行位）、插件目录与 bridges/）。

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
install -m 0755 "$exe" "$stage/mma-shell"
tar -czf "$tarball" -C "$stage" .

echo "released: $tarball"
