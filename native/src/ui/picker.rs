// one picker for everything: the command palette, models, sessions, mode, thinking. type to filter, arrows and Enter to pick
use super::theme::pal;
use crate::state::app::Pith;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Icon, Sizable, WindowExt};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::assets::IconName;
use std::rc::Rc;

pub struct PickItem {
    pub label: SharedString,
    pub hint: Option<SharedString>,
    pub group: Option<SharedString>,
    pub right: Option<SharedString>,
    pub on: bool,
    pub icon: Option<IconName>,
    pub keys: Option<Keystroke>,
    pub tags: Vec<SharedString>,
    pub run: Rc<dyn Fn(&mut Window, &mut App)>,
}

impl PickItem {
    pub fn new(label: impl Into<SharedString>, run: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        PickItem { label: label.into(), hint: None, group: None, right: None, on: false, icon: None, keys: None, tags: vec![], run: Rc::new(run) }
    }
    pub fn hint(mut self, h: impl Into<SharedString>) -> Self { let h = h.into(); self.hint = if h.is_empty() { None } else { Some(h) }; self }
    pub fn group(mut self, g: impl Into<SharedString>) -> Self { self.group = Some(g.into()); self }
    pub fn right(mut self, r: impl Into<SharedString>) -> Self { self.right = Some(r.into()); self }
    pub fn on(mut self, on: bool) -> Self { self.on = on; self }
    pub fn icon(mut self, i: IconName) -> Self { self.icon = Some(i); self }
    // a shortcut shown on the right, "secondary-n" is cmd on macos and ctrl elsewhere
    pub fn keys(mut self, k: &str) -> Self { self.keys = Keystroke::parse(k).ok(); self }
    pub fn tag(mut self, t: impl Into<SharedString>) -> Self { self.tags.push(t.into()); self }
}

// what the list draws: section labels while browsing, items
#[derive(Clone, Copy)]
enum Row { Head(usize), Item(usize) }

const ROW_H: f32 = 34.;
const MAX_ROWS: usize = 10;

pub struct Picker {
    items: Vec<PickItem>,
    // indexes into items, in the order shown
    shown: Vec<usize>,
    rows: Vec<Row>,
    heads: Vec<Option<SharedString>>,
    // index into shown
    sel: usize,
    input: Entity<InputState>,
    scroll: UniformListScrollHandle,
    // the pointer only takes over the selection when it actually moves, not when the list scrolls under it
    mouse: Option<Point<Pixels>>,
    _sub: Subscription,
}

// subsequence match, consecutive letters and word starts score higher
pub fn fuzzy(q: &str, s: &str) -> f32 {
    let (q, s) = (q.to_lowercase(), s.to_lowercase());
    if q.is_empty() { return 1.; }
    if let Some(i) = s.find(&q) { return 10. - (i as f32).min(5.) * 0.2 + if i == 0 { 2. } else { 0. }; }
    let sc: Vec<char> = s.chars().collect();
    let (mut score, mut at, mut run) = (0., 0usize, 0.);
    for c in q.chars() {
        let Some(off) = sc[at..].iter().position(|x| *x == c) else { return 0. };
        let i = at + off;
        run = if off == 0 && at > 0 { run + 1. } else { 0. };
        score += 1. + run + if i == 0 || !sc[i - 1].is_alphanumeric() { 1. } else { 0. };
        at = i + 1;
    }
    score / (q.chars().count() as f32 + 2.)
}

