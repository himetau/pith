// What sits under the transcript (chat.ts .dock): the live line, the todo strip, messages waiting their turn, extension
// widgets, pi's questions and the composer, with any widgets an extension put below it
use crate::state::tab::{Status, Tab};
use crate::ui::asks::Asks;
use crate::ui::composer::Composer;
use crate::ui::frame::spin;
use crate::ui::theme::{geo, mix, pal, u};
use crate::ui::tools;
use gpui_kit::assets::IconName;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::time::{Duration, Instant};

// .tx-wrap, .dock: 900 wide at most, 28 either side
pub const DOCK_W: f32 = 900.;

pub struct Dock {
    tab: Entity<Tab>,
    composer: Entity<Composer>,
    pub working: Entity<Working>,
    pub asks: Entity<Asks>,
    todos_open: bool,
    _sub: Subscription,
}

impl Dock {
    pub fn new(tab: Entity<Tab>, composer: Entity<Composer>, cx: &mut Context<Self>) -> Self {
        let asks = cx.new(|cx| Asks::new(tab.clone(), cx));
        composer.update(cx, |c, _| c.asks = Some(asks.clone()));
        let working = cx.new(|cx| Working::new(tab.clone(), cx));
        let sub = cx.observe(&tab, |_, _, cx| cx.notify());
        // the strip starts open (chat.ts todosOpen)
        Dock { tab, composer, working, asks, todos_open: true, _sub: sub }
    }
}

impl Render for Dock {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let weak = cx.weak_entity();
        let todos = todo_strip(&self.tab, self.todos_open, move |_, _, cx| { let _ = weak.update(cx, |d, cx| { d.todos_open = !d.todos_open; cx.notify(); }); }, cx);
        let (above, below) = widgets(&self.tab, cx);
        h_flex().w_full().justify_center().child(
            v_flex().w_full().max_w(u(DOCK_W)).px(u(28.)).pt(u(4.)).pb(u(14.))
                .child(self.working.clone())
                .children(todos)
                .children(queue(&self.tab, cx))
                .children(above)
                .child(self.asks.clone())
                .child(self.composer.clone())
                .children(below),
        )
    }
}

// ---------- the live line (.live-line) ----------
// its own view so the clock can tick without redrawing the rest
pub struct Working {
    tab: Entity<Tab>,
    // the working line opens into the run's steps
    pub steps_open: bool,
    tick: Option<Task<()>>,
    _sub: Subscription,
}

impl Working {
    pub fn new(tab: Entity<Tab>, cx: &mut Context<Self>) -> Self {
        let sub = cx.observe(&tab, |this, _, cx| { this.clock(cx); cx.notify(); });
        let mut w = Working { tab, steps_open: false, tick: None, _sub: sub };
        w.clock(cx);
        w
    }

    fn busy(&self, cx: &App) -> bool {
        let t = self.tab.read(cx);
        t.running() || t.compacting || t.retry.is_some() || t.status == Status::Starting
    }

    // chat.ts ticks its timers every ~half second
    fn clock(&mut self, cx: &mut Context<Self>) {
        if !self.busy(cx) { self.tick = None; return; }
        if self.tick.is_some() { return; }
        self.tick = Some(cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(480)).await;
            if this.update(cx, |_, cx| cx.notify()).is_err() { break; }
        }));
    }
}

// lib/fmt.ts dur()
pub fn dur(ms: u64) -> String {
    if ms < 1000 { return format!("{ms}ms"); }
    let s = ms as f64 / 1000.;
    if s < 60. { return if s < 10. { format!("{s:.1}s") } else { format!("{}s", s.round()) }; }
    let m = (s / 60.).floor() as u64;
    if m < 60 { format!("{m}m{:02}s", (s % 60.).round() as u64) } else { format!("{}h{:02}m", m / 60, m % 60) }
}

// the part of a tool's args that says what it's doing, on one line (tools.rs uses it too)
pub fn gist(a: &serde_json::Value) -> String {
    let v = ["command", "path", "file_path", "pattern", "url", "query", "task", "action", "prompt", "question"].iter().find_map(|k| a[*k].as_str());
    let s = v.map(String::from).unwrap_or_default();
    let first = s.lines().next().unwrap_or("").trim();
    if first.chars().count() > 80 { format!("{}…", first.chars().take(79).collect::<String>()) } else { first.to_string() }
}

