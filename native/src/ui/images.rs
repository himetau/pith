// Images: everything in pith's image library (what this page and the agent's generate_image made), and a
// prompt box that makes a new one without a session. thumbnails are scaled down on a worker so a big library stays light
use super::page::{self, empty};
use super::theme::pal;
use super::tools::tilde;
use crate::core::{imagegen, sessions, sys};
use crate::state::app::{Pith, View};
use crate::state::tab::Level;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable, WindowExt};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const CARD: f32 = 200.;
const GAP: f32 = 16.;
const MAX_W: f32 = 1120.;
const THUMB: u32 = 400;

#[derive(Clone)]
struct Item { file: PathBuf, prompt: String, model: String, mtime: f64 }

pub struct ImagesPage {
    items: Vec<Item>,
    loaded: bool,
    busy: usize,
    thumbs: HashMap<(PathBuf, u64), Arc<RenderImage>>,
    prompt: Entity<TextareaState>,
    last_view: View,
    _subs: Vec<Subscription>,
}

// the sidecar .txt is the prompt, then "model: ..." after a blank line
fn split_prompt(text: &str) -> (String, String) {
    match text.rsplit_once("\n\nmodel: ") {
        Some((p, m)) => (p.trim().to_string(), m.trim().to_string()),
        None => (text.trim().to_string(), String::new()),
    }
}

// gpui wants bgra
fn thumb(path: &Path) -> Option<Arc<RenderImage>> {
    let mut buf = image::open(path).ok()?.thumbnail(THUMB, THUMB).to_rgba8();
    for px in buf.chunks_exact_mut(4) { px.swap(0, 2); }
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(buf)])))
}

