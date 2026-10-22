// Markdown as the transcript shows it: body-sized headings, quiet inline code, code blocks on the code surface with
// the language and a copy button in the corner
use crate::ui::theme::{is_dark, pal};
use gpui_kit::component::clipboard::Clipboard;
use gpui_kit::component::text::{TextView, TextViewStyle};
use gpui_kit::component::{h_flex, ActiveTheme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

pub fn style(cx: &App) -> TextViewStyle {
    let p = pal(cx);
    let t = cx.theme();
    // the top padding keeps the first line clear of the corner actions
    let code = StyleRefinement::default().bg(p.code_bg).border_color(p.hairline).pt(px(32.)).pb(px(12.)).px(px(14.))
        .text_size(t.mono_font_size * 0.92).line_height(relative(1.55));
    let mut s = TextViewStyle::default()
        .paragraph_gap(rems(0.8))
        .heading_font_size(|level, base| match level { 1 => base * 1.4, 2 => base * 1.22, 3 => base * 1.08, _ => base })
        .code_block(code)
        .inline_code(HighlightStyle { background_color: Some(t.muted), ..Default::default() });
    s.heading_base_font_size = t.font_size;
    s.is_dark = is_dark(cx);
    s
}

// the corner of every fenced block: its language, and copy
pub fn code_actions(block: &gpui_kit::base::text::CodeBlock, _: &mut Window, cx: &mut App) -> impl IntoElement {
    let p = pal(cx);
    // its own code background: the wrapper the text view puts it in is painted with a different token
    h_flex().gap_1().items_center().pl_2().rounded(cx.theme().radius).bg(p.code_bg).font_family(cx.theme().font_family.clone())
        .when_some(block.lang().filter(|l| !l.is_empty()), |d, l| d.child(div().text_xs().text_color(p.dim).child(l)))
        .child(Clipboard::new("copy").value(block.code()).tooltip("Copy"))
}

pub fn markdown(id: impl Into<ElementId>, text: impl Into<SharedString>, cx: &App) -> TextView {
    TextView::markdown(id, text).style(style(cx)).selectable(true).code_block_actions(code_actions)
}

// what someone typed is text, not markdown: their line breaks stay, outside code fences
pub fn typed(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut fence = false;
    for (i, l) in text.split('\n').enumerate() {
        if i > 0 { out.push_str(if fence { "\n" } else { "  \n" }); }
        if l.trim_start().starts_with("```") { fence = !fence; }
        out.push_str(l);
    }
    out
}
