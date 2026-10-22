// one image over everything (overlay.ts lightbox, shell.css .lb): click it or the dark around it, or esc, to close.
// a bar under it with the caption, copy and close. anything that shows an image opens it here
use super::icons::ic;
use super::overlay::{self, btn, Btn, Place};
use super::theme::{em, geo, pal, u};
use gpui_kit::*;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone)]
pub enum Src { Path(PathBuf), Image(Arc<Image>) }

pub struct Lightbox { src: Src, caption: Option<SharedString>, dims: Option<(f32, f32)>, focus: FocusHandle }

fn format_of(p: &std::path::Path) -> ImageFormat {
    match p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase().as_str() {
        "jpg" | "jpeg" => ImageFormat::Jpeg, "gif" => ImageFormat::Gif, "webp" => ImageFormat::Webp, "svg" => ImageFormat::Svg, "bmp" => ImageFormat::Bmp, _ => ImageFormat::Png,
    }
}

// the image's own size, so it can be fitted into 92vw × 84vh keeping its shape
fn dims(src: &Src) -> Option<(f32, f32)> {
    let r = match src {
        Src::Path(p) => image::ImageReader::open(p).ok()?.with_guessed_format().ok()?.into_dimensions().ok()?,
        Src::Image(i) => image::ImageReader::new(std::io::Cursor::new(i.bytes())).with_guessed_format().ok()?.into_dimensions().ok()?,
    };
    Some((r.0 as f32, r.1 as f32))
}

pub fn open(src: Src, caption: Option<String>, window: &mut Window, cx: &mut App) {
    let d = dims(&src);
    let view = cx.new(|cx| Lightbox { src, caption: caption.map(SharedString::from), dims: d, focus: cx.focus_handle() });
    let f = view.read(cx).focus.clone();
    overlay::show(view.into(), Some(f), Place::Lightbox, window, cx);
}

fn copy(src: &Src, cx: &mut App) {
    let img = match src {
        Src::Image(i) => Some((**i).clone()),
        Src::Path(p) => std::fs::read(p).ok().map(|b| Image::from_bytes(format_of(p), b)),
    };
    let Some(img) = img else { return };
    cx.write_to_clipboard(ClipboardItem::new_image(&img));
    crate::state::app::Pith::global(cx).update(cx, |a, cx| a.toast(crate::state::tab::Level::Info, "image copied", cx));
}

impl Render for Lightbox {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (p, g) = (pal(cx), geo(cx));
        let vp = window.viewport_size();
        let (mw, mh) = (f32::from(vp.width) * 0.92, f32::from(vp.height) * 0.84);
        // css px are window px times the scale, the image's own pixels aren't
        let (w, h) = match self.dims { Some((w, h)) => { let k = (mw / w).min(mh / h).min(1.); (w * k, h * k) } None => (mw, mh) };
        let el = match &self.src { Src::Path(p) => img(p.clone()), Src::Image(i) => img(i.clone()) };
        let src = self.src.clone();
        div().track_focus(&self.focus).size_full().relative().flex().items_center().justify_center()
            .on_mouse_down(MouseButton::Left, |_, window, cx| overlay::close(window, cx))
            .child(el.w(px(w)).h(px(h)).rounded(u(g.r2)).shadow(overlay::shadow(20., 60., 0.5, cx)).cursor_pointer())
            // .lb-bar: 26px off the bottom, centred
            .child(div().absolute().left_0().right_0().bottom(u(26.)).flex().justify_center()
                .child(div().flex().gap(u(8.)).items_center()
                    .when_some(self.caption.clone(), |d, c| d.child(div().text_color(p.mid).max_w(vp.width * 0.6).overflow_hidden().text_ellipsis().whitespace_nowrap().child(c)))
                    .child(btn("lb-copy", Btn::Ghost, cx).child(ic("copy").size(em(cx, 1.2))).child("copy").on_mouse_down(MouseButton::Left, move |_, _, cx| { cx.stop_propagation(); copy(&src, cx) }))
                    .child(btn("lb-close", Btn::Ghost, cx).child(ic("close").size(em(cx, 1.2))).child("close").on_mouse_down(MouseButton::Left, |_, window, cx| { cx.stop_propagation(); overlay::close(window, cx) }))))
    }
}

use gpui_kit::prelude::FluentBuilder as _;
