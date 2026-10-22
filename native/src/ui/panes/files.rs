// The project's files (panes.ts Files): a fuzzy finder over what git tracks (or a shallow walk outside a repo) and a
// viewer with a line gutter, highlighting and a gold mark on the line it was opened at
use super::code::{self, Cache};
use super::{dn_color, empty, pb, pb_icon, pbar, url_box};
use crate::core::sys;
use crate::ui::theme::{em, fade, geo, pal, u};
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

// past this the viewer shows the head of the file and says so
const MAX_TEXT: usize = 600_000;
const HITS: usize = 14;

#[derive(Clone, PartialEq)]
struct Opened { path: String, line: Option<usize> }

enum View { Empty, Loading, Text { size: u64, text: String }, Image(PathBuf, u64), Note(&'static str, u64) }

pub enum FilesEvent { Lightbox(PathBuf) }
impl EventEmitter<FilesEvent> for Files {}

pub struct Files {
    input: Entity<InputState>,
    cwd: String,
    tab: Option<EntityId>,
    all: Arc<Vec<String>>,
    all_cwd: String,
    listing: bool,
    query: String,
    // the results only show while the field has focus, like the reference's blur
    finding: bool,
    hits: Vec<String>,
    hit: usize,
    open: HashMap<EntityId, Opened>,
    shown: Option<Opened>,
    view: View,
    hl: Cache,
    version: u64,
    longest: usize,
    scroll: UniformListScrollHandle,
    gen: u64,
    _subs: Vec<Subscription>,
}

impl Files {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("find a file"));
        let subs = vec![cx.subscribe_in(&input, window, |this, s, ev: &InputEvent, window, cx| match ev {
            InputEvent::Change => { this.query = s.read(cx).value().trim().to_string(); this.search(cx); }
            // the tree may have grown since the list was read
            InputEvent::Focus => { this.finding = true; this.load_list(true, cx); cx.notify(); }
            InputEvent::PressEnter { .. } => { if let Some(f) = this.hits.get(this.hit).cloned() { this.pick(f, window, cx); } }
            InputEvent::Blur => { this.finding = false; cx.notify(); }
        })];
        Files {
            input, cwd: String::new(), tab: None, all: Arc::new(vec![]), all_cwd: String::new(), listing: false, query: String::new(), finding: false, hits: vec![], hit: 0,
            open: HashMap::new(), shown: None, view: View::Empty, hl: Cache::default(), version: 0, longest: 0, scroll: UniformListScrollHandle::new(), gen: 0, _subs: subs,
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

    pub fn forget(&mut self, tab: EntityId) { self.open.remove(&tab); }

    // a path from elsewhere (the changes diff, the chat), relative to the session's folder, ~ or absolute
    pub fn open_path(&mut self, tab: EntityId, path: &str, line: Option<usize>, cx: &mut Context<Self>) {
        let abs = if let Some(rest) = path.strip_prefix("~/") { sys::home().join(rest).display().to_string() }
            else if Path::new(path).is_absolute() || self.cwd.is_empty() { path.to_string() }
            else { Path::new(&self.cwd).join(path.trim_start_matches("./")).display().to_string() };
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
        cx.notify();
    }

    fn step(&mut self, d: isize, cx: &mut Context<Self>) {
        if self.hits.is_empty() { return; }
        let n = self.hits.len() as isize;
        self.hit = ((self.hit as isize + d).rem_euclid(n)) as usize;
        cx.notify();
    }

    fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |s, cx| s.set_value("", window, cx));
        self.query.clear();
        self.hits.clear();
        cx.notify();
    }

