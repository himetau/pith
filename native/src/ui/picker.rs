// one picker for everything: the command palette, models, effort, mode (overlay.ts picker). type to filter,
// arrows or ctrl+n/p and Enter to pick, esc or a click outside to close
use super::icons::ic;
use super::overlay::{self, kbd, Place};
use super::theme::{em, geo, pal, u};
use crate::state::app::Pith;
use gpui_kit::component::input::{InputEvent, InputState};
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
    pub keys: Option<SharedString>,
    pub tags: Vec<SharedString>,
    pub run: Rc<dyn Fn(&mut Window, &mut App)>,
}

impl PickItem {
    pub fn new(label: impl Into<SharedString>, run: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        PickItem { label: label.into(), hint: None, group: None, right: None, on: false, keys: None, tags: vec![], run: Rc::new(run) }
    }
    pub fn hint(mut self, h: impl Into<SharedString>) -> Self { let h = h.into(); self.hint = if h.is_empty() { None } else { Some(h) }; self }
    pub fn group(mut self, g: impl Into<SharedString>) -> Self { self.group = Some(g.into()); self }
    pub fn right(mut self, r: impl Into<SharedString>) -> Self { let r = r.into(); self.right = if r.is_empty() { None } else { Some(r) }; self }
    pub fn on(mut self, on: bool) -> Self { self.on = on; self }
    // the webview picker had no icons; kept so callers don't have to change
    pub fn icon(self, _: IconName) -> Self { self }
    // the shortcut, as text ("ctrl+shift+n"). gpui's "secondary-n" spelling is turned into that
    pub fn keys(mut self, k: &str) -> Self {
        let k = if k.contains('+') { k.to_string() } else { k.replace("secondary", if cfg!(target_os = "macos") { "cmd" } else { "ctrl" }).replace('-', "+") };
        self.keys = Some(k.into());
        self
    }
    pub fn tag(mut self, t: impl Into<SharedString>) -> Self { self.tags.push(t.into()); self }
}

pub struct Picker {
    title: SharedString,
    items: Vec<PickItem>,
    // indexes into items, in the order shown
    shown: Vec<usize>,
    // index into shown
    sel: usize,
    input: Entity<InputState>,
    scroll: ScrollHandle,
    // the pointer only takes over the selection when it actually moves, not when the list scrolls under it
    mouse: Option<Point<Pixels>>,
    _sub: Subscription,
}

// composer.ts fuzzy: letters in order, runs and word starts score more, a plain substring a lot more
pub fn fuzzy(q: &str, s: &str) -> f32 {
    if q.is_empty() { return 1.; }
    let q: Vec<char> = q.to_lowercase().chars().collect();
    let t: Vec<char> = s.to_lowercase().chars().collect();
    let (mut score, mut j, mut run) = (0f32, 0usize, 0f32);
    for i in 0..t.len() {
        if j >= q.len() { break; }
        if t[i] == q[j] {
            j += 1;
            run += 1.;
            score += 1. + run * 2. + if i == 0 || "/-_. ".contains(t[i - 1]) { 6. } else { 0. };
        } else { run = 0.; }
    }
    if j < q.len() { return 0.; }
    let qs: String = q.iter().collect();
    let ts: String = t.iter().collect();
    score - t.len() as f32 * 0.02 + if ts.contains(&qs) { 12. } else { 0. }
}

