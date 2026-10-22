// The images page
use super::theme::pal;
use gpui_kit::component::v_flex;
use gpui_kit::*;

pub struct ImagesPage;

impl ImagesPage {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self { ImagesPage }
}

impl Render for ImagesPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex().size_full().items_center().justify_center().text_color(pal(cx).dim).child("Images")
    }
}
