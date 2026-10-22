// The bar under the windows, waybar-ish: what the session is doing on the left, what it costs on the right
use super::overlay::Tip as _;
use super::theme::{em, pal, u};
use crate::state::app::{Pith, View};
use crate::state::tab::{Mode, Status, Tab};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;
use std::collections::HashMap;

pub struct StatusLine {
    app: Entity<Pith>,
    branch: HashMap<String, String>,
    pi_version: String,
    watched: Option<EntityId>,
    _subs: Vec<Subscription>,
}

impl StatusLine {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let app = Pith::global(cx);
        let subs = vec![cx.observe(&app, |this, _, cx| { this.watch(cx); cx.notify(); })];
        // pi --version is slow, so it never runs before the first frame
        cx.spawn(async move |this, cx| {
            let bin = crate::core::sys::setting_str("piPath");
            let v = cx.background_spawn(async move { crate::core::sys::pi_version(if bin.is_empty() { "pi" } else { &bin }) }).await;
            let _ = this.update(cx, |this, cx| { this.pi_version = v; cx.notify(); });
        }).detach();
        let mut s = StatusLine { app, branch: HashMap::new(), pi_version: String::new(), watched: None, _subs: subs };
        s.watch(cx);
        s
    }

    // redraw on the active session's changes, and fetch its branch once it settles
    fn watch(&mut self, cx: &mut Context<Self>) {
        let Some(t) = self.app.read(cx).active.clone() else { return };
        if self.watched == Some(t.entity_id()) { return; }
        self.watched = Some(t.entity_id());
        self._subs.truncate(1);
        self._subs.push(cx.observe(&t, |_, _, cx| cx.notify()));
        self._subs.push(cx.subscribe(&t, |this, t, ev, cx| if let crate::state::tab::TabEvent::Settled = ev { let cwd = t.read(cx).cwd.clone(); this.load_branch(cwd, cx); }));
    }

    fn load_branch(&mut self, cwd: String, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let c = cwd.clone();
            let b = cx.background_spawn(async move { crate::core::sys::git_status(&c)["branch"].as_str().unwrap_or("").to_string() }).await;
            let _ = this.update(cx, |this, cx| { this.branch.insert(cwd, b); cx.notify(); });
        }).detach();
    }
}

// .st-block: the filled label at the left
fn block(word: &str, spin: bool, cx: &App) -> Div {
    let p = pal(cx);
    let (bg, fg) = match word {
        "work" | "compact" | "start" => (p.primary, p.on_primary),
        "wait" => (p.gold, p.surface_lowest),
        "dead" => (p.red, p.surface_lowest),
        _ => (p.high, p.text),
    };
    div().flex().gap(u(6.)).px(u(9.)).rounded(u(4.)).bg(bg).text_color(fg).line_height(relative(1.7))
        .when(spin, |d| d.child(super::frame::spin("st-spin")))
        .child(word.to_string())
}

// .st-seg: a segment, a hairline between neighbours
fn seg(first: bool, cx: &App) -> Div {
    let p = pal(cx);
    div().px(u(8.)).overflow_hidden().text_ellipsis().whitespace_nowrap().when(!first, |d| d.border_l_1().border_color(p.rule))
}

pub fn mode_color(m: Mode, cx: &App) -> Hsla {
    let p = pal(cx);
    match m { Mode::Plan => p.gold, Mode::Auto => p.red, Mode::Edits => p.cyan, Mode::Ask => p.mid }
}