impl Picker {
    pub fn new(title: &str, items: Vec<PickItem>, placeholder: &str, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder.to_string()));
        let sub = cx.subscribe_in(&input, window, |this: &mut Picker, _, ev: &InputEvent, window, cx| match ev {
            InputEvent::Change => { this.sel = 0; this.filter(cx); cx.notify(); }
            InputEvent::PressEnter { .. } => this.pick(window, cx),
            _ => {}
        });
        let mut p = Picker { title: title.to_string().into(), shown: vec![], items, sel: 0, input, scroll: ScrollHandle::new(), mouse: None, _sub: sub };
        p.filter(cx);
        p.sel = p.items.iter().position(|i| i.on).unwrap_or(0);
        p
    }

    fn filter(&mut self, cx: &App) {
        let q = self.input.read(cx).value().trim().to_string();
        if q.is_empty() { self.shown = (0..self.items.len()).collect(); } else {
            let mut v: Vec<(usize, f32)> = self.items.iter().enumerate().map(|(i, it)| {
                let rest = format!("{} {}", it.tags.first().or(it.group.as_ref()).map(|s| s.as_ref()).unwrap_or(""), it.hint.as_deref().unwrap_or(""));
                (i, fuzzy(&q, &it.label).max(fuzzy(&q, &rest) * 0.5))
            }).filter(|x| x.1 > 0.).collect();
            v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            self.shown = v.into_iter().map(|x| x.0).collect();
        }
        self.sel = self.sel.min(self.shown.len().saturating_sub(1));
    }

    fn pick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(&i) = self.shown.get(self.sel) else { return };
        let run = self.items[i].run.clone();
        overlay::close(window, cx);
        run(window, cx);
    }

    fn step(&mut self, d: isize, cx: &mut Context<Self>) {
        if self.shown.is_empty() { return; }
        self.sel = (self.sel as isize + d).clamp(0, self.shown.len() as isize - 1) as usize;
        self.reveal();
        cx.notify();
    }

    // the list's children are group labels and rows mixed, so find the selected row's child index
    fn reveal(&self) {
        let browsing = self.shown.len() == self.items.len();
        let mut child = 0;
        let mut last: Option<&SharedString> = None;
        for (n, &i) in self.shown.iter().enumerate() {
            let g = self.items[i].group.as_ref();
            if browsing && g.is_some() && g != last { child += 1; last = g; }
            if n == self.sel { break; }
            child += 1;
        }
        self.scroll.scroll_to_item(child);
    }
}

impl Render for Picker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (p, g) = (pal(cx), geo(cx));
        let vp = window.viewport_size();
        let w = (vp.width * 0.92).min(px(660. * g.scale));
        let (body, deco) = overlay::frame(self.title.clone(), w, cx);
        let q_empty = self.input.read(cx).value().trim().is_empty();
        let view = cx.entity();
        let mut rows: Vec<AnyElement> = vec![];
        let mut last: Option<SharedString> = None;
        for (n, &i) in self.shown.iter().enumerate() {
            let it = &self.items[i];
            if q_empty && it.group.is_some() && it.group != last {
                last = it.group.clone();
                rows.push(div().pt(u(8.)).px(u(10.)).pb(u(2.)).text_size(em(cx, 0.78)).text_color(p.dim).child(it.group.clone().unwrap_or_default()).into_any_element());
            }
            rows.push(row(it, n, n == self.sel, view.clone(), cx));
        }
        body.on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                let k = &e.keystroke;
                let ctrl = k.modifiers.control && !k.modifiers.alt && !k.modifiers.shift;
                match k.key.as_str() {
                    "down" => { this.step(1, cx); cx.stop_propagation(); }
                    "up" => { this.step(-1, cx); cx.stop_propagation(); }
                    "n" if ctrl => { this.step(1, cx); cx.stop_propagation(); }
                    "p" if ctrl => { this.step(-1, cx); cx.stop_propagation(); }
                    _ => {}
                }
            }))
            // .pk-top
            .child(div().flex().items_center().gap(u(8.)).pt(u(14.)).px(u(14.)).pb(u(10.)).text_color(p.dim).border_b_1().border_color(p.rule)
                .child(ic("search").size(em(cx, 1.2)).text_color(p.dim))
                .child(div().flex_1().min_w_0().text_color(p.text).child(overlay::field(&self.input, 0., cx))))
            .child(if self.shown.is_empty() {
                div().p(u(14.)).text_color(p.dim).child("nothing matches").into_any_element()
            } else {
                div().id("pk-list").max_h(vp.height * 0.56).overflow_y_scroll().track_scroll(&self.scroll).p(u(4.)).children(rows).into_any_element()
            })
            .child(deco)
    }
}

