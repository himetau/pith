// The window to the right of the chat. One per session: web, preview, changes, files, agents
import { act } from "./actions";
import { app } from "./app";
import { api, type WebSlot } from "./lib/api";
import { h, swap } from "./lib/dom";
import { ago, base, bytes, cost, dur, tilde, tokens } from "./lib/fmt";
import { icon } from "./lib/icons";
import { highlight, langOf } from "./lib/md";
import { onTab, type Tab } from "./tab";
import { fuzzy } from "./views/composer";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";

export const PANES = [
  { id: "browser", label: "web", ic: "globe", keys: "alt+1" },
  { id: "preview", label: "preview", ic: "frame", keys: "alt+2" },
  { id: "diff", label: "changes", ic: "diff", keys: "alt+3" },
  { id: "files", label: "files", ic: "file", keys: "alt+4" },
  { id: "agents", label: "agents", ic: "agents", keys: "alt+5" },
  { id: "terminal", label: "term", ic: "terminal", keys: "alt+6" },
] as const;

const abs = (tab: Tab, p: string) => (p.startsWith("/") ? p : p.startsWith("~") ? app.sys.home + p.slice(1) : `${tab.cwd.replace(/\/$/, "")}/${p.replace(/^\.\//, "")}`);

// ---------- web ----------
// the page itself is a native webview the backend lays over .wv-host; this is only the chrome around it
class Browser {
  el = h("div.pane.browser");
  host = h("div.wv-host");
  private url = h("input.url", { spellcheck: false }) as HTMLInputElement;
  private badge = h("span.agent-badge", h("span", { "data-spin": "" }), " agent is driving");
  private driving = new Set<string>();
  private made = new Set<string>();
  private tab: Tab | null = null;

  constructor() {
    const go = (what: "back" | "forward" | "reload") => { if (this.tab) void api.webGo("web", this.tab.id, what).catch(() => {}); };
    this.el.append(
      h("div.pbar",
        h("button.pb", { title: "back", onclick: () => go("back") }, icon("back")),
        h("button.pb", { title: "forward", onclick: () => go("forward") }, icon("fwd")),
        h("button.pb", { title: "reload", onclick: () => go("reload") }, icon("refresh")),
        this.url,
        this.badge,
        h("button.pb", { title: "open in your browser", onclick: async () => { if (!this.tab) return; const u = await api.webUrl("web", this.tab.id); if (u) void api.openExternal(u); } }, icon("external")),
      ),
      this.host,
    );
    this.url.addEventListener("keydown", (e) => {
      if (e.key !== "Enter" || !this.tab) return;
      let u = this.url.value.trim();
      if (!u) return;
      if (!/^[a-z]+:/i.test(u)) u = /\s/.test(u) || !u.includes(".") ? `https://duckduckgo.com/?q=${encodeURIComponent(u)}` : `https://${u}`;
      this.tab.browserUrl = u;
      if (this.made.has(this.tab.id)) void api.webNavigate(this.tab.id, u).catch(() => {});
      this.url.blur();
    });
    api.on("browser:url", (id: string, u: string) => {
      this.made.add(id);
      if (this.tab?.id === id && document.activeElement !== this.url) this.url.value = u;
    });
    onTab((t, w) => {
      if (w === "tool:browser") this.driving.add(t.id);
      if (w === "tooldone:browser" || w === "settled") this.driving.delete(t.id);
      if (t === this.tab) this.drawBadge();
    });
  }

  private drawBadge() {
    const on = !!this.tab && this.driving.has(this.tab.id);
    this.badge.classList.toggle("on", on);
    this.host.classList.toggle("driving", on);
  }

  show(t: Tab) {
    this.tab = t;
    if (document.activeElement !== this.url) this.url.value = t.browserUrl || app.settings.browserHome;
    this.drawBadge();
  }

  // the first layout creates the session's webview at its last known page, later ones only move it
  slot(t: Tab): WebSlot | null {
    const r = this.host.getBoundingClientRect();
    if (r.width < 4 || r.height < 4) return null;
    const first = !this.made.has(t.id);
    this.made.add(t.id);
    return { kind: "web", tab: t.id, url: first ? t.browserUrl || app.settings.browserHome : undefined, x: r.left, y: r.top, w: r.width, h: r.height };
  }

