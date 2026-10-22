// The box you type into. Enter sends, shift+enter is a new line. while pi works, Enter steers and alt+enter queues a follow up.
// / at the start offers commands, @ offers the project's files, images paste or drop in. the footer holds what the next
// message runs with: mode, model, thinking, plus how full the context is
use super::commands::{self, level_hint, level_label, mode_icon, mode_label};
use super::picker::fuzzy;
use super::theme::pal;
use crate::state::app::Pith;
use crate::state::tab::{How, Img, Level, Mode, Status, Tab};
use base64::Engine as _;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Enter, Escape, IndentInline, InputEvent, MoveDown, MoveUp, OutdentInline, Textarea, TextareaState};
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Icon, Side, Sizable};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::assets::IconName;
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

struct Attached { img: Img, thumb: Arc<Image>, id: u64 }

#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuKind { Slash, At }

struct MenuRow { label: SharedString, detail: SharedString, tag: Option<SharedString>, insert: String }

// the completion list over the box: commands after a leading /, files after @
struct Menu { kind: MenuKind, rows: Vec<MenuRow>, sel: usize, start: usize, mouse: Option<Point<Pixels>>, loading: bool }

const MENU_ROW: f32 = 32.;
const MENU_MAX: usize = 8;

pub struct Composer {
    pub tab: Option<Entity<Tab>>,
    input: Entity<TextareaState>,
    attached: Vec<Attached>,
    menu: Option<Menu>,
    menu_scroll: UniformListScrollHandle,
    // what was sent from here, for Up. the transcript's own prompts come first
    sent: Vec<String>,
    // walking history: how far back, and the draft to come back to
    walk: Option<(usize, String)>,
    // the input ask the box is answering, so its prefill goes in once
    ask: Option<String>,
    placeholder: String,
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

fn key(k: &str) -> String { Keystroke::parse(k).map(|k| Kbd::format(&k)).unwrap_or_default() }

impl Composer {
    pub fn new(tab: Option<Entity<Tab>>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        bind_once(cx);
        let input = cx.new(|cx| TextareaState::new(window, cx).auto_grow(1, 14).submit_on_enter(true));
        let mut subs = vec![cx.subscribe_in(&input, window, |this, _, ev: &InputEvent, window, cx| match ev {
            InputEvent::PressEnter { secondary, shift } => {
                if *shift { return; }
                this.submit(if *secondary { How::FollowUp } else { How::Auto }, window, cx);
            }
            InputEvent::Change => { this.update_menu(cx); cx.notify(); }
            InputEvent::Blur => { if this.menu.take().is_some() { cx.notify(); } }
            _ => {}
        })];
        if let Some(t) = &tab { subs.push(cx.observe_in(t, window, |this, _, window, cx| { this.sync_tab(window, cx); cx.notify(); })); }
        subs.push(cx.observe_in(&Pith::global(cx), window, |this, _, window, cx| this.sync_placeholder(window, cx)));
        let mut c = Composer { tab, input, attached: vec![], menu: None, menu_scroll: UniformListScrollHandle::new(), sent: vec![], walk: None, ask: None, placeholder: String::new(), _subs: subs };
        c.sync_placeholder(window, cx);
        c
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

    // attaches an image from elsewhere in the app (an images page, a screenshot)
    #[allow(dead_code)]
    pub fn add_image(&mut self, img: Img, cx: &mut Context<Self>) {
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(&img.data) else { return };
        let Some(fmt) = ImageFormat::from_mime_type(&img.mime) else { return };
        self.attach(fmt, bytes, cx);
    }

    fn cwd(&self, cx: &App) -> String {
        match &self.tab { Some(t) => t.read(cx).cwd.clone(), None => Pith::global(cx).read(cx).home_cwd.clone() }
    }

    // the first thing pi asked that wants typed text, if any. chat shows asks one at a time, oldest first
    fn input_ask(&self, cx: &App) -> Option<crate::state::tab::Ask> {
        let t = self.tab.as_ref()?.read(cx);
        t.asks.first().filter(|a| a.method == "input" || a.method == "editor").cloned()
    }

    fn sync_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ask = self.input_ask(cx);
        if ask.as_ref().map(|a| &a.id) != self.ask.as_ref() {
            self.ask = ask.as_ref().map(|a| a.id.clone());
            if let Some(pre) = ask.and_then(|a| a.prefill).filter(|p| !p.is_empty()) {
                if self.text(cx).trim().is_empty() { self.set_text(&pre, window, cx); }
            }
        }
        self.sync_placeholder(window, cx);
    }

