# pith

a desktop app for [pi](https://github.com/earendil-works/pi). every session you
open is its own `pi --mode rpc`, and pith puts a real window around it: sessions
down the side, chat in the middle, and a pane on the right with a browser the
agent drives, a live preview for designs, your git changes, files, a terminal and
whatever subagents are running.

it themes itself off matugen so it follows the wallpaper live, takes its font
from kitty and its gaps, borders and corners from hyprland, so it looks like the
rest of my desktop instead of an electron app. which it isnt anymore, its tauri
on the system webkitgtk now and runs at the monitors refresh rate.

## running it

you need pi on your PATH, a rust toolchain and webkit2gtk-4.1.

```sh
npm install
npm run dev               # try it
npm run install-desktop   # build it and put it in the launcher
```

`install-desktop` copies a release build to `~/.local/lib/pith` with the icon and
a .desktop entry, so quickshell, rofi and anything else that reads those can open
it. run it again to update. `npm run build` gives you an appimage and a .deb.

or with nix, which brings pi along too (a pi already on your PATH wins):

```sh
nix run github:himetau/pith
```

## what it does

- several sessions at once, background ones ping you when theyre done or stuck
- tool calls fold into one line rows: diffs, command output, screenshots
- `/` commands, `@` files, `!cmd` for the shell, steer it while it works
- ask / edits / plan / auto modes on shift+tab
- `/review`, `/security-review`, `/design` and friends ship with it
- image generation, and a usage page that reads spend straight off pis session
  files

settings are in `~/.config/pith/settings.json`. its built for my setup (arch,
hyprland, matugen) so expect rough edges anywhere else.