  drop(t: Tab) { this.made.delete(t.id); }
}

// ---------- preview ----------
const VIEWPORTS = [{ id: "fill", ic: "desktop", w: 0 }, { id: "tablet", ic: "tablet", w: 820 }, { id: "phone", ic: "phone", w: 390 }];
class Preview {
  el = h("div.pane.preview");
  stage = h("div.pv-stage");
  private label = h("span.pv-path");
  private pickBtn = h("button.pb", { title: "click an element to comment on it" }, icon("pick"), h("span", "comment"));
  private vpBtns = h("div.pv-vps");
  private empty = h("div.pane-empty");
  private tab: Tab | null = null;
  private vp = new Map<string, string>();
  private watched = new Map<string, string>();

  constructor(private relayout: () => void) {
    this.el.append(
      h("div.pbar", this.label, this.vpBtns,
        this.pickBtn,
        h("button.pb", { title: "reload", onclick: () => { if (this.tab) void api.webGo("pv", this.tab.id, "reload").catch(() => {}); } }, icon("refresh")),
        h("button.pb", { title: "open html file", onclick: () => void this.openFile() }, icon("folder")),
        h("button.pb", { title: "open in your browser", onclick: () => { const p = this.tab?.preview; if (p) void api.openExternal(p.url ?? `file://${p.path}`); } }, icon("external")),
      ),
      this.stage, this.empty,
    );
    this.pickBtn.onclick = () => this.setPick(!this.pickBtn.classList.contains("on"));
    api.on("preview:open", (tabId: string, args: { path?: string; url?: string; title?: string }) => {
      const t = app.tabs.get(tabId);
      if (!t) return;
      t.preview = args;
      act.openPane(t, "preview");
    });
    api.on("preview:pick", (tabId: string, d: { selector: string; html: string; text: string }) => {
      const t = app.tabs.get(tabId);
      if (!t) return;
      const tag = (d.html.match(/^<(\w+)/) ?? [])[1] ?? "element";
      const words = d.text.trim().replace(/\s+/g, " ");
      act.quote(t, `${t.preview?.path ? tilde(t.preview.path) : "preview"} · ${d.selector}\n${d.html}`, `<${tag}>${words ? ` “${words.slice(0, 48)}${words.length > 48 ? "…" : ""}”` : ""}  ·  ${d.selector}`);
    });
    api.on("preview:pick-off", () => this.pickBtn.classList.remove("on"));
    // a reload drops the page's picker state, so turn it back on if it was
    api.on("preview:loaded", (tabId: string) => { if (this.tab?.id === tabId && this.pickBtn.classList.contains("on")) this.setPick(true); });
  }

  private setPick(on: boolean) {
    this.pickBtn.classList.toggle("on", on);
    if (this.tab) void api.webEval("pv", this.tab.id, `window.__pithPick && window.__pithPick(${on})`).catch(() => {});
  }

  private async openFile() {
    if (!this.tab) return;
    const [f] = await api.pickFiles();
    if (!f) return;
    this.tab.preview = { path: f };
    this.show(this.tab);
    this.relayout();
  }

  show(t: Tab) {
    this.tab = t;
    const p = t.preview;
    this.empty.hidden = !!p;
    swap(this.empty, h("div", h("div.pe-t", "nothing to preview"), h("div.dim", "ask for a design, use /design, or open an html file"), h("button.btn", { onclick: () => void this.openFile() }, icon("folder"), "open file")));
    swap(this.vpBtns, VIEWPORTS.map((v) => h(`button.pb${(this.vp.get(t.id) ?? "fill") === v.id ? ".on" : ""}`, { title: v.id, onclick: () => { this.vp.set(t.id, v.id); this.show(t); this.relayout(); } }, icon(v.ic))));
    this.stage.classList.toggle("framed", (this.vp.get(t.id) ?? "fill") !== "fill");
    if (!p) { swap(this.label); return; }
    swap(this.label, p.title ?? (p.path ? tilde(p.path) : p.url));
    if (p.path && this.watched.get(t.id) !== p.path) { this.watched.set(t.id, p.path); void api.watchPreview(t.id, p.path); }
  }

