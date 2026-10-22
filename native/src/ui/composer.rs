// The input box (composer.ts): a box with its own border legends like a tui, the mode on the left and model · effort on
// the right. enter sends, shift+enter is a new line; while pi works enter steers and alt+enter queues. / offers
// commands, @ the project's files, a leading ! runs a shell command. images and files come in through the clip, a
// paste or a drop, quotes from elsewhere in the app (preview comments, the terminal, findings) as chips
use super::asks::{AskKey, Asks};
use super::commands;
use super::frame::{boxed, edge, legend, Side};
use super::theme::{geo, mix, pal, u};
use crate::state::app::Pith;
use crate::state::tab::{How, Img, Level, Mode, Tab, TabEvent};
use base64::Engine as _;
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Enter, Escape, IndentInline, InputEvent, MoveDown, MoveUp, OutdentInline, Textarea, TextareaState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex, Icon, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

actions!(composer, [QueueFollowUp]);

pub enum ComposerEvent {
    Submit { text: String, images: Vec<Img>, how: How },
    Stop,
}
impl EventEmitter<ComposerEvent> for Composer {}

struct Attached { img: Img, thumb: Arc<Image>, aspect: f32, id: u64 }
struct Quote { label: Option<String>, text: String, id: u64 }

#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuKind { Slash, At }

struct MenuRow { label: SharedString, hint: SharedString, src: Option<SharedString>, insert: String }

// the completion list over the box: commands after a leading /, files after @
struct Menu { kind: MenuKind, rows: Vec<MenuRow>, sel: usize, start: usize }

// composer.ts caps both lists at 12
const MENU_CAP: usize = 12;

pub struct Composer {
    pub tab: Option<Entity<Tab>>,
    input: Entity<TextareaState>,
    attached: Vec<Attached>,
    quotes: Vec<Quote>,
    menu: Option<Menu>,
    menu_scroll: UniformListScrollHandle,
    // what was sent from this box, for ↑ on an empty box, and how far back ↑ has gone
    hist: Vec<String>,
    hist_at: Option<usize>,
    // home's mode for the session it's about to start, cycled there without touching the default in settings
    home_mode: Mode,
    // the dock's question cards, which take digits, y/n, enter, esc and ↑↓ while the box is empty
    pub asks: Option<Entity<Asks>>,
    _subs: Vec<Subscription>,
}

thread_local! { static LAST: std::cell::RefCell<Option<WeakEntity<Composer>>> = const { std::cell::RefCell::new(None) }; }

// the composer that last had focus, for things elsewhere that want to put something in it
pub fn last_focused() -> Option<Entity<Composer>> { LAST.with(|l| l.borrow().as_ref().and_then(|w| w.upgrade())) }

fn bind_once(cx: &mut App) {
    static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if DONE.swap(true, std::sync::atomic::Ordering::SeqCst) { return; }
    cx.bind_keys([KeyBinding::new("alt-enter", QueueFollowUp, Some("Composer"))]);
}

static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
fn next_id() -> u64 { NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst) }

impl Composer {
    pub fn new(tab: Option<Entity<Tab>>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        bind_once(cx);
        let home = tab.is_none();
        // home's box starts three lines tall (pages.css .home .ta min-height 4.6em); max-height 320px either way
        let input = cx.new(|cx| TextareaState::new(window, cx).auto_grow(if home { 3 } else { 1 }, 14).submit_on_enter(true)
            .placeholder(if home { "what are we doing  ·  / commands  ·  @ files" } else { "what should we do  ·  / commands  ·  @ files  ·  ! shell" }));
        let mut subs = vec![cx.subscribe_in(&input, window, |this, _, ev: &InputEvent, window, cx| match ev {
            InputEvent::PressEnter { secondary, shift } => {
                if *shift { return; }
                this.submit(if *secondary { How::FollowUp } else { How::Auto }, window, cx);
            }
            InputEvent::Change => { this.update_menu(cx); cx.notify(); }
            InputEvent::Blur => { if this.menu.take().is_some() { cx.notify(); } }
            _ => {}
        })];
        if let Some(t) = &tab {
            subs.push(cx.observe(t, |_, _, cx| cx.notify()));
            subs.push(cx.subscribe_in(t, window, |this, _, ev: &TabEvent, window, cx| match ev {
                TabEvent::Quote { label, text } => this.add_quote(text.clone(), Some(label.clone()).filter(|l| !l.is_empty()), window, cx),
                TabEvent::Attach(p) => this.add_paths(std::slice::from_ref(p), window, cx),
                _ => {}
            }));
        }
        subs.push(cx.observe(&Pith::global(cx), |_, _, cx| cx.notify()));
        let home_mode = Pith::global(cx).read(cx).mode_default();
        Composer { tab, input, attached: vec![], quotes: vec![], menu: None, menu_scroll: UniformListScrollHandle::new(), hist: vec![], hist_at: None, home_mode, asks: None, _subs: subs }
    }

