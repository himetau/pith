// One open session: its pi process, the transcript rebuilt from pi's events, and what the chat shows around it
import { api, type Content, type Mode, type Model, type PiState, type Rec, type Resp, type Stats } from "./lib/api";
import { titleFrom } from "./lib/fmt";

export type Img = { data: string; mimeType: string };
export type ToolResult = { content: Content[]; details?: any; isError?: boolean };
export type ToolBlock = { t: "tool"; v: number; id: string; name: string; args: any; argsText: string; status: "streaming" | "queued" | "running" | "done" | "error"; result?: ToolResult; partial?: ToolResult; ms?: number; started?: number };
export type Block =
  | { t: "text"; v: number; text: string }
  | { t: "thinking"; v: number; text: string; redacted?: boolean; started?: number; ms?: number }
  | ToolBlock;

export type Item = { id: number; v: number } & (
  | { k: "user"; text: string; images: Img[]; entryId?: string; ts: number; typed?: string }
  | { k: "assistant"; blocks: (Block | undefined)[]; model?: string; usage?: any; stop?: string; error?: string; ts: number; live: boolean; runEnd?: { ms: number } }
  | { k: "bash"; command: string; output: string; exit?: number; running: boolean; cancelled?: boolean; reqId?: string; excluded?: boolean }
  | { k: "compaction"; summary: string; tokensBefore?: number }
  | { k: "branch"; summary: string }
  | { k: "custom"; type: string; text: string }
  | { k: "note"; level: "info" | "warn" | "error"; text: string }
);
type ItemOf<K extends Item["k"]> = Extract<Item, { k: K }>;

export type Ask = { id: string; method: "select" | "confirm" | "input" | "editor"; title: string; message?: string; options?: string[]; placeholder?: string; prefill?: string; timeout?: number; at: number };
export type Todo = { content: string; status: "pending" | "in_progress" | "completed" };
export type Status = "cold" | "starting" | "idle" | "running" | "dead";

let nextItem = 1;
let nextTab = 1;
const listeners = new Set<(t: Tab, what: string) => void>();
export const onTab = (fn: (t: Tab, what: string) => void) => { listeners.add(fn); return () => listeners.delete(fn); };

export class Tab {
  id = `t${Date.now().toString(36)}${nextTab++}`;
  status: Status = "cold";
  items: Item[] = [];
  state: PiState = {};
  levels: string[] = [];
  stats: Stats | null = null;
  queue = { steering: [] as string[], followUp: [] as string[] };
  asks: Ask[] = [];
  widgets = new Map<string, { lines: string[]; placement: string }>();
  statuses = new Map<string, string>();
  commands: { name: string; description?: string; source: string }[] = [];
  todos: Todo[] = [];
  unread = false;
  error = "";
  compacting = false;
  retry: { attempt: number; max: number; until: number; error: string } | null = null;
  runStart = 0;
  lastUsed = Date.now();
  // when you last sent something, which is what orders the sidebar. a new session counts from when it was made
  sent = 0;
  preview: { path?: string; url?: string; title?: string } | null = null;
  browserUrl = "";
  pane: string | null = null;
  draft = "";
  // a subagent's session: read off disk while its own process runs it, a normal session once you type in it
  watch = false;
  agentRunning = false;
  private peeking: Promise<void> | null = null;
  private peekAgain = false;
  private tools = new Map<string, ToolBlock>();
  // what the user typed for a /template, shown instead of the expansion pi echoes back
  private typed: string[] = [];
  private live: ItemOf<"assistant"> | null = null;
  private loaded = false;

  constructor(public cwd: string, public file: string | undefined, public kind: "chat" | "design", public mode: Mode, public name?: string) {
    if (!file) this.sent = Date.now();
  }

  get title() {
    if (this.state.sessionName || this.name) return (this.state.sessionName || this.name)!;
    const u = this.items.find((i) => i.k === "user") as ItemOf<"user"> | undefined;
    return u ? titleFrom(u.typed ?? u.text) : this.kind === "design" ? "new design" : "new session";
  }
  get running() { return this.status === "running"; }
  get model(): Model | undefined { return this.state.model; }

