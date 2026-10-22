// One session's chat: the transcript, then the dock (what pi is doing, the plan, queued messages, its questions) and the composer.
// one view per tab, so the draft and the scroll position stay with the session. the look is chat.css
use super::composer::{Composer, ComposerEvent};
use super::theme::{em, geo, pal, u};
use super::tools::{self, Ui};
use super::transcript::text::sel;
use super::transcript::{dock, md, media, steps};
use crate::state::app::Pith;
use crate::state::tab::{Assistant, Block, Img, ItemKind, Kind, Level, Status, Tab, TabEvent};
use gpui_kit::assets::IconName;
use gpui_kit::component::{v_flex, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::collections::HashSet;
use std::rc::Rc;
use std::time::Duration;

// .tx-wrap and .dock: 900 wide at most, 28 either side, inside that
pub const MAX_W: f32 = 900.;
pub const SIDE: f32 = 28.;

pub struct ChatView {
    pub tab: Entity<Tab>,
    pub composer: Entity<Composer>,
    pub dock: Entity<dock::Dock>,
    list: ListState,
    seen: (usize, Option<u64>, u64),
    // rows opened or closed by hand, by tool id or a key of the item's own. rows that open themselves stay in neither
    pub open: HashSet<String>,
    closed: HashSet<String>,
    // the working line opens into the run's steps
    steps_open: bool,
    // running tool rows and the working line tick while a run is on
    tick: Option<Task<()>>,
    _subs: Vec<Subscription>,
}

impl ChatView {
    pub fn new(tab: Entity<Tab>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let composer = cx.new(|cx| Composer::new(Some(tab.clone()), window, cx));
        let dock = cx.new(|cx| dock::Dock::new(tab.clone(), composer.clone(), cx));
        let n = tab.read(cx).items.len();
        // the items, then the live line as the last row (it scrolls with the transcript, like .live-line in .tx-wrap)
        let list = ListState::new(n + 1, ListAlignment::Top, px(1200.));
        list.set_follow_mode(FollowMode::Tail);
        list.scroll_to_end();
        let weak = cx.weak_entity();
        list.set_scroll_handler(move |_, _, cx| { let _ = weak.update(cx, |_, cx| cx.notify()); });
        let subs = vec![
            cx.observe(&tab, |this, tab, cx| { this.sync(&tab, cx); this.ticking(cx); cx.notify(); }),
            cx.subscribe_in(&tab, window, |this, _, ev: &TabEvent, window, cx| {
                if let TabEvent::Draft(text) = ev {
                    this.composer.update(cx, |c, cx| c.set_text(text, window, cx));
                    // the view is mid-update here, so focus through the composer, not focus_composer
                    let f = this.composer.read(cx).focus_handle(cx);
                    f.focus(window, cx);
                }
            }),
            cx.subscribe_in(&composer, window, |this, _, ev: &ComposerEvent, _, cx| match ev {
                ComposerEvent::Submit { text, images, how } => {
                    let (text, images, how) = (text.clone(), images.clone(), *how);
                    this.list.set_follow_mode(FollowMode::Tail);
                    this.tab.update(cx, |t, cx| {
                        if let Some(cmd) = text.strip_prefix('!') {
                            let (exclude, cmd) = match cmd.strip_prefix('!') { Some(c) => (true, c), None => (false, cmd) };
                            t.bash(cmd.trim().to_string(), exclude, cx);
                        } else { t.send(text, images, how, cx); }
                    });
                }
                ComposerEvent::Stop => this.tab.update(cx, |t, cx| t.abort(cx)),
            }),
        ];
        let first = tab.read(cx).items.first().map(|i| i.id);
        let v = tab.read(cx).version;
        let mut this = ChatView { tab, composer, dock, list, seen: (n, first, v), open: HashSet::new(), closed: HashSet::new(), steps_open: false, tick: None, _subs: subs };
        this.ticking(cx);
        this
    }

    // keeps the list's measurements in step with the transcript. only the tail is measured again: that's where a stream
    // grows and where results land. a reload (new first item, or fewer items) starts over at the bottom
    fn sync(&mut self, tab: &Entity<Tab>, cx: &App) {
        let t = tab.read(cx);
        let n = t.items.len();
        let first = t.items.first().map(|i| i.id);
        // the live line changes with the status, not the version
        if t.version == self.seen.2 { self.list.remeasure_items(n..n + 1); return; }
        let (old_n, old_first, _) = self.seen;
        if first != old_first || n < old_n {
            self.list.reset(n + 1);
            self.list.scroll_to_end();
        } else {
            if n > old_n { self.list.splice(old_n..old_n, n - old_n); }
            let from = old_n.saturating_sub(2).min(n);
            self.list.remeasure_items(from..n + 1);
        }
        self.seen = (n, first, t.version);
    }

    // a clock for the running rows' elapsed time and the steps view, only while something runs
    fn ticking(&mut self, cx: &mut Context<Self>) {
        let busy = self.tab.read(cx).running();
        if !busy { self.tick = None; return; }
        if self.tick.is_some() { return; }
        self.tick = Some(cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(480)).await;
            if this.update(cx, |v, cx| { let n = v.tab.read(cx).items.len(); if v.steps_open { v.list.remeasure_items(n..n + 1); } cx.notify(); }).is_err() { break; }
        }));
    }

    pub fn focus_composer(this: &Entity<Self>, window: &mut Window, cx: &mut App) { let f = this.read(cx).composer.read(cx).focus_handle(cx); f.focus(window, cx); }

    // flips one row and measures only the item it lives in
    fn toggle(&mut self, key: String, was_open: bool, ix: usize, cx: &mut Context<Self>) {
        if was_open { self.open.remove(&key); self.closed.insert(key); } else { self.closed.remove(&key); self.open.insert(key); }
        if ix < self.list.item_count() { self.list.remeasure_items(ix..ix + 1); }
        cx.notify();
    }

    fn toggler(&self, ix: usize, cx: &Context<Self>) -> Rc<dyn Fn(String, bool, &mut App)> {
        let weak = cx.weak_entity();
        Rc::new(move |key, was, cx| { let _ = weak.update(cx, |v, cx| v.toggle(key, was, ix, cx)); })
    }

    fn render_item(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let tab_e = self.tab.clone();
        let tab = tab_e.read(cx);
        if ix == tab.items.len() { return self.live_line(window, cx); }
        let Some(item) = tab.items.get(ix) else { return div().into_any_element() };
        let p = pal(cx);
        let id = item.id;
        let ui = Ui { open: &self.open, closed: &self.closed, toggle: self.toggler(ix, cx), tab: &tab_e };
        let last_of_turn = |ix: usize| {
            let next = tab.items[ix + 1..].iter().find(|i| matches!(i.kind, ItemKind::User { .. } | ItemKind::Assistant(_)));
            match next { Some(i) => matches!(i.kind, ItemKind::User { .. }), None => !tab.running() }
        };
        // css margins between items, collapsed: the gap above is the bigger of the last one's bottom and this one's top
        let gap = match ix.checked_sub(1).map(|j| margins(tab, j, last_of_turn(j), &ui)) {
            None => if matches!(item.kind, ItemKind::User { .. }) { 4. } else { margins(tab, ix, false, &ui).0 },
            Some((_, b)) => b.max(margins(tab, ix, false, &ui).0),
        };
        let body: AnyElement = match &item.kind {
            ItemKind::User { text, typed, images, entry_id, .. } => self.user(id, text, typed.as_deref(), images, entry_id.clone(), cx),
            ItemKind::Assistant(a) => assistant(id, ix, a, last_of_turn(ix), &ui, tab, cx),
            ItemKind::Bash { command, output, exit, running, cancelled, excluded, .. } => tools::shell(id, command, output, *exit, *running, *cancelled, *excluded, cx).into_any_element(),
            ItemKind::Compaction { summary, tokens_before } => {
                let label = format!("compacted{}", tokens_before.map(|n| format!(" · {} tokens summarised", tools::tok(n))).unwrap_or_default());
                marker(format!("cmp{id}"), label, summary, &ui, cx)
            }
            ItemKind::Branch { summary } => marker(format!("br{id}"), "came back from another branch".into(), summary, &ui, cx),
            // .msg.custom: a tertiary rule on the left, its type, then the text
            ItemKind::Custom { kind, text } => div().px(u(12.)).py(u(6.)).border_l(u(2.)).border_color(p.tertiary)
                .child(div().text_color(p.dim).child(kind.clone()))
                .child(md::prose(format!("c{id}"), text, false, false, Some(&tab_e), cx)).into_any_element(),
            // .note: a short line with a rule, coloured by level
            ItemKind::Note { level, text } => {
                let (c, rule) = match level { Level::Error => (p.red, p.red), Level::Warn => (p.gold, p.gold), Level::Info => (p.mid, p.dim) };
                div().px(u(10.)).py(u(2.)).text_size(em(cx, 0.88)).border_l(u(2.)).border_color(rule).text_color(c)
                    .child(sel(SharedString::from(format!("n{id}")), text.clone())).into_any_element()
            }
        };
        wrap(div().pt(u(gap)).child(body), cx)
    }

    // .live-line: what pi is doing, under the last message. a click on a running one opens its steps
    fn live_line(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = pal(cx);
        let t = self.tab.read(cx);
        let (running, dead, err) = (t.running(), t.status == Status::Dead, t.error.clone());
        let last_bottom = t.items.len().checked_sub(1).map(|j| {
            let ui = Ui { open: &self.open, closed: &self.closed, toggle: self.toggler(j, cx), tab: &self.tab };
            margins(t, j, !t.running(), &ui).1
        }).unwrap_or(0.);
        let steps = (running && self.steps_open).then(|| steps::run_steps(t, cx));
        let weak = cx.weak_entity();
        let tab_e = self.tab.clone();
        let line = div().id("live-line").w_full().min_h(em(cx, 0.9 * 1.6)).text_size(em(cx, 0.9)).text_color(p.mid)
            .child(div().id("live-head").when(running, |d| d.cursor_pointer().on_click(move |_, _, cx| {
                let _ = weak.update(cx, |v, cx| { v.steps_open = !v.steps_open; let n = v.tab.read(cx).items.len(); v.list.remeasure_items(n..n + 1); cx.notify(); });
            })).child(self.dock.read(cx).working.clone()))
            .children(steps)
            // pi stopped: what it said on the way out, and a way back
            .when(dead, |d| d.child(div().flex().flex_wrap().items_baseline().gap(u(8.))
                .child(div().text_color(p.red).child("pi stopped"))
                .when(!err.is_empty(), |d| d.child(div().id("dead-err").w_full().my(u(6.)).px(u(10.)).py(u(8.)).bg(p.del_bg).text_color(p.mid).rounded(u(5.)).max_h(u(200.)).overflow_y_scroll()
                    .child(sel("dead-err-t", err.chars().rev().take(1500).collect::<Vec<_>>().into_iter().rev().collect::<String>()))))
                .child(div().id("restart").ml(u(10.)).text_size(em(cx, 0.8)).px(u(7.)).rounded(u(4.)).border(u(1.)).border_color(p.line).text_color(p.mid).cursor_pointer()
                    .hover(|s| s.bg(pal_hover())).child("start again").on_click(move |_, _, cx| tab_e.update(cx, |t, cx| { let _ = t.ensure(cx); })))));
        wrap(div().pt(u(last_bottom.max(8.))).pb(u(18. + 6.)).child(line), cx)
    }

    // .msg.user: raised, a primary bar on the left, the ❯ prompt, copy and branch on hover
    fn user(&self, id: u64, text: &str, typed: Option<&str>, images: &[Img], entry_id: Option<String>, cx: &Context<Self>) -> AnyElement {
        let p = pal(cx);
        let g = geo(cx);
        let group = SharedString::from(format!("u{id}"));
        let tab = self.tab.clone();
        let exp_key = format!("uexp{id}");
        let exp_open = self.open.contains(&exp_key);
        let ix = self.tab.read(cx).items.iter().position(|i| i.id == id).unwrap_or(0);
        let weak = cx.weak_entity();
        let toggle = move |_: &ClickEvent, _: &mut Window, cx: &mut App| { let k = exp_key.clone(); let _ = weak.update(cx, |v, cx| v.toggle(k, exp_open, ix, cx)); };
        let body = div().flex_1().min_w_0()
            .when(!images.is_empty(), |d| d.child(media::strip(&format!("ui{id}"), images.iter().map(|i| (i.data.as_str(), i.mime.as_str())), 84., 1000., 5., true, 6., cx).mb(u(6.))))
            .child(match typed {
                Some(t) => div().text_color(p.accent).child(sel(SharedString::from(format!("ut{id}")), t.to_string())),
                None => div().child(sel(SharedString::from(format!("ut{id}")), text.to_string())),
            })
            .when(typed.is_some(), |d| {
                let mid = p.mid;
                d.child(div().id(SharedString::from(format!("ux{id}"))).mt(u(2.)).text_size(em(cx, 0.8)).text_color(p.dim).cursor_pointer().hover(move |s| s.text_color(mid))
                    .child(format!("{} expanded prompt", if exp_open { "▼" } else { "▶" })).on_click(toggle))
                    .when(exp_open, |d| d.child(div().mt(u(4.)).text_size(em(cx, 0.9)).text_color(p.dim).child(sel(SharedString::from(format!("ue{id}")), text.to_string()))))
            });
        let copy_text = text.to_string();
        let edit = {
            let (tab, e) = (tab.clone(), entry_id.clone());
            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                let Some(e) = e.clone() else { return };
                if tab.read(cx).running() { Pith::global(cx).update(cx, |a, cx| a.toast(Level::Info, "stop the run first, then branch", cx)); return; }
                tab.update(cx, |t, cx| t.fork(e, cx));
                Pith::global(cx).update(cx, |a, cx| { a.toast(Level::Info, "branched into a new session · edit and send", cx); a.refresh_sessions(cx); });
            }
        };
        div().relative().group(group.clone()).flex().gap(u(4.)).pl(u(8.)).pr(u(12.)).py(u(8.)).rounded(u(g.r2)).bg(p.raised).border_l(u(2.)).border_color(p.primary)
            .child(div().flex_none().w(u(18.)).text_color(p.primary).font_weight(FontWeight::BOLD).child("❯"))
            .child(body)
            .child(div().absolute().top(u(4.)).right(u(6.)).flex().gap(u(2.)).bg(p.raised).rounded(u(5.)).opacity(0.).group_hover(group, |s| s.opacity(1.))
                .child(ia(format!("ucp{id}"), IconName::Copy, 24., 1., "copy", cx).on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(copy_text.clone()))))
                .when(entry_id.is_some(), |d| d.child(ia(format!("ubr{id}"), IconName::GitBranch, 24., 1., "edit and branch from here", cx).on_click(edit))))
            .into_any_element()
    }

    // .chat-empty: what kind of session, where, a few ways to start, and the keys
    fn empty_state(&self, window: &Window, cx: &Context<Self>) -> Option<AnyElement> {
        let p = pal(cx);
        let g = geo(cx);
        let t = self.tab.read(cx);
        // a session off disk shows its history in a moment, not "new session"
        if t.file.is_some() && matches!(t.status, Status::Starting | Status::Cold) { return None; }
        let design = t.kind == Kind::Design;
        let starters: &[(&str, &str)] = if design {
            &[("/design ", "a landing page for…"), ("", "a settings screen for a music app, dark"), ("/image ", "an app icon for…")]
        } else { &[("/review", ""), ("/explain ", "how … works"), ("/init", ""), ("/simplify", "")] };
        let mut row = div().flex().flex_wrap().gap(u(8.)).mt(u(10.));
        for (i, (cmd, rest)) in starters.iter().enumerate() {
            let fill = format!("{cmd}{}", if rest.contains('…') { "" } else { rest });
            let c = self.composer.clone();
            let (hov, dim) = (p.hover, p.dim);
            row = row.child(div().id(("starter", i)).flex().px(u(12.)).py(u(4.)).border(u(1.)).border_color(p.line).rounded(u(g.r2)).cursor_pointer()
                .hover(move |s| s.bg(hov).border_color(dim))
                .child(div().text_color(p.accent).child(if cmd.is_empty() { "❯ ".to_string() } else { cmd.to_string() }))
                .child(div().text_color(p.dim).child(rest.to_string()))
                .on_click(move |_, window, cx| c.update(cx, |c, cx| { c.set_text(&fill, window, cx); c.focus_handle(cx).focus(window, cx); })));
        }
        let kbd = |k: &str, what: &str| div().flex().items_center().gap(u(4.))
            .child(div().text_size(em(cx, 0.85 * 0.78)).line_height(relative(1.)).px(u(5.)).pt(u(2.)).pb(u(3.)).rounded(u(4.)).text_color(p.mid).bg(p.high).border_b(u(1.)).border_color(p.line).child(k.to_string()))
            .child(what.to_string());
        let top = f32::from(window.viewport_size().height) * 0.14 / g.scale.max(0.1);
        Some(div().mt(u(top)).flex().flex_col().gap(u(10.))
            .child(div().text_color(p.dim).text_size(em(cx, 0.85)).child(if design { "design session" } else { "new session" }))
            .child(div().text_size(em(cx, 1.3)).text_color(p.text).child(tools::tilde(&t.cwd)))
            .child(row)
            .child(div().flex().flex_wrap().gap(u(18.)).mt(u(18.)).text_color(p.dim).text_size(em(cx, 0.85))
                .child(kbd("shift+tab", "mode")).child(kbd("ctrl+k", "commands")).child(kbd("@", "files")).child(kbd("!", "shell")))
            .into_any_element())
    }
}