  slot(t: Tab): WebSlot | null {
    const p = t.preview;
    if (!p) return null;
    const r = this.stage.getBoundingClientRect();
    if (r.width < 4 || r.height < 4) return null;
    const vp = VIEWPORTS.find((v) => v.id === (this.vp.get(t.id) ?? "fill"))!;
    const pad = vp.w ? 14 : 0;
    const w = vp.w ? Math.min(vp.w, r.width - pad * 2) : r.width;
    const url = p.url ?? `file://${encodeURI(p.path!)}`;
    return { kind: "pv", tab: t.id, url, x: r.left + (r.width - w) / 2, y: r.top + pad, w, h: r.height - pad * 2 };
  }

  drop(t: Tab) { this.watched.delete(t.id); void api.unwatchPreview(t.id); }
}

// ---------- changes ----------
class Changes {
  el = h("div.pane.changes");
  private head = h("div.pbar");
  private files = h("div.ch-files");
  private diff = h("div.ch-diff");
  private tab: Tab | null = null;
  private sel = new Map<string, string>();

  constructor() { this.el.append(this.head, this.files, this.diff); }

  async show(t: Tab) {
    this.tab = t;
    const st = await api.gitStatus(t.cwd);
    if (this.tab !== t) return;
    if (!st.repo) {
      swap(this.head, h("span.dim", "not a git repository"));
      swap(this.files);
      swap(this.diff, h("div.pane-empty", h("div", h("div.pe-t", "no git here"), h("div.dim", tilde(t.cwd)))));
      return;
    }
    const add = st.files.reduce((a, f) => a + f.add, 0), del = st.files.reduce((a, f) => a + f.del, 0);
    swap(this.head,
      h("span.ch-branch", icon("branch"), st.branch),
      h("span.ch-sum", `${st.files.length} files `, h("span.add", `+${add}`), " ", h("span.del", `-${del}`)),
      h("span.grow"),
      h("button.pb", { title: "refresh", onclick: () => void this.show(t) }, icon("refresh")),
      h("button.btn.sm", { disabled: !st.files.length, onclick: () => void act.runCommand(t, "/review") }, icon("review"), "review"),
      h("button.btn.sm.ghost", { disabled: !st.files.length, onclick: () => void act.runCommand(t, "/security-review") }, "security"),
    );
    let cur = this.sel.get(t.id);
    if (!st.files.some((f) => f.path === cur)) cur = st.files[0]?.path;
    swap(this.files, st.files.length ? st.files.map((f) => h(`div.ch-file${f.path === cur ? ".on" : ""}`, { onclick: () => { this.sel.set(t.id, f.path); void this.show(t); } },
      h(`span.ch-code.c${f.code.trim()[0] === "?" ? "N" : f.code.trim()[0] ?? "M"}`, f.code.trim() === "??" ? "new" : f.code.trim()),
      h("span.ch-path", h("span.dim", f.path.includes("/") ? f.path.replace(/[^/]*$/, "") : ""), base(f.path)),
      h("span.ch-n", h("span.add", `+${f.add}`), " ", h("span.del", `-${f.del}`)))) : h("div.dim.ch-none", "working tree is clean"));
    if (!cur) { swap(this.diff); return; }
    const f = st.files.find((x) => x.path === cur)!;
    const text = await api.gitDiff(t.cwd, cur, f.code === "??");
    if (this.tab !== t) return;
    swap(this.diff, unified(text, cur, (line) => act.openFile(t, cur!, line)));
  }

  focusFile(t: Tab, path: string) { this.sel.set(t.id, path); void this.show(t); }
}

