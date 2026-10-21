# pith

A desktop app for [pi](https://github.com/earendil-works/pi). It runs `pi --mode rpc` for every session you open and puts a proper window around it: sessions down the side, the chat in the middle, and a pane on the right with a browser the agent can drive, a live preview for designs, your git changes, files and running subagents.

Colours come from matugen and follow your wallpaper live. The font is whatever kitty uses. Gaps, borders and corners are read from hyprland, so the panes look like your other windows.

## Running it

It's a Tauri app: Rust and the system WebKitGTK, no bundled Chromium. You need a Rust toolchain and webkit2gtk-4.1.

```sh
npm install
npm run dev
```

pi has to be on your PATH (or set its path in settings). To get it in quickshell's launcher (and rofi, anything that reads .desktop files):

```sh
npm run install-desktop
```

That makes a release build, copies it to `~/.local/lib/pith` with the extension and prompts next to it, renders the icon into the hicolor theme and writes the .desktop entry. Run it again to update. The copy means switching branches in here doesn't break the launcher.

`npm run build` makes an AppImage and a .deb in `src-tauri/target/release/bundle/`.

## What's in it

- Sessions grouped by folder, search, pin, rename, trash. Several can run at once; ones in the background get an unread dot and a notification when they finish or need you.
- The chat shows tool calls as one-line rows you can open: diffs for edits, output for commands, highlighted files for reads, screenshots for the browser. Thinking folds away. Every turn ends with model, time, tokens and cost.
- Composer: `/` for commands (pi's templates, skills and extension commands, plus the app's own), `@` for files, `!cmd` to run something in the shell (`!!cmd` keeps it out of context), paste or drop images. Typing while it works steers it; alt+enter queues for after; esc stops and gives you back what was queued.
- Modes, cycled with shift+tab: **ask** asks before edits and commands, **edits** lets edits through, **plan** is read-only, **auto** never asks.
- Branch from any earlier prompt (edit it and resend), retry a reply, clone, compact, export as html.
- **web**: a real browser per session. The agent drives it through a `browser` tool (navigate, snapshot, click, type, screenshot, eval) while you watch, and you can take over.
- **preview**: shows html the agent makes and reloads when the file changes. Phone and tablet widths. Turn on *comment* and click an element to quote it into the chat.
- **changes**: git status and diffs for the session's folder, with review and security review buttons.
- **files** and **agents**: a file viewer, and the subagents from your subagent extension: running ones, and failures from the last 15 minutes. Click one to open its session, live while it runs; once it's done you can type in it to carry on yourself. A run that dies on its own (provider error, crash) gets resumed on the same session up to 3 times (`PI_SUBAGENT_AUTO_CONTINUE=0` turns that off).
- **term**: a real shell per session, started in its folder and coloured from matugen. Select output and hit *to chat* to quote it.
- `/review`, `/security-review`, `/simplify`, `/design`, `/explain`, `/init` ship with the app (`prompts/commands`). Reviews come back as cards ranked by severity, each linking to the line.
- Design sessions (ctrl+shift+n) start in your design folder with the preview open and a prompt that pushes for work that doesn't look templated.
- Images: `generate_image` for the agent, and an images page to generate or edit one directly. Uses a gemini image model on your endpoint by default. Their tokens count in usage; your endpoint doesn't say what an image costs, so set a price per image in settings if you want that counted too.
- Usage: spend in the last 5 hours, 7 days and this month against budgets you set, a 26 week heatmap, the last 30 days, and breakdowns by model, folder and session. Read straight from pi's session files.
- Agents page to write and edit the agent definitions in `~/.pi/agent/agents` and summon one into the current session.

## How it hangs together

`src-tauri/` is the backend: one pi rpc process per session (`pi.rs`), the session scanner (`sessions.rs`), and a loopback server (`bridge.rs`) that `ext/pith-bridge.ts` talks to. That extension is loaded into every pi pith starts (`-e`). It adds the browser, preview, ask_user, todo_write, report_findings and generate_image tools, and asks the app before edits and commands. Outside pith it does nothing.

The browser and preview panes are native WebKit views laid over the side pane (`webviews.rs`, `browser.rs`), so they hide whenever something floats over them. The terminal is portable-pty (`term.rs`).

`src/` is the ui, plain TypeScript and DOM. `tab.ts` turns pi's event stream into a transcript, `views/` and `panes.ts` draw it.

Settings live in `~/.config/pith/settings.json`, the matugen output in `~/.config/pith/colors.css` (template at `~/.config/matugen/templates/pith-colors.css`), generated images in `~/.local/share/pith/images`.

## Keys

ctrl+n new session · ctrl+= / ctrl+- / ctrl+0 scale · ctrl+shift+n new design · ctrl+k commands · ctrl+p find a session · ctrl+b sidebar · ctrl+\ side pane · alt+1–6 web, preview, changes, files, agents, terminal · ctrl+m model · ctrl+e effort · shift+tab mode · esc stop · ↑ last prompt

## Not there yet

- Budgets are yours to set. pi has no way to ask a provider for plan limits.
- Linux only: AppImage and .deb.
