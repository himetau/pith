# handoff: the native build

pith's ui was a webview (electron, then tauri). It's native now: `native/` is a gpui-kit 0.8 app (gpui-pre 0.3.9),
and the pi underneath is [pi_agent_rust](https://github.com/Dicklesworthstone/pi_agent_rust), not node pi (see "pi" below). Why: the webview build was slow to show anything (`sys_info` ran `pi --version` on the main
thread before the first frame) and its type wasn't native on macos. The tauri build is on the `tauri` branch, electron on `electron`.

The look and features are the tauri build's, 1:1: `ui/theme.rs` reads matugen's whole palette and hyprland's gaps,
border and rounding, everything is the terminal's mono font, and css pixels go through `theme::u()` so the `scale`
setting zooms like the webview did (the kit's Root sets the rem to the theme font size every frame, so `u()` divides
by fontSize). `ui/frame.rs` draws the windows with their titles in the border; gpui paints a box's border over its
children, so a window's border is its own `edge()` layer under the legends. The tauri build's css and ts were the spec.

## Layout
- `native/src/core/`: what pith reads and runs, no ui. `pi.rs` (one `pi --mode rpc` per session), `sessions.rs`,
  `sys.rs`, `term.rs`, `bridge.rs` (the http bridge `ext/pith.js` calls), `imagegen.rs` (the images page and the agent's
  generate_image both), `web.rs` (offscreen pages), `browser.rs` (the agent's browser tool on them), `preview.rs`
  (design previews, reload on change).
- `native/src/state/`: `tab.rs` is a port of the old `tab.ts` (pi events in, transcript items out), `app.rs` is
  `Pith`, the global entity: tabs, routing pi's events to them, spawning pi, settings, the session list. `hub.rs`
  gathers the subagent runs of every open tab for the agents pane.
- `native/src/ui/`: `workspace.rs` (window, sidebar, header, keys), `chat.rs` + `transcript/` + `tools.rs`
  (transcript), `composer.rs` + `commands.rs` + `picker.rs` (typing, slash and @ menus, palette, model picker),
  `home.rs`, `panes/` (right panel: changes, files, terminal on alacritty_terminal, agents, browser, preview),
  `settings.rs` `usage.rs` `agents.rs` `images.rs` (pages), `theme.rs` (palette, fonts, appearance).
- `native/src/platform.rs`: macos menu bar, notifications, matugen's file watched for the accent.

## How it works
- First frame doesn't wait on anything: settings are read sync (a file), the session list and pi come later on
  worker threads. Opening a session paints its history off disk (`sys::session_entries`) and starts pi behind it.
- Web pages never sit in the window. `core/web.rs` keeps one offscreen page per id: on linux a WebKitGTK view in a
  `gtk::OffscreenWindow` on its own gtk thread (gpui doesn't use glib), on macos a WKWebView in a borderless window
  far off screen, driven on the main queue. The panes show snapshots of them every 350ms while visible and forward
  clicks and the wheel as DOM events. WebKitGTK aborts trying to make a gl context offscreen unless the view's
  hardware acceleration policy is Never.
- The macos side of `web.rs` and the menus has only been compiled (`cargo check --target aarch64-apple-darwin`),
  never run.

## pi
pi_agent_rust, a release binary pinned in `scripts/fetch-pi.sh` and `flake.nix` (same version, same sums). It goes in
`lib/pith/pi` beside the app (`install-desktop.sh`, the flake), or `native/target/pi/pi` for a dev build (`dev/run.sh`
fetches it); `sys::pi_bin` picks, `piPath` overrides. Its rpc is the node pi's with differences pith works around:
- `set_output_mode compact` right after start: a streaming update carries its delta only. pi can drop deltas when its
  stdout backs up, so the `*_end` events and `message_end` set each block whole. A tool call only gets its id at
  `toolcall_end`, it streams in under a stand-in key until then.
- No `get_entries`: history comes off the session file (pi writes each entry as it happens), which also carries the
  entry ids fork needs. No `clone`: `sessions::clone_file` copies the file under a new id and pi `switch_session`s to it.
  No thinking levels command: they follow from the model's `reasoning`, pi clamps what's too high.
- `agent_end` is the end of a prompt (pi works steers in before it). There's no queue in pi pith can read or take back,
  so follow-ups wait in `Tab::queue.1` and go one per run as runs end; steers go to pi straight away.
- Approvals: pi asks before any tool with effects (`ask_request`, header "approval") and its mode can't change while it
  runs, so it always asks and `Tab::on_ask` settles it by the session's mode (auto, edits, a read-only shell command,
  "allow for this session"), else shows a card. `ask` (questions) comes through the same event. Plan mode is pi's own:
  `--plan-mode` at start, `set_plan_mode` / `approve_plan` on shift+tab.
- `!!cmd` (output kept out of context) runs in pith, pi's `bash` has no way to exclude it.
- Global extensions are off (`--no-extensions`); `ext/pith.js` is the one pith loads, pith's own tools (browser,
  preview, report_findings, generate_image) as calls back into the bridge. pi keeps env vars that look like secrets
  from extensions and only lets them fetch https, so the bridge token rides in the url (`PITH_BRIDGE`) and pith sets
  `PI_HTTP_ALLOW_LOOPBACK=1`. Extension tools are capped at 60s.
- Subagents are pi's `subagent` tool (switched on with `--tools`). Each child is its own `pi --mode json` with no
  session; pi reports it on the tool call (`pi.subagent.progress.v1`, then `pi.subagent.result.v2`) and its events go
  to `<agent dir>/agent-hub/<pi pid>/<agent>-<n>.transcript.jsonl`. Opening a run replays that file, read-only.
- No `pi auth`: `imagegen::api_key` resolves keys the way pi does (auth.json, models.json, `<PROVIDER>_API_KEY`).

## Testing without touching the desktop
```sh
cd native && cargo build
dev/run.sh qa                        # your build on its own headless sway (gpu), fake model, scratch config
dev/ctl.py qa '{"type":"use a tool please"}' '{"keys":"enter"}' '{"dump":true}'
dev/shot.sh qa /tmp/x.png
dev/run.sh qa stop
```
`dev/fakemodel.py` is an openai-compatible stand-in; words in the prompt pick what it does (tool, edit, read, write,
slow, long, ponder, todo, question, delegate, browse <url>, preview <path>). `run.sh` gives the scratch agent dir a
`scout` agent for "delegate". `FAKE_PORT=<n>` gives a run its own copy. The `appearance`
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