  emit(what: string) { for (const l of listeners) l(this, what); }
  private push<T extends Item>(it: Omit<T, "id" | "v">): T {
    const item = { ...it, id: nextItem++, v: 1 } as unknown as T;
    this.items.push(item);
    return item;
  }
  note(level: "info" | "warn" | "error", text: string) { this.push<ItemOf<"note">>({ k: "note", level, text }); this.emit("items"); }

  // ---------- process ----------
  async ensure(): Promise<boolean> {
    this.lastUsed = Date.now();
    if (this.status === "idle" || this.status === "running") return true;
    if (this.watch && this.agentRunning) { this.note("info", "this agent is still running in its own process. once it's done you can carry on here"); return false; }
    if (this.status === "starting") {
      await new Promise<void>((r) => { const off = onTab((t, w) => { if (t === this && w === "status" && this.status !== "starting") { off(); r(); } }); });
      return (this.status as Status) === "idle" || (this.status as Status) === "running";
    }
    this.status = "starting";
    this.error = "";
    this.emit("status");
    const resuming = !!this.file;
    const r = await api.start({ tab: this.id, cwd: this.cwd, session: this.file, kind: this.kind, mode: this.mode });
    if (!r.ok) {
      this.status = "dead";
      this.error = r.error || "pi didn't start";
      this.emit("status");
      return false;
    }
    this.state = r.state ?? {};
    this.file = this.state.sessionFile ?? this.file;
    this.saveMeta();
    // history goes in before anyone can send, a load landing after a live message would wipe it
    if (resuming && (!this.loaded || this.watch)) await this.load();
    this.watch = false;
    this.loaded = true;
    this.status = this.state.isStreaming ? "running" : "idle";
    this.emit("status");
    void this.afterStart();
    return true;
  }

  private async afterStart() {
    const [lv, cmds] = await Promise.all([this.cmd({ type: "get_available_thinking_levels" }), this.cmd({ type: "get_commands" })]);
    if (lv.success) this.levels = lv.data.levels;
    if (cmds.success) this.commands = cmds.data.commands;
    void this.refreshStats();
    this.emit("meta");
  }

  async stop() {
    await api.stop(this.id);
    this.status = "cold";
    this.emit("status");
  }

  cmd(c: Rec): Promise<Resp> { return api.cmd(this.id, c); }

  // ---------- history ----------
  // entries, not messages: messages stop at the last compaction, entries keep everything on the branch
  async load() {
    const r = await this.cmd({ type: "get_entries" });
    if (r.success) this.fill(r.data.entries, r.data.leafId);
  }

  // re-reads a watched session; calls that land mid-read get folded into one more read after it
  peek(): Promise<void> {
    if (!this.watch || !this.file) return Promise.resolve();
    if (this.peeking) { this.peekAgain = true; return this.peeking; }
    this.peeking = (async () => {
      do {
        this.peekAgain = false;
        const d = await api.agentSession(this.file!).catch(() => null);
        if (d && this.watch) this.fill(d.entries, d.leafId);
      } while (this.peekAgain);
      this.peeking = null;
    })();
    return this.peeking;
  }

  private fill(entries: any[], leafId: string | null) {
    const byId = new Map<string, any>();
    for (const e of entries) byId.set(e.id, e);
    const path: any[] = [];
    for (let id = leafId; id && byId.has(id); id = byId.get(id).parentId) path.push(byId.get(id));
    path.reverse();
    this.items = [];
    this.tools.clear();
    this.todos = [];
    for (const e of path) {
      if (e.type === "message") this.addMessage(e.message, e.id);
      else if (e.type === "compaction") this.push<ItemOf<"compaction">>({ k: "compaction", summary: e.summary, tokensBefore: e.tokensBefore });
      else if (e.type === "branch_summary") this.push<ItemOf<"branch">>({ k: "branch", summary: e.summary });
      else if (e.type === "custom_message" && e.display) this.push<ItemOf<"custom">>({ k: "custom", type: e.customType, text: textOf(e.content) });
    }
    // off disk there's no tool_execution_end to wait for, a call without a result is still going
    if (this.watch && this.agentRunning) for (const tb of this.tools.values()) if (!tb.result) tb.status = "running";
    this.loaded = true;
    this.emit("items");
  }

