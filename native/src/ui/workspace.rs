// The window, as main.ts laid it out: the sidebar window, the main one with its legends, the side one, the status line
// under all three. the keyboard, the palette and the session menus live here too
use super::agents::AgentsPage;
use super::chat::ChatView;
use super::home::Home;
use super::icons::ic;
use super::images::ImagesPage;
use super::overlay::{self, MenuItem, Tip as _};
use super::panes::{Pane, SidePanel};
use super::picker::{self, fuzzy, PickItem};
use super::settings::SettingsPage;
use super::theme::{em, geo, mix, pal, u};
use super::usage::UsagePage;
use crate::state::app::{Pith, View};
use crate::state::tab::{How, Kind, Level, Status, Tab};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::collections::{HashMap, HashSet};

actions!(pith, [NewSession, NewDesign, Palette, FindSession, CloseTab, ToggleSidebar, TogglePanel, TogglePane, OpenSettings, FocusComposer, StopRun, NextTab, PrevTab,
    SwitchModel, PickEffort, ScaleUp, ScaleDown, ScaleReset, ToggleFps, Pane1, Pane2, Pane3, Pane4, Pane5, Pane6, Quit]);

pub fn bind_keys(cx: &mut App) {
    let m = if cfg!(target_os = "macos") { "cmd" } else { "ctrl" };
    cx.bind_keys([
        KeyBinding::new(&format!("{m}-n"), NewSession, None),
        KeyBinding::new(&format!("{m}-shift-n"), NewDesign, None),
        KeyBinding::new(&format!("{m}-k"), Palette, None),
        KeyBinding::new(&format!("{m}-p"), FindSession, None),
        KeyBinding::new(&format!("{m}-w"), CloseTab, None),
        KeyBinding::new(&format!("{m}-b"), ToggleSidebar, None),
        KeyBinding::new(&format!("{m}-\\"), TogglePane, None),
        // the native build's panel toggle, kept: it brings back whichever pane was up
        KeyBinding::new(&format!("{m}-j"), TogglePanel, None),
        KeyBinding::new(&format!("{m}-,"), OpenSettings, None),
        KeyBinding::new(&format!("{m}-l"), FocusComposer, None),
        KeyBinding::new(&format!("{m}-m"), SwitchModel, None),
        KeyBinding::new(&format!("{m}-e"), PickEffort, None),
        KeyBinding::new(&format!("{m}-="), ScaleUp, None),
        KeyBinding::new(&format!("{m}-+"), ScaleUp, None),
        KeyBinding::new(&format!("{m}-shift-+"), ScaleUp, None),
        KeyBinding::new(&format!("{m}-shift-="), ScaleUp, None),
        KeyBinding::new(&format!("{m}--"), ScaleDown, None),
        KeyBinding::new(&format!("{m}-0"), ScaleReset, None),
        KeyBinding::new(&format!("{m}-shift-f"), ToggleFps, None),
        KeyBinding::new("alt-1", Pane1, None),
        KeyBinding::new("alt-2", Pane2, None),
        KeyBinding::new("alt-3", Pane3, None),
        KeyBinding::new("alt-4", Pane4, None),
        KeyBinding::new("alt-5", Pane5, None),
        KeyBinding::new("alt-6", Pane6, None),
        KeyBinding::new("ctrl-tab", NextTab, None),
        KeyBinding::new("ctrl-shift-tab", PrevTab, None),
        KeyBinding::new("escape", StopRun, Some("Workspace")),
        KeyBinding::new(&format!("{m}-q"), Quit, None),
    ]);
    super::overlay::bind_keys(cx);
    super::panes::bind_keys(cx);
}

pub const TOP: f32 = 44.;
const SIDE_W: f32 = 264.;

// panes.ts PANES: the order alt+1…6 and the legend buttons use
pub const PANES: [(Pane, &str, &str, &str); 6] = [
    (Pane::Browser, "web", "globe", "alt+1"), (Pane::Preview, "preview", "frame", "alt+2"), (Pane::Changes, "changes", "diff", "alt+3"),
    (Pane::Files, "files", "file", "alt+4"), (Pane::Agents, "agents", "agents", "alt+5"), (Pane::Terminal, "term", "terminal", "alt+6"),
];

// look.ts SCALES: the stops ctrl+= and ctrl+- step through, the same the settings page offers
pub const SCALES: [f64; 8] = [0.9, 1., 1.1, 1.25, 1.4, 1.5, 1.75, 2.];

pub struct Workspace {
    app: Entity<Pith>,
    home: Entity<Home>,
    settings: Option<Entity<SettingsPage>>,
    usage: Option<Entity<UsagePage>>,
    agents: Option<Entity<AgentsPage>>,
    images: Option<Entity<ImagesPage>>,
    side: Option<Entity<SidePanel>>,
    chats: HashMap<EntityId, Entity<ChatView>>,
    search: Entity<InputState>,
    query: String,
    // folded groups, and groups showing past their first six (in memory, like the webview)
    folded: HashSet<String>,
    show_all: HashSet<String>,
    focus: FocusHandle,
    status: Entity<super::status::StatusLine>,
    // which window has the active border: 0 sidebar, 1 main, 2 side. it follows clicks, like the compositor
    focus_win: u8,
    _subs: Vec<Subscription>,
}

