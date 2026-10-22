// The window to the right of the chat. One per session (Tab::pane, None when closed): web, preview, changes, files,
// agents, term. it follows the active session; each session keeps its own shell for as long as its tab is open.
// the window frame and its legends are workspace.rs's, the tabs in them come from SidePanel::legends
mod agents;
mod changes;
mod code;
mod files;
mod terminal;
mod web;

use super::frame::{legend, legend_btn, Side};
use super::theme::{em, fade, geo, pal, u};
use crate::state::app::Pith;
use crate::state::hub::Hub;
use crate::state::tab::{Tab, TabEvent};
use agents::Agents;
use changes::{Changes, ChangesEvent};
use files::Files;
use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::collections::HashMap;
use std::sync::Mutex;
use terminal::TermView;
use web::WebPane;

pub fn bind_keys(cx: &mut App) { terminal::bind_keys(cx); }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pane { Browser, Preview, Changes, Files, Agents, Terminal }
// the legend's tabs, in order, with what alt+1…6 opens
pub const PANES: [(Pane, &str, &str); 6] = [(Pane::Browser, "web", "alt+1"), (Pane::Preview, "preview", "alt+2"), (Pane::Changes, "changes", "alt+3"),
    (Pane::Files, "files", "alt+4"), (Pane::Agents, "agents", "alt+5"), (Pane::Terminal, "term", "alt+6")];

pub const MIN_W: f32 = 320.;
pub const MAX_W: f32 = 900.;

// the active session's side window on a pane (act.openPane)
pub fn open(pane: Pane, cx: &mut App) {
    let app = Pith::global(cx);
    let Some(t) = app.read(cx).active.clone() else { return };
    app.update(cx, |a, cx| a.set_pane(&t, Some(pane), cx));
}
// the pane that's open closes, any other opens (act.togglePane: the header icons, alt+1…6, ctrl+\ with the current one)
#[allow(dead_code)]
pub fn toggle(pane: Pane, cx: &mut App) {
    let app = Pith::global(cx);
    let Some(t) = app.read(cx).active.clone() else { return };
    app.update(cx, |a, cx| a.toggle_pane(&t, pane, cx));
}
#[allow(dead_code)]
pub fn close(cx: &mut App) {
    let app = Pith::global(cx);
    let Some(t) = app.read(cx).active.clone() else { return };
    app.update(cx, |a, cx| a.set_pane(&t, None, cx));
}
// /browser [url]: the web pane, at that page when there's one
#[allow(dead_code)]
pub fn browse(url: Option<&str>, cx: &mut App) {
    let app = Pith::global(cx);
    let Some(t) = app.read(cx).active.clone() else { return };
    if let Some(u) = url.map(str::trim).filter(|u| !u.is_empty()) { let id = t.read(cx).id.clone(); web::go_to(&t, &id, u, cx); }
    app.update(cx, |a, cx| a.set_pane(&t, Some(Pane::Browser), cx));
}

// a file asked for from outside the panel (a path in the chat, a tool row). it opens the panel and lands in Files
static REVEAL: Mutex<Option<(String, Option<usize>)>> = Mutex::new(None);
#[allow(dead_code)]
pub fn reveal(path: &str, line: Option<usize>, cx: &mut App) {
    *REVEAL.lock().unwrap() = Some((path.to_string(), line));
    open(Pane::Files, cx);
}

struct ResizeDrag;
struct Ghost;
impl Render for Ghost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement { Empty }
}

