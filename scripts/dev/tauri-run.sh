#!/bin/bash
# starts the tauri debug build on a hidden Xvfb display (:77) with the dev control dir enabled.
# drive it with scripts/dev/ctl.py, screenshot with: import -display :77 -window root out.png
# usage: scripts/dev/tauri-run.sh [seconds to wait]   (state lives in $PITH_DEV_DIR, default /tmp/pith-dev)
D="${PITH_DEV_DIR:-/tmp/pith-dev}"
mkdir -p "$D/ctl"
if ! { [ -f "$D/xvfb.pid" ] && kill -0 "$(cat "$D/xvfb.pid")" 2>/dev/null; }; then
  setsid Xvfb :77 -screen 0 1480x940x24 >/dev/null 2>&1 & echo $! > "$D/xvfb.pid"; sleep 1.5
fi
if [ -f "$D/tauri.pid" ]; then kill "$(cat "$D/tauri.pid")" 2>/dev/null; sleep 1; fi
rm -f "$D"/ctl/*
cd "$(dirname "$0")/../../src-tauri"
env -u WAYLAND_DISPLAY DISPLAY=:77 GDK_BACKEND=x11 PITH_CTL="$D/ctl" setsid target/debug/pith > "$D/tauri.log" 2>&1 &
echo $! > "$D/tauri.pid"
sleep "${1:-4}"