// a sidebar row: a session on disk, an open tab, or both (sidebar.ts Row)
#[derive(Clone)]
struct Row { file: Option<String>, tab: Option<Entity<Tab>>, title: String, cwd: String, updated: f64, first: String }

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let app = Pith::global(cx);
        let home = cx.new(|cx| Home::new(window, cx));
        Home::focus(&home, window, cx);
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("search"));
        let focus = cx.focus_handle();
        overlay::init(focus.clone(), cx);
        let layer = overlay::layer(cx);
        let subs = vec![
            cx.observe_in(&app, window, |this, app, window, cx| {
                // drop views of closed tabs, keep the rest so drafts and scroll survive switching
                let live: HashSet<EntityId> = app.read(cx).tabs.iter().map(|t| t.entity_id()).collect();
                this.chats.retain(|k, _| live.contains(k));
                if let Some(t) = app.read(cx).active.clone() { this.chat_for(&t, window, cx); }
                cx.notify();
            }),
            cx.observe(&layer, |_, _, cx| cx.notify()),
            cx.subscribe_in(&search, window, |this, s, ev: &InputEvent, _, cx| if let InputEvent::Change = ev { this.query = s.read(cx).value().to_string(); cx.notify(); }),
            cx.observe_window_appearance(window, |_, window, cx| super::theme::apply(Some(window), cx)),
            // coming back to the window picks up sessions made elsewhere
            cx.observe_window_activation(window, |this, window, cx| if window.is_window_active() { this.app.update(cx, |a, cx| a.refresh_sessions(cx)); }),
        ];
        let status = cx.new(super::status::StatusLine::new);
        Workspace { app, home, settings: None, usage: None, agents: None, images: None, side: None, chats: HashMap::new(), search, query: String::new(),
            folded: Default::default(), show_all: Default::default(), focus, status, focus_win: 1, _subs: subs }
    }

    fn chat_for(&mut self, tab: &Entity<Tab>, window: &mut Window, cx: &mut Context<Self>) -> Entity<ChatView> {
        if let Some(c) = self.chats.get(&tab.entity_id()) { return c.clone(); }
        let c = cx.new(|cx| ChatView::new(tab.clone(), window, cx));
        ChatView::focus_composer(&c, window, cx);
        self.chats.insert(tab.entity_id(), c.clone());
        c
    }

    // the chat on screen, when there is one
    fn chat(&self, cx: &App) -> Option<(Entity<Tab>, Entity<ChatView>)> {
        let a = self.app.read(cx);
        if a.view != View::Chat { return None; }
        let t = a.active.clone()?;
        let c = self.chats.get(&t.entity_id())?.clone();
        Some((t, c))
    }

    // files dropped anywhere in the window land in the composer that's showing
    fn drop_paths(&mut self, paths: &ExternalPaths, window: &mut Window, cx: &mut Context<Self>) {
        let composer = match (self.app.read(cx).view, self.app.read(cx).active.clone()) {
            (View::Chat, Some(t)) => self.chats.get(&t.entity_id()).map(|c| c.read(cx).composer.clone()),
            (View::Home, _) => Some(self.home.read(cx).composer()),
            _ => None,
        };
        if let Some(c) = composer { c.update(cx, |c, cx| c.add_paths(paths.paths(), window, cx)); }
    }

    fn new_session(&mut self, kind: Kind, window: &mut Window, cx: &mut Context<Self>) {
        self.app.update(cx, |a, cx| a.new_home(kind, cx));
        Home::focus(&self.home.clone(), window, cx);
    }

    // panes.ts togglePane: the pane that's up closes, anything else opens
    fn toggle_pane(&mut self, pane: Pane, cx: &mut Context<Self>) {
        if self.app.read(cx).active.is_none() { return; }
        super::panes::toggle(pane, cx);
    }

    // look.ts stepScale: the next stop up or down, or back to 1
    fn step_scale(&mut self, dir: i32, window: &mut Window, cx: &mut Context<Self>) {
        let cur = self.app.read(cx).settings["scale"].as_f64().filter(|s| *s > 0.).unwrap_or(1.);
        let next = match dir { 0 => 1., d if d > 0 => SCALES.iter().copied().find(|x| *x > cur + 0.001).unwrap_or(cur), _ => SCALES.iter().rev().copied().find(|x| *x < cur - 0.001).unwrap_or(cur) };
        if (next - cur).abs() < 0.001 { return; }
        self.app.update(cx, |a, cx| a.save_settings(serde_json::json!({ "scale": next }), cx));
        super::theme::apply(Some(window), cx);
    }

    // ---------- palette (main.ts palette) ----------
    fn palette(&mut self, sessions_only: bool, window: &mut Window, cx: &mut Context<Self>) {
        let app = self.app.clone();
        let t = { let a = app.read(cx); if a.view == View::Chat { a.active.clone() } else { None } };
        let mut items: Vec<PickItem> = vec![];
        let w = cx.entity();
        let act = |label: &str, run: Box<dyn Fn(&mut Window, &mut App)>| PickItem::new(label.to_string(), move |w, cx| run(w, cx)).group("actions");
        if !sessions_only {
            items.push(act("new session", Box::new({ let w = w.clone(); move |window, cx| w.update(cx, |w, cx| w.new_session(Kind::Chat, window, cx)) })).keys("ctrl+n"));
            items.push(act("new design", Box::new({ let w = w.clone(); move |window, cx| w.update(cx, |w, cx| w.new_session(Kind::Design, window, cx)) })).keys("ctrl+shift+n"));
            if let Some(t) = t.clone() {
                items.push(act("switch model", Box::new(|window, cx| picker::models(window, cx))).keys("ctrl+m"));
                items.push(act("effort", Box::new({ let t = t.clone(); move |window, cx| picker::effort(t.clone(), window, cx) })).keys("ctrl+e"));
                items.push(act("mode", Box::new({ let t = t.clone(); move |window, cx| picker::modes(t.clone(), window, cx) })).keys("shift+tab"));
                for (pane, label, _, keys) in PANES {
                    items.push(act(&format!("{label} pane"), Box::new(move |_, cx| super::panes::open(pane, cx))).keys(keys));
                }
                for (label, cmd) in [("review changes", "/review"), ("security review", "/security-review"), ("simplify the changed code", "/simplify")] {
                    let t = t.clone();
                    items.push(act(label, Box::new(move |_, cx| run_command(&t, cmd, cx))).hint(cmd));
                }
                items.push(act("compact context", Box::new({ let t = t.clone(); move |_, cx| t.update(cx, |t, cx| t.compact(cx)) })).hint("/compact"));
                items.push(act("rename session", Box::new({ let t = t.clone(); move |window, cx| rename(t.clone(), window, cx) })));
                items.push(act("clone session", Box::new({ let t = t.clone(); move |_, cx| t.update(cx, |t, cx| t.clone_session(cx)) })));
                items.push(act("export as html", Box::new({ let t = t.clone(); move |_, cx| export_html(&t, cx) })));
                for c in t.read(cx).commands.clone() {
                    let t = t.clone();
                    let text = format!("/{} ", c.name);
                    // fills the composer, doesn't send
                    items.push(PickItem::new(format!("/{}", c.name), move |_, cx| t.update(cx, |_, cx| cx.emit(crate::state::tab::TabEvent::Draft(text.clone())))).hint(c.description.clone()).group("commands"));
                }
            }
            for (label, v) in [("usage", View::Usage), ("agents", View::Agents), ("images", View::Images)] {
                let a = app.clone();
                items.push(act(label, Box::new(move |_, cx| a.update(cx, |a, cx| a.show(v, cx)))));
            }
            items.push(act("settings", Box::new({ let a = app.clone(); move |_, cx| a.update(cx, |a, cx| a.show(View::Settings, cx)) })).keys("ctrl+,"));
            items.push(act("toggle sidebar", Box::new({ let a = app.clone(); move |_, cx| a.update(cx, |a, cx| { let s = !a.sidebar; a.save_settings(serde_json::json!({ "sidebar": s }), cx) }) })).keys("ctrl+b"));
        }
        for s in app.read(cx).sessions.iter().take(300) {
            let (a, f) = (app.clone(), s.file.clone());
            items.push(PickItem::new(session_title(s), move |_, cx| a.update(cx, |a, cx| a.open(&f, cx))).hint(base(&s.cwd)).right(ago(s.updated)).group("sessions"));
        }
        if sessions_only { picker::open_with("sessions", "find a session", items, window, cx) } else { picker::open_with("commands", "do something or find a session", items, window, cx) }
    }

    // ---------- sidebar (views/sidebar.ts) ----------
    fn rows(&self, cx: &App) -> Vec<Row> {
        let a = self.app.read(cx);
        let mut rows: Vec<Row> = a.sessions.iter().map(|s| Row { file: Some(s.file.clone()), tab: None, title: session_title(s), cwd: s.cwd.clone(), updated: s.updated, first: s.first_prompt.clone() }).collect();
        let mut at: HashMap<String, usize> = rows.iter().enumerate().map(|(i, r)| (r.file.clone().unwrap_or_default(), i)).collect();
        for t in &a.tabs {
            let tr = t.read(cx);
            let key = tr.file.clone().unwrap_or_else(|| tr.id.clone());
            let named = tr.items.iter().any(|i| matches!(i.kind, crate::state::tab::ItemKind::User { .. })) || tr.name.is_some() || tr.state["sessionName"].as_str().is_some_and(|s| !s.is_empty());
            match at.get(&key) {
                Some(&i) => {
                    let r = &mut rows[i];
                    if named { r.title = tr.title(); }
                    r.updated = r.updated.max(tr.sent);
                    r.tab = Some(t.clone());
                    r.cwd = tr.cwd.clone();
                }
                None => {
                    at.insert(key, rows.len());
                    rows.push(Row { file: tr.file.clone(), tab: Some(t.clone()), title: tr.title(), cwd: tr.cwd.clone(), updated: if tr.sent > 0. { tr.sent } else { tr.last_used_ms() }, first: String::new() });
                }
            }
        }
        rows
    }

    fn sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let (p, g) = (pal(cx), geo(cx));
        let view = self.app.read(cx).view;
        let q = self.query.trim().to_string();
        let mut rows = self.rows(cx);
        if q.is_empty() { rows.sort_by(|a, b| b.updated.partial_cmp(&a.updated).unwrap_or(std::cmp::Ordering::Equal)); } else {
            let mut sc: Vec<(Row, f32)> = rows.into_iter().map(|r| { let s = fuzzy(&q, &r.title).max(fuzzy(&q, &r.cwd) * 0.6).max(fuzzy(&q, &r.first) * 0.5); (r, s) }).filter(|x| x.1 > 0.).collect();
            sc.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            rows = sc.into_iter().map(|x| x.0).collect();
        }
        let pinned: HashSet<String> = self.app.read(cx).pinned().into_iter().collect();
        let is_pinned = |r: &Row| r.file.as_ref().is_some_and(|f| pinned.contains(f));
        let mut out: Vec<AnyElement> = vec![];
        let pins: Vec<Row> = rows.iter().filter(|r| is_pinned(r)).cloned().collect();
        if !pins.is_empty() && q.is_empty() {
            out.push(div().flex().items_baseline().gap(u(6.)).pt(u(12.)).px(u(6.)).pb(u(3.)).text_size(em(cx, 0.86)).text_color(p.text).child("pinned").into_any_element());
            for r in &pins { out.push(self.row(r, false, cx)); }
        }
        if !q.is_empty() {
            for r in rows.iter().take(60) { out.push(self.row(r, true, cx)); }
        } else {
            let mut order: Vec<String> = vec![];
            let mut groups: HashMap<String, Vec<Row>> = HashMap::new();
            for r in rows.into_iter().filter(|r| !is_pinned(r)) {
                if !groups.contains_key(&r.cwd) { order.push(r.cwd.clone()); }
                groups.entry(r.cwd.clone()).or_default().push(r);
            }
            for cwd in order {
                let rs = &groups[&cwd];
                let fold = self.folded.contains(&cwd);
                out.push(self.group(&cwd, fold, rs.iter().any(|r| r.tab.as_ref().is_some_and(|t| t.read(cx).running())), cx));
                if fold { continue; }
                let limit = if self.show_all.contains(&cwd) { rs.len() } else { 6 };
                for r in rs.iter().take(limit) { out.push(self.row(r, false, cx)); }
                if rs.len() > limit {
                    let c = cwd.clone();
                    let text = p.text;
                    out.push(div().id(SharedString::from(format!("more-{cwd}"))).w_full().pl(u(26.)).pr(u(8.)).py(u(2.)).text_size(em(cx, 0.84)).text_color(p.dim).cursor_pointer()
                        .hover(move |s| s.text_color(text)).child(format!("{} older", rs.len() - limit))
                        .on_click(cx.listener(move |this, _, _, cx| { this.show_all.insert(c.clone()); cx.notify(); })).into_any_element());
                }
            }
        }
        if out.is_empty() { out.push(div().py(u(20.)).px(u(10.)).text_color(p.dim).child(if q.is_empty() { "no sessions yet" } else { "nothing matches" }).into_any_element()); }

        let new_hover = mix(p.primary, 0.88, gpui_kit::white());
        let app = self.app.clone();
        div().flex_1().min_h_0().flex().flex_col().pt(u(12.))
            // .sb-top: new + search
            .child(div().flex().gap(u(6.)).pt(u(4.)).px(u(10.)).pb(u(8.))
                .child(div().id("sb-new").flex().flex_none().items_center().gap(u(5.)).px(u(10.)).py(u(4.)).rounded(u(g.r2)).bg(p.primary).text_color(p.on_primary).cursor_pointer()
                    .hover(move |s| s.bg(new_hover)).tip("new session (ctrl+n)", cx)
                    .child(ic("plus").size(em(cx, 1.2))).child("new")
                    .on_click(cx.listener(|this, _, window, cx| this.new_session(Kind::Chat, window, cx))))
                .child(div().flex_1().min_w_0().flex().items_center().gap(u(6.)).px(u(8.)).rounded(u(g.r2)).bg(p.raised).text_color(p.dim)
                    .child(ic("search").size(em(cx, 1.2)))
                    .child(div().flex_1().min_w_0().text_color(p.text).child(overlay::field(&self.search, 4., cx)))))
            .child(div().id("sb-list").flex_1().min_h_0().overflow_y_scroll().px(u(6.)).pb(u(10.)).children(out))
            // .sb-nav: two by two
            .child(div().grid().grid_cols(2).gap(u(2.)).p(u(6.)).border_t_1().border_color(p.rule)
                .children([("usage", "bars", View::Usage), ("agents", "agents", View::Agents), ("images", "image", View::Images), ("settings", "gear", View::Settings)].into_iter().map(|(label, icon, v)| {
                    let on = view == v;
                    let a = app.clone();
                    let (text, hover) = (p.text, p.hover);
                    div().id(label).flex().items_center().gap(u(7.)).px(u(8.)).py(u(5.)).rounded(u(g.r2)).text_size(em(cx, 0.9)).cursor_pointer()
                        .text_color(if on { p.accent } else { p.dim }).when(on, |d| d.bg(p.sel)).when(!on, |d| d.hover(move |s| s.text_color(text).bg(hover)))
                        .tip(label, cx)
                        .child(ic(icon).size(em(cx, 0.9 * 1.2))).child(label)
                        .on_click(move |_, _, cx| a.update(cx, |a, cx| a.show(v, cx)))
                })))
    }

    // .sb-grp: caret, folder name, its parent, a spinner while one of its sessions works, + on hover
    fn group(&self, cwd: &str, fold: bool, running: bool, cx: &mut Context<Self>) -> AnyElement {
        let p = pal(cx);
        let t = super::tools::tilde(cwd);
        let name = if t == "~" { "~".to_string() } else { base(cwd) };
        let parent = if t == "~" { String::new() } else { match t.rfind('/') { Some(i) => t[..i].to_string(), None => String::new() } };
        let (c1, c2, c3) = (cwd.to_string(), cwd.to_string(), cwd.to_string());
        let tip = format!("new session in {t}");
        let a = self.app.clone();
        let (text, hover) = (p.text, p.hover);
        div().id(SharedString::from(format!("grp-{cwd}"))).group("grp").flex().items_baseline().gap(u(6.)).pt(u(12.)).px(u(6.)).pb(u(3.)).text_size(em(cx, 0.86)).text_color(p.mid).cursor_pointer()
            .tip(c3, cx)
            .child(div().flex_none().w(em(cx, 0.86 * 0.6)).text_color(p.dim).child(if fold { "▸" } else { "▾" }))
            .child(div().flex_none().text_color(p.text).child(name))
            .child(div().flex_1().min_w_0().text_size(em(cx, 0.86 * 0.9)).text_color(p.dim).overflow_hidden().text_ellipsis().whitespace_nowrap().child(parent))
            .when(running, |d| d.child(div().text_color(p.accent).child(super::frame::spin(SharedString::from(format!("grun-{cwd}"))))))
            .child(div().id(SharedString::from(format!("gnew-{cwd}"))).flex_none().self_center().w(u(20.)).h(u(18.)).flex().items_center().justify_center().rounded(u(4.)).text_color(p.dim)
                .invisible().group_hover("grp", |s| s.visible()).hover(move |s| s.text_color(text).bg(hover))
                .tip(tip, cx)
                .child(ic("plus").size(em(cx, 0.86 * 1.2)))
                .on_click(move |_, _, cx| { cx.stop_propagation(); a.update(cx, |a, cx| { a.fresh(c2.clone(), Kind::Chat, cx); }); }))
            .on_click(cx.listener(move |this, _, _, cx| { if !this.folded.remove(&c1) { this.folded.insert(c1.clone()); } cx.notify(); }))
            .into_any_element()
    }

    // .sb-row: status glyph, title (◇ for designs), ago; the folder under the title while searching
    fn row(&self, r: &Row, with_cwd: bool, cx: &mut Context<Self>) -> AnyElement {
        let (p, g) = (pal(cx), geo(cx));
        let a = self.app.read(cx);
        let on = r.tab.is_some() && r.tab == a.active && a.view == View::Chat;
        let id = SharedString::from(format!("row-{}", r.file.clone().unwrap_or_else(|| r.tab.as_ref().map(|t| t.read(cx).id.clone()).unwrap_or_default())));
        let (status, unread, asks, design) = r.tab.as_ref().map(|t| { let t = t.read(cx); (t.status, t.unread, !t.asks.is_empty(), t.kind == Kind::Design) })
            .unwrap_or_else(|| (Status::Cold, false, false, r.file.as_ref().is_some_and(|f| a.meta[f.as_str()]["kind"] == "design")));
        let glyph: AnyElement = if asks {
            div().font_weight(FontWeight::BOLD).text_color(p.gold).child("?")
                .with_animation(SharedString::from(format!("q-{id}")), Animation::new(std::time::Duration::from_millis(1100)).repeat(), |d, t| d.opacity(if t < 0.5 { 1. } else { 0.25 })).into_any_element()
        } else if matches!(status, Status::Running | Status::Starting) { div().text_color(p.accent).child(super::frame::spin(SharedString::from(format!("sp-{id}")))).into_any_element() }
        else if status == Status::Dead { div().text_size(em(cx, 0.9)).text_color(p.red).child("×").into_any_element() }
        else if unread { div().text_size(em(cx, 0.7)).text_color(p.accent).child("●").into_any_element() }
        else if r.tab.is_some() && status == Status::Idle { div().text_size(em(cx, 0.9)).font_weight(FontWeight::BOLD).text_color(p.mid).child("·").into_any_element() }
        else { div().into_any_element() };
        let row = r.clone();
        let (text, hover, high) = (p.text, p.hover, p.high);
        let cwd = r.cwd.clone();
        let click = r.clone();
        let title = div().flex_1().min_w_0().flex().overflow_hidden().whitespace_nowrap()
            .when(design, |d| d.child(div().flex_none().text_color(p.tertiary).child("◇ ")))
            .child(div().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().child(r.title.clone()));
        let (ago_sz, ago_txt, dim) = (em(cx, 0.78), ago(r.updated), p.dim);
        let ago_el = move || div().flex_none().text_size(ago_sz).text_color(dim).child(ago_txt.clone());
        // the grid: glyph, title, ago; searching puts the folder under the title and ago beside it, a row down
        let line = |d: Div| d.flex().items_center().gap(u(4.));
        div().id(id).flex().flex_col().gap(u(4.)).pl(u(6.)).pr(u(8.)).py(u(4.)).rounded(u(g.r2)).cursor_pointer()
            .text_color(if on { p.text } else { p.mid })
            .when(on, |d| d.bg(high)).when(!on, |d| d.hover(move |s| s.bg(hover).text_color(text)))
            .tip(cwd, cx)
            .child(line(div()).child(div().flex_none().w(u(16.)).flex().justify_center().text_color(p.dim).child(glyph)).child(title).when(!with_cwd, |d| d.child(ago_el())))
            .when(with_cwd, |d| d.child(line(div()).child(div().flex_none().w(u(16.)))
                .child(div().flex_1().min_w_0().text_size(em(cx, 0.8)).text_color(p.dim).overflow_hidden().text_ellipsis().whitespace_nowrap().child(base(&r.cwd)))
                .child(ago_el())))
            .on_click(move |_, _, cx| { let a = Pith::global(cx); a.update(cx, |a, cx| match (&click.tab, &click.file) { (Some(t), _) => a.focus(t.clone(), cx), (None, Some(f)) => a.open(f, cx), _ => {} }); })
            .on_mouse_down(MouseButton::Right, move |e, _, cx| row_menu(&row, e.position, cx))
            .into_any_element()
    }

    // ---------- the main window's legends: what's open on the left, the panes and a menu on the right ----------
    fn legends(&mut self, focused: bool, cx: &mut Context<Self>) -> (Div, Option<Div>) {
        use super::frame::{legend, legend_btn, legend_sub, legend_t, legend_tag, Side};
        let gutter = pal(cx).gutter;
        let app = self.app.read(cx);
        let t = if app.view == View::Chat { app.active.clone() } else { None };
        let Some(t) = t else {
            let name = match app.view { View::Settings => "settings", View::Usage => "usage", View::Agents => "agents", View::Images => "images", _ => "home" };
            return (legend(Side::L, gutter, cx).child(legend_t(name, focused, cx)), None);
        };
        let (title, cwd, design) = { let t = t.read(cx); (t.title(), super::tools::tilde(&t.cwd), t.kind == Kind::Design) };
        let cur = t.read(cx).pane;
        let t1 = t.clone();
        let l = legend(Side::L, gutter, cx).max_w(relative(1.)).mr(u(210.))
            .when(design, |d| d.child(legend_tag("design", cx)))
            .child(legend_t(title, focused, cx).id("leg-title").flex_none().max_w(em(cx, 0.84 * 46. * 0.6))
                .tip("double-click to rename", cx)
                .on_click(move |e, window, cx| if e.click_count() == 2 { rename(t1.clone(), window, cx) }))
            .child(legend_sub(cwd, cx));
        let fsz = em(cx, 0.84 * 1.45);
        let t2 = t.clone();
        let r = legend(Side::R, gutter, cx)
            .children(PANES.into_iter().map(|(pane, label, icon, keys)| {
                let on = cur == Some(pane);
                let tip = format!("{label} ({keys})");
                legend_btn(SharedString::from(format!("lp-{label}")), on, cx).px(u(4.)).tip(tip, cx)
                    .child(ic(icon).size(fsz))
                    .on_click(move |_, _, cx| super::panes::toggle(pane, cx))
            }))
            .child(legend_btn("lp-more", false, cx).px(u(4.)).tip("more", cx).child(ic("more").size(fsz))
                .on_mouse_down(MouseButton::Left, move |e, _, cx| { cx.stop_propagation(); session_menu(&t2, e.position, cx) }));
        (l, Some(r))
    }
}