function unified(text: string, path: string, open: (line: number) => void) {
  const box = h("div.diff.unified");
  let o = 0, n = 0;
  const lang = langOf(path);
  for (const l of text.split("\n")) {
    if (/^(diff --git|index |--- |\+\+\+ |new file|deleted file|similarity|rename )/.test(l)) continue;
    const hunk = l.match(/^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@(.*)$/);
    if (hunk) { o = +hunk[1]; n = +hunk[2]; box.append(h("div.dl.hunk", h("span.dn", "⋯"), h("span.dn", ""), h("span.dc", `line ${n}${hunk[3]?.trim() ? `  ·  ${hunk[3].trim()}` : ""}`))); continue; }
    if (l.startsWith("\\")) continue;
    const sign = l[0], code = l.slice(1);
    const html = highlight(code, lang);
    if (sign === "+") { const ln = n++; box.append(h("div.dl.add", { ondblclick: () => open(ln) }, h("span.dn", ""), h("span.dn", String(ln)), h("span.dc", { html }))); }
    else if (sign === "-") box.append(h("div.dl.del", h("span.dn", String(o++)), h("span.dn", ""), h("span.dc", { html })));
    else if (sign === " ") { const ln = n; box.append(h("div.dl", { ondblclick: () => open(ln) }, h("span.dn", String(o++)), h("span.dn", String(n++)), h("span.dc", { html }))); }
  }
  if (!box.childElementCount) box.append(h("div.dim.ch-none", "binary file, or no textual change"));
  return box;
}

// ---------- files ----------
class Files {
  el = h("div.pane.files");
  private find = h("input.url", { placeholder: "find a file", spellcheck: false }) as HTMLInputElement;
  private results = h("div.f-results");
  private head = h("div.f-head");
  private body = h("div.f-body");
  private tab: Tab | null = null;
  private open = new Map<string, { path: string; line?: number }>();
  private all: string[] = [];

  constructor() {
    this.el.append(h("div.pbar", icon("search"), this.find), this.results, this.head, this.body);
    this.find.addEventListener("input", () => this.search());
    this.find.addEventListener("focus", async () => { if (this.tab) { this.all = await api.listFiles(this.tab.cwd); this.search(); } });
    this.find.addEventListener("blur", () => setTimeout(() => swap(this.results), 150));
    this.find.addEventListener("keydown", (e) => {
      if (e.key === "Enter") { const first = this.results.querySelector<HTMLElement>(".f-hit"); first?.dispatchEvent(new MouseEvent("mousedown")); }
      if (e.key === "Escape") { this.find.value = ""; swap(this.results); this.find.blur(); }
    });
  }

  private search() {
    const q = this.find.value.trim();
    if (!q || !this.tab) return swap(this.results);
    const t = this.tab;
    const hits = this.all.map((f) => ({ f, s: fuzzy(q, f) })).filter((x) => x.s > 0).sort((a, b) => b.s - a.s).slice(0, 14);
    swap(this.results, hits.map(({ f }) => h("div.f-hit", { onmousedown: (e: Event) => { e.preventDefault(); this.find.value = ""; swap(this.results); this.openPath(t, abs(t, f)); } }, f)));
  }

  openPath(t: Tab, path: string, line?: number) {
    this.open.set(t.id, { path: abs(t, path), line });
    if (this.tab === t) void this.show(t);
  }

  async show(t: Tab) {
    this.tab = t;
    const o = this.open.get(t.id);
    if (!o) {
      swap(this.head);
      swap(this.body, h("div.pane-empty", h("div", h("div.pe-t", "no file open"), h("div.dim", "click a path in the chat, or find one above"))));
      return;
    }
    const r = await api.read(o.path);
    if (this.tab !== t) return;
    swap(this.head,
      h("span.f-path", { title: o.path }, tilde(o.path)), h("span.dim", ` ${bytes(r.size)}`), h("span.grow"),
      h("button.pb", { title: "copy path", onclick: () => void api.copy(o.path) }, icon("copy")),
      h("button.pb", { title: "show in folder", onclick: () => void api.reveal(o.path) }, icon("folder")),
      h("button.pb", { title: "open with default app", onclick: () => void api.openPath(o.path) }, icon("external")),
    );
    if (r.kind === "image") return swap(this.body, h("div.f-img", h("img", { src: `data:${r.mimeType};base64,${r.data}`, onclick: (e: Event) => act.lightbox((e.target as HTMLImageElement).src) })));
    if (r.kind !== "text") return swap(this.body, h("div.pane-empty", h("div.dim", r.kind === "missing" ? "file doesn't exist" : r.kind === "big" ? "too big to show" : "binary file")));
    const ls = r.text.split("\n");
    const pre = h("pre.f-code", h("code.gutter", ls.map((_, i) => String(i + 1)).join("\n")), h("code.hljs", { html: highlight(r.text.length > 400000 ? r.text.slice(0, 400000) : r.text, langOf(o.path)) }));
    swap(this.body, pre);
    if (o.line) {
      const lh = parseFloat(getComputedStyle(pre).lineHeight) || 20;
      const mark = h("div.f-mark", { style: { top: `${(o.line - 1) * lh + 12}px`, height: `${lh}px` } });
      pre.append(mark);
      requestAnimationFrame(() => { this.body.scrollTop = Math.max(0, (o.line! - 6) * lh); });
    }
  }
}

