# handoff: the tauri port (branch `tauri`)

`main` is the working Electron build (commit 14b0ec8 and before). This branch ports it to Tauri 2 so it stops shipping Chromium: Electron idles at ~765 MB before any pi session is open. The UI (`src/`) is the same code; the backend is new Rust in `src-tauri/`.

## Where it stands

Builds and typechecks clean (`cd src-tauri && cargo build`, `npx tsc --noEmit`). It launches, and the home screen renders right: font, matugen colours, hyprland borders and legends. Nothing past that has been checked in the Tauri build yet.

### Done
- `src-tauri/src/`
  - `pi.rs`: pi rpc processes, JSONL, responses matched by id
  - `sessions.rs`: session scan and usage report, ported from `electron/sessions.ts`
  - `sys.rs`: settings, theme, kitty font, hyprctl, git, files, agents, hub, images, per-session meta
  - `bridge.rs`: tiny_http loopback server, same protocol as before, so `ext/pith-bridge.ts` didn't change
  - `term.rs`: portable-pty, which replaces `electron/shell-pty.py`
  - `imagegen.rs`: the images page's generator
  - `webviews.rs` + `browser.rs`: the browser and preview panes are native child webviews (`window.add_child`, needs tauri's `unstable` feature) laid over the side pane
  - `lib.rs`: every command and the window setup
- `src/lib/api.ts` is now a Tauri adapter with the same `Api` shape as the old preload, so the views barely changed.
- The panes report their rect with `api.webLayout(slots)`: `Side.layout()` in `src/panes.ts`. Anything not in the list gets hidden, and so does everything while an overlay (palette, menu, dialog) is open, because native webviews sit on top of HTML.
- Preview element picks come back through the page title (`pith-pick:{json}` caught by `on_document_title_changed`), so no IPC is needed in pages we don't own.

### Blocking bug: JS evaluation into webviews never comes back
`eval_json` in `webviews.rs` is how the agent's browser tool and the dev harness run JS in a page. Every way of evaluating was tried against the main webview, and none ever executes or calls back:
- Tauri `eval()`: `document.body.style.background='red'` changes nothing
- `eval_with_callback`
- WebKit's `evaluate_javascript`
- WebKit's `call_async_javascript_function`

`with_webview` itself does run: the closure sees uri `tauri://localhost`. Meanwhile IPC from the page works, since the UI loads its sessions through `invoke`.

Measured: idle, the WebKitWebProcess sits at ~110% CPU and 580–950 MB RSS, and pith's own process at ~260% CPU, with only ~15 `web_layout` calls in 6 s. So it isn't a call flood. Things that made no difference:
- CSP set to null
- `WEBKIT_DISABLE_COMPOSITING_MODE=1`

Everything so far ran under Xvfb (`GDK_BACKEND=x11`, software GL, "MESA-EGL DRI3 error" in the log), so first rule out the test display:
1. Run `target/debug/pith` normally on Wayland and watch `ps` CPU for the pith process and its WebKitWebProcess. If it idles quietly there, Xvfb was the problem; test on the real display (or a headless sway).
2. If it still spins, find what's hot: `perf top -p <pid>`, or `gdb -p <pid>` then `thread apply all bt`. Main-thread starvation would explain both the CPU and the eval callbacks never being dispatched. Suspects:
   - the resize loop between `auto_resize()` on the main webview and the child webviews
   - the `notify` watchers in `lib.rs` (`watch_dir`, `dev_control`)
   - the ResizeObserver/MutationObserver pair in `Side`
3. Once evaluation works, check that `eval_json`'s park-on-window-and-poll trick works on other sites' pages (strict CSP sites such as github.com). The browser tool depends on it.

### Also found, not fixed
- The sidebar showed 1 of 5 sessions in the Tauri build. Probably a bug in `sessions.rs` `scan()`/`parse()`: check `read_to_string` on non-UTF-8, `parse_iso`, and the cache retain. The Electron build lists all 5.
- WebKit draws a focus ring on the composer textarea (`:focus-visible` in `src/styles/base.css`). Turn it off for `.ta`, `.sb-search`, `.pk-in`, `.url`.

### Not tested at all yet
- pi sessions end to end
- permission and question cards
- the browser tool (navigate, snapshot, click, type, screenshot via WebKit `snapshot`)
- preview and its live reload
- terminal
- drag and drop (`api.onDrop`)
- notifications, image clipboard
- `npx tauri build` (AppImage and deb; resources map `../ext` → `ext`, `../prompts` → `prompts`)
- the Wayland app_id for `StartupWMClass` (`glib::set_prgname("pith")` is set, unverified)

### After it works
- Delete `electron/`, `scripts/build-main.mjs`, the electron and electron-builder deps and the electron `build` block in `package.json`. Add `tauri:dev` / `tauri:build` scripts.
- Point `scripts/install-desktop.sh` at the Tauri binary, and update the README.
- Measure idle memory on the real display against Electron's ~765 MB. That was the point of the port.
- The user asked for a new app icon: **voxel style, not slopped**. Not started. Tauri wants `src-tauri/icons/{32x32,128x128,128x128@2x,icon}.png`; `npx tauri icon <1024px png>` makes the set. Right now those are copies of `packaging/icon.svg`.

## Testing without opening windows on the user's screen
```sh
cd src-tauri && cargo build && cd ..
scripts/dev/tauri-run.sh 6                       # debug build on Xvfb :77, control dir enabled
scripts/dev/ctl.py js "window.pith_.app.view"    # runs an expression in the ui (needs eval to work, see above)
import -display :77 -window root /tmp/pith-dev/shot.png
```
`window.pith_` exposes `{ app, act, chatFor, api }` for poking at state. Stop with `kill $(cat /tmp/pith-dev/tauri.pid) $(cat /tmp/pith-dev/xvfb.pid)`.

Be careful with `pkill -f`: it matches your own shell command line. Kill by pid file. And never kill all `pi --mode rpc` processes: the user runs pith themselves, and those are their sessions.

## Conventions
Commits are lowercase and casual, in the user's voice. There's no git identity in this environment, so commit with `-c user.name=himetau -c user.email=168368537+himetau@users.noreply.github.com`. Comments are short and lowercase, explaining why, like the rest of the code.
