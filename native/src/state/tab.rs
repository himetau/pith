// One open session: its pi process, the transcript rebuilt from pi's events, and what the chat shows around it.
// a port of the old ui's tab.ts, same events in, same items out
use crate::core::pi::PiProc;
use futures::future::{FutureExt, Shared};
use gpui_kit::*;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status { Cold, Starting, Idle, Running, Dead }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind { Chat, Design }
impl Kind {
    pub fn as_str(self) -> &'static str { match self { Kind::Chat => "chat", Kind::Design => "design" } }
    pub fn parse(s: &str) -> Self { if s == "design" { Kind::Design } else { Kind::Chat } }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode { Ask, Edits, Plan, Auto }
impl Mode {
    pub const ALL: [Mode; 4] = [Mode::Ask, Mode::Edits, Mode::Plan, Mode::Auto];
    pub fn as_str(self) -> &'static str { match self { Mode::Ask => "ask", Mode::Edits => "edits", Mode::Plan => "plan", Mode::Auto => "auto" } }
    pub fn parse(s: &str) -> Self { match s { "edits" => Mode::Edits, "plan" => Mode::Plan, "auto" => Mode::Auto, _ => Mode::Ask } }
    pub fn hint(self) -> &'static str {
        match self {
            Mode::Ask => "asks before edits and commands",
            Mode::Edits => "edits files freely, asks before commands",
            Mode::Plan => "read-only, ends with a plan",
            Mode::Auto => "never asks",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Img { pub data: String, pub mime: String }

#[derive(Clone, Debug, Default)]
pub struct ToolResult { pub content: Value, pub details: Value, pub is_error: bool }
impl ToolResult {
    fn from(v: &Value) -> Self { ToolResult { content: v["content"].clone(), details: v["details"].clone(), is_error: v["isError"].as_bool().unwrap_or(false) } }
    pub fn text(&self) -> String { text_of(&self.content) }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToolStatus { Streaming, Queued, Running, Done, Error }

#[derive(Clone, Debug)]
pub struct ToolBlock {
    pub id: String,
    pub name: String,
    pub args: Value,
    pub args_text: String,
    pub status: ToolStatus,
    pub result: Option<ToolResult>,
    pub partial: Option<ToolResult>,
    pub ms: Option<u64>,
    pub started: Option<Instant>,
}

#[derive(Clone, Debug)]
pub enum Block {
    Text(String),
    Thinking { text: String, redacted: bool, started: Option<Instant>, ms: Option<u64> },
    // the call itself lives in Tab::tools, results land there by id
    Tool(String),
}

#[derive(Clone, Debug)]
pub struct Assistant {
    pub blocks: Vec<Option<Block>>,
    pub model: Option<String>,
    pub usage: Value,
    pub stop: Option<String>,
    pub error: Option<String>,
    pub ts: f64,
    pub live: bool,
    pub run_ms: Option<u64>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level { Info, Warn, Error }

// ts on a user message is kept for when the transcript shows times
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub enum ItemKind {
    User { text: String, images: Vec<Img>, entry_id: Option<String>, ts: f64, typed: Option<String> },
    Assistant(Assistant),
    Bash { command: String, output: String, exit: Option<i64>, running: bool, cancelled: bool, req_id: Option<String>, excluded: bool },
    Compaction { summary: String, tokens_before: Option<u64> },
    Branch { summary: String },
    Custom { kind: String, text: String },
    Note { level: Level, text: String },
}

#[derive(Clone, Debug)]
pub struct Item { pub id: u64, pub kind: ItemKind }

#[derive(Clone, Debug)]
pub struct Ask {
    pub id: String,
    pub method: String,
    pub title: String,
    pub message: Option<String>,
    pub options: Vec<String>,
    pub placeholder: Option<String>,
    pub prefill: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Todo { pub content: String, pub status: String }

#[derive(Clone, Debug)]
pub struct Command { pub name: String, pub description: String, pub source: String }

#[derive(Clone, Debug)]
// `until` is when the next try goes; the live line only says which try it is, like chat.ts
#[allow(dead_code)]
pub struct Retry { pub attempt: u64, pub max: u64, pub until: Instant, pub error: String }

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum How { Auto, FollowUp }

// what views listen for besides plain notify()
#[derive(Clone, Debug)]
pub enum TabEvent {
    Toast(Level, String),
    Draft(String),
    Settled,
    Preview,
    // the agent is driving the session's browser page
    Browser,
    ToolDone(String),
    // quote something into this session's composer as a chip, sent as a > blockquote before the text
    // (preview comments, terminal "to chat", findings "fix this")
    Quote { label: String, text: String },
    // attach an image file to the composer (images page "use in the current chat")
    Attach(std::path::PathBuf),
}
impl EventEmitter<TabEvent> for Tab {}

// what the app does with a pi process it spawned; set by app.rs so the tab doesn't need to know about the router
pub type Spawner = Arc<dyn Fn(&str, &str, Option<&str>, Kind, Mode) -> Result<Arc<PiProc>, String> + Send + Sync>;

static NEXT_ITEM: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
static NEXT_TAB: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub struct Tab {
    pub id: String,
    pub cwd: String,
    pub file: Option<String>,
    pub kind: Kind,
    pub mode: Mode,
    pub name: Option<String>,
    pub status: Status,
    pub items: Vec<Item>,
    pub tools: HashMap<String, ToolBlock>,
    pub state: Value,
    pub levels: Vec<String>,
    pub stats: Value,
    pub queue: (Vec<String>, Vec<String>),
    pub asks: Vec<Ask>,
    pub widgets: Vec<(String, Vec<String>)>,
    // widgets an extension put under the composer rather than over it (widgetPlacement "belowEditor")
    pub widgets_below: std::collections::HashSet<String>,
    pub statuses: Vec<(String, String)>,
    pub commands: Vec<Command>,
    pub todos: Vec<Todo>,
    pub unread: bool,
    pub error: String,
    pub compacting: bool,
    pub retry: Option<Retry>,
    pub run_start: Option<Instant>,
    pub last_used: Instant,
    pub preview: Option<Value>,
    // when you last sent something (ms), which is what orders the sidebar. a new session counts from when it was made
    pub sent: f64,
    // bumps on every transcript change, the chat view re-measures when it moves
    pub version: u64,
    proc: Option<Arc<PiProc>>,
    spawner: Spawner,
    starting: Option<Shared<Task<bool>>>,
    typed: Vec<String>,
    live: Option<usize>,
    loaded: bool,
}

impl Tab {
    pub fn new(cwd: String, file: Option<String>, kind: Kind, mode: Mode, name: Option<String>, spawner: Spawner) -> Self {
        let n = NEXT_TAB.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let sent = if file.is_none() { now_ms() } else { 0. };
        Tab {
            id: format!("t{}{}", std::process::id(), n), cwd, file, kind, mode, name, status: Status::Cold,
            items: vec![], tools: HashMap::new(), state: json!({}), levels: vec![], stats: Value::Null,
            queue: (vec![], vec![]), asks: vec![], widgets: vec![], widgets_below: Default::default(), statuses: vec![], commands: vec![], todos: vec![],
            unread: false, error: String::new(), compacting: false, retry: None, run_start: None, last_used: Instant::now(),
            preview: None, sent, version: 0, proc: None, spawner, starting: None, typed: vec![], live: None, loaded: false,
        }
    }

    pub fn title(&self) -> String {
        if let Some(n) = self.state["sessionName"].as_str().filter(|s| !s.is_empty()).or(self.name.as_deref()) { return n.to_string(); }
        for it in &self.items {
            if let ItemKind::User { text, typed, .. } = &it.kind { return title_from(typed.as_deref().unwrap_or(text)); }
        }
        if self.kind == Kind::Design { "new design".into() } else { "new session".into() }
    }
    pub fn running(&self) -> bool { self.status == Status::Running }
    pub fn live_process(&self) -> bool { matches!(self.status, Status::Idle | Status::Running | Status::Starting) }
    pub fn model_label(&self) -> Option<String> { self.state["model"]["id"].as_str().map(String::from) }

    fn bump(&mut self, cx: &mut Context<Self>) { self.version += 1; cx.notify(); }

    fn push(&mut self, kind: ItemKind) -> usize {
        let id = NEXT_ITEM.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.items.push(Item { id, kind });
        self.loaded = true;
        self.items.len() - 1
    }

    pub fn note(&mut self, level: Level, text: impl Into<String>, cx: &mut Context<Self>) {
        self.push(ItemKind::Note { level, text: text.into() });
        self.bump(cx);
    }

    // ---------- process ----------
    // starts pi if it isn't running. resolves true once the session can take a prompt
    pub fn ensure(&mut self, cx: &mut Context<Self>) -> Shared<Task<bool>> {
        self.last_used = Instant::now();
        if matches!(self.status, Status::Idle | Status::Running) { return Task::ready(true).shared(); }
        if let Some(t) = &self.starting { return t.clone(); }
        self.status = Status::Starting;
        self.error.clear();
        cx.notify();
        let resuming = self.file.is_some();
        let spawned = (self.spawner)(&self.id, &self.cwd, self.file.as_deref(), self.kind, self.mode);
        let task = cx.spawn(async move |this, cx| {
            let proc = match spawned {
                Ok(p) => p,
                Err(e) => { let _ = this.update(cx, |t, cx| t.died(e, cx)); return false; }
            };
            let st = proc.send(json!({ "type": "get_state" })).await;
            if st["success"] != true {
                let tail = proc.stderr.lock().unwrap().trim().to_string();
                let err = if tail.is_empty() { st["error"].as_str().unwrap_or("pi didn't start").to_string() } else { tail };
                let _ = this.update(cx, |t, cx| t.died(err, cx));
                return false;
            }
            let entries = if resuming { Some(proc.send(json!({ "type": "get_entries" })).await) } else { None };
            let ok = this.update(cx, |t, cx| {
                t.proc = Some(proc.clone());
                t.state = st["data"].clone();
                if let Some(f) = t.state["sessionFile"].as_str() { t.file = Some(f.to_string()); }
                // kind and mode go next to the file from the start, so a design reopens as one
                t.save_meta();
                // history goes in before anyone can send, a load landing after a live message would wipe it
                if let Some(r) = &entries { if r["success"] == true { t.fill(&r["data"]["entries"], r["data"]["leafId"].as_str()); } }
                t.loaded = true;
                t.status = if t.state["isStreaming"] == true { Status::Running } else { Status::Idle };
                t.starting = None;
                t.bump(cx);
                t.after_start(cx);
            }).is_ok();
            ok
        });
        let shared = task.shared();
        self.starting = Some(shared.clone());
        shared
    }

    fn died(&mut self, err: String, cx: &mut Context<Self>) {
        self.status = Status::Dead;
        self.error = err;
        self.starting = None;
        self.proc = None;
        cx.notify();
    }

    pub fn exited(&mut self, code: Option<i32>, stderr: String, cx: &mut Context<Self>) {
        self.proc = None;
        self.starting = None;
        let was = self.status;
        self.status = if code == Some(0) || was == Status::Cold { Status::Cold } else { Status::Dead };
        if self.status == Status::Dead {
            self.error = stderr.trim().lines().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
            if self.error.is_empty() { self.error = format!("pi exited ({})", code.map(|c| c.to_string()).unwrap_or("signal".into())); }
        }
        for it in &mut self.items { if let ItemKind::Assistant(a) = &mut it.kind { a.live = false; } }
        self.live = None;
        self.bump(cx);
    }

    fn after_start(&mut self, cx: &mut Context<Self>) {
        let (a, b) = (self.cmd(json!({ "type": "get_available_thinking_levels" })), self.cmd(json!({ "type": "get_commands" })));
        cx.spawn(async move |this, cx| {
            let (lv, cmds) = futures::join!(a, b);
            let _ = this.update(cx, |t, cx| {
                if lv["success"] == true { t.levels = strings(&lv["data"]["levels"]); }
                if cmds["success"] == true {
                    t.commands = cmds["data"]["commands"].as_array().map(|a| a.iter().map(|c| Command {
                        name: c["name"].as_str().unwrap_or("").into(), description: c["description"].as_str().unwrap_or("").into(), source: c["source"].as_str().unwrap_or("").into(),
                    }).collect()).unwrap_or_default();
                }
                cx.notify();
                t.refresh_stats(cx);
            });
        }).detach();
    }

    pub fn stop(&mut self, cx: &mut Context<Self>) {
        if let Some(p) = self.proc.take() { p.stop(); }
        self.starting = None;
        self.status = Status::Cold;
        cx.notify();
    }

    // a command to pi; answers "pi isn't running" without a process instead of starting one
    pub fn cmd(&self, c: Value) -> impl std::future::Future<Output = Value> + 'static {
        let p = self.proc.clone();
        async move {
            match p { Some(p) => p.send(c).await, None => json!({ "type": "response", "success": false, "error": "pi isn't running" }) }
        }
    }

    // ---------- history ----------
    // paints a session straight off disk, before pi is up. pi's own get_entries replaces it once it is
    pub fn fill_from_disk(&mut self, d: &Value, cx: &mut Context<Self>) {
        if self.loaded { return; }
        self.fill(&d["entries"], d["leafId"].as_str());
        self.loaded = false;
        self.bump(cx);
    }

    // entries, not messages: messages stop at the last compaction, entries keep everything on the branch
    pub fn fill(&mut self, entries: &Value, leaf: Option<&str>) {
        let empty = vec![];
        let list = entries.as_array().unwrap_or(&empty);
        let by_id: HashMap<&str, &Value> = list.iter().filter_map(|e| Some((e["id"].as_str()?, e))).collect();
        let mut path = vec![];
        let mut id = leaf;
        while let Some(i) = id {
            let Some(e) = by_id.get(i) else { break };
            path.push(*e);
            id = e["parentId"].as_str();
        }
        path.reverse();
        self.items.clear();
        self.tools.clear();
        self.todos.clear();
        self.live = None;
        for e in path {
            match e["type"].as_str().unwrap_or("") {
                "message" => self.add_message(&e["message"], e["id"].as_str()),
                "compaction" => { self.push(ItemKind::Compaction { summary: s(&e["summary"]), tokens_before: e["tokensBefore"].as_u64() }); }
                "branch_summary" => { self.push(ItemKind::Branch { summary: s(&e["summary"]) }); }
                "custom_message" if e["display"] == true => { self.push(ItemKind::Custom { kind: s(&e["customType"]), text: text_of(&e["content"]) }); }
                _ => {}
            }
        }
        self.version += 1;
    }

    fn add_message(&mut self, m: &Value, entry_id: Option<&str>) {
        match m["role"].as_str().unwrap_or("") {
            "user" => {
                let (text, images) = split_content(&m["content"]);
                let queued = if entry_id.is_none() && !self.typed.is_empty() { Some(self.typed.remove(0)) } else { None };
                let typed = queued.filter(|_| !text.starts_with('/'));
                self.push(ItemKind::User { text, images, entry_id: entry_id.map(String::from), ts: m["timestamp"].as_f64().unwrap_or(0.0), typed });
            }
            "assistant" => {
                let i = self.push(ItemKind::Assistant(Assistant { blocks: vec![], model: None, usage: Value::Null, stop: None, error: None, ts: m["timestamp"].as_f64().unwrap_or(0.0), live: false, run_ms: None }));
                self.fill_assistant(i, m);
            }
            "toolResult" => self.attach_result(s(&m["toolCallId"]).as_str(), ToolResult::from(m), m["durationMs"].as_u64()),
            "bashExecution" => {
                self.push(ItemKind::Bash { command: s(&m["command"]), output: s(&m["output"]), exit: m["exitCode"].as_i64(), running: false, cancelled: m["cancelled"] == true, req_id: None, excluded: m["excludeFromContext"] == true });
            }
            "custom" if m["display"] == true => { self.push(ItemKind::Custom { kind: s(&m["customType"]), text: text_of(&m["content"]) }); }
            "compactionSummary" => { self.push(ItemKind::Compaction { summary: s(&m["summary"]), tokens_before: m["tokensBefore"].as_u64() }); }
            "branchSummary" => { self.push(ItemKind::Branch { summary: s(&m["summary"]) }); }
            _ => {}
        }
    }

    fn fill_assistant(&mut self, i: usize, m: &Value) {
        let ItemKind::Assistant(a) = &self.items[i].kind else { return };
        let live = a.live;
        let prev = a.blocks.clone();
        let mut blocks = vec![];
        for c in m["content"].as_array().into_iter().flatten() {
            match c["type"].as_str().unwrap_or("") {
                "text" => blocks.push(Some(Block::Text(s(&c["text"])))),
                "thinking" => {
                    let ms = match prev.get(blocks.len()) { Some(Some(Block::Thinking { ms, .. })) => *ms, _ => None };
                    blocks.push(Some(Block::Thinking { text: s(&c["thinking"]), redacted: c["redacted"] == true, started: None, ms }));
                }
                "toolCall" => {
                    let id = s(&c["id"]);
                    let status = if live || m["stopReason"] == "pending" { ToolStatus::Queued } else { ToolStatus::Done };
                    let tb = self.tools.entry(id.clone()).or_insert_with(|| ToolBlock { id: id.clone(), name: String::new(), args: Value::Null, args_text: String::new(), status, result: None, partial: None, ms: None, started: None });
                    tb.args = c["arguments"].clone();
                    tb.name = s(&c["name"]);
                    if tb.name == "todo_write" { if let Some(t) = todos_of(&c["arguments"]) { self.todos = t; } }
                    blocks.push(Some(Block::Tool(id)));
                }
                _ => {}
            }
        }
        let ItemKind::Assistant(a) = &mut self.items[i].kind else { return };
        a.blocks = blocks;
        a.model = m["model"].as_str().map(String::from);
        a.usage = m["usage"].clone();
        a.stop = m["stopReason"].as_str().map(String::from);
        a.error = m["errorMessage"].as_str().map(String::from);
        if let Some(ts) = m["timestamp"].as_f64() { a.ts = ts; }
    }

    fn attach_result(&mut self, id: &str, res: ToolResult, ms: Option<u64>) {
        let Some(tb) = self.tools.get_mut(id) else { return };
        tb.status = if res.is_error { ToolStatus::Error } else { ToolStatus::Done };
        tb.result = Some(res);
        tb.partial = None;
        if ms.is_some() { tb.ms = ms; }
    }

    // ---------- live events ----------
    pub fn on_event(&mut self, r: &Value, cx: &mut Context<Self>) {
        match r["type"].as_str().unwrap_or("") {
            "agent_start" => {
                if self.status != Status::Running { self.status = Status::Running; self.run_start = Some(Instant::now()); cx.notify(); }
            }
            "message_start" => {
                let m = &r["message"];
                match m["role"].as_str().unwrap_or("") {
                    "assistant" => {
                        let i = self.push(ItemKind::Assistant(Assistant { blocks: vec![], model: None, usage: Value::Null, stop: None, error: None, ts: m["timestamp"].as_f64().unwrap_or_else(now_ms), live: true, run_ms: None }));
                        self.live = Some(i);
                    }
                    "system" | "toolResult" => {}
                    _ => self.add_message(m, None),
                }
                self.bump(cx);
            }
            "message_update" => {
                if let Some(i) = self.live { self.on_delta(i, &r["assistantMessageEvent"], &r["usage"]); self.bump(cx); }
            }
            "message_end" => {
                let m = &r["message"];
                if m["role"] == "assistant" {
                    if let Some(i) = self.live.take() {
                        let thinking_ms: Vec<Option<u64>> = match &self.items[i].kind {
                            ItemKind::Assistant(a) => a.blocks.iter().map(|b| match b { Some(Block::Thinking { ms, .. }) => *ms, _ => None }).collect(),
                            _ => vec![],
                        };
                        self.fill_assistant(i, m);
                        if let ItemKind::Assistant(a) = &mut self.items[i].kind {
                            for (j, b) in a.blocks.iter_mut().enumerate() {
                                if let (Some(Block::Thinking { ms, .. }), Some(Some(t))) = (b, thinking_ms.get(j)) { *ms = Some(*t); }
                            }
                            a.live = false;
                        }
                    }
                } else if m["role"] == "toolResult" {
                    self.attach_result(&s(&m["toolCallId"]), ToolResult::from(m), m["durationMs"].as_u64());
                }
                self.bump(cx);
            }
            "tool_execution_start" => {
                let id = s(&r["toolCallId"]);
                if let Some(tb) = self.tools.get_mut(&id) {
                    tb.status = ToolStatus::Running;
                    tb.started = Some(Instant::now());
                    if !r["args"].is_null() { tb.args = r["args"].clone(); }
                }
                let name = s(&r["toolName"]);
                if name == "todo_write" { if let Some(t) = todos_of(&r["args"]) { self.todos = t; } }
                if name == "preview" {
                    let mut p = r["args"].clone();
                    if let Some(path) = p["path"].as_str() {
                        if !path.starts_with('/') { p["path"] = json!(format!("{}/{}", self.cwd.trim_end_matches('/'), path.trim_start_matches("./"))); }
                    }
                    self.preview = Some(p);
                    cx.emit(TabEvent::Preview);
                }
                self.bump(cx);
            }
            "tool_execution_update" => {
                if let Some(tb) = self.tools.get_mut(&s(&r["toolCallId"])) { tb.partial = Some(ToolResult::from(&r["partialResult"])); self.bump(cx); }
            }
            "tool_execution_end" => {
                let err = r["isError"] == true;
                if let Some(tb) = self.tools.get_mut(&s(&r["toolCallId"])) {
                    if tb.result.is_none() { tb.result = Some(ToolResult::from(&r["result"])); }
                    tb.status = if err { ToolStatus::Error } else { ToolStatus::Done };
                    if let Some(ms) = r["durationMs"].as_u64() { tb.ms = Some(ms); }
                    else if let Some(st) = tb.started { tb.ms = Some(st.elapsed().as_millis() as u64); }
                    tb.partial = None;
                }
                cx.emit(TabEvent::ToolDone(s(&r["toolName"])));
                self.bump(cx);
            }
            "turn_end" => self.refresh_stats(cx),
            "agent_settled" => {
                self.status = Status::Idle;
                self.retry = None;
                let aborted = r["aborted"] == true;
                if let Some(st) = self.run_start.take() {
                    if let Some(a) = self.items.iter_mut().rev().find_map(|i| if let ItemKind::Assistant(a) = &mut i.kind { Some(a) } else { None }) {
                        a.run_ms = Some(st.elapsed().as_millis() as u64);
                    }
                }
                for it in &mut self.items { if let ItemKind::Assistant(a) = &mut it.kind { a.live = false; } }
                self.live = None;
                // tool calls the abort cut off never get an end event
                for tb in self.tools.values_mut() {
                    if matches!(tb.status, ToolStatus::Running | ToolStatus::Queued | ToolStatus::Streaming) { tb.status = if aborted { ToolStatus::Error } else { ToolStatus::Done }; }
                }
                if aborted { self.push(ItemKind::Note { level: Level::Warn, text: "Stopped".into() }); }
                self.bump(cx);
                cx.emit(TabEvent::Settled);
                self.refresh_stats(cx);
                self.sync_entry_ids(cx);
            }
            "queue_update" => { self.queue = (strings(&r["steering"]), strings(&r["followUp"])); cx.notify(); }
            "compaction_start" => { self.compacting = true; cx.notify(); }
            "compaction_end" => {
                self.compacting = false;
                if r["result"].is_object() { self.push(ItemKind::Compaction { summary: s(&r["result"]["summary"]), tokens_before: r["result"]["tokensBefore"].as_u64() }); }
                else if let Some(e) = r["errorMessage"].as_str() { self.push(ItemKind::Note { level: Level::Error, text: format!("Compaction failed: {e}") }); }
                self.bump(cx);
                self.refresh_stats(cx);
            }
            "auto_retry_start" => {
                self.retry = Some(Retry { attempt: r["attempt"].as_u64().unwrap_or(1), max: r["maxAttempts"].as_u64().unwrap_or(1), until: Instant::now() + std::time::Duration::from_millis(r["delayMs"].as_u64().unwrap_or(0)), error: s(&r["errorMessage"]) });
                cx.notify();
            }
            "auto_retry_end" => {
                self.retry = None;
                if r["success"] != true { if let Some(e) = r["finalError"].as_str() { self.push(ItemKind::Note { level: Level::Error, text: e.into() }); self.bump(cx); } }
                cx.notify();
            }
            "session_info_changed" => { self.state["sessionName"] = r["name"].clone(); cx.notify(); }
            "thinking_level_changed" => { self.state["thinkingLevel"] = r["level"].clone(); cx.notify(); }
            "bash_execution_update" => {
                let id = s(&r["id"]);
                for it in self.items.iter_mut().rev() {
                    if let ItemKind::Bash { req_id: Some(q), output, .. } = &mut it.kind { if *q == id { output.push_str(r["delta"].as_str().unwrap_or("")); break; } }
                }
                self.bump(cx);
            }
            "extension_ui_request" => self.on_ui(r, cx),
            "extension_error" => {
                let ext = r["extensionPath"].as_str().and_then(|p| p.rsplit('/').next()).unwrap_or("extension");
                self.note(Level::Warn, format!("{ext}: {}", s(&r["error"])), cx);
            }
            _ => {}
        }
    }

    fn on_delta(&mut self, i: usize, e: &Value, usage: &Value) {
        let idx = e["contentIndex"].as_u64().map(|x| x as usize);
        let ItemKind::Assistant(a) = &mut self.items[i].kind else { return };
        if let Some(ix) = idx { if a.blocks.len() <= ix { a.blocks.resize(ix + 1, None); } }
        let delta = e["delta"].as_str().unwrap_or("");
        match (e["type"].as_str().unwrap_or(""), idx) {
            ("text_start", Some(ix)) => a.blocks[ix] = Some(Block::Text(String::new())),
            ("text_delta", Some(ix)) => if let Some(Block::Text(t)) = &mut a.blocks[ix] { t.push_str(delta) },
            ("text_end", Some(ix)) => if let (Some(Block::Text(t)), Some(c)) = (&mut a.blocks[ix], e["content"].as_str()) { *t = c.into() },
            ("thinking_start", Some(ix)) => a.blocks[ix] = Some(Block::Thinking { text: String::new(), redacted: false, started: Some(Instant::now()), ms: None }),
            ("thinking_delta", Some(ix)) => if let Some(Block::Thinking { text, .. }) = &mut a.blocks[ix] { text.push_str(delta) },
            ("thinking_end", Some(ix)) => if let Some(Block::Thinking { text, started, ms, .. }) = &mut a.blocks[ix] {
                if let Some(c) = e["content"].as_str() { *text = c.into(); }
                *ms = Some(started.map(|s| s.elapsed().as_millis() as u64).unwrap_or(0));
            },
            ("toolcall_start", Some(ix)) => {
                let id = s(&e["id"]);
                a.blocks[ix] = Some(Block::Tool(id.clone()));
                self.tools.insert(id.clone(), ToolBlock { id, name: s(&e["toolName"]), args: json!({}), args_text: String::new(), status: ToolStatus::Streaming, result: None, partial: None, ms: None, started: None });
            }
            ("toolcall_delta", Some(ix)) => if let Some(Block::Tool(id)) = &a.blocks[ix] { if let Some(tb) = self.tools.get_mut(id) { tb.args_text.push_str(delta); } },
            ("toolcall_end", Some(ix)) => if let Some(Block::Tool(id)) = &a.blocks[ix] {
                if let Some(tb) = self.tools.get_mut(id) {
                    tb.args = e["toolCall"]["arguments"].clone();
                    if tb.status == ToolStatus::Streaming { tb.status = ToolStatus::Queued; }
                }
            },
            _ => {}
        }
        if !usage.is_null() { if let ItemKind::Assistant(a) = &mut self.items[i].kind { a.usage = usage.clone(); } }
    }

    fn on_ui(&mut self, r: &Value, cx: &mut Context<Self>) {
        match r["method"].as_str().unwrap_or("") {
            m @ ("select" | "confirm" | "input" | "editor") => {
                let id = s(&r["id"]);
                self.asks.push(Ask { id: id.clone(), method: m.into(), title: s(&r["title"]), message: r["message"].as_str().map(String::from), options: strings(&r["options"]), placeholder: r["placeholder"].as_str().map(String::from), prefill: r["prefill"].as_str().map(String::from) });
                if let Some(ms) = r["timeout"].as_u64() {
                    cx.spawn(async move |this, cx| {
                        cx.background_executor().timer(std::time::Duration::from_millis(ms)).await;
                        let _ = this.update(cx, |t, cx| { t.asks.retain(|a| a.id != id); cx.notify(); });
                    }).detach();
                }
                cx.notify();
            }
            "notify" => {
                let level = match r["notifyType"].as_str() { Some("error") => Level::Error, Some("warning") => Level::Warn, _ => Level::Info };
                cx.emit(TabEvent::Toast(level, s(&r["message"])));
            }
            "setStatus" => {
                let key = s(&r["statusKey"]);
                self.statuses.retain(|(k, _)| *k != key);
                if let Some(t) = r["statusText"].as_str() { self.statuses.push((key, strip_ansi(t))); }
                cx.notify();
            }
            "setWidget" => {
                let key = s(&r["widgetKey"]);
                self.widgets.retain(|(k, _)| *k != key);
                if r["widgetPlacement"] == "belowEditor" { self.widgets_below.insert(key.clone()); } else { self.widgets_below.remove(&key); }
                if let Some(lines) = r["widgetLines"].as_array() { self.widgets.push((key, lines.iter().map(|l| strip_ansi(l.as_str().unwrap_or(""))).collect())); }
                cx.notify();
            }
            "set_editor_text" => cx.emit(TabEvent::Draft(s(&r["text"]))),
            _ => {}
        }
    }

    pub fn answer(&mut self, id: &str, payload: Value, cx: &mut Context<Self>) {
        self.asks.retain(|a| a.id != id);
        if let Some(p) = &self.proc { p.ui_response(id, payload); }
        cx.notify();
    }

    pub fn refresh_stats(&mut self, cx: &mut Context<Self>) {
        let f = self.cmd(json!({ "type": "get_session_stats" }));
        cx.spawn(async move |this, cx| {
            let r = f.await;
            if r["success"] != true { return; }
            let _ = this.update(cx, |t, cx| {
                if let Some(f) = r["data"]["sessionFile"].as_str() { t.file = Some(f.into()); }
                t.stats = r["data"].clone();
                cx.notify();
            });
        }).detach();
    }

    // fork needs entry ids on user messages; live ones only get them once pi has written them
    fn sync_entry_ids(&mut self, cx: &mut Context<Self>) {
        let f = self.cmd(json!({ "type": "get_fork_messages" }));
        cx.spawn(async move |this, cx| {
            let r = f.await;
            if r["success"] != true { return; }
            let _ = this.update(cx, |t, _| {
                let fm: Vec<String> = r["data"]["messages"].as_array().into_iter().flatten().map(|m| s(&m["entryId"])).collect();
                let mut users: Vec<&mut Option<String>> = t.items.iter_mut().filter_map(|i| if let ItemKind::User { entry_id, .. } = &mut i.kind { Some(entry_id) } else { None }).collect();
                let off = users.len().saturating_sub(fm.len());
                for (i, e) in fm.into_iter().enumerate() { if let Some(u) = users.get_mut(i + off) { if u.is_none() { **u = Some(e); } } }
            });
        }).detach();
    }

    // ---------- actions ----------
    pub fn send(&mut self, text: String, images: Vec<Img>, how: How, cx: &mut Context<Self>) {
        self.sent = now_ms();
        let ready = self.ensure(cx);
        cx.spawn(async move |this, cx| {
            if !ready.await { return; }
            let Ok((cmd, idle)) = this.update(cx, |t, cx| {
                t.last_used = Instant::now();
                let is_template = text.starts_with('/') && !t.commands.iter().any(|c| c.source == "extension" && text[1..].split_whitespace().next() == Some(c.name.as_str()));
                if is_template { t.typed.push(text.clone()); }
                let mut base = json!({ "message": text });
                if !images.is_empty() { base["images"] = json!(images.iter().map(|i| json!({ "type": "image", "data": i.data, "mimeType": i.mime })).collect::<Vec<_>>()); }
                let streaming = t.running() || t.state["isStreaming"] == true;
                if streaming {
                    if text.starts_with('/') { base["type"] = json!("prompt"); base["streamingBehavior"] = json!(if how == How::FollowUp { "followUp" } else { "steer" }); }
                    else { base["type"] = json!(if how == How::FollowUp { "follow_up" } else { "steer" }); }
                } else {
                    base["type"] = json!("prompt");
                    t.status = Status::Running;
                    t.run_start = Some(Instant::now());
                    cx.notify();
                }
                (t.cmd(base), !streaming)
            }) else { return };
            let r = cmd.await;
            let _ = this.update(cx, |t, cx| {
                if r["success"] == true && idle && r["data"]["disposition"] == "handled" {
                    t.status = Status::Idle;
                    t.run_start = None;
                    t.reload(cx);
                }
                if r["success"] != true {
                    if t.status == Status::Running && t.state["isStreaming"] != true { t.status = Status::Idle; }
                    t.note(Level::Error, r["error"].as_str().unwrap_or("pi didn't take that").to_string(), cx);
                }
                cx.notify();
            });
        }).detach();
    }

    pub fn abort(&mut self, cx: &mut Context<Self>) {
        if !self.running() { return; }
        let (q, a) = (self.cmd(json!({ "type": "clear_queue" })), self.cmd(json!({ "type": "abort" })));
        cx.spawn(async move |this, cx| {
            let q = q.await;
            a.await;
            let mut back: Vec<String> = strings(&q["data"]["steering"]);
            back.extend(strings(&q["data"]["followUp"]));
            if !back.is_empty() { let _ = this.update(cx, |_, cx| cx.emit(TabEvent::Draft(back.join("\n\n")))); }
        }).detach();
    }

    // takes one queued message back into the composer. pi can only clear the whole queue, so the rest go back in order
    pub fn unqueue(&mut self, text: String, cx: &mut Context<Self>) {
        let clear = self.cmd(json!({ "type": "clear_queue" }));
        let p = self.proc.clone();
        cx.spawn(async move |this, cx| {
            let q = clear.await;
            if q["success"] != true { return; }
            if let Some(p) = p {
                for m in strings(&q["data"]["steering"]).into_iter().filter(|m| *m != text) { p.send(json!({ "type": "steer", "message": m })).await; }
                for m in strings(&q["data"]["followUp"]).into_iter().filter(|m| *m != text) { p.send(json!({ "type": "follow_up", "message": m })).await; }
            }
            let _ = this.update(cx, |_, cx| cx.emit(TabEvent::Draft(text)));
        }).detach();
    }

    pub fn bash(&mut self, command: String, exclude: bool, cx: &mut Context<Self>) {
        let ready = self.ensure(cx);
        cx.spawn(async move |this, cx| {
            if !ready.await { return; }
            let req = format!("b{}", now_ms() as u64);
            let Ok(f) = this.update(cx, |t, cx| {
                t.push(ItemKind::Bash { command: command.clone(), output: String::new(), exit: None, running: true, cancelled: false, req_id: Some(req.clone()), excluded: exclude });
                t.bump(cx);
                t.cmd(json!({ "id": req, "type": "bash", "command": command, "excludeFromContext": exclude }))
            }) else { return };
            let r = f.await;
            let _ = this.update(cx, |t, cx| {
                for it in t.items.iter_mut().rev() {
                    if let ItemKind::Bash { req_id: Some(q), output, exit, running, cancelled, .. } = &mut it.kind {
                        if *q != req { continue; }
                        *running = false;
                        if r["success"] == true { *output = s(&r["data"]["output"]); *exit = r["data"]["exitCode"].as_i64(); *cancelled = r["data"]["cancelled"] == true; }
                        else { output.push('\n'); output.push_str(r["error"].as_str().unwrap_or("")); *exit = Some(-1); }
                        break;
                    }
                }
                t.bump(cx);
            });
        }).detach();
    }

    // runs a command that needs pi, then hands back its answer on the foreground
    pub fn with_pi(&mut self, c: Value, cx: &mut Context<Self>, then: impl FnOnce(&mut Tab, Value, &mut Context<Tab>) + 'static) {
        let ready = self.ensure(cx);
        cx.spawn(async move |this, cx| {
            if !ready.await { return; }
            let Ok(f) = this.update(cx, |t, _| t.cmd(c)) else { return };
            let r = f.await;
            let _ = this.update(cx, |t, cx| then(t, r, cx));
        }).detach();
    }

    pub fn compact(&mut self, cx: &mut Context<Self>) {
        self.compacting = true;
        cx.notify();
        self.with_pi(json!({ "type": "compact" }), cx, |t, r, cx| {
            t.compacting = false;
            if r["success"] != true { t.note(Level::Error, r["error"].as_str().unwrap_or("Compaction failed").to_string(), cx); }
            cx.notify();
        });
    }

    pub fn set_model(&mut self, provider: String, id: String, cx: &mut Context<Self>) {
        self.with_pi(json!({ "type": "set_model", "provider": provider, "modelId": id }), cx, |t, r, cx| {
            if r["success"] != true { t.note(Level::Error, r["error"].as_str().unwrap_or("Couldn't switch model").to_string(), cx); return; }
            t.state["model"] = r["data"].clone();
            t.reload_state(false, cx);
        });
    }

    pub fn set_thinking(&mut self, level: String, cx: &mut Context<Self>) {
        let l = level.clone();
        self.with_pi(json!({ "type": "set_thinking_level", "level": level }), cx, move |t, r, cx| {
            if r["success"] == true { t.state["thinkingLevel"] = json!(l); cx.notify(); }
        });
    }

    pub fn set_mode(&mut self, m: Mode, cx: &mut Context<Self>) {
        self.mode = m;
        crate::state::app::set_mode(&self.id, m);
        self.save_meta();
        cx.notify();
    }

    pub fn save_meta(&self) {
        if let Some(f) = &self.file { crate::core::sys::save_meta(f, json!({ "kind": self.kind.as_str(), "mode": self.mode.as_str() })); }
    }

    pub fn rename(&mut self, name: String, cx: &mut Context<Self>) {
        self.name = Some(name.clone());
        self.state["sessionName"] = json!(name.clone());
        cx.notify();
        if self.file.is_some() || self.live_process() { self.with_pi(json!({ "type": "set_session_name", "name": name }), cx, |_, _, _| {}); }
    }

    // pi's new_session on this process. the palette doesn't offer it any more (the webview never did)
    #[allow(dead_code)]
    pub fn new_session(&mut self, cx: &mut Context<Self>) {
        self.with_pi(json!({ "type": "new_session" }), cx, |t, r, cx| {
            if r["success"] == true && r["data"]["cancelled"] != true { t.name = None; t.reload_state(true, cx); }
        });
    }

    pub fn fork(&mut self, entry_id: String, cx: &mut Context<Self>) {
        self.with_pi(json!({ "type": "fork", "entryId": entry_id }), cx, |t, r, cx| {
            if r["success"] != true || r["data"]["cancelled"] == true { t.note(Level::Error, r["error"].as_str().unwrap_or("Fork was cancelled").to_string(), cx); return; }
            t.reload_state(true, cx);
            cx.emit(TabEvent::Draft(s(&r["data"]["text"])));
        });
    }

    // a copy of this session on a new file, which this process switches to (pi's clone)
    pub fn clone_session(&mut self, cx: &mut Context<Self>) {
        self.with_pi(json!({ "type": "clone" }), cx, |t, r, cx| {
            if r["success"] == true && r["data"]["cancelled"] != true { t.reload_state(true, cx); }
        });
    }

    // pi writes the session out as html; hands back where
    pub fn export_html(&mut self, cx: &mut Context<Self>, then: impl FnOnce(&mut Tab, Option<String>, &mut Context<Tab>) + 'static) {
        self.with_pi(json!({ "type": "export_html" }), cx, move |t, r, cx| then(t, if r["success"] == true { r["data"]["path"].as_str().map(String::from) } else { None }, cx));
    }

    // the last reply's text, from pi (empty when it isn't running)
    pub fn last_text(&self) -> impl std::future::Future<Output = String> + 'static {
        let f = self.cmd(json!({ "type": "get_last_assistant_text" }));
        async move { let r = f.await; if r["success"] == true { s(&r["data"]["text"]) } else { String::new() } }
    }

    // ms since the epoch this tab was last used, for sorting next to disk times
    pub fn last_used_ms(&self) -> f64 { now_ms() - self.last_used.elapsed().as_millis() as f64 }

    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let f = self.cmd(json!({ "type": "get_entries" }));
        cx.spawn(async move |this, cx| {
            let r = f.await;
            if r["success"] == true { let _ = this.update(cx, |t, cx| { t.fill(&r["data"]["entries"], r["data"]["leafId"].as_str()); t.bump(cx); }); }
        }).detach();
    }

    fn reload_state(&mut self, entries: bool, cx: &mut Context<Self>) {
        let (st, lv) = (self.cmd(json!({ "type": "get_state" })), self.cmd(json!({ "type": "get_available_thinking_levels" })));
        cx.spawn(async move |this, cx| {
            let (st, lv) = futures::join!(st, lv);
            let _ = this.update(cx, |t, cx| {
                if st["success"] == true {
                    t.state = st["data"].clone();
                    if let Some(f) = t.state["sessionFile"].as_str() { t.file = Some(f.into()); }
                    t.save_meta();
                }
                if lv["success"] == true { t.levels = strings(&lv["data"]["levels"]); }
                if entries { t.todos.clear(); t.reload(cx); }
                t.refresh_stats(cx);
                cx.notify();
            });
        }).detach();
    }

    #[allow(dead_code)]
    pub fn context_percent(&self) -> Option<f64> { self.stats["contextUsage"]["percent"].as_f64() }
    #[allow(dead_code)]
    pub fn cost(&self) -> f64 { self.stats["cost"].as_f64().unwrap_or(0.0) }
}

// ---------- helpers ----------
pub fn s(v: &Value) -> String { v.as_str().unwrap_or("").to_string() }
pub fn strings(v: &Value) -> Vec<String> { v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default() }
pub fn now_ms() -> f64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0) }

pub fn text_of(c: &Value) -> String {
    if let Some(t) = c.as_str() { return t.into(); }
    c.as_array().map(|a| a.iter().filter(|x| x["type"] == "text").filter_map(|x| x["text"].as_str()).collect::<Vec<_>>().join("\n")).unwrap_or_default()
}

fn split_content(c: &Value) -> (String, Vec<Img>) {
    if let Some(t) = c.as_str() { return (t.into(), vec![]); }
    let imgs = c.as_array().into_iter().flatten().filter(|x| x["type"] == "image").map(|x| Img { data: s(&x["data"]), mime: s(&x["mimeType"]) }).collect();
    (text_of(c), imgs)
}

fn todos_of(v: &Value) -> Option<Vec<Todo>> {
    Some(v["todos"].as_array()?.iter().map(|t| Todo { content: s(&t["content"]), status: s(&t["status"]) }).collect())
}

pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\x1b' && it.peek() == Some(&'[') {
            it.next();
            for d in it.by_ref() { if d.is_ascii_alphabetic() { break; } }
        } else { out.push(c); }
    }
    out
}

// a sidebar-sized title from the first thing someone typed
pub fn title_from(t: &str) -> String {
    // fmt.ts titleFrom: whitespace squeezed, cut back to a word before 60
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.is_empty() { return "new session".into(); }
    if t.chars().count() <= 60 { return t; }
    let cut: String = t.chars().take(58).collect();
    let cut = match cut.rfind(' ') { Some(i) => cut[..i].to_string(), None => cut };
    format!("{cut}…")
}