pub struct SidePanel {
    app: Entity<Pith>,
    tab: Option<Entity<Tab>>,
    // what's on screen, so a pane only gets shown again when it changes
    shown: Option<(EntityId, Pane)>,
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
        let browser = cx.new(|cx| WebPane::new(web::Kind::Browser, window, cx));
        let preview = cx.new(|cx| WebPane::new(web::Kind::Preview, window, cx));
        let subs = vec![
            cx.observe_in(&app, window, |this, _, window, cx| this.sync(window, cx)),
            cx.observe(&agents, |_, _, cx| cx.notify()),
            cx.observe(&changes, |_, _, cx| cx.notify()),
            cx.subscribe_in(&changes, window, |this, _, ev: &ChangesEvent, window, cx| match ev {
                ChangesEvent::OpenFile(path, line) => this.open_file(path.clone(), Some(*line), window, cx),
            }),
            cx.subscribe_in(&files, window, |this, _, ev: &files::FilesEvent, window, cx| match ev {
                files::FilesEvent::Lightbox(path) => {
                    let _ = this;
                    super::lightbox::open(super::lightbox::Src::Path(path.clone()), None, window, cx);
                }
            }),
        ];
        let mut this = SidePanel {
            app, tab: None, shown: None, changes, files, agents, terms: HashMap::new(), browser, preview,
            stale: true, save_w: None, tab_sub: None, _subs: subs,
        };
        this.sync(window, cx);
        this
    }

    pub fn current(&self, cx: &App) -> Option<Pane> { self.tab.as_ref().and_then(|t| t.read(cx).pane) }

    // runs on every app notify, so it only does real work when the session or its pane moved
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (active, side, live) = {
            let a = self.app.read(cx);
            (a.active.clone(), a.side, a.tabs.iter().map(|t| t.entity_id()).collect::<std::collections::HashSet<_>>())
        };
        // a closed session takes its shell with it
        let gone: Vec<EntityId> = self.terms.keys().filter(|k| !live.contains(k)).copied().collect();
        for k in gone {
            self.terms.remove(&k);
            self.changes.update(cx, |c, _| c.forget(k));
            self.files.update(cx, |f, _| f.forget(k));
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
        let Some(tab) = self.tab.clone() else { return };
        // app.side is the open flag the rest of the window toggles; the session's own pane follows it
        let pane = tab.read(cx).pane;
        if !moved && side != pane.is_some() {
            let want = if side { Some(Pane::Browser) } else { None };
            tab.update(cx, |t, _| t.pane = want);
        }
        if let Some((path, line)) = REVEAL.lock().unwrap().take() { self.open_file(path, line, window, cx); return; }
        let now = tab.read(cx).pane.map(|p| (tab.entity_id(), p));
        if now != self.shown {
            let focus = now.is_some_and(|(_, p)| p == Pane::Terminal);
            self.show_pane(focus, window, cx);
        }
    }

    fn touched(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.stale = true;
        if self.current(cx) == Some(Pane::Changes) { self.show_pane(false, window, cx); }
    }

    fn show_pane(&mut self, focus: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tab.clone() else { return };
        let (id, cwd) = (tab.entity_id(), tab.read(cx).cwd.clone());
        let pane = tab.read(cx).pane;
        self.shown = pane.map(|p| (id, p));
        match pane {
            Some(Pane::Changes) => { if self.stale { self.stale = false; self.changes.update(cx, |c, cx| c.show(&tab, cx)); } }
            Some(Pane::Files) => self.files.update(cx, |f, cx| f.show(id, &cwd, cx)),
            Some(Pane::Terminal) => {
                let t = self.terms.entry(id).or_insert_with(|| cx.new(|cx| TermView::new(cwd, window, cx))).clone();
                if focus { let h = t.read(cx).focus_handle(); h.focus(window, cx); }
            }
            Some(Pane::Agents) | None => {}
            Some(Pane::Browser) => self.browser.update(cx, |w, cx| w.show(&tab, cx)),
            Some(Pane::Preview) => self.preview.update(cx, |w, cx| w.show(&tab, cx)),
        }
        // snapshots stop while a page isn't on screen
        if pane != Some(Pane::Browser) { self.browser.update(cx, |w, _| w.hide()); }
        if pane != Some(Pane::Preview) { self.preview.update(cx, |w, _| w.hide()); }
        cx.notify();
    }

    fn open_file(&mut self, path: String, line: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tab.clone() else { return };
        let cwd = tab.read(cx).cwd.clone();
        // git's patch paths can keep their b/ prefix; drop it when the plain path is the real one
        let path = match path.strip_prefix("b/") { Some(p) if !std::path::Path::new(&cwd).join(&path).exists() => p.to_string(), _ => path };
        tab.update(cx, |t, _| t.pane = Some(Pane::Files));
        self.app.update(cx, |a, _| a.side = true);
        self.files.update(cx, |f, cx| { f.show(tab.entity_id(), &cwd, cx); f.open_path(tab.entity_id(), &path, line, cx); });
        self.show_pane(false, window, cx);
    }

    fn resize(&mut self, e: &DragMoveEvent<ResizeDrag>, window: &mut Window, cx: &mut Context<Self>) {
        let vw = f32::from(window.viewport_size().width);
        let w = (vw - f32::from(e.event.position.x)).clamp(MIN_W, MAX_W.min(vw - MIN_W).max(MIN_W)).round();
        if (self.app.read(cx).side_w - w).abs() < 0.5 { return; }
        self.app.update(cx, |a, cx| { a.side_w = w; cx.notify(); });
        // the width is saved once the drag settles, not on every move
        let app = self.app.clone();
        self.save_w = Some(cx.spawn(async move |_, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(400)).await;
            let _ = app.update(cx, |a, cx| a.save_settings(serde_json::json!({ "paneWidth": w }), cx));
        }));
    }

    // the side window's legends for workspace.rs to put on its border: the pane tabs on the left, a close on the right
    pub fn legends(cx: &mut App) -> (Div, Div) {
        let p = pal(cx);
        let app = Pith::global(cx);
        let tab = app.read(cx).active.clone();
        let cur = tab.as_ref().and_then(|t| t.read(cx).pane);
        let running = Hub::global(cx).read(cx).running_count();
        let fs = geo(cx).fs * 0.84;
        let l = legend(Side::L, p.gutter, cx).children(PANES.iter().map(|&(pane, label, keys)| {
            let (app, tab) = (app.clone(), tab.clone());
            legend_btn(SharedString::from(format!("sp-{label}")), cur == Some(pane), cx).px(u(5.))
                .tooltip(move |w, cx| gpui_kit::component::tooltip::Tooltip::new(format!("{label} ({keys})")).build(w, cx))
                .child(label)
                .when(pane == Pane::Agents && running > 0, |d| d.child(div().min_w(u(fs * 0.85 * 1.3)).flex().justify_center().rounded(u(3.)).bg(p.primary)
                    .text_color(p.on_primary).text_size(u(fs * 0.85)).child(running.to_string())))
                .on_click(move |_, _, cx| if let Some(t) = &tab { app.update(cx, |a, cx| a.set_pane(t, Some(pane), cx)); })
        }));
        let r = legend(Side::R, p.gutter, cx).child(legend_btn("sp-close", false, cx)
            .tooltip(|w, cx| gpui_kit::component::tooltip::Tooltip::new("close (ctrl+\\)").build(w, cx))
            .child(Icon::new(IconName::X).size(u(fs * 1.2)))
            .on_click(move |_, _, cx| if let Some(t) = &tab { app.update(cx, |a, cx| a.set_pane(t, None, cx)); }));
        (l, r)
    }

    fn terminal(&self, id: EntityId, cx: &mut Context<Self>) -> AnyElement {
        let p = pal(cx);
        let Some(term) = self.terms.get(&id).cloned() else { return div().flex_1().into_any_element() };
        let cwd = self.tab.as_ref().map(|t| super::tools::tilde(&t.read(cx).cwd)).unwrap_or_default();
        let (t1, t2) = (term.clone(), term.clone());
        let tab = self.tab.clone();
        v_flex().flex_1().min_h_0()
            .child(pbar(cx).child(path_label(cwd, cx))
                .child(pb("tm-quote", false, cx).tooltip(|w, cx| gpui_kit::component::tooltip::Tooltip::new("quote the selection into the chat").build(w, cx))
                    .child(pb_icon(IconName::Copy, cx)).child("to chat")
                    .on_click(move |_, _, cx| {
                        let sel = t1.read(cx).selection().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
                        let app = Pith::global(cx);
                        match (sel, &tab) {
                            (Some(sel), Some(tab)) => {
                                let n = sel.split('\n').count();
                                tab.update(cx, |_, cx| cx.emit(TabEvent::Quote { label: format!("terminal  ·  {n} lines"), text: format!("terminal output:\n{sel}") }));
                            }
                            _ => app.update(cx, |a, cx| a.toast(crate::state::tab::Level::Info, "select some terminal output first", cx)),
                        }
                    }))
                .child(pb("tm-new", false, cx).tooltip(|w, cx| gpui_kit::component::tooltip::Tooltip::new("new shell").build(w, cx))
                    .child(pb_icon(IconName::RotateCw, cx))
                    .on_click(move |_, window, cx| t2.update(cx, |t, cx| t.new_shell(window, cx)))))
            .child(div().flex_1().min_h_0().bg(p.surface_low).child(term))
            .into_any_element()
    }
}