    fn sync_placeholder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ph = match &self.tab {
            None => "Ask anything. / for commands, @ to mention a file".to_string(),
            Some(t) => match self.input_ask(cx) {
                Some(a) => a.placeholder.clone().filter(|p| !p.is_empty()).unwrap_or_else(|| if a.title.is_empty() { "Type your answer".into() } else { a.title.clone() }) + " (Enter answers)",
                None if t.read(cx).running() => format!("Steer pi while it works, or {} to queue a follow-up", key("alt-enter")),
                None => "Reply to pi. / for commands, @ to mention a file".into(),
            },
        };
        if ph != self.placeholder {
            self.placeholder = ph.clone();
            self.input.update(cx, |s, cx| s.set_placeholder(ph, window, cx));
        }
    }

    fn submit(&mut self, how: How, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.text(cx).trim().to_string();
        // an open question from pi takes the text as its answer
        if let (Some(ask), Some(t)) = (self.input_ask(cx), self.tab.clone()) {
            if text.is_empty() { return; }
            self.clear(window, cx);
            t.update(cx, |t, cx| t.answer(&ask.id, serde_json::json!({ "value": text }), cx));
            return;
        }
        if text.is_empty() && self.attached.is_empty() { return; }
        self.remember(&text);
        if self.attached.is_empty() && commands::is_builtin(&text) {
            self.clear(window, cx);
            let tab = self.tab.clone();
            // after this update, a command may well touch this composer
            window.defer(cx, move |window, cx| { commands::run_builtin(tab, &text, window, cx); });
            return;
        }
        let images = self.attached.drain(..).map(|a| a.img).collect();
        self.clear(window, cx);
        cx.emit(ComposerEvent::Submit { text, images, how });
    }

    fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |s, cx| s.set_value("", window, cx));
        self.menu = None;
        self.walk = None;
        cx.notify();
    }

    fn remember(&mut self, text: &str) {
        if text.is_empty() { return; }
        self.sent.retain(|s| s != text);
        self.sent.push(text.to_string());
    }

    // ---------- history ----------
    fn history(&self, cx: &App) -> Vec<String> {
        let mut h: Vec<String> = vec![];
        if let Some(t) = &self.tab {
            for it in &t.read(cx).items {
                if let crate::state::tab::ItemKind::User { text, typed, .. } = &it.kind {
                    let s = typed.clone().unwrap_or_else(|| text.clone());
                    if !s.trim().is_empty() && h.last() != Some(&s) { h.push(s); }
                }
            }
        }
        for s in &self.sent { if !h.contains(s) { h.push(s.clone()); } }
        h
    }

    // Up walks back from the first line, Down forward from the last. false when the key should move the caret instead
    fn walk_history(&mut self, back: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let (line, lines, empty) = {
            let s = self.input.read(cx);
            let v = s.value();
            (s.cursor_position().line as usize, v.split('\n').count(), v.trim().is_empty())
        };
        let h = self.history(cx);
        if h.is_empty() { return false; }
        let next = match (back, &self.walk) {
            (true, None) if empty || line == 0 => 0,
            (true, Some((i, _))) if line == 0 => (*i + 1).min(h.len() - 1),
            (false, Some((i, _))) if line + 1 >= lines => { if *i == 0 { usize::MAX } else { *i - 1 } }
            _ => return false,
        };
        let draft = match self.walk.take() { Some((_, d)) => d, None => self.text(cx) };
        let text = if next == usize::MAX { draft.clone() } else { h[h.len() - 1 - next].clone() };
        if next != usize::MAX { self.walk = Some((next, draft)); }
        self.input.update(cx, |s, cx| {
            s.set_value(text.clone(), window, cx);
            // going back leaves the caret on the first line so Up keeps going, forward on the last
            let at = if back { 0 } else { text.len() };
            s.set_selected_range(at..at, cx);
        });
        self.menu = None;
        cx.notify();
        true
    }

    // ---------- completion ----------
    fn update_menu(&mut self, cx: &mut Context<Self>) {
        let (value, cursor) = { let s = self.input.read(cx); (s.value().to_string(), s.cursor()) };
        let before = &value[..cursor.min(value.len())];
        let prev_sel = self.menu.as_ref().map(|m| (m.kind, m.sel));
        let menu = if let Some(q) = before.strip_prefix('/').filter(|q| !q.contains(char::is_whitespace)) {
            let tab = self.tab.as_ref().map(|t| t.read(cx));
            let mut cmds: Vec<(commands::Cmd, f32)> = commands::commands_for(tab).into_iter().map(|c| { let s = fuzzy(q, &c.name); (c, s) }).filter(|x| x.1 > 0.).collect();
            if !q.is_empty() { cmds.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)); }
            let rows: Vec<MenuRow> = cmds.into_iter().map(|(c, _)| MenuRow {
                label: format!("/{}", c.name).into(),
                detail: match &c.arg { Some(a) if !c.desc.is_empty() => format!("{}  {a}", c.desc), Some(a) => a.clone(), None => c.desc.clone() }.into(),
                tag: c.tag().map(SharedString::from),
                insert: format!("/{} ", c.name),
            }).collect();
            Some(Menu { kind: MenuKind::Slash, rows, sel: 0, start: 0, mouse: None, loading: false })
        } else if let Some(at) = mention_start(before) {
            let q = &before[at + 1..];
            let cwd = self.cwd(cx);
            match self.files(&cwd, cx) {
                Some(files) => Some(Menu { kind: MenuKind::At, rows: file_rows(q, &files), sel: 0, start: at, mouse: None, loading: false }),
                // still listing; the menu fills in when it's done
                None => Some(Menu { kind: MenuKind::At, rows: vec![], sel: 0, start: at, mouse: None, loading: true }),
            }
        } else { None };
        self.menu = menu.filter(|m| !m.rows.is_empty() || m.kind == MenuKind::At);
        // typing on keeps the selection at the top, the best match
        if let (Some(m), Some((k, _))) = (&mut self.menu, prev_sel) { if m.kind != k { m.sel = 0; } }
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
                let _ = this.update(cx, |c, cx| { if c.menu.as_ref().is_some_and(|m| m.kind == MenuKind::At) { c.update_menu(cx); cx.notify(); } });
            }).detach();
        }
        // a stale list still answers right away, the fresh one replaces it when it lands
        entry.filter(|e| !e.1.is_empty() || !e.2).map(|e| e.1)
    }

    fn menu_step(&mut self, d: isize, cx: &mut Context<Self>) {
        let Some(m) = &mut self.menu else { return };
        if m.rows.is_empty() { return; }
        let n = m.rows.len() as isize;
        m.sel = ((m.sel as isize + d).rem_euclid(n)) as usize;
        self.menu_scroll.scroll_to_item(m.sel, ScrollStrategy::Nearest);
        cx.notify();
    }

    fn accept(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(m) = self.menu.take() else { return };
        let Some(row) = m.rows.get(m.sel) else { return };
        let (value, cursor) = { let s = self.input.read(cx); (s.value().to_string(), s.cursor()) };
        // the whole word under the caret goes, plus one space after it, so accepting mid-word doesn't leave a tail
        let tail = value[cursor..].find(char::is_whitespace).unwrap_or(value.len() - cursor);
        let mut end = cursor + tail;
        if value[end..].starts_with(' ') { end += 1; }
        let start = if m.kind == MenuKind::Slash { 0 } else { m.start };
        let insert = row.insert.clone();
        self.input.update(cx, |s, cx| {
            s.set_selected_range(start..end, cx);
            s.replace(insert, window, cx);
        });
        cx.notify();
    }

    // ---------- images and paths ----------
    fn attach(&mut self, fmt: ImageFormat, bytes: Vec<u8>, cx: &mut Context<Self>) {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        // what the models take
        if !matches!(fmt, ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif | ImageFormat::Webp) {
            return toast(Level::Warn, "Only PNG, JPEG, GIF and WebP images can be attached", cx);
        }
        if bytes.len() > 20 * 1024 * 1024 { return toast(Level::Warn, "That image is over 20 MB", cx); }
        let img = Img { data: base64::engine::general_purpose::STANDARD.encode(&bytes), mime: fmt.mime_type().into() };
        self.attached.push(Attached { img, thumb: Arc::new(Image::from_bytes(fmt, bytes)), id: N.fetch_add(1, std::sync::atomic::Ordering::SeqCst) });
        cx.notify();
    }

    // dropped or pasted files: images attach, anything else goes in as an @mention
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
        if !mention.is_empty() {
            let need_space = { let s = self.input.read(cx); let v = s.value(); let c = s.cursor(); c > 0 && !v[..c].ends_with(char::is_whitespace) };
            let text = if need_space { format!(" {mention}") } else { mention };
            self.input.update(cx, |s, cx| s.insert(text, window, cx));
        }
        self.focus_handle(cx).focus(window, cx);
        cx.notify();
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

