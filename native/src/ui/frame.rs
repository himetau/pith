// the windows, as the webview build drew them: gaps, border width and corners are hyprland's, the focused one gets
// the active border, and titles sit in the top border, cutting it like a tui box (shell.css .win and .legend)
use super::theme::{geo, pal, u};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;

// gpui paints a box's border over its children, which would run through the legends. so a window's border is its own
// layer: add edge() after the content and before the legends. the padding keeps the content inside it, like a css border
pub fn win(cx: &App) -> Div {
    let (p, g) = (pal(cx), geo(cx));
    div().relative().min_w_0().min_h_0().flex().flex_col().bg(p.bg).p(u(g.bw)).rounded(u(g.round))
}
pub fn win_color(focused: bool, cx: &App) -> Hsla { let p = pal(cx); if focused { p.win_on } else { p.line } }

// a box inside a window with its own border legends (the composer, settings groups, ask cards). same layering as win()
pub fn boxed(cx: &App) -> Div {
    let (p, g) = (pal(cx), geo(cx));
    div().relative().bg(p.bg).p(u(g.bw)).rounded(u(g.round))
}

// the border itself, over the content and under the legends
pub fn edge(color: Hsla, cx: &App) -> Div {
    let g = geo(cx);
    div().absolute().top_0().left_0().right_0().bottom_0().border(u(g.bw)).border_color(color).rounded(u(g.round))
}

#[derive(Clone, Copy, PartialEq)]
pub enum Side { L, R }

// the strip of text sitting on the top border. `outer` is what's above the border: the gutter for windows,
// the window's own ground for boxes inside one
pub fn legend(side: Side, outer: Hsla, cx: &App) -> Div {
    let (p, g) = (pal(cx), geo(cx));
    let lh = g.fs * 0.84 * 1.5;
    // css places absolute children from inside the border; these windows draw theirs as a layer, so add it back
    let top = g.bw / 2. - lh / 2.;
    div().absolute().top(u(top)).when(side == Side::L, |d| d.left(u(12. + g.bw))).when(side == Side::R, |d| d.right(u(12. + g.bw)))
        .flex().items_center().gap(u(2.)).px(u(4.)).h(u(lh)).text_size(u(g.fs * 0.84)).line_height(u(lh))
        .whitespace_nowrap()
        // the half above the border is the outer ground, the half below the window's
        .child(div().absolute().top_0().left_0().right_0().h(relative(0.5)).bg(outer))
        .child(div().absolute().bottom_0().left_0().right_0().h(relative(0.5)).bg(p.bg))
}

// .legend-t: the window's title
pub fn legend_t(text: impl Into<SharedString>, focused: bool, cx: &App) -> Div {
    let p = pal(cx);
    div().px(u(4.)).text_color(if focused { p.text } else { p.mid }).overflow_hidden().text_ellipsis().whitespace_nowrap().child(text.into())
}
// .legend-sub: the dim bit after it
pub fn legend_sub(text: impl Into<SharedString>, cx: &App) -> Div {
    div().px(u(4.)).text_color(pal(cx).dim).min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().child(text.into())
}
// .legend-tag: a filled pill
pub fn legend_tag(text: impl Into<SharedString>, cx: &App) -> Div {
    let p = pal(cx);
    div().px(u(6.)).rounded(u(3.)).bg(p.tertiary_container).text_color(p.on_tertiary_container).text_size(u(geo(cx).fs * 0.84 * 0.9)).child(text.into())
}
// .legend-btn: dim, brighter on hover, the accent when on
pub fn legend_btn(id: impl Into<ElementId>, on: bool, cx: &App) -> Stateful<Div> {
    let p = pal(cx);
    let text = p.text;
    div().id(id).px(u(6.)).rounded(u(3.)).flex().items_center().gap(u(4.)).cursor_pointer().whitespace_nowrap()
        .text_color(if on { p.accent } else { p.dim }).when(!on, |d| d.hover(move |s| s.text_color(text)))
}

// [data-spin]: braille frames, a touch bigger and bolder than the text around them
pub fn spin(id: impl Into<ElementId>) -> impl IntoElement {
    const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    div().min_w(u(8.)).font_weight(FontWeight::BOLD)
        .with_animation(id, Animation::new(std::time::Duration::from_millis(800)).repeat(), |d, t| {
            let i = ((t * FRAMES.len() as f32) as usize).min(FRAMES.len() - 1);
            d.child(FRAMES[i])
        })
}
