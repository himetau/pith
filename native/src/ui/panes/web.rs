// The web and preview panes (panes.ts Browser, Preview): the session's offscreen page (core/web.rs) shown as live
// snapshots. clicks and the wheel go to the page as DOM events, the same way the agent drives it.
// the preview's comment mode is native: the hovered element's box is drawn over the snapshot, a click quotes it
use super::{btn, empty, path_label, pb, pb_icon, pbar, url_box};
use crate::core::{browser, preview, web};
use crate::state::tab::{Tab, TabEvent};
use crate::ui::frame::spin;
use crate::ui::theme::{em, pal, u};
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::json;
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind { Browser, Preview }

actions!(pith_panes, [ExitPick]);

// the preview's widths: fill the pane, or a tablet or phone framed on stripes
const VIEWPORTS: [(&str, IconName, f32); 3] = [("fill", IconName::Monitor, 0.), ("tablet", IconName::Tablet, 820.), ("phone", IconName::Smartphone, 390.)];

// the comment picker's css selector for an element (webviews.rs PICKER)
const SEL: &str = r#"const sel = (el) => { const parts = []; for (let e = el; e && e !== document.body && parts.length < 5; e = e.parentElement) {
      if (e.id) { parts.unshift('#' + CSS.escape(e.id)); break; }
      let s = e.tagName.toLowerCase(); const cls = [...e.classList].slice(0, 2); if (cls.length) s += '.' + cls.map(CSS.escape).join('.');
      const sib = e.parentElement ? [...e.parentElement.children].filter((c) => c.tagName === e.tagName) : []; if (sib.length > 1) s += ':nth-of-type(' + (sib.indexOf(e) + 1) + ')';
      parts.unshift(s); } return parts.join(' > '); };"#;

pub struct WebPane {
    kind: Kind,
    tab: Option<Entity<Tab>>,
    tab_id: Option<String>,
    shot: Option<Arc<Image>>,
    last: u64,
    page: web::Page,
    // where the page sits in the window, for mapping clicks onto it, and the stage around it
    bounds: Rc<Cell<Bounds<Pixels>>>,
    stage: Rc<Cell<Bounds<Pixels>>>,
    size: (i32, i32),
    visible: bool,
    url: Entity<InputState>,
    url_shown: String,
    vp: HashMap<EntityId, usize>,
    pick: bool,
    // the element under the pointer in comment mode: its box in page pixels and its selector
    hover: Option<(Bounds<Pixels>, String)>,
    hovering: bool,
    focus: FocusHandle,
    _tab_sub: Option<Subscription>,
    _subs: Vec<Subscription>,
    _poll: Task<()>,
}

pub fn id_for(kind: Kind, tab: &str) -> String { match kind { Kind::Browser => browser::page_id(tab), Kind::Preview => preview::page_id(tab) } }

// what the url bar makes of what was typed: a url as is, a bare domain gets https, anything else is a search
fn resolve(s: &str) -> String {
    let s = s.trim();
    let scheme = s.split_once(':').is_some_and(|(a, _)| !a.is_empty() && a.chars().all(|c| c.is_ascii_alphabetic()));
    if scheme { return s.to_string(); }
    if s.contains(char::is_whitespace) || !s.contains('.') {
        let q: String = s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
        return format!("https://duckduckgo.com/?q={q}");
    }
    format!("https://{s}")
}

fn home(cx: &App) -> String { crate::state::app::Pith::global(cx).read(cx).settings["browserHome"].as_str().filter(|s| !s.is_empty()).unwrap_or("https://duckduckgo.com").to_string() }

// /browser <url> and the url bar: the session's page goes there, made if it isn't yet
pub fn go_to(tab: &Entity<Tab>, tab_id: &str, url: &str, cx: &mut App) {
    let u = resolve(url);
    tab.update(cx, |t, _| t.browser_url = u.clone());
    web::open(&browser::page_id(tab_id), &u, 1100, 800);
}

