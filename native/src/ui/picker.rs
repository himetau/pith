// one picker for everything: the command palette, models, sessions. type to filter, arrows and Enter to pick
use super::theme::pal;
use crate::state::app::Pith;
use gpui_kit::component::input::{Input, InputEvent, InputState};
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
    pub run: Rc<dyn Fn(&mut Window, &mut App)>,
}

impl PickItem {
    pub fn new(label: impl Into<SharedString>, run: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        PickItem { label: label.into(), hint: None, group: None, right: None, on: false, run: Rc::new(run) }
    }
    pub fn hint(mut self, h: impl Into<SharedString>) -> Self { self.hint = Some(h.into()); self }
    pub fn group(mut self, g: impl Into<SharedString>) -> Self { self.group = Some(g.into()); self }
    pub fn right(mut self, r: impl Into<SharedString>) -> Self { self.right = Some(r.into()); self }
    pub fn on(mut self, on: bool) -> Self { self.on = on; self }
}

pub struct Picker {
    items: Vec<PickItem>,
    shown: Vec<usize>,
    sel: usize,
    input: Entity<InputState>,
    scroll: UniformListScrollHandle,
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
        let sel = items.iter().position(|i| i.on).unwrap_or(0);
        let mut p = Picker { shown: (0..items.len()).collect(), items, sel, input, scroll: UniformListScrollHandle::new(), _sub: sub };
        p.scroll.scroll_to_item(sel, ScrollStrategy::Center);
        p
    }

    fn filter(&mut self, cx: &App) {
        let q = self.input.read(cx).value().trim().to_string();
        let mut v: Vec<(usize, f32)> = self.items.iter().enumerate().map(|(i, it)| {
            let rest = format!("{} {}", it.group.as_deref().unwrap_or(""), it.hint.as_deref().unwrap_or(""));
            (i, fuzzy(&q, &it.label).max(fuzzy(&q, &rest) * 0.5))
        }).filter(|x| x.1 > 0.).collect();
        if !q.is_empty() { v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap()); }
        self.shown = v.into_iter().map(|x| x.0).collect();
        self.sel = 0;
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
    }

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
        self.scroll.scroll_to_item(self.sel, ScrollStrategy::Nearest);
        cx.notify();
    }
}

impl Render for Picker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = pal(cx);
        let view = cx.entity();
        let count = self.shown.len();
        v_flex().w_full().gap_2()
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| match e.keystroke.key.as_str() {
                "down" => { this.step(1, cx); cx.stop_propagation(); }
                "up" => { this.step(-1, cx); cx.stop_propagation(); }
                _ => {}
            }))
            .child(Input::new(&self.input).prefix(Icon::new(IconName::Search).small().text_color(p.dim)).appearance(false).cleanable(false))
            .child(div().h_px().bg(cx.theme().border))
            .child(if count == 0 {
                div().p_3().text_sm().text_color(p.dim).child("Nothing matches").into_any_element()
            } else {
                uniform_list("picker-list", count, move |range, _, cx| {
                    let this = view.read(cx);
                    let p = pal(cx);
                    range.map(|r| {
                        let i = this.shown[r];
                        let it = &this.items[i];
                        let sel = r == this.sel;
                        let v = view.clone();
                        h_flex().id(("pick", r)).h(px(32.)).px_2().gap_2().items_center().rounded(cx.theme().radius)
                            .when(sel, |d| d.bg(cx.theme().muted))
                            .child(div().w(px(10.)).text_xs().text_color(cx.theme().primary).child(if it.on { "●" } else { "" }))
                            .child(div().flex_none().max_w(relative(0.6)).truncate().text_color(cx.theme().foreground).child(it.label.clone()))
                            .when_some(it.group.clone(), |d, g| d.child(div().flex_none().text_xs().px_1p5().rounded(px(4.)).border_1().border_color(p.hairline).text_color(p.dim).child(g)))
                            .child(div().flex_1().min_w_0().truncate().text_sm().text_color(p.dim).child(it.hint.clone().unwrap_or_default()))
                            .when_some(it.right.clone(), |d, r| d.child(div().flex_none().text_xs().text_color(p.dim).child(r)))
                            .on_mouse_move(move |_, _, cx| v.update(cx, |this, cx| if this.sel != r { this.sel = r; cx.notify(); }))
                            .on_click({ let v = view.clone(); move |_, window, cx| v.update(cx, |this, cx| { this.sel = r; this.pick(window, cx); }) })
                    }).collect()
                }).track_scroll(&self.scroll).h(px((count.min(11) as f32) * 32.)).into_any_element()
            })
    }
}

impl Focusable for Picker {
    fn focus_handle(&self, cx: &App) -> FocusHandle { self.input.read(cx).focus_handle(cx) }
}

pub fn open(items: Vec<PickItem>, placeholder: &str, window: &mut Window, cx: &mut App) {
    let ph = placeholder.to_string();
    let view = cx.new(|cx| Picker::new(items, &ph, window, cx));
    let f = view.read(cx).focus_handle(cx);
    window.open_dialog(cx, move |d, _, _| d.w(px(620.)).margin_top(px(90.)).close_button(false).child(view.clone()));
    f.focus(window, cx);
}

pub fn models(window: &mut Window, cx: &mut App) {
    let app = Pith::global(cx);
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
    let items = app.read(cx).models.iter().map(|m| {
        let (t, prov, id) = (tab.clone(), m.provider.clone(), m.id.clone());
        let right = [if m.context > 0 { format!("{}k", m.context / 1000) } else { String::new() }, if m.reasoning { "thinks".into() } else { String::new() }, if m.sees { "sees".into() } else { String::new() }]
            .into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
        PickItem::new(m.id.clone(), move |_, cx| t.update(cx, |t, cx| t.set_model(prov.clone(), id.clone(), cx)))
            .group(m.provider.clone()).right(right).on(m.id == cur_id && m.provider == cur_p)
            .hint(if m.name != m.id { m.name.clone() } else { String::new() })
    }).collect();
    open(items, "Switch model", window, cx);
}
