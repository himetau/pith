// Subagents running right now, live off the hub file the subagent extension keeps in <agent dir>/runtime.
// finished ones drop off; failures stay for 15 minutes so you notice them, or until dismissed
use crate::core::sys;
use crate::state::app::Pith;
use crate::state::tab::now_ms;
use crate::ui::theme::pal;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Sizable};
use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::Value;
use std::collections::HashSet;
use std::time::Duration;

const RUNNING: [&str; 4] = ["spawning", "thinking", "tool", "streaming"];
const FAILED_FOR: f64 = 15. * 60_000.;

fn running(a: &Value) -> bool { RUNNING.contains(&a["status"].as_str().unwrap_or("")) }
fn key(a: &Value) -> String { format!("{}:{}", a["id"], a["startedAt"]) }

pub struct Agents {
    hub: Value,
    dismissed: HashSet<String>,
    open: HashSet<String>,
    _watch: Option<notify::RecommendedWatcher>,
    // held so the channel stays open even when the watcher couldn't start
    _tx: async_channel::Sender<()>,
    _tasks: Vec<Task<()>>,
}

impl Agents {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let (tx, rx) = async_channel::unbounded::<()>();
        let _ = tx.try_send(());
        let watch = watch(sys::agent_dir().join("runtime"), tx.clone());
        // the file changing is the main signal; a slow re-read catches a subagent dying without saying so
        let read = cx.spawn(async move |this, cx| loop {
            let tick = cx.background_executor().timer(Duration::from_secs(5));
            let _ = futures::future::select(Box::pin(rx.recv()), tick).await;
            while rx.try_recv().is_ok() {}
            let hub = cx.background_spawn(async { sys::read_agent_hub() }).await;
            if this.update(cx, |this, cx| { if this.hub != hub { this.hub = hub; cx.notify(); } }).is_err() { break; }
        });
        // elapsed times tick while something runs
        let tick = cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            if this.update(cx, |this, cx| if this.running_count() > 0 { cx.notify(); }).is_err() { break; }
        });
        Agents { hub: Value::Null, dismissed: HashSet::new(), open: HashSet::new(), _watch: watch, _tx: tx, _tasks: vec![read, tick] }
    }

    fn all(&self) -> Vec<Value> { self.hub["agents"].as_array().cloned().unwrap_or_default() }

    pub fn running_count(&self) -> usize { self.hub["agents"].as_array().map(|a| a.iter().filter(|x| running(x)).count()).unwrap_or(0) }

    fn shown(&self) -> Vec<Value> {
        let now = now_ms();
        let mut v: Vec<Value> = self.all().into_iter().filter(|a| running(a) || (
            matches!(a["status"].as_str(), Some("failed" | "aborted")) && a["errorMessage"] != "Killed by user"
            && now - a["endedAt"].as_f64().or(a["startedAt"].as_f64()).unwrap_or(0.) < FAILED_FOR && !self.dismissed.contains(&key(a))
        )).collect();
        // running first, then newest
        v.sort_by(|a, b| running(b).cmp(&running(a)).then(b["startedAt"].as_f64().unwrap_or(0.).partial_cmp(&a["startedAt"].as_f64().unwrap_or(0.)).unwrap_or(std::cmp::Ordering::Equal)));
        v
    }

    fn card(&self, a: &Value, cx: &mut Context<Self>) -> AnyElement {
        let p = pal(cx);
        let t = cx.theme();
        let run = running(a);
        let k = key(a);
        let open = self.open.contains(&k);
        let status = a["status"].as_str().unwrap_or("");
        let status = if status == "spawning" && a["retries"].as_u64().unwrap_or(0) > 0 { "continuing" } else { status };
        let started = a["startedAt"].as_f64().unwrap_or(0.);
        let when = if run { elapsed(now_ms() - started) } else { format!("{} ago", elapsed(now_ms() - a["endedAt"].as_f64().unwrap_or(started))) };
        let u = &a["usage"];
        let toks = u["input"].as_f64().unwrap_or(0.) + u["output"].as_f64().unwrap_or(0.);
        let mut meta = vec![format!("{} turns", u["turns"].as_u64().unwrap_or(0)), format!("{} tokens", short(toks))];
        if let Some(c) = u["cost"].as_f64().filter(|c| *c > 0.) { meta.push(format!("${c:.2}")); }
        if let Some(m) = a["model"].as_str().filter(|m| !m.is_empty()) { meta.push(m.to_string()); }
        let file = a["sessionFile"].as_str().filter(|f| !f.is_empty()).map(String::from);
        let has_file = file.is_some();
        let (k1, k2) = (k.clone(), k.clone());
        let activity = a["currentActivity"].as_str().filter(|s| !s.is_empty()).map(String::from);
        let err = a["errorMessage"].as_str().filter(|s| !s.is_empty() && !run).map(String::from);

        v_flex().id(SharedString::from(format!("ag-{k}"))).p_3().gap_1().rounded(t.radius_lg).border_1()
            .border_color(if run { t.border } else { p.red.opacity(0.28) }).bg(p.raised).text_sm()
            .child(h_flex().gap_2().items_center()
                .child(if run { Spinner::new().xsmall().color(t.primary).into_any_element() } else { div().size(px(7.)).rounded_full().bg(p.red).into_any_element() })
                .child(div().font_weight(FontWeight::MEDIUM).text_color(t.foreground).child(a["agent"].as_str().unwrap_or("agent").to_string()))
                .child(div().text_xs().text_color(p.dim).child(status.to_string()))
                .when(a["retries"].as_u64().unwrap_or(0) > 0, |d| d.child(div().text_xs().px_1p5().rounded(px(4.)).border_1().border_color(p.hairline).text_color(p.dim).child(format!("retry {}", a["retries"]))))
                .child(div().flex_1())
                .child(div().text_xs().text_color(p.dim).child(when))
                .child(Button::new(SharedString::from(format!("ag-more-{k}"))).icon(if open { IconName::ChevronDown } else { IconName::ChevronRight }).ghost().xsmall().tooltip(if open { "Hide timeline" } else { "Timeline" })
                    .on_click(cx.listener(move |this, _, _, cx| { cx.stop_propagation(); if !this.open.remove(&k1) { this.open.insert(k1.clone()); } cx.notify(); })))
                .when(!run, |d| d.child(Button::new(SharedString::from(format!("ag-x-{k}"))).icon(IconName::X).ghost().xsmall().tooltip("Dismiss")
                    .on_click(cx.listener(move |this, _, _, cx| { cx.stop_propagation(); this.dismissed.insert(k2.clone()); cx.notify(); })))))
            .child(div().text_color(p.mid).line_clamp(2).child(a["task"].as_str().unwrap_or("").to_string()))
            .when_some(activity, |d, s| d.child(h_flex().gap_1().text_xs().text_color(p.mid).child(div().text_color(p.dim).child("›")).child(div().min_w_0().truncate().child(s))))
            .when_some(err, |d, e| d.child(div().text_xs().text_color(p.red).line_clamp(3).child(e)))
            .child(div().text_xs().text_color(p.dim).child(meta.join(" · ")))
            .when(open, |d| {
                let evs: Vec<Value> = a["timeline"].as_array().map(|t| t.iter().rev().take(30).rev().cloned().collect()).unwrap_or_default();
                let out = a["finalOutput"].as_str().filter(|s| !s.is_empty()).map(String::from)
                    .or_else(|| a["textStream"].as_str().filter(|s| !s.is_empty()).map(|s| { let n = s.len().saturating_sub(3000); s[s.ceil_char_boundary(n)..].to_string() }));
                d.child(v_flex().mt_1().gap_0p5().text_xs()
                    .children(evs.iter().map(|e| h_flex().gap_2().min_w_0()
                        .child(div().flex_none().text_color(p.dim).font_family(t.mono_font_family.clone()).child(clock(e["timestamp"].as_f64().unwrap_or(0.))))
                        .child(div().min_w_0().truncate().text_color(if e["type"] == "error" { p.red } else { p.mid }).child(e["summary"].as_str().unwrap_or("").to_string()))))
                    .when_some(out, |d, o| d.child(div().mt_1().p_2().rounded(t.radius).bg(p.code_bg).font_family(t.mono_font_family.clone()).text_color(p.mid).child(o))))
            })
            .when(has_file, |d| d.cursor_pointer().hover(|s| s.border_color(p.dim.opacity(0.5))))
            .on_click(move |_, _, cx| if let Some(f) = &file { let f = f.clone(); Pith::global(cx).update(cx, |a, cx| a.open(&f, cx)); })
            .into_any_element()
    }
}

