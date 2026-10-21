// Sessions, grouped by folder, newest first. Open ones show what their process is doing
import { app } from "../app";
import { api, type SessionInfo } from "../lib/api";
import { h, swap } from "../lib/dom";
import { ago, base, tilde, titleFrom } from "../lib/fmt";
import { icon } from "../lib/icons";
import { onTab, type Tab } from "../tab";
import { fuzzy } from "./composer";
import { menu, prompt } from "./overlay";

type Row = { file?: string; tab?: Tab; title: string; cwd: string; updated: number; info?: SessionInfo };

export class Sidebar {
  el: HTMLElement;
  private list = h("div.sb-list");
  private q = h("input.sb-search", { placeholder: "search", spellcheck: false }) as HTMLInputElement;
  private folded = new Set<string>();
  private showAll = new Set<string>();

  constructor() {
    this.el = h("div.sidebar",
      h("div.sb-top",
        h("button.sb-new", { onclick: () => app.show("home"), title: "new session (ctrl+n)" }, icon("plus"), "new"),
        h("div.sb-q", icon("search"), this.q),
      ),
      this.list,
      h("nav.sb-nav",
        navBtn("usage", "bars", "usage"), navBtn("agents", "agents", "agents"), navBtn("images", "image", "images"), navBtn("settings", "gear", "settings"),
      ),
    );
    this.q.addEventListener("input", () => this.draw());
    this.q.addEventListener("keydown", (e) => { if (e.key === "Escape") { this.q.value = ""; this.draw(); this.q.blur(); } });
    app.on("sessions", () => this.draw());
    app.on("tabs", () => this.draw());
    app.on("view", () => { this.draw(); this.drawNav(); });
    app.on("settings", () => this.draw());
    onTab((_t, w) => { if (w === "status" || w === "meta" || w === "asks") this.draw(); });
    this.draw();
  }

  focusSearch() { this.q.focus(); this.q.select(); }

  private drawNav() {
    for (const b of this.el.querySelectorAll<HTMLElement>(".sb-nav button")) b.classList.toggle("on", app.view === b.dataset.view);
  }

  private rows(): Row[] {
    const rows = new Map<string, Row>();
    for (const s of app.sessions) rows.set(s.file, { file: s.file, title: s.name || titleFrom(s.firstPrompt), cwd: s.cwd, updated: s.updated, info: s });
    for (const t of app.tabs.values()) {
      const key = t.file ?? t.id;
      const prev = rows.get(key);
      const hasItems = t.items.some((i) => i.k === "user");
      rows.set(key, { file: t.file, tab: t, title: hasItems || t.name || t.state.sessionName ? t.title : prev?.title ?? t.title, cwd: t.cwd, updated: Math.max(prev?.updated ?? 0, t.lastUsed) });
    }
    return [...rows.values()];
  }