// pi's own commands from the palette run straight away in that session (act.runCommand)
fn run_command(t: &Entity<Tab>, text: &str, cx: &mut App) {
    let app = Pith::global(cx);
    let (active, chat) = { let a = app.read(cx); (a.active.as_ref() == Some(t), a.view == View::Chat) };
    if !active || !chat { app.update(cx, |a, cx| a.focus(t.clone(), cx)); }
    t.update(cx, |t, cx| t.send(text.to_string(), vec![], How::Auto, cx));
}

fn open_path(p: &str, cx: &mut App) { cx.open_with_system(std::path::Path::new(p)); }

fn export_html(t: &Entity<Tab>, cx: &mut App) {
    t.update(cx, |t, cx| t.export_html(cx, |_, p, cx| if let Some(p) = p { open_path(&p, cx) }));
}

fn copy_last(t: &Entity<Tab>, cx: &mut App) {
    let f = t.read(cx).last_text();
    cx.spawn(async move |cx| {
        let x = f.await;
        if x.is_empty() { return; }
        let _ = cx.update(|cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(x));
            Pith::global(cx).update(cx, |a, cx| a.toast(Level::Info, "copied", cx));
        });
    }).detach();
}

// the ⋯ in the main window's border (main.ts sessionMenu)
fn session_menu(t: &Entity<Tab>, at: Point<Pixels>, cx: &mut App) {
    let (file, cwd, cold) = { let t = t.read(cx); (t.file.clone(), t.cwd.clone(), t.status == Status::Cold) };
    let mut items = vec![
        MenuItem::new("rename", { let t = t.clone(); move |window, cx| rename(t.clone(), window, cx) }),
        MenuItem::new("compact context", { let t = t.clone(); move |_, cx| t.update(cx, |t, cx| t.compact(cx)) }).hint("/compact"),
        MenuItem::new("clone into a new session", { let t = t.clone(); move |_, cx| t.update(cx, |t, cx| t.clone_session(cx)) }),
        MenuItem::new("export as html", { let t = t.clone(); move |_, cx| export_html(&t, cx) }),
        MenuItem::new("copy last reply", { let t = t.clone(); move |_, cx| copy_last(&t, cx) }),
    ];
    if let Some(f) = file { items.push(MenuItem::new("show session file", move |_, cx| cx.reveal_path(std::path::Path::new(&f)))); }
    items.push(MenuItem::new("open folder", move |_, cx| open_path(&cwd, cx)));
    if !cold { items.push(MenuItem::new("stop process", { let t = t.clone(); move |_, cx| t.update(cx, |t, cx| t.stop(cx)) }).hint("comes back when you send")); }
    items.push(MenuItem::new("close", { let t = t.clone(); move |_, cx| { let a = Pith::global(cx); a.update(cx, |a, cx| a.close(&t, cx)); } }));
    overlay::menu(at, items, cx);
}