    pub fn focus_handle(&self, cx: &App) -> FocusHandle { self.input.read(cx).focus_handle(cx) }

    pub fn set_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |s, cx| {
            s.set_value(text.to_string(), window, cx);
            s.set_selected_range(text.len()..text.len(), cx);
        });
        self.menu = None;
        cx.notify();
    }

    pub fn text(&self, cx: &App) -> String { self.input.read(cx).value().to_string() }

    // the mode a session started from home gets
    pub fn pending_mode(&self) -> Mode { self.home_mode }

    // attaches an image from elsewhere in the app (the images page)
    #[allow(dead_code)]
    pub fn add_image(&mut self, img: Img, cx: &mut Context<Self>) {
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(&img.data) else { return };
        let Some(fmt) = ImageFormat::from_mime_type(&img.mime) else { return };
        self.attach(fmt, bytes, cx);
    }

    // a quote chip: goes out as > lines before the text
    pub fn add_quote(&mut self, text: String, label: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.quotes.push(Quote { label, text, id: next_id() });
        self.focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    fn cwd(&self, cx: &App) -> String {
        match &self.tab { Some(t) => t.read(cx).cwd.clone(), None => Pith::global(cx).read(cx).home_cwd.clone() }
    }

    fn mode(&self, cx: &App) -> Mode { self.tab.as_ref().map(|t| t.read(cx).mode).unwrap_or(self.home_mode) }

    fn cycle_mode(&mut self, cx: &mut Context<Self>) {
        let next = |m: Mode| Mode::ALL[(Mode::ALL.iter().position(|x| *x == m).unwrap_or(0) + 1) % 4];
        match &self.tab {
            Some(t) => t.update(cx, |t, cx| { let n = next(t.mode); t.set_mode(n, cx) }),
            None => self.home_mode = next(self.home_mode),
        }
        cx.notify();
    }

    fn submit(&mut self, how: How, window: &mut Window, cx: &mut Context<Self>) {
        let raw = self.text(cx).trim().to_string();
        if raw.is_empty() && self.attached.is_empty() && self.quotes.is_empty() { return; }
        let mut text = raw.clone();
        if !self.quotes.is_empty() {
            let q = self.quotes.iter().map(|q| q.text.split('\n').map(|l| format!("> {l}")).collect::<Vec<_>>().join("\n")).collect::<Vec<_>>().join("\n\n");
            text = if raw.is_empty() { q } else { format!("{q}\n\n{raw}") };
        }
        if !raw.is_empty() { self.hist.push(raw); }
        self.hist_at = None;
        let images: Vec<Img> = self.attached.drain(..).map(|a| a.img).collect();
        self.quotes.clear();
        self.clear(window, cx);
        // pith's own commands. with no session only the pages open; anything else starts one first (home.ts go)
        let no_tab_ok = text.strip_prefix('/').and_then(|r| r.split_whitespace().next()).is_some_and(|n| n == "usage" || n == "settings");
        if commands::is_builtin(&text) && (self.tab.is_some() || no_tab_ok) {
            let tab = self.tab.clone();
            // after this update, a command may well touch this composer
            window.defer(cx, move |window, cx| { commands::run_builtin(tab, &text, window, cx); });
            return;
        }
        cx.emit(ComposerEvent::Submit { text, images, how });
    }

    fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |s, cx| s.set_value("", window, cx));
        self.menu = None;
        cx.notify();
    }

    // ---------- history: ↑ on an empty box brings back what was sent, ↓ at the end walks forward again ----------
    fn hist_up(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.text(cx).is_empty() || self.hist.is_empty() { return false; }
        let at = self.hist_at.map(|a| (a + 1).min(self.hist.len() - 1)).unwrap_or(0);
        self.hist_at = Some(at);
        let t = self.hist[self.hist.len() - 1 - at].clone();
        self.set_text(&t, window, cx);
        true
    }

    fn hist_down(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(at) = self.hist_at else { return false };
        let (len, cur) = { let s = self.input.read(cx); (s.value().len(), s.cursor()) };
        if cur != len { return false; }
        self.hist_at = at.checked_sub(1);
        let t = match self.hist_at { Some(a) => self.hist[self.hist.len() - 1 - a].clone(), None => String::new() };
        self.set_text(&t, window, cx);
        true
    }

    // the top question card takes keys while the box is empty
    fn ask_key(&mut self, k: AskKey, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.text(cx).is_empty() { return false; }
        let Some(a) = self.asks.clone() else { return false };
        a.update(cx, |a, cx| a.key(k, window, cx))
    }

    // ---------- completion ----------
    fn update_menu(&mut self, cx: &mut Context<Self>) {
        let (value, cursor) = { let s = self.input.read(cx); (s.value().to_string(), s.cursor()) };
        let before = &value[..cursor.min(value.len())];
        let rows = if let Some(q) = before.strip_prefix('/').filter(|q| !q.contains(char::is_whitespace)) {
            let tab = self.tab.as_ref().map(|t| t.read(cx));
            let mut cmds: Vec<(commands::Cmd, f32)> = commands::commands_for(tab).into_iter().map(|c| { let s = fuzzy(q, &c.name); (c, s) }).filter(|x| x.1 > 0.).collect();
            cmds.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            Some((MenuKind::Slash, 0, cmds.into_iter().take(MENU_CAP).map(|(c, _)| MenuRow {
                label: format!("/{}", c.name).into(),
                hint: format!("{}{}", c.desc, c.arg.as_ref().map(|a| format!("  {a}")).unwrap_or_default()).into(),
                src: c.tag().map(SharedString::from),
                insert: format!("/{} ", c.name),
            }).collect::<Vec<_>>()))
        } else if let Some(at) = mention_start(before) {
            let q = &before[at + 1..];
            let cwd = self.cwd(cx);
            // still listing: the menu opens when it's done
            self.files(&cwd, cx).map(|files| (MenuKind::At, at, file_rows(q, &files)))
        } else { None };
        self.menu = rows.filter(|r| !r.2.is_empty()).map(|(kind, start, rows)| Menu { kind, rows, sel: 0, start });
        self.menu_scroll.scroll_to_item(0, ScrollStrategy::Top);
    }

    fn files(&mut self, cwd: &str, cx: &mut Context<Self>) -> Option<Arc<Vec<String>>> {
        static CACHE: Mutex<Option<HashMap<String, (Instant, Arc<Vec<String>>, bool)>>> = Mutex::new(None);
        let mut g = CACHE.lock().unwrap();
        let map = g.get_or_insert_with(HashMap::new);
        let entry = map.get(cwd).cloned();
        let stale = entry.as_ref().map_or(true, |e| e.0.elapsed() > Duration::from_secs(30));
        let loading = entry.as_ref().is_some_and(|e| e.2);
        if stale && !loading {
            let old = entry.as_ref().map(|e| (e.0, e.1.clone())).unwrap_or((Instant::now(), Arc::new(vec![])));
            map.insert(cwd.to_string(), (old.0, old.1, true));
            let dir = cwd.to_string();
            cx.spawn(async move |this, cx| {
                let d2 = dir.clone();
                let list = cx.background_spawn(async move { crate::core::sys::list_files(&d2) }).await;
                CACHE.lock().unwrap().get_or_insert_with(HashMap::new).insert(dir, (Instant::now(), Arc::new(list), false));
                let _ = this.update(cx, |c, cx| { c.update_menu(cx); cx.notify(); });
            }).detach();
        }
        // a stale list still answers right away, the fresh one replaces it when it lands
        entry.filter(|e| !e.1.is_empty() || !e.2).map(|e| e.1)
    }

    fn menu_step(&mut self, d: isize, cx: &mut Context<Self>) {
        let Some(m) = &mut self.menu else { return };
        let n = m.rows.len() as isize;
        m.sel = ((m.sel as isize + d).rem_euclid(n)) as usize;
        self.menu_scroll.scroll_to_item(m.sel, ScrollStrategy::Nearest);
        cx.notify();
    }

    fn accept(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(m) = self.menu.take() else { return };
        let Some(row) = m.rows.get(m.sel) else { return };
        let (value, cursor) = { let s = self.input.read(cx); (s.value().to_string(), s.cursor()) };
        // the word under the caret goes, plus one space after it, so accepting mid-word doesn't leave a tail
        let (start, end) = if m.kind == MenuKind::Slash {
            let tail = value[cursor..].find(char::is_whitespace).unwrap_or(value.len() - cursor);
            let mut end = cursor + tail;
            if value[end..].starts_with(' ') { end += 1; }
            (0, end)
        } else { (m.start, cursor) };
        let insert = row.insert.clone();
        self.input.update(cx, |s, cx| {
            s.set_selected_range(start..end, cx);
            s.replace(insert, window, cx);
        });
        cx.notify();
    }

    // ---------- images, files and paths ----------
    fn attach(&mut self, fmt: ImageFormat, bytes: Vec<u8>, cx: &mut Context<Self>) {
        // what the models take
        if !matches!(fmt, ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif | ImageFormat::Webp) {
            return toast(Level::Warn, "only png, jpeg, gif and webp images can be attached", cx);
        }
        if bytes.len() > 20 * 1024 * 1024 { return toast(Level::Warn, "that image is over 20 MB", cx); }
        // the chip is 56px tall at the image's own width (.chip.img img { height: 56px })
        let aspect = image::ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format().ok().and_then(|r| r.into_dimensions().ok())
            .filter(|(w, h)| *w > 0 && *h > 0).map(|(w, h)| w as f32 / h as f32).unwrap_or(1.).clamp(0.25, 4.);
        let img = Img { data: base64::engine::general_purpose::STANDARD.encode(&bytes), mime: fmt.mime_type().into() };
        self.attached.push(Attached { img, thumb: Arc::new(Image::from_bytes(fmt, bytes)), aspect, id: next_id() });
        cx.notify();
    }

    // picked, dropped or pasted files: images attach, anything else goes in as an @mention
    pub fn add_paths(&mut self, paths: &[PathBuf], window: &mut Window, cx: &mut Context<Self>) {
        let cwd = self.cwd(cx);
        let mut mention = String::new();
        for p in paths {
            let fmt = crate::core::sys::mime_of(p).and_then(ImageFormat::from_mime_type).filter(|f| !matches!(f, ImageFormat::Svg));
            match fmt.map(|f| (f, std::fs::read(p))) {
                Some((f, Ok(bytes))) => self.attach(f, bytes, cx),
                _ => {
                    let rel = p.strip_prefix(&cwd).ok().filter(|r| !r.as_os_str().is_empty()).unwrap_or(p);
                    let s = rel.display().to_string();
                    mention.push_str(&format!("@{} ", if s.contains(' ') { format!("\"{s}\"") } else { s }));
                }
            }
        }
        if !mention.is_empty() { self.input.update(cx, |s, cx| s.insert(mention, window, cx)); }
        self.focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    // the clip button: a file picker
    fn pick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: true, prompt: Some("attach".into()) });
        let w = window.window_handle();
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let _ = w.update(cx, |_, window, cx| { let _ = this.update(cx, |c, cx| c.add_paths(&paths, window, cx)); });
        }).detach();
    }

    // true when the paste was something to attach rather than text to type
    pub fn paste(&mut self, item: &ClipboardItem, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let mut used = false;
        for e in item.entries() {
            match e {
                ClipboardEntry::Image(img) => { self.attach(img.format, img.bytes.clone(), cx); used = true; }
                ClipboardEntry::ExternalPaths(p) => { self.add_paths(p.paths(), window, cx); used = true; }
                ClipboardEntry::String(_) => {}
            }
        }
        if used { return true; }
        // a file manager's copy often arrives as plain text: one path (or file:// uri) per line, all of them images
        let Some(text) = item.text() else { return false };
        let paths: Vec<PathBuf> = text.lines().map(str::trim).filter(|l| !l.is_empty())
            .map(|l| PathBuf::from(l.strip_prefix("file://").map(percent_decode).unwrap_or_else(|| l.to_string()))).collect();
        let images = !paths.is_empty() && paths.len() <= 20 && paths.iter().all(|p| p.is_absolute() && p.is_file() && crate::core::sys::mime_of(p).is_some_and(|m| m != "image/svg+xml"));
        if images { self.add_paths(&paths, window, cx); }
        images
    }
}

