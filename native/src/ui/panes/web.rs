// The Browser and Preview panes: the session's offscreen page (core/web.rs) shown as live snapshots.
// clicks and the wheel go to the page as DOM events, the same way the agent drives it
use super::super::theme::pal;
use crate::core::{browser, preview, web};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind { Browser, Preview }

pub struct WebPane {
    kind: Kind,
    tab: Option<String>,
    shot: Option<Arc<Image>>,
    last: u64,
    page: web::Page,
    // where the snapshot sits in the window, for mapping clicks onto the page
    bounds: Rc<Cell<Bounds<Pixels>>>,
    size: (i32, i32),
    visible: bool,
    _poll: Task<()>,
}

pub fn id_for(kind: Kind, tab: &str) -> String { match kind { Kind::Browser => browser::page_id(tab), Kind::Preview => preview::page_id(tab) } }

impl WebPane {
    pub fn new(kind: Kind, cx: &mut Context<Self>) -> Self {
        // snapshots only while the pane is on screen; a page nobody's looking at costs nothing
        let poll = cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(350)).await;
            let Ok(Some(id)) = this.update(cx, |p, _| if p.visible { p.tab.as_ref().map(|t| id_for(p.kind, t)) } else { None }) else {
                if this.upgrade().is_none() { break; }
                continue;
            };
            if web::page(&id).is_none() { continue; }
            let got = cx.background_spawn(async move { web::refresh(&id).await; (web::page(&id), web::snapshot(&id).await.ok()) }).await;
            let _ = this.update(cx, |p, cx| {
                if let Some(pg) = got.0 { p.page = pg; }
                if let Some(png) = got.1 {
                    use std::hash::{Hash, Hasher};
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    png.hash(&mut h);
                    let sum = h.finish();
                    if sum != p.last { p.last = sum; p.shot = Some(Arc::new(Image::from_bytes(ImageFormat::Png, png))); }
                }
                cx.notify();
            });
        });
        WebPane { kind, tab: None, shot: None, last: 0, page: web::Page::default(), bounds: Rc::new(Cell::new(Bounds::default())), size: (0, 0), visible: false, _poll: poll }
    }

    pub fn show(&mut self, tab: &str, cx: &mut Context<Self>) {
        if self.tab.as_deref() != Some(tab) { self.tab = Some(tab.to_string()); self.shot = None; self.last = 0; self.size = (0, 0); }
        self.visible = true;
        cx.notify();
    }
    pub fn hide(&mut self) { self.visible = false; }

    fn id(&self) -> Option<String> { self.tab.as_ref().map(|t| id_for(self.kind, t)) }

    fn click(&mut self, pos: Point<Pixels>) {
        let (Some(id), b) = (self.id(), self.bounds.get()) else { return };
        let (x, y) = (f32::from(pos.x - b.origin.x), f32::from(pos.y - b.origin.y));
        if x < 0. || y < 0. { return; }
        std::thread::spawn(move || {
            let _ = futures::executor::block_on(web::eval(&id, &format!(
                "const el = document.elementFromPoint({x}, {y}); if (!el) return false; if (el.focus) el.focus(); \
                 const o = {{ bubbles: true, cancelable: true, clientX: {x}, clientY: {y}, button: 0, view: window }}; \
                 el.dispatchEvent(new MouseEvent('mousedown', o)); el.dispatchEvent(new MouseEvent('mouseup', o)); el.click(); return true")));
        });
    }

    fn scroll(&mut self, dy: f32) {
        let Some(id) = self.id() else { return };
        std::thread::spawn(move || { let _ = futures::executor::block_on(web::eval(&id, &format!("scrollBy(0, {dy}); return true"))); });
    }

    fn navigate(&self, what: &'static str) { if let Some(id) = self.id() { web::go(&id, what); } }
}

impl Render for WebPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = pal(cx);
        let t = cx.theme().clone();
        let Some(id) = self.id() else { return div().into_any_element() };
        let exists = web::page(&id).is_some();
        // the page is laid out at the pane's size, so a snapshot maps 1:1 onto the pane
        let b = self.bounds.get();
        let want = (f32::from(b.size.width) as i32, f32::from(b.size.height) as i32);
        if exists && want.0 > 50 && want.1 > 50 && want != self.size { self.size = want; web::resize(&id, want.0, want.1); }
        let _ = window;
        let url = self.page.url.clone();
        let shown = if url.starts_with("file://") { super::super::tools::tilde(&url[7..]) } else { url.clone() };
        let bar = h_flex().h(px(34.)).flex_none().px_2().gap_1().items_center().border_b_1().border_color(t.border)
            .when(self.kind == Kind::Browser, |d| d
                .child(Button::new("wb-back").icon(IconName::ArrowLeft).ghost().xsmall().tooltip("Back").on_click(cx.listener(|this, _, _, _| this.navigate("back"))))
                .child(Button::new("wb-fwd").icon(IconName::ArrowRight).ghost().xsmall().tooltip("Forward").on_click(cx.listener(|this, _, _, _| this.navigate("forward")))))
            .child(Button::new("wb-reload").icon(IconName::RotateCw).ghost().xsmall().tooltip("Reload").on_click(cx.listener(|this, _, _, _| this.navigate("reload"))))
            .child(div().flex_1().min_w_0().px_2().h(px(24.)).flex().items_center().rounded(t.radius).bg(t.muted).text_xs().text_color(p.mid)
                .child(div().truncate().child(if shown.is_empty() { "about:blank".to_string() } else { shown })))
            .when(self.page.loading, |d| d.child(Spinner::new().xsmall().color(p.dim)))
            .when(!url.is_empty() && !url.starts_with("about:"), |d| d.child(Button::new("wb-ext").icon(IconName::ExternalLink).ghost().xsmall().tooltip("Open in your browser")
                .on_click(move |_, _, cx| cx.open_url(&url))));
        let bounds = self.bounds.clone();
        let body = div().id("web-body").flex_1().min_h_0().relative().overflow_hidden().bg(p.code_bg)
            .child(canvas(move |b, _, _| bounds.set(b), |_, _, _, _| {}).absolute().size_full())
            .map(|d| match (&self.shot, exists) {
                (Some(img_), _) => d.child(img(img_.clone()).absolute().top_0().left_0().object_fit(ObjectFit::None)),
                (None, true) => d.child(div().size_full().flex().items_center().justify_center().child(Spinner::new().small().color(p.dim))),
                (None, false) => d.child(div().size_full().flex().items_center().justify_center().text_sm().text_color(p.dim)
                    .child(if self.kind == Kind::Browser { "The browser opens when the agent uses it" } else { "Designs the agent previews show up here" })),
            })
            .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, _| this.click(e.position)))
            .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, window, _| {
                let dy = f32::from(e.delta.pixel_delta(window.line_height()).y);
                this.scroll(-dy);
            }));
        v_flex().size_full().border_t_1().border_color(t.border).child(bar).child(body).into_any_element()
    }
}