// where an @mention being typed starts: an @ at the start or after a space, with no space since
fn mention_start(before: &str) -> Option<usize> {
    let at = before.rfind('@')?;
    let ok_before = at == 0 || before[..at].ends_with(char::is_whitespace);
    let word = &before[at + 1..];
    (ok_before && !word.contains(char::is_whitespace)).then_some(at)
}

// files ranked by how well the name matches, then the path; shallow, short paths win ties
fn file_rows(q: &str, files: &[String]) -> Vec<MenuRow> {
    let mut hits: Vec<(f32, &String)> = files.iter().filter_map(|f| {
        let name = f.rsplit('/').next().unwrap_or(f);
        let depth = f.matches('/').count() as f32;
        let s = if q.is_empty() { 1. } else { (fuzzy(q, name) * 1.6).max(fuzzy(q, f)) };
        (s > 0.).then(|| (s - depth * 0.05 - f.len() as f32 * 0.002, f))
    }).collect();
    hits.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    hits.into_iter().take(60).map(|(_, f)| {
        let (dir, name) = match f.rsplit_once('/') { Some((d, n)) => (d.to_string(), n.to_string()), None => (String::new(), f.clone()) };
        let insert = if f.contains(' ') { format!("@\"{f}\" ") } else { format!("@{f} ") };
        MenuRow { label: name.into(), detail: dir.into(), tag: None, insert }
    }).collect()
}

fn chip(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    Button::new(id).label(label).ghost().xsmall()
}

