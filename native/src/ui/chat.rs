// One session's chat: the transcript, then the dock (what pi is doing, the plan, queued messages, its questions) and the composer.
// one view per tab, so the draft and the scroll position stay with the session
use super::composer::{Composer, ComposerEvent};
use super::theme::pal;
use super::tools::{self, Ui};
use super::transcript::{dock, md, media};
use crate::state::app::Pith;
use crate::state::tab::{Assistant, Block, ItemKind, Level, Status, Tab, TabEvent};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::clipboard::Clipboard;
use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::text::TextView;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Icon, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::collections::HashSet;
use std::rc::Rc;

pub const MAX_W: f32 = 760.;

pub struct ChatView {
    pub tab: Entity<Tab>,
    pub composer: Entity<Composer>,
    pub dock: Entity<dock::Dock>,
    list: ListState,
    seen: (usize, Option<u64>, u64),
    // rows opened or closed by hand, by tool id or a key of the item's own. rows that open themselves stay in neither
    pub open: HashSet<String>,
    closed: HashSet<String>,
    _subs: Vec<Subscription>,
}

impl ChatView {
    pub fn new(tab: Entity<Tab>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let composer = cx.new(|cx| Composer::new(Some(tab.clone()), window, cx));
        let dock = cx.new(|cx| dock::Dock::new(tab.clone(), composer.clone(), cx));
        let n = tab.read(cx).items.len();
        let list = ListState::new(n, ListAlignment::Top, px(1200.));
        list.set_follow_mode(FollowMode::Tail);
        list.scroll_to_end();
        let subs = vec![
            cx.observe(&tab, |this, tab, cx| { this.sync(&tab, cx); cx.notify(); }),
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
                    this.list.scroll_to_end();
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
        ChatView { tab, composer, dock, list, seen: (n, first, v), open: HashSet::new(), closed: HashSet::new(), _subs: subs }
    }

    // keeps the list's measurements in step with the transcript. only the tail is measured again: that's where a stream
    // grows and where results land. a reload (new first item, or fewer items) starts over at the bottom
    fn sync(&mut self, tab: &Entity<Tab>, cx: &App) {
        let t = tab.read(cx);
        let n = t.items.len();
        let first = t.items.first().map(|i| i.id);
        if t.version == self.seen.2 { return; }
        let (old_n, old_first, _) = self.seen;
        if first != old_first || n < old_n {
            self.list.reset(n);
            self.list.scroll_to_end();
        } else {
            if n > old_n { self.list.splice(old_n..old_n, n - old_n); }
            let from = old_n.saturating_sub(2).min(n);
            if from < n { self.list.remeasure_items(from..n); }
        }
        self.seen = (n, first, t.version);
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

    fn render_item(&mut self, ix: usize, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let tab_e = self.tab.clone();
        let tab = tab_e.read(cx);
        let Some(item) = tab.items.get(ix) else { return div().into_any_element() };
        let p = pal(cx);
        let id = item.id;
        let ui = Ui { open: &self.open, closed: &self.closed, toggle: self.toggler(ix, cx), tab: &tab_e, cwd: &tab.cwd };
        // a turn's first message gets more room above it than the steps inside a turn
        let gap_top = match &item.kind { ItemKind::User { .. } if ix > 0 => px(20.), _ => px(6.) };
        let body: AnyElement = match &item.kind {
            ItemKind::User { text, typed, images, entry_id, .. } => user(id, text, typed.as_deref(), images, entry_id.clone(), tab.running(), &tab_e, cx),
            ItemKind::Assistant(a) => {
                let last_of_turn = {
                    let next = tab.items[ix + 1..].iter().find(|i| matches!(i.kind, ItemKind::User { .. } | ItemKind::Assistant(_)));
                    match next { Some(i) => matches!(i.kind, ItemKind::User { .. }), None => !tab.running() }
                };
                assistant(id, a, last_of_turn, &ui, tab, cx)
            }
            ItemKind::Bash { command, output, exit, running, cancelled, excluded, .. } => tools::shell(id, command, output, *exit, *running, *cancelled, *excluded, cx).into_any_element(),
            ItemKind::Compaction { summary, tokens_before } => {
                let label = match tokens_before { Some(n) => format!("Context compacted from {}", tools::tokens(*n)), None => "Context compacted".into() };
                marker(format!("cmp{id}"), label, summary, &ui, cx)
            }
            ItemKind::Branch { summary } => marker(format!("br{id}"), "Came back from another branch".into(), summary, &ui, cx),
            ItemKind::Custom { kind, text } => v_flex().gap_1().child(div().text_xs().text_color(p.dim).child(kind.clone()))
                .child(md::markdown(ElementId::Name(format!("c{id}").into()), text.clone(), cx)).into_any_element(),
            ItemKind::Note { level, text } => {
                let (c, icon) = match level { Level::Error => (p.red, IconName::CircleAlert), Level::Warn => (p.gold, IconName::TriangleAlert), Level::Info => (p.dim, IconName::Info) };
                h_flex().gap_2().items_start().text_sm().text_color(c)
                    .child(div().flex_none().pt(px(3.)).child(Icon::new(icon).xsmall()))
                    .child(div().min_w_0().child(text.clone())).into_any_element()
            }
        };
        h_flex().w_full().justify_center().px_6().child(div().w_full().max_w(px(MAX_W)).pt(gap_top).pb(px(6.)).child(body)).into_any_element()
    }

    fn empty_state(&self, cx: &Context<Self>) -> AnyElement {
        let p = pal(cx);
        let t = self.tab.read(cx);
        if t.file.is_some() && matches!(t.status, Status::Starting | Status::Cold) {
            return h_flex().gap_2().items_center().text_sm().text_color(p.dim).child(Spinner::new().xsmall().color(p.dim)).child("Opening the session").into_any_element();
        }
        let design = t.kind == crate::state::tab::Kind::Design;
        let starters: &[(&str, &str)] = if design {
            &[("A landing page for…", ""), ("A settings screen for a music app, dark", "A settings screen for a music app, dark"), ("An app icon for…", "/image ")]
        } else {
            &[("Review the changes on this branch", "/review"), ("Explain how … works", "/explain "), ("Write an AGENTS.md for this project", "/init"), ("Simplify what changed", "/simplify")]
        };
        let mut col = v_flex().gap_1().w(px(420.)).max_w_full()
            .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).text_color(cx.theme().foreground).child(if design { "New design" } else { "New session" }))
            .child(div().text_sm().text_color(p.dim).mb_4().child(tools::tilde(&t.cwd)));
        for (i, (label, fill)) in starters.iter().enumerate() {
            let (c, fill) = (self.composer.clone(), fill.to_string());
            col = col.child(Button::new(("starter", i)).label(*label).ghost().small().justify_start()
                .on_click(move |_, window, cx| c.update(cx, |c, cx| { c.set_text(&fill, window, cx); c.focus_handle(cx).focus(window, cx); })));
        }
        let key = |k: &str, what: &str| h_flex().gap_1().child(div().px_1().rounded(px(4.)).border_1().border_color(cx.theme().border).text_color(p.mid).child(k.to_string())).child(what.to_string());
        let m = if cfg!(target_os = "macos") { "⌘" } else { "Ctrl+" };
        col.child(h_flex().gap_4().mt_6().text_xs().text_color(p.dim).child(key(&format!("{m}K"), "commands")).child(key("!", "shell")).child(key("Esc", "stop")))
            .into_any_element()
    }
}