impl Render for SidePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = pal(cx);
        let tab = self.tab.as_ref().map(|t| t.entity_id());
        let body: AnyElement = match (self.current(cx), tab) {
            (_, None) | (None, _) => div().flex_1().into_any_element(),
            (Some(Pane::Changes), _) => div().flex_1().min_h_0().flex().flex_col().child(self.changes.clone()).into_any_element(),
            (Some(Pane::Files), _) => div().flex_1().min_h_0().flex().flex_col().child(self.files.clone()).into_any_element(),
            (Some(Pane::Terminal), Some(id)) => self.terminal(id, cx),
            (Some(Pane::Agents), _) => div().flex_1().min_h_0().flex().flex_col().child(self.agents.clone()).into_any_element(),
            (Some(Pane::Browser), _) => div().flex_1().min_h_0().flex().flex_col().child(self.browser.clone()).into_any_element(),
            (Some(Pane::Preview), _) => div().flex_1().min_h_0().flex().flex_col().child(self.preview.clone()).into_any_element(),
        };
        let line = p.line;
        v_flex().id("side-panel").size_full().relative().bg(p.bg)
            .on_drag_move(cx.listener(Self::resize))
            .child(body)
            .child(div().id("side-resize").absolute().left_0().top_0().bottom_0().w(u(6.)).cursor(CursorStyle::ResizeLeftRight)
                .hover(move |s| s.bg(line.opacity(0.6)))
                .on_drag(ResizeDrag, |_, _, _, cx| cx.new(|_| Ghost)))
    }
}

