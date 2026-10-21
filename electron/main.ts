// Electron main. Starts pi processes for the renderer, serves the bridge their extension calls,
// and watches the files the ui follows live (matugen colours, subagent state, previewed designs)
import { app, BrowserWindow, clipboard, ClipboardItem, dialog, ipcMain, nativeImage, Notification, shell, type WebContents } from "electron";
import { existsSync, mkdirSync, readFileSync, rmSync, watch, writeFileSync, type FSWatcher } from "node:fs";
import { dirname, join } from "node:path";
import { PiProc, type Rec } from "./pi";
import { startBridge } from "./bridge";
import { attachBrowser, detachBrowser, runBrowser } from "./browser";
import { agentDir, listSessions, usageReport } from "./sessions";
import { agentDefs, configDir, dataDir, gitBranch, gitDiff, gitStatus, hyprLook, imagesDir, listFiles, listImages, loadMeta, loadSettings, piVersion, readAgentHub, readAny, readTheme, saveAgent, saveMeta, saveSettings, terminalFont, type SessionMeta, type Settings } from "./sys";
import { generateImage } from "../ext/imagegen";

const root = join(__dirname, "..", "..");
const ext = join(root, "ext", "pith-bridge.ts");
const prompts = join(root, "prompts");
const dev = !!process.env.PITH_DEV;

app.setName("pith");
// chromium's caches and the browser pane's logins go with the data, ~/.config/pith is only settings and colours
app.setPath("userData", join(dataDir, "electron"));
app.commandLine.appendSwitch("enable-features", "WaylandWindowDecorations");

let win: BrowserWindow | null = null;
const procs = new Map<string, PiProc>();
const modes = new Map<string, string>();
const send = (ch: string, ...a: unknown[]) => { if (win && !win.isDestroyed()) win.webContents.send(ch, ...a); };

let bridge = { url: "", token: "" };

async function bridgeCall(tab: string, op: string, args: any) {
  switch (op) {
    case "mode":
      return { mode: modes.get(tab) ?? loadSettings().mode };
    case "browser":
      return runBrowser(tab, args, () => send("pane:open", tab, "browser"), (url) => send("browser:url", tab, url));
    case "preview":
      send("preview:open", tab, args);
      return {};
    case "image-settings": {
      const s = loadSettings();
      return { model: s.imageModel, provider: s.imageProvider, dir: imagesDir, pi: s.piPath };
    }
  }
  throw new Error(`unknown op ${op}`);
}

type StartOpts = { tab: string; cwd: string; session?: string; kind?: "chat" | "design"; model?: string; thinking?: string; mode?: string; name?: string };

