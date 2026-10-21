// App-wide state: open sessions, which view is up, settings, the session list from disk
import { api, type Mode, type Model, type SessionInfo, type Settings, type SysInfo } from "./lib/api";
import { titleFrom } from "./lib/fmt";
import { Tab } from "./tab";

export type View = "home" | "chat" | "usage" | "settings" | "agents" | "images";

type Ev = "view" | "tabs" | "sessions" | "settings" | "models" | "toast";
const subs = new Map<Ev, Set<() => void>>();

export const app = {
  tabs: new Map<string, Tab>(),
  active: null as Tab | null,
  view: "home" as View,
  settings: null as unknown as Settings,
  sys: null as unknown as SysInfo,
  sessions: [] as SessionInfo[],
  meta: {} as Record<string, { kind?: "chat" | "design"; mode?: Mode }>,
  models: [] as Model[],
  toasts: [] as { id: number; level: string; text: string }[],
  homeCwd: "",
  homeKind: "chat" as "chat" | "design",

  on(ev: Ev, fn: () => void) {
    if (!subs.has(ev)) subs.set(ev, new Set());
    subs.get(ev)!.add(fn);
    return () => subs.get(ev)!.delete(fn);
  },
  emit(ev: Ev) { for (const f of subs.get(ev) ?? []) f(); },

  show(view: View) {
    this.view = view;
    this.emit("view");
  },

  focus(t: Tab) {
    this.active = t;
    t.unread = false;
    t.lastUsed = Date.now();
    this.view = "chat";
    this.emit("view");
    this.emit("tabs");
    if (t.watch) void t.peek();
    else if (t.status === "cold") void t.ensure().then(() => this.trim());
  },

  // a subagent's run, opened as its own tab you can watch, and carry on in once it's done
  openAgent(a: { sessionFile: string; cwd: string; agent: string; task: string; running: boolean }) {
    let t = [...this.tabs.values()].find((x) => x.file === a.sessionFile);
    if (!t) {
      t = new Tab(a.cwd, a.sessionFile, "chat", this.settings.mode, `${a.agent} · ${titleFrom(a.task)}`);
      t.watch = true;
      t.agentRunning = a.running;
      // keep whatever side pane you opened it from, usually the agents list
      t.pane = this.active?.pane ?? null;
      this.add(t);
    }
    this.focus(t);
    return t;
  },

  // open a session file, reusing its tab when it's already open
  open(s: SessionInfo) {
    const m = this.meta[s.file] ?? {};
    const t = [...this.tabs.values()].find((x) => x.file === s.file) ?? this.add(new Tab(s.cwd, s.file, m.kind ?? "chat", m.mode ?? this.settings.mode, s.name));
    this.focus(t);
    return t;
  },

  add(t: Tab) {
    this.tabs.set(t.id, t);
    this.emit("tabs");
    return t;
  },

  fresh(cwd: string, kind: "chat" | "design" = "chat", mode?: Mode) {
    const t = this.add(new Tab(cwd, undefined, kind, mode ?? this.settings.mode));
    this.focus(t);
    return t;
  },

  async close(t: Tab) {
    await t.stop();
    this.tabs.delete(t.id);
    if (this.active === t) { this.active = null; this.view = "home"; this.emit("view"); }
    this.emit("tabs");
  },

  // idle sessions past the limit give their process back, oldest first. they come back on click
  trim() {
    const idle = [...this.tabs.values()].filter((t) => t !== this.active && t.status === "idle" && !t.asks.length).sort((a, b) => a.lastUsed - b.lastUsed);
    const live = [...this.tabs.values()].filter((t) => t.status === "idle" || t.status === "running" || t.status === "starting").length;
    for (let i = 0; i < live - this.settings.maxLive && i < idle.length; i++) void idle[i].stop();
  },

  async refreshSessions() {
    [this.sessions, this.meta] = await Promise.all([api.sessions(), api.meta()]);
    this.emit("sessions");
  },

  async loadModels(t?: Tab) {
    const tab = t ?? this.active ?? [...this.tabs.values()].find((x) => x.status === "idle" || x.status === "running");
    if (!tab || !(await tab.ensure())) return;
    const r = await tab.cmd({ type: "get_available_models" });
    if (r.success) { this.models = r.data.models; this.emit("models"); }
  },

  async saveSettings(patch: Partial<Settings>) {
    this.settings = await api.saveSettings(patch);
    this.emit("settings");
  },

  toast(text: string, level = "info") {
    const id = Date.now() + Math.random();
    this.toasts.push({ id, level, text });
    this.emit("toast");
    setTimeout(() => { this.toasts = this.toasts.filter((x) => x.id !== id); this.emit("toast"); }, level === "error" ? 7000 : 3800);
  },
};