impl WebPane {
    pub fn new(kind: Kind, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // snapshots only while the pane is on screen; a page nobody's looking at costs nothing
        let poll = cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(350)).await;
            let Ok(Some(id)) = this.update(cx, |p, _| if p.visible { p.id() } else { None }) else {
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
        let url = cx.new(|cx| InputState::new(window, cx));
        let subs = vec![cx.subscribe_in(&url, window, |this, s, ev: &InputEvent, window, cx| match ev {
            InputEvent::PressEnter { .. } => {
                let v = s.read(cx).value().trim().to_string();
                let Some(tab) = this.tab.clone() else { return };
                if v.is_empty() { return; }
                let id = tab.read(cx).id.clone();
                go_to(&tab, &id, &v, cx);
                this.url_shown.clear();
                window.blur(cx);
                cx.notify();
            }
            InputEvent::Blur => { this.url_shown.clear(); cx.notify(); }
            _ => {}
        })];
        WebPane {
            kind, tab: None, tab_id: None, shot: None, last: 0, page: web::Page::default(), bounds: Rc::new(Cell::new(Bounds::default())), stage: Rc::new(Cell::new(Bounds::default())),
            size: (0, 0), visible: false, url, url_shown: String::new(), vp: HashMap::new(), pick: false, hover: None, hovering: false, focus: cx.focus_handle(),
            _tab_sub: None, _subs: subs, _poll: poll,
        }
    }

    pub fn show(&mut self, tab: &Entity<Tab>, cx: &mut Context<Self>) {
        if self.tab.as_ref() != Some(tab) {
            self.tab = Some(tab.clone());
            self.tab_id = Some(tab.read(cx).id.clone());
            self.shot = None;
            self.last = 0;
            self.size = (0, 0);
            self.hover = None;
            self.url_shown.clear();
            self._tab_sub = Some(cx.observe(tab, |_, _, cx| cx.notify()));
        }
        self.visible = true;
        // the first look at a session's pane makes its page: the web pane at its last page or the home page,
        // the preview at what the agent (or you) last opened
        let (id, t) = (tab.read(cx).id.clone(), tab.read(cx));
        match self.kind {
            Kind::Browser if web::page(&browser::page_id(&id)).is_none() => {
                let u = if t.browser_url.is_empty() { home(cx) } else { t.browser_url.clone() };
                web::open(&browser::page_id(&id), &u, 1100, 800);
            }
            Kind::Preview if web::page(&preview::page_id(&id)).is_none() => {
                if let Some(p) = t.preview.clone() { preview::show(&id, p["path"].as_str(), p["url"].as_str()); }
            }
            _ => {}
        }
        cx.notify();
    }
    pub fn hide(&mut self) { self.visible = false; }

    fn id(&self) -> Option<String> { self.tab_id.as_ref().map(|t| id_for(self.kind, t)) }

    // window position to page pixels, None off the page
    fn to_page(&self, pos: Point<Pixels>) -> Option<(f32, f32)> {
        let b = self.bounds.get();
        let (x, y) = (f32::from(pos.x - b.origin.x), f32::from(pos.y - b.origin.y));
        (x >= 0. && y >= 0. && x <= f32::from(b.size.width) && y <= f32::from(b.size.height)).then_some((x, y))
    }

