// The panel on the right: what the session touched (changes), the project's files, a terminal, running agents.
// one panel per window, it follows the active session
use super::theme::pal;
use crate::state::app::Pith;
use gpui_kit::component::{v_flex, ActiveTheme};
use gpui_kit::*;

pub struct SidePanel {
    _subs: Vec<Subscription>,
}

impl SidePanel {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let app = Pith::global(cx);
        SidePanel { _subs: vec![cx.observe(&app, |_, _, cx| cx.notify())] }
    }
}

impl Render for SidePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = pal(cx);
        v_flex().size_full().items_center().justify_center().text_sm().text_color(p.dim).bg(cx.theme().background).child("Changes, files and terminal go here")
    }
}
