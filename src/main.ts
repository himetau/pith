// Renderer entry. Lays out the windows and wires pi's events, the pickers and the keyboard
import "@xterm/xterm/css/xterm.css";
import "./styles/base.css";
import "./styles/shell.css";
import "./styles/chat.css";
import "./styles/panes.css";
import "./styles/pages.css";
import { act, MODES } from "./actions";
import { app } from "./app";
import { api, type Mode } from "./lib/api";
import { h, swap } from "./lib/dom";
import { ago, base, home, tilde, titleFrom } from "./lib/fmt";
import { icon } from "./lib/icons";
import { applyLook, applyTheme, stepScale } from "./look";
import { PANES, Side } from "./panes";
import { onTab, type Tab } from "./tab";
import { AgentsPage } from "./views/agents";
import { chatFor, dropChat } from "./views/chat";
import { Home } from "./views/home";
import { ImagesPage } from "./views/images";
import { lightbox, menu, picker, prompt, Toasts, type PickItem } from "./views/overlay";
import { SettingsPage } from "./views/settings";
import { Sidebar } from "./views/sidebar";
import { StatusLine } from "./views/statusline";
import { UsagePage } from "./views/usage";

async function boot() {
  const [sys, settings] = await Promise.all([api.sys(), api.settings()]);
  app.sys = sys;
  app.settings = settings;
  home.dir = sys.home;
  applyTheme(sys.theme.css);
  applyLook();
  api.on("theme", (t: { css: string }) => applyTheme(t.css));
  void app.refreshSessions();
  setInterval(() => void app.refreshSessions(), 30000);
  addEventListener("focus", () => void app.refreshSessions());
  mount();
}