// .pk-row: mark, label, tag, hint, right, kbd
fn row(it: &PickItem, n: usize, sel: bool, view: Entity<Picker>, cx: &App) -> AnyElement {
    let p = pal(cx);
    let (v1, v2) = (view.clone(), view);
    div().id(("pick", n)).flex().items_baseline().gap(u(10.)).px(u(10.)).py(u(4.)).rounded(u(5.)).cursor_pointer()
        .when(sel, |d| d.bg(p.high))
        .child(div().flex_none().w(em(cx, 0.7 * 0.6)).text_size(em(cx, 0.7)).text_color(p.accent).child(if it.on { "●" } else { "" }))
        .child(div().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().child(it.label.clone()))
        .children(it.tags.iter().map(|t| div().flex_none().text_size(em(cx, 0.78)).text_color(p.mid).whitespace_nowrap().px(u(5.)).border_1().border_color(p.line).rounded(u(4.)).child(t.clone())))
        .when_some(it.hint.clone(), |d, h| d.child(div().flex_1().min_w_0().text_size(em(cx, 0.88)).text_color(p.dim).overflow_hidden().text_ellipsis().whitespace_nowrap().child(h)))
        .when_some(it.right.clone(), |d, r| d.child(div().flex_none().ml_auto().text_size(em(cx, 0.82)).text_color(p.dim).whitespace_nowrap().child(r)))
        .when_some(it.keys.clone(), |d, k| d.child(kbd(k, cx)))
        .on_mouse_move(move |e: &MouseMoveEvent, _, cx| v1.update(cx, |this, cx| {
            let moved = this.mouse.is_some_and(|m| m != e.position);
            this.mouse = Some(e.position);
            if moved && this.sel != n { this.sel = n; cx.notify(); }
        }))
        .on_mouse_down(MouseButton::Left, move |_, window, cx| { cx.stop_propagation(); v2.update(cx, |this, cx| { this.sel = n; this.pick(window, cx); }) })
        .into_any_element()
}

// a picker titled `title` (the legend), "type to filter" in the box
pub fn open(items: Vec<PickItem>, title: &str, window: &mut Window, cx: &mut App) { open_with(&title.to_lowercase(), "type to filter", items, window, cx) }

pub fn open_with(title: &str, placeholder: &str, items: Vec<PickItem>, window: &mut Window, cx: &mut App) {
    let view = cx.new(|cx| Picker::new(title, items, placeholder, window, cx));
    let f = view.read(cx).input.read(cx).focus_handle(cx);
    view.read(cx).reveal();
    overlay::show(view.into(), Some(f), Place::Top, window, cx);
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
    // the current one first, then its provider's
    list.sort_by_key(|m| (!(m.id == cur_id && m.provider == cur_p), m.provider != cur_p));
    let items = list.iter().map(|m| {
        let (t, prov, id) = (tab.clone(), m.provider.clone(), m.id.clone());
        let right = [if m.context > 0 { format!("{}k", (m.context as f64 / 1000.).round()) } else { String::new() }, if m.reasoning { "thinks".into() } else { String::new() }, if m.sees { "sees".into() } else { String::new() }]
            .into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
        PickItem::new(m.id.clone(), move |_, cx| t.update(cx, |t, cx| t.set_model(prov.clone(), id.clone(), cx)))
            .group(m.provider.clone()).tag(m.provider.clone()).on(m.id == cur_id && m.provider == cur_p)
            .hint(if !m.name.is_empty() && m.name != m.id { m.name.clone() } else { String::new() }).right(right)
    }).collect();
    open_with("model", "search models", items, window, cx);
}

// the mode picker, from the status line, the palette and the composer's legend
pub fn modes(tab: Entity<crate::state::tab::Tab>, window: &mut Window, cx: &mut App) {
    use crate::state::tab::Mode;
    let cur = tab.read(cx).mode;
    let items = Mode::ALL.into_iter().map(|m| { let t = tab.clone(); PickItem::new(m.as_str(), move |_, cx| t.update(cx, |t, cx| t.set_mode(m, cx))).hint(m.hint()).on(m == cur) }).collect();
    open_with("mode", "type to filter", items, window, cx);
}

// thinking effort, for models that take one
pub fn effort(tab: Entity<crate::state::tab::Tab>, window: &mut Window, cx: &mut App) {
    let (levels, cur) = { let t = tab.read(cx); (t.levels.clone(), t.state["thinkingLevel"].as_str().unwrap_or("").to_string()) };
    if levels.is_empty() { Pith::global(cx).update(cx, |a, cx| a.toast(crate::state::tab::Level::Info, "this model doesn't take an effort level", cx)); return; }
    let items = levels.into_iter().map(|l| { let (t, l2) = (tab.clone(), l.clone()); PickItem::new(l.clone(), move |_, cx| t.update(cx, |t, cx| t.set_thinking(l2.clone(), cx))).on(l == cur) }).collect();
    open_with("effort", "type to filter", items, window, cx);
}