  draw() {
    const q = this.q.value.trim();
    let rows = this.rows();
    if (q) rows = rows.map((r) => ({ r, s: Math.max(fuzzy(q, r.title), fuzzy(q, r.cwd) * 0.6, r.info ? fuzzy(q, r.info.firstPrompt) * 0.5 : 0) })).filter((x) => x.s > 0).sort((a, b) => b.s - a.s).map((x) => x.r);
    else rows.sort((a, b) => b.updated - a.updated);
    const pinned = new Set(app.settings.pinned);
    const out: HTMLElement[] = [];
    const pins = rows.filter((r) => r.file && pinned.has(r.file));
    if (pins.length && !q) out.push(h("div.sb-grp.static", h("span.sb-gname", "pinned")), ...pins.map((r) => this.row(r)));
    if (q) out.push(...rows.slice(0, 60).map((r) => this.row(r, true)));
    else {
      const groups = new Map<string, Row[]>();
      for (const r of rows) {
        if (r.file && pinned.has(r.file)) continue;
        groups.set(r.cwd, [...(groups.get(r.cwd) ?? []), r]);
      }
      for (const [cwd, rs] of groups) {
        const fold = this.folded.has(cwd);
        const running = rs.some((r) => r.tab?.running);
        out.push(h(`div.sb-grp${fold ? ".fold" : ""}`, { title: cwd, onclick: () => { if (fold) this.folded.delete(cwd); else this.folded.add(cwd); this.draw(); } },
          h("span.sb-caret", fold ? "▸" : "▾"), h("span.sb-gname", tilde(cwd) === "~" ? "~" : base(cwd) || "/"), h("span.sb-gpath", tilde(cwd) === "~" ? "" : tilde(cwd).replace(/\/[^/]*$/, "")), running ? h("span.sb-grun", { "data-spin": "" }) : null,
          h("button.sb-gnew", { title: `new session in ${tilde(cwd)}`, onclick: (e: Event) => { e.stopPropagation(); app.fresh(cwd); } }, icon("plus"))));
        if (fold) continue;
        const limit = this.showAll.has(cwd) ? rs.length : 6;
        out.push(...rs.slice(0, limit).map((r) => this.row(r)));
        if (rs.length > limit) out.push(h("button.sb-more", { onclick: () => { this.showAll.add(cwd); this.draw(); } }, `${rs.length - limit} older`));
      }
    }
    if (!out.length) out.push(h("div.sb-empty", q ? "nothing matches" : "no sessions yet"));
    swap(this.list, out);
  }

  private row(r: Row, withCwd = false) {
    const t = r.tab;
    const active = !!t && t === app.active && app.view === "chat";
    let glyph: HTMLElement;
    if (t?.asks.length) glyph = h("span.sg.q", "?");
    else if (t?.running || t?.status === "starting") glyph = h("span.sg.run", { "data-spin": "" });
    else if (t?.status === "dead") glyph = h("span.sg.dead", "×");
    else if (t?.unread) glyph = h("span.sg.unread", "●");
    else if (t && t.status === "idle") glyph = h("span.sg.live", "·");
    else glyph = h("span.sg");
    const el = h(`div.sb-row${active ? ".on" : ""}${t?.kind === "design" ? ".design" : ""}`, {
      onclick: () => { if (t) app.focus(t); else if (r.info) app.open(r.info); },
      oncontextmenu: (e: MouseEvent) => { e.preventDefault(); this.context(e, r); },
      title: r.cwd,
    }, glyph, h("span.sb-title", r.title), withCwd ? h("span.sb-cwd", base(r.cwd)) : null, h("span.sb-ago", ago(r.updated)));
    return el;
  }

  private context(e: MouseEvent, r: Row) {
    const pinned = !!r.file && app.settings.pinned.includes(r.file);
    menu(e.clientX, e.clientY, [
      { label: "rename", run: async () => {
        const name = await prompt("rename session", r.title);
        if (!name) return;
        const t = r.tab ?? (r.info ? app.open(r.info) : null);
        await t?.rename(name);
        void app.refreshSessions();
      } },
      r.file && { label: pinned ? "unpin" : "pin", run: () => void app.saveSettings({ pinned: pinned ? app.settings.pinned.filter((f) => f !== r.file) : [...app.settings.pinned, r.file!] }) },
      { label: "new session here", run: () => app.fresh(r.cwd) },
      { label: "open folder", run: () => void api.openPath(r.cwd) },
      r.tab && r.tab.status !== "cold" && { label: "stop process", run: () => void r.tab!.stop() },
      r.tab && { label: "close", run: () => void app.close(r.tab!) },
      r.file && { label: "move to trash", danger: true, run: async () => {
        if (r.tab) await app.close(r.tab);
        await api.trashSession(r.file!);
        void app.refreshSessions();
      } },
    ]);
  }
}

function navBtn(view: string, ic: string, label: string) {
  return h("button", { "data-view": view, onclick: () => app.show(view as any), title: label }, icon(ic), h("span", label));
}
