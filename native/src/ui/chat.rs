// One session's chat: the transcript, any question pi is waiting on, and the composer.
// one view per tab, so the draft and the scroll position stay with the session
use super::composer::{Composer, ComposerEvent};
use super::theme::pal;
use super::tools;
use crate::state::app::Pith;
use crate::state::tab::{Ask, Block, How, ItemKind, Level, Status, Tab, TabEvent};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::text::TextView;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Icon, Sizable};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::assets::IconName;
use std::collections::HashSet;

pub const MAX_W: f32 = 760.;

pub struct ChatView {
    pub tab: Entity<Tab>,
    pub composer: Entity<Composer>,
    list: ListState,
    seen: (usize, Option<u64>, u64),
    // which tool rows and thinking blocks are open, by tool id or item id + block index
    pub open: HashSet<String>,
    _subs: Vec<Subscription>,
}

impl ChatView {
    pub fn new(tab: Entity<Tab>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let composer = cx.new(|cx| Composer::new(Some(tab.clone()), window, cx));
        let n = tab.read(cx).items.len();
        let list = ListState::new(n, ListAlignment::Top, px(1200.));
        list.set_follow_mode(FollowMode::Tail);
        list.scroll_to_end();
        let subs = vec![
            cx.observe(&tab, |this, tab, cx| { this.sync(&tab, cx); cx.notify(); }),
            cx.subscribe_in(&tab, window, |this, _, ev: &TabEvent, window, cx| {
                if let TabEvent::Draft(text) = ev { this.composer.update(cx, |c, cx| c.set_text(text, window, cx)); }
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
        ChatView { tab, composer, list, seen: (n, first, v), open: HashSet::new(), _subs: subs }
    }

    // keeps the list's measurements in step with the transcript. a reload (new first item, or fewer items) starts over at the bottom
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
            // the streaming tail grows in place
            let from = old_n.saturating_sub(2).min(n);
            if from < n { self.list.remeasure_items(from..n); }
        }
        self.seen = (n, first, t.version);
    }

    pub fn focus_composer(this: &Entity<Self>, window: &mut Window, cx: &mut App) { let f = this.read(cx).composer.read(cx).focus_handle(cx); f.focus(window, cx); }

    fn toggle(&mut self, key: String, cx: &mut Context<Self>) {
        if !self.open.remove(&key) { self.open.insert(key); }
        self.list.remeasure();
        cx.notify();
    }

    fn render_item(&mut self, ix: usize, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let tab = self.tab.read(cx);
        let Some(item) = tab.items.get(ix) else { return div().into_any_element() };
        let p = pal(cx);
        let id = item.id;
        let body: AnyElement = match &item.kind {
            ItemKind::User { text, typed, images, .. } => {
                let shown = typed.clone().unwrap_or_else(|| text.clone());
                h_flex().w_full().justify_end().child(
                    v_flex().max_w(relative(0.82)).px_3p5().py_2().rounded(px(14.)).bg(cx.theme().muted).text_color(cx.theme().foreground)
                        .when(!images.is_empty(), |d| d.child(div().text_xs().text_color(p.dim).child(format!("{} image{}", images.len(), if images.len() == 1 { "" } else { "s" }))))
                        .child(TextView::markdown(ElementId::Name(format!("u{id}").into()), shown).selectable(true)),
                ).into_any_element()
            }
            ItemKind::Assistant(a) => {
                let mut col = v_flex().w_full().gap_2();
                for (bi, b) in a.blocks.iter().enumerate() {
                    let Some(b) = b else { continue };
                    match b {
                        Block::Text(t) if !t.trim().is_empty() => {
                            col = col.child(TextView::markdown(ElementId::Name(format!("a{id}-{bi}").into()), t.clone()).selectable(true).stream_fade(a.live));
                        }
                        Block::Thinking { text, ms, redacted, .. } => {
                            let key = format!("th{id}-{bi}");
                            let open = self.open.contains(&key);
                            let label = match ms { Some(ms) => format!("Thought for {}", tools::dur(*ms)), None if a.live => "Thinking…".into(), None => "Thought".into() };
                            let k2 = key.clone();
                            col = col.child(
                                v_flex().gap_1()
                                    .child(h_flex().id(SharedString::from(key)).gap_1().text_sm().text_color(p.dim).cursor_pointer().hover(|s| s.text_color(cx.theme().foreground))
                                        .child(label)
                                        .child(Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronRight }).xsmall())
                                        .on_click(cx.listener(move |this, _, _, cx| this.toggle(k2.clone(), cx))))
                                    .when(open && !*redacted, |d| d.child(
                                        div().pl_3().border_l_1().border_color(p.hairline).text_sm().text_color(p.mid)
                                            .child(TextView::markdown(ElementId::Name(format!("t{id}-{bi}").into()), text.clone()).selectable(true)),
                                    )),
                            );
                        }
                        Block::Tool(tid) => {
                            if let Some(tb) = tab.tools.get(tid) {
                                let open = self.open.contains(tid);
                                let k = tid.clone();
                                col = col.child(tools::row(tb, &tab.cwd, open, cx, cx.listener(move |this, _: &ClickEvent, _, cx| this.toggle(k.clone(), cx))));
                            }
                        }
                        _ => {}
                    }
                }
                if let Some(e) = &a.error { col = col.child(div().text_sm().text_color(p.red).child(e.clone())); }
                if let Some(ms) = a.run_ms { col = col.child(div().text_xs().text_color(p.dim).child(format!("Worked for {}", tools::dur(ms)))); }
                col.into_any_element()
            }
            ItemKind::Bash { command, output, exit, running, .. } => tools::shell(command, output, *exit, *running, cx).into_any_element(),
            ItemKind::Compaction { tokens_before, .. } => divider(match tokens_before { Some(n) => format!("Context compacted from {}", tools::tokens(*n)), None => "Context compacted".into() }, cx),
            ItemKind::Branch { .. } => divider("Branch summary".into(), cx),
            ItemKind::Custom { kind, text } => v_flex().gap_1().child(div().text_xs().text_color(p.dim).child(kind.clone()))
                .child(TextView::markdown(ElementId::Name(format!("c{id}").into()), text.clone())).into_any_element(),
            ItemKind::Note { level, text } => div().text_sm().text_color(match level { Level::Error => p.red, Level::Warn => p.gold, Level::Info => p.dim }).child(text.clone()).into_any_element(),
        };
        h_flex().w_full().justify_center().px_6().child(div().w_full().max_w(px(MAX_W)).py_2().child(body)).into_any_element()
    }
}