function start(o: StartOpts) {
  procs.get(o.tab)?.stop();
  const s = loadSettings();
  if (o.mode) modes.set(o.tab, o.mode);
  const args = ["-e", ext, "--append-system-prompt", join(prompts, "pith.md"), "--prompt-template", join(prompts, "commands")];
  if (o.kind === "design") args.push("--append-system-prompt", join(prompts, "design-system.md"));
  if (o.session) args.push("--session", o.session);
  if (o.model) args.push("--model", o.model);
  if (o.thinking) args.push("--thinking", o.thinking);
  if (o.name) args.push("--name", o.name);
  if (s.extraArgs.trim()) args.push(...(s.extraArgs.match(/"[^"]*"|\S+/g) ?? []).map((a) => a.replace(/^"|"$/g, "")));
  if (!existsSync(o.cwd)) mkdirSync(o.cwd, { recursive: true });
  const env = { ...process.env, PITH_BRIDGE: bridge.url, PITH_TOKEN: bridge.token, PITH_TAB: o.tab };
  const p = new PiProc(s.piPath, args, o.cwd, env, (r: Rec) => send("pi:event", o.tab, r), (code, stderr) => {
    // a replaced or stopped process exiting isn't news
    if (procs.get(o.tab) !== p) return;
    procs.delete(o.tab);
    send("pi:exit", o.tab, code, stderr);
  });
  procs.set(o.tab, p);
  return p;
}

function createWindow() {
  win = new BrowserWindow({
    width: 1480, height: 940, minWidth: 760, minHeight: 480,
    frame: false,
    show: !process.env.PITH_HIDDEN,
    backgroundColor: (readTheme().css.match(/--surface-lowest:\s*(#[0-9a-f]+)/i) ?? [])[1] ?? "#140c0d",
    title: "pith",
    icon: nativeImage.createFromPath(join(root, "packaging", "icon.png")),
    webPreferences: { preload: join(__dirname, "preload.cjs"), contextIsolation: true, sandbox: false, webviewTag: true, spellcheck: true, backgroundThrottling: !process.env.PITH_HIDDEN, offscreen: !!process.env.PITH_OSR },
  });
  win.webContents.setWindowOpenHandler(({ url }) => { void shell.openExternal(url); return { action: "deny" }; });
  win.webContents.on("will-navigate", (e, url) => { if (!url.startsWith("file:") && !url.startsWith("http://localhost:5173")) { e.preventDefault(); void shell.openExternal(url); } });
  // the two kinds of webview: the agent's browser and the design preview. only the preview gets the picker preload
  win.webContents.on("will-attach-webview", (_e, prefs, params) => {
    delete (prefs as any).preloadURL;
    prefs.nodeIntegration = false;
    prefs.contextIsolation = true;
    prefs.preload = params.partition === "pith-preview" ? join(__dirname, "pick-preload.cjs") : undefined;
  });
  win.webContents.on("did-attach-webview", (_e, wc: WebContents) => {
    wc.setWindowOpenHandler(({ url }) => { wc.loadURL(url).catch(() => {}); return { action: "deny" }; });
  });
  if (process.env.PITH_CTL) devControl(win, process.env.PITH_CTL);
  if (dev) void win.loadURL("http://localhost:5173");
  else void win.loadFile(join(root, "dist", "ui", "index.html"));
  win.on("closed", () => { win = null; });
}

// dev only: drop {id, js} or {id, shot} json files in $PITH_CTL to drive a hidden window from scripts
function devControl(w: BrowserWindow, dir: string) {
  mkdirSync(dir, { recursive: true });
  watch(dir, async (_e, f) => {
    if (!f || !String(f).endsWith(".cmd.json")) return;
    const file = join(dir, String(f));
    let cmd: { js?: string; shot?: string; size?: [number, number] };
    try { cmd = JSON.parse(readFileSync(file, "utf8")); rmSync(file); } catch { return; }
    const out = file.replace(".cmd.json", ".out.json");
    try {
      if (cmd.size) w.setSize(cmd.size[0], cmd.size[1]);
      if (cmd.js) writeFileSync(out, JSON.stringify({ ok: true, value: await w.webContents.executeJavaScript(cmd.js, true) }));
      if (cmd.shot) {
        // a hidden window stops painting, ask for a frame first or capturePage waits forever
        void w.webContents.executeJavaScript(`(() => { const d = document.createElement("i"); d.style.cssText = "position:fixed;left:0;top:0;width:1px;height:1px;opacity:.01"; document.body.append(d); let n = 0; const f = () => { d.style.left = (n++ % 2) + "px"; if (n < 90) requestAnimationFrame(f); else d.remove(); }; f(); })()`);
        w.webContents.invalidate();
        await new Promise((r) => setTimeout(r, 200));
        const img = await Promise.race([w.webContents.capturePage(undefined, { stayHidden: true }), new Promise<never>((_, rej) => setTimeout(() => rej(new Error("capture timed out")), 8000))]); writeFileSync(cmd.shot, img.toPNG());
        writeFileSync(out, JSON.stringify({ ok: true }));
      }
    } catch (e) { writeFileSync(out, JSON.stringify({ ok: false, error: String(e) })); }
  });
}

// ---------- watchers ----------
function watchDebounced(dir: string, test: (f: string) => boolean, fire: () => void): FSWatcher | null {
  let t: NodeJS.Timeout | null = null;
  try {
    mkdirSync(dir, { recursive: true });
    return watch(dir, (_ev, f) => {
      if (!f || !test(String(f))) return;
      if (t) clearTimeout(t);
      t = setTimeout(fire, 120);
    });
  } catch { return null; }
}

const previewWatch = new Map<string, FSWatcher>();

// ---------- ipc ----------
const h = (ch: string, fn: (...a: any[]) => unknown) => ipcMain.handle(ch, (_e, ...a) => fn(...a));

h("pi:start", async (o: StartOpts) => {
  const p = start(o);
  const st = await p.send({ type: "get_state" });
  if (!st.success) return { ok: false, error: p.stderr.trim() || st.error };
  return { ok: true, state: st.data, pid: p.pid };
});
h("pi:cmd", (tab: string, cmd: Rec) => procs.get(tab)?.send(cmd) ?? { type: "response", success: false, error: "this session isn't running" });
h("pi:ui", (tab: string, id: string, payload: object) => procs.get(tab)?.uiResponse(id, payload));
h("pi:stop", (tab: string) => { procs.get(tab)?.stop(); procs.delete(tab); detachBrowser(tab); });
h("pi:mode", (tab: string, mode: string) => { modes.set(tab, mode); });
h("pi:live", () => [...procs.keys()]);

h("sessions:list", () => listSessions());
h("meta:all", () => loadMeta());
h("meta:set", (file: string, patch: SessionMeta) => saveMeta(file, patch));
h("sessions:trash", async (file: string) => { await shell.trashItem(file); });
h("usage:get", () => usageReport());

h("settings:get", () => loadSettings());
h("settings:set", (patch: Partial<Settings>) => saveSettings(patch));
h("sys:info", () => {
  const s = loadSettings();
  return { home: app.getPath("home"), piVersion: piVersion(s.piPath), font: terminalFont(), hypr: hyprLook(), theme: readTheme(), imagesDir, agentDir, configDir, platform: process.platform };
});

h("git:status", (cwd: string) => gitStatus(cwd));
h("git:diff", (cwd: string, file: string, untracked: boolean) => gitDiff(cwd, file, untracked));
h("git:branch", (cwd: string) => gitBranch(cwd));
h("fs:list", (cwd: string) => listFiles(cwd));
h("fs:read", (path: string) => readAny(path));
h("fs:write", (path: string, text: string) => { mkdirSync(dirname(path), { recursive: true }); writeFileSync(path, text); });
h("fs:exists", (path: string) => existsSync(path));
h("fs:pickDir", async (def?: string) => {
  const r = await dialog.showOpenDialog(win!, { properties: ["openDirectory", "createDirectory"], defaultPath: def });
  return r.canceled ? null : r.filePaths[0];
});
h("fs:pickFiles", async () => {
  const r = await dialog.showOpenDialog(win!, { properties: ["openFile", "multiSelections"] });
  return r.canceled ? [] : r.filePaths;
});

h("preview:watch", (tab: string, path: string) => {
  previewWatch.get(tab)?.close();
  try {
    const w = watch(dirname(path), { recursive: true }, () => send("preview:changed", tab));
    previewWatch.set(tab, w);
  } catch { /* not watchable, manual reload still works */ }
});
h("preview:unwatch", (tab: string) => { previewWatch.get(tab)?.close(); previewWatch.delete(tab); });
h("browser:attach", (tab: string, wcId: number) => attachBrowser(tab, wcId));

h("agents:defs", (cwd?: string) => agentDefs(cwd));
h("agents:save", (def: any, cwd?: string, scope?: "user" | "project") => saveAgent(def, cwd, scope));
h("agents:hub", () => readAgentHub());

h("images:list", () => listImages());
h("images:generate", async (prompt: string, inputs?: { data: string; mimeType: string }[]) => {
  const s = loadSettings();
  return generateImage({ prompt, inputs, model: s.imageModel, provider: s.imageProvider, outDir: imagesDir, pi: s.piPath });
});

h("open:external", (url: string) => shell.openExternal(url));
h("open:path", (p: string) => shell.openPath(p));
h("open:reveal", (p: string) => shell.showItemInFolder(p));
h("clip:write", (t: string) => clipboard.writeText(t));
h("clip:image", (data: string) => {
  const png = nativeImage.createFromBuffer(Buffer.from(data, "base64")).toPNG();
  return clipboard.write([new ClipboardItem({ "image/png": new Blob([new Uint8Array(png)], { type: "image/png" }) })]);
});
h("notify", (title: string, body: string) => {
  if (win?.isFocused() || !Notification.isSupported()) return;
  const n = new Notification({ title, body, silent: false });
  n.on("click", () => { win?.show(); win?.focus(); });
  n.show();
});
h("win:focused", () => win?.isFocused() ?? false);
h("win:minimize", () => win?.minimize());
h("win:maximize", () => (win?.isMaximized() ? win.unmaximize() : win?.maximize()));
h("win:close", () => win?.close());

app.whenReady().then(async () => {
  bridge = await startBridge(bridgeCall);
  createWindow();
  watchDebounced(configDir, (f) => f === "colors.css", () => send("theme", readTheme()));
  watchDebounced(join(agentDir, "runtime"), (f) => f.startsWith("agents-state"), () => send("agents:hub", readAgentHub()));
});

app.on("window-all-closed", () => {
  for (const p of procs.values()) p.stop();
  app.quit();
});