// ---------- agents (live) ----------
export let hub: any = null;
class Agents {
  el = h("div.pane.agents");
  private list = h("div.ag-list");
  private openIds = new Set<number>();
  constructor() {
    this.el.append(h("div.pbar", h("span", "subagents"), h("span.grow"), h("button.btn.sm.ghost", { onclick: () => app.show("agents") }, "library")), this.list);
    void api.agentHub().then((s) => { hub = s; this.draw(); });
    api.on("agents:hub", (s) => { hub = s; this.draw(); app.emit("tabs"); });
  }
  draw() {
    const agents: any[] = (hub?.agents ?? []).slice().sort((a: any, b: any) => b.startedAt - a.startedAt);
    if (!agents.length) return swap(this.list, h("div.pane-empty", h("div", h("div.pe-t", "no agents yet"), h("div.dim", "when pi uses the subagent tool, or /summon, they show up here live"))));
    swap(this.list, agents.map((a) => {
      const run = ["spawning", "thinking", "tool", "streaming"].includes(a.status);
      const open = this.openIds.has(a.id);
      return h(`div.ag${run ? ".run" : ""}${a.status === "failed" ? ".bad" : ""}`,
        h("div.ag-head", { onclick: () => { if (open) this.openIds.delete(a.id); else this.openIds.add(a.id); this.draw(); } },
          run ? h("span.ac-spin", { "data-spin": "" }) : h(`span.ac-dot${a.status === "failed" ? ".bad" : ""}`),
          h("span.ac-name", a.agent), h("span.ag-st", a.status), h("span.grow"),
          h("span.dim", run ? h("span", { "data-since": String(a.startedAt) }) : a.endedAt ? dur(a.endedAt - a.startedAt) : ago(a.startedAt))),
        h("div.ag-task", a.task),
        run && a.currentActivity ? h("div.ac-now", h("span.dim", "› "), a.currentActivity) : null,
        h("div.ag-meta", `${a.usage?.turns ?? 0} turns · ${tokens((a.usage?.input ?? 0) + (a.usage?.output ?? 0))} tok · ${cost(a.usage?.cost ?? 0)}${a.model ? ` · ${a.model}` : ""}${a.pid ? ` · pid ${a.pid}` : ""}`),
        open ? h("div.ag-more",
          (a.timeline ?? []).slice(-30).map((e: any) => h(`div.ag-ev.${e.type}`, h("span.dim", new Date(e.timestamp).toLocaleTimeString()), " ", e.summary)),
          a.finalOutput ? h("pre.tool-out", a.finalOutput) : a.textStream ? h("pre.tool-out", a.textStream.slice(-3000)) : null,
        ) : null,
      );
    }));
  }
  runningCount() { return (hub?.agents ?? []).filter((a: any) => ["spawning", "thinking", "tool", "streaming"].includes(a.status)).length; }
}

// ---------- terminal ----------
// a real shell per session, started in its folder. colours are the matugen palette
function termTheme() {
  const v = (n: string) => getComputedStyle(document.documentElement).getPropertyValue(n).trim();
  return {
    background: v("--surface-low"), foreground: v("--on-surface"), cursor: v("--primary"), cursorAccent: v("--surface-low"),
    selectionBackground: /^#[0-9a-f]{6}$/i.test(v("--primary")) ? `${v("--primary")}59` : "#ffffff40",
    black: v("--surface-highest"), red: v("--red"), green: v("--green"), yellow: v("--gold"), blue: v("--blue"), magenta: v("--magenta"), cyan: v("--cyan"), white: v("--on-surface-variant"),
    brightBlack: v("--outline"), brightRed: v("--red"), brightGreen: v("--green"), brightYellow: v("--gold"), brightBlue: v("--blue"), brightMagenta: v("--magenta"), brightCyan: v("--cyan"), brightWhite: v("--on-surface"),
  };
}