// ---------- the bits every pane is made of (panes.css) ----------

// .pbar: the pane's top row, a rule under it
pub(crate) fn pbar(cx: &App) -> Div {
    let p = pal(cx);
    h_flex().flex_none().min_w_0().items_center().gap(u(4.)).pt(u(14.)).px(u(8.)).pb(u(6.)).border_b(u(1.)).border_color(p.rule).text_size(em(cx, 0.9))
}
// .pb: a quiet button in it, the accent when on
pub(crate) fn pb(id: impl Into<ElementId>, on: bool, cx: &App) -> Stateful<Div> {
    let p = pal(cx);
    let (text, hover) = (p.text, p.hover);
    div().id(id).flex_none().flex().items_center().gap(u(5.)).h(u(26.)).px(u(6.)).rounded(u(5.)).whitespace_nowrap().cursor_pointer()
        .text_color(if on { p.accent } else { p.dim }).when(on, |d| d.bg(p.sel))
        .when(!on, |d| d.hover(move |s| s.text_color(text).bg(hover)))
}
// .ic at the pbar's size
pub(crate) fn pb_icon(name: IconName, cx: &App) -> Icon { Icon::new(name).size(em(cx, 0.9 * 1.2)) }
// .pv-path: the dim-ish label that takes the room
pub(crate) fn path_label(text: impl Into<SharedString>, cx: &App) -> Div {
    div().flex_1().min_w_0().pl(u(4.)).overflow_hidden().text_ellipsis().whitespace_nowrap().text_color(pal(cx).mid).child(text.into())
}
// .btn.sm, plain or .ghost; dimmed and dead when disabled
pub(crate) fn btn_sm(id: impl Into<ElementId>, ghost: bool, disabled: bool, cx: &App) -> Stateful<Div> {
    let (p, g) = (pal(cx), geo(cx));
    let high = p.high;
    div().id(id).flex_none().flex().items_center().gap(u(6.)).py(u(3.)).px(u(9.)).rounded(u(g.r2)).border(u(1.)).border_color(p.line).text_color(p.text)
        .text_size(em(cx, 0.9 * 0.88)).whitespace_nowrap().when(!ghost, |d| d.bg(p.raised))
        .when(disabled, |d| d.opacity(0.45)).when(!disabled, |d| d.cursor_pointer().hover(move |s| s.bg(high)))
}
// .btn (full size)
pub(crate) fn btn(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
    let (p, g) = (pal(cx), geo(cx));
    let high = p.high;
    div().id(id).flex().items_center().gap(u(6.)).py(u(5.)).px(u(12.)).rounded(u(g.r2)).border(u(1.)).border_color(p.line).text_color(p.text).bg(p.raised)
        .whitespace_nowrap().cursor_pointer().hover(move |s| s.bg(high))
}
// .pane-empty: a title and a dim line, centred
pub(crate) fn empty(title: Option<&str>, sub: impl Into<SharedString>, cx: &App) -> Div {
    let p = pal(cx);
    div().flex_1().min_h_0().flex().items_center().justify_center().p(u(30.)).text_color(p.mid)
        .child(v_flex().gap(u(8.)).items_center().text_center()
            .when_some(title.map(String::from), |d, t| d.child(div().text_color(p.text).child(t)))
            .child(div().text_color(p.dim).child(sub.into())))
}
// the .url field around an Input: raised, the primary border while it has focus
pub(crate) fn url_box(focused: bool, raised: bool, cx: &App) -> Div {
    let p = pal(cx);
    div().flex_1().min_w_0().h(u(26.)).flex().items_center().rounded(u(5.)).border(u(1.))
        .border_color(if focused { p.primary } else { gpui_kit::transparent_black() }).when(raised, |d| d.bg(p.raised))
}
// .dn / .gutter numbers: the dim colour at 70%
pub(crate) fn dn_color(cx: &App) -> Hsla { fade(pal(cx).dim, 0.7) }
