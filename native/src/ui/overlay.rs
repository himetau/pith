// things that float, as overlay.ts drew them: one modal at a time (the picker, a prompt, the lightbox) on a scrim,
// context menus at the pointer, toasts at the bottom, the fps counter. the workspace draws the layer over everything
use super::theme::{em, fade, geo, mix, pal, u};
use crate::state::app::Pith;
use crate::state::tab::Level;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::rc::Rc;

actions!(overlay, [Dismiss]);

pub fn bind_keys(cx: &mut App) {
    // deeper than the workspace's escape (stop the run), so esc closes what floats first
    cx.bind_keys([KeyBinding::new("escape", Dismiss, Some("Overlay"))]);
}

#[derive(Clone, Copy, PartialEq)]
pub enum Place { Top, Center, Lightbox }

struct Modal { view: AnyView, place: Place }

#[derive(Clone)]
pub struct MenuItem { pub label: SharedString, pub hint: Option<SharedString>, pub danger: bool, pub run: Rc<dyn Fn(&mut Window, &mut App)> }
impl MenuItem {
    pub fn new(label: impl Into<SharedString>, run: impl Fn(&mut Window, &mut App) + 'static) -> Self { MenuItem { label: label.into(), hint: None, danger: false, run: Rc::new(run) } }
    pub fn hint(mut self, h: impl Into<SharedString>) -> Self { self.hint = Some(h.into()); self }
    pub fn danger(mut self) -> Self { self.danger = true; self }
}

struct Menu { at: Point<Pixels>, items: Vec<MenuItem> }

pub struct Layer {
    modal: Option<Modal>,
    menu: Option<Menu>,
    // where focus goes when something closes: the window itself, so typing lands in the composer like it did on body
    back: Option<FocusHandle>,
    fps: Option<Entity<Fps>>,
}

struct G(Entity<Layer>);
impl Global for G {}

pub fn init(back: FocusHandle, cx: &mut App) {
    let l = cx.new(|_| Layer { modal: None, menu: None, back: Some(back), fps: None });
    cx.set_global(G(l));
}
pub fn layer(cx: &App) -> Entity<Layer> { cx.global::<G>().0.clone() }
pub fn is_open(cx: &App) -> bool { cx.try_global::<G>().is_some_and(|g| { let l = g.0.read(cx); l.modal.is_some() || l.menu.is_some() }) }

pub fn show(view: AnyView, focus: Option<FocusHandle>, place: Place, window: &mut Window, cx: &mut App) {
    layer(cx).update(cx, |l, cx| { l.menu = None; l.modal = Some(Modal { view, place }); cx.notify(); });
    if let Some(f) = focus { f.focus(window, cx); }
}

pub fn close(window: &mut Window, cx: &mut App) {
    let back = layer(cx).update(cx, |l, cx| { l.modal = None; l.menu = None; cx.notify(); l.back.clone() });
    if let Some(b) = back { b.focus(window, cx); }
}

pub fn menu(at: Point<Pixels>, items: Vec<MenuItem>, cx: &mut App) {
    layer(cx).update(cx, |l, cx| { l.menu = Some(Menu { at, items }); cx.notify(); });
}

pub fn toggle_fps(cx: &mut App) {
    layer(cx).update(cx, |l, cx| {
        l.fps = if l.fps.is_some() { None } else { Some(cx.new(|_| Fps { frames: 0, since: std::time::Instant::now(), text: "… fps".into() })) };
        cx.notify();
    });
}

pub fn shadow(y: f32, blur: f32, a: f32, cx: &App) -> Vec<BoxShadow> {
    let s = geo(cx).scale;
    vec![BoxShadow { color: hsla(0., 0., 0., a), offset: point(px(0.), px(y * s)), blur_radius: px(blur * s), spread_radius: px(0.), inset: false }]
}

// a hover tooltip, held back while something floats so it doesn't sit on a menu
pub trait Tip { fn tip(self, text: impl Into<SharedString>, cx: &App) -> Self; }
impl<E: StatefulInteractiveElement> Tip for E {
    fn tip(self, text: impl Into<SharedString>, cx: &App) -> Self {
        if is_open(cx) { return self; }
        let t: SharedString = text.into();
        self.tooltip(move |w, cx| gpui_kit::component::tooltip::Tooltip::new(t.clone()).build(w, cx))
    }
}

// .btn, .btn.ghost, .btn.pri
#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq)]
pub enum Btn { Plain, Ghost, Pri }
pub fn btn(id: impl Into<ElementId>, kind: Btn, cx: &App) -> Stateful<Div> {
    let p = pal(cx);
    let (bg, fg, line, hover) = match kind {
        Btn::Plain => (p.raised, p.text, p.line, p.high),
        Btn::Ghost => (fade(p.raised, 0.), p.text, p.line, p.high),
        Btn::Pri => (p.primary, p.on_primary, p.primary, mix(p.primary, 0.88, gpui_kit::white())),
    };
    div().id(id).flex().items_center().gap(u(6.)).px(u(12.)).py(u(5.)).rounded(u(geo(cx).r2)).border_1().border_color(line)
        .bg(bg).text_color(fg).whitespace_nowrap().cursor_pointer().hover(move |s| s.bg(hover))
}

// a plain css input: no box of its own, body text, padding py above and below
// (the kit's input runs taller than it's told, so a box of the right height holds it)
pub fn field(state: &Entity<InputState>, py: f32, cx: &App) -> Div {
    let g = geo(cx);
    div().flex_1().min_w_0().h(u(g.fs * 1.55 + py * 2.)).flex().items_center()
        .child(Input::new(state).appearance(false).px(u(0.)).py(u(0.)).h(u(g.fs * 1.55)).text_size(u(g.fs)).line_height(relative(1.55)))
}

// kbd: the little key cap
pub fn kbd(text: impl Into<SharedString>, cx: &App) -> Div {
    let p = pal(cx);
    let s = geo(cx).scale;
    div().flex_none().text_size(em(cx, 0.78)).line_height(relative(1.)).pt(u(2.)).pb(u(3.)).px(u(5.)).rounded(u(4.)).text_color(p.mid).bg(p.high)
        .shadow(vec![BoxShadow { color: p.line, offset: point(px(0.), px(-1. * s)), blur_radius: px(0.), spread_radius: px(0.), inset: true }])
        .child(text.into())
}

// the frame pickers and prompts share (.picker, .dlg): the active border, the title in it
pub fn frame(title: impl Into<SharedString>, w: Pixels, cx: &App) -> (Div, Div) {
    let (p, g) = (pal(cx), geo(cx));
    let body = div().relative().w(w).bg(p.bg).rounded(u(g.round)).p(u(g.bw)).shadow(shadow(24., 60., 0.45, cx)).flex().flex_col();
    let deco = div().absolute().top_0().left_0().right_0().bottom_0()
        .child(super::frame::edge(p.win_on, cx))
        .child(super::frame::legend(super::frame::Side::L, p.bg, cx).px(u(6.)).text_color(p.text).child(title.into()));
    (body, deco)
}

// what the workspace draws over its windows
pub fn render(window: &mut Window, cx: &mut App) -> AnyElement {
    let (p, g) = (pal(cx), geo(cx));
    let vp = window.viewport_size();
    let l = layer(cx);
    let (modal, menu, fps) = { let l = l.read(cx); (l.modal.as_ref().map(|m| (m.view.clone(), m.place)), l.menu.as_ref().map(|m| (m.at, m.items.clone())), l.fps.clone()) };
    let mut root = div().absolute().top_0().left_0().right_0().bottom_0().key_context("Overlay")
        .on_action(|_: &Dismiss, window, cx| if is_open(cx) { close(window, cx) } else { cx.propagate() });
    if let Some((view, place)) = modal {
        let scrim = fade(p.surface_lowest, if place == Place::Lightbox { 0.88 } else { 0.55 });
        root = root.child(div().id("scrim").absolute().top_0().left_0().right_0().bottom_0().bg(scrim).occlude().flex().flex_col().items_center()
            .when(place == Place::Top, |d| d.pt(vp.height * 0.12))
            .when(place != Place::Top, |d| d.justify_center())
            .on_mouse_down(MouseButton::Left, |_, window, cx| close(window, cx))
            // the lightbox is the whole window and handles its own clicks
            .when(place == Place::Lightbox, |d| d.child(div().size_full().child(view.clone())))
            .when(place != Place::Lightbox, |d| d.child(div().on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()).child(view))));
    }
    if let Some((at, items)) = menu {
        let r2 = g.r2;
        root = root.child(anchored().position(at).snap_to_window_with_margin(px(8.)).child(
            div().id("ctx-menu").min_w(u(190.)).p(u(4.)).bg(p.raised).border_1().border_color(p.line).rounded(u(r2)).shadow(shadow(10., 30., 0.35, cx)).occlude()
                .on_mouse_down_out(|_, _, cx| { layer(cx).update(cx, |l, cx| { l.menu = None; cx.notify(); }); })
                .children(items.into_iter().enumerate().map(|(i, it)| {
                    let run = it.run.clone();
                    let high = p.high;
                    div().id(("ctx", i)).flex().justify_between().gap(u(16.)).w_full().px(u(10.)).py(u(4.)).rounded(u(4.)).text_size(em(cx, 0.92)).cursor_pointer()
                        .when(it.danger, |d| d.text_color(p.red)).hover(move |s| s.bg(high))
                        .child(div().child(it.label.clone()))
                        .when_some(it.hint.clone(), |d, h| d.child(div().text_color(p.dim).child(h)))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(move |_, window, cx| { layer(cx).update(cx, |l, cx| { l.menu = None; cx.notify(); }); run(window, cx); })
                })),
        ));
    }
    root = root.child(toasts(cx));
    if let Some(f) = fps {
        root = root.child(div().absolute().right(u(14.)).bottom(u(34.)).px(u(8.)).py(u(2.)).rounded(u(4.)).bg(p.surface_highest).text_color(p.text).text_size(em(cx, 0.85)).child(f));
    }
    root.into_any_element()
}