// what you sent: a bubble on the right, its images above the text, copy and edit under it on hover and in its menu
#[allow(clippy::too_many_arguments)]
fn user(id: u64, text: &str, typed: Option<&str>, images: &[crate::state::tab::Img], entry_id: Option<String>, running: bool, tab: &Entity<Tab>, cx: &App) -> AnyElement {
    let shown = typed.unwrap_or(text).to_string();
    let group = SharedString::from(format!("u{id}"));
    let edit = {
        let (tab, e) = (tab.clone(), entry_id.clone());
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            let Some(e) = e.clone() else { return };
            if tab.read(cx).running() {
                Pith::global(cx).update(cx, |a, cx| a.toast(Level::Info, "Stop the run first, then edit", cx));
                return;
            }
            tab.update(cx, |t, cx| t.fork(e, cx));
        }
    };
    let edit2 = edit.clone();
    let copy = shown.clone();
    let bubble = v_flex().id(SharedString::from(format!("ub{id}"))).max_w(relative(0.82)).gap_2().px_3p5().py_2().rounded(px(14.)).bg(cx.theme().muted).text_color(cx.theme().foreground)
        .when(!images.is_empty(), |d| d.child(media::strip(images.iter().map(|i| (i.data.as_str(), i.mime.as_str())), px(96.), cx)))
        .when(!shown.trim().is_empty(), |d| d.child(md::markdown(ElementId::Name(format!("u{id}").into()), md::typed(&shown), cx)))
        .context_menu(move |menu, _, _| {
            let (c, e) = (copy.clone(), edit2.clone());
            menu.item(PopupMenuItem::new("Copy").icon(IconName::Copy).on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(c.clone()))))
                .item(PopupMenuItem::new("Edit and resend from here").icon(IconName::Pencil).disabled(running).on_click(move |ev, w, cx| e(ev, w, cx)))
        });
    v_flex().w_full().items_end().gap_1().group(group.clone())
        .child(bubble)
        .child(h_flex().gap_0p5().opacity(0.).group_hover(group, |s| s.opacity(1.))
            .child(Clipboard::new(SharedString::from(format!("ucopy{id}"))).value(shown).tooltip("Copy"))
            .when(entry_id.is_some(), |d| d.child(Button::new(SharedString::from(format!("uedit{id}"))).icon(IconName::Pencil).ghost().xsmall()
                .tooltip("Edit and resend from here").disabled(running).on_click(edit))))
        .into_any_element()
}