fn since(t: Instant) -> String { dur(t.elapsed().as_millis() as u64) }

// TODO(transcript): the run-steps view (chat.ts runSteps: every thinking stretch, tool call and reply since the last
// prompt with durations, and a live tail of what's happening now) belongs here. the transcript agent owns it; until
// it lands, clicking "working" only flips the chevron
fn run_steps(_tab: &Entity<Tab>, _cx: &App) -> Option<AnyElement> { None }

impl Render for Working {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (p, g) = (pal(cx), geo(cx));
        let t = self.tab.read(cx);
        let line = h_flex().w_full().flex_wrap().items_center().gap(u(8.)).min_h(u(g.fs * 0.9 * 1.6)).mt(u(8.)).mb(u(18.)).text_size(u(g.fs * 0.9)).text_color(p.mid);
        let dim = |s: String| div().text_color(p.dim).child(s);
        // TODO(panes): a watched subagent session says "subagent working · live from its session" or "a subagent's
        // finished run · type to carry on with it" here, once tabs know they're watching one
        if t.status == Status::Starting {
            return line.child(spin("live-spin")).child("starting pi").into_any_element();
        }
        if t.status == Status::Dead {
            let err = t.error.clone();
            let tab = self.tab.clone();
            let (hover, text) = (p.hover, p.text);
            let tail: String = err.chars().rev().take(1500).collect::<Vec<_>>().into_iter().rev().collect();
            return line.child(div().text_color(p.red).child("pi stopped"))
                .when(!err.is_empty(), |d| d.child(div().id("dead-err").w_full().my(u(6.)).py(u(8.)).px(u(10.)).rounded(u(5.)).bg(p.del_bg).text_color(p.mid)
                    .max_h(u(200.)).overflow_y_scroll().child(tail)))
                .child(div().id("restart").ml(u(10.)).px(u(7.)).rounded(u(4.)).border_1().border_color(p.line).text_size(u(g.fs * 0.9 * 0.8)).text_color(p.mid).cursor_pointer()
                    .hover(move |s| s.text_color(text).bg(hover)).child("start again")
                    .on_click(move |_, _, cx| tab.update(cx, |t, cx| { let _ = t.ensure(cx); })))
                .into_any_element();
        }
        if t.compacting { return line.child(spin("live-spin")).child("compacting context").into_any_element(); }
        if let Some(r) = &t.retry {
            return line.child(div().text_color(p.gold).child(spin("live-spin"))).child(format!("retry {}/{} ·", r.attempt, r.max))
                .child(dim(r.error.chars().take(120).collect())).into_any_element();
        }
        if t.running() {
            let open = self.steps_open;
            let took = t.run_start.map(since).unwrap_or_default();
            let (text, under) = (p.text, p.text);
            let head = h_flex().id("run-head").items_center().gap(u(8.)).cursor_pointer().group("run-head")
                .child(div().text_color(p.accent).child(spin("run-spin")))
                .child(div().text_color(text).group_hover("run-head", move |s| s.underline().text_decoration_color(under)).child("working"))
                .child(dim(took))
                .child(Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronRight }).size(u(g.fs * 0.9 * 1.2)).text_color(p.dim))
                .tooltip(move |w, cx| Tooltip::new(if open { "hide the steps" } else { "show what it's doing, step by step" }).build(w, cx))
                .on_click(cx.listener(|this, _, _, cx| { this.steps_open = !this.steps_open; cx.notify(); }));
            let steps = if open { run_steps(&self.tab, cx) } else { None };
            return line.child(head).child(dim("·  esc to stop".into())).children(steps).into_any_element();
        }
        div().into_any_element()
    }
}

