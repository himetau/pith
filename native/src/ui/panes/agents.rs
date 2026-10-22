// Subagents running right now, live off the hub (state/hub.rs). finished ones drop off; failures stay for 15 minutes
// so you notice them, or until dismissed. a card opens the run as a watched tab
use super::{btn_sm, empty, pbar};
use crate::core::sys;
use crate::state::app::{Pith, View};
use crate::state::hub::{running, AgentRun, Hub};
use crate::state::tab::now_ms;
use crate::ui::frame::spin;
use crate::ui::theme::{geo, mix, pal, u};
use gpui_kit::assets::IconName;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::Value;
use std::collections::HashSet;
use std::time::Duration;

const FAILED_FOR: f64 = 15. * 60_000.;

fn key(a: &Value) -> String { format!("{}:{}", a["id"], a["startedAt"]) }
// dismissals outlive a restart, like the reference's localStorage
fn dismissed_file() -> std::path::PathBuf { sys::data_dir().join("agents-dismissed.json") }

pub struct Agents {
    dismissed: HashSet<String>,
    open: HashSet<String>,
    _tasks: Vec<Task<()>>,
    _sub: Subscription,
}

impl Agents {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let hub = Hub::global(cx);
        let sub = cx.observe(&hub, |_, _, cx| cx.notify());
        // elapsed times tick while something runs; failures age out on their own, not only when the hub changes
        let tick = cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            if this.update(cx, |_, cx| if Hub::global(cx).read(cx).running_count() > 0 { cx.notify(); }).is_err() { break; }
        });
        let age = cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_secs(60)).await;
            if this.update(cx, |_, cx| cx.notify()).is_err() { break; }
        });
        let dismissed = std::fs::read_to_string(dismissed_file()).ok().and_then(|t| serde_json::from_str::<Vec<String>>(&t).ok()).unwrap_or_default().into_iter().collect();
        Agents { dismissed, open: HashSet::new(), _tasks: vec![tick, age], _sub: sub }
    }

    fn shown(&self, cx: &App) -> Vec<Value> {
        let now = now_ms();
        let mut v: Vec<Value> = Hub::global(cx).read(cx).agents().into_iter().filter(|a| running(a) || (
            matches!(a["status"].as_str(), Some("failed" | "aborted")) && a["errorMessage"] != "Killed by user"
            && now - a["endedAt"].as_f64().filter(|e| *e > 0.).or(a["startedAt"].as_f64()).unwrap_or(0.) < FAILED_FOR && !self.dismissed.contains(&key(a))
        )).collect();
        v.sort_by(|a, b| b["startedAt"].as_f64().unwrap_or(0.).partial_cmp(&a["startedAt"].as_f64().unwrap_or(0.)).unwrap_or(std::cmp::Ordering::Equal));
        v
    }

    // the dismissed list only keeps runs the hub still knows about
    fn dismiss(&mut self, k: String, cx: &mut Context<Self>) {
        self.dismissed.insert(k);
        let live: HashSet<String> = Hub::global(cx).read(cx).agents().iter().map(key).collect();
        self.dismissed.retain(|k| live.contains(k));
        let list: Vec<&String> = self.dismissed.iter().collect();
        let _ = std::fs::create_dir_all(sys::data_dir());
        let _ = std::fs::write(dismissed_file(), serde_json::to_string(&list).unwrap_or_default());
        cx.notify();
    }

    fn card(&self, a: &Value, cx: &mut Context<Self>) -> AnyElement {
        let (p, g) = (pal(cx), geo(cx));
        let run = running(a);
        let k = key(a);
        let open = self.open.contains(&k);
        let status = a["status"].as_str().unwrap_or("");
        let retries = a["retries"].as_u64().unwrap_or(0);
        let status = if status == "spawning" && retries > 0 { "continuing" } else { status };
        let started = a["startedAt"].as_f64().unwrap_or(0.);
        let when = if run { dur(now_ms() - started) } else { ago(a["endedAt"].as_f64().filter(|e| *e > 0.).unwrap_or(started)) };
        let u_ = &a["usage"];
        let toks = u_["input"].as_f64().unwrap_or(0.) + u_["output"].as_f64().unwrap_or(0.);
        let mut meta = format!("{} turns · {} tok · {}", u_["turns"].as_u64().unwrap_or(0), tokens(toks), cost(u_["cost"].as_f64().unwrap_or(0.)));
        if let Some(m) = a["model"].as_str().filter(|m| !m.is_empty()) { meta.push_str(&format!(" · {m}")); }
        if let Some(pid) = a["pid"].as_u64().filter(|p| *p > 0) { meta.push_str(&format!(" · pid {pid}")); }
        let run_of = AgentRun::of(a);
        let (k1, k2) = (k.clone(), k.clone());
        let activity = a["currentActivity"].as_str().filter(|s| !s.is_empty()).map(String::from);
        let fs = g.fs * 0.9;
        let (text, hover, mid) = (p.text, p.hover, p.mid);
        let mini = move |id: SharedString, icon: IconName| div().id(id).px(u(2.)).rounded(u(4.)).flex().items_center().self_center().text_color(mid).cursor_pointer()
            .hover(move |s| s.text_color(text).bg(hover)).child(Icon::new(icon).size(u(fs * 0.8 * 1.2)));

        let head = h_flex().id(SharedString::from(format!("ag-h-{k}"))).gap(u(8.)).items_baseline().cursor_pointer()
            .when(run_of.is_some(), |d| d.tooltip(|w, cx| Tooltip::new("open its session").build(w, cx)))
            .child(if run { div().text_color(p.accent).child(spin(SharedString::from(format!("ag-s-{k}")))).into_any_element() }
                else { div().text_color(p.red).text_size(u(fs * 0.7)).child("●").into_any_element() })
            .child(div().text_color(p.tertiary).child(a["agent"].as_str().unwrap_or("agent").to_string()))
            .child(div().text_color(p.dim).text_size(u(fs * 0.85)).child(status.to_string()))
            .when(retries > 0, |d| d.child(div().id(SharedString::from(format!("ag-r-{k}"))).text_size(u(fs * 0.75)).text_color(p.dim).border(u(1.)).border_color(p.line).rounded(u(3.)).px(u(4.))
                .tooltip(|w, cx| Tooltip::new("auto-continued after it failed").build(w, cx)).child(format!("↻ {retries}"))))
            .child(div().flex_1())
            .child(div().text_color(p.dim).child(when))
            .child(mini(SharedString::from(format!("ag-more-{k}")), if open { IconName::ChevronDown } else { IconName::ChevronRight })
                .tooltip(move |w, cx| Tooltip::new(if open { "less" } else { "timeline" }).build(w, cx))
                .on_click(cx.listener(move |this, _, _, cx| { cx.stop_propagation(); if !this.open.remove(&k1) { this.open.insert(k1.clone()); } cx.notify(); })))
            .when(!run, |d| d.child(mini(SharedString::from(format!("ag-x-{k}")), IconName::X).tooltip(|w, cx| Tooltip::new("dismiss").build(w, cx))
                .on_click(cx.listener(move |this, _, _, cx| { cx.stop_propagation(); this.dismiss(k2.clone(), cx); }))))
            .on_click(cx.listener(move |this, _, _, cx| match &run_of {
                Some(r) => { let r = r.clone(); Pith::global(cx).update(cx, |a, cx| { a.open_agent(r, cx); }); }
                // no session to open: the timeline is all there is
                None => { if !this.open.remove(&k) { this.open.insert(k.clone()); } cx.notify(); }
            }));

        let border = if run { mix(p.primary, 0.45, p.line) } else { mix(p.red, 0.4, p.line) };
        v_flex().border(u(1.)).border_color(border).rounded(u(g.r2)).py(u(6.)).px(u(10.)).text_size(u(fs)).bg(p.surface_low)
            .child(head)
            .child(div().my(u(2.)).text_color(p.mid).line_clamp(2).child(a["task"].as_str().unwrap_or("").to_string()))
            .when_some(activity, |d, s| d.child(div().mt(u(2.)).text_size(u(fs * 0.9)).text_color(p.mid).overflow_hidden().text_ellipsis().whitespace_nowrap()
                .child(format!("› {s}"))))
            .child(div().text_size(u(fs * 0.82)).text_color(p.dim).child(meta))
            .when(open, |d| {
                let evs: Vec<Value> = a["timeline"].as_array().map(|t| t.iter().rev().take(30).rev().cloned().collect()).unwrap_or_default();
                let out = a["finalOutput"].as_str().filter(|s| !s.is_empty()).map(String::from)
                    .or_else(|| a["textStream"].as_str().filter(|s| !s.is_empty()).map(|s| { let n = s.len().saturating_sub(3000); s[s.ceil_char_boundary(n)..].to_string() }));
                d.child(v_flex().mt(u(6.)).gap(u(1.)).text_size(u(fs * 0.86))
                    .children(evs.iter().map(|e| h_flex().min_w_0().whitespace_nowrap().text_color(if e["type"] == "error" { p.red } else { p.mid })
                        .child(div().flex_none().text_color(p.dim).child(format!("{}\u{a0}", clock(e["timestamp"].as_f64().unwrap_or(0.)))))
                        .child(div().min_w_0().overflow_hidden().text_ellipsis().child(e["summary"].as_str().unwrap_or("").to_string()))))
                    .when_some(out, |d, o| d.child(div().id(SharedString::from(format!("ag-o-{}", key(a)))).mt(u(6.)).max_h(u(380.)).overflow_y_scroll()
                        .text_size(u(fs * 0.86 * 0.92)).text_color(p.mid).child(o))))
            })
            .into_any_element()
    }
}

