#!/bin/bash
# puts pith in app launchers (quickshell, rofi): the release build, an icon in the hicolor theme and a .desktop entry.
# the binary gets copied out of the repo with ext/, prompts/ and pi (pi_agent_rust) next to it, where pith looks for them,
# so switching branches or cleaning target/ doesn't take the launcher down. it builds the release first.
#   scripts/install-desktop.sh [--build]   --build builds a leaner pi instead of fetching it, see fetch-pi.sh
set -e
repo="$(cd "$(dirname "$0")/.." && pwd)"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
lib="$HOME/.local/lib/pith"
(cd "$repo/native" && cargo build --release)
bin="$repo/native/target/release/pith"

mkdir -p "$lib"
# copy then rename, so a pith that's running keeps its old binary instead of failing with "text file busy"
cp "$bin" "$lib/.pith.new" && mv -f "$lib/.pith.new" "$lib/pith"
rm -rf "$lib/ext" "$lib/prompts"
cp -r "$repo/ext" "$repo/prompts" "$lib/"
"$repo/scripts/fetch-pi.sh" "$lib" "$@"

# launchers look icons up by name, so it goes in the theme instead of being referenced by path.
# each size is rendered on its own, a voxel icon scaled down from 1024 goes to mush
for s in 16 32 48 64 128 256 512; do
  mkdir -p "$data/icons/hicolor/${s}x${s}/apps"
  python3 "$repo/packaging/voxel-icon.py" "$data/icons/hicolor/${s}x${s}/apps/.pith.png" $s >/dev/null
  mv -f "$data/icons/hicolor/${s}x${s}/apps/.pith.png" "$data/icons/hicolor/${s}x${s}/apps/pith.png"
done
# the old icon was an svg, and scalable wins over the pngs
rm -f "$data/icons/hicolor/scalable/apps/pith.svg"

mkdir -p "$data/applications"
# written whole then renamed: launchers watching the folder can catch a half-written file and drop the entry
sed "s|^Exec=.*|Exec=$lib/pith|" "$repo/packaging/pith.desktop" > "$data/applications/.pith.desktop.new"
mv -f "$data/applications/.pith.desktop.new" "$data/applications/pith.desktop"

command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t "$data/icons/hicolor" || true
command -v update-desktop-database >/dev/null && update-desktop-database -q "$data/applications" || true
echo "installed $data/applications/pith.desktop -> $lib/pith"