fn divider(label: String, cx: &App) -> AnyElement {
    let p = pal(cx);
    h_flex().gap_3().items_center().text_xs().text_color(p.dim)
        .child(div().h_px().flex_1().bg(p.hairline)).child(label).child(div().h_px().flex_1().bg(p.hairline)).into_any_element()
}

fn ask_card(ask: &Ask, tab: Entity<Tab>, cx: &App) -> impl IntoElement {
    let p = pal(cx);
    let mut card = v_flex().gap_2().p_3().rounded(cx.theme().radius_lg).border_1().border_color(cx.theme().border).bg(p.raised)
        .child(div().font_weight(FontWeight::SEMIBOLD).child(ask.title.clone()))
        .when_some(ask.message.clone(), |d, m| d.child(div().text_sm().text_color(p.mid).child(m)));
    let id = ask.id.clone();
    match ask.method.as_str() {
        "confirm" => {
            let (t1, t2, i1, i2) = (tab.clone(), tab, id.clone(), id);
            card = card.child(h_flex().gap_2().justify_end()
                .child(Button::new("ask-no").label("No").small().outline().on_click(move |_, _, cx| t1.update(cx, |t, cx| t.answer(&i1, serde_json::json!({ "confirmed": false }), cx))))
                .child(Button::new("ask-yes").label("Yes").small().primary().on_click(move |_, _, cx| t2.update(cx, |t, cx| t.answer(&i2, serde_json::json!({ "confirmed": true }), cx)))));
        }
        "select" => {
            let mut opts = v_flex().gap_1();
            for (i, o) in ask.options.iter().enumerate() {
                let (t, i_, v) = (tab.clone(), id.clone(), o.clone());
                opts = opts.child(Button::new(("ask-opt", i)).label(o.clone()).small().ghost().justify_start()
                    .on_click(move |_, _, cx| t.update(cx, |t, cx| t.answer(&i_, serde_json::json!({ "value": v }), cx))));
            }
            let t = tab.clone();
            card = card.child(opts).child(h_flex().justify_end().child(Button::new("ask-skip").label("Skip").small().ghost()
                .on_click(move |_, _, cx| t.update(cx, |t, cx| t.answer(&id, serde_json::json!({ "cancelled": true }), cx)))));
        }
        _ => {
            // input and editor answers go through the composer: whatever you send next is the answer
            card = card.child(div().text_xs().text_color(p.dim).child("Type your answer below and press Enter."));
        }
    }
    card
}

impl Render for ChatView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = self.tab.read(cx);
        let p = pal(cx);
        let status = tab.status;
        let err = tab.error.clone();
        let asks = tab.asks.clone();
        let empty = tab.items.is_empty();
        let starting = status == Status::Starting || status == Status::Cold;
        let tab_e = self.tab.clone();
        let _ = window;
        v_flex().size_full().min_h_0()
            .child(
                div().flex_1().min_h_0().relative()
                    .when(empty && starting, |d| d.child(div().absolute().inset_0().flex().items_center().justify_center().text_sm().text_color(p.dim).child("Opening…")))
                    .child(list(self.list.clone(), cx.processor(|this, ix, window, cx| this.render_item(ix, window, cx))).size_full().pt_4().pb_2()),
            )
            .child(
                h_flex().w_full().justify_center().px_6().pb_4().pt_1().child(
                    v_flex().w_full().max_w(px(MAX_W)).gap_2()
                        .when(status == Status::Dead, |d| d.child(
                            v_flex().gap_2().p_3().rounded(cx.theme().radius_lg).border_1().border_color(p.red.opacity(0.4))
                                .child(div().text_sm().text_color(p.red).child("pi stopped"))
                                .when(!err.is_empty(), |d| d.child(div().font_family(cx.theme().mono_font_family.clone()).text_xs().text_color(p.mid).child(err.clone())))
                                .child(h_flex().child(Button::new("restart").label("Restart").small().outline().on_click(move |_, _, cx| tab_e.update(cx, |t, cx| { let _ = t.ensure(cx); })))),
                        ))
                        .children(asks.iter().take(1).map(|a| ask_card(a, self.tab.clone(), cx)))
                        .child(self.composer.clone()),
                ),
            )
    }
}

pub fn open_chat(app: &Entity<Pith>, cx: &App) -> Option<Entity<Tab>> { app.read(cx).active.clone() }