class Terminal {
  el = h("div.pane.terminal");
  private host = h("div.term-host");
  private label = h("span.pv-path");
  private terms = new Map<string, { term: XTerm; fit: FitAddon; el: HTMLElement; alive: boolean; cwd: string }>();
  private tab: Tab | null = null;

  constructor() {
    this.el.append(
      h("div.pbar", this.label,
        h("button.pb", { title: "quote the selection into the chat", onclick: () => this.quoteSelection() }, icon("copy"), h("span", "to chat")),
        h("button.pb", { title: "new shell", onclick: () => { if (this.tab) this.restart(this.tab); } }, icon("refresh"))),
      this.host,
    );
    api.on("term:data", (id: string, d: string) => this.terms.get(id)?.term.write(d));
    api.on("term:exit", (id: string, code: number | null) => {
      const x = this.terms.get(id);
      if (!x) return;
      x.alive = false;
      x.term.write(`\r\n\x1b[2m[shell exited${code ? ` with ${code}` : ""}, press a key for a new one]\x1b[0m\r\n`);
    });
    api.on("theme", () => setTimeout(() => { for (const x of this.terms.values()) x.term.options.theme = termTheme(); }, 50));
    new ResizeObserver(() => this.fit()).observe(this.host);
  }

  private fit() {
    if (!this.tab || !this.el.classList.contains("on")) return;
    const x = this.terms.get(this.tab.id);
    if (!x || !x.el.offsetWidth) return;
    try { x.fit.fit(); } catch { return; }
    void api.termResize(this.tab.id, x.term.cols, x.term.rows);
  }

  private start(t: Tab) {
    const x = this.terms.get(t.id)!;
    x.alive = true;
    void api.termOpen(t.id, t.cwd, x.term.cols || 80, x.term.rows || 24);
  }

  private restart(t: Tab) {
    const x = this.terms.get(t.id);
    if (!x) return;
    void api.termClose(t.id);
    x.term.reset();
    this.start(t);
    x.term.focus();
  }

  private quoteSelection() {
    const t = this.tab, x = t && this.terms.get(t.id);
    const sel = x?.term.getSelection().trim();
    if (!t || !sel) return app.toast("select some terminal output first");
    act.quote(t, `terminal output:\n${sel}`, `terminal  ·  ${sel.split("\n").length} lines`);
  }

  show(t: Tab) {
    this.tab = t;
    swap(this.label, tilde(t.cwd));
    let x = this.terms.get(t.id);
    if (!x) {
      const el = h("div.term-box");
      this.host.append(el);
      const css = getComputedStyle(document.documentElement);
      const term = new XTerm({
        fontFamily: css.getPropertyValue("--mono").trim() || "monospace",
        fontSize: app.settings.fontSize,
        lineHeight: 1.15,
        cursorBlink: true,
        cursorStyle: "block",
        allowProposedApi: true,
        scrollback: 8000,
        theme: termTheme(),
      });
      const fit = new FitAddon();
      term.loadAddon(fit);
      term.open(el);
      x = { term, fit, el, alive: false, cwd: t.cwd };
      this.terms.set(t.id, x);
      const xx = x;
      term.onData((d) => { if (xx.alive) void api.termWrite(t.id, d); else { xx.term.reset(); this.start(t); } });
      requestAnimationFrame(() => { try { fit.fit(); } catch { /* not laid out yet */ } this.start(t); });
    }
    for (const [id, v] of this.terms) v.el.classList.toggle("on", id === t.id);
    requestAnimationFrame(() => { this.fit(); x!.term.focus(); });
  }

  drop(t: Tab) {
    const x = this.terms.get(t.id);
    if (!x) return;
    void api.termClose(t.id);
    x.term.dispose();
    x.el.remove();
    this.terms.delete(t.id);
  }
}