// what a message typed into a session does: !cmd runs a shell command (!!cmd keeps it out of context), pith's own
// /commands run here, anything else goes to pi
pub fn dispatch(tab: Entity<Tab>, text: String, images: Vec<Img>, how: How, window: &mut Window, cx: &mut App) {
    if let Some(cmd) = text.strip_prefix('!').filter(|_| images.is_empty()) {
        let (exclude, cmd) = match cmd.strip_prefix('!') { Some(c) => (true, c), None => (false, cmd) };
        let cmd = cmd.trim().to_string();
        if !cmd.is_empty() { tab.update(cx, |t, cx| t.bash(cmd, exclude, cx)); }
        return;
    }
    if commands::is_builtin(&text) { commands::run_builtin(Some(tab), &text, window, cx); return; }
    tab.update(cx, |t, cx| t.send(text, images, how, cx));
}

pub fn sentence(s: &str) -> String { let mut c = s.chars(); c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default() }

fn toast(level: Level, text: &str, cx: &mut App) {
    let t = text.to_string();
    Pith::global(cx).update(cx, |a, cx| a.toast(level, t, cx));
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) { out.push(v); i += 3; continue; }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// composer.ts's fuzzy: subsequence, runs and hits after a separator score, a straight substring scores more
pub fn fuzzy(q: &str, s: &str) -> f32 {
    if q.is_empty() { return 1.; }
    let (q, t): (Vec<char>, Vec<char>) = (q.to_lowercase().chars().collect(), s.to_lowercase().chars().collect());
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
    let (ql, tl): (String, String) = (q.iter().collect(), t.iter().collect());
    // never quite zero, so a long name that does match stays in the list
    (score - t.len() as f32 * 0.02 + if tl.contains(&ql) { 12. } else { 0. }).max(0.01)
}