    fn click(&mut self, pos: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(id), Some((x, y))) = (self.id(), self.to_page(pos)) else { return };
        if self.pick { self.focus.focus(window, cx); return self.pick_at(id, x, y, cx); }
        std::thread::spawn(move || {
            let _ = futures::executor::block_on(web::eval(&id, &format!(
                "const el = document.elementFromPoint({x}, {y}); if (!el) return false; if (el.focus) el.focus(); \
                 const o = {{ bubbles: true, cancelable: true, clientX: {x}, clientY: {y}, button: 0, view: window }}; \
                 el.dispatchEvent(new MouseEvent('mousedown', o)); el.dispatchEvent(new MouseEvent('mouseup', o)); el.click(); return true")));
        });
    }

    fn scroll(&mut self, dy: f32) {
        let Some(id) = self.id() else { return };
        // the hovered box moved with the page
        self.hover = None;
        std::thread::spawn(move || { let _ = futures::executor::block_on(web::eval(&id, &format!("scrollBy(0, {dy}); return true"))); });
    }

    fn navigate(&self, what: &'static str) { if let Some(id) = self.id() { web::go(&id, what); } }

    // ---------- comment mode ----------
    fn set_pick(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.pick = on;
        self.hover = None;
        if on { self.focus.focus(window, cx); }
        cx.notify();
    }

    // the element under the pointer, one look at a time
    fn hover_at(&mut self, pos: Point<Pixels>, cx: &mut Context<Self>) {
        let (Some(id), Some((x, y))) = (self.id(), self.to_page(pos)) else { if self.hover.take().is_some() { cx.notify(); } return };
        if self.hovering { return; }
        self.hovering = true;
        let js = format!("{SEL} const el = document.elementFromPoint({x}, {y}); if (!el) return null; const r = el.getBoundingClientRect(); \
            return {{ x: r.left, y: r.top, w: r.width, h: r.height, selector: sel(el) }};");
        cx.spawn(async move |this, cx| {
            let r = cx.background_spawn(async move { web::eval(&id, &js).await }).await;
            let _ = this.update(cx, |this, cx| {
                this.hovering = false;
                if !this.pick { return; }
                this.hover = r.ok().filter(|v| v.is_object()).map(|v| {
                    let f = |k: &str| px(v[k].as_f64().unwrap_or(0.) as f32);
                    (Bounds::new(point(f("x"), f("y")), size(f("w"), f("h"))), v["selector"].as_str().unwrap_or("").to_string())
                });
                cx.notify();
            });
        }).detach();
    }

    // a click in comment mode quotes the element into the composer: `<tag> "its text" · selector`
    fn pick_at(&mut self, id: String, x: f32, y: f32, cx: &mut Context<Self>) {
        let js = format!("{SEL} const el = document.elementFromPoint({x}, {y}); if (!el) return null; \
            const html = el.outerHTML.length > 700 ? el.outerHTML.slice(0, 700) + '…' : el.outerHTML; \
            return {{ selector: sel(el), html, text: (el.innerText || '').slice(0, 200) }};");
        let Some(tab) = self.tab.clone() else { return };
        cx.spawn(async move |_, cx| {
            let Ok(d) = cx.background_spawn(async move { web::eval(&id, &js).await }).await else { return };
            if !d.is_object() { return; }
            let _ = tab.update(cx, |t, cx| {
                let (selector, html) = (d["selector"].as_str().unwrap_or(""), d["html"].as_str().unwrap_or(""));
                let tag = html.strip_prefix('<').map(|h| h.chars().take_while(|c| c.is_alphanumeric()).collect::<String>()).filter(|t| !t.is_empty()).unwrap_or_else(|| "element".into());
                let words = d["text"].as_str().unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ");
                let quoted = if words.is_empty() { String::new() } else {
                    let w: String = words.chars().take(48).collect();
                    format!(" “{w}{}”", if words.chars().count() > 48 { "…" } else { "" })
                };
                let src = t.preview.as_ref().and_then(|p| p["path"].as_str()).map(crate::ui::tools::tilde).unwrap_or_else(|| "preview".into());
                cx.emit(TabEvent::Quote { label: format!("<{tag}>{quoted}  ·  {selector}"), text: format!("{src} · {selector}\n{html}") });
            });
        }).detach();
    }

    // "open html file": the preview shows a file you pick
    fn open_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("open".into()) });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let Some(p) = paths.into_iter().next() else { return };
            let _ = this.update(cx, |this, cx| this.preview_path(p.display().to_string(), cx));
        }).detach();
    }

    pub fn preview_path(&mut self, path: String, cx: &mut Context<Self>) {
        let Some(tab) = self.tab.clone() else { return };
        tab.update(cx, |t, cx| {
            t.preview = Some(json!({ "path": path }));
            preview::show(&t.id, Some(&path), None);
            cx.emit(TabEvent::Preview);
        });
        self.shot = None;
        self.last = 0;
        self.size = (0, 0);
        cx.notify();
    }

    fn bar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let p = pal(cx);
        let tip = |t: &'static str| move |w: &mut Window, cx: &mut App| Tooltip::new(t).build(w, cx);
        let tab = self.tab.clone();
        match self.kind {
            Kind::Browser => {
                let focused = self.url.read(cx).focus_handle(cx).is_focused(window);
                // the bar follows the page unless you're typing in it
                if !focused {
                    let want = if !self.page.url.is_empty() { self.page.url.clone() } else { tab.as_ref().map(|t| t.read(cx).browser_url.clone()).filter(|u| !u.is_empty()).unwrap_or_else(|| home(cx)) };
                    if want != self.url_shown { self.url_shown = want.clone(); self.url.update(cx, |s, cx| s.set_value(want, window, cx)); }
                }
                let driving = tab.as_ref().is_some_and(|t| t.read(cx).driving);
                let page = self.page.url.clone();
                pbar(cx)
                    .child(pb("wb-back", false, cx).tooltip(tip("back")).child(pb_icon(IconName::ChevronLeft, cx)).on_click(cx.listener(|this, _, _, _| this.navigate("back"))))
                    .child(pb("wb-fwd", false, cx).tooltip(tip("forward")).child(pb_icon(IconName::ChevronRight, cx)).on_click(cx.listener(|this, _, _, _| this.navigate("forward"))))
                    .child(pb("wb-reload", false, cx).tooltip(tip("reload")).child(pb_icon(IconName::RotateCw, cx)).on_click(cx.listener(|this, _, _, _| this.navigate("reload"))))
                    .child(url_box(focused, true, cx).child(Input::new(&self.url).appearance(false).h(u(24.)).px(u(10.)).text_size(em(cx, 0.9))))
                    .when(driving, |d| d.child(h_flex().flex_none().gap(u(4.)).px(u(8.)).h(u(22.)).rounded(u(11.)).bg(p.primary).text_color(p.on_primary).text_size(em(cx, 0.9 * 0.82))
                        .child(spin("wb-drive")).child("agent is driving")))
                    .child(pb("wb-ext", false, cx).tooltip(tip("open in your browser")).child(pb_icon(IconName::ExternalLink, cx))
                        .on_click(move |_, _, cx| if !page.is_empty() && !page.starts_with("about:") { cx.open_url(&page) }))
            }
            Kind::Preview => {
                let pv = tab.as_ref().and_then(|t| t.read(cx).preview.clone());
                let label = pv.as_ref().map(|p| p["title"].as_str().map(String::from).or_else(|| p["path"].as_str().map(crate::ui::tools::tilde)).or_else(|| p["url"].as_str().map(String::from)).unwrap_or_default()).unwrap_or_default();
                let vp = tab.as_ref().and_then(|t| self.vp.get(&t.entity_id()).copied()).unwrap_or(0);
                let ext = pv.as_ref().and_then(|p| p["url"].as_str().map(String::from).or_else(|| p["path"].as_str().map(|s| format!("file://{s}"))));
                pbar(cx).child(path_label(label, cx))
                    .child(h_flex().flex_none().children(VIEWPORTS.iter().enumerate().map(|(i, (name, icon, _))| {
                        let name = *name;
                        pb(SharedString::from(format!("pv-vp-{name}")), vp == i, cx).tooltip(move |w, cx| Tooltip::new(name).build(w, cx)).child(pb_icon(icon.clone(), cx))
                            .on_click(cx.listener(move |this, _, _, cx| { if let Some(t) = &this.tab { this.vp.insert(t.entity_id(), i); } this.hover = None; cx.notify(); }))
                    })))
                    .child(pb("pv-pick", self.pick, cx).tooltip(tip("click an element to comment on it")).child(pb_icon(IconName::MousePointer2, cx)).child("comment")
                        .on_click(cx.listener(|this, _, window, cx| { let on = !this.pick; this.set_pick(on, window, cx); })))
                    .child(pb("pv-reload", false, cx).tooltip(tip("reload")).child(pb_icon(IconName::RotateCw, cx)).on_click(cx.listener(|this, _, _, _| this.navigate("reload"))))
                    .child(pb("pv-open", false, cx).tooltip(tip("open html file")).child(pb_icon(IconName::Folder, cx)).on_click(cx.listener(|this, _, window, cx| this.open_file(window, cx))))
                    .child(pb("pv-ext", false, cx).tooltip(tip("open in your browser")).child(pb_icon(IconName::ExternalLink, cx))
                        .on_click(move |_, _, cx| if let Some(u) = &ext { cx.open_url(u) }))
            }
        }
    }
}

