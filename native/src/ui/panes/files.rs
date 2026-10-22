// The project's files: a fuzzy finder over what git tracks (or a shallow walk outside a repo) and a viewer.
// text goes through the diff view as an unchanged file, so it's virtualized, numbered, selectable and highlighted
// whenever gpui-kit's syntax features are on
use crate::core::sys;
use crate::ui::theme::pal;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::diff::{Diff, DiffFile, DiffLinePosition, DiffLineRange, DiffSide, DiffState};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Icon, Sizable};
use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

// past this the viewer shows the head of the file and says so
const MAX_TEXT: usize = 600_000;
const HITS: usize = 60;

#[derive(Clone, PartialEq)]
struct Opened { path: String, line: Option<usize> }

enum View { Empty, Loading, Text { size: u64, cut: bool }, Image(PathBuf, u64), Note(&'static str, u64) }

pub struct Files {
    input: Entity<InputState>,
    cwd: String,
    tab: Option<EntityId>,
    all: Arc<Vec<String>>,
    all_cwd: String,
    listing: bool,
    query: String,
    hits: Vec<String>,
    hit: usize,
    scroll: UniformListScrollHandle,
    open: HashMap<EntityId, Opened>,
    shown: Option<Opened>,
    view: View,
    doc: Entity<DiffState>,
    gen: u64,
    _subs: Vec<Subscription>,
}

impl Files {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Go to file"));
        let subs = vec![cx.subscribe_in(&input, window, |this, s, ev: &InputEvent, window, cx| match ev {
            InputEvent::Change => { this.query = s.read(cx).value().trim().to_string(); this.search(cx); }
            // the tree may have grown since the list was read
            InputEvent::Focus => this.load_list(true, cx),
            InputEvent::PressEnter { .. } => { if let Some(f) = this.hits.get(this.hit).cloned() { this.pick(f, window, cx); } }
            InputEvent::Blur => {}
        })];
        let doc = cx.new(|cx| DiffState::new([], cx));
        Files {
            input, cwd: String::new(), tab: None, all: Arc::new(vec![]), all_cwd: String::new(), listing: false, query: String::new(), hits: vec![], hit: 0,
            scroll: UniformListScrollHandle::new(), open: HashMap::new(), shown: None, view: View::Empty, doc, gen: 0, _subs: subs,
        }
    }

    pub fn show(&mut self, tab: EntityId, cwd: &str, cx: &mut Context<Self>) {
        let moved = self.tab != Some(tab) || self.cwd != cwd;
        self.tab = Some(tab);
        if self.cwd != cwd { self.cwd = cwd.to_string(); self.all = Arc::new(vec![]); self.all_cwd.clear(); self.search(cx); }
        if moved { self.shown = None; }
        self.load_list(false, cx);
        self.load(false, cx);
    }

    pub fn find_handle(&self, cx: &App) -> FocusHandle { self.input.read(cx).focus_handle(cx) }

    pub fn forget(&mut self, tab: EntityId) { self.open.remove(&tab); }

    // a path from elsewhere (the changes diff, the chat), relative to the session's folder or absolute
    pub fn open_path(&mut self, tab: EntityId, path: &str, line: Option<usize>, cx: &mut Context<Self>) {
        let abs = if Path::new(path).is_absolute() || self.cwd.is_empty() { path.to_string() } else { Path::new(&self.cwd).join(path).display().to_string() };
        self.open.insert(tab, Opened { path: abs, line });
        if self.tab == Some(tab) { self.load(true, cx); }
    }

    fn load_list(&mut self, force: bool, cx: &mut Context<Self>) {
        if self.cwd.is_empty() || self.listing || (!force && self.all_cwd == self.cwd) { return; }
        let cwd = self.cwd.clone();
        self.listing = true;
        cx.spawn(async move |this, cx| {
            let c2 = cwd.clone();
            let list = cx.background_spawn(async move { sys::list_files(&c2) }).await;
            let _ = this.update(cx, |this, cx| {
                this.listing = false;
                if this.cwd != cwd { return this.load_list(false, cx); }
                this.all = Arc::new(list);
                this.all_cwd = cwd;
                this.search(cx);
            });
        }).detach();
    }

    fn search(&mut self, cx: &mut Context<Self>) {
        self.hit = 0;
        if self.query.is_empty() { self.hits.clear(); cx.notify(); return; }
        let mut scored: Vec<(f32, &String)> = self.all.iter().filter_map(|f| { let s = fuzzy(&self.query, f); (s > 0.).then_some((s, f)) }).collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        self.hits = scored.into_iter().take(HITS).map(|(_, f)| f.clone()).collect();
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        cx.notify();
    }

    fn step(&mut self, d: isize, cx: &mut Context<Self>) {
        if self.hits.is_empty() { return; }
        let n = self.hits.len() as isize;
        self.hit = ((self.hit as isize + d).rem_euclid(n)) as usize;
        self.scroll.scroll_to_item(self.hit, ScrollStrategy::Nearest);
        cx.notify();
    }

    fn pick(&mut self, rel: String, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |s, cx| s.set_value("", window, cx));
        self.query.clear();
        self.hits.clear();
        if let Some(tab) = self.tab { self.open_path(tab, &rel, None, cx); }
        window.blur(cx);
        cx.notify();
    }