// where an @mention being typed starts: an @ at the start or after a space, with no space or @ since
fn mention_start(before: &str) -> Option<usize> {
    let at = before.rfind('@')?;
    let ok_before = at == 0 || before[..at].ends_with(char::is_whitespace);
    let word = &before[at + 1..];
    (ok_before && !word.contains(char::is_whitespace)).then_some(at)
}

fn file_rows(q: &str, files: &[String]) -> Vec<MenuRow> {
    let mut hits: Vec<(f32, &String)> = files.iter().map(|f| (fuzzy(q, f), f)).filter(|x| x.0 > 0.).collect();
    hits.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    hits.into_iter().take(MENU_CAP).map(|(_, f)| {
        let insert = if f.contains(' ') { format!("@\"{f}\" ") } else { format!("@{f} ") };
        MenuRow { label: f.clone().into(), hint: "".into(), src: None, insert }
    }).collect()
}

// lib/fmt.ts tokens()
pub fn tok(n: u64) -> String {
    let f = n as f64;
    if n == 0 { "0".into() } else if n < 1000 { n.to_string() } else if n < 10_000 { format!("{:.1}k", f / 1e3) } else if n < 1_000_000 { format!("{}k", (f / 1e3).round()) }
    else if n < 10_000_000 { format!("{:.2}M", f / 1e6) } else { format!("{:.1}M", f / 1e6) }
}

