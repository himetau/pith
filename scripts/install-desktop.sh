#!/bin/bash
# puts pith in app launchers (quickshell, rofi): an icon in the hicolor theme and a .desktop entry.
# it runs the repo's build, so `npm run build` is all an update needs. --appimage runs release/pith.AppImage instead
set -e
repo="$(cd "$(dirname "$0")/.." && pwd)"
data="${XDG_DATA_HOME:-$HOME/.local/share}"

# launchers look icons up by name, so it goes in the theme instead of being referenced by path
for s in 16 32 48 64 128 256 512; do
  mkdir -p "$data/icons/hicolor/${s}x${s}/apps"
  rsvg-convert -w $s -h $s "$repo/packaging/icon.svg" -o "$data/icons/hicolor/${s}x${s}/apps/pith.png"
done
mkdir -p "$data/icons/hicolor/scalable/apps"
cp "$repo/packaging/icon.svg" "$data/icons/hicolor/scalable/apps/pith.svg"

if [ "$1" = "--appimage" ]; then exec_line="$repo/release/pith.AppImage"
else exec_line="$repo/node_modules/electron/dist/electron $repo"; fi

mkdir -p "$data/applications"
sed "s|^Exec=.*|Exec=$exec_line|" "$repo/packaging/pith.desktop" > "$data/applications/pith.desktop"

command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t "$data/icons/hicolor" || true
command -v update-desktop-database >/dev/null && update-desktop-database -q "$data/applications" || true
echo "installed $data/applications/pith.desktop -> $exec_line"