    fn load(&mut self, force: bool, cx: &mut Context<Self>) {
        let Some(o) = self.tab.and_then(|t| self.open.get(&t).cloned()) else {
            self.shown = None;
            self.view = View::Empty;
            cx.notify();
            return;
        };
        if !force && self.shown.as_ref() == Some(&o) { return; }
        self.shown = Some(o.clone());
        self.gen += 1;
        let gen = self.gen;
        let p = PathBuf::from(&o.path);
        if sys::mime_of(&p).is_some() {
            let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            self.view = View::Image(p, size);
            cx.notify();
            return;
        }
        self.view = View::Loading;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let path = o.path.clone();
            let (doc, view) = cx.background_spawn(async move {
                let r = sys::read_any(&path);
                let size = r["size"].as_u64().unwrap_or(0);
                match r["kind"].as_str() {
                    Some("text") => {
                        let mut text = r["text"].as_str().unwrap_or("").to_string();
                        let cut = text.len() > MAX_TEXT;
                        if cut { text.truncate(text.floor_char_boundary(MAX_TEXT)); }
                        (Some(DiffFile::unchanged(path.clone(), &text)), View::Text { size, cut })
                    }
                    Some("missing") => (None, View::Note("This file doesn't exist", 0)),
                    Some("big") => (None, View::Note("Too big to show here", size)),
                    _ => (None, View::Note("A binary file", size)),
                }
            }).await;
            let _ = this.update(cx, |this, cx| {
                if this.gen != gen { return; }
                let path = o.path.clone();
                this.doc.update(cx, |d, cx| {
                    d.set_files(doc, cx);
                    if let Some(l) = o.line {
                        let pos = DiffLinePosition::new(path.clone(), DiffSide::Modified, l);
                        d.scroll_to_line(pos, cx);
                        d.set_selected_lines(Some(DiffLineRange::new(path, DiffSide::Modified, l, l)), cx);
                    }
                });
                this.view = view;
                cx.notify();
            });
        }).detach();
    }

    fn header(&self, o: &Opened, size: u64, cx: &mut Context<Self>) -> impl IntoElement {
        let p = pal(cx);
        let t = cx.theme();
        let rel = Path::new(&o.path).strip_prefix(&self.cwd).map(|r| r.display().to_string()).unwrap_or_else(|_| crate::ui::tools::tilde(&o.path));
        let (dir, base) = match rel.rfind('/') { Some(i) => (rel[..i].to_string(), rel[i + 1..].to_string()), None => (String::new(), rel.clone()) };
        let (p1, p2, p3) = (o.path.clone(), o.path.clone(), o.path.clone());
        h_flex().h(px(34.)).flex_none().pl_3().pr_1().gap_2().items_center().border_b_1().border_color(t.border).text_sm()
            .child(div().flex_none().text_color(t.foreground).font_weight(FontWeight::MEDIUM).child(base))
            .child(div().min_w_0().flex_1().truncate().text_xs().text_color(p.dim).child(dir))
            .when(size > 0, |d| d.child(div().flex_none().text_xs().text_color(p.dim).child(bytes(size))))
            .child(Button::new("f-copy").icon(IconName::Copy).ghost().xsmall().tooltip("Copy path").on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(p1.clone()))))
            .child(Button::new("f-reveal").icon(IconName::FolderOpen).ghost().xsmall().tooltip("Show in folder").on_click(move |_, _, cx| cx.reveal_path(Path::new(&p2))))
            .child(Button::new("f-ext").icon(IconName::ExternalLink).ghost().xsmall().tooltip("Open with default app").on_click(move |_, _, cx| cx.open_with_system(Path::new(&p3))))
    }
}

impl Render for Files {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = pal(cx);
        let t = cx.theme().clone();
        let finding = !self.query.is_empty();
        let view = cx.entity();
        let count = self.hits.len();