// .chip button: a small x
fn chip_x(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
    let p = pal(cx);
    let text = p.text;
    div().id(id).flex_none().size(u(18.)).rounded(u(4.)).flex().items_center().justify_center().cursor_pointer().bg(p.raised).text_color(p.dim)
        .hover(move |s| s.text_color(text))
        .child(Icon::new(IconName::X).size(u(geo(cx).fs * 0.9)))
}

impl Composer {
    // .menu: over the box, .mi rows with the name, where it comes from, and a hint
    fn render_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let m = self.menu.as_ref()?;
        let (p, g) = (pal(cx), geo(cx));
        let view = cx.entity();
        let n = m.rows.len();
        let row_h = 6. + g.fs * 1.55;
        let list = uniform_list("composer-menu", n, move |range, _, cx| {
            let this = view.read(cx);
            let Some(m) = &this.menu else { return vec![] };
            let (p, g) = (pal(cx), geo(cx));
            range.filter_map(|i| {
                let row = m.rows.get(i)?;
                let v = view.clone();
                Some(h_flex().id(("cm", i)).w_full().h(u(row_h)).px(u(10.)).gap(u(12.)).items_center().rounded(u(4.)).cursor_pointer()
                    .when(i == m.sel, |d| d.bg(p.high))
                    .child(div().flex_none().whitespace_nowrap().text_color(p.text).child(row.label.clone()))
                    .when_some(row.src.clone(), |d, s| d.child(div().flex_none().text_size(u(g.fs * 0.75)).text_color(p.tertiary).child(s)))
                    .child(div().min_w_0().truncate().text_size(u(g.fs * 0.88)).text_color(p.dim).child(row.hint.clone()))
                    // mouse down, not click: the box keeps focus so the menu doesn't close under the pointer
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        cx.stop_propagation();
                        v.update(cx, |this, cx| { if let Some(m) = &mut this.menu { m.sel = i; } this.accept(window, cx); });
                    })
                    .into_any_element())
            }).collect()
        }).track_scroll(&self.menu_scroll).h(u((n as f32 * row_h).min(320. - 10.)));
        Some(
            deferred(div().absolute().left(u(-2.)).right(u(-2.)).bottom_full().mb(u(6.)).child(
                div().p(u(4.)).rounded(u(g.r2)).border_1().border_color(p.line).bg(p.raised)
                    .shadow(vec![BoxShadow { color: hsla(0., 0., 0., 0.3), offset: point(px(0.), px(-10.)), blur_radius: px(30.), spread_radius: px(0.), inset: false }])
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(list),
            )).with_priority(1).into_any_element(),
        )
    }

    // .chips: quotes, then images
    fn render_chips(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.attached.is_empty() && self.quotes.is_empty() { return None; }
        let (p, g) = (pal(cx), geo(cx));
        let mut row = h_flex().flex_wrap().gap(u(6.)).mb(u(8.));
        for q in &self.quotes {
            let id = q.id;
            let shown = q.label.clone().unwrap_or_else(|| if q.text.chars().count() > 140 { format!("{}…", q.text.chars().take(140).collect::<String>()) } else { q.text.clone() });
            row = row.child(h_flex().id(("quote", id as usize)).relative().max_w_full().items_start().gap(u(6.)).py(u(4.)).pr(u(4.)).pl(u(10.)).rounded(u(5.)).bg(p.raised)
                .text_size(u(g.fs * 0.84))
                .child(div().absolute().left_0().top_0().bottom_0().w(u(2.)).rounded_l(u(5.)).bg(p.tertiary))
                .child(div().min_w_0().max_h(u(g.fs * 0.84 * 4.6)).overflow_hidden().text_color(p.mid).child(shown))
                .child(chip_x(("quote-x", id as usize), cx).on_click(cx.listener(move |this, _, _, cx| { this.quotes.retain(|q| q.id != id); cx.notify(); }))));
        }
        for a in &self.attached {
            let id = a.id;
            row = row.child(div().relative().flex_none()
                .child(img(a.thumb.clone()).h(u(56.)).w(u(56. * a.aspect)).rounded(u(5.)).border_1().border_color(p.line))
                .child(chip_x(("img-x", id as usize), cx).absolute().top(u(-6.)).right(u(-6.)).border_1().border_color(p.line)
                    .on_click(cx.listener(move |this, _, _, cx| { this.attached.retain(|a| a.id != id); cx.notify(); }))));
        }
        Some(row.into_any_element())
    }
}

