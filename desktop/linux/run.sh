#!/usr/bin/env bash
set -euo pipefail
package_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
export MMA_PLUGIN_DIR="$package_dir"
exec "$package_dir/mma-shell" "$@"
