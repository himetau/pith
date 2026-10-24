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

// one card per question of a pi ask_request: a tool waiting on approval, or a question from the ask tool.
// a request's answers go back together, once each of its cards is answered
#[derive(Clone, Debug)]
pub struct Ask {
    pub id: String,
    pub req: String,
    pub qid: String,
    pub kind: AskKind,
}

#[derive(Clone, Debug)]
pub enum AskKind {
    Perm { tool: String, input: Value },
    // {question, header?, options: [{label, description?}], multi?}
    Question(Value),
}

// how a card was answered
#[derive(Clone, Debug)]
pub enum Answer { Allow, AllowTool, Deny, Picked(Vec<String>), Other(String), Dismiss }

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
    // (steered into pi, waiting for the turn to take them; follow-ups, held here and sent one per run as runs end,
    // since pi can't hand one back)
    pub queue: (Vec<String>, Vec<String>),
    pub asks: Vec<Ask>,
    // answers so far per ask_request, sent once the request's last card is answered
    answers: HashMap<String, Vec<Value>>,
    // tools allowed for the rest of this session ("allow bash for this session")
    allowed: std::collections::HashSet<String>,
    // the tool call pi started last: its approval card comes right after, and doesn't say which call it's for
    last_tool: Option<String>,
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
    // the side window: the pane open for this session, None when it's closed
    pub pane: Option<crate::ui::panes::Pane>,
    // where the web pane goes first when nothing's there yet, and what its url bar shows
    pub browser_url: String,
    // the agent is using the browser right now ("agent is driving")
    pub driving: bool,
    // a subagent run this tab is showing: its transcript, replayed as it grows. read-only, its result went back
    // to the session that started it
    pub watch: bool,
    pub agent_running: bool,
    pub peeking: bool,
    peek_again: bool,
    // the subagent runs this session started, as pi reports them on the subagent tool call (state/hub.rs)
    pub subruns: Vec<Value>,
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
            queue: (vec![], vec![]), asks: vec![], answers: HashMap::new(), allowed: Default::default(), last_tool: None, commands: vec![], todos: vec![],
            unread: false, error: String::new(), compacting: false, retry: None, run_start: None, last_used: Instant::now(),
            preview: None, sent, pane: None, browser_url: String::new(), driving: false, watch: false, agent_running: false, peeking: false, peek_again: false, subruns: vec![],
            version: 0, proc: None, spawner, starting: None, typed: vec![], live: None, loaded: false,
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
        if self.watch {
            self.note(Level::Info, "a subagent's run is read-only here. its result went back to the session that started it", cx);
            return Task::ready(false).shared();
        }
        self.status = Status::Starting;
        self.error.clear();
        cx.notify();
        // a session opened off disk is painted already (fill_from_disk), no need to read it twice
        let resume = self.file.clone().filter(|_| self.items.is_empty());
        let spawned = (self.spawner)(&self.id, &self.cwd, self.file.as_deref(), self.kind, self.mode);
        let task = cx.spawn(async move |this, cx| {
            let proc = match spawned {
                Ok(p) => p,
                Err(e) => { let _ = this.update(cx, |t, cx| t.died(e, cx)); return false; }
            };
            // compact: a streaming update carries its delta only, not the whole message so far twice over
            let (st, _) = futures::join!(proc.send(json!({ "type": "get_state" })), proc.send(json!({ "type": "set_output_mode", "mode": "compact" })));
            if st["success"] != true {
                let tail = proc.last_words();
                let err = if tail.is_empty() { st["error"].as_str().unwrap_or("pi didn't start").to_string() } else { tail };
                let _ = this.update(cx, |t, cx| t.died(err, cx));
                return false;
            }
            // the whole branch off disk, compactions included: pi only hands back what's still in context
            let entries = match resume { Some(f) => Some(cx.background_spawn(async move { crate::core::sys::session_entries(&f) }).await), None => None };
            let ok = this.update(cx, |t, cx| {
                t.proc = Some(proc.clone());
                t.state = st["data"].clone();
                if let Some(f) = t.state["sessionFile"].as_str() { t.file = Some(f.to_string()); }
                // kind and mode go next to the file from the start, so a design reopens as one
                t.save_meta();
                // history goes in before anyone can send, a load landing after a live message would wipe it
                if let Some(d) = &entries { t.fill(&d["entries"], d["leafId"].as_str()); }
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
        self.levels = levels_of(&self.state["model"]);
        let cmds = self.cmd(json!({ "type": "get_commands" }));
        cx.spawn(async move |this, cx| {
            let cmds = cmds.await;
            let _ = this.update(cx, |t, cx| {
                if cmds["success"] == true {
                    // pi puts where a template came from on the end of its description, "(path:commands)"
                    let desc = |c: &Value| { let d = c["description"].as_str().unwrap_or(""); d.rsplit_once(" (").filter(|(_, t)| t.ends_with(')') && t.contains(':')).map(|(h, _)| h).unwrap_or(d).to_string() };
                    t.commands = cmds["data"]["commands"].as_array().map(|a| a.iter().map(|c| Command {
                        name: c["name"].as_str().unwrap_or("").into(), description: desc(c), source: c["source"].as_str().unwrap_or("").into(),
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
    // paints a session straight off disk, before pi is up
    pub fn fill_from_disk(&mut self, d: &Value, cx: &mut Context<Self>) {
        if self.loaded { return; }
        self.fill(&d["entries"], d["leafId"].as_str());
        self.loaded = false;
        self.bump(cx);
    }

    // re-reads a watched subagent's transcript (the events its pi wrote, one per line) and replays it from the top.
    // calls landing mid-read fold into one more read after it
    pub fn peek(&mut self, cx: &mut Context<Self>) {
        let Some(file) = self.file.clone().filter(|_| self.watch) else { return };
        if self.peeking { self.peek_again = true; return; }
        self.peeking = true;
        cx.spawn(async move |this, cx| loop {
            let f = file.clone();
            let evs: Vec<Value> = cx.background_spawn(async move {
                std::fs::read_to_string(&f).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
            }).await;
            let again = this.update(cx, |t, cx| {
                if t.watch {
                    t.items.clear();
                    t.tools.clear();
                    t.todos.clear();
                    t.live = None;
                    for e in &evs { t.on_event(e, cx); }
                    t.agent_running = !evs.iter().any(|e| e["type"] == "agent_end");
                    t.status = if t.agent_running { Status::Running } else { Status::Cold };
                    t.loaded = false;
                    t.bump(cx);
                }
                let again = std::mem::take(&mut t.peek_again);
                if !again { t.peeking = false; }
                again
            });
            if !matches!(again, Ok(true)) { break; }
        }).detach();
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
        if tb.name == "todo" { if let Some(t) = todos_of(&res.details) { self.todos = t; } }
        let sub = (tb.name == "subagent").then(|| res.details.clone());
        tb.status = if res.is_error { ToolStatus::Error } else { ToolStatus::Done };
        tb.result = Some(res);
        tb.partial = None;
        if ms.is_some() { tb.ms = ms; }
        if let Some(d) = sub { self.merge_subruns(id, &d, false); }
    }

    // pi reports one child per progress update ({result}) and all of them at the end ({results}). each run is kept
    // by its tool call and step (or agent), with when it was first seen, when it ended, and the pi that ran it
    fn note_subagent(&mut self, call: &str, details: &Value, cx: &mut Context<Self>) {
        if self.merge_subruns(call, details, true) { crate::state::hub::changed(cx); }
    }

    fn merge_subruns(&mut self, call: &str, details: &Value, live: bool) -> bool {
        let list: Vec<Value> = match (details["result"].is_object(), details["results"].as_array()) {
            (true, _) => vec![details["result"].clone()],
            (false, Some(l)) => l.clone(),
            _ => return false,
        };
        let pid = self.proc.as_ref().map(|p| p.pid);
        let now = now_ms();
        let mut changed = false;
        for (i, r) in list.into_iter().enumerate() {
            let step = r["step"].as_u64().map(|x| x.to_string()).or_else(|| r["agent"].as_str().map(String::from)).unwrap_or_else(|| i.to_string());
            let key = format!("{}/{call}/{step}", self.id);
            let ended = !matches!(r["status"].as_str(), Some("starting" | "running"));
            let pos = self.subruns.iter().position(|x| x["key"] == key.as_str());
            let mut v = r;
            v["key"] = json!(key);
            v["tab"] = json!(self.id);
            v["cwd"] = json!(v["cwd"].as_str().filter(|c| !c.is_empty()).unwrap_or(&self.cwd));
            match pos {
                Some(p) => {
                    let old = &self.subruns[p];
                    v["startedAt"] = old["startedAt"].clone();
                    v["piPid"] = old["piPid"].clone();
                    v["endedAt"] = if old["endedAt"].is_number() { old["endedAt"].clone() } else if ended && live { json!(now) } else { Value::Null };
                    if self.subruns[p] != v { self.subruns[p] = v; changed = true; }
                }
                None => {
                    v["startedAt"] = json!(if live { now } else { 0. });
                    v["piPid"] = json!(if live { pid } else { None });
                    v["endedAt"] = if ended && live { json!(now) } else { Value::Null };
                    self.subruns.push(v);
                    changed = true;
                }
            }
        }
        changed
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
                    _ => {
                        if m["role"] == "user" { let t = text_of(&m["content"]); if let Some(i) = self.queue.0.iter().position(|q| *q == t) { self.queue.0.remove(i); } }
                        self.add_message(m, None)
                    }
                }
                self.bump(cx);
            }
            "message_update" => {
                if let Some(i) = self.live { self.on_delta(i, &r["assistantMessageEvent"]); self.bump(cx); }
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
                self.last_tool = Some(id);
                if r["toolName"] == "preview" {
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
                // approval bookkeeping comes through here too, as details with no content: not output
                if r["partialResult"]["details"]["schema"].as_str().is_some_and(|x| x.starts_with("pi.tool")) { return; }
                if r["toolName"] == "subagent" { self.note_subagent(&s(&r["toolCallId"]), &r["partialResult"]["details"], cx); }
                if let Some(tb) = self.tools.get_mut(&s(&r["toolCallId"])) { tb.partial = Some(ToolResult::from(&r["partialResult"])); self.bump(cx); }
            }
            "tool_execution_end" => {
                let err = r["isError"] == true;
                let id = s(&r["toolCallId"]);
                if let Some(tb) = self.tools.get_mut(&id) {
                    if tb.result.is_none() { tb.result = Some(ToolResult::from(&r["result"])); }
                    tb.status = if err { ToolStatus::Error } else { ToolStatus::Done };
                    if let Some(ms) = r["durationMs"].as_u64() { tb.ms = Some(ms); }
                    else if let Some(st) = tb.started { tb.ms = Some(st.elapsed().as_millis() as u64); }
                    tb.partial = None;
                }
                if r["toolName"] == "todo" { if let Some(t) = todos_of(&r["result"]["details"]) { self.todos = t; } }
                if r["toolName"] == "subagent" { self.note_subagent(&id, &r["result"]["details"], cx); }
                if r["toolName"] == "browser" { self.driving = false; }
                // a card for a call that's over (pi timed it out) goes with it
                if self.last_tool.as_deref() == Some(id.as_str()) { self.last_tool = None; }
                cx.emit(TabEvent::ToolDone(s(&r["toolName"])));
                self.bump(cx);
            }
            "turn_end" => self.refresh_stats(cx),
            // one per prompt, after pi has worked through whatever was steered in
            "agent_end" => {
                self.driving = false;
                self.retry = None;
                self.state["isStreaming"] = json!(false);
                let aborted = r["error"].as_str().is_some_and(|e| e.to_lowercase().contains("abort"));
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
                self.asks.clear();
                self.answers.clear();
                self.queue.0.clear();
                if aborted { self.push(ItemKind::Note { level: Level::Warn, text: "Stopped".into() }); }
                else if let Some(e) = r["error"].as_str().filter(|e| !e.is_empty()) { self.push(ItemKind::Note { level: Level::Error, text: e.into() }); }
                // the next follow-up starts the next run; settled is when there's none left
                if !aborted && !self.queue.1.is_empty() {
                    let next = self.queue.1.remove(0);
                    // idle for a moment, so it goes as a prompt and not a steer
                    self.status = Status::Idle;
                    self.bump(cx);
                    self.send(next, vec![], How::Auto, cx);
                    return;
                }
                if self.watch { return; }
                self.status = Status::Idle;
                self.bump(cx);
                cx.emit(TabEvent::Settled);
                self.refresh_stats(cx);
                self.sync_entry_ids(cx);
            }
            "auto_compaction_start" => { self.compacting = true; cx.notify(); }
            "auto_compaction_end" => {
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
            "failover_start" => self.note(Level::Info, format!("{} failed, trying {}", s(&r["fromModel"]), s(&r["toModel"])), cx),
            "ask_request" => self.on_ask(r, cx),
            _ => {}
        }
    }

    // pi streams one content block at a time by index. a tool call only gets its id at its end, so it goes in
    // under a stand-in key until then. deltas can be dropped when pi's output backs up, the *_end events carry it whole
    fn on_delta(&mut self, i: usize, e: &Value) {
        let idx = e["contentIndex"].as_u64().map(|x| x as usize);
        let item = self.items[i].id;
        let ItemKind::Assistant(a) = &mut self.items[i].kind else { return };
        if let Some(ix) = idx { if a.blocks.len() <= ix { a.blocks.resize(ix + 1, None); } }
        let delta = e["delta"].as_str().unwrap_or("");
        match (e["type"].as_str().unwrap_or(""), idx) {
            ("text_start", Some(ix)) => a.blocks[ix] = Some(Block::Text(String::new())),
            ("text_delta", Some(ix)) => if let Some(Block::Text(t)) = &mut a.blocks[ix] { t.push_str(delta) },
            ("text_end", Some(ix)) => if let Some(c) = e["content"].as_str() { a.blocks[ix] = Some(Block::Text(c.into())) },
            ("thinking_start", Some(ix)) => a.blocks[ix] = Some(Block::Thinking { text: String::new(), redacted: false, started: Some(Instant::now()), ms: None }),
            ("thinking_delta", Some(ix)) => if let Some(Block::Thinking { text, .. }) = &mut a.blocks[ix] { text.push_str(delta) },
            ("thinking_end", Some(ix)) => if let Some(Block::Thinking { text, started, ms, .. }) = &mut a.blocks[ix] {
                if let Some(c) = e["content"].as_str() { *text = c.into(); }
                *ms = Some(started.map(|s| s.elapsed().as_millis() as u64).unwrap_or(0));
            },
            ("toolcall_start", Some(ix)) => {
                let id = format!("#{item}:{ix}");
                a.blocks[ix] = Some(Block::Tool(id.clone()));
                self.tools.insert(id.clone(), ToolBlock { id, name: String::new(), args: json!({}), args_text: String::new(), status: ToolStatus::Streaming, result: None, partial: None, ms: None, started: None });
            }
            ("toolcall_delta", Some(ix)) => if let Some(Block::Tool(id)) = &a.blocks[ix] { if let Some(tb) = self.tools.get_mut(id) { tb.args_text.push_str(delta); } },
            ("toolcall_end", Some(ix)) => {
                let call = &e["toolCall"];
                let id = s(&call["id"]);
                let mut tb = match &a.blocks[ix] { Some(Block::Tool(old)) => self.tools.remove(old), _ => None }
                    .unwrap_or_else(|| ToolBlock { id: id.clone(), name: String::new(), args: json!({}), args_text: String::new(), status: ToolStatus::Streaming, result: None, partial: None, ms: None, started: None });
                tb.id = id.clone();
                tb.name = s(&call["name"]);
                tb.args = call["arguments"].clone();
                if tb.status == ToolStatus::Streaming { tb.status = ToolStatus::Queued; }
                a.blocks[ix] = Some(Block::Tool(id.clone()));
                self.tools.insert(id, tb);
            }
            _ => {}
        }
    }

    // ---------- asks ----------
    // pi asks before a tool runs (header "approval") and for the ask tool. approvals are settled here when the
    // session's mode says so; pi itself always asks, since its own mode can't change while it runs
    fn on_ask(&mut self, r: &Value, cx: &mut Context<Self>) {
        // a replayed transcript's questions were answered long ago
        if self.watch { return; }
        let req = s(&r["id"]);
        let qs = r["questions"].as_array().cloned().unwrap_or_default();
        for (n, q) in qs.iter().enumerate() {
            let qid = q["id"].as_str().map(String::from).unwrap_or_else(|| n.to_string());
            let kind = if q["header"] == "approval" {
                let tb = self.last_tool.as_ref().and_then(|id| self.tools.get(id));
                let tool = tb.map(|t| t.name.clone()).unwrap_or_else(|| q["question"].as_str().and_then(|t| t.split('`').nth(1)).unwrap_or("").to_string());
                let input = tb.map(|t| t.args.clone()).unwrap_or(Value::Null);
                if let Some(yes) = self.auto_answer(&tool, &input) {
                    self.answers.entry(req.clone()).or_default().push(answer_json(&qid, if yes { &Answer::Allow } else { &Answer::Deny }));
                    continue;
                }
                AskKind::Perm { tool, input }
            } else {
                AskKind::Question(q.clone())
            };
            self.asks.push(Ask { id: format!("{req}/{qid}"), req: req.clone(), qid, kind });
        }
        self.flush_answers(&req);
        if let Some(ms) = r["timeoutMs"].as_u64() {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(std::time::Duration::from_millis(ms)).await;
                let _ = this.update(cx, |t, cx| { t.asks.retain(|a| a.req != req); t.answers.remove(&req); cx.notify(); });
            }).detach();
        }
        cx.notify();
    }

    // Some(allow) when the mode decides, None to ask the user
    fn auto_answer(&self, tool: &str, input: &Value) -> Option<bool> {
        let edit = matches!(tool, "edit" | "write" | "hashline_edit" | "ast_edit" | "generate_image");
        match self.mode {
            // asking the user, keeping the plan and pith's own panes are never worth a card of their own
            _ if matches!(tool, "ask" | "todo" | "submit_plan" | "preview" | "browser" | "report_findings") => Some(true),
            Mode::Auto => Some(true),
            Mode::Edits if edit => Some(true),
            _ if self.allowed.contains(tool) => Some(true),
            Mode::Ask | Mode::Edits if tool == "bash" && safe_command(input["command"].as_str().unwrap_or("")) => Some(true),
            _ => None,
        }
    }

    pub fn answer(&mut self, id: &str, ans: Answer, cx: &mut Context<Self>) {
        let Some(pos) = self.asks.iter().position(|a| a.id == id) else { return };
        let a = self.asks.remove(pos);
        if let (Answer::AllowTool, AskKind::Perm { tool, .. }) = (&ans, &a.kind) { self.allowed.insert(tool.clone()); }
        if matches!(ans, Answer::Dismiss) {
            // one skipped question drops the request
            self.asks.retain(|x| x.req != a.req);
            self.answers.remove(&a.req);
            if let Some(p) = &self.proc { p.ask_response(json!({ "requestId": a.req, "dismissed": true })); }
        } else {
            self.answers.entry(a.req.clone()).or_default().push(answer_json(&a.qid, &ans));
            self.flush_answers(&a.req);
        }
        cx.notify();
    }

    // sends a request's answers once none of its cards are left
    fn flush_answers(&mut self, req: &str) {
        if self.asks.iter().any(|a| a.req == req) { return; }
        let Some(answers) = self.answers.remove(req) else { return };
        if let Some(p) = &self.proc { p.ask_response(json!({ "requestId": req, "answers": answers })); }
    }

    // spend and counts from get_session_stats, the context meter from get_state (pi estimates it per branch)
    pub fn refresh_stats(&mut self, cx: &mut Context<Self>) {
        let (f, g) = (self.cmd(json!({ "type": "get_session_stats" })), self.cmd(json!({ "type": "get_state" })));
        cx.spawn(async move |this, cx| {
            let (r, st) = futures::join!(f, g);
            if r["success"] != true { return; }
            let _ = this.update(cx, |t, cx| {
                if let Some(f) = r["data"]["sessionFile"].as_str() { t.file = Some(f.into()); }
                let mut stats = r["data"].clone();
                if st["success"] == true {
                    let d = &st["data"];
                    let (tokens, window) = (d["contextTokens"].as_f64(), d["contextWindow"].as_f64());
                    stats["contextUsage"] = json!({ "tokens": tokens, "contextWindow": window, "percent": tokens.zip(window).filter(|(_, w)| *w > 0.).map(|(t, w)| t / w * 100.) });
                    t.state["isStreaming"] = d["isStreaming"].clone();
                    t.state["thinkingLevel"] = d["thinkingLevel"].clone();
                }
                t.stats = stats;
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
            let Ok(cmd) = this.update(cx, |t, cx| {
                t.last_used = Instant::now();
                // pi expands a /template before it echoes the prompt back; what was typed is kept to show instead
                if text.starts_with('/') { t.typed.push(text.clone()); }
                let mut base = json!({ "message": text });
                if !images.is_empty() { base["images"] = json!(images.iter().map(|i| json!({ "type": "image", "data": i.data, "mimeType": i.mime })).collect::<Vec<_>>()); }
                let streaming = t.running() || t.state["isStreaming"] == true;
                if streaming && how == How::FollowUp {
                    t.queue.1.push(text.clone());
                    cx.notify();
                    return futures::future::ready(json!({ "success": true })).boxed();
                }
                if streaming {
                    t.queue.0.push(text.clone());
                    if text.starts_with('/') { base["type"] = json!("prompt"); base["streamingBehavior"] = json!("steer"); }
                    else { base["type"] = json!("steer"); }
                } else {
                    base["type"] = json!("prompt");
                    t.status = Status::Running;
                    t.run_start = Some(Instant::now());
                }
                cx.notify();
                t.cmd(base).boxed()
            }) else { return };
            let r = cmd.await;
            let _ = this.update(cx, |t, cx| {
                if r["success"] != true {
                    if t.status == Status::Running && t.state["isStreaming"] != true { t.status = Status::Idle; }
                    t.note(Level::Error, r["error"].as_str().unwrap_or("pi didn't take that").to_string(), cx);
                }
                cx.notify();
            });
        }).detach();
    }

    // stops the run; whatever was waiting goes back into the composer
    pub fn abort(&mut self, cx: &mut Context<Self>) {
        if !self.running() { return; }
        let a = self.cmd(json!({ "type": "abort" }));
        cx.background_spawn(a).detach();
        let (steer, follow) = std::mem::take(&mut self.queue);
        let back: Vec<String> = steer.into_iter().chain(follow).collect();
        if !back.is_empty() { cx.emit(TabEvent::Draft(back.join("\n\n"))); }
        cx.notify();
    }

    // takes a queued follow-up back into the composer
    pub fn unqueue(&mut self, text: String, cx: &mut Context<Self>) {
        if let Some(i) = self.queue.1.iter().position(|q| *q == text) { self.queue.1.remove(i); }
        cx.emit(TabEvent::Draft(text));
        cx.notify();
    }

    // !cmd runs through pi, so the output joins the context. !!cmd keeps it out: pith runs that one itself
    pub fn bash(&mut self, command: String, exclude: bool, cx: &mut Context<Self>) {
        let req = format!("b{}", now_ms() as u64);
        self.push(ItemKind::Bash { command: command.clone(), output: String::new(), exit: None, running: true, cancelled: false, req_id: Some(req.clone()), excluded: exclude });
        self.bump(cx);
        let run: futures::future::LocalBoxFuture<'static, Value> = if exclude {
            let cwd = self.cwd.clone();
            cx.background_spawn(async move {
                match std::process::Command::new("sh").arg("-c").arg(&command).current_dir(&cwd).output() {
                    Ok(o) => {
                        let mut out = String::from_utf8_lossy(&o.stdout).into_owned();
                        out.push_str(&String::from_utf8_lossy(&o.stderr));
                        json!({ "success": true, "data": { "output": out, "exitCode": o.status.code() } })
                    }
                    Err(e) => json!({ "success": false, "error": e.to_string() }),
                }
            }).boxed_local()
        } else {
            let ready = self.ensure(cx);
            let this = cx.weak_entity();
            let mut acx = cx.to_async();
            async move {
                if !ready.await { return json!({ "success": false, "error": "pi isn't running" }); }
                let Ok(f) = this.update(&mut acx, |t, _| t.cmd(json!({ "type": "bash", "command": command }))) else { return Value::Null };
                f.await
            }.boxed_local()
        };
        cx.spawn(async move |this, cx| {
            let r = run.await;
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
            let err = r["error"].as_str().unwrap_or("Compaction failed");
            // pi says "not available" when everything still fits in what it keeps anyway
            if r["success"] != true { if err.contains("not available") { t.note(Level::Info, "Nothing to compact yet", cx) } else { t.note(Level::Error, err.to_string(), cx) } }
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

    // ask, edits and auto are pith's to apply (on_ask); plan is pi's own plan mode. leaving it approves a plan
    // the model handed in, so carrying it out is the next thing, else just turns it off
    pub fn set_mode(&mut self, m: Mode, cx: &mut Context<Self>) {
        let was = self.mode;
        self.mode = m;
        if let Some(p) = self.proc.clone().filter(|_| (was == Mode::Plan) != (m == Mode::Plan)) {
            cx.background_spawn(async move {
                if m == Mode::Plan { p.send(json!({ "type": "set_plan_mode", "mode": "on" })).await; }
                else if p.send(json!({ "type": "approve_plan" })).await["success"] != true { p.send(json!({ "type": "set_plan_mode", "mode": "off" })).await; }
            }).detach();
        }
        self.save_meta();
        cx.notify();
    }

    pub fn save_meta(&self) {
        if self.watch { return; }
        if let Some(f) = &self.file { crate::core::sys::save_meta(f, json!({ "kind": self.kind.as_str(), "mode": self.mode.as_str() })); }
    }

    pub fn rename(&mut self, name: String, cx: &mut Context<Self>) {
        self.name = Some(name.clone());
        self.state["sessionName"] = json!(name.clone());
        cx.notify();
        if self.file.is_some() || self.live_process() { self.with_pi(json!({ "type": "set_session_name", "name": name }), cx, |_, _, _| {}); }
    }

    pub fn fork(&mut self, entry_id: String, cx: &mut Context<Self>) {
        self.with_pi(json!({ "type": "fork", "entryId": entry_id }), cx, |t, r, cx| {
            if r["success"] != true || r["data"]["cancelled"] == true { t.note(Level::Error, r["error"].as_str().unwrap_or("Fork was cancelled").to_string(), cx); return; }
            t.reload_state(true, cx);
            cx.emit(TabEvent::Draft(s(&r["data"]["text"])));
        });
    }

    // a copy of this session on a new file, which this process switches to. pi only forks before a prompt,
    // so the copy is made here: same entries, a new session id, the old file as its parent
    pub fn clone_session(&mut self, cx: &mut Context<Self>) {
        let Some(file) = self.file.clone() else { self.note(Level::Warn, "nothing to clone yet", cx); return };
        let ready = self.ensure(cx);
        cx.spawn(async move |this, cx| {
            if !ready.await { return; }
            let copy = cx.background_spawn(async move { crate::core::sessions::clone_file(&file) }).await;
            let Ok(Ok(f)) = this.update(cx, |t, _| copy.map(|c| t.cmd(json!({ "type": "switch_session", "sessionPath": c })))) else {
                let _ = this.update(cx, |t, cx| t.note(Level::Error, "couldn't copy the session", cx));
                return;
            };
            let r = f.await;
            let _ = this.update(cx, |t, cx| {
                if r["success"] == true && r["data"]["cancelled"] != true { t.reload_state(true, cx); }
                else { t.note(Level::Error, r["error"].as_str().unwrap_or("couldn't switch to the copy").to_string(), cx); }
            });
        }).detach();
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

    // same prompt, fresh answer: branch from the prompt that started a turn and send it again (transcript.ts retry).
    // the branch's history is in before the prompt goes, so the new run streams onto it
    pub fn retry(&mut self, entry_id: String, text: String, images: Vec<Img>, cx: &mut Context<Self>) {
        if self.running() { return; }
        self.with_pi(json!({ "type": "fork", "entryId": entry_id }), cx, move |t, r, cx| {
            if r["success"] != true || r["data"]["cancelled"] == true { t.note(Level::Error, r["error"].as_str().unwrap_or("fork was cancelled").to_string(), cx); return; }
            let st = t.cmd(json!({ "type": "get_state" }));
            cx.spawn(async move |this, cx| {
                let st = st.await;
                let Ok(file) = this.update(cx, |t, _| {
                    if st["success"] == true {
                        t.state = st["data"].clone();
                        if let Some(f) = t.state["sessionFile"].as_str() { t.file = Some(f.into()); }
                        t.save_meta();
                    }
                    t.file.clone()
                }) else { return };
                let d = match file { Some(f) => cx.background_spawn(async move { crate::core::sys::session_entries(&f) }).await, None => Value::Null };
                let _ = this.update(cx, |t, cx| {
                    t.todos.clear();
                    t.fill(&d["entries"], d["leafId"].as_str());
                    t.bump(cx);
                    t.send(text, images, How::Auto, cx);
                });
            }).detach();
        });
    }

    // the transcript again, off the session file (pi writes each entry as it happens)
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let Some(f) = self.file.clone() else { return };
        cx.spawn(async move |this, cx| {
            let d = cx.background_spawn(async move { crate::core::sys::session_entries(&f) }).await;
            let _ = this.update(cx, |t, cx| { t.fill(&d["entries"], d["leafId"].as_str()); t.bump(cx); });
        }).detach();
    }

    fn reload_state(&mut self, entries: bool, cx: &mut Context<Self>) {
        let st = self.cmd(json!({ "type": "get_state" }));
        cx.spawn(async move |this, cx| {
            let st = st.await;
            let _ = this.update(cx, |t, cx| {
                if st["success"] == true {
                    t.state = st["data"].clone();
                    if let Some(f) = t.state["sessionFile"].as_str() { t.file = Some(f.into()); }
                    t.save_meta();
                    t.levels = levels_of(&t.state["model"]);
                }
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
// what set_thinking_level takes for a model. pi clamps anything past what the model does, so xhigh is safe to offer
pub fn levels_of(model: &Value) -> Vec<String> {
    let l: &[&str] = if model["reasoning"] == true { &["off", "minimal", "low", "medium", "high", "xhigh"] } else { &["off"] };
    l.iter().map(|x| x.to_string()).collect()
}

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

// pi's todo list (details of its todo tool, schema todo_list.v1), flattened out of its phases
fn todos_of(details: &Value) -> Option<Vec<Todo>> {
    let phases = details["list"]["phases"].as_array()?;
    Some(phases.iter().flat_map(|p| p["tasks"].as_array().cloned().unwrap_or_default()).map(|t| Todo {
        content: s(&t["content"]),
        status: match t["state"].as_str().unwrap_or("") { "in_progress" => "in_progress", "done" | "dropped" => "completed", _ => "pending" }.into(),
    }).collect())
}

fn answer_json(qid: &str, a: &Answer) -> Value {
    match a {
        Answer::Allow | Answer::AllowTool => json!({ "questionId": qid, "selected": ["Allow"] }),
        Answer::Deny | Answer::Dismiss => json!({ "questionId": qid, "selected": ["Deny"] }),
        Answer::Picked(l) => json!({ "questionId": qid, "selected": l }),
        Answer::Other(t) => json!({ "questionId": qid, "selected": [], "other": t }),
    }
}

// a shell command that only looks around: fine to run without asking in ask and edits modes
fn safe_command(cmd: &str) -> bool {
    const READ: &[&str] = &["ls", "cat", "head", "tail", "wc", "pwd", "rg", "grep", "find", "fd", "tree", "file", "stat", "which", "echo", "git status", "git diff", "git log", "git show"];
    const WRITE: &[&str] = &[">", "|", ";", "&", "`", "$(", "-delete", "-exec", "-fprint"];
    let c = cmd.trim();
    !c.contains('\n') && !WRITE.iter().any(|w| c.contains(w))
        && READ.iter().any(|r| c == *r || c.starts_with(&format!("{r} ")))
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