// right-click on a sidebar row (sidebar.ts context)
fn row_menu(r: &Row, at: Point<Pixels>, cx: &mut App) {
    let pinned = r.file.as_ref().is_some_and(|f| Pith::global(cx).read(cx).pinned().contains(f));
    let mut items = vec![MenuItem::new("rename", { let r = r.clone(); move |window, cx| {
        let r = r.clone();
        prompt_rename(&r.title.clone(), move |name, _, cx| {
            let app = Pith::global(cx);
            let t = r.tab.clone().or_else(|| { let f = r.file.clone()?; app.update(cx, |a, cx| a.open(&f, cx)); app.read(cx).tabs.iter().find(|t| t.read(cx).file.as_deref() == Some(&f)).cloned() });
            if let Some(t) = t { t.update(cx, |t, cx| t.rename(name, cx)); }
            app.update(cx, |a, cx| a.refresh_sessions(cx));
        }, window, cx);
    } })];
    if let Some(f) = r.file.clone() { items.push(MenuItem::new(if pinned { "unpin" } else { "pin" }, move |_, cx| Pith::global(cx).update(cx, |a, cx| a.toggle_pin(&f, cx)))); }
    let cwd = r.cwd.clone();
    items.push(MenuItem::new("new session here", { let c = cwd.clone(); move |_, cx| { Pith::global(cx).update(cx, |a, cx| { a.fresh(c.clone(), Kind::Chat, cx); }); } }));
    items.push(MenuItem::new("open folder", move |_, cx| open_path(&cwd, cx)));
    if let Some(t) = r.tab.clone() {
        if t.read(cx).status != Status::Cold { let t2 = t.clone(); items.push(MenuItem::new("stop process", move |_, cx| t2.update(cx, |t, cx| t.stop(cx)))); }
        items.push(MenuItem::new("close", move |_, cx| Pith::global(cx).update(cx, |a, cx| a.close(&t, cx))));
    }
    if let Some(f) = r.file.clone() { items.push(MenuItem::new("move to trash", move |_, cx| Pith::global(cx).update(cx, |a, cx| a.trash(&f, cx))).danger()); }
    overlay::menu(at, items, cx);
}

