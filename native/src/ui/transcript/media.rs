// Images in the transcript: what you attached, what a tool returned. decoded once, the list redraws them every frame
use crate::ui::theme::pal;
use base64::Engine as _;
use gpui_kit::component::{h_flex, ActiveTheme};
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

// at most `h` tall and a few times as wide, never scaled up, sized from the header: the list measures the row before the image
// has loaded, so its height can't wait for it
pub fn thumb(data: &str, mime: &str, h: Pixels, cx: &App) -> AnyElement {
    let p = pal(cx);
    let Some((i, (w0, h0))) = image(data, mime) else {
        return div().h(h).w(h).rounded(cx.theme().radius).bg(p.code_bg).into_any_element();
    };
    let max_w = h * 2.4;
    let (w0, h0) = (w0.max(1) as f32, h0.max(1) as f32);
    let scale = (h.as_f32() / h0).min(max_w.as_f32() / w0).min(1.);
    div().flex_none().w(px(w0 * scale)).h(px(h0 * scale)).max_w_full().overflow_hidden().rounded(cx.theme().radius).border_1().border_color(p.hairline).bg(p.code_bg)
        .child(img(i).size_full().object_fit(ObjectFit::Contain))
        .into_any_element()
}

pub fn strip<'a>(imgs: impl IntoIterator<Item = (&'a str, &'a str)>, h: Pixels, cx: &App) -> Div {
    h_flex().flex_wrap().gap_2().children(imgs.into_iter().map(|(d, m)| thumb(d, m, h, cx)))
}
