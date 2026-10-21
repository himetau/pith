// The input box. Its border carries the mode on the left and model · effort on the right, like a tui box
import { act, MODES } from "../actions";
import { app } from "../app";
import { commandsFor, type Cmd } from "../commands";
import { api } from "../lib/api";
import { autosize, h, swap } from "../lib/dom";
import { tokens } from "../lib/fmt";
import { icon } from "../lib/icons";
import type { Img, Tab } from "../tab";

type Menu = { kind: "slash" | "at"; items: { label: string; hint: string; insert: string; src?: string }[]; sel: number; start: number };

const fileCache = new Map<string, { at: number; files: string[] }>();
async function filesIn(cwd: string) {
  const c = fileCache.get(cwd);
  if (c && Date.now() - c.at < 30000) return c.files;
  const files = await api.listFiles(cwd);
  fileCache.set(cwd, { at: Date.now(), files });
  return files;
}

// subsequence match, rewarding hits at the start and after separators
export function fuzzy(q: string, s: string): number {
  if (!q) return 1;
  q = q.toLowerCase();
  const t = s.toLowerCase();
  let score = 0, j = 0, run = 0;
  for (let i = 0; i < t.length && j < q.length; i++) {
    if (t[i] === q[j]) {
      j++;
      run++;
      score += 1 + run * 2 + (i === 0 || "/-_. ".includes(t[i - 1]) ? 6 : 0);
    } else run = 0;
  }
  if (j < q.length) return 0;
  return score - t.length * 0.02 + (t.includes(q) ? 12 : 0);
}

export class Composer {
  el: HTMLElement;
  ta = h("textarea.ta", { rows: 1, spellcheck: true }) as HTMLTextAreaElement;
  images: Img[] = [];
  quotes: { text: string; label?: string }[] = [];
  private chips = h("div.chips");
  private menuEl = h("div.menu");
  private menu: Menu | null = null;
  private lmode = h("button.legend-btn.mode");
  private lmodel = h("button.legend-btn");
  private ctx = h("div.ctx");
  private sendBtn = h("button.send");
  private hist: string[] = [];
  private histAt = -1;

  constructor(private o: { tab: () => Tab | null; send: (text: string, images: Img[], how: "auto" | "steer" | "followUp") => void; placeholder?: string; cwd?: () => string; home?: boolean }) {
    this.el = h("div.composer",
      h("div.legend.l", this.lmode),
      h("div.legend.r", this.lmodel),
      this.menuEl,
      this.chips,
      h("div.ta-row", this.ta),
      h("div.c-foot",
        h("button.cbtn", { title: "attach images or files", onclick: () => void this.pick() }, icon("clip")),
        h("span.c-hint"),
        this.ctx,
        this.sendBtn,
      ),
    );
    this.ta.placeholder = o.placeholder ?? "what should we do  ·  / commands  ·  @ files  ·  ! shell";
    this.ta.addEventListener("input", () => { autosize(this.ta); this.updateMenu(); this.updateShell(); const t = this.o.tab(); if (t) t.draft = this.ta.value; });
    this.ta.addEventListener("keydown", (e) => this.key(e));
    // a width change (window, side pane, scale) rewraps the text, so the height has to follow. only width: autosize changes the height
    let w = 0;
    new ResizeObserver(() => { if (this.ta.clientWidth !== w) { w = this.ta.clientWidth; autosize(this.ta); } }).observe(this.ta);
    this.ta.addEventListener("paste", (e) => this.paste(e));
    this.ta.addEventListener("blur", () => setTimeout(() => this.closeMenu(), 150));
    this.el.addEventListener("dragover", (e) => { e.preventDefault(); this.el.classList.add("drop"); });
    this.el.addEventListener("dragleave", () => this.el.classList.remove("drop"));
    this.el.addEventListener("drop", (e) => this.drop(e));
    this.lmode.onclick = () => { const t = this.o.tab(); if (t) act.pickMode(t); else this.cycleHomeMode(); };
    this.lmodel.onclick = () => { const t = this.o.tab(); if (t) act.pickModel(t); };
    this.sendBtn.onclick = () => { const t = this.o.tab(); if (t?.running && !this.ta.value.trim() && !this.images.length) void t.abort(); else this.submit("auto"); };
    this.refresh();
  }

  private homeMode = app.settings?.mode ?? "ask";
  get pendingMode() { return this.homeMode; }
  private cycleHomeMode() {
    const i = MODES.findIndex((m) => m.id === this.homeMode);
    this.homeMode = MODES[(i + 1) % MODES.length].id;
    this.refresh();
  }

  focus() { this.ta.focus(); }
  setText(t: string) { this.ta.value = t; autosize(this.ta); this.updateShell(); this.ta.setSelectionRange(t.length, t.length); }
  addImage(img: Img) { this.images.push(img); this.renderChips(); }
  addQuote(text: string, label?: string) { this.quotes.push({ text, label }); this.renderChips(); this.focus(); }