fn prompt_rename(cur: &str, ok: impl Fn(String, &mut Window, &mut App) + 'static, window: &mut Window, cx: &mut App) {
    overlay::prompt("rename session", cur, ok, window, cx);
}

pub fn rename(tab: Entity<Tab>, window: &mut Window, cx: &mut App) {
    let cur = tab.read(cx).title();
    prompt_rename(&cur, move |name, _, cx| {
        tab.update(cx, |t, cx| t.rename(name, cx));
        Pith::global(cx).update(cx, |a, cx| a.refresh_sessions(cx));
    }, window, cx);
}

pub fn session_title(s: &crate::core::sessions::SessionInfo) -> String {
    s.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| crate::state::tab::title_from(&s.first_prompt))
}

// fmt.ts base: the last path segment
pub fn base(p: &str) -> String { let t = p.trim_end_matches('/'); t.rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or(p).to_string() }

// fmt.ts ago: now, 5m, 3h, 2d, then the date
pub fn ago(ms: f64) -> String {
    let s = ((crate::state::tab::now_ms() - ms) / 1000.).max(0.);
    if s < 45. { return "now".into(); }
    if s < 3600. { return format!("{}m", (s / 60.).round()); }
    if s < 86400. { return format!("{}h", (s / 3600.).round()); }
    if s < 86400. * 7. { return format!("{}d", (s / 86400.).round()); }
    let (y, m, d) = local_date(ms);
    let (ny, _, _) = local_date(crate::state::tab::now_ms());
    const M: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    if y != ny { format!("{} {}, {:02}", M[m as usize - 1], d, y % 100) } else { format!("{} {}", M[m as usize - 1], d) }
}