impl Render for WebPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = pal(cx);
        let bar = self.bar(window, cx);
        let Some(id) = self.id() else { return v_flex().size_full().child(bar).into_any_element() };
        let exists = web::page(&id).is_some();
        let empty_preview = self.kind == Kind::Preview && self.tab.as_ref().is_some_and(|t| t.read(cx).preview.is_none());
        // the page is laid out at its box's size, so a snapshot maps 1:1 onto it
        let vp = if self.kind == Kind::Preview { self.tab.as_ref().and_then(|t| self.vp.get(&t.entity_id()).copied()).unwrap_or(0) } else { 0 };
        let framed = vp > 0;
        let st = self.stage.get();
        let (sw, sh) = (f32::from(st.size.width), f32::from(st.size.height));
        let pad = if framed { 14. } else { 0. };
        let (pw, ph) = if framed { (VIEWPORTS[vp].2.min(sw - pad * 2.), sh - pad * 2.) } else { (sw, sh) };
        let want = (pw as i32, ph as i32);
        if exists && want.0 > 50 && want.1 > 50 && want != self.size { self.size = want; web::resize(&id, want.0, want.1); }

        let (bounds, stage) = (self.bounds.clone(), self.stage.clone());
        let driving = self.kind == Kind::Browser && self.tab.as_ref().is_some_and(|t| t.read(cx).driving);
        let shot = self.shot.clone();
        let hover = if self.pick { self.hover.clone() } else { None };
        let pink: Hsla = rgb(0xff00cc).into();
        let page = div().id("web-page").relative().overflow_hidden().w(px(pw.max(0.))).h(px(ph.max(0.))).bg(p.surface_low)
            .child(canvas(move |b, _, _| bounds.set(b), |_, _, _, _| {}).absolute().top_0().left_0().size_full())
            .when_some(shot, |d, s| d.child(img(s).absolute().top_0().left_0().object_fit(ObjectFit::None)))
            .when_some(hover, |d, (b, sel)| d
                .child(div().absolute().left(b.origin.x).top(b.origin.y).w(b.size.width).h(b.size.height).border(px(2.)).border_color(pink).bg(pink.opacity(0.08)))
                .child(div().absolute().left(b.origin.x.max(px(0.))).top(if b.origin.y > px(22.) { b.origin.y - px(22.) } else { b.origin.y + b.size.height + px(2.) })
                    .px(px(6.)).rounded(px(3.)).bg(pink).text_color(gpui_kit::white()).text_size(px(11.)).line_height(px(17.6)).whitespace_nowrap().child(sel)))
            .when(self.pick, |d| d.cursor(CursorStyle::Crosshair))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, window, cx| this.click(e.position, window, cx)))
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| if this.pick { this.hover_at(e.position, cx) }))
            .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, window, _| {
                let dy = f32::from(e.delta.pixel_delta(window.line_height()).y);
                this.scroll(-dy);
            }));
        let body: AnyElement = if empty_preview {
            // .preview .pane-empty
            empty(None, "", cx).bg(p.bg).child(v_flex().absolute().gap(u(8.)).items_center().text_center()
                .child(div().text_color(p.text).child("nothing to preview")).child(div().text_color(p.dim).child("ask for a design, use /design, or open an html file"))
                .child(btn("pv-empty-open", cx).child(pb_icon(IconName::Folder, cx)).child("open file").on_click(cx.listener(|this, _, window, cx| this.open_file(window, cx)))))
                .into_any_element()
        } else {
            div().flex_1().min_h_0().relative().overflow_hidden().bg(p.surface_low)
                .child(canvas(move |b, _, _| stage.set(b), |_, _, _, _| {}).absolute().top_0().left_0().size_full())
                // .pv-stage.framed: stripes of the window ground on the low surface
                .when(framed, |d| d.child(div().absolute().top_0().left_0().size_full().bg(pattern_slash(p.bg, 8., 8.))))
                .child(div().size_full().flex().justify_center().pt(px(pad)).child(page))
                .when(driving, |d| d.child(div().absolute().top_0().left_0().size_full().border(u(2.)).border_color(p.primary)))
                .into_any_element()
        };
        v_flex().id("web-pane").size_full().track_focus(&self.focus)
            // esc leaves comment mode; the context only exists while it's on, so esc otherwise still stops the run
            .when(self.pick, |d| d.key_context("PithPick")).on_action(cx.listener(|this, _: &ExitPick, window, cx| this.set_pick(false, window, cx)))
            .child(bar).child(body).into_any_element()
    }
}
