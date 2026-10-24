#!/bin/bash
# fetches the pi pith runs (pi_agent_rust, the native port) into a folder, checked against the release's sha256.
#   scripts/fetch-pi.sh <dir>           puts <dir>/pi there, skips the download when it's already this version
#   scripts/fetch-pi.sh <dir> --build   builds it from the same tag instead, without the bpe token tables and the
#                                       second tui: ~40MB less per session once it has answered (it estimates tokens
#                                       as bytes/4 until the provider reports real usage). needs rustup; pi's
#                                       rust-toolchain.toml pulls its pinned nightly, and the first build takes a while
# the version and sums are pinned here; flake.nix pins the same release
set -e
VERSION=0.8.0
dir=${1:?usage: fetch-pi.sh <dir> [--build]}
mkdir -p "$dir"

if [ "$2" = --build ]; then
  [ -x "$dir/pi" ] && [ "$(cat "$dir/.pi-build" 2>/dev/null)" = "$VERSION" ] && exit 0
  src="${XDG_CACHE_HOME:-$HOME/.cache}/pith/pi_agent_rust"
  if [ -d "$src/.git" ]; then git -C "$src" fetch -q --depth 1 origin tag "v$VERSION"
  else git clone -q --depth 1 --branch "v$VERSION" https://github.com/Dicklesworthstone/pi_agent_rust "$src"; fi
  git -C "$src" checkout -q "v$VERSION"
  (cd "$src" && cargo build --profile dist --no-default-features --features sqlite-sessions,tui --bin pi)
  install -m755 "$src/target/dist/pi" "$dir/.pi.new" && mv -f "$dir/.pi.new" "$dir/pi"
  echo "$VERSION" > "$dir/.pi-build"
  echo "pi $VERSION (built) -> $dir/pi"
  exit 0
fi

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)               asset=pi-linux-amd64;  sum=c04e7f87391becb17cb1ec7544d0fd767231dfb5a8361644ea8870ba3d39056d ;;
  Linux-aarch64|Linux-arm64)  asset=pi-linux-arm64;  sum=3276504d19554d7aa8af22703674186b9887c48cffc1cd1238b7bf523100abf9 ;;
  Darwin-arm64)               asset=pi-darwin-arm64; sum=9999ee5b05283a79b277162f739df9ff36b2ff0f8dca6cf1d6cc4e1aae5d05d9 ;;
  *) echo "no pi_agent_rust build for $(uname -sm)" >&2; exit 1 ;;
esac
# a built one of this version stays, it's the same pi with less in it
[ -x "$dir/pi" ] && [ "$(cat "$dir/.pi-build" 2>/dev/null)" = "$VERSION" ] && exit 0
[ -x "$dir/pi" ] && "$dir/pi" --version 2>/dev/null | grep -q "^pi $VERSION " && exit 0

tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
curl -fsSL "https://github.com/Dicklesworthstone/pi_agent_rust/releases/download/v$VERSION/$asset.tar.xz" -o "$tmp/pi.tar.xz"
echo "$sum  $tmp/pi.tar.xz" | sha256sum -c --quiet - 2>/dev/null || { echo "$sum  $tmp/pi.tar.xz" | shasum -a 256 -c --quiet -; }
tar -xJf "$tmp/pi.tar.xz" -C "$tmp" pi
# copy then rename, so a pi that's running keeps its old binary
install -m755 "$tmp/pi" "$dir/.pi.new" && mv -f "$dir/.pi.new" "$dir/pi"
rm -f "$dir/.pi-build"
echo "pi $VERSION -> $dir/pi"