fn pal_hover() -> Hsla { gpui_kit::hsla(0., 0., 1., 0.06) }

// .tx-wrap: centred, at most 900 wide with 28 either side
fn wrap(inner: Div, _cx: &App) -> AnyElement {
    div().w_full().flex().justify_center().child(div().w_full().max_w(u(MAX_W)).px(u(SIDE)).child(inner)).into_any_element()
}

// .ia: a small icon button, dim until hovered
fn ia(id: String, icon: IconName, size: f32, font: f32, _title: &str, cx: &App) -> Stateful<Div> {
    let p = pal(cx);
    let (txt, hov) = (p.text, p.hover);
    div().id(SharedString::from(id)).flex_none().size(u(size)).flex().items_center().justify_center().rounded(u(5.)).text_color(p.dim).cursor_pointer()
        .hover(move |s| s.text_color(txt).bg(hov)).child(Icon::new(icon).size(em(cx, font * 1.2)))
}

// an item's css margins (top, bottom) in px, with what its last child pushes through
fn margins(tab: &Tab, ix: usize, last_of_turn: bool, ui: &Ui) -> (f32, f32) {
    match &tab.items[ix].kind {
        ItemKind::User { .. } => (26., 14.),
        ItemKind::Assistant(a) => {
            let mut b: f32 = 4.;
            if a.error.is_some() && a.stop.as_deref() != Some("aborted") { b = b.max(8.); }
            else if let Some(Some(Block::Tool(id))) = a.blocks.iter().rev().find(|b| b.is_some()) {
                if ui.open.contains(id) { b = b.max(10.); }
            }
            if footer(a, last_of_turn) { b = b.max(22.); }
            (4., b)
        }
        ItemKind::Bash { .. } => (14., 14.),
        ItemKind::Compaction { .. } | ItemKind::Branch { .. } => (18., 18.),
        ItemKind::Custom { .. } => (10., 10.),
        ItemKind::Note { .. } => (6., 6.),
    }
}