fn local_date(ms: f64) -> (i32, u32, u32) {
    let t = (ms / 1000.) as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&t, &mut tm) };
    (tm.tm_year + 1900, tm.tm_mon as u32 + 1, tm.tm_mday as u32)
}

impl Workspace {
    // #app's grid: the sidebar window, the main one, the side one, the status line under all three
    fn windows(&mut self, sidebar: bool, side_open: bool, side_w: f32, content: AnyElement, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (p, g) = (pal(cx), geo(cx));
        let fw = self.focus_win;
        let mut row = div().flex().flex_row().flex_1().min_h_0().gap(u(g.gap)).items_stretch();
        if sidebar {
            let body = self.sidebar(cx);
            row = row.child(super::frame::win(cx).id("win-side").w(u(SIDE_W)).flex_none()
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| { if this.focus_win != 0 { this.focus_win = 0; cx.notify(); } }))
                .child(body)
                .child(super::frame::edge(super::frame::win_color(fw == 0, cx), cx))
                .child(super::frame::legend(super::frame::Side::L, p.gutter, cx).child(super::frame::legend_t("pith", fw == 0, cx))));
        }
        let (l, r) = self.legends(fw == 1, cx);
        row = row.child(super::frame::win(cx).id("win-main").flex_1().min_w_0()
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| { if this.focus_win != 1 { this.focus_win = 1; cx.notify(); } }))
            .child(div().flex_1().min_h_0().flex().flex_col().overflow_hidden().rounded(u(g.round)).child(content))
            .child(super::frame::edge(super::frame::win_color(fw == 1, cx), cx))
            .child(l).children(r));
        if side_open {
            let side = self.side.get_or_insert_with(|| cx.new(|cx| SidePanel::new(window, cx))).clone();
            row = row.child(super::frame::win(cx).id("win-pane").w(u(side_w)).flex_none()
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| { if this.focus_win != 2 { this.focus_win = 2; cx.notify(); } }))
                .child(div().flex_1().min_h_0().flex().flex_col().overflow_hidden().rounded(u(g.round)).child(side))
                .child(super::frame::edge(super::frame::win_color(fw == 2, cx), cx))
                .map(|d| { let (l, r) = SidePanel::legends(cx); d.child(l).child(r) }));
        }
        div().flex().flex_col().flex_1().min_h_0().gap(u(g.gap)).child(row).child(div().h(u(26.)).flex_none().child(self.status.clone()))
    }

    // typing with nothing focused lands in the composer (main.ts: keys on body focus it)
    fn type_anywhere(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &e.keystroke;
        if k.modifiers.control || k.modifiers.alt || k.modifiers.platform || overlay::is_open(cx) { return; }
        let Some(ch) = k.key_char.as_ref() else { return };
        if ch.chars().count() != 1 || ch.chars().any(|c| c.is_control()) { return; }
        if !(self.focus.is_focused(window) || window.focused(cx).is_none()) { return; }
        let Some((_, c)) = self.chat(cx) else { return };
        ChatView::focus_composer(&c, window, cx);
        let ks = k.clone();
        window.defer(cx, move |window, cx| { window.dispatch_keystroke(ks, cx); });
        cx.stop_propagation();
    }
}