impl ImagesPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let prompt = cx.new(|cx| TextareaState::new(window, cx).auto_grow(2, 8).submit_on_enter(true)
            .placeholder("Describe an image: subject, style, composition, colours, any text in it"));
        let app = Pith::global(cx);
        let subs = vec![
            cx.observe(&app, |this: &mut Self, app, cx| {
                let v = app.read(cx).view;
                if v == View::Images && this.last_view != View::Images { this.load(cx); }
                this.last_view = v;
            }),
            cx.subscribe_in(&prompt, window, |this, _, ev: &InputEvent, window, cx| if let InputEvent::PressEnter { shift: false, .. } = ev { this.generate(window, cx) }),
        ];
        let mut page = ImagesPage { items: vec![], loaded: false, busy: 0, thumbs: HashMap::new(), prompt, last_view: View::Images, _subs: subs };
        page.load(cx);
        page
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let items: Vec<Item> = cx.background_spawn(async {
                sys::list_images().into_iter().map(|v| {
                    let (prompt, model) = split_prompt(v["prompt"].as_str().unwrap_or(""));
                    Item { file: PathBuf::from(v["file"].as_str().unwrap_or("")), prompt, model, mtime: v["mtime"].as_f64().unwrap_or(0.) }
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

    fn generate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.prompt.read(cx).value().trim().to_string();
        if text.is_empty() { self.prompt.update(cx, |s, cx| s.focus(window, cx)); return; }
        self.prompt.update(cx, |s, cx| s.set_value("", window, cx));
        self.busy += 1;
        cx.notify();
        let s = sys::load_settings();
        let str_of = |k: &str| s[k].as_str().unwrap_or("").to_string();
        let (model, provider, pi, price) = (str_of("imageModel"), str_of("imageProvider"), s["piPath"].as_str().filter(|p| !p.is_empty()).unwrap_or("pi").to_string(), s["imageCost"].as_f64().unwrap_or(0.));
        let prompt = text.clone();
        cx.spawn_in(window, async move |this, cx| {
            let res = cx.background_spawn(async move {
                let out = imagegen::generate(&prompt, &[], &model, &provider, &sys::images_dir(), &pi)?;
                // not part of any pi session, so it goes in pith's own log for the usage page
                sessions::log_spend(&format!("{provider}/{model}"), out.input, out.output, price * out.files.len() as f64);
                Ok::<_, String>(out.files.len())
            }).await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy -= 1;
                match res {
                    Ok(_) => this.load(cx),
                    Err(e) => {
                        Pith::global(cx).update(cx, |a, cx| a.toast(Level::Error, format!("Couldn't generate: {e}"), cx));
                        // give the prompt back so it can be fixed and tried again
                        if this.prompt.read(cx).value().trim().is_empty() { this.prompt.update(cx, |s, cx| s.set_value(text, window, cx)); }
                    }
                }
                cx.notify();
            });
        }).detach();
    }

    fn generator(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = pal(cx);
        let s = &Pith::global(cx).read(cx).settings;
        let model = format!("{} / {}", s["imageProvider"].as_str().unwrap_or(""), s["imageModel"].as_str().unwrap_or(""));
        let app = Pith::global(cx);
        v_flex().rounded(cx.theme().radius_lg).border_1().border_color(cx.theme().border).bg(p.raised)
            .child(div().px_1().pt_1().child(Textarea::new(&self.prompt).appearance(false)))
            .child(h_flex().px_3().pb_2().pt_1().gap_2().items_center()
                .child(div().id("im-model").min_w_0().truncate().text_xs().text_color(p.dim).cursor_pointer().hover(|s| s.text_color(cx.theme().foreground)).child(model)
                    .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("Change in Settings").build(window, cx))
                    .on_click(move |_, _, cx| app.update(cx, |a, cx| a.show(View::Settings, cx))))
                .child(div().flex_1())
                .when(self.busy > 0, |d| d.child(div().text_xs().text_color(p.dim).child(if self.busy == 1 { "Generating…".to_string() } else { format!("Generating {}…", self.busy) })))
                .child(Button::new("im-gen").icon(IconName::Sparkles).label("Generate").primary().small().on_click(cx.listener(|this, _, window, cx| this.generate(window, cx)))))
    }

    fn card(&self, i: usize, it: &Item, cx: &mut Context<Self>) -> AnyElement {
        let p = pal(cx);
        let r = cx.theme().radius_lg;
        let pic: AnyElement = match self.thumbs.get(&(it.file.clone(), it.mtime as u64)) {
            Some(t) => img(t.clone()).size_full().rounded(r).object_fit(ObjectFit::Cover).into_any_element(),
            // formats the thumbnailer can't read (webp, gif) go to gpui whole
            None if self.loaded && !matches!(it.file.extension().and_then(|e| e.to_str()), Some("png" | "jpg" | "jpeg")) =>
                img(it.file.clone()).size_full().rounded(r).object_fit(ObjectFit::Cover).into_any_element(),
            None => div().into_any_element(),
        };
        let first = it.prompt.lines().next().unwrap_or("").to_string();
        let caption = if first.is_empty() { it.file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default() } else { first };
        let item = it.clone();
        v_flex().id(("im", i)).gap_2().cursor_pointer().group("card")
            .child(div().w_full().aspect_square().rounded(r).bg(cx.theme().muted).border_1().border_color(p.hairline).overflow_hidden()
                .group_hover("card", |s| s.border_color(cx.theme().border)).child(pic))
            .child(v_flex().gap_0p5().px_0p5()
                .child(div().text_sm().line_clamp(2).text_color(p.mid).group_hover("card", |s| s.text_color(cx.theme().foreground)).child(caption))
                .child(div().text_xs().text_color(p.dim).child(super::workspace::ago(it.mtime))))
            .on_click(move |_, window, cx| lightbox(item.clone(), window, cx))
            .into_any_element()
    }
}