// live runs get their footer, ones off disk only what the message itself recorded
fn footer(a: &Assistant, last_of_turn: bool) -> bool { a.run_ms.is_some() || (!a.live && a.stop.as_deref() == Some("stop") && last_of_turn) }

#[derive(PartialEq, Clone, Copy)]
enum BK { Prose, Think, Tool }

fn assistant(id: u64, ix: usize, a: &Assistant, last_of_turn: bool, ui: &Ui, tab: &Tab, cx: &App) -> AnyElement {
    let p = pal(cx);
    let shown: Vec<(usize, &Block)> = a.blocks.iter().enumerate().filter_map(|(i, b)| b.as_ref().map(|b| (i, b)))
        .filter(|(_, b)| !matches!(b, Block::Text(t) if t.trim().is_empty() && !a.live)).collect();
    let mut col = div().flex().flex_col().w_full();
    let mut prev: Option<BK> = None;
    let mut prev_bottom = 0.;
    for (k, (bi, b)) in shown.iter().enumerate() {
        let last = a.live && k + 1 == shown.len();
        let kind = match b { Block::Text(_) => BK::Prose, Block::Thinking { .. } => BK::Think, Block::Tool(_) => BK::Tool };
        // .blocks spacing: 2px, more around prose
        // the last block's bottom margin (an open body) collapses into this gap
        let gap = match (prev, kind) { (None, _) => 0., (Some(BK::Prose), BK::Prose) => 8., (Some(BK::Tool | BK::Think), BK::Prose) => 10., (Some(BK::Prose), BK::Tool) => 8., _ => 2f32 }.max(if prev.is_some() { prev_bottom } else { 0. });
        prev = Some(kind);
        let (el, bottom): (AnyElement, f32) = match b {
            Block::Text(t) => (md::prose(format!("a{id}-{bi}"), t, false, last, Some(ui.tab), cx).into_any_element(), 0.),
            Block::Thinking { text, ms, redacted, .. } => thinking(format!("th{id}-{bi}"), text, *ms, *redacted, last && ms.is_none(), ui, cx),
            Block::Tool(tid) => match tab.tools.get(tid) { Some(tb) => tools::row(tb, ui, cx), None => continue },
        };
        prev_bottom = bottom;
        col = col.child(div().w_full().when(gap > 0., |d| d.mt(u(gap))).child(el));
    }
    if let Some(e) = a.error.as_ref().filter(|_| a.stop.as_deref() != Some("aborted")) {
        // .aerr
        col = col.child(div().mt(u(8.)).px(u(10.)).py(u(6.)).border_l(u(2.)).border_color(p.red).text_color(p.red).bg(p.del_bg).rounded_r(u(4.)).text_size(em(cx, 0.9))
            .child(sel(SharedString::from(format!("ae{id}")), e.clone())));
    }
    if footer(a, last_of_turn) {
        let reply = a.blocks.iter().flatten().filter_map(|b| if let Block::Text(t) = b { Some(t.as_str()) } else { None }).collect::<Vec<_>>().join("\n\n");
        let u_ = &a.usage;
        let mut meta = vec![];
        if let Some(m) = &a.model { meta.push(m.clone()); }
        if let Some(ms) = a.run_ms { meta.push(tools::dur(ms)); }
        if u_.is_object() { meta.push(format!("{}↑ {}↓", tools::tok(u_["input"].as_u64().unwrap_or(0) + u_["cacheRead"].as_u64().unwrap_or(0)), tools::tok(u_["output"].as_u64().unwrap_or(0)))); }
        if let Some(c) = u_["cost"]["total"].as_f64().filter(|c| *c > 0.) { meta.push(tools::cost(c)); }
        let group = SharedString::from(format!("tf{id}"));
        let rule = || div().flex_1().min_w(u(20.)).child(tools::dashed(p.line));
        // retry: branch from the prompt that started this turn and send it again
        let tab_e = ui.tab.clone();
        let retry = move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            let t = tab_e.read(cx);
            if t.running() { return; }
            let Some((entry, text, images)) = t.items[..=ix.min(t.items.len().saturating_sub(1))].iter().rev().find_map(|i| match &i.kind {
                ItemKind::User { text, typed, images, entry_id, .. } => Some((entry_id.clone(), typed.clone().unwrap_or(text.clone()), images.clone())),
                _ => None,
            }) else { return };
            let Some(entry) = entry else { return };
            tab_e.update(cx, |t, cx| t.retry(entry, text, images, cx));
        };
        col = col.child(div().group(group.clone()).mt(u(14.)).flex().items_center().gap(u(10.)).text_size(em(cx, 0.78)).text_color(p.dim)
            .child(rule())
            .child(div().whitespace_nowrap().child(meta.join(" · ")))
            .when(!reply.trim().is_empty(), |d| d.child(ia(format!("tfc{id}"), IconName::Copy, 20., 0.78, "copy reply", cx).opacity(0.).group_hover(group.clone(), |s| s.opacity(1.))
                .on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(reply.clone())))))
            .child(ia(format!("tfr{id}"), IconName::RefreshCw, 20., 0.78, "retry: branch from the prompt before this and send it again", cx).opacity(0.).group_hover(group.clone(), |s| s.opacity(1.)).on_click(retry))
            .child(rule()));
    }
    col.into_any_element()
}