  private addMessage(m: any, entryId?: string) {
    switch (m.role) {
      case "user": {
        const { text, images } = splitContent(m.content);
        const queued = !entryId ? this.typed.shift() : undefined;
        const typed = queued && !text.startsWith("/") ? queued : undefined;
        this.push<ItemOf<"user">>({ k: "user", text, images, entryId, ts: m.timestamp, typed });
        break;
      }
      case "assistant": {
        const it = this.push<ItemOf<"assistant">>({ k: "assistant", blocks: [], ts: m.timestamp, live: false });
        this.fillAssistant(it, m);
        break;
      }
      case "toolResult":
        this.attachResult(m.toolCallId, { content: m.content, details: m.details, isError: m.isError }, m.durationMs);
        break;
      case "bashExecution":
        this.push<ItemOf<"bash">>({ k: "bash", command: m.command, output: m.output, exit: m.exitCode, running: false, cancelled: m.cancelled, excluded: m.excludeFromContext });
        break;
      case "custom":
        if (m.display) this.push<ItemOf<"custom">>({ k: "custom", type: m.customType, text: textOf(m.content) });
        break;
      case "compactionSummary":
        this.push<ItemOf<"compaction">>({ k: "compaction", summary: m.summary, tokensBefore: m.tokensBefore });
        break;
      case "branchSummary":
        this.push<ItemOf<"branch">>({ k: "branch", summary: m.summary });
        break;
    }
  }

  private fillAssistant(it: ItemOf<"assistant">, m: any) {
    const blocks: (Block | undefined)[] = [];
    for (const c of m.content ?? []) {
      if (c.type === "text") blocks.push({ t: "text", v: 1, text: c.text });
      else if (c.type === "thinking") {
        const prev = it.blocks[blocks.length];
        blocks.push({ t: "thinking", v: 1, text: c.thinking, redacted: c.redacted, ms: prev?.t === "thinking" ? prev.ms : undefined });
      } else if (c.type === "toolCall") {
        const known = this.tools.get(c.id);
        const tb: ToolBlock = known ?? { t: "tool", v: 1, id: c.id, name: c.name, args: c.arguments, argsText: "", status: it.live || m.stopReason === "pending" ? "queued" : "done" };
        tb.args = c.arguments;
        tb.name = c.name;
        tb.v++;
        this.tools.set(c.id, tb);
        blocks.push(tb);
        if (c.name === "todo_write" && Array.isArray(c.arguments?.todos)) this.todos = c.arguments.todos;
      }
    }
    it.blocks = blocks;
    it.model = m.model;
    it.usage = m.usage;
    it.stop = m.stopReason;
    it.error = m.errorMessage;
    it.ts = m.timestamp ?? it.ts;
    it.v++;
  }

  private attachResult(id: string, res: ToolResult, ms?: number) {
    const tb = this.tools.get(id);
    if (!tb) return;
    tb.result = res;
    tb.partial = undefined;
    tb.status = res.isError ? "error" : "done";
    if (ms != null) tb.ms = ms;
    tb.v++;
    this.bumpOwner(tb);
  }

  private bumpOwner(tb: ToolBlock) {
    for (let i = this.items.length - 1; i >= 0; i--) {
      const it = this.items[i];
      if (it.k === "assistant" && it.blocks.includes(tb)) { it.v++; return; }
    }
  }

