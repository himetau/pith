// Image library and a quick generator that doesn't need a session (images.ts). thumbnails are scaled down on a
// worker so a big library stays light
use super::frame::spin;
use super::lightbox::{self, Src};
use super::page::{self, ago, btn, head, ia, ic, legend_box, wide, Btn};
use super::theme::{em, geo, pal, u};
use super::tools::tilde;
use crate::core::{imagegen, sessions, sys};
use crate::state::app::{Pith, View};
use crate::state::tab::{Level, TabEvent};
use base64::Engine;
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const THUMB: u32 = 440;

#[derive(Clone)]
struct Item { file: PathBuf, prompt: String, mtime: f64 }

// a reference image going along with the prompt: mime and base64 for the request, the picture for its chip
#[derive(Clone)]
struct Ref { mime: String, data: String, pic: Arc<Image> }

pub struct ImagesPage {
    items: Vec<Item>,
    loaded: bool,
    busy: usize,
    refs: Vec<Ref>,
    thumbs: HashMap<(PathBuf, u64), Arc<RenderImage>>,
    prompt: Entity<TextareaState>,
    last_view: View,
    _subs: Vec<Subscription>,
}

// the sidecar .txt is the prompt, then "model: ..." after a blank line
fn split_prompt(text: &str) -> String {
    text.rsplit_once("\n\nmodel: ").map(|x| x.0).unwrap_or(text).trim().to_string()
}

// gpui wants bgra
fn thumb(path: &Path) -> Option<Arc<RenderImage>> {
    let mut buf = image::open(path).ok()?.thumbnail(THUMB, THUMB).to_rgba8();
    for px in buf.chunks_exact_mut(4) { px.swap(0, 2); }
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(buf)])))
}

fn ref_of(fmt: ImageFormat, bytes: Vec<u8>) -> Option<Ref> {
    if !matches!(fmt, ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif | ImageFormat::Webp) { return None; }
    Some(Ref { mime: fmt.mime_type().into(), data: base64::engine::general_purpose::STANDARD.encode(&bytes), pic: Arc::new(Image::from_bytes(fmt, bytes)) })
}
fn ref_from_file(p: &Path) -> Option<Ref> {
    let fmt = sys::mime_of(p).and_then(ImageFormat::from_mime_type)?;
    ref_of(fmt, std::fs::read(p).ok()?)
}