// .think: ∴ and how long it thought; the whole of it on click, the last few lines while it streams
fn thinking(key: String, text: &str, ms: Option<u64>, redacted: bool, live: bool, ui: &Ui, cx: &App) -> (AnyElement, f32) {
    let p = pal(cx);
    let open = ui.is_open(&key, false);
    let label = if live { "thinking".to_string() } else if redacted && text.is_empty() { "thought (hidden by the provider)".into() }
        else if let Some(ms) = ms { format!("thought for {}", tools::dur(ms)) } else { format!("thought · {} tokens", tools::tok((text.chars().count() as f64 / 4.).round() as u64)) };
    let tail: String = if live && !open { text.lines().filter(|l| !l.trim().is_empty()).collect::<Vec<_>>().iter().rev().take(3).rev().cloned().collect::<Vec<_>>().join("\n") } else { String::new() };
    let (mid, hov) = (p.mid, p.hover);
    let rule = |d: Div| d.ml(u(22.)).pl(u(12.)).border_l(u(1.)).border_color(p.rule).italic();
    // the tail's and the body's bottom margins push through to whatever comes next
    let bottom = if open { 8. } else if !tail.is_empty() { 4. } else { 0. };
    (div().w_full().text_size(em(cx, 0.9)).text_color(p.dim)
        .child(div().flex().child(div().id(SharedString::from(key.clone())).flex().items_baseline().gap(u(8.)).pl(u(2.)).pr(u(8.)).py(u(1.)).rounded(u(4.)).italic().cursor_pointer()
            .hover(move |s| s.text_color(mid).bg(hov)).on_click(ui.on(key.clone(), false))
            .child(div().flex_none().w(u(12.)).flex().justify_center().not_italic().text_color(if live { p.accent } else { p.tertiary })
                .child(if live { super::frame::spin(SharedString::from(format!("{key}-sp"))).into_any_element() } else { div().child("∴").into_any_element() }))
            .child(label)))
        .when(!tail.is_empty(), |d| d.child(rule(div()).mt(u(2.)).max_h(em(cx, 0.9 * 4.8)).overflow_hidden().text_color(p.dim.opacity(0.8)).child(tail)))
        .when(open, |d| d.child(rule(div()).mt(u(4.)).max_h(u(420.)).id(SharedString::from(format!("{key}-b"))).overflow_y_scroll()
            .child(md::prose(format!("{key}-md"), text, true, false, None, cx))))
        .into_any_element(), bottom)
}