  // ---------- live events ----------
  onEvent(r: Rec) {
    switch (r.type) {
      case "agent_start":
        if (this.status !== "running") { this.status = "running"; this.runStart = Date.now(); this.emit("status"); }
        break;
      case "message_start": {
        const m = r.message;
        if (m.role === "assistant") {
          this.live = this.push<ItemOf<"assistant">>({ k: "assistant", blocks: [], ts: m.timestamp ?? Date.now(), live: true });
        } else if (m.role !== "system" && m.role !== "toolResult") this.addMessage(m);
        this.emit("items");
        break;
      }
      case "message_update":
        if (this.live) this.onDelta(this.live, r.assistantMessageEvent, r.usage);
        break;
      case "message_end": {
        const m = r.message;
        if (m.role === "assistant" && this.live) {
          const thinkingMs = this.live.blocks.map((b) => (b?.t === "thinking" ? b.ms : undefined));
          this.fillAssistant(this.live, m);
          this.live.blocks.forEach((b, i) => { if (b?.t === "thinking" && thinkingMs[i]) b.ms = thinkingMs[i]; });
          this.live.live = false;
          this.live = null;
        } else if (m.role === "toolResult") this.attachResult(m.toolCallId, { content: m.content, details: m.details, isError: m.isError }, m.durationMs);
        this.emit("items");
        break;
      }
      case "tool_execution_start": {
        const tb = this.tools.get(r.toolCallId);
        if (tb) { tb.status = "running"; tb.started = Date.now(); tb.args = r.args ?? tb.args; tb.v++; this.bumpOwner(tb); }
        if (r.toolName === "todo_write" && Array.isArray(r.args?.todos)) { this.todos = r.args.todos; this.emit("todos"); }
        if (r.toolName === "preview") {
          const p: string | undefined = r.args?.path;
          this.preview = { path: p && !p.startsWith("/") ? `${this.cwd.replace(/\/$/, "")}/${p.replace(/^\.\//, "")}` : p, url: r.args?.url, title: r.args?.title };
        }
        this.emit("items");
        this.emit("tool:" + r.toolName);
        break;
      }
      case "tool_execution_update": {
        const tb = this.tools.get(r.toolCallId);
        if (tb) { tb.partial = r.partialResult; tb.v++; this.bumpOwner(tb); this.emit("items"); }
        break;
      }
      case "tool_execution_end": {
        const tb = this.tools.get(r.toolCallId);
        if (tb && !tb.result) { tb.result = r.result; tb.status = r.isError ? "error" : "done"; tb.ms = r.durationMs; tb.v++; this.bumpOwner(tb); }
        else if (tb) { tb.ms = r.durationMs ?? tb.ms; tb.status = r.isError ? "error" : "done"; }
        this.emit("items");
        this.emit("tooldone:" + r.toolName);
        break;
      }
      case "turn_end":
        void this.refreshStats();
        break;
      case "agent_settled": {
        this.status = "idle";
        this.retry = null;
        const last = [...this.items].reverse().find((i) => i.k === "assistant") as ItemOf<"assistant"> | undefined;
        if (last && this.runStart) { last.runEnd = { ms: Date.now() - this.runStart }; last.v++; }
        this.runStart = 0;
        for (const it of this.items) if (it.k === "assistant" && it.live) { it.live = false; it.v++; }
        // tool calls the abort cut off never get an end event
        for (const tb of this.tools.values()) if (tb.status === "running" || tb.status === "queued" || tb.status === "streaming") { tb.status = r.aborted ? "error" : "done"; tb.v++; this.bumpOwner(tb); }
        if (r.aborted) this.note("warn", "stopped");
        this.emit("status");
        this.emit("items");
        this.emit("settled");
        void this.refreshStats();
        void this.syncEntryIds();
        break;
      }
      case "queue_update":
        this.queue = { steering: r.steering ?? [], followUp: r.followUp ?? [] };
        this.emit("queue");
        break;
      case "compaction_start":
        this.compacting = true;
        this.emit("status");
        break;
      case "compaction_end":
        this.compacting = false;
        if (r.result) this.push<ItemOf<"compaction">>({ k: "compaction", summary: r.result.summary, tokensBefore: r.result.tokensBefore });
        else if (r.errorMessage) this.note("error", `compaction failed: ${r.errorMessage}`);
        this.emit("status");
        this.emit("items");
        void this.refreshStats();
        break;
      case "auto_retry_start":
        this.retry = { attempt: r.attempt, max: r.maxAttempts, until: Date.now() + r.delayMs, error: r.errorMessage };
        this.emit("status");
        break;
      case "auto_retry_end":
        this.retry = null;
        if (!r.success && r.finalError) this.note("error", r.finalError);
        this.emit("status");
        break;
      case "session_info_changed":
        this.state.sessionName = r.name;
        this.emit("meta");
        break;
      case "thinking_level_changed":
        this.state.thinkingLevel = r.level;
        this.emit("meta");
        break;
      case "bash_execution_update": {
        const b = this.items.find((i) => i.k === "bash" && i.reqId === r.id) as ItemOf<"bash"> | undefined;
        if (b) { b.output += r.delta; b.v++; this.emit("items"); }
        break;
      }
      case "extension_ui_request":
        this.onUi(r);
        break;
      case "extension_error":
        this.note("warn", `${r.extensionPath?.split("/").pop()}: ${r.error}`);
        break;
    }
  }