impl Render for Composer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (p, g) = (pal(cx), geo(cx));
        let focused = self.focus_handle(cx).is_focused(window);
        if focused { LAST.with(|l| *l.borrow_mut() = Some(cx.entity().downgrade())); }
        let home = self.tab.is_none();
        let text = self.text(cx);
        let shell = text.starts_with('!');
        let typed = !text.trim().is_empty();
        let mode = self.mode(cx);
        let (running, model, ctx) = match &self.tab {
            Some(t) => {
                let t = t.read(cx);
                let m = &t.state["model"];
                let name = m["name"].as_str().filter(|s| !s.is_empty()).or(m["id"].as_str()).map(String::from);
                let lvl = t.state["thinkingLevel"].as_str().filter(|l| !l.is_empty() && m["reasoning"] == true);
                let model = match name { Some(n) => match lvl { Some(l) => format!("{n}  ·  {l}"), None => n }, None => "…".into() };
                let cu = &t.stats["contextUsage"];
                let ctx = cu["contextWindow"].as_u64().filter(|w| *w > 0).map(|w| (cu["percent"].as_f64().unwrap_or(0.), cu["tokens"].as_u64(), w));
                (t.running(), Some(model), ctx)
            }
            None => (false, None, None),
        };
        let stop = running && !typed && self.attached.is_empty();

        // the border: dashed accent while something is dragged over, green-ish for a shell command, brighter when focused
        let border = if shell { mix(p.green, 0.7, p.line) } else if focused { p.win_on } else { p.line };
        let primary = p.primary;
        let mode_c = if shell { p.green } else { super::status::mode_color(mode, cx) };
        let tab = self.tab.clone();
        let lmode = div().id("c-mode").px(u(6.)).rounded(u(3.)).cursor_pointer().whitespace_nowrap().text_color(mode_c)
            .child(if shell { "$ shell".to_string() } else { format!("❯ {}", mode.as_str()) })
            .on_click(cx.listener(move |this, _, window, cx| match &tab {
                Some(t) => super::picker::modes(t.clone(), window, cx),
                None => this.cycle_mode(cx),
            }));
        let lmodel = model.filter(|m| !m.is_empty()).map(|m| {
            let (dim, text) = (p.dim, p.text);
            legend(Side::R, p.bg, cx).child(div().id("c-model").px(u(6.)).rounded(u(3.)).cursor_pointer().whitespace_nowrap().text_color(dim).hover(move |s| s.text_color(text))
                .child(m).on_click(|_, window, cx| super::picker::models(window, cx)))
        });

        // .c-foot: the clip, the running hint, the context gauge, send
        let fs = g.fs;
        let ctx_el = ctx.map(|(pct, tokens, win)| {
            let on = ((pct / 100.) * 12.).round() as usize;
            let tip = format!("context {} of {}", tokens.map(tok).unwrap_or_else(|| "unknown until the next reply".into()), tok(win));
            h_flex().id("c-ctx").items_center().gap(u(6.)).text_size(u(fs * 0.78)).text_color(p.dim)
                .child(h_flex().gap(u(2.)).children((0..12).map(|i| div().w(u(4.)).h(u(10.)).rounded(u(1.))
                    .bg(if i < on && pct > 80. { p.gold } else if i < on { p.mid } else { p.high }))))
                .child(if tokens.is_none() { "—".to_string() } else { format!("{}%", pct.round()) })
                .tooltip(move |w, cx| Tooltip::new(tip.clone()).build(w, cx))
        });
        let (hover, text_c, dim) = (p.hover, p.text, p.dim);
        let clip = div().id("c-clip").flex_none().size(u(26.)).rounded(u(5.)).flex().items_center().justify_center().cursor_pointer().text_color(dim)
            .hover(move |s| s.text_color(text_c).bg(hover))
            .child(Icon::new(IconName::Paperclip).size(u(fs * 1.2)))
            .tooltip(|w, cx| Tooltip::new("attach images or files").build(w, cx))
            .on_click(cx.listener(|this, _, window, cx| this.pick(window, cx)));
        let send_tip = if running { if typed { "steer: send now, between tool calls (enter)  ·  alt+enter queues for after" } else { "stop (esc)" } } else { "send (enter)" };
        let stop_hover = mix(p.red, 0.25, p.high);
        let send = div().id("c-send").flex_none().w(u(30.)).h(u(28.)).rounded(u(g.r2)).flex().items_center().justify_center().cursor_pointer()
            .when(running, |d| d.bg(p.high).text_color(p.text).hover(move |s| s.bg(stop_hover)))
            .when(!running, |d| d.bg(p.primary).text_color(p.on_primary))
            .child(Icon::new(if stop { IconName::Square } else { IconName::ArrowUp }).size(u(fs * if stop { 1.0 } else { 1.2 })))
            .tooltip(move |w, cx| Tooltip::new(send_tip).build(w, cx))
            .on_click(cx.listener(move |this, _, window, cx| if stop { cx.emit(ComposerEvent::Stop) } else { this.submit(How::Auto, window, cx) }));
        let footer = h_flex().items_center().gap(u(10.)).mt(u(6.)).min_h(u(28.))
            .child(clip)
            .child(div().flex_1().min_w_0().truncate().text_size(u(fs * 0.78)).text_color(p.dim).child(if running { "enter steers · alt+enter queues · esc stops" } else { "" }))
            .children(ctx_el)
            .child(send);

        let menu = if focused { self.render_menu(cx) } else { None };
        let chips = self.render_chips(cx);
        let line_h = fs * 1.55;
        boxed(cx).id("composer").w_full().key_context("Composer").group("composer")
            .when(!home, |d| d.mt(u(12.)))
            .on_action(cx.listener(|this, _: &QueueFollowUp, window, cx| this.submit(How::FollowUp, window, cx)))
            // digits and y/n answer the top question while the box is empty
            .capture_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                let k = &e.keystroke;
                if k.modifiers.control || k.modifiers.alt || k.modifiers.platform || this.asks.is_none() { return; }
                let Some(c) = k.key.chars().next().filter(|_| k.key.chars().count() == 1) else { return };
                if this.ask_key(AskKey::Char(c), window, cx) { cx.stop_propagation(); }
            }))
            .capture_action(cx.listener(|this, _: &MoveUp, window, cx| {
                if this.menu.is_some() { this.menu_step(-1, cx); cx.stop_propagation(); }
                else if this.ask_key(AskKey::Up, window, cx) || this.hist_up(window, cx) { cx.stop_propagation(); }
            }))
            .capture_action(cx.listener(|this, _: &MoveDown, window, cx| {
                if this.menu.is_some() { this.menu_step(1, cx); cx.stop_propagation(); }
                else if this.ask_key(AskKey::Down, window, cx) || this.hist_down(window, cx) { cx.stop_propagation(); }
            }))
            // tab and shift+tab take the menu's pick; shift+tab otherwise runs through the modes
            .capture_action(cx.listener(|this, _: &IndentInline, window, cx| {
                if this.menu.is_some() { this.accept(window, cx); cx.stop_propagation(); }
            }))
            .capture_action(cx.listener(|this, _: &OutdentInline, window, cx| {
                if this.menu.is_some() { this.accept(window, cx); } else { this.cycle_mode(cx); }
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, e: &Enter, window, cx| {
                if e.shift { return; }
                if let Some(m) = &this.menu {
                    // a command that's already typed out in full just sends
                    if m.kind == MenuKind::Slash && m.rows.get(m.sel).is_some_and(|r| r.insert.trim() == this.text(cx).trim()) { this.menu = None; return; }
                    this.accept(window, cx);
                    cx.stop_propagation();
                    return;
                }
                if this.attached.is_empty() && this.quotes.is_empty() && this.ask_key(AskKey::Enter, window, cx) { cx.stop_propagation(); }
            }))
            .capture_action(cx.listener(|this, _: &Escape, window, cx| {
                if this.menu.take().is_some() { cx.notify(); cx.stop_propagation(); return; }
                // esc stops a run, and denies or skips the top question (composer.ts and chat.ts both see it)
                let asked = this.ask_key(AskKey::Escape, window, cx);
                let stopped = this.tab.as_ref().is_some_and(|t| t.read(cx).running());
                if stopped { cx.emit(ComposerEvent::Stop); }
                if asked || stopped { cx.stop_propagation(); }
            }))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| { this.add_paths(paths.paths(), window, cx); cx.stop_propagation(); }))
            .children(menu)
            .child(
                v_flex().w_full().pt(u(if home { 18. } else { 14. })).px(u(12.)).pb(u(6.))
                    .children(chips)
                    .child(div().mx(u(-2.)).child(Textarea::new(&self.input).appearance(false).xsmall().text_size(u(fs)).line_height(u(line_h)).text_color(p.text).on_paste({
                        let v = cx.entity().downgrade();
                        move |item, window, cx| v.update(cx, |this, cx| this.paste(item, window, cx)).unwrap_or(false)
                    })))
                    .child(footer),
            )
            .child(edge(border, cx).group_drag_over::<ExternalPaths>("composer", move |s| s.border_color(primary).border_dashed()))
            .child(legend(Side::L, p.bg, cx).child(lmode))
            .children(lmodel)
    }
}
