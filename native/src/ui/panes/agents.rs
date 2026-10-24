// Subagents running right now, across open sessions (state/hub.rs). finished ones drop off; failures stay for 15
// minutes so you notice them, or until dismissed. a card opens the run's transcript as a watched tab
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

fn key(a: &Value) -> String { a["key"].as_str().unwrap_or("").to_string() }
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
            if this.update(cx, |_, cx| if Hub::running_count(cx) > 0 { cx.notify(); }).is_err() { break; }
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
        Hub::agents(cx).into_iter().filter(|a| running(a) || (
            a["status"] == "failed" && a["endedAt"].as_f64().is_some_and(|e| now - e < FAILED_FOR) && !self.dismissed.contains(&key(a))
        )).collect()
    }

    // the dismissed list only keeps runs the hub still knows about
    fn dismiss(&mut self, k: String, cx: &mut Context<Self>) {
        self.dismissed.insert(k);
        let live: HashSet<String> = Hub::agents(cx).iter().map(key).collect();
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
        let started = a["startedAt"].as_f64().unwrap_or(0.);
        let when = if run { dur(now_ms() - started) } else { ago(a["endedAt"].as_f64().unwrap_or(started)) };
        let mut meta = vec![];
        if let Some(m) = a["model"].as_str().filter(|m| !m.is_empty()) { meta.push(m.to_string()); }
        if let Some(pid) = a["pid"].as_u64().filter(|p| *p > 0) { meta.push(format!("pid {pid}")); }
        let meta = meta.join(" · ");
        let run_of = AgentRun::of(a, cx);
        let (k1, k2) = (k.clone(), k.clone());
        // what it's saying right now: the last line of its output so far, or why it failed
        let output = a["output"].as_str().unwrap_or("").to_string();
        let activity = a["error"].as_str().filter(|e| !e.is_empty()).map(String::from)
            .or_else(|| output.lines().rev().find(|l| !l.trim().is_empty()).map(String::from));
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
            .child(div().flex_1())
            .child(div().text_color(p.dim).child(when))
            .child(mini(SharedString::from(format!("ag-more-{k}")), if open { IconName::ChevronDown } else { IconName::ChevronRight })
                .tooltip(move |w, cx| Tooltip::new(if open { "less" } else { "output" }).build(w, cx))
                .on_click(cx.listener(move |this, _, _, cx| { cx.stop_propagation(); if !this.open.remove(&k1) { this.open.insert(k1.clone()); } cx.notify(); })))
            .when(!run, |d| d.child(mini(SharedString::from(format!("ag-x-{k}")), IconName::X).tooltip(|w, cx| Tooltip::new("dismiss").build(w, cx))
                .on_click(cx.listener(move |this, _, _, cx| { cx.stop_propagation(); this.dismiss(k2.clone(), cx); }))))
            .on_click(cx.listener(move |this, _, _, cx| match &run_of {
                Some(r) => { let r = r.clone(); Pith::global(cx).update(cx, |a, cx| { a.open_agent(r, cx); }); }
                // no transcript to open: its output is all there is
                None => { if !this.open.remove(&k) { this.open.insert(k.clone()); } cx.notify(); }
            }));

        let border = if run { mix(p.primary, 0.45, p.line) } else { mix(p.red, 0.4, p.line) };
        v_flex().border(u(1.)).border_color(border).rounded(u(g.r2)).py(u(6.)).px(u(10.)).text_size(u(fs)).bg(p.surface_low)
            .child(head)
            .child(div().my(u(2.)).text_color(p.mid).line_clamp(2).child(a["task"].as_str().unwrap_or("").to_string()))
            .when_some(activity, |d, s| d.child(div().mt(u(2.)).text_size(u(fs * 0.9)).text_color(p.mid).overflow_hidden().text_ellipsis().whitespace_nowrap()
                .child(format!("› {s}"))))
            .when(!meta.is_empty(), |d| d.child(div().text_size(u(fs * 0.82)).text_color(p.dim).child(meta)))
            .when(open && !output.is_empty(), |d| d.child(div().id(SharedString::from(format!("ag-o-{}", key(a)))).mt(u(6.)).max_h(u(380.)).overflow_y_scroll()
                .text_size(u(fs * 0.86 * 0.92)).text_color(p.mid).child(output.clone())))
            .into_any_element()
    }
}

impl Render for Agents {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let shown = self.shown(cx);
        let bar = pbar(cx).child(div().h(u(26.)).flex().items_center().child("subagents")).child(div().flex_1())
            .child(btn_sm("ag-library", true, false, cx).child("library").on_click(|_, _, cx| Pith::global(cx).update(cx, |a, cx| a.show(View::Agents, cx))));
        let body: AnyElement = if shown.is_empty() {
            empty(Some("nothing running"), "when pi uses the subagent tool, or you summon one from the library, they show up here live. finished ones drop off, failures stay a while", cx).into_any_element()
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
