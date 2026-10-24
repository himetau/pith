# pith

a desktop app for [pi](https://github.com/earendil-works/pi), running
[pi_agent_rust](https://github.com/Dicklesworthstone/pi_agent_rust), the native
port. every session you open is its own `pi --mode rpc`, and pith puts a real window around it: sessions
down the side, chat in the middle, and a panel on the right with your git
changes, files, a terminal, running subagents, the browser the agent drives and
a live preview for designs.

its native all the way down now. the ui is drawn with [gpui](https://gpui.rs)
(zeds ui framework) through gpui-kit, not a webview, and the agent is a rust
binary, not node. pith is up in about 300ms, a session's pi in about 25ms, and
the whole thing idles at a fraction of the old electron build's memory. web pages (the agents browser, design previews) render offscreen in the
platforms own webkit and show up in the panel as live snapshots, so theres no
webview in the window at all.

it looks like the rest of my desktop: colours come from matugen
(`~/.config/pith/colors.css`) and follow the wallpaper live, the font is whatever
kitty uses, and gaps, borders and corners are read from hyprland, so the panes
look like your other windows, titles cut into their borders like a tui.

## running it

you need a rust toolchain, and on linux webkit2gtk-4.1 and vulkan. pith runs the
pi_agent_rust release pinned in `scripts/fetch-pi.sh` (checked against its sha256),
which the dev scripts and the installer fetch for you; `piPath` in the settings
points it at another one.

```sh
cd native && cargo run --release   # try it
scripts/install-desktop.sh          # build it and put it in the launcher
```

`install-desktop.sh` copies the release build to `~/.local/lib/pith` with pi, the
icon and a .desktop entry, so quickshell, rofi and anything else that reads those
can open it. run it again to update.

or with nix, on linux or an apple silicon mac. it brings the same pi along:

```sh
nix run github:himetau/pith
```

the old builds live on in branches: tauri on `tauri`, electron on `electron`, both
on the node pi.

## what it does

- several sessions at once, background ones ping you when theyre done or stuck
- subagents run in their own processes, live in the agents pane, each one opens as its own transcript
- tool calls fold into one line rows: diffs, command output, todo lists, images
- `/` commands, `@` files, `!cmd` for the shell, paste or drop images, steer it while it works
- ask / edits / plan / auto modes on shift+tab (plan is pi's own plan mode), ctrl+k for everything else, alt+1-6 for the panes
- `/review`, `/security-review`, `/design` and friends ship with it
- image generation, and a usage page that reads spend straight off pis session files

settings are in `~/.config/pith/settings.json` (or the settings page).

## hacking on it

`native/dev/run.sh` runs your build on its own headless sway against a fake model
and a scratch config, so nothing touches your screen, sessions or keys.
`native/dev/ctl.py` drives it (type, keys, clicks, dump the state) and
`native/dev/shot.sh` screenshots it. see `native/src/dev.rs` for the commands.
