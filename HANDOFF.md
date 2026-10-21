# handoff: the tauri port (branch `tauri`)

`main` is the old Electron build. This branch is pith on Tauri 2: Rust in `src-tauri/`, the same ui in `src/`, the system WebKitGTK instead of a bundled Chromium. Electron is gone from this branch.

The launcher runs the Tauri build now: `npm run install-desktop` builds a release binary and copies it to `~/.local/lib/pith/` with `ext/` and `prompts/` beside it, so switching branches in the repo doesn't touch it.

## What was wrong, and is fixed
- **eval into webviews never ran, and it idled hot.** `watch_dir` in `lib.rs` fired on every notify event, Access included, and the handler reads the file it watches, so it fed itself and starved the main thread. Only Create/Modify count now. Idle on a real (headless sway) Wayland session: ~0.7% CPU total.
- child webviews: tauri's `add_child` packs into a vertical GtkBox on linux and ignores position/size, so `webviews.rs` moves them into a GtkOverlay over the main webview and places them through `get-child-position`.
- sidebar showing too few sessions: `read_to_string` failed on non-UTF-8 files; it reads lossily now. It matches what's on disk.
- composer focus ring: `:focus-visible` only outlines buttons now.
- terminal stayed visible over other panes: `.term-box.on` set `visibility: visible`, which beats the hidden pane. It only turns on inside `.pane.on`.
- the installed copy fell back to the repo's `ext/`: tauri's `resource_dir` isn't the exe's folder for a plain binary, so setup checks next to the exe too.

## Checked in the Tauri build (Xvfb harness + headless sway)
pi sessions end to end, the question card, the browser tool (navigate, snapshot, click; also on github.com, strict CSP), preview with live reload, terminal, the agents pane, `npx tauri build` (AppImage 103 MB, .deb 4.3 MB), Wayland app_id is `pith`.

Not checked, because it would mean touching the real desktop: drag and drop, notifications, the image clipboard.

## Memory, fresh start, idle, no session open, same headless sway
| | RSS sum | PSS sum |
|---|---|---|
| Electron | 704 MB | 251 MB |
| Tauri | 496 MB | 234 MB |

PSS counts shared libraries once, so it's the honest number: the win is small, the WebKit web process alone is ~115 MB PSS. The bigger difference is on disk (9.3 MB binary vs 284 MB unpacked Electron).

## Added on this branch
- agents pane: only running agents, and failed or aborted ones from the last 15 minutes (a kill by the user doesn't count). Failures can be dismissed. Click one to open its session as a tab: read straight off disk while it runs (`agents_session` in `sys.rs`, `Tab.watch`/`peek()`), and a normal session you can type in once it's done.
- markdown in replies is coloured like opencode: headings, bold, italics, inline code and list markers each get a theme accent (`--md-*` in `base.css`), quieter inside thinking.
- the sidebar only moves a session to the top when you send in it, not when you open it (`Tab.sent`).
- voxel icon: `packaging/voxel-icon.py` renders it at any size; the install script renders each hicolor size on its own.

## Outside the repo
The subagent extension at `~/.pi/agent/extensions/subagent` (not in git) was changed for the agents pane:
- `runner.ts`: every run keeps a session in `~/.pi/agent/subagent-sessions/` (pruned after 30 days) instead of `--no-session`. A run that fails on its own is resumed on that session with a "pick up where you left off" prompt, up to 3 times with backoff (`PI_SUBAGENT_AUTO_CONTINUE=0` turns it off, a number sets the count). A user abort is never retried.
- `store.ts`: records carry `sessionFile` and `retries`; `markRetrying()` puts a record back to `spawning` with no pid between attempts, which `read_agent_hub` in `sys.rs` knows not to mark aborted.

## Testing without opening windows on the user's screen
```sh
npm run build:ui && (cd src-tauri && cargo build)
scripts/dev/tauri-run.sh 6                       # debug build on Xvfb :77, control dir enabled
scripts/dev/ctl.py js "window.pith_.app.view"    # runs an expression in the ui
import -display :77 -window root /tmp/pith-dev/shot.png
```
`window.pith_` exposes `{ app, act, chatFor, api }`. Set `PI_CODING_AGENT_DIR` to a scratch copy of `~/.pi/agent` (auth.json, models.json, settings.json) so test sessions and agents don't land in the user's lists. For real Wayland without a window on screen: `WLR_BACKENDS=headless WLR_RENDERER=pixman sway -c <config>` and point `WAYLAND_DISPLAY` at its socket.

Kill by pid, and check the pid is yours: `setsid` behind `&&` hands back a wrapper shell's pid, not the app's.

## Conventions
Commits are lowercase and casual, in the user's voice, with `-c user.name=himetau -c user.email=168368537+himetau@users.noreply.github.com`. Comments are short and lowercase, explaining why.