  refresh() {
    const t = this.o.tab();
    const mode = t?.mode ?? this.homeMode;
    this.lmode.className = `legend-btn mode m-${mode}`;
    swap(this.lmode, this.ta.value.startsWith("!") ? "$ shell" : `❯ ${mode}`);
    const model = t?.model;
    const lvl = t?.state.thinkingLevel;
    swap(this.lmodel, model ? `${model.name ?? model.id}${lvl && model.reasoning ? `  ·  ${lvl}` : ""}` : t ? "…" : "");
    const running = !!t?.running;
    this.el.classList.toggle("running", running);
    swap(this.sendBtn, running && !this.ta.value.trim() && !this.images.length ? icon("stop") : icon("up"));
    this.sendBtn.title = running ? (this.ta.value.trim() ? "steer: send now, between tool calls (enter)  ·  alt+enter queues for after" : "stop (esc)") : "send (enter)";
    const hint = this.el.querySelector(".c-hint")!;
    swap(hint, running ? "enter steers · alt+enter queues · esc stops" : "");
    const cu = t?.stats?.contextUsage;
    if (cu?.contextWindow) {
      const pct = cu.percent ?? 0;
      const cells = 12, on = Math.round((pct / 100) * cells);
      swap(this.ctx, h("span.cells", Array.from({ length: cells }, (_, i) => h(`i${i < on ? ".on" : ""}${pct > 80 && i < on ? ".hot" : ""}`))), h("span", cu.tokens == null ? "—" : `${Math.round(pct)}%`));
      this.ctx.title = `context ${cu.tokens == null ? "unknown until the next reply" : tokens(cu.tokens)} of ${tokens(cu.contextWindow)}`;
    } else swap(this.ctx);
  }

  private updateShell() {
    this.el.classList.toggle("shell", this.ta.value.startsWith("!"));
    this.refresh();
  }

  private renderChips() {
    swap(this.chips,
      this.quotes.map((q, i) => h("div.chip.quote", h("span.q", q.label ?? (q.text.length > 140 ? q.text.slice(0, 140) + "…" : q.text)), h("button", { onclick: () => { this.quotes.splice(i, 1); this.renderChips(); } }, icon("close")))),
      this.images.map((im, i) => h("div.chip.img", h("img", { src: `data:${im.mimeType};base64,${im.data}` }), h("button", { onclick: () => { this.images.splice(i, 1); this.renderChips(); } }, icon("close")))),
    );
  }

  private async pick() {
    const files = await api.pickFiles();
    for (const f of files) await this.attachPath(f);
  }

  async addPaths(paths: string[]) { for (const p of paths) await this.attachPath(p); this.focus(); }

  private async attachPath(p: string) {
    const r = await api.read(p);
    if (r.kind === "image") this.addImage({ data: r.data, mimeType: r.mimeType });
    else this.insertAt(`@${p} `);
  }

  private insertAt(s: string) {
    const { selectionStart: a, selectionEnd: b, value } = this.ta;
    this.ta.value = value.slice(0, a) + s + value.slice(b);
    this.ta.setSelectionRange(a + s.length, a + s.length);
    autosize(this.ta);
  }

  private paste(e: ClipboardEvent) {
    const items = [...(e.clipboardData?.items ?? [])].filter((i) => i.type.startsWith("image/"));
    if (!items.length) return;
    e.preventDefault();
    for (const it of items) {
      const f = it.getAsFile();
      if (f) void this.fileToImg(f).then((im) => this.addImage(im));
    }
  }

  private async drop(e: DragEvent) {
    e.preventDefault();
    this.el.classList.remove("drop");
    for (const f of [...(e.dataTransfer?.files ?? [])]) {
      const path = (f as File & { path?: string }).path;
      if (f.type.startsWith("image/")) this.addImage(await this.fileToImg(f));
      else if (path) this.insertAt(`@${path} `);
    }
  }

  private fileToImg(f: File): Promise<Img> {
    return new Promise((resolve) => {
      const r = new FileReader();
      r.onload = () => { const s = String(r.result); resolve({ mimeType: f.type || "image/png", data: s.slice(s.indexOf(",") + 1) }); };
      r.readAsDataURL(f);
    });
  }