// ---------- the window ----------
export class Side {
  el: HTMLElement;
  private body = h("div.side-body");
  private tabsEl = h("div.legend.l.side-tabs");
  browser = new Browser();
  preview = new Preview(() => this.layout());
  changes = new Changes();
  files = new Files();
  agents = new Agents();
  terminal = new Terminal();
  private current: string | null = null;

  constructor() {
    this.el = h("div.win.side.closed", this.tabsEl, h("div.legend.r", h("button.legend-btn", { title: "close (ctrl+\\)", onclick: () => { if (app.active) this.close(app.active); } }, icon("close"))), this.body);
    this.body.append(this.browser.el, this.preview.el, this.changes.el, this.files.el, this.agents.el, this.terminal.el);
    act.openPane = (t, k) => this.open(t, k);
    act.togglePane = (t, k) => { if (t.pane === k && this.current === k) this.close(t); else this.open(t, k); };
    act.openFile = (t, p, line) => { this.files.openPath(t, p, line); this.open(t, "files"); };
    act.refreshDiff = (t) => { if (t === app.active && this.current === "diff") void this.changes.show(t); };
    act.previewHtml = async (t, html) => {
      const path = `${app.sys.imagesDir.replace(/\/images$/, "")}/scratch/snippet-${Date.now().toString(36)}.html`;
      await api.write(path, html);
      t.preview = { path, title: "code block" };
      this.open(t, "preview");
    };
    app.on("view", () => this.sync());
    app.on("tabs", () => this.sync());
    onTab((t, w) => { if (w === "settled" && t === app.active && this.current === "diff") void this.changes.show(t); });
    // the pane webviews are native and sit on top of everything, so they follow the layout and get out
    // of the way of anything floating
    new ResizeObserver(() => this.layout()).observe(this.body);
    addEventListener("resize", () => this.layout());
    const watch = new MutationObserver(() => this.layout());
    queueMicrotask(() => { const o = document.getElementById("overlay"); if (o) watch.observe(o, { childList: true }); });
    watch.observe(document.body, { attributes: true, attributeFilter: ["class"] });
  }

  private raf = 0;
  layout() {
    if (this.raf) return;
    this.raf = requestAnimationFrame(() => {
      this.raf = 0;
      const t = app.view === "chat" ? app.active : null;
      const covered = !!document.querySelector("#overlay .scrim, #overlay .ctx-menu");
      const slots: WebSlot[] = [];
      if (t && !covered && this.current === "browser") { const s = this.browser.slot(t); if (s) slots.push(s); }
      if (t && !covered && this.current === "preview") { const s = this.preview.slot(t); if (s) slots.push(s); }
      void api.webLayout(slots).catch((e) => console.error("web layout", e));
    });
  }

  open(t: Tab, kind: string) {
    t.pane = kind;
    if (t === app.active) this.sync();
  }

  close(t: Tab) { t.pane = null; this.sync(); }

  sync() {
    const t = app.view === "chat" ? app.active : null;
    const kind = t?.pane ?? null;
    this.el.classList.toggle("closed", !kind);
    document.body.classList.toggle("has-side", !!kind);
    swap(this.tabsEl, PANES.map((p) => h(`button.legend-btn${p.id === kind ? ".on" : ""}`, { title: `${p.label} (${p.keys})`, onclick: () => t && this.open(t, p.id) }, p.label,
      p.id === "agents" && this.agents.runningCount() ? h("span.count", String(this.agents.runningCount())) : null)));
    for (const p of [this.browser, this.preview, this.changes, this.files, this.agents, this.terminal]) p.el.classList.remove("on");
    this.current = kind;
    this.layout();
    if (!t || !kind) return;
    const pane = ({ browser: this.browser, preview: this.preview, diff: this.changes, files: this.files, agents: this.agents, terminal: this.terminal } as Record<string, { el: HTMLElement; show?: (t: Tab) => unknown }>)[kind];
    pane.el.classList.add("on");
    pane.show?.(t);
    if (kind === "agents") this.agents.draw();
  }

  drop(t: Tab) { this.browser.drop(t); this.preview.drop(t); this.terminal.drop(t); void api.webClose(t.id); }
}
