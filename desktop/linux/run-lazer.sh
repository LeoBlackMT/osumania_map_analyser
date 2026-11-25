#!/usr/bin/env bash
set -euo pipefail
package_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
if pgrep -u "$UID" -x 'osu!' >/dev/null; then
    printf '%s\n' 'Close osu!lazer first, then run this launcher so LaneAcc can read its memory.' >&2
    exit 1
fi
if curl --fail --silent --max-time 1 http://127.0.0.1:24061/shell-config >/dev/null 2>&1; then
    printf '%s\n' 'Close the existing mma-shell first, then run this launcher to start both the shell and osu!lazer.' >&2
    exit 1
fi
# The dashboard is available even while tosu is waiting for the game.
if ! curl --fail --silent --max-time 1 http://127.0.0.1:24050/ >/dev/null 2>&1; then
    systemctl --user start tosu.service
fi
exec "$package_dir/run.sh" --launch-lazer
