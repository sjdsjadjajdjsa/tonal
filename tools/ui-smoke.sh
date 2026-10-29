#!/bin/sh
# Opens the real app on each main view with the debug hooks, takes a window
# snapshot and checks it rendered (not blank). Needs a display (Wayland, X11 or
# xvfb-run), mpv and ImageMagick (magick or convert). Snapshots land in target/ui-smoke/.
set -eu
cd "$(dirname "$0")/.."

cargo build --release --features debug-hooks
out=target/ui-smoke
rm -rf "$out"
mkdir -p "$out"
export XDG_DATA_HOME="$out/data" XDG_CACHE_HOME="$out/cache" TONAL_DEBUG_SNAPSHOT_SECS=4
fail=0
im=magick
command -v magick >/dev/null 2>&1 || im=convert

for view in home queue lyrics settings mini; do
    page=$view
    [ "$view" = home ] && page=""
    TONAL_DEBUG_VIEW=$page TONAL_DEBUG_SNAPSHOT="$out/$view.png" target/release/tonal >"$out/$view.log" 2>&1 &
    pid=$!
    sleep 7
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
    if [ ! -s "$out/$view.png" ]; then
        echo "FAIL $view: no snapshot"
        fail=1
        continue
    fi
    spread=$($im "$out/$view.png" -alpha off -colorspace gray -format '%[fx:standard_deviation*1000]' info:)
    if [ "${spread%.*}" -lt 5 ]; then
        echo "FAIL $view: blank window"
        fail=1
    else
        echo "ok   $view"
    fi
done

exit $fail