// #toasts: bottom centre, 38px up, newest last
fn toasts(cx: &App) -> impl IntoElement {
    let (p, g) = (pal(cx), geo(cx));
    let list = Pith::global(cx).read(cx).toasts.clone();
    div().absolute().left_0().right_0().bottom(u(38.)).flex().flex_col().items_center().gap(u(6.))
        .children(list.into_iter().map(|t| {
            let (edge, tk) = match t.level { Level::Error => (p.red, "!"), Level::Warn => (p.gold, "~"), Level::Info => (p.mid, "›") };
            // border-left 3px in the level's colour: the coloured box shows through on the left
            div().max_w(u(460.)).rounded(u(g.r2)).bg(edge).pl(u(3.)).shadow(shadow(8., 24., 0.3, cx))
                .child(div().flex().gap(u(8.)).px(u(12.)).py(u(6.)).bg(p.raised).border_t_1().border_r_1().border_b_1().border_color(p.line)
                    .rounded_tr(u(g.r2)).rounded_br(u(g.r2)).text_size(em(cx, 0.9))
                    .child(div().text_color(p.dim).child(tk)).child(div().child(t.text.clone())))
        }))
}

// ctrl+shift+f: frames per second. it asks for every frame while it's on, so it shows what the display delivers
pub struct Fps { frames: u32, since: std::time::Instant, text: SharedString }
impl Render for Fps {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.frames += 1;
        let el = self.since.elapsed().as_secs_f32();
        if el >= 1. { self.text = format!("{} fps", (self.frames as f32 / el).round()).into(); self.frames = 0; self.since = std::time::Instant::now(); }
        window.request_animation_frame();
        div().child(self.text.clone())
    }
}