        let results: AnyElement = if count == 0 {
            v_flex().flex_1().p_4().text_sm().text_color(p.dim).child(if self.listing { "Looking…" } else { "Nothing matches" }).into_any_element()
        } else {
            uniform_list("f-hits", count, move |range, _, cx| {
                let this = view.read(cx);
                let p = pal(cx);
                let t = cx.theme();
                range.map(|i| {
                    let f = this.hits[i].clone();
                    let sel = i == this.hit;
                    let (dir, base) = match f.rfind('/') { Some(k) => (f[..k].to_string(), f[k + 1..].to_string()), None => (String::new(), f.clone()) };
                    let v = view.clone();
                    h_flex().id(SharedString::from(format!("hit-{f}"))).w_full().h(px(28.)).px_2().gap_2().items_center().rounded(t.radius).text_sm()
                        .when(sel, |d| d.bg(t.list_active))
                        .child(Icon::new(IconName::File).xsmall().text_color(p.dim))
                        .child(div().flex_none().text_color(t.foreground).child(base))
                        .child(div().min_w_0().truncate().text_xs().text_color(p.dim).child(dir))
                        .on_mouse_move({ let v = v.clone(); move |_, _, cx| v.update(cx, |this, cx| if this.hit != i { this.hit = i; cx.notify(); }) })
                        .on_click(move |_, window, cx| { let f = f.clone(); v.update(cx, |this, cx| this.pick(f, window, cx)) })
                }).collect()
            }).track_scroll(&self.scroll).flex_1().p_1().into_any_element()
        };

        let body: AnyElement = match (&self.view, &self.shown) {
            (_, _) if finding => results,
            (View::Empty, _) | (_, None) => v_flex().flex_1().items_center().justify_center().gap_1().p_6().text_sm()
                .child(div().text_color(t.foreground).child("No file open"))
                .child(div().text_color(p.dim).text_center().child("Search above, or double click a line in Changes"))
                .into_any_element(),
            (View::Loading, _) => v_flex().flex_1().items_center().justify_center().child(Spinner::new().small().color(p.dim)).into_any_element(),
            (View::Image(path, size), Some(o)) => v_flex().flex_1().min_h_0().child(self.header(o, *size, cx))
                .child(div().id("f-img").flex_1().min_h_0().overflow_y_scroll().p_4().flex().justify_center().items_start()
                    .child(img(path.clone()).max_w_full().rounded(t.radius).object_fit(ObjectFit::ScaleDown)))
                .into_any_element(),
            (View::Note(msg, size), Some(o)) => v_flex().flex_1().min_h_0().child(self.header(o, *size, cx))
                .child(v_flex().flex_1().items_center().justify_center().text_sm().text_color(p.dim).child(*msg))
                .into_any_element(),
            (View::Text { size, cut }, Some(o)) => v_flex().flex_1().min_h_0().child(self.header(o, *size, cx))
                .when(*cut, |d| d.child(div().px_3().py_1().text_xs().text_color(p.gold).border_b_1().border_color(t.border).child("Showing the first 600 KB")))
                .child(div().flex_1().min_h_0().child(Diff::new(&self.doc).header_visible(false).size_full()))
                .into_any_element(),
        };

        v_flex().size_full()
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| match e.keystroke.key.as_str() {
                "down" if !this.hits.is_empty() => { this.step(1, cx); cx.stop_propagation(); }
                "up" if !this.hits.is_empty() => { this.step(-1, cx); cx.stop_propagation(); }
                "escape" if !this.query.is_empty() => {
                    this.input.update(cx, |s, cx| s.set_value("", window, cx));
                    this.query.clear();
                    this.hits.clear();
                    cx.notify();
                    cx.stop_propagation();
                }
                _ => {}
            }))
            .child(div().px_2().py_1p5().flex_none().border_b_1().border_color(t.border)
                .child(Input::new(&self.input).small().appearance(false).prefix(Icon::new(IconName::Search).xsmall().text_color(p.dim)).cleanable(true)))
            .child(body)
    }
}

// the composer's scorer: consecutive runs and word starts count, shorter paths win ties
pub fn fuzzy(q: &str, s: &str) -> f32 {
    if q.is_empty() { return 1.; }
    let q: Vec<char> = q.to_lowercase().chars().collect();
    let t: Vec<char> = s.to_lowercase().chars().collect();
    let (mut score, mut j, mut run) = (0f32, 0usize, 0f32);
    for i in 0..t.len() {
        if j >= q.len() { break; }
        if t[i] == q[j] {
            j += 1;
            run += 1.;
            score += 1. + run * 2. + if i == 0 || "/-_. ".contains(t[i - 1]) { 6. } else { 0. };
        } else { run = 0.; }
    }
    if j < q.len() { return 0.; }
    let whole: String = q.iter().collect();
    // the file name itself matching beats a match spread over directories
    let name = s.rsplit('/').next().unwrap_or(s).to_lowercase();
    score - t.len() as f32 * 0.02 + if s.to_lowercase().contains(&whole) { 12. } else { 0. } + if name.contains(&whole) { 10. } else { 0. }
}

pub fn bytes(n: u64) -> String {
    if n < 1024 { format!("{n} B") } else if n < 1024 * 1024 { format!("{:.1} KB", n as f64 / 1024.) } else { format!("{:.1} MB", n as f64 / 1048576.) }
}