impl Render for StatusLine {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = pal(cx);
        let a = self.app.read(cx);
        let live = a.tabs.iter().filter(|t| matches!(t.read(cx).status, Status::Idle | Status::Running)).count();
        let busy = a.tabs.iter().filter(|t| t.read(cx).running()).count();
        let row = div().flex().items_center().gap(u(2.)).text_size(em(cx, 0.8)).text_color(p.mid).min_w_0().overflow_hidden().whitespace_nowrap().pb(u(2.)).h_full();
        let t: Option<Entity<Tab>> = if a.view == View::Chat { a.active.clone() } else { None };
        let Some(t) = t else {
            let view = match a.view { View::Home | View::Chat => "home", View::Settings => "settings", View::Usage => "usage", View::Agents => "agents", View::Images => "images" };
            return row
                .child(block(view, false, cx))
                .child(seg(true, cx).text_color(p.dim).child(format!("{live} pi running{}", if busy > 0 { format!(" · {busy} working") } else { String::new() })))
                .child(div().flex_1())
                .child(seg(true, cx).text_color(p.dim).child(format!("pi {}", self.pi_version.trim_start_matches("pi").trim())));
        };
        let tab = t.read(cx);
        if !self.branch.contains_key(&tab.cwd) { self.branch.insert(tab.cwd.clone(), String::new()); let c = tab.cwd.clone(); self.load_branch(c, cx); }
        let tab = t.read(cx);
        // TODO(panes): a watched subagent tab says "agent", or "work" while its own process runs it (statusline.ts)
        let word = match tab.status {
            Status::Running => if tab.compacting { "compact" } else { "work" },
            Status::Starting => "start",
            Status::Dead => "dead",
            Status::Cold => "cold",
            _ => if !tab.asks.is_empty() { "wait" } else { "idle" },
        };
        let st = &tab.stats;
        let cu = &st["contextUsage"];
        let pct = cu["percent"].as_f64().unwrap_or(0.);
        let model_id = tab.state["model"]["id"].as_str().map(String::from);
        let reasoning = tab.state["model"]["reasoning"].as_bool().unwrap_or(false);
        let level = tab.state["thinkingLevel"].as_str().unwrap_or("effort").to_string();
        let br = self.branch.get(&tab.cwd).cloned().unwrap_or_default();
        let mode = tab.mode;
        let has_stats = !st.is_null();
        let tok = |k: &str| st["tokens"][k].as_f64().unwrap_or(0.);
        let (tin, tout, cost) = (tok("input") + tok("cacheRead"), tok("output"), st["cost"].as_f64().unwrap_or(0.));
        let window_ctx = cu["contextWindow"].as_f64().unwrap_or(0.);
        let ctx_known = !cu["tokens"].is_null();
        let statuses: Vec<String> = tab.statuses.iter().map(|(_, v)| v.clone()).collect();
        let cwd = super::tools::tilde(&tab.cwd);
        let full = tab.cwd.clone();
        let ctx_tip = format!("context {} / {}", if ctx_known { super::page::tokens(cu["tokens"].as_f64().unwrap_or(0.)) } else { "?".into() }, super::page::tokens(window_ctx));
        let mode_tip = format!("{} (shift+tab)", mode.hint());
        let running = tab.running();
        let _ = window;
        let t1 = t.clone();
        let app = self.app.clone();
        row
            .child(block(word, running, cx))
            .child(seg(true, cx).id("st-cwd").tip(full, cx).child(cwd))
            .when(!br.is_empty(), |d| d.child(seg(false, cx).text_color(p.dim).child(format!("⎇ {br}"))))
            .children(statuses.into_iter().map(|s| seg(false, cx).text_color(p.tertiary).child(s)))
            .child(div().flex_1())
            .child(seg(true, cx).id("st-mode").cursor_pointer().text_color(mode_color(mode, cx)).hover(|s| s.text_color(pal(cx).text)).tip(mode_tip, cx).child(mode.as_str())
                .on_click(move |_, window, cx| super::picker::modes(t1.clone(), window, cx)))
            .child(seg(false, cx).id("st-model").cursor_pointer().tip("model (ctrl+m)", cx).hover(|s| s.text_color(pal(cx).text)).child(model_id.unwrap_or_else(|| "model".into()))
                .on_click(|_, window, cx| super::picker::models(window, cx)))
            .when(reasoning, |d| d.child(seg(false, cx).id("st-effort").cursor_pointer().tip("effort (ctrl+e)", cx).hover(|s| s.text_color(pal(cx).text)).child(level)
                .on_click({ let t = t.clone(); move |_, window, cx| super::picker::effort(t.clone(), window, cx) })))
            .when(window_ctx > 0., |d| d.child(seg(false, cx).id("st-ctx").tip(ctx_tip, cx).flex().items_center().gap(u(6.))
                .child(div().w(u(54.)).h(u(6.)).rounded(u(3.)).bg(p.high).overflow_hidden()
                    .child(div().h_full().rounded(u(3.)).w(relative((pct.min(100.) / 100.) as f32)).bg(if pct > 80. { p.gold } else { p.mid })))
                .child(if ctx_known { format!("{}%", pct.round()) } else { "—".into() })))
            .when(has_stats, |d| d
                .child(seg(false, cx).text_color(p.dim).child(format!("{}↑ {}↓", super::page::tokens(tin), super::page::tokens(tout))))
                .child(seg(false, cx).id("st-cost").cursor_pointer().text_color(p.text).tip("usage", cx).child(super::page::cost(cost))
                    .on_click(move |_, _, cx| app.update(cx, |a, cx| a.show(View::Usage, cx)))))
            .child(seg(false, cx).text_color(p.dim).child(format!("{live} live")))
    }
}
