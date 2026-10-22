// The panel on the right: what the session changed, the project's files, a shell in its folder, running subagents.
// one panel per window. it follows the active session and remembers which pane each session was on;
// each session keeps its own shell for as long as its tab is open
mod agents;
mod changes;
mod files;
mod terminal;
mod web;

use super::theme::pal;
use super::workspace::TOP;
use crate::state::app::Pith;
use crate::state::tab::{How, Tab, TabEvent};
use agents::Agents;
use changes::{Changes, ChangesEvent};
use files::Files;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Sizable};
use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::collections::HashMap;
use std::sync::Mutex;
use terminal::TermView;
use web::WebPane;

pub fn bind_keys(cx: &mut App) { terminal::bind_keys(cx); }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pane { Changes, Files, Terminal, Agents, Browser, Preview }
// browser and preview only get a tab once the session has used them
const PANES: [(Pane, &str); 6] = [(Pane::Changes, "Changes"), (Pane::Files, "Files"), (Pane::Terminal, "Terminal"), (Pane::Agents, "Agents"), (Pane::Browser, "Browser"), (Pane::Preview, "Preview")];

pub const MIN_W: f32 = 320.;

// a file asked for from outside the panel (a path in the chat). it opens the panel and lands in Files
static REVEAL: Mutex<Option<(String, Option<usize>)>> = Mutex::new(None);
#[allow(dead_code)]
pub fn reveal(path: &str, line: Option<usize>, cx: &mut App) {
    *REVEAL.lock().unwrap() = Some((path.to_string(), line));
    Pith::global(cx).update(cx, |a, cx| { a.side = true; cx.notify(); });
}

// a pane asked for from outside the panel (/diff, /terminal): opens the panel on it
static OPEN: Mutex<Option<Pane>> = Mutex::new(None);
// for callers already inside an update of the app state: they set app.side themselves
pub fn request(pane: Pane) { *OPEN.lock().unwrap() = Some(pane); }
pub fn open(pane: Pane, cx: &mut App) {
    request(pane);
    Pith::global(cx).update(cx, |a, cx| { a.side = true; cx.notify(); });
}

struct ResizeDrag;
struct Ghost;
impl Render for Ghost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement { Empty }
}

pub struct SidePanel {
    app: Entity<Pith>,
    tab: Option<Entity<Tab>>,
    open: bool,
    pane: HashMap<EntityId, Pane>,
    changes: Entity<Changes>,
    files: Entity<Files>,
    agents: Entity<Agents>,
    terms: HashMap<EntityId, Entity<TermView>>,
    browser: Entity<WebPane>,
    preview: Entity<WebPane>,
    // the session changed something while Changes wasn't on screen
    stale: bool,
    save_w: Option<Task<()>>,
    tab_sub: Option<Subscription>,
    _subs: Vec<Subscription>,
}