fn lightbox(it: Item, window: &mut Window, cx: &mut App) {
    let vp = window.viewport_size();
    let w = (vp.width - px(96.)).min(px(1000.)).max(px(360.));
    let h = (vp.height - px(300.)).max(px(240.));
    window.open_dialog(cx, move |d, _, cx| {
        let p = pal(cx);
        let (f1, f2, f3, prompt) = (it.file.clone(), it.file.clone(), it.file.clone(), it.prompt.clone());
        let name = it.file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let meta = [if it.model.is_empty() { None } else { Some(it.model.clone()) }, Some(super::workspace::ago(it.mtime) + " ago"), Some(name)].into_iter().flatten().collect::<Vec<_>>().join(" · ");
        d.w(w).margin_top(px(40.))
            .child(v_flex().gap_3()
                .child(div().w_full().h(h).rounded(cx.theme().radius_lg).bg(cx.theme().muted).overflow_hidden()
                    .child(img(f1).size_full().object_fit(ObjectFit::Contain)))
                .child(div().id("lb-prompt").max_h(px(140.)).overflow_y_scroll().text_sm().text_color(cx.theme().foreground)
                    .child(if prompt.is_empty() { "No prompt saved with this one".to_string() } else { prompt.clone() }))
                .child(div().text_xs().text_color(p.dim).truncate().child(meta)))
            .footer(h_flex().w_full().gap_2()
                .child(Button::new("lb-copy").icon(IconName::Copy).label("Copy prompt").outline().small().disabled(prompt.is_empty())
                    .on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(prompt.clone()))))
                .child(Button::new("lb-reveal").icon(IconName::FolderOpen).label("Show in folder").outline().small().on_click(move |_, _, cx| cx.reveal_path(&f2)))
                .child(div().flex_1())
                .child(Button::new("lb-open").icon(IconName::ExternalLink).label("Open").ghost().small().on_click(move |_, _, cx| cx.open_with_system(&f3))))
    });
}

impl Render for ImagesPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = pal(cx);
        let dir = sys::images_dir();
        let d2 = dir.clone();
        let folder = Button::new("im-folder").icon(IconName::FolderOpen).label("Show folder").ghost().small()
            .on_click(move |_, _, cx| { let _ = std::fs::create_dir_all(&d2); cx.open_with_system(&d2); });
        let summary = if self.items.is_empty() { tilde(&dir.display().to_string()) } else { format!("{} images · {}", self.items.len(), tilde(&dir.display().to_string())) };

        // as many columns as fit the content width
        let sidebar = if Pith::global(cx).read(cx).sidebar { 264. } else { 0. };
        let avail = (f32::from(window.viewport_size().width) - sidebar - 64.).min(MAX_W);
        let cols = (((avail + GAP) / (CARD + GAP)).floor() as u16).max(2);

        let body: AnyElement = if !self.loaded {
            h_flex().py(px(80.)).justify_center().child(Spinner::new().small().color(p.dim)).into_any_element()
        } else if self.items.is_empty() && self.busy == 0 {
            empty(IconName::Image, "No images yet", "Describe one above and press Enter. Images an agent makes with generate_image land here too.", None, cx).into_any_element()
        } else {
            let pending = (0..self.busy).map(|i| v_flex().id(("im-wait", i)).gap_2()
                .child(div().w_full().aspect_square().rounded(cx.theme().radius_lg).bg(cx.theme().muted).border_1().border_color(p.hairline)
                    .flex().flex_col().items_center().justify_center().gap_2().text_xs().text_color(p.dim)
                    .child(Spinner::new().small()).child("Generating…"))
                .into_any_element()).collect::<Vec<_>>();
            let cards = self.items.clone().iter().enumerate().map(|(i, it)| self.card(i, it, cx)).collect::<Vec<_>>();
            div().grid().grid_cols(cols).gap(px(GAP)).children(pending).children(cards).into_any_element()
        };

        page::column("images", MAX_W, v_flex()
            .child(page::intro(summary, folder, cx))
            .child(div().mt_3().child(self.generator(cx)))
            .child(div().mt_8().child(body)))
    }
}