// while something floats, the window's own keys stand back and let it have them (main.ts returns early too)
macro_rules! guarded {
    ($cx:ident, $act:ty, |$this:ident, $w:pat_param, $c:ident| $body:expr) => {
        $cx.listener(move |$this: &mut Workspace, _: &$act, $w: &mut Window, $c: &mut Context<Workspace>| { if overlay::is_open($c) { $c.propagate(); return; } $body; })
    };
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let app = self.app.read(cx);
        let view = app.view;
        let sidebar = app.sidebar;
        let active = app.active.clone();
        let (side_open, side_w) = (app.side && view == View::Chat, app.side_w);
        let content: AnyElement = match (view, active) {
            (View::Chat, Some(t)) => self.chat_for(&t, window, cx).into_any_element(),
            (View::Settings, _) => self.settings.get_or_insert_with(|| cx.new(|cx| SettingsPage::new(window, cx))).clone().into_any_element(),
            (View::Usage, _) => self.usage.get_or_insert_with(|| cx.new(|cx| UsagePage::new(window, cx))).clone().into_any_element(),
            (View::Agents, _) => self.agents.get_or_insert_with(|| cx.new(|cx| AgentsPage::new(window, cx))).clone().into_any_element(),
            (View::Images, _) => self.images.get_or_insert_with(|| cx.new(|cx| ImagesPage::new(window, cx))).clone().into_any_element(),
            _ => self.home.clone().into_any_element(),
        };
        let (p, g) = (pal(cx), geo(cx));
        div().id("workspace").key_context("Workspace").track_focus(&self.focus).size_full().relative().flex().flex_col().bg(p.gutter).text_color(p.text)
            .font_family(g.mono.clone()).font_weight(g.weight).text_size(u(g.fs)).line_height(relative(1.55))
            .pt(u(g.gap + 4.)).px(u(g.gap)).gap(u(g.gap))
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| this.type_anywhere(e, window, cx)))
            .on_action(guarded!(cx, NewSession, |this, window, cx| this.new_session(Kind::Chat, window, cx)))
            .on_action(guarded!(cx, NewDesign, |this, window, cx| this.new_session(Kind::Design, window, cx)))
            .on_action(guarded!(cx, Palette, |this, window, cx| this.palette(false, window, cx)))
            .on_action(guarded!(cx, FindSession, |this, window, cx| this.palette(true, window, cx)))
            .on_action(guarded!(cx, SwitchModel, |this, window, cx| if this.chat(cx).map(|x| x.0).is_some() { picker::models(window, cx) }))
            .on_action(guarded!(cx, PickEffort, |this, window, cx| if let Some(t) = this.chat(cx).map(|x| x.0) { picker::effort(t, window, cx) }))
            .on_action(guarded!(cx, OpenSettings, |this, _, cx| this.app.update(cx, |a, cx| a.show(View::Settings, cx))))
            .on_action(guarded!(cx, ToggleSidebar, |this, _, cx| this.app.update(cx, |a, cx| { let s = !a.sidebar; a.save_settings(serde_json::json!({ "sidebar": s }), cx) })))
            .on_action(guarded!(cx, CloseTab, |this, _, cx| { let t = this.app.read(cx).active.clone(); if let Some(t) = t { this.app.update(cx, |a, cx| a.close(&t, cx)); } }))
            .on_action(cx.listener(|this, _: &StopRun, window, cx| {
                if overlay::is_open(cx) { overlay::close(window, cx); return; }
                // esc in the sidebar's search clears it and lets go
                if this.search.read(cx).focus_handle(cx).is_focused(window) {
                    this.search.update(cx, |s, cx| s.set_value("", window, cx));
                    this.query.clear();
                    this.focus.focus(window, cx);
                    cx.notify();
                    return;
                }
                if let Some(t) = this.chat(cx).map(|x| x.0) { if t.read(cx).running() { t.update(cx, |t, cx| t.abort(cx)); return; } }
                cx.propagate();
            }))
            .on_action(guarded!(cx, FocusComposer, |this, window, cx| { if let Some((_, c)) = this.chat(cx) { ChatView::focus_composer(&c, window, cx); } }))
            .on_action(guarded!(cx, TogglePanel, |this, _, cx| this.app.update(cx, |a, cx| { a.side = !a.side; cx.notify(); })))
            .on_action(guarded!(cx, TogglePane, |this, _, cx| {
                if this.app.read(cx).side { this.app.update(cx, |a, cx| { a.side = false; cx.notify(); }); } else if this.app.read(cx).active.is_some() { super::panes::open(Pane::Browser, cx); }
            }))
            .on_action(guarded!(cx, Pane1, |this, _, cx| this.toggle_pane(PANES[0].0, cx)))
            .on_action(guarded!(cx, Pane2, |this, _, cx| this.toggle_pane(PANES[1].0, cx)))
            .on_action(guarded!(cx, Pane3, |this, _, cx| this.toggle_pane(PANES[2].0, cx)))
            .on_action(guarded!(cx, Pane4, |this, _, cx| this.toggle_pane(PANES[3].0, cx)))
            .on_action(guarded!(cx, Pane5, |this, _, cx| this.toggle_pane(PANES[4].0, cx)))
            .on_action(guarded!(cx, Pane6, |this, _, cx| this.toggle_pane(PANES[5].0, cx)))
            .on_action(guarded!(cx, ScaleUp, |this, window, cx| this.step_scale(1, window, cx)))
            .on_action(guarded!(cx, ScaleDown, |this, window, cx| this.step_scale(-1, window, cx)))
            .on_action(guarded!(cx, ScaleReset, |this, window, cx| this.step_scale(0, window, cx)))
            .on_action(cx.listener(|_, _: &ToggleFps, _, cx| overlay::toggle_fps(cx)))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| this.drop_paths(paths, window, cx)))
            .on_action(guarded!(cx, NextTab, |this, _, cx| cycle(&this.app, 1, cx)))
            .on_action(guarded!(cx, PrevTab, |this, _, cx| cycle(&this.app, -1, cx)))
            .child(self.windows(sidebar, side_open, side_w, content, window, cx))
            .child(overlay::render(window, cx))
    }
}

fn cycle(app: &Entity<Pith>, d: isize, cx: &mut App) {
    let a = app.read(cx);
    if a.tabs.is_empty() { return; }
    let i = a.active.as_ref().and_then(|t| a.tabs.iter().position(|x| x == t)).map(|i| i as isize).unwrap_or(-1);
    let n = a.tabs.len() as isize;
    let next = a.tabs[((i + d).rem_euclid(n)) as usize].clone();
    app.update(cx, |a, cx| a.focus(next, cx));
}