impl SidePanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let app = Pith::global(cx);
        let changes = cx.new(Changes::new);
        let files = cx.new(|cx| Files::new(window, cx));
        let agents = cx.new(Agents::new);
        let browser = cx.new(|cx| WebPane::new(web::Kind::Browser, cx));
        let preview = cx.new(|cx| WebPane::new(web::Kind::Preview, cx));
        let subs = vec![
            cx.observe_in(&app, window, |this, _, window, cx| this.sync(window, cx)),
            cx.observe(&agents, |_, _, cx| cx.notify()),
            cx.observe(&changes, |_, _, cx| cx.notify()),
            cx.subscribe_in(&changes, window, |this, _, ev: &ChangesEvent, window, cx| match ev {
                ChangesEvent::OpenFile(path, line) => this.open_file(path.clone(), Some(*line), window, cx),
            }),
        ];
        let mut this = SidePanel {
            app, tab: None, open: false, pane: HashMap::new(), changes, files, agents, terms: HashMap::new(), browser, preview,
            stale: true, save_w: None, tab_sub: None, _subs: subs,
        };
        this.sync(window, cx);
        this
    }

    pub fn current(&self) -> Pane { self.pane() }
    fn pane(&self) -> Pane { self.tab.as_ref().and_then(|t| self.pane.get(&t.entity_id()).copied()).unwrap_or(Pane::Changes) }

    // runs on every app notify, so it only does real work when the session, or the panel's visibility, moved
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (active, open, live) = {
            let a = self.app.read(cx);
            (a.active.clone(), a.side, a.tabs.iter().map(|t| t.entity_id()).collect::<std::collections::HashSet<_>>())
        };
        if self.terms.len() + self.pane.len() > 0 {
            // a closed session takes its shell with it
            let gone: Vec<EntityId> = self.terms.keys().chain(self.pane.keys()).filter(|k| !live.contains(k)).copied().collect();
            for k in gone {
                self.terms.remove(&k);
                self.pane.remove(&k);
                self.changes.update(cx, |c, _| c.forget(k));
                self.files.update(cx, |f, _| f.forget(k));
            }
        }
        let moved = active.as_ref().map(|t| t.entity_id()) != self.tab.as_ref().map(|t| t.entity_id());
        if moved {
            self.tab = active.clone();
            self.stale = true;
            self.tab_sub = active.as_ref().map(|t| cx.subscribe_in(t, window, |this, _, ev: &TabEvent, window, cx| match ev {
                TabEvent::Settled => this.touched(window, cx),
                TabEvent::ToolDone(name) if !matches!(name.as_str(), "read" | "grep" | "find" | "ls") => this.touched(window, cx),
                _ => {}
            }));
        }
        let shown = open && !self.open;
        self.open = open;
        let asked = OPEN.lock().unwrap().take();
        if let (Some(pane), Some(tab)) = (asked, self.tab.clone()) { self.pane.insert(tab.entity_id(), pane); self.show_pane(false, window, cx); return; }
        if let Some((path, line)) = REVEAL.lock().unwrap().take() {
            if self.tab.is_some() { self.open_file(path, line, window, cx); return; }
        }
        if open && (moved || shown) { self.show_pane(false, window, cx); }
    }

    fn touched(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.stale = true;
        if self.open && self.pane() == Pane::Changes { self.show_pane(false, window, cx); }
    }

    fn show_pane(&mut self, focus: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tab.clone() else { return };
        let (id, cwd) = (tab.entity_id(), tab.read(cx).cwd.clone());
        match self.pane() {
            Pane::Changes => {
                if self.stale { self.stale = false; self.changes.update(cx, |c, cx| c.show(id, &cwd, cx)); }
            }
            Pane::Files => {
                self.files.update(cx, |f, cx| f.show(id, &cwd, cx));
                if focus { let h = self.files.read(cx).find_handle(cx); h.focus(window, cx); }
            }
            Pane::Terminal => {
                let t = self.terms.entry(id).or_insert_with(|| cx.new(|cx| TermView::new(cwd, window, cx))).clone();
                if focus { let h = t.read(cx).focus_handle(); h.focus(window, cx); }
            }
            Pane::Agents => {}
            Pane::Browser => self.browser.update(cx, |w, cx| w.show(&tab.read(cx).id.clone(), cx)),
            Pane::Preview => self.preview.update(cx, |w, cx| w.show(&tab.read(cx).id.clone(), cx)),
        }
        let pane = self.pane();
        // snapshots stop while a page isn't on screen
        if pane != Pane::Browser || !self.open { self.browser.update(cx, |w, _| w.hide()); }
        if pane != Pane::Preview || !self.open { self.preview.update(cx, |w, _| w.hide()); }
        cx.notify();
    }

    fn set_pane(&mut self, pane: Pane, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = &self.tab else { return };
        self.pane.insert(tab.entity_id(), pane);
        // switching to Changes always looks again: it's cheap and the tree may have moved under it
        if pane == Pane::Changes { self.stale = true; }
        self.show_pane(true, window, cx);
    }

    fn open_file(&mut self, path: String, line: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tab.clone() else { return };
        let cwd = tab.read(cx).cwd.clone();
        // git's patch paths can keep their b/ prefix; drop it when the plain path is the real one
        let path = match path.strip_prefix("b/") { Some(p) if !std::path::Path::new(&cwd).join(&path).exists() => p.to_string(), _ => path };
        self.pane.insert(tab.entity_id(), Pane::Files);
        self.files.update(cx, |f, cx| { f.show(tab.entity_id(), &cwd, cx); f.open_path(tab.entity_id(), &path, line, cx); });
        self.show_pane(false, window, cx);
    }

    fn resize(&mut self, e: &DragMoveEvent<ResizeDrag>, window: &mut Window, cx: &mut Context<Self>) {
        let vw = f32::from(window.viewport_size().width);
        let w = (vw - f32::from(e.event.position.x)).clamp(MIN_W, (vw - 520.).max(MIN_W)).round();
        // main.ts: at least 320, and the main window keeps 520
        if (self.app.read(cx).side_w - w).abs() < 0.5 { return; }
        self.app.update(cx, |a, cx| { a.side_w = w; cx.notify(); });
        // the width is saved once the drag settles, not on every move
        let app = self.app.clone();
        self.save_w = Some(cx.spawn(async move |_, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(400)).await;
            let _ = app.update(cx, |a, cx| a.save_settings(serde_json::json!({ "paneWidth": w }), cx));
        }));
    }

    fn strip(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = pal(cx);
        let t = cx.theme().clone();
        let cur = self.pane();
        let running = self.agents.read(cx).running_count();
        let busy = self.changes.read(cx).busy;
        h_flex().h(px(TOP)).flex_none().pl_2().pr_2().gap_0p5().items_center().window_control_area(WindowControlArea::Drag)
            .on_mouse_down(MouseButton::Left, |_, window, _| if !cfg!(target_os = "macos") { window.start_window_move() })
            .children(PANES.iter().filter(|(pane, _)| self.has(*pane, cx)).map(|(pane, label)| {
                let (pane, on) = (*pane, *pane == cur);
                h_flex().id(*label).h(px(26.)).px_2p5().gap_1p5().items_center().rounded(t.radius).text_sm()
                    .text_color(if on { t.foreground } else { p.dim })
                    .when(on, |d| d.bg(t.muted).font_weight(FontWeight::MEDIUM))
                    // always styled on hover, or a tab that was hovered when picked keeps looking hovered
                    .hover(|s| s.text_color(t.foreground))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(*label)
                    .when(pane == Pane::Agents && running > 0, |d| d.child(
                        div().px_1p5().h(px(16.)).flex().items_center().rounded_full().bg(t.primary.opacity(0.16)).text_xs().text_color(t.primary).child(running.to_string())))
                    .on_click(cx.listener(move |this, _, window, cx| this.set_pane(pane, window, cx)))
            }))
            .child(div().flex_1())
            .when(cur == Pane::Changes, |d| d.child(div().on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()).child(
                if busy { div().size(px(24.)).flex().items_center().justify_center().child(Spinner::new().xsmall().color(p.dim)).into_any_element() }
                else { Button::new("ch-refresh").icon(IconName::RefreshCw).ghost().xsmall().tooltip("Refresh")
                    .on_click(cx.listener(|this, _, _, cx| this.changes.update(cx, |c, cx| c.refresh(cx)))).into_any_element() })))
    }

    fn has(&self, pane: Pane, cx: &App) -> bool {
        let Some(tab) = &self.tab else { return !matches!(pane, Pane::Browser | Pane::Preview) };
        let t = tab.read(cx);
        match pane {
            Pane::Browser => crate::core::web::page(&crate::core::browser::page_id(&t.id)).is_some(),
            Pane::Preview => t.preview.is_some(),
            _ => true,
        }
    }

    fn changes_bar(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let (branch, n, add, del) = self.changes.read(cx).summary()?;
        let p = pal(cx);
        let t = cx.theme();
        let tab = self.tab.clone();
        Some(h_flex().h(px(34.)).flex_none().px_3().gap_2().items_center().border_b_1().border_color(t.border).text_sm()
            .child(changes::branch_icon().text_color(p.dim))
            .child(div().min_w_0().truncate().text_color(p.mid).child(branch))
            .when(n > 0, |d| d
                .child(div().flex_none().text_xs().text_color(p.dim).child(if n == 1 { "1 file".to_string() } else { format!("{n} files") }))
                .child(h_flex().flex_none().gap_1p5().text_xs().font_family(t.mono_font_family.clone())
                    .child(div().text_color(p.green).child(format!("+{add}")))
                    .child(div().text_color(p.red).child(format!("−{del}")))))
            .child(div().flex_1())
            .when(n > 0, |d| d.child(Button::new("ch-review").label("Review").ghost().xsmall().tooltip("Ask the session to review these changes")
                .on_click(move |_, _, cx| if let Some(t) = &tab { t.update(cx, |t, cx| t.send("/review".into(), vec![], How::Auto, cx)); }))))
    }
}