// ---------- prompt: a one-line question with ok / cancel (overlay.ts prompt) ----------
pub struct Prompt { title: SharedString, input: Entity<InputState>, ok: Rc<dyn Fn(String, &mut Window, &mut App)>, _sub: Subscription }

impl Prompt {
    fn done(&self, window: &mut Window, cx: &mut App) {
        let v = self.input.read(cx).value().trim().to_string();
        let ok = self.ok.clone();
        close(window, cx);
        if !v.is_empty() { ok(v, window, cx); }
    }
}

pub fn prompt(title: &str, value: &str, ok: impl Fn(String, &mut Window, &mut App) + 'static, window: &mut Window, cx: &mut App) {
    let (title, value) = (SharedString::from(title.to_string()), value.to_string());
    let view = cx.new(|cx| {
        let input = cx.new(|cx| InputState::new(window, cx).default_value(value));
        // the whole value selected, so typing replaces it
        input.update(cx, |i, cx| i.select_all(window, cx));
        let sub = cx.subscribe_in(&input, window, |this: &mut Prompt, _, ev: &InputEvent, window, cx| if let InputEvent::PressEnter { .. } = ev { this.done(window, cx) });
        Prompt { title, input, ok: Rc::new(ok), _sub: sub }
    });
    let f = view.read(cx).input.read(cx).focus_handle(cx);
    show(view.into(), Some(f), Place::Center, window, cx);
}

impl Render for Prompt {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (p, g) = (pal(cx), geo(cx));
        let w = (window.viewport_size().width * 0.92).min(px(660. * g.scale));
        let (body, deco) = frame(self.title.clone(), w, cx);
        let focused = self.input.read(cx).focus_handle(cx).is_focused(window);
        let view = cx.entity();
        body.child(div().flex().flex_col().gap(u(12.)).pt(u(18.)).px(u(14.)).pb(u(12.))
            .child(div().bg(p.raised).border_1().border_color(if focused { p.primary } else { p.line }).rounded(u(g.r2)).px(u(10.)).py(u(7.))
                .child(field(&self.input, 0., cx)))
            .child(div().flex().justify_end().gap(u(8.))
                .child(btn("dlg-cancel", Btn::Ghost, cx).child("cancel").on_click(|_, window, cx| close(window, cx)))
                .child(btn("dlg-ok", Btn::Pri, cx).child("ok").on_click(move |_, window, cx| { let v = view.clone(); v.update(cx, |p, cx| p.done(window, cx)) }))))
            .child(deco)
    }
}