fn assistant(id: u64, a: &Assistant, last_of_turn: bool, ui: &Ui, tab: &Tab, cx: &App) -> AnyElement {
    let p = pal(cx);
    let mut col = v_flex().w_full().gap_2();
    let shown: Vec<(usize, &Block)> = a.blocks.iter().enumerate().filter_map(|(i, b)| b.as_ref().map(|b| (i, b))).collect();
    for (k, (bi, b)) in shown.iter().enumerate() {
        let tail = a.live && k + 1 == shown.len();
        match b {
            Block::Text(t) if !t.trim().is_empty() => {
                col = col.child(md::markdown(ElementId::Name(format!("a{id}-{bi}").into()), t.clone(), cx).stream_fade(a.live));
            }
            Block::Thinking { text, ms, redacted, .. } => {
                let key = format!("th{id}-{bi}");
                let open = ui.is_open(&key, false);
                let live = tail && ms.is_none();
                let label = match ms {
                    Some(ms) => format!("Thought for {}", tools::dur(*ms)),
                    None if live => "Thinking".into(),
                    None if *redacted && text.is_empty() => "Thought, hidden by the provider".into(),
                    None => "Thought".into(),
                };
                let can_open = !text.trim().is_empty();
                let last: String = if live && !open { text.trim_end().lines().rev().filter(|l| !l.trim().is_empty()).take(2).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n") } else { String::new() };
                col = col.child(
                    v_flex().gap_1()
                        .child(h_flex().id(SharedString::from(key.clone())).gap_1p5().items_center().text_sm().text_color(p.dim)
                            .when(can_open, |d| d.cursor_pointer().hover(|s| s.text_color(cx.theme().foreground)).on_click(ui.on(key.clone(), false)))
                            .when(live, |d| d.child(Spinner::new().xsmall().color(p.dim)))
                            .when(!live, |d| d.child(Icon::new(IconName::Brain).xsmall()))
                            .child(label)
                            .when(can_open, |d| d.child(Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronRight }).xsmall())))
                        .when(!last.is_empty(), |d| d.child(div().pl(px(22.)).text_sm().text_color(p.dim.opacity(0.8)).line_clamp(2).child(last)))
                        .when(open && can_open, |d| d.child(
                            div().ml(px(6.)).pl(px(15.)).border_l_1().border_color(p.hairline).text_sm().text_color(p.mid)
                                .child(TextView::markdown(ElementId::Name(format!("t{id}-{bi}").into()), text.clone()).selectable(true).text_color(p.mid)),
                        )),
                );
            }
            Block::Tool(tid) => {
                if let Some(tb) = tab.tools.get(tid) { col = col.child(tools::row(tb, ui, cx)); }
            }
            _ => {}
        }
    }
    if let Some(e) = a.error.as_ref().filter(|_| a.stop.as_deref() != Some("aborted")) {
        col = col.child(h_flex().gap_2().items_start().px_3().py_2().rounded(cx.theme().radius).bg(p.red.opacity(0.08)).text_sm().text_color(p.red)
            .child(div().flex_none().pt(px(3.)).child(Icon::new(IconName::CircleAlert).xsmall())).child(div().min_w_0().child(e.clone())));
    }
    // the end of a turn: how long it took, what it cost, and the reply to copy
    if !a.live && (a.run_ms.is_some() || (last_of_turn && a.stop.as_deref() == Some("stop"))) {
        let reply = a.blocks.iter().flatten().filter_map(|b| if let Block::Text(t) = b { Some(t.as_str()) } else { None }).collect::<Vec<_>>().join("\n\n");
        let u = &a.usage;
        let mut meta = vec![];
        if let Some(ms) = a.run_ms { meta.push(format!("Worked for {}", tools::dur(ms))); }
        if let Some(m) = &a.model { meta.push(m.clone()); }
        if let Some(c) = u["cost"]["total"].as_f64().filter(|c| *c > 0.) { meta.push(format!("${c:.3}")); }
        col = col.child(h_flex().gap_1().items_center().text_xs().text_color(p.dim).mt_neg_1()
            .when(!reply.trim().is_empty(), |d| d.child(Clipboard::new(SharedString::from(format!("acopy{id}"))).value(reply).tooltip("Copy reply")))
            .child(meta.join("  ·  ")));
    }
    col.into_any_element()
}