impl ImagesPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let prompt = cx.new(|cx| TextareaState::new(window, cx).auto_grow(2, 9).submit_on_enter(true)
            .placeholder("describe the image: subject, style, composition, colours, any text in it"));
        let app = Pith::global(cx);
        let subs = vec![
            cx.observe(&app, |this: &mut Self, app, cx| {
                let v = app.read(cx).view;
                if v == View::Images && this.last_view != View::Images { this.load(cx); }
                this.last_view = v;
            }),
            cx.subscribe_in(&prompt, window, |this, _, ev: &InputEvent, window, cx| if let InputEvent::PressEnter { shift: false, .. } = ev { this.generate(window, cx) }),
        ];
        let mut page = ImagesPage { items: vec![], loaded: false, busy: 0, refs: vec![], thumbs: HashMap::new(), prompt, last_view: View::Images, _subs: subs };
        page.load(cx);
        page
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let items: Vec<Item> = cx.background_spawn(async {
                sys::list_images().into_iter().map(|v| Item {
                    file: PathBuf::from(v["file"].as_str().unwrap_or("")), prompt: split_prompt(v["prompt"].as_str().unwrap_or("")), mtime: v["mtime"].as_f64().unwrap_or(0.),
                }).collect()
            }).await;
            let Ok(missing) = this.update(cx, |this, cx| {
                this.items = items.clone();
                this.loaded = true;
                cx.notify();
                items.iter().map(|i| (i.file.clone(), i.mtime as u64)).filter(|k| !this.thumbs.contains_key(k)).collect::<Vec<_>>()
            }) else { return };
            // a few at a time, so the first rows fill in quickly
            for chunk in missing.chunks(6) {
                let chunk = chunk.to_vec();
                let done = cx.background_spawn(async move { chunk.into_iter().filter_map(|k| Some((thumb(&k.0)?, k))).collect::<Vec<_>>() }).await;
                if this.update(cx, |this, cx| { for (t, k) in done { this.thumbs.insert(k, t); } cx.notify(); }).is_err() { return; }
            }
        }).detach();
    }

    fn add_ref(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: true, prompt: Some("add".into()) });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let refs = cx.background_spawn(async move { paths.iter().filter_map(|p| ref_from_file(p)).collect::<Vec<_>>() }).await;
            let _ = this.update(cx, |this, cx| { this.refs.extend(refs); cx.notify(); });
        }).detach();
    }

    // images pasted into the prompt become references
    fn paste(&mut self, item: &ClipboardItem, cx: &mut Context<Self>) -> bool {
        let mut used = false;
        for e in item.entries() {
            match e {
                ClipboardEntry::Image(img) => { if let Some(r) = ref_of(img.format, img.bytes.clone()) { self.refs.push(r); used = true; } }
                ClipboardEntry::ExternalPaths(p) => { for f in p.paths() { if let Some(r) = ref_from_file(f) { self.refs.push(r); used = true; } } }
                ClipboardEntry::String(_) => {}
            }
        }
        if used { cx.notify(); }
        used
    }

    fn use_in_chat(&mut self, file: PathBuf, cx: &mut Context<Self>) {
        let app = Pith::global(cx);
        let Some(t) = app.read(cx).active.clone() else { app.update(cx, |a, cx| a.toast(Level::Warn, "open a session first", cx)); return };
        t.update(cx, |_, cx| cx.emit(TabEvent::Attach(file)));
        app.update(cx, |a, cx| a.focus(t, cx));
    }

    fn use_as_ref(&mut self, file: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(r) = ref_from_file(&file) { self.refs.push(r); }
        self.prompt.update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
    }

    fn generate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.prompt.read(cx).value().trim().to_string();
        if text.is_empty() { self.prompt.update(cx, |s, cx| s.focus(window, cx)); return; }
        let refs: Vec<(String, String)> = std::mem::take(&mut self.refs).into_iter().map(|r| (r.mime, r.data)).collect();
        self.prompt.update(cx, |s, cx| s.set_value("", window, cx));
        self.busy += 1;
        cx.notify();
        let s = sys::load_settings();
        let str_of = |k: &str| s[k].as_str().unwrap_or("").to_string();
        let (model, provider, pi, price) = (str_of("imageModel"), str_of("imageProvider"), s["piPath"].as_str().filter(|p| !p.is_empty()).unwrap_or("pi").to_string(), s["imageCost"].as_f64().unwrap_or(0.));
        let prompt = text.clone();
        cx.spawn_in(window, async move |this, cx| {
            let res = cx.background_spawn(async move {
                let out = imagegen::generate(&prompt, &refs, &model, &provider, &sys::images_dir(), &pi)?;
                // not part of any pi session, so it goes in pith's own log for the usage page
                sessions::log_spend(&format!("{provider}/{model}"), out.input, out.output, price * out.files.len() as f64);
                Ok::<_, String>(out.files.len())
            }).await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy -= 1;
                if let Err(e) = res {
                    Pith::global(cx).update(cx, |a, cx| a.toast(Level::Error, e, cx));
                    // give the prompt back so it can be fixed and tried again
                    this.prompt.update(cx, |s, cx| s.set_value(text, window, cx));
                }
                this.load(cx);
                cx.notify();
            });
        }).detach();
    }

    fn generator(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = pal(cx);
        let fs = geo(cx).fs;
        let s = &Pith::global(cx).read(cx).settings;
        let model = format!("{} / {}", s["imageProvider"].as_str().unwrap_or(""), s["imageModel"].as_str().unwrap_or(""));
        let this = cx.entity().downgrade();
        let _ = window;
        let ta = Textarea::new(&self.prompt).appearance(false).p_0().text_size(u(fs)).line_height(u(fs * 1.55)).w_full()
            // the kit pads its editor inside; .im-prompt has none
            .mx(u(-10.)).mt(u(-8.)).mb(u(-2.))
            .on_paste(move |item, _, cx| this.update(cx, |t, cx| t.paste(item, cx)).unwrap_or(false));
        let refs = div().flex().flex_wrap().gap(u(8.)).when(!self.refs.is_empty(), |d| d.mt(u(10.)))
            .children(self.refs.iter().enumerate().map(|(i, r)| div().flex().items_start().gap(u(6.)).rounded(u(5.))
                .child(img(r.pic.clone()).h(u(56.)).rounded(u(5.)).border(u(1.)).border_color(p.line))
                .child(div().id(("im-ref-x", i)).flex().items_center().justify_center().size(u(18.)).rounded(u(4.)).bg(p.raised).text_color(p.dim).cursor_pointer()
                    .child(page::icon("close").size(em(cx, 1.)))
                    .on_click(cx.listener(move |this, _, _, cx| { if i < this.refs.len() { this.refs.remove(i); } cx.notify(); })))));
        let bar = div().flex().items_center().gap(u(10.)).mt(u(10.))
            .child(btn("im-ref", Btn::Ghost, true, cx).child(ic("clip", cx)).child("reference image").on_click(cx.listener(|this, _, window, cx| this.add_ref(window, cx))))
            .child(div().min_w_0().truncate().text_size(em(cx, 0.84)).text_color(p.dim).child(model))
            .child(div().flex_1())
            .child(btn("im-gen", Btn::Pri, false, cx).child(ic("sparkle", cx)).child("generate").on_click(cx.listener(|this, _, window, cx| this.generate(window, cx))));
        wide().mb(u(22.)).child(legend_box("generate", div().pt(u(18.)).px(u(14.)).pb(u(10.)).child(ta).child(refs).child(bar), cx))
    }

    fn card(&self, i: usize, it: &Item, cx: &mut Context<Self>) -> AnyElement {
        let (p, g) = (pal(cx), geo(cx));
        let pic: AnyElement = match self.thumbs.get(&(it.file.clone(), it.mtime as u64)) {
            Some(t) => img(t.clone()).size_full().object_fit(ObjectFit::Cover).into_any_element(),
            // formats the thumbnailer can't read (webp, gif) go to gpui whole
            None if !matches!(it.file.extension().and_then(|e| e.to_str()), Some("png" | "jpg" | "jpeg")) => img(it.file.clone()).size_full().object_fit(ObjectFit::Cover).into_any_element(),
            None => div().into_any_element(),
        };
        let first = it.prompt.lines().next().unwrap_or("").to_string();
        let caption = if first.is_empty() { tilde(&it.file.display().to_string()) } else { first.clone() };
        let (f1, f2, f3, f4) = (it.file.clone(), it.file.clone(), it.file.clone(), it.file.clone());
        div().flex().flex_col().border(u(1.)).border_color(p.line).rounded(u(g.round)).overflow_hidden().bg(p.surface_low)
            .child(div().id(("im-pic", i)).w_full().aspect_square().bg(p.high).overflow_hidden().cursor_pointer().child(pic)
                .on_click(move |_, window, cx| lightbox::open(Src::Path(f1.clone()), Some(first.clone()), window, cx)))
            .child(div().py(u(6.)).px(u(8.)).text_size(em(cx, 0.84))
                .child(div().text_color(p.mid).line_clamp(2).child(caption))
                .child(div().flex().items_center().gap(u(2.)).mt(u(4.))
                    .child(div().text_color(p.dim).child(ago(it.mtime)))
                    .child(div().flex_1())
                    .child(ia(("im-use", i), "clip", cx).tooltip(|w, cx| gpui_kit::component::tooltip::Tooltip::new("use in the current chat").build(w, cx))
                        .on_click(cx.listener(move |this, _, _, cx| this.use_in_chat(f2.clone(), cx))))
                    .child(ia(("im-asref", i), "edit", cx).tooltip(|w, cx| gpui_kit::component::tooltip::Tooltip::new("use as reference").build(w, cx))
                        .on_click(cx.listener(move |this, _, window, cx| this.use_as_ref(f3.clone(), window, cx))))
                    .child(ia(("im-show", i), "folder", cx).tooltip(|w, cx| gpui_kit::component::tooltip::Tooltip::new("show in folder").build(w, cx))
                        .on_click(move |_, _, cx| cx.reveal_path(&f4)))))
            .into_any_element()
    }
}

