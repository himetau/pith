#!/bin/bash
# shot.sh [name] out.png: screenshot of the headless sway native/dev/run.sh started
if [ $# -ge 2 ]; then NAME=$1; OUT=$2; else NAME=dev; OUT=$1; fi
XDG_RUNTIME_DIR=/tmp/pn-$NAME WAYLAND_DISPLAY=wayland-1 grim "$OUT"