impl Render for Agents {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let shown = self.shown(cx);
        let bar = pbar(cx).child(div().h(u(26.)).flex().items_center().child("subagents")).child(div().flex_1())
            .child(btn_sm("ag-library", true, false, cx).child("library").on_click(|_, _, cx| Pith::global(cx).update(cx, |a, cx| a.show(View::Agents, cx))));
        let body: AnyElement = if shown.is_empty() {
            empty(Some("nothing running"), "when pi uses the subagent tool, or /summon, they show up here live. finished ones drop off, failures stay a while", cx).into_any_element()
        } else {
            let cards: Vec<AnyElement> = shown.iter().map(|a| self.card(a, cx)).collect();
            div().id("ag-list").flex_1().min_h_0().overflow_y_scroll().child(v_flex().p(u(8.)).gap(u(6.)).children(cards)).into_any_element()
        };
        v_flex().size_full().child(bar).child(body)
    }
}

// fmt.ts dur, the live clock of a running agent
fn dur(ms: f64) -> String {
    let ms = ms.max(0.);
    if ms < 1000. { return format!("{}ms", ms.round()); }
    let s = ms / 1000.;
    if s < 60. { return if s < 10. { format!("{s:.1}s") } else { format!("{}s", s.round()) }; }
    let m = (s / 60.).floor() as u64;
    if m < 60 { return format!("{m}m{:02}s", (s % 60.).round() as u64); }
    format!("{}h{:02}m", m / 60, m % 60)
}

// fmt.ts ago
fn ago(ts: f64) -> String {
    let s = ((now_ms() - ts) / 1000.).max(0.);
    if s < 45. { "now".into() } else if s < 3600. { format!("{}m", (s / 60.).round()) } else if s < 86400. { format!("{}h", (s / 3600.).round()) } else { format!("{}d", (s / 86400.).round()) }
}

// fmt.ts tokens and cost
fn tokens(n: f64) -> String {
    if n <= 0. { "0".into() } else if n < 1000. { format!("{n}") } else if n < 10_000. { format!("{:.1}k", n / 1000.) } else if n < 1e6 { format!("{}k", (n / 1000.).round()) }
    else if n < 1e7 { format!("{:.2}M", n / 1e6) } else { format!("{:.1}M", n / 1e6) }
}
fn cost(n: f64) -> String {
    if n <= 0. { "$0".into() } else if n < 0.01 { format!("${n:.4}") } else if n < 10. { format!("${n:.2}") } else if n < 1000. { format!("${n:.1}") } else { format!("${}", n.round()) }
}

// hh:mm:ss in local time, from an epoch-ms timestamp
fn clock(ms: f64) -> String {
    let secs = (ms / 1000.) as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&secs, &mut tm) };
    format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec)
}