// .im-card.wait .im-ph: diagonal stripes, raised and surface-low, 10px each
fn stripes(cx: &App) -> impl IntoElement {
    let p = pal(cx);
    let (a, b) = (p.raised, p.surface_low);
    let unit = f32::from(gpui_kit::px(1.)) * geo(cx).scale;
    canvas(|_, _, _| (), move |bounds, _, window, _| {
        window.paint_quad(fill(bounds, a));
        let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
        let step = 20. * unit * std::f32::consts::SQRT_2;
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            let mut k = 0.;
            while k * step < w + h {
                let (x0, x1) = (k * step + step / 2., (k + 1.) * step);
                let mut pb = PathBuilder::fill();
                let o = bounds.origin;
                pb.move_to(o + point(px(x0), px(0.)));
                pb.line_to(o + point(px(x1), px(0.)));
                pb.line_to(o + point(px(x1 - h), px(h)));
                pb.line_to(o + point(px(x0 - h), px(h)));
                pb.close();
                if let Ok(path) = pb.build() { window.paint_path(path, b); }
                k += 1.;
            }
        });
    }).absolute().top_0().left_0().size_full()
}

impl Render for ImagesPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (p, g) = (pal(cx), geo(cx));
        let dir = sys::images_dir();
        let folder = btn("im-folder", Btn::Ghost, true, cx).child(ic("folder", cx)).child("folder")
            .on_click(move |_, _, cx| { let _ = std::fs::create_dir_all(&dir); cx.open_with_system(&dir); });

        // repeat(auto-fill, minmax(220px, 1fr)) against the page's width
        let vw = f32::from(window.viewport_size().width) / g.scale;
        let main_w = vw - 2. * g.gap - if Pith::global(cx).read(cx).sidebar { 264. + g.gap } else { 0. } - 2. * g.bw;
        let avail = (main_w - 68.).min(page::MAX_W);
        let cols = (((avail + 12.) / (220. + 12.)).floor() as u16).max(1);

        let pending = (0..self.busy).map(|i| div().flex().flex_col().border(u(1.)).border_color(p.line).rounded(u(g.round)).overflow_hidden().bg(p.surface_low)
            .child(div().relative().w_full().aspect_square().child(stripes(cx))
                .child(div().absolute().size_full().flex().items_center().justify_center().gap(u(4.)).text_color(p.dim).child(spin(("im-spin", i))).child("painting")))
            .into_any_element()).collect::<Vec<_>>();
        let cards = self.items.clone().iter().enumerate().map(|(i, it)| self.card(i, it, cx)).collect::<Vec<_>>();
        let grid = wide().grid().grid_cols(cols).gap(u(12.)).children(pending).children(cards)
            .when(self.loaded && self.items.is_empty() && self.busy == 0, |d| d.child(div().col_span_full().py(u(20.)).text_color(p.dim).child("nothing generated yet. images the agent makes with generate_image land here too.")));

        page::page("images", vec![
            head("images", None, vec![folder.into_any_element()], cx).into_any_element(),
            self.generator(window, cx).into_any_element(),
            grid.into_any_element(),
        ])
    }
}
