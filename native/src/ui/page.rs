// What the full pages (settings, usage, agents, images) share: the centred scrolling column,
// section headings, grouped rows with hairlines between them, empty states, number formatting
use super::theme::pal;
use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Icon};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;

// a centred column that scrolls as a whole
pub fn column(id: &'static str, max_w: f32, body: impl IntoElement) -> Stateful<Div> {
    div().id(id).size_full().overflow_y_scroll()
        .child(h_flex().w_full().justify_center().px_8().pt_6().pb(px(64.)).child(v_flex().w_full().max_w(px(max_w)).child(body)))
}

// a line under the window header: what's on the page, and the page's own actions on the right
pub fn intro(text: impl Into<SharedString>, actions: impl IntoElement, cx: &App) -> Div {
    h_flex().gap_2().items_center().min_h(px(32.))
        .child(div().flex_1().min_w_0().truncate().text_sm().text_color(pal(cx).dim).child(text.into()))
        .child(h_flex().flex_none().gap_1().child(actions))
}

pub fn heading(title: impl Into<SharedString>, cx: &App) -> Div {
    div().mt_8().mb_2().text_sm().font_weight(FontWeight::SEMIBOLD).text_color(cx.theme().foreground).child(title.into())
}

// rows in one hairline box, a hairline between each. the native settings look
pub fn group(rows: Vec<AnyElement>, cx: &App) -> Div {
    let p = pal(cx);
    let n = rows.len();
    v_flex().rounded(cx.theme().radius_lg).border_1().border_color(p.hairline)
        .children(rows.into_iter().enumerate().map(move |(i, r)| div().when(i + 1 < n, |d| d.border_b_1().border_color(p.hairline)).child(r)))
}

// label and help on the left, the control on the right
pub fn row(label: impl Into<SharedString>, hint: impl Into<SharedString>, ctl: impl IntoElement, cx: &App) -> AnyElement {
    let p = pal(cx);
    let hint: SharedString = hint.into();
    h_flex().gap_6().px_4().py_3().items_center().min_h(px(52.))
        .child(v_flex().flex_1().min_w_0().gap_0p5()
            .child(div().text_sm().text_color(cx.theme().foreground).child(label.into()))
            .when(!hint.is_empty(), |d| d.child(div().text_xs().text_color(p.dim).child(hint))))
        .child(h_flex().flex_none().w(px(320.)).justify_end().child(ctl))
        .into_any_element()
}

// a page or panel with nothing to show yet: say why, and what to do about it
pub fn empty(icon: IconName, title: impl Into<SharedString>, body: impl Into<SharedString>, action: Option<AnyElement>, cx: &App) -> Div {
    let p = pal(cx);
    v_flex().items_center().gap_2().py(px(56.)).px_6()
        .child(div().size(px(44.)).mb_1().rounded_full().bg(cx.theme().muted).flex().items_center().justify_center()
            .child(Icon::new(icon).size_5().text_color(p.dim)))
        .child(div().text_color(cx.theme().foreground).font_weight(FontWeight::MEDIUM).child(title.into()))
        .child(div().max_w(px(420.)).text_sm().text_center().text_color(p.dim).child(body.into()))
        .when_some(action, |d, a| d.child(div().mt_2().child(a)))
}


pub fn cost(n: f64) -> String {
    if n <= 0. { "$0".into() }
    else if n < 0.01 { format!("${n:.4}") }
    else if n < 10. { format!("${n:.2}") }
    else if n < 1000. { format!("${n:.1}") }
    else { format!("${}", group_digits(n.round() as u64)) }
}

pub fn tokens(n: f64) -> String {
    if n <= 0. { "0".into() }
    else if n < 1000. { format!("{}", n as u64) }
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

// a segmented control. the one in the kit marks its selection too faintly on these greys
pub fn segmented(id: impl Into<SharedString>, options: &[&'static str], selected: usize, on: impl Fn(usize, &mut Window, &mut App) + 'static, cx: &App) -> Div {
    let p = pal(cx);
    let id: SharedString = id.into();
    let on = std::rc::Rc::new(on);
    h_flex().p(px(2.)).gap(px(2.)).rounded(cx.theme().radius).border_1().border_color(cx.theme().border)
        .children(options.iter().enumerate().map(|(i, label)| {
            let on = on.clone();
            let sel = i == selected;
            div().id(SharedString::from(format!("{id}-{i}"))).px_3().h(px(24.)).flex().items_center().rounded(cx.theme().radius - px(3.)).text_sm().cursor_pointer()
                .when(sel, |d| d.bg(cx.theme().secondary_active).text_color(cx.theme().foreground).font_weight(FontWeight::MEDIUM))
                .when(!sel, |d| d.text_color(p.mid).hover(|s| s.text_color(cx.theme().foreground)))
                .child(*label)
                .on_click(move |_, window, cx| on(i, window, cx))
        }))
}
