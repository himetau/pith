# handoff: the native build (branch `gpui`)

pith's ui was a webview (electron, then tauri). It's native now: `native/` is a gpui-kit 0.8 app (gpui-pre 0.3.9),
same pi underneath. Why: the webview build was slow to show anything (`sys_info` ran `pi --version` on the main
thread before the first frame), its type wasn't native on macos, and the look was a hyprland rice that read as a web
page anywhere else. The tauri build is on the `tauri` branch, electron on `electron`.

## Layout
- `native/src/core/`: what pith reads and runs, no ui. `pi.rs` (one `pi --mode rpc` per session), `sessions.rs`,
  `sys.rs`, `term.rs`, `bridge.rs` (the extension's http bridge), `imagegen.rs` came over from the tauri build. New: `web.rs`
  (offscreen pages), `browser.rs` (the agent's browser tool on them), `preview.rs` (design previews, reload on change).
- `native/src/state/`: `tab.rs` is a port of the old `tab.ts` (pi events in, transcript items out), `app.rs` is
  `Pith`, the global entity: tabs, routing pi's events to them, spawning pi, settings, the session list.
- `native/src/ui/`: `workspace.rs` (window, sidebar, header, keys), `chat.rs` + `transcript/` + `tools.rs`
  (transcript), `composer.rs` + `commands.rs` + `picker.rs` (typing, slash and @ menus, palette, model picker),
  `home.rs`, `panes/` (right panel: changes, files, terminal on alacritty_terminal, agents, browser, preview),
  `settings.rs` `usage.rs` `agents.rs` `images.rs` (pages), `theme.rs` (palette, fonts, appearance).
- `native/src/platform.rs`: macos menu bar, notifications, matugen's file watched for the accent.

## How it works
- First frame doesn't wait on anything: settings are read sync (a file), the session list and pi come later on
  worker threads. Opening a session paints its history off disk (`sys::agent_session`) and starts pi behind it;
  pi's own `get_entries` replaces it once it's up.
- Web pages never sit in the window. `core/web.rs` keeps one offscreen page per id: on linux a WebKitGTK view in a
  `gtk::OffscreenWindow` on its own gtk thread (gpui doesn't use glib), on macos a WKWebView in a borderless window
  far off screen, driven on the main queue. The panes show snapshots of them every 350ms while visible and forward
  clicks and the wheel as DOM events. WebKitGTK aborts trying to make a gl context offscreen unless the view's
  hardware acceleration policy is Never.
- The macos side of `web.rs` and the menus has only been compiled (`cargo check --target aarch64-apple-darwin`),
  never run.

## Testing without touching the desktop
```sh
cd native && cargo build
dev/run.sh qa                        # your build on its own headless sway (gpu), fake model, scratch config
dev/ctl.py qa '{"type":"use a tool please"}' '{"keys":"enter"}' '{"dump":true}'
dev/shot.sh qa /tmp/x.png
dev/run.sh qa stop
```
`dev/fakemodel.py` is an openai-compatible stand-in; words in the prompt pick what it does (tool, edit, read, write,
slow, long, ponder, browse <url>, preview <path>). `FAKE_PORT=<n>` gives a run its own copy. The `appearance`
setting (system/light/dark) is how to see light mode, the headless sway is always dark.

macos type-check from linux: stub `CC`/`AR` for aarch64-apple-darwin, and an empty
`target/mac/aarch64-apple-darwin/debug/build/gpui-pre-apple-*/out/stitched_shaders.metal`, which gpui's build script
only writes on a mac host.

## Not done / not checked
- Never run on a mac. Never run under a real Wayland or X11 session either, only headless sway.
- Real clipboard paste and drag from a file manager were tested through dev hooks only.
- Typing into the browser/preview pane (clicks and scrolling go through, keys don't).
- IME and dead keys in the terminal.

## Conventions
Commits are lowercase and casual, in the user's voice. The repo's local git config has the identity (himetau,
GitHub noreply address); never the proton one. Comments are short and lowercase, explaining why.