  private onDelta(it: ItemOf<"assistant">, e: any, usage: any) {
    const i = e.contentIndex;
    const b = i != null ? it.blocks[i] : undefined;
    switch (e.type) {
      case "text_start": it.blocks[i] = { t: "text", v: 1, text: "" }; break;
      case "text_delta": if (b?.t === "text") { b.text += e.delta; b.v++; } break;
      case "text_end": if (b?.t === "text" && typeof e.content === "string") { b.text = e.content; b.v++; } break;
      case "thinking_start": it.blocks[i] = { t: "thinking", v: 1, text: "", started: Date.now() }; break;
      case "thinking_delta": if (b?.t === "thinking") { b.text += e.delta; b.v++; } break;
      case "thinking_end":
        if (b?.t === "thinking") { if (typeof e.content === "string") b.text = e.content; b.ms = Date.now() - (b.started ?? Date.now()); b.v++; }
        break;
      case "toolcall_start": {
        const tb: ToolBlock = { t: "tool", v: 1, id: e.id, name: e.toolName, args: {}, argsText: "", status: "streaming" };
        this.tools.set(e.id, tb);
        it.blocks[i] = tb;
        break;
      }
      case "toolcall_delta": if (b?.t === "tool") { b.argsText += e.delta; b.v++; } break;
      case "toolcall_end":
        if (b?.t === "tool") { b.args = e.toolCall.arguments; b.status = b.status === "streaming" ? "queued" : b.status; b.v++; }
        break;
    }
    if (usage) it.usage = usage;
    it.v++;
    this.emit("items");
  }

  private onUi(r: Rec) {
    switch (r.method) {
      case "select": case "confirm": case "input": case "editor":
        this.asks.push({ id: r.id, method: r.method, title: r.title ?? "", message: r.message, options: r.options, placeholder: r.placeholder, prefill: r.prefill, timeout: r.timeout, at: Date.now() });
        this.emit("asks");
        if (r.timeout) setTimeout(() => { this.asks = this.asks.filter((a) => a.id !== r.id); this.emit("asks"); }, r.timeout);
        break;
      case "notify":
        this.emit(`toast:${r.notifyType ?? "info"}:${r.message}`);
        break;
      case "setStatus":
        if (r.statusText) this.statuses.set(r.statusKey, stripAnsi(r.statusText)); else this.statuses.delete(r.statusKey);
        this.emit("meta");
        break;
      case "setWidget":
        if (r.widgetLines) this.widgets.set(r.widgetKey, { lines: r.widgetLines.map(stripAnsi), placement: r.widgetPlacement ?? "aboveEditor" });
        else this.widgets.delete(r.widgetKey);
        this.emit("widgets");
        break;
      case "set_editor_text":
        this.draft = r.text ?? "";
        this.emit("draft");
        break;
    }
  }