// .marker: a dashed rule across with what it marks; the summary behind it on click
fn marker(key: String, label: String, summary: &str, ui: &Ui, cx: &App) -> AnyElement {
    let p = pal(cx);
    let open = ui.is_open(&key, false);
    let rule = || div().flex_1().min_w(u(20.)).child(tools::dashed(p.line));
    div().w_full().text_size(em(cx, 0.85)).text_color(p.dim)
        .child(div().id(SharedString::from(key.clone())).flex().items_center().gap(u(12.)).cursor_pointer().on_click(ui.on(key.clone(), false))
            .child(rule()).child(label).child(rule()))
        .when(open && !summary.trim().is_empty(), |d| d.child(div().mt(u(10.)).text_size(em(cx, 0.85 * 1.05)).child(md::prose(format!("{key}-sum"), summary, true, false, None, cx))))
        .into_any_element()
}

impl Render for ChatView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = self.tab.read(cx);
        let p = pal(cx);
        let empty = tab.items.is_empty();
        let running = tab.running();
        let empty_el = if empty { self.empty_state(window, cx) } else { None };
        // .jump: scrolled up while it runs, a way back down to the new output
        let jump = (running && !self.list.is_following_tail()).then(|| {
            let list = self.list.clone();
            div().absolute().bottom(u(10.)).left_0().right_0().flex().justify_center().child(
                div().id("jump").flex().items_center().gap(u(6.)).px(u(12.)).py(u(3.)).rounded(u(20.)).bg(p.high).border(u(1.)).border_color(p.line)
                    .text_size(em(cx, 0.84)).text_color(p.text).cursor_pointer().shadow_md()
                    .child(Icon::new(IconName::ArrowDown).size(em(cx, 0.84 * 1.2))).child("new output")
                    .on_click(cx.listener(move |_, _, _, cx| { list.set_follow_mode(FollowMode::Tail); cx.notify(); })))
        });
        let bg = p.bg;
        v_flex().size_full().min_h_0()
            .child(
                div().flex_1().min_h_0().relative()
                    .child(list(self.list.clone(), cx.processor(|this, ix, window, cx| this.render_item(ix, window, cx))).size_full().pt(u(22.)))
                    .when_some(empty_el, |d, e| d.child(div().absolute().top(u(22.)).left_0().right_0().child(wrap(div().child(e), cx))))
                    // .chat-scroll's mask: the top 22px fade out
                    .child(div().absolute().top_0().left_0().right_0().h(u(22.)).bg(linear_gradient(180., linear_color_stop(bg, 0.), linear_color_stop(bg.opacity(0.), 1.))))
                    .child(div().absolute().inset_0().child(gpui_kit::base::Scrollbar::vertical(&self.list).mode(gpui_kit::base::ScrollbarMode::Always)))
                    .children(jump),
            )
            // the live line, todos, queue, widgets, questions and the composer (transcript/dock.rs)
            .child(self.dock.clone())
    }
}
