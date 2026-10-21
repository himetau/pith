// A session's chat: scrollback, then the dock (tasks, queued messages, prompts, the composer)
import { act } from "../actions";
import { app } from "../app";
import { runBuiltin } from "../commands";
import { api } from "../lib/api";
import { h, swap } from "../lib/dom";
import { dur, plural, tilde } from "../lib/fmt";
import { icon } from "../lib/icons";
import { onTab, type Img, type Tab } from "../tab";
import { askCard, type KeyHandler } from "./asks";
import { Composer } from "./composer";
import { todoList } from "./tools";
import { Transcript } from "./transcript";

const views = new Map<string, ChatView>();
export const chatFor = (t: Tab) => views.get(t.id) ?? new ChatView(t);
export const dropChat = (t: Tab) => { views.get(t.id)?.dispose(); views.delete(t.id); };

export class ChatView {
  el: HTMLElement;
  composer: Composer;
  private tx: Transcript;
  private scroll = h("div.chat-scroll");
  private live = h("div.live-line");
  private todos = h("div.todo-strip");
  private queue = h("div.queue");
  private widgetsA = h("div.widgets");
  private widgetsB = h("div.widgets");
  private asks = h("div.asks");
  private empty = h("div.chat-empty");
  private jump = h("button.jump", icon("down"), "new output");
  private stick = true;
  private raf = 0;
  private askKey: KeyHandler | null = null;
  private todosOpen = true;
  private off: () => void;

  constructor(private tab: Tab) {
    views.set(tab.id, this);
    this.tx = new Transcript(tab, (text, entryId) => void this.edit(text, entryId));
    this.composer = new Composer({ tab: () => this.tab, send: (t, i, how) => void this.send(t, i, how) });
    this.el = h("div.chat",
      this.scroll,
      this.jump,
      h("div.dock", this.todos, this.queue, this.widgetsA, this.asks, this.composer.el, this.widgetsB),
    );
    this.scroll.append(h("div.tx-wrap", this.empty, this.tx.el, this.live));
    this.scroll.addEventListener("scroll", () => {
      const s = this.scroll;
      this.stick = s.scrollHeight - s.scrollTop - s.clientHeight < 60;
      this.jump.classList.toggle("on", !this.stick && tab.running);
    });
    this.jump.onclick = () => { this.stick = true; this.toBottom(); };
    this.off = onTab((t, what) => { if (t === this.tab) this.on(what); });
    if (tab.draft) this.composer.setText(tab.draft);
    this.renderAll();
  }

  dispose() { this.off(); }

  private on(what: string) {
    if (what === "items") this.schedule();
    else if (what === "status") { this.schedule(); this.drawLive(); this.composer.refresh(); this.drawTodos(); }
    else if (what === "meta" || what === "stats") this.composer.refresh();
    else if (what === "asks") this.drawAsks();
    else if (what === "queue") this.drawQueue();
    else if (what === "widgets") this.drawWidgets();
    else if (what === "todos") this.drawTodos();
    else if (what === "draft") { this.composer.setText(this.tab.draft); this.composer.focus(); }
  }

  private schedule() {
    if (this.raf) return;
    this.raf = requestAnimationFrame(() => { this.raf = 0; this.drawItems(); });
  }

  renderAll() {
    this.drawItems();
    this.drawLive();
    this.drawAsks();
    this.drawQueue();
    this.drawWidgets();
    this.drawTodos();
    this.composer.refresh();
  }

  private drawItems() {
    const stick = this.stick;
    this.tx.update();
    swap(this.empty, this.tab.items.length ? null : emptyState(this.tab, (t) => this.composer.setText(t)));
    this.drawTodos();
    if (stick) this.toBottom();
    else this.jump.classList.toggle("on", this.tab.running);
  }

  toBottom() { this.scroll.scrollTop = this.scroll.scrollHeight; }

  private drawLive() {
    const t = this.tab;
    if (t.status === "starting") return swap(this.live, h("span", { "data-spin": "" }), " starting pi");
    if (t.watch) return swap(this.live, t.agentRunning
      ? [h("span.run-spin", { "data-spin": "" }), h("span.run-word", "subagent working"), h("span.dim", "·  live from its session")]
      : h("span.dim", "a subagent's finished run  ·  type to carry on with it"));
    if (t.status === "dead") return swap(this.live, h("span.bad", "pi stopped"), t.error ? h("pre.dead-err", t.error.slice(-1500)) : null, h("button.mini", { onclick: () => void t.ensure() }, "start again"));
    if (t.compacting) return swap(this.live, h("span", { "data-spin": "" }), " compacting context");
    if (t.retry) return swap(this.live, h("span.warn", { "data-spin": "" }), ` retry ${t.retry.attempt}/${t.retry.max} · `, h("span.dim", t.retry.error.slice(0, 120)));
    if (t.running) return swap(this.live, h("span.run-spin", { "data-spin": "" }), h("span.run-word", "working"), h("span.dim", { "data-since": String(t.runStart || Date.now()) }), h("span.dim", "·  esc to stop"));
    swap(this.live);
  }

  private drawAsks() {
    const a = this.tab.asks;
    if (!a.length) { swap(this.asks); this.askKey = null; return; }
    const cards = a.map((x) => askCard(x, this.tab));
    swap(this.asks, cards.map((c) => c.el));
    this.askKey = cards[0].key;
    if (this.stick) requestAnimationFrame(() => this.toBottom());
  }