impl Picker {
    pub fn new(items: Vec<PickItem>, placeholder: &str, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder.to_string()));
        let sub = cx.subscribe_in(&input, window, |this: &mut Picker, _, ev: &InputEvent, window, cx| match ev {
            InputEvent::Change => { this.filter(cx); cx.notify(); }
            InputEvent::PressEnter { .. } => this.pick(window, cx),
            _ => {}
        });
        let mut p = Picker { shown: vec![], rows: vec![], heads: vec![], items, sel: 0, input, scroll: UniformListScrollHandle::new(), mouse: None, _sub: sub };
        p.filter(cx);
        p.sel = p.shown.iter().position(|&i| p.items[i].on).unwrap_or(0);
        let r = p.row_of(p.sel);
        p.scroll.scroll_to_item(r, ScrollStrategy::Center);
        p
    }

    fn filter(&mut self, cx: &App) {
        let q = self.input.read(cx).value().trim().to_string();
        self.rows.clear();
        if q.is_empty() {
            // browsing: grouped, groups in the order they first appear, items keep theirs
            let mut groups: Vec<Option<SharedString>> = vec![];
            for it in &self.items { if !groups.contains(&it.group) { groups.push(it.group.clone()); } }
            self.shown.clear();
            for (gi, g) in groups.iter().enumerate() {
                if g.is_some() { self.rows.push(Row::Head(gi)); }
                for (i, it) in self.items.iter().enumerate() {
                    if it.group != *g { continue; }
                    self.rows.push(Row::Item(self.shown.len()));
                    self.shown.push(i);
                }
            }
            self.heads = groups;
        } else {
            let mut v: Vec<(usize, f32)> = self.items.iter().enumerate().map(|(i, it)| {
                let rest = format!("{} {} {}", it.group.as_deref().unwrap_or(""), it.hint.as_deref().unwrap_or(""), it.tags.iter().map(|t| t.as_ref()).collect::<Vec<_>>().join(" "));
                (i, fuzzy(&q, &it.label).max(fuzzy(&q, &rest) * 0.5))
            }).filter(|x| x.1 > 0.).collect();
            v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            self.shown = v.into_iter().map(|x| x.0).collect();
            self.rows = (0..self.shown.len()).map(Row::Item).collect();
        }
        self.sel = 0;
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
    }

    fn row_of(&self, sel: usize) -> usize { self.rows.iter().position(|r| matches!(r, Row::Item(i) if *i == sel)).unwrap_or(0) }

    fn pick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(&i) = self.shown.get(self.sel) else { return };
        let run = self.items[i].run.clone();
        window.close_dialog(cx);
        run(window, cx);
    }

    fn step(&mut self, d: isize, cx: &mut Context<Self>) {
        if self.shown.is_empty() { return; }
        let n = self.shown.len() as isize;
        self.sel = ((self.sel as isize + d).rem_euclid(n)) as usize;
        let r = self.row_of(self.sel);
        // keep a group's label in view when stepping up onto its first item
        let top = if r > 0 && matches!(self.rows[r - 1], Row::Head(_)) { r - 1 } else { r };
        if self.sel == 0 { self.scroll.scroll_to_item(0, ScrollStrategy::Top); }
        else { self.scroll.scroll_to_item(if d < 0 { top } else { r }, ScrollStrategy::Nearest); }
        cx.notify();
    }
}

impl Render for Picker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = pal(cx);
        let view = cx.entity();
        let count = self.rows.len();
        let browsing = self.input.read(cx).value().trim().is_empty();
        v_flex().w_full()
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                let k = &e.keystroke;
                let ctrl = k.modifiers.control && !k.modifiers.alt && !k.modifiers.shift;
                match k.key.as_str() {
                    "down" => { this.step(1, cx); cx.stop_propagation(); }
                    "up" => { this.step(-1, cx); cx.stop_propagation(); }
                    "n" if ctrl => { this.step(1, cx); cx.stop_propagation(); }
                    "p" if ctrl => { this.step(-1, cx); cx.stop_propagation(); }
                    "pagedown" => { this.step(MAX_ROWS as isize - 1, cx); cx.stop_propagation(); }
                    "pageup" => { this.step(-(MAX_ROWS as isize - 1), cx); cx.stop_propagation(); }
                    _ => {}
                }
            }))
            .child(div().px_3().py_1().child(Input::new(&self.input).prefix(Icon::new(IconName::Search).small().text_color(p.dim)).appearance(false).cleanable(false)))
            .child(div().h_px().bg(cx.theme().border))
            .child(if self.shown.is_empty() {
                div().px_4().py_6().text_sm().text_color(p.dim).text_center().child("Nothing matches").into_any_element()
            } else {
                div().p_1p5().child(uniform_list("picker-list", count, move |range, _, cx| {
                    let this = view.read(cx);
                    range.map(|r| match this.rows[r] {
                        Row::Head(g) => group_label(this.heads[g].clone().unwrap_or_default(), cx),
                        Row::Item(s) => {
                            let it = &this.items[this.shown[s]];
                            item_row(it, s, s == this.sel, browsing, view.clone(), cx)
                        }
                    }).collect()
                }).track_scroll(&self.scroll).h(px((count.min(MAX_ROWS) as f32) * ROW_H))).into_any_element()
            })
    }
}

fn group_label(label: SharedString, cx: &App) -> AnyElement {
    let p = pal(cx);
    h_flex().w_full().h(px(ROW_H)).px_2().pt_2().items_center().text_xs().font_weight(FontWeight::MEDIUM).text_color(p.dim).child(label).into_any_element()
}

fn item_row(it: &PickItem, s: usize, sel: bool, browsing: bool, view: Entity<Picker>, cx: &App) -> AnyElement {
    let p = pal(cx);
    let (v1, v2) = (view.clone(), view);
    h_flex().id(("pick", s)).w_full().h(px(ROW_H)).px_2().gap_2().items_center().rounded(cx.theme().radius)
        .when(sel, |d| d.bg(cx.theme().muted))
        // a fixed lane for the icon, or the check on the current choice, so labels line up
        .child(h_flex().flex_none().w(px(16.)).justify_center().child(match (it.on, it.icon) {
            (true, _) => Icon::new(IconName::Check).small().text_color(cx.theme().primary).into_any_element(),
            (false, Some(i)) => Icon::new(i).small().text_color(p.dim).into_any_element(),
            _ => div().into_any_element(),
        }))
        .child(div().flex_none().max_w(relative(0.62)).truncate().text_color(cx.theme().foreground).child(it.label.clone()))
        .child(div().flex_1().min_w_0().truncate().text_sm().text_color(p.dim).child(it.hint.clone().unwrap_or_default()))
        .children(it.tags.iter().map(|t| div().flex_none().text_xs().px_1p5().rounded(px(4.)).border_1().border_color(cx.theme().border).text_color(p.mid).child(t.clone())))
        // searching mixes groups, so each row says which one it's from
        .when(!browsing && it.right.is_none() && it.keys.is_none(), |d| d.when_some(it.group.clone(), |d, g| d.child(div().flex_none().text_xs().text_color(p.dim).child(g))))
        .when_some(it.right.clone(), |d, r| d.child(div().flex_none().text_xs().text_color(p.dim).child(r)))
        .when_some(it.keys.clone(), |d, k| d.child(Kbd::new(k)))
        .on_mouse_move(move |e: &MouseMoveEvent, _, cx| v1.update(cx, |this, cx| {
            let moved = this.mouse.is_some_and(|m| m != e.position);
            this.mouse = Some(e.position);
            if moved && this.sel != s { this.sel = s; cx.notify(); }
        }))
        .on_click(move |_, window, cx| v2.update(cx, |this, cx| { this.sel = s; this.pick(window, cx); }))
        .into_any_element()
}

