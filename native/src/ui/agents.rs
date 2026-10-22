// The agents page
use super::theme::pal;
use gpui_kit::component::v_flex;
use gpui_kit::*;

pub struct AgentsPage;

impl AgentsPage {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self { AgentsPage }
}

impl Render for AgentsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex().size_full().items_center().justify_center().text_color(pal(cx).dim).child("Agents")
    }
}
