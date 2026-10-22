#!/bin/bash
# runs this checkout's native debug build on its own headless sway, against a fake model and a scratch pi + pith config,
# so nothing touches your screen, your sessions or your keys.
#   native/dev/run.sh [name]          (re)start the app. name keeps parallel runs apart, default "dev"
#   native/dev/run.sh [name] stop     stop the app and its sway
#   native/dev/shot.sh [name] out.png screenshot
#   native/dev/ctl.py [name] '<json>' drive it, see src/dev.rs for the commands
# state lives in $PITH_DEV_DIR/<name> (default /tmp/pith-native-dev). the fake model (dev/fakemodel.py) is shared, port 18734;
# FAKE_PORT=<n> starts (or reuses) one of your own, for when you've changed its script
NAME=${1:-dev}
HERE=$(cd "$(dirname "$0")/.." && pwd)
ST=${PITH_DEV_DIR:-/tmp/pith-native-dev}/$NAME
# wayland sockets have a short path limit, so the runtime dir can't live under $ST
RT=/tmp/pn-$NAME
mkdir -p "$ST/ctl" "$ST/agent/sessions" "$ST/xdg/config/pith" "$ST/xdg/data" "$ST/work" "$RT"
chmod 700 "$RT"

stop_app() { [ -f "$ST/app.pid" ] && kill "$(cat "$ST/app.pid")" 2>/dev/null; rm -f "$ST/app.pid"; }
if [ "$2" = stop ]; then
  stop_app
  [ -f "$ST/sway.pid" ] && kill "$(cat "$ST/sway.pid")" 2>/dev/null; rm -f "$ST/sway.pid"
  exit 0
fi

if ! { [ -f "$ST/sway.pid" ] && kill -0 "$(cat "$ST/sway.pid")" 2>/dev/null; }; then
  printf 'output HEADLESS-1 resolution %s scale 1\ndefault_border none\n' "${SIZE:-1480x940}" > "$ST/sway.conf"
  rm -f "$RT"/wayland-* "$RT"/sway-ipc*
  XDG_RUNTIME_DIR=$RT WLR_BACKENDS=headless WLR_RENDERER=gles2 WLR_LIBINPUT_NO_DEVICES=1 setsid sway -c "$ST/sway.conf" > "$ST/sway.log" 2>&1 &
  echo $! > "$ST/sway.pid"
  for _ in $(seq 1 50); do ls "$RT"/wayland-* >/dev/null 2>&1 && break; sleep 0.1; done
fi
PORT=${FAKE_PORT:-18734}
if ! curl -s -m 1 http://127.0.0.1:$PORT/v1/models >/dev/null; then
  setsid python3 -I "$HERE/dev/fakemodel.py" $PORT > /tmp/pith-native-fakemodel-$PORT.log 2>&1 &
  sleep 0.4
fi
cat > "$ST/agent/models.json" <<EOF
{ "providers": { "fake": { "baseUrl": "http://127.0.0.1:$PORT/v1", "api": "openai-completions", "apiKey": "fake", "models": [
  { "id": "fake-1", "name": "Fake One", "reasoning": true, "input": ["text"], "contextWindow": 200000, "maxTokens": 8000, "cost": { "input": 1, "output": 5, "cacheRead": 0.1, "cacheWrite": 1 } },
  { "id": "fake-2", "name": "Fake Two", "reasoning": false, "input": ["text", "image"], "contextWindow": 128000, "maxTokens": 8000, "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 } } ] } } }
EOF
echo '{ "defaultProvider": "fake", "defaultModel": "fake-1", "lastChangelogVersion": "1.3.0" }' > "$ST/agent/settings.json"
[ -f "$ST/xdg/config/pith/settings.json" ] || echo "{ \"defaultCwd\": \"$ST/work\", \"mode\": \"auto\" }" > "$ST/xdg/config/pith/settings.json"
[ -f "$ST/work/notes.txt" ] || { echo "hello" > "$ST/work/notes.txt"; git -C "$ST/work" init -q 2>/dev/null; }

stop_app
rm -f "$ST"/ctl/*
cd "$HERE" || exit 1
env XDG_RUNTIME_DIR="$RT" WAYLAND_DISPLAY=wayland-1 RUST_LOG=${RUST_LOG:-warn} PITH_CTL="$ST/ctl" \
  PI_CODING_AGENT_DIR="$ST/agent" PI_OFFLINE=1 XDG_CONFIG_HOME="$ST/xdg/config" XDG_DATA_HOME="$ST/xdg/data" \
  setsid "./target/${BUILD:-debug}/pith" > "$ST/app.log" 2>&1 &
echo $! > "$ST/app.pid"
# ready once the control dir answers
for _ in $(seq 1 60); do python3 -I "$HERE/dev/ctl.py" "$NAME" '{"dump":true}' 2>/dev/null | grep -q '"ok":true' && exit 0; sleep 0.1; done
echo "app didn't come up, see $ST/app.log" >&2; exit 1