// ---------- the todo strip (.todo-strip) ----------
pub fn todo_strip(tab: &Entity<Tab>, open: bool, on_toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static, cx: &App) -> Option<AnyElement> {
    let (p, g) = (pal(cx), geo(cx));
    let t = tab.read(cx);
    let todos = &t.todos;
    let done = todos.iter().filter(|x| x.status == "completed").count();
    if todos.is_empty() || (done == todos.len() && !t.running()) { return None; }
    let cur = todos.iter().find(|x| x.status == "in_progress").map(|x| x.content.clone());
    let half = mix(p.primary, 0.45, p.high);
    let bar = h_flex().flex_none().gap(u(2.)).children(todos.iter().enumerate().map(|(i, x)| {
        let seg = div().w(u(10.)).h(u(6.)).rounded(u(2.));
        match x.status.as_str() {
            "completed" => seg.bg(p.primary).into_any_element(),
            // .ts-bar i.in_progress blinks
            "in_progress" => seg.bg(half).with_animation(("ts-blink", i), Animation::new(Duration::from_millis(1200)).repeat(), |d, t| d.opacity(0.55 + 0.45 * (t * std::f32::consts::TAU).cos().abs())).into_any_element(),
            _ => seg.bg(p.high).into_any_element(),
        }
    }));
    let fs = g.fs * 0.9;
    Some(v_flex().mb(u(8.)).py(u(6.)).px(u(10.)).border_1().border_color(p.rule).rounded(u(g.r2)).bg(p.surface_low).text_size(u(fs))
        .child(h_flex().id("ts-head").w_full().gap(u(10.)).items_center().text_color(p.mid).cursor_pointer()
            .child(bar)
            .child(div().flex_none().child(format!("{done}/{}", todos.len())))
            .child(div().flex_1().min_w_0().truncate().text_color(p.dim).children(cur.map(|c| format!(" · {c}"))))
            .child(Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronRight }).size(u(fs * 1.2)))
            .on_click(on_toggle))
        .when(open, |d| d.child(div().mt(u(6.)).pl(u(2.)).child(tools::todo_list(todos.iter().map(|x| (x.content.as_str(), x.status.as_str())), cx))))
        .into_any_element())
}

// ---------- queued messages (.queue .qchip): each can be taken back into the composer ----------
pub fn queue(tab: &Entity<Tab>, cx: &App) -> Option<AnyElement> {
    let (p, g) = (pal(cx), geo(cx));
    let (steer, follow) = tab.read(cx).queue.clone();
    if steer.is_empty() && follow.is_empty() { return None; }
    let rows = steer.into_iter().map(|s| ("next", s)).chain(follow.into_iter().map(|s| ("after", s)));
    let fs = g.fs * 0.88;
    let mut col = v_flex().gap(u(4.)).mb(u(8.));
    for (i, (k, text)) in rows.enumerate() {
        let (t, back) = (tab.clone(), text.clone());
        let (hover, tx) = (p.hover, p.text);
        col = col.child(
            h_flex().items_center().gap(u(8.)).min_w_0().py(u(3.)).pr(u(4.)).pl(u(10.)).rounded(u(g.r2)).bg(p.raised).text_size(u(fs))
                .child(div().flex_none().text_size(u(fs * 0.85)).text_color(p.accent).child(k))
                .child(div().flex_1().min_w_0().truncate().text_color(p.mid).child(text.replace('\n', " ")))
                .child(div().id(("unqueue", i)).flex_none().size(u(22.)).rounded(u(4.)).flex().items_center().justify_center().cursor_pointer().text_color(p.dim)
                    .hover(move |s| s.text_color(tx).bg(hover))
                    .child(Icon::new(IconName::X).size(u(fs * 1.2)))
                    .tooltip(|w, cx| Tooltip::new("take it back").build(w, cx))
                    .on_click(move |_, _, cx| t.update(cx, |t, cx| t.unqueue(back.clone(), cx)))),
        );
    }
    Some(col.into_any_element())
}

// ---------- extension widgets (.widget), over the composer and under it ----------
pub fn widgets(tab: &Entity<Tab>, cx: &App) -> (Vec<AnyElement>, Vec<AnyElement>) {
    let (p, g) = (pal(cx), geo(cx));
    let t = tab.read(cx);
    let (mut above, mut below) = (vec![], vec![]);
    for (key, lines) in &t.widgets {
        let el = div().w_full().mb(u(8.)).py(u(6.)).px(u(10.)).rounded(u(g.r2)).bg(p.surface_low).text_color(p.mid).text_size(u(g.fs * 0.85))
            .child(lines.join("\n")).into_any_element();
        if t.widgets_below.contains(key) { below.push(el) } else { above.push(el) }
    }
    (above, below)
}
