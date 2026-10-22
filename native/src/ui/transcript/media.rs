// Images in the transcript: what you attached, what a tool returned. decoded once, the list redraws them every frame
use crate::ui::theme::{pal, u};
use gpui_kit::prelude::FluentBuilder as _;
use base64::Engine as _;
use gpui_kit::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

// the image and its pixel size, read from the header so the row can be laid out before the image has loaded
type Decoded = Option<(Arc<Image>, (u32, u32))>;
thread_local! {
    static DECODED: RefCell<HashMap<u64, Decoded>> = RefCell::new(HashMap::new());
}

// keyed by where the base64 lives and a sample of it: hashing a megabyte every frame would undo the cache
fn key(data: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (data.as_ptr() as usize, data.len()).hash(&mut h);
    let b = data.as_bytes();
    b[..b.len().min(256)].hash(&mut h);
    b[b.len().saturating_sub(256)..].hash(&mut h);
    h.finish()
}

pub fn image(data: &str, mime: &str) -> Decoded {
    let k = key(data);
    if let Some(i) = DECODED.with(|c| c.borrow().get(&k).cloned()) { return i; }
    let img = base64::engine::general_purpose::STANDARD.decode(data.trim()).ok().map(|bytes| {
        let dims = ::image::ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format().ok().and_then(|r| r.into_dimensions().ok()).unwrap_or((4, 3));
        (Arc::new(Image::from_bytes(ImageFormat::from_mime_type(mime).unwrap_or(ImageFormat::Png), bytes)), dims)
    });
    DECODED.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() > 200 { c.clear(); }
        c.insert(k, img.clone());
    });
    img
}

// at most `max_h` tall and `max_w` wide, never scaled up, sized from the header: the list measures the row before
// the image has loaded, so its height can't wait for it. a click opens it in the lightbox (cursor: zoom-in)
pub fn pic(id: impl Into<ElementId>, data: &str, mime: &str, max_h: f32, max_w: f32, radius: f32, border: bool, cx: &App) -> Stateful<Div> {
    let p = pal(cx);
    let id = id.into();
    let Some((i, (w0, h0))) = image(data, mime) else {
        return div().id(id).h(u(max_h.min(84.))).w(u(max_h.min(84.))).rounded(u(radius)).bg(p.surface_low);
    };
    let (w0, h0) = (w0.max(1) as f32, h0.max(1) as f32);
    let scale = (max_h / h0).min(max_w / w0).min(1.);
    let i2 = i.clone();
    div().id(id).flex_none().w(u(w0 * scale)).h(u(h0 * scale)).max_w_full().overflow_hidden().rounded(u(radius))
        .when(border, |d| d.border(u(1.)).border_color(p.line)).cursor(CursorStyle::PointingHand)
        .child(img(i).size_full().object_fit(ObjectFit::Contain))
        .on_click(move |_, window, cx| { cx.stop_propagation(); crate::ui::lightbox::open(crate::ui::lightbox::Src::Image(i2.clone()), None, window, cx); })
}

// a row of them, wrapping (.uimgs, .tool-imgs, .gen-imgs)
pub fn strip<'a>(key: &str, imgs: impl IntoIterator<Item = (&'a str, &'a str)>, max_h: f32, max_w: f32, radius: f32, border: bool, gap: f32, cx: &App) -> Div {
    div().flex().flex_wrap().gap(u(gap)).children(imgs.into_iter().enumerate().map(|(n, (d, m))| pic(SharedString::from(format!("{key}-{n}")), d, m, max_h, max_w, radius, border, cx)))
}
