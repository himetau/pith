// What sits between the transcript and the composer: the working line, the plan, and messages waiting their turn
use crate::state::tab::{Block, ItemKind, Status, Tab, ToolStatus};
use crate::ui::theme::pal;
use crate::ui::tools;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Icon, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::time::{Duration, Instant};

// its own view so the clock can tick once a second without redrawing the transcript
pub struct Working {
    tab: Entity<Tab>,
    tick: Option<Task<()>>,
    _sub: Subscription,
}

impl Working {
    pub fn new(tab: Entity<Tab>, cx: &mut Context<Self>) -> Self {
        let sub = cx.observe(&tab, |this, _, cx| { this.clock(cx); cx.notify(); });
        let mut w = Working { tab, tick: None, _sub: sub };
        w.clock(cx);
        w
    }

    fn busy(&self, cx: &App) -> bool {
        let t = self.tab.read(cx);
        t.running() || t.compacting || t.retry.is_some() || t.status == Status::Starting
    }

    fn clock(&mut self, cx: &mut Context<Self>) {
        if !self.busy(cx) { self.tick = None; return; }
        if self.tick.is_some() { return; }
        self.tick = Some(cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(1000)).await;
            if this.update(cx, |_, cx| cx.notify()).is_err() { break; }
        }));
    }
}

fn elapsed(since: Instant) -> String {
    let s = since.elapsed().as_secs();
    if s < 60 { format!("{s}s") } else if s < 3600 { format!("{}m {:02}s", s / 60, s % 60) } else { format!("{}h {:02}m", s / 3600, (s % 3600) / 60) }
}

// the part of a tool's args that says what it's doing, on one line
pub fn gist(a: &serde_json::Value) -> String {
    let v = ["command", "path", "file_path", "pattern", "url", "query", "task", "action", "prompt", "question"].iter().find_map(|k| a[*k].as_str());
    let s = v.map(String::from).unwrap_or_default();
    let first = s.lines().next().unwrap_or("").trim();
    if first.chars().count() > 80 { format!("{}…", first.chars().take(79).collect::<String>()) } else { first.to_string() }
}

// what the run is doing right now, from the newest assistant message
fn now(t: &Tab) -> String {
    let Some(a) = t.items.iter().rev().find_map(|i| if let ItemKind::Assistant(a) = &i.kind { Some(a) } else { None }) else { return "Waiting for the model".into() };
    for b in a.blocks.iter().rev().flatten() {
        match b {
            Block::Tool(id) => if let Some(tb) = t.tools.get(id) {
                let what = match tb.status {
                    ToolStatus::Streaming => format!("Writing {} call", tb.name),
                    ToolStatus::Queued | ToolStatus::Running => format!("Running {}", tb.name),
                    _ => continue,
                };
                let g = gist(&tb.args);
                return if g.is_empty() { what } else { format!("{what} · {g}") };
            },
            Block::Thinking { ms: None, .. } if a.live => return "Thinking".into(),
            Block::Text(_) if a.live => return "Writing".into(),
            _ => {}
        }
        if a.live { break; }
    }
    if a.live { "Thinking".into() } else { "Waiting for the model".into() }
}

impl Render for Working {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = pal(cx);
        let t = self.tab.read(cx);
        let line = |spin: Hsla, head: String, rest: Option<String>| {
            h_flex().gap_2().items_center().min_w_0().text_sm().px_1()
                .child(Spinner::new().xsmall().color(spin))
                .child(div().flex_none().text_color(cx.theme().foreground).child(head))
                .when_some(rest, |d, r| d.child(div().min_w_0().truncate().text_color(p.dim).child(r)))
        };
        let el = if let Some(r) = &t.retry {
            let wait = r.until.saturating_duration_since(Instant::now());
            let when = if wait.is_zero() { "Retrying".to_string() } else { format!("Retrying in {}s", wait.as_secs() + 1) };
            let err = r.error.lines().next().unwrap_or("").to_string();
            Some(line(p.gold, when, Some(format!("attempt {} of {}{}", r.attempt, r.max, if err.is_empty() { String::new() } else { format!(" · {err}") }))))
        } else if t.compacting {
            Some(line(p.dim, "Compacting context".into(), None))
        } else if t.status == Status::Starting {
            Some(line(p.dim, "Starting pi".into(), None))
        } else if t.running() {
            let took = t.run_start.map(elapsed).unwrap_or_default();
            Some(line(p.dim, now(t), Some(took)))
        } else { None };
        div().w_full().children(el)
    }
}

// messages sent while pi works wait here until it reaches them. each can be taken back into the composer
pub fn queue(tab: &Entity<Tab>, cx: &App) -> Option<AnyElement> {
    let p = pal(cx);
    let (steer, follow) = tab.read(cx).queue.clone();
    if steer.is_empty() && follow.is_empty() { return None; }
    let rows = steer.into_iter().map(|s| ("Next", s)).chain(follow.into_iter().map(|s| ("Queued", s)));
    let mut col = v_flex().gap_1();
    for (i, (k, text)) in rows.enumerate() {
        let (t, back) = (tab.clone(), text.clone());
        col = col.child(
            h_flex().gap_2().items_center().min_w_0().pl_3().pr_1().py_0p5().rounded(cx.theme().radius).bg(cx.theme().muted).text_sm()
                .child(div().flex_none().text_xs().text_color(p.dim).child(k))
                .child(div().flex_1().min_w_0().truncate().text_color(p.mid).child(text.lines().next().unwrap_or("").to_string()))
                .child(Button::new(("unqueue", i)).icon(IconName::CornerUpLeft).ghost().xsmall().tooltip("Edit in the composer")
                    .on_click(move |_, _, cx| t.update(cx, |t, cx| t.unqueue(back.clone(), cx)))),
        );
    }
    Some(col.into_any_element())
}

// the plan from todo_write: one line with the current step, the whole list when opened
pub fn plan(tab: &Entity<Tab>, open: bool, on_toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static, cx: &App) -> Option<AnyElement> {
    let p = pal(cx);
    let t = tab.read(cx);
    let todos = &t.todos;
    let done = todos.iter().filter(|x| x.status == "completed").count();
    if todos.is_empty() || (done == todos.len() && !t.running()) { return None; }
    let cur = todos.iter().find(|x| x.status == "in_progress").map(|x| x.content.clone());
    let pips = h_flex().gap(px(3.)).children(todos.iter().map(|x| {
        let c = match x.status.as_str() { "completed" => p.green, "in_progress" => cx.theme().primary, _ => p.hairline };
        div().w(px(10.)).h(px(4.)).rounded_full().bg(c)
    }));
    Some(v_flex().gap_1()
        .child(h_flex().id("plan-head").gap_2().items_center().min_w_0().px_1().text_sm().cursor_pointer().rounded(cx.theme().radius).hover(|s| s.bg(cx.theme().muted))
            .child(Icon::new(IconName::ListTodo).xsmall().text_color(p.dim))
            .child(pips)
            .child(div().flex_none().text_color(p.mid).child(format!("{done} of {}", todos.len())))
            .when_some(cur, |d, c| d.child(div().min_w_0().truncate().text_color(p.dim).child(c)))
            .child(div().flex_1())
            .child(Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronUp }).xsmall().text_color(p.dim))
            .on_click(on_toggle))
        .when(open, |d| d.child(div().pl_1().child(tools::todo_list(todos.iter().map(|x| (x.content.as_str(), x.status.as_str())), cx))))
        .into_any_element())
}