// a rule across the transcript, with what it marks; the summary behind it on click
fn marker(key: String, label: String, summary: &str, ui: &Ui, cx: &App) -> AnyElement {
    let p = pal(cx);
    let open = ui.is_open(&key, false);
    let has = !summary.trim().is_empty();
    v_flex().gap_2()
        .child(h_flex().id(SharedString::from(key.clone())).gap_3().items_center().text_xs().text_color(p.dim)
            .when(has, |d| d.cursor_pointer().hover(|s| s.text_color(cx.theme().foreground)).on_click(ui.on(key.clone(), false)))
            .child(div().h_px().flex_1().bg(p.hairline)).child(label)
            .when(has, |d| d.child(Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronRight }).xsmall()))
            .child(div().h_px().flex_1().bg(p.hairline)))
        .when(open && has, |d| d.child(div().px_4().text_sm().text_color(p.mid)
            .child(TextView::markdown(ElementId::Name(format!("{key}-sum").into()), summary.to_string()).selectable(true).text_color(p.mid))))
        .into_any_element()
}

impl Render for ChatView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = self.tab.read(cx);
        let status = tab.status;
        let empty = tab.items.is_empty();
        v_flex().size_full().min_h_0()
            .child(
                div().flex_1().min_h_0().relative()
                    .when(empty && status != Status::Dead, |d| d.child(div().absolute().inset_0().flex().items_center().justify_center().px_6().child(self.empty_state(cx))))
                    .child(list(self.list.clone(), cx.processor(|this, ix, window, cx| this.render_item(ix, window, cx))).size_full().pt_4().pb_2()),
            )
            // the live line, todos, queue, widgets, questions and the composer (transcript/dock.rs)
            .child(self.dock.clone())
    }
}