    fn pick(&mut self, rel: String, window: &mut Window, cx: &mut Context<Self>) {
        self.clear(window, cx);
        if let Some(tab) = self.tab { self.open_path(tab, &rel, None, cx); }
        window.blur(cx);
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
        if sys::mime_of(&p).is_some() && p.exists() {
            let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            self.view = View::Image(p, size);
            cx.notify();
            return;
        }
        self.view = View::Loading;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let path = o.path.clone();
            let view = cx.background_spawn(async move {
                let r = sys::read_any(&path);
                let size = r["size"].as_u64().unwrap_or(0);
                match r["kind"].as_str() {
                    Some("text") => {
                        let mut text = r["text"].as_str().unwrap_or("").to_string();
                        if text.len() > MAX_TEXT { text.truncate(text.floor_char_boundary(MAX_TEXT)); }
                        View::Text { size, text }
                    }
                    Some("missing") => View::Note("file doesn't exist", 0),
                    Some("big") => View::Note("too big to show", size),
                    _ => View::Note("binary file", size),
                }
            }).await;
            let _ = this.update(cx, |this, cx| {
                if this.gen != gen { return; }
                if let View::Text { text, .. } = &view {
                    this.version += 1;
                    this.longest = text.split('\n').enumerate().max_by_key(|(_, l)| l.len()).map(|(i, _)| i).unwrap_or(0);
                }
                this.view = view;
                // the mark sits a few lines down from the top, like the reference's scroll to line - 6
                if let Some(l) = o.line { this.scroll.scroll_to_item_strict(l.saturating_sub(6), ScrollStrategy::Top); }
                else { this.scroll.scroll_to_item(0, ScrollStrategy::Top); }
                cx.notify();
            });
        }).detach();
    }

    fn head(&self, o: &Opened, size: u64, cx: &mut Context<Self>) -> impl IntoElement {
        let p = pal(cx);
        let (p1, p2, p3) = (o.path.clone(), o.path.clone(), o.path.clone());
        let tip = |t: &'static str| move |w: &mut Window, cx: &mut App| Tooltip::new(t).build(w, cx);
        h_flex().flex_none().min_w_0().items_center().gap(u(4.)).pt(u(4.)).pb(u(4.)).pl(u(12.)).pr(u(8.)).border_b(u(1.)).border_color(p.rule).text_size(em(cx, 0.88))
            .child(div().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().text_color(p.mid).child(crate::ui::tools::tilde(&o.path)))
            .child(div().flex_none().text_color(p.dim).child(format!("\u{a0}{}", bytes(size))))
            .child(div().flex_1())
            .child(pb("f-copy", false, cx).tooltip(tip("copy path")).child(pb_icon(IconName::Copy, cx)).on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(p1.clone()))))
            .child(pb("f-reveal", false, cx).tooltip(tip("show in folder")).child(pb_icon(IconName::Folder, cx)).on_click(move |_, _, cx| cx.reveal_path(Path::new(&p2))))
            .child(pb("f-ext", false, cx).tooltip(tip("open with default app")).child(pb_icon(IconName::ExternalLink, cx)).on_click(move |_, _, cx| cx.open_with_system(Path::new(&p3))))
    }

    fn code(&mut self, text: &str, line: Option<usize>, cx: &mut Context<Self>) -> AnyElement {
        let (p, g) = (pal(cx), geo(cx));
        let path = self.shown.as_ref().map(|o| o.path.clone()).unwrap_or_default();
        let lines = self.hl.lines(self.version, text, code::lang_of(&path), cx);
        let n = lines.len();
        let digits = n.to_string().len().max(1);
        let fs = g.fs * 0.88;
        let (gold, dn) = (p.gold, dn_color(cx));
        uniform_list("f-code", n, move |range, _, _| {
            range.map(|i| {
                let on = line == Some(i + 1);
                h_flex().id(i).whitespace_nowrap().pr(u(12.)).text_size(u(fs)).line_height(u(fs * 1.6))
                    .when(on, |d| d.bg(fade(gold, 0.16)).border_l(u(2.)).border_color(gold))
                    .child(div().w(u(fs * 0.6 * digits as f32 + 14.)).flex_none().flex().justify_end().pr(u(14.)).text_color(dn).when(on, |d| d.ml(u(-2.))).child((i + 1).to_string()))
                    .child(code::styled(&lines[i]))
            }).collect()
        }).track_scroll(&self.scroll).with_width_from_item(Some(self.longest)).with_horizontal_sizing_behavior(ListHorizontalSizingBehavior::Unconstrained)
            .flex_1().min_h_0().pt(u(12.)).pb(u(40.)).text_size(u(fs)).line_height(relative(1.6)).font_family(g.mono.clone())
            .into_any_element()
    }
}