  answer(id: string, payload: object) {
    this.asks = this.asks.filter((a) => a.id !== id);
    void api.ui(this.id, id, payload);
    this.emit("asks");
  }

  async refreshStats() {
    const r = await this.cmd({ type: "get_session_stats" });
    if (r.success) { this.stats = r.data; if (r.data.sessionFile) this.file = r.data.sessionFile; this.emit("stats"); }
  }

  // fork needs entry ids on user messages; live ones only get them once pi has written them
  private async syncEntryIds() {
    const r = await this.cmd({ type: "get_fork_messages" });
    if (!r.success) return;
    const users = this.items.filter((i) => i.k === "user") as ItemOf<"user">[];
    const fm: { entryId: string }[] = r.data.messages;
    const off = users.length - fm.length;
    fm.forEach((m, i) => { const u = users[i + off]; if (u && !u.entryId) u.entryId = m.entryId; });
  }

  // ---------- actions ----------
  async send(text: string, images: Img[] = [], how: "auto" | "steer" | "followUp" = "auto") {
    if (!(await this.ensure())) return;
    this.lastUsed = this.sent = Date.now();
    const imgs = images.map((i) => ({ type: "image", data: i.data, mimeType: i.mimeType }));
    const base = { message: text, ...(imgs.length ? { images: imgs } : {}) };
    const isTemplate = text.startsWith("/") && !this.commands.some((c) => c.source === "extension" && text.slice(1).split(/\s/)[0] === c.name);
    if (isTemplate) this.typed.push(text);
    let r: Resp;
    if (this.running || this.state.isStreaming) {
      if (text.startsWith("/")) r = await this.cmd({ type: "prompt", ...base, streamingBehavior: how === "followUp" ? "followUp" : "steer" });
      else r = await this.cmd({ type: how === "followUp" ? "follow_up" : "steer", ...base });
    } else {
      this.status = "running";
      this.runStart = Date.now();
      this.emit("status");
      r = await this.cmd({ type: "prompt", ...base });
      if (r.success && r.data?.disposition === "handled") { this.status = "idle"; this.runStart = 0; this.emit("status"); void this.load(); }
    }
    if (!r.success) {
      if (this.status === "running" && !this.state.isStreaming) { this.status = "idle"; this.emit("status"); }
      this.note("error", r.error ?? "pi didn't take that");
    }
  }

  async abort() {
    if (!this.running) return;
    const q = await this.cmd({ type: "clear_queue" });
    await this.cmd({ type: "abort" });
    const back = q.success ? [...(q.data.steering ?? []), ...(q.data.followUp ?? [])].join("\n\n") : "";
    if (back) { this.draft = back; this.emit("draft"); }
  }

  async unqueue(text: string) {
    const q = await this.cmd({ type: "clear_queue" });
    if (!q.success) return;
    for (const s of q.data.steering ?? []) if (s !== text) await this.cmd({ type: "steer", message: s });
    for (const s of q.data.followUp ?? []) if (s !== text) await this.cmd({ type: "follow_up", message: s });
    this.draft = text;
    this.emit("draft");
  }

  async bash(command: string, exclude = false) {
    if (!(await this.ensure())) return;
    const reqId = `b${Date.now()}`;
    const it = this.push<ItemOf<"bash">>({ k: "bash", command, output: "", running: true, reqId, excluded: exclude });
    this.emit("items");
    const r = await this.cmd({ id: reqId, type: "bash", command, excludeFromContext: exclude });
    it.running = false;
    if (r.success) { it.output = r.data.output; it.exit = r.data.exitCode; it.cancelled = r.data.cancelled; }
    else { it.output += `\n${r.error}`; it.exit = -1; }
    it.v++;
    this.emit("items");
  }

