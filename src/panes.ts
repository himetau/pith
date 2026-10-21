// The window to the right of the chat. One per session: web, preview, changes, files, agents
import { act } from "./actions";
import { app } from "./app";
import { api } from "./lib/api";
import { debounce, h, swap } from "./lib/dom";
import { ago, base, bytes, cost, dur, tilde, tokens } from "./lib/fmt";
import { icon } from "./lib/icons";
import { highlight, langOf } from "./lib/md";
import { onTab, type Tab } from "./tab";
import { fuzzy } from "./views/composer";

export const PANES = [
  { id: "browser", label: "web", ic: "globe", keys: "alt+1" },
  { id: "preview", label: "preview", ic: "frame", keys: "alt+2" },
  { id: "diff", label: "changes", ic: "diff", keys: "alt+3" },
  { id: "files", label: "files", ic: "file", keys: "alt+4" },
  { id: "agents", label: "agents", ic: "agents", keys: "alt+5" },
] as const;

const abs = (tab: Tab, p: string) => (p.startsWith("/") ? p : p.startsWith("~") ? app.sys.home + p.slice(1) : `${tab.cwd.replace(/\/$/, "")}/${p.replace(/^\.\//, "")}`);

// ---------- web ----------
class Browser {
  el = h("div.pane.browser");
  private host = h("div.wv-host");
  private url = h("input.url", { spellcheck: false }) as HTMLInputElement;
  private views = new Map<string, Electron.WebviewTag>();
  private badge = h("span.agent-badge", h("span", { "data-spin": "" }), " agent is driving");
  private tab: Tab | null = null;

  constructor() {
    this.el.append(
      h("div.pbar",
        h("button.pb", { title: "back", onclick: () => this.wv()?.canGoBack() && this.wv()!.goBack() }, icon("back")),
        h("button.pb", { title: "forward", onclick: () => this.wv()?.canGoForward() && this.wv()!.goForward() }, icon("fwd")),
        h("button.pb", { title: "reload", onclick: () => this.wv()?.reload() }, icon("refresh")),
        this.url,
        this.badge,
        h("button.pb", { title: "open in your browser", onclick: () => { const u = this.wv()?.getURL(); if (u) void api.openExternal(u); } }, icon("external")),
      ),
      this.host,
    );
    this.url.addEventListener("keydown", (e) => {
      if (e.key !== "Enter") return;
      let u = this.url.value.trim();
      if (!u) return;
      if (!/^[a-z]+:/i.test(u)) u = /\s/.test(u) || !u.includes(".") ? `https://duckduckgo.com/?q=${encodeURIComponent(u)}` : `https://${u}`;
      void this.wv()?.loadURL(u);
      this.wv()?.focus();
    });
    onTab((t, w) => {
      if (w === "tool:browser") this.view(t)?.classList.add("driving");
      if (w === "tooldone:browser" || w === "settled") this.view(t)?.classList.remove("driving");
      if (t === this.tab) this.badge.classList.toggle("on", !!this.view(t)?.classList.contains("driving"));
    });
  }

  private wv() { return this.tab ? this.views.get(this.tab.id) : undefined; }
  private view(t: Tab) { return this.views.get(t.id); }

  // every session keeps its own page alive while the pane shows another
  ensure(t: Tab) {
    let wv = this.views.get(t.id);
    if (wv) return wv;
    wv = document.createElement("webview") as Electron.WebviewTag;
    wv.setAttribute("partition", "persist:pith-web");
    wv.setAttribute("allowpopups", "");
    wv.setAttribute("src", t.browserUrl || app.settings.browserHome);
    let attached = false;
    wv.addEventListener("dom-ready", () => {
      if (!attached) { attached = true; void api.attachBrowser(t.id, wv!.getWebContentsId()); }
    });
    const sync = () => { t.browserUrl = wv!.getURL(); if (this.tab === t && document.activeElement !== this.url) this.url.value = t.browserUrl; };
    wv.addEventListener("did-navigate", sync);
    wv.addEventListener("did-navigate-in-page", sync);
    wv.addEventListener("did-start-loading", () => wv!.classList.add("loading"));
    wv.addEventListener("did-stop-loading", () => { wv!.classList.remove("loading"); sync(); });
    this.views.set(t.id, wv);
    this.host.append(wv);
    return wv;
  }

  show(t: Tab) {
    this.tab = t;
    const wv = this.ensure(t);
    for (const [id, v] of this.views) v.classList.toggle("on", id === t.id);
    if (t.browserUrl && wv.getAttribute("src") !== t.browserUrl) { try { if (wv.getURL() !== t.browserUrl) void wv.loadURL(t.browserUrl); } catch { /* not attached yet */ } }
    try { this.url.value = wv.getURL() || t.browserUrl; } catch { this.url.value = t.browserUrl; }
    this.badge.classList.toggle("on", wv.classList.contains("driving"));
  }

  drop(t: Tab) { this.views.get(t.id)?.remove(); this.views.delete(t.id); }
}

// ---------- preview ----------
const VIEWPORTS = [{ id: "fill", ic: "desktop", w: 0 }, { id: "tablet", ic: "tablet", w: 820 }, { id: "phone", ic: "phone", w: 390 }];
class Preview {
  el = h("div.pane.preview");
  private stage = h("div.pv-stage");
  private label = h("span.pv-path");
  private pickBtn = h("button.pb", { title: "click an element to comment on it" }, icon("pick"), h("span", "comment"));
  private vpBtns = h("div.pv-vps");
  private empty = h("div.pane-empty");
  private views = new Map<string, Electron.WebviewTag>();
  private tab: Tab | null = null;
  private vp = new Map<string, string>();