impl Composer {
    fn render_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let m = self.menu.as_ref()?;
        let p = pal(cx);
        let view = cx.entity();
        let n = m.rows.len();
        let kind = m.kind;
        let body = if n == 0 {
            div().px_3().py_2().text_sm().text_color(p.dim).child(if m.loading { "Listing files…" } else { "No files match" }).into_any_element()
        } else {
            uniform_list("composer-menu", n, move |range, _, cx| {
                let this = view.read(cx);
                let Some(m) = &this.menu else { return vec![] };
                let p = pal(cx);
                range.filter_map(|i| {
                    let row = m.rows.get(i)?;
                    let sel = i == m.sel;
                    let (v1, v2) = (view.clone(), view.clone());
                    Some(h_flex().id(("cm", i)).w_full().h(px(MENU_ROW)).px_2().gap_2().items_center().rounded(cx.theme().radius)
                        .when(sel, |d| d.bg(cx.theme().muted))
                        .when(kind == MenuKind::At, |d| d.child(Icon::new(IconName::File).small().text_color(p.dim)))
                        .child(div().flex_none().max_w(relative(0.5)).truncate().text_sm().text_color(cx.theme().foreground)
                            .when(kind == MenuKind::Slash, |d| d.font_weight(FontWeight::MEDIUM)).child(row.label.clone()))
                        .child(div().flex_1().min_w_0().truncate().text_sm().text_color(p.dim).child(row.detail.clone()))
                        .when_some(row.tag.clone(), |d, t| d.child(div().flex_none().text_xs().text_color(p.dim).child(t)))
                        .on_mouse_move(move |e: &MouseMoveEvent, _, cx| v1.update(cx, |this, cx| {
                            let Some(m) = &mut this.menu else { return };
                            let moved = m.mouse.is_some_and(|p| p != e.position);
                            m.mouse = Some(e.position);
                            if moved && m.sel != i { m.sel = i; cx.notify(); }
                        }))
                        // mouse down, not click: the box keeps focus so the menu doesn't close under the pointer
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            cx.stop_propagation();
                            v2.update(cx, |this, cx| { if let Some(m) = &mut this.menu { m.sel = i; } this.accept(window, cx); });
                        })
                        .into_any_element())
                }).collect()
            }).track_scroll(&self.menu_scroll).h(px(n.min(MENU_MAX) as f32 * MENU_ROW)).into_any_element()
        };
        // over the transcript in a session; on the home screen there's more room below
        let below = self.tab.is_none();
        Some(
            deferred(div().absolute().left_0().right_0().when(below, |d| d.top_full().mt_2()).when(!below, |d| d.bottom_full().mb_2()).child(
                v_flex().p_1().rounded(cx.theme().radius_lg).border_1().border_color(cx.theme().border).bg(cx.theme().popover).shadow_lg()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(body),
            )).with_priority(1).into_any_element(),
        )
    }

    fn render_attached(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = pal(cx);
        h_flex().gap_2().px_3().pt_3().flex_wrap().children(self.attached.iter().map(|a| {
            let id = a.id;
            div().id(("img", id as usize)).group("thumb").relative().size(px(56.)).rounded(cx.theme().radius).border_1().border_color(cx.theme().border).bg(p.code_bg)
                .child(img(a.thumb.clone()).size_full().rounded(cx.theme().radius).object_fit(ObjectFit::Cover))
                .child(div().absolute().top(px(-6.)).right(px(-6.)).invisible().group_hover("thumb", |s| s.visible()).child(
                    Button::new(("img-x", id as usize)).icon(IconName::Close).xsmall().rounded_full().secondary().tooltip("Remove")
                        .on_click(cx.listener(move |this, _, _, cx| { this.attached.retain(|a| a.id != id); cx.notify(); })),
                ))
        }))
    }
}

