// What the full pages (settings, usage, agents, images) share, as pages.css and base.css draw it: the .page column,
// the "❯ title" head, .btn, .segs, .toggle, kbd, .link, the .s-in field and the boxes with a legend in their border
use super::frame::{boxed, edge, legend, Side};
use super::theme::{em, geo, mix, pal, u};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::Icon;
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;
use std::rc::Rc;

// .page > * { max-width: 1080px; margin: auto }
pub const MAX_W: f32 = 1080.;

// .page: scrolls as a whole, 26 34 60 padding, its blocks centred
pub fn page(id: &'static str, kids: Vec<AnyElement>) -> Stateful<Div> {
    div().id(id).size_full().overflow_y_scroll()
        .child(div().w_full().flex().flex_col().items_center().pt(u(26.)).px(u(34.)).pb(u(60.)).children(kids))
}
// one of the page's blocks, as wide as the column allows
pub fn wide() -> Div { div().w_full().max_w(u(MAX_W)).flex_none() }

// .page-head: "❯ title", a dim line after it, the page's buttons on the right
pub fn head(title: &str, sub: Option<SharedString>, actions: Vec<AnyElement>, cx: &App) -> Div {
    let p = pal(cx);
    let text = format!("❯ {title}");
    let h1 = StyledText::new(text).with_highlights([(0..4, HighlightStyle { color: Some(p.primary), ..Default::default() })]);
    wide().flex().items_baseline().gap(u(14.)).mb(u(18.))
        .child(div().flex_none().text_size(em(cx, 1.35)).font_weight(FontWeight::BOLD).child(h1))
        .when_some(sub, |d, s| d.child(div().min_w_0().text_color(p.dim).child(s)))
        .child(div().flex_1())
        // buttons sit where css's baseline puts them, a pixel under the head's top; gpui has no baseline for them
        .children(actions.into_iter().map(|a| div().self_start().mt(u(1.)).child(a)))
}

// .g-note: a dim paragraph under a head or inside a group, 72ch at most
pub fn note(text: impl Into<SharedString>, cx: &App) -> Div {
    div().mt(u(-6.)).mb(u(16.)).text_size(em(cx, 0.9)).max_w(u(geo(cx).fs * 0.9 * 0.6 * 72.)).text_color(pal(cx).dim).child(text.into())
}

#[derive(Clone, Copy, PartialEq)]
pub enum Btn { Plain, Pri, Ghost }

// .btn, .btn.pri, .btn.ghost, and .sm
pub fn btn(id: impl Into<ElementId>, kind: Btn, sm: bool, cx: &App) -> Stateful<Div> {
    let (p, g) = (pal(cx), geo(cx));
    let (bg, fg, line, hover) = match kind {
        Btn::Plain => (p.raised, p.text, p.line, p.high),
        Btn::Pri => (p.primary, p.on_primary, p.primary, mix(p.primary, 0.88, gpui_kit::white())),
        Btn::Ghost => (p.raised.opacity(0.), p.text, p.line, p.high),
    };
    div().id(id).flex().flex_none().items_center().gap(u(6.)).whitespace_nowrap().cursor_pointer()
        .px(u(if sm { 9. } else { 12. })).py(u(if sm { 3. } else { 5. })).when(sm, |d| d.text_size(em(cx, 0.88)))
        .rounded(u(g.r2)).border(u(1.)).border_color(line).bg(bg).text_color(fg)
        .hover(move |s| s.bg(hover))
}

// .ia: a 24px icon button, dim until hovered
pub fn ia(id: impl Into<ElementId>, name: &str, cx: &App) -> Stateful<Div> {
    let p = pal(cx);
    let (text, hover) = (p.text, p.hover);
    div().id(id).flex().flex_none().items_center().justify_center().size(u(24.)).rounded(u(5.)).cursor_pointer().text_color(p.dim)
        .hover(move |s| s.text_color(text).bg(hover))
        .child(ic(name, cx))
}