impl Render for SidePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = pal(cx);
        let t = cx.theme().clone();
        let tab = self.tab.as_ref().map(|t| t.entity_id());
        let body: AnyElement = match (self.pane(), tab) {
            (_, None) => v_flex().flex_1().items_center().justify_center().text_sm().text_color(p.dim).child("Open a session to see its files").into_any_element(),
            (Pane::Changes, _) => v_flex().flex_1().min_h_0().children(self.changes_bar(cx)).child(div().flex_1().min_h_0().child(self.changes.clone())).into_any_element(),
            (Pane::Files, _) => div().flex_1().min_h_0().child(self.files.clone()).into_any_element(),
            (Pane::Terminal, Some(id)) => match self.terms.get(&id) {
                Some(term) => div().flex_1().min_h_0().border_t_1().border_color(t.border).child(term.clone()).into_any_element(),
                None => div().flex_1().into_any_element(),
            },
            (Pane::Agents, _) => div().flex_1().min_h_0().border_t_1().border_color(t.border).child(self.agents.clone()).into_any_element(),
            (Pane::Browser, _) => div().flex_1().min_h_0().child(self.browser.clone()).into_any_element(),
            (Pane::Preview, _) => div().flex_1().min_h_0().child(self.preview.clone()).into_any_element(),
        };
        v_flex().id("side-panel").size_full().relative().bg(t.background)
            .on_drag_move(cx.listener(Self::resize))
            .child(self.strip(cx))
            .child(body)
            .child(div().id("side-resize").absolute().left_0().top_0().bottom_0().w(px(5.)).cursor(CursorStyle::ResizeLeftRight)
                .hover(|s| s.bg(t.primary.opacity(0.4)))
                .on_drag(ResizeDrag, |_, _, _, cx| cx.new(|_| Ghost)))
    }
}