  async compact(instructions?: string) {
    if (!(await this.ensure())) return;
    this.compacting = true;
    this.emit("status");
    const r = await this.cmd({ type: "compact", ...(instructions ? { customInstructions: instructions } : {}) });
    this.compacting = false;
    if (!r.success) this.note("error", r.error ?? "compaction failed");
    this.emit("status");
  }

  async setModel(m: Model) {
    if (!(await this.ensure())) return;
    const r = await this.cmd({ type: "set_model", provider: m.provider, modelId: m.id });
    if (r.success) {
      this.state.model = r.data;
      const lv = await this.cmd({ type: "get_available_thinking_levels" });
      if (lv.success) this.levels = lv.data.levels;
      const st = await this.cmd({ type: "get_state" });
      if (st.success) this.state = st.data;
      this.emit("meta");
      void this.refreshStats();
    } else this.note("error", r.error ?? "couldn't switch model");
  }

  async setThinking(level: string) {
    if (!(await this.ensure())) return;
    const r = await this.cmd({ type: "set_thinking_level", level });
    if (r.success) { this.state.thinkingLevel = level; this.emit("meta"); }
  }

  setMode(m: Mode) {
    this.mode = m;
    void api.setMode(this.id, m);
    this.saveMeta();
    this.emit("meta");
  }

  private saveMeta() { if (this.file) void api.setMeta(this.file, { kind: this.kind, mode: this.mode }); }

  async rename(name: string) {
    this.name = name;
    this.state.sessionName = name;
    this.emit("meta");
    if (this.status === "idle" || this.status === "running") await this.cmd({ type: "set_session_name", name });
    else if (this.file && (await this.ensure())) await this.cmd({ type: "set_session_name", name });
  }

  // forking switches this process to a new session file that ends just before the message
  async fork(entryId: string): Promise<string | null> {
    if (!(await this.ensure())) return null;
    const r = await this.cmd({ type: "fork", entryId });
    if (!r.success || r.data?.cancelled) { this.note("error", r.error ?? "fork was cancelled"); return null; }
    await this.reloadState();
    return r.data.text ?? "";
  }

  async clone() {
    if (!(await this.ensure())) return;
    const r = await this.cmd({ type: "clone" });
    if (r.success && !r.data?.cancelled) await this.reloadState();
  }

  async newSession() {
    if (!(await this.ensure())) return;
    const r = await this.cmd({ type: "new_session" });
    if (r.success && !r.data?.cancelled) { this.name = undefined; await this.reloadState(); }
  }

  private async reloadState() {
    const st = await this.cmd({ type: "get_state" });
    if (st.success) { this.state = st.data; this.file = st.data.sessionFile; this.saveMeta(); }
    this.todos = [];
    await this.load();
    void this.refreshStats();
    this.emit("meta");
  }

  async exportHtml(): Promise<string | null> {
    if (!(await this.ensure())) return null;
    const r = await this.cmd({ type: "export_html" });
    return r.success ? r.data.path : null;
  }

  async lastText(): Promise<string> {
    const r = await this.cmd({ type: "get_last_assistant_text" });
    return r.success ? r.data.text ?? "" : "";
  }
}

export function textOf(c: unknown): string {
  if (typeof c === "string") return c;
  if (!Array.isArray(c)) return "";
  return c.filter((x) => x?.type === "text").map((x) => x.text).join("\n");
}

function splitContent(c: unknown): { text: string; images: Img[] } {
  if (typeof c === "string") return { text: c, images: [] };
  const arr = Array.isArray(c) ? c : [];
  return { text: arr.filter((x) => x.type === "text").map((x) => x.text).join("\n"), images: arr.filter((x) => x.type === "image").map((x) => ({ data: x.data, mimeType: x.mimeType })) };
}

const stripAnsi = (s: string) => String(s).replace(/\x1b\[[0-9;]*m/g, "");