// .link: the accent, underlined
pub fn link(id: impl Into<ElementId>, text: impl Into<SharedString>, cx: &App) -> Stateful<Div> {
    let p = pal(cx);
    div().id(id).cursor_pointer().text_color(p.accent).underline().text_decoration_color(p.accent.opacity(0.4)).child(text.into())
}

// .segs / .seg: a row of choices in a thin frame
pub fn segs(id: impl Into<SharedString>, opts: &[&str], sel: usize, on: impl Fn(usize, &mut Window, &mut App) + 'static, cx: &App) -> Div {
    let (p, g) = (pal(cx), geo(cx));
    let id: SharedString = id.into();
    let on = Rc::new(on);
    let text = p.text;
    div().flex().flex_none().border(u(1.)).border_color(p.line).rounded(u(g.r2)).p(u(2.)).gap(u(2.))
        .children(opts.iter().enumerate().map(|(i, o)| {
            let on = on.clone();
            div().id(SharedString::from(format!("{id}-{i}"))).px(u(10.)).py(u(2.)).rounded(u((g.r2 - 2.).max(0.))).cursor_pointer()
                .when(i == sel, |d| d.bg(p.high).text_color(p.text))
                .when(i != sel, |d| d.text_color(p.dim).hover(move |s| s.text_color(text)))
                .child(o.to_string())
                .on_click(move |_, w, cx| on(i, w, cx))
        }))
}

// .toggle: a 36x20 switch, the knob slides right and takes the primary when on
pub fn toggle(id: impl Into<ElementId>, on: bool, f: impl Fn(bool, &mut Window, &mut App) + 'static, cx: &App) -> Stateful<Div> {
    let p = pal(cx);
    div().id(id).relative().flex_none().w(u(36.)).h(u(20.)).rounded(u(20.)).border(u(1.)).border_color(p.line).cursor_pointer()
        .bg(if on { mix(p.primary, 0.3, p.high) } else { p.high })
        // absolute children are placed from the border box here, css places them inside the border
        .child(div().absolute().top(u(3.)).left(u(if on { 19. } else { 3. })).size(u(14.)).rounded_full().bg(if on { p.primary } else { p.dim }))
        .on_click(move |_, w, cx| f(!on, w, cx))
}

// kbd: small, raised, a line under it
pub fn kbd(text: impl Into<SharedString>, cx: &App) -> Div {
    let p = pal(cx);
    div().flex_none().text_size(em(cx, 0.78)).line_height(relative(1.)).pt(u(2.)).pb(u(2.)).px(u(5.)).rounded(u(4.))
        .text_color(p.mid).bg(p.high).border_b(u(1.)).border_color(p.line).whitespace_nowrap().child(text.into())
}

// .s-in: raised, a line round it that turns primary while it has focus
pub fn s_box(focused: bool, cx: &App) -> Div {
    let (p, g) = (pal(cx), geo(cx));
    div().w_full().min_w_0().flex().items_center().bg(p.raised).border(u(1.)).border_color(if focused { p.primary } else { p.line }).rounded(u(g.r2))
        .px(u(10.)).py(u(5.)).text_color(p.text)
}
pub fn s_in(st: &Entity<InputState>, window: &Window, cx: &App) -> Div {
    let fs = geo(cx).fs;
    s_box(st.read(cx).focus_handle(cx).is_focused(window), cx).child(Styled::h(Input::new(st).appearance(false).px_0().py_0(), u(fs * 1.55)).text_size(u(fs)))
}

// a box inside a window with its title in the top border (.s-group, .im-gen, .ag-summon and their .s-legend)
pub fn legend_box(title: impl Into<SharedString>, body: impl IntoElement, cx: &App) -> Div {
    let p = pal(cx);
    boxed(cx).child(body).child(edge(p.line, cx)).child(legend(Side::L, p.bg, cx).px(u(6.)).text_color(p.text).child(title.into()))
}