  constructor() {
    this.el.append(
      h("div.pbar", this.label, this.vpBtns,
        this.pickBtn,
        h("button.pb", { title: "reload", onclick: () => this.wv()?.reload() }, icon("refresh")),
        h("button.pb", { title: "open html file", onclick: () => void this.openFile() }, icon("folder")),
        h("button.pb", { title: "open in your browser", onclick: () => { const p = this.tab?.preview; if (p) void api.openExternal(p.url ?? `file://${p.path}`); } }, icon("external")),
      ),
      this.stage, this.empty,
    );
    this.pickBtn.onclick = () => {
      const on = !this.pickBtn.classList.contains("on");
      this.pickBtn.classList.toggle("on", on);
      this.wv()?.send("pick", on);
    };
    api.on("preview:changed", debounce((tab: string) => { this.views.get(tab)?.reload(); }, 150));
    api.on("preview:open", (tabId: string, args: { path?: string; url?: string; title?: string }) => {
      const t = app.tabs.get(tabId);
      if (!t) return;
      t.preview = args;
      act.openPane(t, "preview");
    });
  }

  private wv() { return this.tab ? this.views.get(this.tab.id) : undefined; }

  private async openFile() {
    if (!this.tab) return;
    const [f] = await api.pickFiles();
    if (!f) return;
    this.tab.preview = { path: f };
    this.show(this.tab);
  }

  show(t: Tab) {
    this.tab = t;
    const p = t.preview;
    for (const [id, v] of this.views) v.classList.toggle("on", id === t.id && !!p);
    this.empty.hidden = !!p;
    swap(this.empty, h("div", h("div.pe-t", "nothing to preview"), h("div.dim", "ask for a design, use /design, or open an html file"), h("button.btn", { onclick: () => void this.openFile() }, icon("folder"), "open file")));
    swap(this.vpBtns, VIEWPORTS.map((v) => h(`button.pb${(this.vp.get(t.id) ?? "fill") === v.id ? ".on" : ""}`, { title: v.id, onclick: () => { this.vp.set(t.id, v.id); this.show(t); } }, icon(v.ic))));
    if (!p) { swap(this.label); return; }
    const src = p.url ?? `file://${p.path}`;
    swap(this.label, p.title ?? (p.path ? tilde(p.path) : p.url));
    let wv = this.views.get(t.id);
    if (!wv) {
      wv = document.createElement("webview") as Electron.WebviewTag;
      wv.setAttribute("partition", "pith-preview");
      wv.addEventListener("ipc-message", (e) => {
        if (e.channel === "pick") {
          const d = e.args[0] as { selector: string; html: string; text: string };
          const tag = (d.html.match(/^<(\w+)/) ?? [])[1] ?? "element";
          const words = d.text.trim().replace(/\s+/g, " ");
          act.quote(t, `${t.preview?.path ? tilde(t.preview.path) : "preview"} · ${d.selector}\n${d.html}`, `<${tag}>${words ? ` “${words.slice(0, 48)}${words.length > 48 ? "…" : ""}”` : ""}  ·  ${d.selector}`);
        }
        if (e.channel === "pick-off") this.pickBtn.classList.remove("on");
      });
      wv.addEventListener("dom-ready", () => { wv!.dataset.ready = "1"; });
      wv.addEventListener("did-finish-load", () => { if (this.pickBtn.classList.contains("on") && this.tab === t) wv!.send("pick", true); });
      this.views.set(t.id, wv);
      this.stage.append(wv);
    }
    wv.classList.add("on");
    if (wv.dataset.src !== src) {
      // src only counts before the guest exists, after that it has to be a navigation
      if (wv.dataset.ready) void wv.loadURL(src).catch(() => {}); else wv.setAttribute("src", src);
      wv.dataset.src = src;
      if (p.path) void api.watchPreview(t.id, p.path);
    }
    const vp = VIEWPORTS.find((v) => v.id === (this.vp.get(t.id) ?? "fill"))!;
    wv.style.width = vp.w ? `${vp.w}px` : "100%";
    this.stage.classList.toggle("framed", !!vp.w);
  }

  drop(t: Tab) { this.views.get(t.id)?.remove(); this.views.delete(t.id); void api.unwatchPreview(t.id); }
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

// ---------- the window ----------
export class Side {
  el: HTMLElement;
  private body = h("div.side-body");
  private tabsEl = h("div.legend.l.side-tabs");
  browser = new Browser();
  preview = new Preview();
  changes = new Changes();
  files = new Files();
  agents = new Agents();
  private current: string | null = null;

  constructor() {
    this.el = h("div.win.side.closed", this.tabsEl, h("div.legend.r", h("button.legend-btn", { title: "close (ctrl+\\)", onclick: () => { if (app.active) this.close(app.active); } }, icon("close"))), this.body);
    this.body.append(this.browser.el, this.preview.el, this.changes.el, this.files.el, this.agents.el);
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
    for (const p of [this.browser, this.preview, this.changes, this.files, this.agents]) p.el.classList.remove("on");
    this.current = kind;
    if (!t || !kind) return;
    const pane = ({ browser: this.browser, preview: this.preview, diff: this.changes, files: this.files, agents: this.agents } as Record<string, { el: HTMLElement; show?: (t: Tab) => unknown }>)[kind];
    pane.el.classList.add("on");
    pane.show?.(t);
    if (kind === "agents") this.agents.draw();
  }

  drop(t: Tab) { this.browser.drop(t); this.preview.drop(t); }
}