  // ---------- menus ----------
  private async updateMenu() {
    const v = this.ta.value, pos = this.ta.selectionStart;
    const before = v.slice(0, pos);
    const slash = before.match(/^\/(\S*)$/);
    if (slash) {
      const q = slash[1];
      const cmds = commandsFor(this.o.tab()).map((c) => ({ c, s: fuzzy(q, c.name) })).filter((x) => x.s > 0).sort((a, b) => b.s - a.s).slice(0, 12);
      this.menu = { kind: "slash", start: 0, sel: 0, items: cmds.map(({ c }) => ({ label: `/${c.name}`, hint: c.desc + (c.arg ? `  ${c.arg}` : ""), insert: `/${c.name} `, src: srcLabel(c) })) };
      return this.drawMenu();
    }
    const at = before.match(/(?:^|\s)@([^\s@]*)$/);
    const cwd = this.o.tab()?.cwd ?? this.o.cwd?.();
    if (at && cwd) {
      const q = at[1], start = pos - q.length - 1;
      const files = await filesIn(cwd);
      if (this.ta.value.slice(0, this.ta.selectionStart) !== before) return;
      const hits = files.map((f) => ({ f, s: fuzzy(q, f) })).filter((x) => x.s > 0).sort((a, b) => b.s - a.s).slice(0, 12);
      this.menu = { kind: "at", start, sel: 0, items: hits.map(({ f }) => ({ label: f, hint: "", insert: `@${f} ` })) };
      return this.drawMenu();
    }
    this.closeMenu();
  }

  private drawMenu() {
    const m = this.menu;
    if (!m || !m.items.length) return this.closeMenu();
    this.menuEl.classList.add("on");
    swap(this.menuEl, m.items.map((it, i) => h(`div.mi${i === m.sel ? ".sel" : ""}`, { onmousedown: (e: Event) => { e.preventDefault(); m.sel = i; this.accept(); } },
      h("span.ml", it.label), it.src ? h("span.msrc", it.src) : null, h("span.mh", it.hint))));
    this.menuEl.querySelector(".sel")?.scrollIntoView({ block: "nearest" });
  }

  private closeMenu() { this.menu = null; this.menuEl.classList.remove("on"); }

  private accept() {
    const m = this.menu!;
    const it = m.items[m.sel];
    const v = this.ta.value, pos = this.ta.selectionStart;
    if (m.kind === "slash") this.ta.value = it.insert + v.slice(pos).replace(/^\S*\s?/, "");
    else this.ta.value = v.slice(0, m.start) + it.insert + v.slice(pos);
    const at = m.kind === "slash" ? it.insert.length : m.start + it.insert.length;
    this.ta.setSelectionRange(at, at);
    this.closeMenu();
    autosize(this.ta);
    this.updateShell();
  }

  private key(e: KeyboardEvent) {
    const m = this.menu;
    if (m) {
      if (e.key === "ArrowDown") { e.preventDefault(); m.sel = (m.sel + 1) % m.items.length; return this.drawMenu(); }
      if (e.key === "ArrowUp") { e.preventDefault(); m.sel = (m.sel - 1 + m.items.length) % m.items.length; return this.drawMenu(); }
      if (e.key === "Tab" || (e.key === "Enter" && !e.shiftKey && m.kind === "at")) { e.preventDefault(); return this.accept(); }
      if (e.key === "Enter" && !e.shiftKey && m.kind === "slash") {
        const it = m.items[m.sel];
        if (this.ta.value.trim() !== it.insert.trim()) { e.preventDefault(); return this.accept(); }
      }
      if (e.key === "Escape") { e.preventDefault(); return this.closeMenu(); }
    }
    const t = this.o.tab();
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      this.submit(e.altKey ? "followUp" : "auto");
      return;
    }
    if (e.key === "Escape" && t?.running) { e.preventDefault(); void t.abort(); return; }
    if (e.key === "Tab" && e.shiftKey) { e.preventDefault(); if (t) { const i = MODES.findIndex((x) => x.id === t.mode); t.setMode(MODES[(i + 1) % MODES.length].id); } else this.cycleHomeMode(); return; }
    if (e.key === "ArrowUp" && !this.ta.value && this.hist.length) { e.preventDefault(); this.histAt = Math.min(this.histAt + 1, this.hist.length - 1); this.setText(this.hist[this.hist.length - 1 - this.histAt]); return; }
    if (e.key === "ArrowDown" && this.histAt >= 0 && this.ta.selectionStart === this.ta.value.length) { e.preventDefault(); this.histAt--; this.setText(this.histAt >= 0 ? this.hist[this.hist.length - 1 - this.histAt] : ""); }
  }

  submit(how: "auto" | "steer" | "followUp") {
    let text = this.ta.value.trim();
    if (!text && !this.images.length && !this.quotes.length) return;
    if (this.quotes.length) text = this.quotes.map((q) => q.text.split("\n").map((l) => `> ${l}`).join("\n")).join("\n\n") + (text ? `\n\n${text}` : "");
    if (text) this.hist.push(this.ta.value.trim());
    this.histAt = -1;
    const imgs = this.images;
    this.images = [];
    this.quotes = [];
    this.renderChips();
    this.setText("");
    const t = this.o.tab();
    if (t) t.draft = "";
    this.closeMenu();
    this.o.send(text, imgs, how);
  }
}

function srcLabel(c: Cmd) {
  return c.source === "app" ? "" : c.source === "prompt" ? "template" : c.source;
}