// .pane-empty: a quiet line in the middle of the space
pub fn empty(text: impl Into<SharedString>, cx: &App) -> Div {
    div().flex_1().flex().items_center().justify_center().p(u(30.)).text_color(pal(cx).dim).child(text.into())
}

// pi's numbers, the way lib/fmt.ts writes them
pub fn cost(n: f64) -> String {
    if n == 0. { "$0".into() }
    else if n < 0.01 { format!("${n:.4}") }
    else if n < 10. { format!("${n:.2}") }
    else if n < 1000. { format!("${n:.1}") }
    else { format!("${}", group_digits(n.round() as u64)) }
}

pub fn tokens(n: f64) -> String {
    if n == 0. { "0".into() }
    else if n < 1000. { format!("{}", n.round() as i64) }
    else if n < 10_000. { format!("{:.1}k", n / 1000.) }
    else if n < 1_000_000. { format!("{}k", (n / 1000.).round()) }
    else if n < 10_000_000. { format!("{:.2}M", n / 1e6) }
    else { format!("{:.1}M", n / 1e6) }
}

pub fn group_digits(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 { out.push(','); }
        out.push(c);
    }
    out
}

// lib/fmt.ts base(): the last part of a path
pub fn base(p: &str) -> String { p.trim_end_matches('/').rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or(p).to_string() }

// lib/fmt.ts ago(): now, 5m, 3h, 2d, then the date
pub fn ago(ms: f64) -> String {
    let s = ((crate::state::tab::now_ms() - ms) / 1000.).max(0.);
    if s < 45. { return "now".into(); }
    if s < 3600. { return format!("{}m", (s / 60.).round()); }
    if s < 86400. { return format!("{}h", (s / 3600.).round()); }
    if s < 86400. * 7. { return format!("{}d", (s / 86400.).round()); }
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let tm = |ms: f64| { let t = (ms / 1000.) as libc::time_t; let mut o: libc::tm = unsafe { std::mem::zeroed() }; unsafe { libc::localtime_r(&t, &mut o) }; o };
    let (d, now) = (tm(ms), tm(crate::state::tab::now_ms()));
    let m = MONTHS[d.tm_mon.clamp(0, 11) as usize];
    if d.tm_year != now.tm_year { format!("{m} {}, {:02}", d.tm_mday, (d.tm_year + 1900) % 100) } else { format!("{m} {}", d.tm_mday) }
}