function mount() {
  const sidebar = new Sidebar();
  const side = new Side();
  const homeView = new Home();
  const pages = { usage: new UsagePage(), settings: new SettingsPage(), agents: new AgentsPage(), images: new ImagesPage() };
  const status = new StatusLine();
  const body = h("div.main-body");
  const legL = h("div.legend.l");
  const legR = h("div.legend.r");
  const sideWin = h("div.win.sidebar-win", h("div.legend.l", h("span.legend-t", "pith")), sidebar.el);
  const mainWin = h("div.win.main", legL, legR, body);
  const resize = h("div.resize", { title: "drag to resize" });
  const root = h("div#app", sideWin, mainWin, side.el, status.el);
  side.el.append(resize);
  document.body.append(root, h("div#overlay"), new Toasts().el);
  document.body.classList.toggle("no-sidebar", !app.settings.sidebar);

  // focus follows the mouse into a window, like the compositor does
  for (const w of [sideWin, mainWin, side.el]) w.addEventListener("mousedown", () => { for (const x of [sideWin, mainWin, side.el]) x.classList.toggle("focus", x === w); });
  mainWin.classList.add("focus");

  resize.addEventListener("mousedown", (e) => {
    e.preventDefault();
    const startX = e.clientX, startW = app.settings.paneWidth;
    document.body.classList.add("resizing");
    const move = (m: MouseEvent) => document.documentElement.style.setProperty("--side-w", `${Math.max(320, Math.min(innerWidth - 520, startW + (startX - m.clientX)))}px`);
    const up = (m: MouseEvent) => {
      removeEventListener("mousemove", move);
      removeEventListener("mouseup", up);
      document.body.classList.remove("resizing");
      void app.saveSettings({ paneWidth: Math.max(320, Math.min(innerWidth - 520, startW + (startX - m.clientX))) });
    };
    addEventListener("mousemove", move);
    addEventListener("mouseup", up);
  });

  let shown: HTMLElement | null = null;
  const render = () => {
    let el: HTMLElement;
    const t = app.active;
    if (app.view === "chat" && t) el = chatFor(t).el;
    else if (app.view === "home" || (app.view === "chat" && !t)) { el = homeView.el; homeView.draw(); }
    else el = pages[app.view as keyof typeof pages].el;
    if (shown !== el) { body.replaceChildren(el); shown = el; }
    drawLegends();
    if (app.view === "chat" && t) { const c = chatFor(t); c.renderAll(); c.toBottom(); setTimeout(() => c.composer.focus(), 0); }
    if (app.view === "home") setTimeout(() => homeView.composer.focus(), 0);
  };

  const drawLegends = () => {
    const t = app.active;
    if (app.view !== "chat" || !t) {
      swap(legL, h("span.legend-t", app.view === "home" || (app.view === "chat" && !t) ? "home" : app.view));
      swap(legR);
      return;
    }
    swap(legL,
      t.kind === "design" ? h("span.legend-tag", "design") : null,
      h("span.legend-t", { title: "double-click to rename", ondblclick: () => void rename(t) }, t.title),
      h("span.legend-sub", tilde(t.cwd)),
    );
    swap(legR,
      PANES.map((p) => h(`button.legend-btn.ico${t.pane === p.id ? ".on" : ""}`, { title: `${p.label} (${p.keys})`, onclick: () => act.togglePane(t, p.id) }, icon(p.ic))),
      h("button.legend-btn.ico", { title: "more", onclick: (e: MouseEvent) => sessionMenu(t, e) }, icon("more")),
    );
  };

  app.on("view", render);
  app.on("tabs", drawLegends);
  onTab((t, w) => { if (t === app.active && (w === "meta" || w === "items" || w === "status")) drawLegends(); });

  // ---------- pi -> tabs ----------
  api.on("pi:event", (id: string, rec) => app.tabs.get(id)?.onEvent(rec));
  api.on("pi:exit", (id: string, code: number | null, stderr: string) => {
    const t = app.tabs.get(id);
    if (!t || t.status === "cold") return;
    t.status = "dead";
    t.error = stderr.trim() || `pi exited with ${code}`;
    t.emit("status");
  });
  api.on("pane:open", (id: string, kind: string) => { const t = app.tabs.get(id); if (t) act.openPane(t, kind); });
  api.on("browser:url", (id: string, url: string) => { const t = app.tabs.get(id); if (t) t.browserUrl = url; });

  // ---------- actions ----------
  act.quote = (t, text, label) => { if (app.active !== t) app.focus(t); chatFor(t).composer.addQuote(text, label); };
  act.attach = (t, img) => { chatFor(t).composer.addImage(img); };
  act.lightbox = (src, cap) => lightbox(src, cap);
  act.runCommand = async (t, text) => { if (!t) return false; if (app.active !== t || app.view !== "chat") app.focus(t); await chatFor(t).send(text, [], "auto"); return true; };
  act.pickMode = (t) => picker({ title: "mode", items: MODES.map((m) => ({ label: m.label, hint: m.hint, on: t.mode === m.id, run: () => t.setMode(m.id as Mode) })) });
  act.pickEffort = (t) => {
    if (!t.levels.length) return app.toast("this model doesn't take an effort level");
    picker({ title: "effort", items: t.levels.map((l) => ({ label: l, on: t.state.thinkingLevel === l, run: () => void t.setThinking(l) })) });
  };
  act.pickModel = async (t) => {
    if (!app.models.length) await app.loadModels(t);
    const cur = t.model;
    const items: PickItem[] = app.models.map((m) => ({
      label: m.id, group: m.provider, hint: m.name && m.name !== m.id ? m.name : undefined,
      right: [m.contextWindow ? `${Math.round(m.contextWindow / 1000)}k` : "", m.reasoning ? "thinks" : "", m.input?.includes("image") ? "sees" : ""].filter(Boolean).join(" · "),
      on: cur?.id === m.id && cur?.provider === m.provider,
      run: () => void t.setModel(m),
    }));
    items.sort((a, b) => Number(!!b.on) - Number(!!a.on) || (a.group === cur?.provider ? -1 : 0) - (b.group === cur?.provider ? -1 : 0));
    picker({ title: "model", items, placeholder: "search models" });
  };
  act.palette = () => palette();

  // ---------- keys ----------
  addEventListener("keydown", (e) => {
    const t = app.active, inChat = app.view === "chat" && !!t;
    if (document.querySelector("#overlay .scrim, #overlay .ctx-menu")) return;
    if (inChat && chatFor(t!).handleKey(e)) { e.preventDefault(); return; }
    const k = [e.ctrlKey && "ctrl", e.altKey && "alt", e.shiftKey && "shift", e.key.length === 1 ? e.key.toLowerCase() : e.key].filter(Boolean).join("+");
    const run = (fn: () => void) => { e.preventDefault(); fn(); };
    switch (k) {
      case "ctrl+n": return run(() => { app.homeKind = "chat"; app.show("home"); });
      case "ctrl+shift+n": return run(() => { app.homeKind = "design"; app.homeCwd = app.settings.designDir; app.show("home"); });
      case "ctrl+k": return run(() => palette());
      case "ctrl+p": return run(() => palette(true));
      case "ctrl+b": return run(() => { const on = !document.body.classList.toggle("no-sidebar"); void app.saveSettings({ sidebar: on }); });
      case "ctrl+\\": return run(() => { if (t) act.togglePane(t, t.pane ?? "browser"); });
      case "ctrl+,": return run(() => app.show("settings"));
      case "ctrl+=": case "ctrl++": case "ctrl+shift++": return run(() => stepScale(1));
      case "ctrl+-": return run(() => stepScale(-1));
      case "ctrl+0": return run(() => stepScale(0));
      case "ctrl+m": if (inChat) return run(() => act.pickModel(t!)); return;
      case "ctrl+e": if (inChat) return run(() => act.pickEffort(t!)); return;
      case "Escape":
        // the composer handles its own esc
        if (inChat && t!.running && e.target === document.body) return run(() => void t!.abort());
        return;
    }
    const pane = k.match(/^alt\+([1-6])$/);
    if (pane && t) return run(() => act.togglePane(t, PANES[Number(pane[1]) - 1].id));
    // typing anywhere lands in the composer
    if (inChat && e.key.length === 1 && !e.ctrlKey && !e.altKey && !e.metaKey && e.target === document.body) chatFor(t!).composer.focus();
  });

  api.onDrop((paths) => {
    const t = app.view === "chat" ? app.active : null;
    void (t ? chatFor(t).composer : homeView.composer).addPaths(paths);
  });
  addEventListener("dragover", (e) => e.preventDefault());
  addEventListener("drop", (e) => e.preventDefault());

  // closing a session also drops its webviews and chat
  const close = app.close.bind(app);
  app.close = async (t: Tab) => { await close(t); side.drop(t); dropChat(t); };

  render();
  side.sync();
  // handle for poking at it from devtools
  (window as unknown as { pith_: unknown }).pith_ = { app, act, chatFor, api };
}

