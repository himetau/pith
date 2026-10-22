// one image over everything, click or Esc to close (overlay.ts lightbox). anything that shows an image opens it here
use gpui_kit::*;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone)]
pub enum Src { Path(PathBuf), Image(Arc<Image>) }

// TODO(overlays): the real one: scrim at surface-lowest 88%, image max 92vw/84vh with r2 corners and a shadow,
// a bar under it with the caption, "copy image" and close
pub fn open(src: Src, caption: Option<String>, window: &mut Window, cx: &mut App) {
    use gpui_kit::component::WindowExt;
    let src2 = src.clone();
    let _ = caption;
    window.open_dialog(cx, move |d, _, _| {
        let el: AnyElement = match &src2 { Src::Path(p) => img(p.clone()).max_w(px(1200.)).into_any_element(), Src::Image(i) => img(i.clone()).max_w(px(1200.)).into_any_element() };
        d.p_0().close_button(false).child(el)
    });
}