// the webview's own 16px line icons (lib/icons.ts), stroked at 1.4 with round ends. the colour comes from the text
// around them like the kit's icons
const ICONS: &[(&str, &str)] = &[
    ("plus", "M8 3v10M3 8h10"),
    ("close", "M4 4l8 8M12 4l-8 8"),
    ("search", "M7 12A5 5 0 1 0 7 2a5 5 0 0 0 0 10zM10.6 10.6L14 14"),
    ("folder", "M2 4.5V12a1 1 0 0 0 1 1h10a1 1 0 0 0 1-1V6a1 1 0 0 0-1-1H8L6.5 3.5H3A1 1 0 0 0 2 4.5z"),
    ("globe", "M8 14A6 6 0 1 0 8 2a6 6 0 0 0 0 12zM2 8h12M8 2c-2 2-2 10 0 12M8 2c2 2 2 10 0 12"),
    ("frame", "M2.5 3.5h11v9h-11zM2.5 6h11M4.5 4.75h.01M6 4.75h.01"),
    ("diff", "M5 2v7M1.5 5.5h7M10 11h4.5M10.5 3l3 3-3 3"),
    ("file", "M4 1.5h5l3 3V14a.5.5 0 0 1-.5.5h-7.5A.5.5 0 0 1 3.5 14V2a.5.5 0 0 1 .5-.5zM9 1.5V5h3"),
    ("agents", "M5 7a2 2 0 1 0 0-4 2 2 0 0 0 0 4zM11.5 7a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3zM1.5 13c0-2 1.5-4 3.5-4s3.5 2 3.5 4M9.5 9.5c.5-.3 1.2-.5 2-.5 1.6 0 3 1.5 3 3.5"),
    ("image", "M2 3h12v10H2zM2 11l3.5-3.5 3 3 2-2L14 12M10.5 6.5h.01"),
    ("gear", "M8 10a2 2 0 1 0 0-4 2 2 0 0 0 0 4zM8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.4 3.4l1.4 1.4M11.2 11.2l1.4 1.4M3.4 12.6l1.4-1.4M11.2 4.8l1.4-1.4"),
    ("bars", "M2 14h12M3.5 11V8M6.5 11V4M9.5 11V6.5M12.5 11V2.5"),
    ("up", "M8 13V3M3.5 7.5L8 3l4.5 4.5"),
    ("stop", "M4.5 4.5h7v7h-7z"),
    ("clip", "M12.5 7.5l-5 5a3 3 0 0 1-4.25-4.25l5.5-5.5a2 2 0 0 1 2.8 2.8L6.2 11a1 1 0 0 1-1.4-1.4L9.5 5"),
    ("chevron", "M6 4l4 4-4 4"),
    ("down", "M4 6l4 4 4-4"),
    ("copy", "M5.5 5.5h7v8h-7zM3.5 10.5v-8h7"),
    ("branch", "M4.5 2v12M4.5 10c0-3 7-2 7-5.5M11.5 4.5V2"),
    ("refresh", "M13 8a5 5 0 1 1-1.5-3.5M13 2.5v3h-3"),
    ("external", "M9 2.5h4.5V7M13.5 2.5L7.5 8.5M11.5 9.5v3.5h-9v-9H6"),
    ("pin", "M6 2.5h4l-.5 4 2 2v1h-7v-1l2-2zM8 9.5V14"),
    ("trash", "M2.5 4h11M6 4V2.5h4V4M4 4l.7 9.5h6.6L12 4"),
    ("more", "M3.5 8h.01M8 8h.01M12.5 8h.01"),
    ("check", "M3 8.5l3 3 7-7"),
    ("back", "M10 3.5L5.5 8l4.5 4.5"),
    ("fwd", "M6 3.5l4.5 4.5L6 12.5"),
    ("pick", "M2.5 2.5l4.5 11 1.5-4.5 4.5-1.5zM9 9l4 4"),
    ("sidebar", "M2 3h12v10H2zM6 3v10"),
    ("pane", "M2 3h12v10H2zM10 3v10"),
    ("terminal", "M2.5 3.5h11v9h-11zM4.5 6.5l2 1.5-2 1.5M8 10h3"),
    ("phone", "M5 1.5h6v13H5zM7.5 12.5h1"),
    ("tablet", "M3 2h10v12H3zM7.5 12h1"),
    ("desktop", "M1.5 2.5h13v8.5h-13zM6 14h4M8 11v3"),
    ("edit", "M10.5 2.5l3 3-8 8h-3v-3z"),
    ("sparkle", "M8 2v3M8 11v3M2 8h3M11 8h3M4 4l1.5 1.5M10.5 10.5L12 12M4 12l1.5-1.5M10.5 5.5L12 4"),
    ("review", "M2.5 2.5h8l3 3v8h-11zM5 8l2 2 4-4"),
    ("list", "M5.5 4h8M5.5 8h8M5.5 12h8M2.5 4h.01M2.5 8h.01M2.5 12h.01"),
    ("window", "M3 8h10"),
    ("max", "M3.5 3.5h9v9h-9z"),
];
pub fn icon(name: &str) -> Icon {
    let d = ICONS.iter().find(|x| x.0 == name).or_else(|| ICONS.iter().find(|x| x.0 == "more")).map(|x| x.1).unwrap_or("");
    let svg = format!(r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="#fff" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="{d}"/></svg>"##);
    Icon::default().data(svg.as_bytes())
}
// .ic: 1.2em of the text around it
pub fn ic(name: &str, cx: &App) -> Icon { icon(name).size(em(cx, 1.2)) }
