// The subagent extension's hub file (<agent dir>/runtime/agents-state.json), read live for the whole app.
// the agents pane draws it, and tabs watching a subagent's session follow it: running or not, a fresh read when on screen
use super::app::{Pith, View};
use crate::core::sys;
use gpui_kit::*;
use serde_json::Value;
use std::time::Duration;

pub const RUNNING: [&str; 4] = ["spawning", "thinking", "tool", "streaming"];
pub fn running(a: &Value) -> bool { RUNNING.contains(&a["status"].as_str().unwrap_or("")) }

pub struct Hub {
    pub data: Value,
    _watch: Option<notify::RecommendedWatcher>,
    // held so the channel stays open even when the watcher couldn't start
    _tx: async_channel::Sender<()>,
    _task: Task<()>,
}

struct Global_(Entity<Hub>);
impl Global for Global_ {}

impl Hub {
    pub fn global(cx: &App) -> Entity<Hub> { cx.global::<Global_>().0.clone() }

    pub fn init(cx: &mut App) {
        let hub = cx.new(|cx| {
            let (tx, rx) = async_channel::unbounded::<()>();
            let _ = tx.try_send(());
            let watch = watch(sys::agent_dir().join("runtime"), tx.clone());
            // the file changing is the main signal; a slow re-read catches a subagent dying without saying so
            let task = cx.spawn(async move |this, cx| loop {
                let tick = cx.background_executor().timer(Duration::from_secs(5));
                let _ = futures::future::select(Box::pin(rx.recv()), tick).await;
                while rx.try_recv().is_ok() {}
                let data = cx.background_spawn(async { sys::read_agent_hub() }).await;
                let changed = this.update(cx, |h: &mut Hub, cx| { if h.data != data { h.data = data; cx.notify(); true } else { false } });
                match changed { Ok(true) => { let _ = cx.update(|cx| sync_tabs(cx)); } Ok(false) => {} Err(_) => break }
            });
            Hub { data: Value::Null, _watch: watch, _tx: tx, _task: task }
        });
        cx.set_global(Global_(hub));
    }

    pub fn agents(&self) -> Vec<Value> { self.data["agents"].as_array().cloned().unwrap_or_default() }
    pub fn running_count(&self) -> usize { self.data["agents"].as_array().map(|a| a.iter().filter(|x| running(x)).count()).unwrap_or(0) }

    // a run by whatever the caller knows about it (act.agentRun): its session file, the hub's id or its task. newest wins
    #[allow(dead_code)]
    pub fn find(&self, q: &AgentRef) -> Option<AgentRun> {
        let mut hits: Vec<Value> = self.agents().into_iter().filter(|x| {
            (q.session_file.is_some() && x["sessionFile"].as_str() == q.session_file.as_deref())
                || (q.hub_id.is_some() && x["id"].as_u64() == q.hub_id)
                || (q.task.is_some() && x["task"].as_str() == q.task.as_deref())
        }).collect();
        hits.sort_by(|a, b| b["startedAt"].as_f64().unwrap_or(0.).partial_cmp(&a["startedAt"].as_f64().unwrap_or(0.)).unwrap_or(std::cmp::Ordering::Equal));
        let a = hits.first();
        let file = q.session_file.clone().or_else(|| a.and_then(|a| a["sessionFile"].as_str()).filter(|f| !f.is_empty()).map(String::from))?;
        let st = |k: &str| a.and_then(|a| a[k].as_str()).map(String::from);
        Some(AgentRun {
            file,
            cwd: st("cwd").or_else(|| q.cwd.clone()).unwrap_or_default(),
            agent: st("agent").or_else(|| q.agent.clone()).unwrap_or_else(|| "agent".into()),
            task: st("task").or_else(|| q.task.clone()).unwrap_or_default(),
            running: a.is_some_and(running),
        })
    }
}

// what a caller knows about a subagent run (a subagent tool card, a todo, a hub entry)
#[derive(Clone, Default, Debug)]
#[allow(dead_code)]
pub struct AgentRef { pub session_file: Option<String>, pub hub_id: Option<u64>, pub task: Option<String>, pub agent: Option<String>, pub cwd: Option<String> }

// a run to open as a watched tab, see Pith::open_agent
#[derive(Clone, Default, Debug)]
pub struct AgentRun { pub file: String, pub cwd: String, pub agent: String, pub task: String, pub running: bool }

impl AgentRun {
    pub fn of(a: &Value) -> Option<AgentRun> {
        let file = a["sessionFile"].as_str().filter(|f| !f.is_empty())?.to_string();
        let s = |k: &str| a[k].as_str().unwrap_or("").to_string();
        Some(AgentRun { file, cwd: s("cwd"), agent: a["agent"].as_str().unwrap_or("agent").into(), task: s("task"), running: running(a) })
    }
}

fn sync_tabs(cx: &mut App) {
    let hub = Hub::global(cx).read(cx).data.clone();
    let app = Pith::global(cx);
    let (tabs, active, chat) = { let a = app.read(cx); (a.tabs.clone(), a.active.clone(), a.view == View::Chat) };
    for t in tabs {
        if !t.read(cx).watch { continue; }
        let file = t.read(cx).file.clone();
        let run = hub["agents"].as_array().and_then(|l| l.iter().find(|x| x["sessionFile"].as_str().is_some() && x["sessionFile"].as_str() == file.as_deref())).is_some_and(running);
        let on = chat && active.as_ref() == Some(&t);
        t.update(cx, |t, cx| {
            if run != t.agent_running { t.agent_running = run; cx.notify(); }
            if on { t.peek(cx); }
        });
    }
}

// the extension rewrites agents-state*.json; only create and modify count, reading it ourselves fires access events
fn watch(dir: std::path::PathBuf, tx: async_channel::Sender<()>) -> Option<notify::RecommendedWatcher> {
    use notify::Watcher;
    let _ = std::fs::create_dir_all(&dir);
    let mut w = notify::recommended_watcher(move |ev: notify::Result<notify::Event>| {
        let Ok(ev) = ev else { return };
        if !matches!(ev.kind, notify::EventKind::Create(_) | notify::EventKind::Modify(_)) { return; }
        if ev.paths.iter().any(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("agents-state"))) { let _ = tx.try_send(()); }
    }).ok()?;
    w.watch(&dir, notify::RecursiveMode::NonRecursive).ok()?;
    Some(w)
}