async function rename(t: Tab) {
  const name = await prompt("rename session", t.title);
  if (name) { await t.rename(name); void app.refreshSessions(); }
}

function sessionMenu(t: Tab, e: MouseEvent) {
  menu(e.clientX, e.clientY, [
    { label: "rename", run: () => void rename(t) },
    { label: "compact context", hint: "/compact", run: () => void t.compact() },
    { label: "clone into a new session", run: () => void t.clone() },
    { label: "export as html", run: async () => { const p = await t.exportHtml(); if (p) void api.openPath(p); } },
    { label: "copy last reply", run: async () => { const x = await t.lastText(); if (x) { void api.copy(x); app.toast("copied"); } } },
    t.file && { label: "show session file", run: () => void api.reveal(t.file!) },
    { label: "open folder", run: () => void api.openPath(t.cwd) },
    t.status !== "cold" && { label: "stop process", hint: "comes back when you send", run: () => void t.stop() },
    { label: "close", run: () => void app.close(t) },
  ]);
}

function palette(sessionsOnly = false) {
  const t = app.view === "chat" ? app.active : null;
  const items: PickItem[] = [];
  const add = (label: string, run: () => void, keys?: string, hint?: string) => items.push({ label, run, keys, hint, group: "actions" });
  if (!sessionsOnly) {
    add("new session", () => { app.homeKind = "chat"; app.show("home"); }, "ctrl+n");
    add("new design", () => { app.homeKind = "design"; app.homeCwd = app.settings.designDir; app.show("home"); }, "ctrl+shift+n");
    if (t) {
      add("switch model", () => act.pickModel(t), "ctrl+m");
      add("effort", () => act.pickEffort(t), "ctrl+e");
      add("mode", () => act.pickMode(t), "shift+tab");
      for (const p of PANES) add(`${p.label} pane`, () => act.openPane(t, p.id), p.keys);
      add("review changes", () => void act.runCommand(t, "/review"), undefined, "/review");
      add("security review", () => void act.runCommand(t, "/security-review"), undefined, "/security-review");
      add("simplify the changed code", () => void act.runCommand(t, "/simplify"), undefined, "/simplify");
      add("compact context", () => void t.compact(), undefined, "/compact");
      add("rename session", () => void rename(t));
      add("clone session", () => void t.clone());
      add("export as html", async () => { const p = await t.exportHtml(); if (p) void api.openPath(p); });
      for (const c of t.commands) items.push({ label: `/${c.name}`, hint: c.description, group: "commands", run: () => { const ch = chatFor(t); ch.composer.setText(`/${c.name} `); ch.composer.focus(); } });
    }
    add("usage", () => app.show("usage"));
    add("agents", () => app.show("agents"));
    add("images", () => app.show("images"));
    add("settings", () => app.show("settings"), "ctrl+,");
    add("toggle sidebar", () => document.body.classList.toggle("no-sidebar"), "ctrl+b");
  }
  for (const s of app.sessions.slice(0, 300)) items.push({ label: s.name || titleFrom(s.firstPrompt), hint: base(s.cwd), right: ago(s.updated), group: "sessions", run: () => app.open(s) });
  picker({ title: sessionsOnly ? "sessions" : "commands", items, placeholder: sessionsOnly ? "find a session" : "do something or find a session" });
}

void boot();