impl Render for Agents {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = pal(cx);
        let shown = self.shown();
        if shown.is_empty() {
            return v_flex().size_full().items_center().justify_center().gap_1().p_6().text_sm()
                .child(div().text_color(cx.theme().foreground).child("Nothing running"))
                .child(div().text_color(p.dim).text_center().max_w(px(300.)).child("Subagents show up here while they work. Finished ones drop off; failures stay for a while."))
                .into_any_element();
        }
        let cards: Vec<AnyElement> = shown.iter().map(|a| self.card(a, cx)).collect();
        div().id("ag-list").size_full().overflow_y_scroll().child(v_flex().p_2().gap_2().children(cards)).into_any_element()
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

fn elapsed(ms: f64) -> String {
    let s = (ms / 1000.).max(0.) as u64;
    if s < 60 { format!("{s}s") } else if s < 3600 { format!("{}m {:02}s", s / 60, s % 60) } else { format!("{}h {:02}m", s / 3600, (s % 3600) / 60) }
}

fn short(n: f64) -> String {
    if n >= 1e6 { format!("{:.1}M", n / 1e6) } else if n >= 1e3 { format!("{:.1}k", n / 1e3) } else { format!("{n:.0}") }
}

// hh:mm:ss in local time, from an epoch-ms timestamp
fn clock(ms: f64) -> String {
    let secs = (ms / 1000.) as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&secs, &mut tm) };
    format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec)
}