impl Render for Composer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = pal(cx);
        let focused = self.input.read(cx).focus_handle(cx).is_focused(window);
        if focused { LAST.with(|l| *l.borrow_mut() = Some(cx.entity().downgrade())); }
        let tab = self.tab.clone();
        let (running, mode, model, level, reasoning, ctx, cost) = match &tab {
            Some(t) => {
                let t = t.read(cx);
                let model = t.state["model"]["name"].as_str().filter(|s| !s.is_empty()).map(String::from).or_else(|| t.model_label());
                (t.running(), t.mode, model, t.state["thinkingLevel"].as_str().map(String::from), t.state["model"]["reasoning"] != false, t.context_percent(), t.cost())
            }
            None => (false, Pith::global(cx).read(cx).mode_default(), None, None, false, None, 0.0),
        };
        let empty = self.text(cx).trim().is_empty();
        let answering = self.input_ask(cx).is_some();
        let border = if focused { cx.theme().ring } else { cx.theme().border };
        let input_focus = self.focus_handle(cx);

        let mut footer = h_flex().gap_0p5().items_center().px_2().pb_2();
        // mode: a menu, each one says what it lets pi do
        let t1 = tab.clone();
        let f1 = input_focus.clone();
        footer = footer.child(chip("mode", mode_label(mode)).icon(mode_icon(mode)).dropdown_caret(true)
            .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _, cx| {
                let p = pal(cx);
                let mut menu = menu.check_side(Side::Right).min_w(px(280.));
                for m in Mode::ALL {
                    let (t, f) = (t1.clone(), f1.clone());
                    menu = menu.item(PopupMenuItem::element(move |_, cx| {
                        h_flex().flex_1().gap_2p5().items_center().py_1()
                            .child(Icon::new(mode_icon(m)).small().text_color(p.dim))
                            .child(v_flex().child(div().text_sm().text_color(cx.theme().foreground).child(mode_label(m))).child(div().text_xs().text_color(p.dim).child(sentence(m.hint()))))
                    }).checked(m == mode).on_click(move |_, window, cx| {
                        match &t {
                            Some(t) => t.update(cx, |t, cx| t.set_mode(m, cx)),
                            None => Pith::global(cx).update(cx, |a, cx| a.save_settings(serde_json::json!({ "mode": m.as_str() }), cx)),
                        }
                        f.focus(window, cx);
                    }));
                }
                menu
            }));
        if let Some(m) = model {
            footer = footer.child(chip("model", m).dropdown_caret(true).tooltip_with_action("Switch model", &super::workspace::SwitchModel, None)
                .on_click(|_, window, cx| super::picker::models(window, cx)));
        }
        if let (Some(l), Some(t), true) = (level, tab.clone(), reasoning) {
            let levels = t.read(cx).levels.clone();
            let f2 = input_focus.clone();
            footer = footer.child(chip("think", if l == "off" { "No thinking".to_string() } else { format!("Thinking: {}", level_label(&l)) }).icon(IconName::Brain).dropdown_caret(true)
                .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _, cx| {
                    let p = pal(cx);
                    let mut menu = menu.check_side(Side::Right).min_w(px(240.));
                    for lv in &levels {
                        let (t, f, lv2) = (t.clone(), f2.clone(), lv.clone());
                        let (label, hint) = (level_label(lv), level_hint(lv));
                        menu = menu.item(PopupMenuItem::element(move |_, cx| {
                            v_flex().flex_1().py_0p5().child(div().text_sm().text_color(cx.theme().foreground).child(label.clone())).when(!hint.is_empty(), |d| d.child(div().text_xs().text_color(p.dim).child(hint)))
                        }).checked(*lv == l).on_click(move |_, window, cx| { t.update(cx, |t, cx| t.set_thinking(lv2.clone(), cx)); f.focus(window, cx); }));
                    }
                    menu
                }));
        }
        footer = footer.child(div().flex_1());
        // the placeholder says it while the box is empty
        if running && !empty {
            footer = footer.child(div().text_xs().text_color(p.dim).mr_2().child(format!("{} steers · {} queues · {} stops", key("enter"), key("alt-enter"), key("escape"))));
        } else if answering {
            footer = footer.child(div().text_xs().text_color(p.gold).mr_2().child("pi is waiting for your answer"));
        }
        if let Some(pct) = ctx {
            footer = footer.child(div().text_xs().text_color(if pct > 80. { p.gold } else { p.dim }).mr_2().child(format!("{:.0}% context", pct)));
        }
        if cost > 0.0 { footer = footer.child(div().text_xs().text_color(p.dim).mr_2().child(format!("${cost:.2}"))); }
        let starting = tab.as_ref().is_some_and(|t| t.read(cx).status == Status::Starting);
        let nothing = empty && self.attached.is_empty();
        footer = footer.child(if running && nothing {
            Button::new("stop").icon(IconName::Square).small().rounded_full().tooltip(format!("Stop ({})", key("escape"))).on_click(cx.listener(|_, _, _, cx| cx.emit(ComposerEvent::Stop)))
        } else {
            Button::new("send").icon(IconName::ArrowUp).small().primary().rounded_full().disabled(nothing || starting && empty)
                .tooltip(if answering { "Answer".to_string() } else if running { format!("Steer ({}) · queue ({})", key("enter"), key("alt-enter")) } else { "Send".to_string() })
                .on_click(cx.listener(|this, _, window, cx| this.submit(How::Auto, window, cx)))
        });

        let menu = if focused { self.render_menu(cx) } else { None };
        let accent = cx.theme().primary;
        div().relative().w_full().key_context("Composer")
            .on_action(cx.listener(|this, _: &QueueFollowUp, window, cx| this.submit(How::FollowUp, window, cx)))
            .capture_action(cx.listener(|this, _: &MoveUp, window, cx| {
                if this.menu.is_some() { this.menu_step(-1, cx); cx.stop_propagation(); }
                else if this.walk_history(true, window, cx) { cx.stop_propagation(); }
            }))
            .capture_action(cx.listener(|this, _: &MoveDown, window, cx| {
                if this.menu.is_some() { this.menu_step(1, cx); cx.stop_propagation(); }
                else if this.walk.is_some() && this.walk_history(false, window, cx) { cx.stop_propagation(); }
            }))
            .capture_action(cx.listener(|this, _: &IndentInline, window, cx| {
                if this.menu.as_ref().is_some_and(|m| !m.rows.is_empty()) { this.accept(window, cx); cx.stop_propagation(); }
            }))
            .capture_action(cx.listener(|this, _: &OutdentInline, _, cx| {
                // shift+tab runs through the modes, like in the terminal
                if this.menu.is_some() { this.menu_step(-1, cx); cx.stop_propagation(); return; }
                let next = |m: Mode| Mode::ALL[(Mode::ALL.iter().position(|x| *x == m).unwrap_or(0) + 1) % 4];
                match &this.tab {
                    Some(t) => t.update(cx, |t, cx| { let n = next(t.mode); t.set_mode(n, cx) }),
                    None => { let n = next(Pith::global(cx).read(cx).mode_default()); Pith::global(cx).update(cx, |a, cx| a.save_settings(serde_json::json!({ "mode": n.as_str() }), cx)) }
                }
                cx.notify();
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, e: &Enter, window, cx| {
                let Some(m) = &this.menu else { return };
                if e.shift || m.rows.is_empty() { return; }
                // a command that's already typed out in full just sends
                if m.kind == MenuKind::Slash && m.rows.get(m.sel).is_some_and(|r| r.insert.trim() == this.text(cx).trim()) { this.menu = None; return; }
                this.accept(window, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &Escape, _, cx| {
                if this.menu.take().is_some() { cx.notify(); cx.stop_propagation(); }
            }))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| { this.add_paths(paths.paths(), window, cx); cx.stop_propagation(); }))
            .children(menu)
            .child(
                v_flex().w_full().rounded(px(14.)).border_1().border_color(border).bg(p.raised).shadow_sm()
                    .drag_over::<ExternalPaths>(move |s, _, _, _| s.border_color(accent).bg(accent.opacity(0.06)))
                    .when(!self.attached.is_empty(), |d| d.child(self.render_attached(cx)))
                    .child(div().px_1().pt_1().child(Textarea::new(&self.input).appearance(false).on_paste({
                        let v = cx.entity().downgrade();
                        move |item, window, cx| v.update(cx, |this, cx| this.paste(item, window, cx)).unwrap_or(false)
                    })))
                    .child(footer),
            )
    }
}