impl Render for Files {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = pal(cx);
        let focused = self.input.read(cx).focus_handle(cx).is_focused(window);
        let results = (self.finding && !self.hits.is_empty()).then(|| {
            let high = p.high;
            div().id("f-results").flex_none().max_h(relative(0.5)).overflow_y_scroll().p(u(4.)).border_b(u(1.)).border_color(p.rule)
                .children(self.hits.iter().enumerate().map(|(i, f)| {
                    let f2 = f.clone();
                    div().id(SharedString::from(format!("hit-{f}"))).py(u(2.)).px(u(10.)).rounded(u(4.)).cursor_pointer().text_size(em(cx, 0.9))
                        .overflow_hidden().text_ellipsis().whitespace_nowrap().when(i == self.hit, |d| d.bg(high)).hover(move |s| s.bg(high))
                        // mouse down, before the field's blur takes the results away
                        .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, window, cx| { cx.stop_propagation(); this.pick(f2.clone(), window, cx); }))
                        .child(f.clone())
                }))
        });

        let shown = self.shown.clone();
        let body: AnyElement = match (&self.view, &shown) {
            (View::Empty, _) | (_, None) => empty(Some("no file open"), "click a path in the chat, or find one above", cx).into_any_element(),
            (View::Loading, _) => div().flex_1().into_any_element(),
            (View::Image(path, size), Some(o)) => {
                let p2 = path.clone();
                v_flex().flex_1().min_h_0().child(self.head(o, *size, cx))
                    .child(div().id("f-img").flex_1().min_h_0().overflow_y_scroll().p(u(14.)).flex().justify_center().items_start()
                        .child(img(path.clone()).max_w_full().rounded(u(5.)).object_fit(ObjectFit::ScaleDown).cursor(CursorStyle::PointingHand).id("f-img-i")
                            .on_click(cx.listener(move |_, _, _, cx| cx.emit(FilesEvent::Lightbox(p2.clone()))))))
                    .into_any_element()
            }
            (View::Note(msg, size), Some(o)) => v_flex().flex_1().min_h_0().child(self.head(o, *size, cx)).child(empty(None, *msg, cx)).into_any_element(),
            (View::Text { size, text }, Some(o)) => {
                let (text, size) = (text.clone(), *size);
                v_flex().flex_1().min_h_0().child(self.head(o, size, cx)).child(self.code(&text, o.line, cx)).into_any_element()
            }
        };

        v_flex().size_full()
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| match e.keystroke.key.as_str() {
                "down" if !this.hits.is_empty() => { this.step(1, cx); cx.stop_propagation(); }
                "up" if !this.hits.is_empty() => { this.step(-1, cx); cx.stop_propagation(); }
                "escape" if this.finding => { this.clear(window, cx); window.blur(cx); cx.stop_propagation(); }
                _ => {}
            }))
            .child(pbar(cx).text_color(p.dim).child(pb_icon(IconName::Search, cx))
                .child(url_box(focused, false, cx).child(Input::new(&self.input).appearance(false).h(u(24.)).px(u(10.)).text_size(em(cx, 0.9)))))
            .children(results)
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

// fmt.ts bytes: 147B, 1.2KB
pub fn bytes(n: u64) -> String {
    if n < 1024 { format!("{n}B") } else if n < 1024 * 1024 { format!("{:.1}KB", n as f64 / 1024.) } else { format!("{:.1}MB", n as f64 / 1048576.) }
}