  // number keys and y/n go to the top prompt while the composer is empty
  handleKey(e: KeyboardEvent): boolean {
    if (!this.askKey) return false;
    const inField = e.target instanceof HTMLInputElement || (e.target instanceof HTMLTextAreaElement && e.target !== this.composer.ta);
    if (inField || (e.target === this.composer.ta && this.composer.ta.value)) return false;
    if (e.ctrlKey || e.altKey || e.metaKey) return false;
    return this.askKey(e);
  }

  private drawQueue() {
    const q = this.tab.queue;
    const all = [...q.steering.map((s) => ({ s, k: "next" })), ...q.followUp.map((s) => ({ s, k: "after" }))];
    swap(this.queue, all.map(({ s, k }) => h("div.qchip", h("span.qk", k), h("span.qt", s), h("button", { title: "take it back", onclick: () => void this.tab.unqueue(s) }, icon("close")))));
  }

  private drawWidgets() {
    const w = [...this.tab.widgets.values()];
    swap(this.widgetsA, w.filter((x) => x.placement !== "belowEditor").map((x) => h("pre.widget", x.lines.join("\n"))));
    swap(this.widgetsB, w.filter((x) => x.placement === "belowEditor").map((x) => h("pre.widget", x.lines.join("\n"))));
  }

  private drawTodos() {
    const todos = this.tab.todos;
    const done = todos.filter((t) => t.status === "completed").length;
    if (!todos.length || (done === todos.length && !this.tab.running)) return swap(this.todos);
    const cur = todos.find((t) => t.status === "in_progress");
    swap(this.todos,
      h("button.ts-head", { onclick: () => { this.todosOpen = !this.todosOpen; this.drawTodos(); } },
        h("span.ts-bar", todos.map((t) => h(`i.${t.status}`))), h("span", `${done}/${todos.length}`), cur ? h("span.dim", ` · ${cur.content}`) : null, icon(this.todosOpen ? "down" : "chevron")),
      this.todosOpen ? todoList(todos) : null,
    );
  }

  async send(text: string, images: Img[], how: "auto" | "steer" | "followUp") {
    this.stick = true;
    if (text.startsWith("!") && !images.length) {
      const cmd = text.replace(/^!!?/, "").trim();
      if (cmd) void this.tab.bash(cmd, text.startsWith("!!"));
      return;
    }
    if (text.startsWith("/") && (await runBuiltin(this.tab, text))) return;
    void this.tab.send(text, images, how);
    app.emit("tabs");
  }

  private async edit(text: string, entryId: string) {
    if (this.tab.running) { app.toast("stop the run first, then branch"); return; }
    const t = await this.tab.fork(entryId);
    if (t == null) return;
    this.composer.setText(t || text);
    this.composer.focus();
    app.toast("branched into a new session · edit and send");
    void app.refreshSessions();
  }
}

function emptyState(tab: Tab, fill: (t: string) => void) {
  const starters = tab.kind === "design"
    ? [["/design ", "a landing page for…"], ["", "a settings screen for a music app, dark"], ["/image ", "an app icon for…"]]
    : [["/review", ""], ["/explain ", "how … works"], ["/init", ""], ["/simplify", ""]];
  return h("div.empty-inner",
    h("div.empty-k", tab.kind === "design" ? "design session" : "new session"),
    h("div.empty-cwd", tilde(tab.cwd)),
    h("div.empty-starters", starters.map(([cmd, rest]) => h("button.starter", { onclick: () => fill(cmd + (rest.includes("…") ? "" : rest)) }, h("span.accent", cmd || "❯ "), h("span.dim", rest)))),
    h("div.empty-keys", h("span", h("kbd", "shift+tab"), " mode"), h("span", h("kbd", "ctrl+k"), " commands"), h("span", h("kbd", "@"), " files"), h("span", h("kbd", "!"), " shell")),
  );
}

// spinners and elapsed timers tick on one shared clock
let spinI = 0;
setInterval(() => {
  spinI++;
  const f = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"[spinI % 10];
  for (const el of document.querySelectorAll<HTMLElement>("[data-spin]")) el.textContent = f;
  if (spinI % 6) return;
  for (const el of document.querySelectorAll<HTMLElement>("[data-since]")) el.textContent = dur(Date.now() - Number(el.dataset.since));
}, 80);

// a finished run in the background: unread dot, and a notification when the window isn't looked at
onTab((t, what) => {
  if (what !== "settled") return;
  const bg = app.active !== t || app.view !== "chat";
  if (bg) { t.unread = true; app.emit("tabs"); }
  if (app.settings.notify) {
    const last = [...t.items].reverse().find((i) => i.k === "assistant");
    const text = last && last.k === "assistant" ? last.blocks.filter((b) => b?.t === "text").map((b) => (b as { text: string }).text).join(" ") : "";
    const asks = t.asks.length ? ` · ${plural(t.asks.length, "question")} waiting` : "";
    void api.notify(t.title, (text.slice(0, 180) || "finished") + asks);
  }
  void app.refreshSessions();
});
onTab((t, what) => {
  if (what === "asks" && t.asks.length && app.settings.notify) void api.notify(t.title, "needs your answer");
  if (what.startsWith("toast:")) { const [, level, ...rest] = what.split(":"); app.toast(rest.join(":"), level); }
  if (what === "tool:preview" || what === "tool:browser") act.openPane(t, what.slice(5));
  if (what === "tooldone:edit" || what === "tooldone:write") act.refreshDiff(t);
});
