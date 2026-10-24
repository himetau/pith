// Subagent runs across every open session, for the agents pane and the links in the chat. pi reports each child on
// the subagent tool call that started it (Tab::subruns); each child's events stream into a transcript under
// <agent dir>/agent-hub/<pi pid>/<agent>-<n>.transcript.jsonl, which is what opening a run replays
use super::app::Pith;
use crate::core::sys;
use gpui_kit::*;
use serde_json::Value;

pub fn running(a: &Value) -> bool { matches!(a["status"].as_str(), Some("starting" | "running")) }

// only here to be observed: tabs call changed() when a run moves
pub struct Hub;

struct Global_(Entity<Hub>);
impl Global for Global_ {}

impl Hub {
    pub fn global(cx: &App) -> Entity<Hub> { cx.global::<Global_>().0.clone() }
    pub fn init(cx: &mut App) { let h = cx.new(|_| Hub); cx.set_global(Global_(h)); }

    // every run in every open session, newest first
    pub fn agents(cx: &App) -> Vec<Value> {
        let mut v: Vec<Value> = Pith::global(cx).read(cx).tabs.iter().flat_map(|t| t.read(cx).subruns.clone()).collect();
        v.sort_by(|a, b| b["startedAt"].as_f64().unwrap_or(0.).total_cmp(&a["startedAt"].as_f64().unwrap_or(0.)));
        v
    }
    pub fn running_count(cx: &App) -> usize { Self::agents(cx).iter().filter(|a| running(a)).count() }
}

pub fn changed(cx: &mut App) {
    if let Some(h) = cx.try_global::<Global_>().map(|g| g.0.clone()) { h.update(cx, |_, cx| cx.notify()); }
}

// a run to open as a watched tab, see Pith::open_agent
#[derive(Clone, Default, Debug)]
pub struct AgentRun { pub file: String, pub cwd: String, pub agent: String, pub task: String, pub running: bool }

impl AgentRun {
    // its transcript: the n-th run of that agent by that pi, where n counts the session's runs of it in order
    pub fn of(a: &Value, cx: &App) -> Option<AgentRun> {
        let pid = a["piPid"].as_u64()?;
        let agent = a["agent"].as_str().unwrap_or("agent").to_string();
        let tab = a["tab"].as_str().unwrap_or("");
        let app = Pith::global(cx).read(cx);
        let t = app.tabs.iter().find(|t| t.read(cx).id == tab)?.read(cx);
        let n = t.subruns.iter().filter(|r| r["piPid"].as_u64() == Some(pid) && r["agent"].as_str() == Some(&agent))
            .position(|r| r["key"] == a["key"])? + 1;
        let file = sys::subagent_transcript(pid, &agent, n)?;
        let s = |k: &str| a[k].as_str().unwrap_or("").to_string();
        Some(AgentRun { file, cwd: s("cwd"), agent, task: s("task"), running: running(a) })
    }
}