impl Focusable for Picker {
    fn focus_handle(&self, cx: &App) -> FocusHandle { self.input.read(cx).focus_handle(cx) }
}

pub fn open(items: Vec<PickItem>, placeholder: &str, window: &mut Window, cx: &mut App) {
    let ph = placeholder.to_string();
    let view = cx.new(|cx| Picker::new(items, &ph, window, cx));
    let f = view.read(cx).focus_handle(cx);
    window.open_dialog(cx, move |d, _, _| d.w(px(640.)).margin_top(px(88.)).p_0().close_button(false).child(view.clone()));
    f.focus(window, cx);
}

pub fn models(window: &mut Window, cx: &mut App) {
    let app = Pith::global(cx);
    if app.read(cx).active.is_none() { return; }
    if app.read(cx).models.is_empty() {
        app.update(cx, |a, cx| a.load_models(cx));
        // they come from pi; the picker opens once they're in
        let w = window.window_handle();
        let sub = std::rc::Rc::new(std::cell::RefCell::new(None));
        let s2 = sub.clone();
        *sub.borrow_mut() = Some(cx.observe(&app, move |app, cx| {
            if app.read(cx).models.is_empty() { return; }
            s2.borrow_mut().take();
            let _ = w.update(cx, |_, window, cx| models(window, cx));
        }));
        return;
    }
    let Some(tab) = app.read(cx).active.clone() else { return };
    let (cur_id, cur_p) = { let t = tab.read(cx); (t.state["model"]["id"].as_str().unwrap_or("").to_string(), t.state["model"]["provider"].as_str().unwrap_or("").to_string()) };
    let mut list = app.read(cx).models.clone();
    // the current provider first, then the rest by name
    list.sort_by(|a, b| (a.provider != cur_p, &a.provider).cmp(&(b.provider != cur_p, &b.provider)));
    let items = list.iter().map(|m| {
        let (t, prov, id) = (tab.clone(), m.provider.clone(), m.id.clone());
        let mut it = PickItem::new(if m.name.is_empty() { m.id.clone() } else { m.name.clone() }, move |_, cx| t.update(cx, |t, cx| t.set_model(prov.clone(), id.clone(), cx)))
            .group(m.provider.clone()).on(m.id == cur_id && m.provider == cur_p)
            .hint(if !m.name.is_empty() && m.name != m.id { m.id.clone() } else { String::new() });
        if m.reasoning { it = it.tag("Reasoning"); }
        if m.sees { it = it.tag("Vision"); }
        if m.context > 0 { it = it.right(super::tools::tokens(m.context)); }
        it
    }).collect();
    open(items, "Switch model", window, cx);
}

// the mode picker, from the status line and the composer's legend
pub fn modes(tab: Entity<crate::state::tab::Tab>, window: &mut Window, cx: &mut App) {
    use crate::state::tab::Mode;
    let cur = tab.read(cx).mode;
    let items = Mode::ALL.into_iter().map(|m| { let t = tab.clone(); PickItem::new(m.as_str(), move |_, cx| t.update(cx, |t, cx| t.set_mode(m, cx))).hint(m.hint()).on(m == cur) }).collect();
    open(items, "type to filter", window, cx);
}

// thinking effort, for models that take one
pub fn effort(tab: Entity<crate::state::tab::Tab>, window: &mut Window, cx: &mut App) {
    let (levels, cur) = { let t = tab.read(cx); (t.levels.clone(), t.state["thinkingLevel"].as_str().unwrap_or("").to_string()) };
    if levels.is_empty() { tab.update(cx, |_, cx| cx.emit(crate::state::tab::TabEvent::Toast(crate::state::tab::Level::Info, "this model doesn't take an effort level".into()))); return; }
    let items = levels.into_iter().map(|l| { let (t, l2) = (tab.clone(), l.clone()); PickItem::new(l.clone(), move |_, cx| t.update(cx, |t, cx| t.set_thinking(l2.clone(), cx))).on(l == cur) }).collect();
    open(items, "type to filter", window, cx);
}
