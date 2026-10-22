// The window: sessions down the left, the session or page you're on to the right, a header over it.
// on macos the traffic lights sit in the sidebar's top strip
use super::chat::ChatView;
use super::home::Home;
use super::picker::{self, PickItem};
use super::agents::AgentsPage;
use super::images::ImagesPage;
use super::panes::SidePanel;
use super::settings::SettingsPage;
use super::usage::UsagePage;
use super::theme::pal;
use crate::core::sessions::SessionInfo;
use crate::state::app::{Pith, View};
use crate::state::tab::{Kind, Level, Status, Tab};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Icon, Sizable};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::assets::IconName;
use std::collections::{BTreeMap, HashMap};

actions!(pith, [NewSession, Palette, CloseTab, ToggleSidebar, TogglePanel, OpenSettings, FocusComposer, StopRun, NextTab, PrevTab, SwitchModel, Quit]);

pub fn bind_keys(cx: &mut App) {
    let m = if cfg!(target_os = "macos") { "cmd" } else { "ctrl" };
    cx.bind_keys([
        KeyBinding::new(&format!("{m}-n"), NewSession, None),
        KeyBinding::new(&format!("{m}-k"), Palette, None),
        KeyBinding::new(&format!("{m}-p"), Palette, None),
        KeyBinding::new(&format!("{m}-w"), CloseTab, None),
        KeyBinding::new(&format!("{m}-b"), ToggleSidebar, None),
        KeyBinding::new(&format!("{m}-j"), TogglePanel, None),
        KeyBinding::new(&format!("{m}-,"), OpenSettings, None),
        KeyBinding::new(&format!("{m}-l"), FocusComposer, None),
        KeyBinding::new(&format!("{m}-m"), SwitchModel, None),
        KeyBinding::new("ctrl-tab", NextTab, None),
        KeyBinding::new("ctrl-shift-tab", PrevTab, None),
        KeyBinding::new("escape", StopRun, Some("Workspace")),
        KeyBinding::new(&format!("{m}-q"), Quit, None),
    ]);
    super::panes::bind_keys(cx);
}

pub const TOP: f32 = 44.;
const SIDE_W: f32 = 264.;

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
    collapsed: std::collections::HashSet<String>,
    focus: FocusHandle,
    _subs: Vec<Subscription>,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let app = Pith::global(cx);
        let home = cx.new(|cx| Home::new(window, cx));
        Home::focus(&home, window, cx);
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let subs = vec![
            cx.observe_in(&app, window, |this, app, window, cx| {
                // drop views of closed tabs, keep the rest so drafts and scroll survive switching
                let live: std::collections::HashSet<EntityId> = app.read(cx).tabs.iter().map(|t| t.entity_id()).collect();
                this.chats.retain(|k, _| live.contains(k));
                if let Some(t) = app.read(cx).active.clone() { this.chat_for(&t, window, cx); }
                cx.notify();
            }),
            cx.subscribe_in(&search, window, |this, s, ev: &InputEvent, _, cx| if let InputEvent::Change = ev { this.query = s.read(cx).value().to_string(); cx.notify(); }),
            cx.observe_window_appearance(window, |_, window, cx| super::theme::apply(Some(window), cx)),
        ];
        Workspace { app, home, settings: None, usage: None, agents: None, images: None, side: None, chats: HashMap::new(), search, query: String::new(), collapsed: Default::default(), focus: cx.focus_handle(), _subs: subs }
    }

    fn chat_for(&mut self, tab: &Entity<Tab>, window: &mut Window, cx: &mut Context<Self>) -> Entity<ChatView> {
        if let Some(c) = self.chats.get(&tab.entity_id()) { return c.clone(); }
        let c = cx.new(|cx| ChatView::new(tab.clone(), window, cx));
        ChatView::focus_composer(&c, window, cx);
        self.chats.insert(tab.entity_id(), c.clone());
        c
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

    fn new_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.app.update(cx, |a, cx| a.show(View::Home, cx));
        Home::focus(&self.home.clone(), window, cx);
    }

    fn palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let app = self.app.clone();
        let mut items = vec![
            PickItem::new("New session", { let w = cx.entity(); move |window, cx| w.update(cx, |w, cx| w.new_session(window, cx)) }).group("Go").icon(IconName::SquarePen).keys("secondary-n"),
            PickItem::new("Settings", { let a = app.clone(); move |_, cx| a.update(cx, |a, cx| a.show(View::Settings, cx)) }).group("Go").icon(IconName::Settings).keys("secondary-,"),
            PickItem::new("Toggle sidebar", { let a = app.clone(); move |_, cx| a.update(cx, |a, cx| { let s = !a.sidebar; a.save_settings(serde_json::json!({ "sidebar": s }), cx) }) }).group("View").icon(IconName::PanelLeft).keys("secondary-b"),
        ];
        if let Some(t) = app.read(cx).active.clone() {
            let (t1, t2, t3) = (t.clone(), t.clone(), t.clone());
            items.push(PickItem::new("Switch model…", |window, cx| picker::models(window, cx)).group("Session").icon(IconName::Bot).keys("secondary-m"));
            items.push(PickItem::new("Compact context", move |_, cx| t1.update(cx, |t, cx| t.compact(cx))).group("Session").icon(IconName::Shrink));
            items.push(PickItem::new("Start over in this session", move |_, cx| t2.update(cx, |t, cx| t.new_session(cx))).group("Session").icon(IconName::Eraser));
            items.push(PickItem::new("Rename session…", move |window, cx| rename(t3.clone(), window, cx)).group("Session").icon(IconName::PenLine));
            let cmds = t.read(cx).commands.clone();
            for c in cmds {
                let t = t.clone();
                let name = format!("/{}", c.name);
                let n2 = name.clone();
                items.push(PickItem::new(name, move |_, cx| t.update(cx, |t, cx| t.send(n2.clone(), vec![], crate::state::tab::How::Auto, cx))).hint(c.description.clone()).group("Commands").icon(IconName::Slash));
            }
        }
        for s in app.read(cx).sessions.iter().take(200) {
            let (a, f) = (app.clone(), s.file.clone());
            items.push(PickItem::new(session_title(s), move |_, cx| a.update(cx, |a, cx| a.open(&f, cx))).group("Recent sessions").icon(IconName::MessageSquare).hint(super::tools::tilde(&s.cwd)).right(ago(s.updated)));
        }
        picker::open(items, "Search sessions and commands", window, cx);
    }

    // ---------- sidebar ----------
    fn sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let (tabs, sessions, active, view) = { let a = self.app.read(cx); (a.tabs.clone(), a.sessions.clone(), a.active.clone(), a.view) };
        let p = pal(cx);
        let q = self.query.to_lowercase();
        let active_file = active.as_ref().and_then(|t| t.read(cx).file.clone());
        let active_id = active.as_ref().map(|t| t.entity_id());
        let open: HashMap<String, Entity<Tab>> = tabs.iter().filter_map(|t| Some((t.read(cx).file.clone()?, t.clone()))).collect();
        let on_chat = view == View::Chat;

        // open tabs that aren't on disk yet go first, then everything else by project, newest project first
        let mut rows: Vec<AnyElement> = vec![];
        let fresh: Vec<Entity<Tab>> = tabs.iter().filter(|t| t.read(cx).file.as_ref().map_or(true, |f| !sessions.iter().any(|s| &s.file == f))).cloned().collect();
        for t in fresh {
            let title = t.read(cx).title();
            if !q.is_empty() && !title.to_lowercase().contains(&q) { continue; }
            let on = on_chat && active_id == Some(t.entity_id());
            rows.push(self.row(SharedString::from(format!("fresh-{}", t.entity_id())), title, String::new(), on, Some(t.clone()), RowClick::Tab(t), cx));
        }
        let mut groups: BTreeMap<String, Vec<&SessionInfo>> = BTreeMap::new();
        for s in sessions.iter() {
            if !q.is_empty() && !session_title(s).to_lowercase().contains(&q) && !s.cwd.to_lowercase().contains(&q) { continue; }
            groups.entry(s.cwd.clone()).or_default().push(s);
        }
        let mut order: Vec<(String, f64)> = groups.iter().map(|(k, v)| (k.clone(), v.iter().map(|s| s.updated).fold(0., f64::max))).collect();
        order.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        for (cwd, _) in order {
            let list = &groups[&cwd];
            let name = std::path::Path::new(&cwd).file_name().map(|n| n.to_string_lossy().to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| super::tools::tilde(&cwd));
            let collapsed = self.collapsed.contains(&cwd) && q.is_empty();
            let (c1, c2) = (cwd.clone(), cwd.clone());
            let a = self.app.clone();
            rows.push(h_flex().id(SharedString::from(format!("grp-{cwd}"))).group("grp").h(px(28.)).mt_2().px_2().gap_1().items_center().text_xs().text_color(p.dim).cursor_pointer()
                .child(Icon::new(if collapsed { IconName::ChevronRight } else { IconName::ChevronDown }).xsmall())
                .child(div().font_weight(FontWeight::MEDIUM).text_color(p.mid).truncate().child(name))
                .child(div().flex_1())
                .child(div().invisible().group_hover("grp", |s| s.visible()).child(Button::new(SharedString::from(format!("new-{cwd}"))).icon(IconName::Plus).ghost().xsmall().tooltip("New session here")
                    .on_click(move |_, _, cx| { cx.stop_propagation(); a.update(cx, |a, cx| { a.fresh(c2.clone(), Kind::Chat, cx); }); })))
                .on_click(cx.listener(move |this, _, _, cx| { if !this.collapsed.remove(&c1) { this.collapsed.insert(c1.clone()); } cx.notify(); }))
                .into_any_element());
            if collapsed { continue; }
            for s in list.iter() {
                let on = on_chat && active_file.as_deref() == Some(&s.file);
                let tab = open.get(&s.file).cloned();
                let title = tab.as_ref().map(|t| t.read(cx).title()).unwrap_or_else(|| session_title(s));
                rows.push(self.row(SharedString::from(s.file.clone()), title, ago(s.updated), on, tab, RowClick::File(s.file.clone()), cx));
            }
        }
        if rows.is_empty() {
            rows.push(div().p_4().text_sm().text_color(p.dim).child(if q.is_empty() { "No sessions yet" } else { "Nothing matches" }).into_any_element());
        }

        let a1 = self.app.clone();
        let mac = cfg!(target_os = "macos");
        v_flex().w(px(SIDE_W)).h_full().flex_none().bg(cx.theme().sidebar).border_r_1().border_color(cx.theme().sidebar_border)
            .child(
                // the strip the window is dragged by, with the traffic lights on macos
                h_flex().h(px(TOP)).flex_none().pl(px(if mac { 84. } else { 14. })).pr_2().items_center().window_control_area(WindowControlArea::Drag)
                    .on_mouse_down(MouseButton::Left, |_, window, _| if !cfg!(target_os = "macos") { window.start_window_move() })
                    .when(!mac, |d| d.child(div().font_weight(FontWeight::SEMIBOLD).text_color(cx.theme().foreground).child("pith")))
                    .child(div().flex_1())
                    .child(Button::new("sb-new").icon(IconName::SquarePen).ghost().small().tooltip("New session").on_click(cx.listener(|this, _, window, cx| this.new_session(window, cx)))),
            )
            .child(div().px_2().pb_2().child(Input::new(&self.search).small().prefix(Icon::new(IconName::Search).xsmall().text_color(p.dim)).cleanable(true)))
            .child(div().id("sb-list").flex_1().min_h_0().overflow_y_scroll().px_2().pb_2().children(rows))
            .child(
                v_flex().p_2().gap_0p5().border_t_1().border_color(cx.theme().sidebar_border)
                    .child(nav("nav-agents", IconName::Bot, "Agents", view == View::Agents, { let a = self.app.clone(); move |_, _, cx| a.update(cx, |a, cx| a.show(View::Agents, cx)) }, cx))
                    .child(nav("nav-images", IconName::Image, "Images", view == View::Images, { let a = self.app.clone(); move |_, _, cx| a.update(cx, |a, cx| a.show(View::Images, cx)) }, cx))
                    .child(nav("nav-usage", IconName::ChartColumn, "Usage", view == View::Usage, { let a = self.app.clone(); move |_, _, cx| a.update(cx, |a, cx| a.show(View::Usage, cx)) }, cx))
                    .child(nav("nav-settings", IconName::Settings, "Settings", view == View::Settings, move |_, _, cx| a1.update(cx, |a, cx| a.show(View::Settings, cx)), cx)),
            )
    }

    fn row(&self, id: SharedString, title: String, right: String, on: bool, tab: Option<Entity<Tab>>, click: RowClick, cx: &mut Context<Self>) -> AnyElement {
        let p = pal(cx);
        let (status, unread, asks) = tab.as_ref().map(|t| { let t = t.read(cx); (t.status, t.unread, !t.asks.is_empty()) }).unwrap_or((Status::Cold, false, false));
        let mark: AnyElement = if asks { div().size(px(7.)).rounded_full().bg(p.gold).into_any_element() }
            else if status == Status::Running || status == Status::Starting { Spinner::new().xsmall().color(cx.theme().primary).into_any_element() }
            else if status == Status::Dead { div().size(px(7.)).rounded_full().bg(p.red).into_any_element() }
            else if unread { div().size(px(7.)).rounded_full().bg(cx.theme().primary).into_any_element() }
            else { div().into_any_element() };
        let app = self.app.clone();
        h_flex().id(id).h(px(30.)).px_2().gap_2().items_center().rounded(cx.theme().radius).cursor_pointer()
            .text_color(if on { cx.theme().foreground } else { p.mid })
            .when(on, |d| d.bg(cx.theme().sidebar_accent))
            .when(!on, |d| d.hover(|s| s.bg(cx.theme().muted.opacity(0.6))))
            .child(div().flex_1().min_w_0().truncate().text_sm().child(title))
            .child(h_flex().flex_none().w(px(28.)).justify_end().child(if matches!(mark_kind(status, unread, asks), true) { mark } else { div().text_xs().text_color(p.dim).child(right).into_any_element() }))
            .on_click(move |_, _, cx| app.update(cx, |a, cx| match &click { RowClick::File(f) => a.open(f, cx), RowClick::Tab(t) => a.focus(t.clone(), cx) }))
            .into_any_element()
    }

    // ---------- header ----------
    fn header(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let app = self.app.read(cx);
        let p = pal(cx);
        let title: String = match app.view {
            View::Home => String::new(),
            View::Settings => "Settings".into(),
            View::Usage => "Usage".into(),
            View::Agents => "Agents".into(),
            View::Images => "Images".into(),
            View::Chat => app.active.as_ref().map(|t| t.read(cx).title()).unwrap_or_default(),
        };
        // just the folder's name: the sidebar already groups by project, and a full path crowds the title out
        let cwd = if app.view == View::Chat { app.active.as_ref().map(|t| { let c = t.read(cx).cwd.clone(); std::path::Path::new(&c).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(c) }) } else { None };
        let sidebar = app.sidebar;
        let mac = cfg!(target_os = "macos");
        let a = self.app.clone();
        h_flex().h(px(TOP)).flex_none().px_3().gap_2().items_center().window_control_area(WindowControlArea::Drag)
            .on_mouse_down(MouseButton::Left, |_, window, _| if !cfg!(target_os = "macos") { window.start_window_move() })
            .when(!sidebar, |d| d.pl(px(if mac { 84. } else { 12. })))
            .child(Button::new("toggle-side").icon(if sidebar { IconName::PanelLeftClose } else { IconName::PanelLeftOpen }).ghost().small().tooltip("Toggle sidebar")
                .on_click(move |_, _, cx| a.update(cx, |a, cx| { let s = !a.sidebar; a.save_settings(serde_json::json!({ "sidebar": s }), cx) })))
            .child(div().min_w_0().truncate().font_weight(FontWeight::MEDIUM).text_color(cx.theme().foreground).child(title))
            .when_some(cwd, |d, c| d.child(div().flex_none().text_xs().text_color(p.dim).child(c)))
            .child(div().flex_1())
            .when(app.view == View::Chat, |d| d.child(Button::new("toggle-panel").icon(if app.side { IconName::PanelRightClose } else { IconName::PanelRightOpen }).ghost().small().tooltip("Changes, files, terminal")
                .on_click(cx.listener(|this, _, _, cx| this.app.update(cx, |a, cx| { a.side = !a.side; cx.notify(); })))))
            .child(Button::new("palette").icon(IconName::Command).ghost().small().tooltip(if mac { "Command palette ⌘K" } else { "Command palette Ctrl+K" })
                .on_click(cx.listener(|this, _, window, cx| this.palette(window, cx))))
    }

    fn toasts(&self, cx: &App) -> impl IntoElement {
        let p = pal(cx);
        let toasts = self.app.read(cx).toasts.clone();
        v_flex().absolute().bottom(px(110.)).left_0().right_0().items_center().gap_2()
            .children(toasts.into_iter().map(|t| {
                h_flex().max_w(px(520.)).px_3().py_2().gap_2().rounded(cx.theme().radius_lg).bg(p.raised).border_1().border_color(cx.theme().border).shadow_lg().text_sm()
                    .child(Icon::new(match t.level { Level::Error => IconName::CircleX, Level::Warn => IconName::TriangleAlert, Level::Info => IconName::Info }).small()
                        .text_color(match t.level { Level::Error => p.red, Level::Warn => p.gold, Level::Info => p.dim }))
                    .child(t.text.clone())
            }))
    }
}

enum RowClick { File(String), Tab(Entity<Tab>) }
fn mark_kind(status: Status, unread: bool, asks: bool) -> bool { asks || unread || matches!(status, Status::Running | Status::Starting | Status::Dead) }

fn nav(id: &'static str, icon: IconName, label: &'static str, on: bool, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static, cx: &App) -> impl IntoElement {
    let p = pal(cx);
    h_flex().id(id).h(px(30.)).px_2().gap_2().items_center().rounded(cx.theme().radius).cursor_pointer().text_sm()
        .text_color(if on { cx.theme().foreground } else { p.mid })
        .when(on, |d| d.bg(cx.theme().sidebar_accent)).when(!on, |d| d.hover(|s| s.bg(cx.theme().muted.opacity(0.6))))
        .child(Icon::new(icon).small()).child(label).on_click(f)
}

pub fn session_title(s: &SessionInfo) -> String {
    s.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| { let t = crate::state::tab::title_from(&s.first_prompt); if t.is_empty() { "Untitled".into() } else { t } })
}

pub fn ago(ms: f64) -> String {
    let s = ((crate::state::tab::now_ms() - ms) / 1000.).max(0.) as u64;
    if s < 60 { "now".into() } else if s < 3600 { format!("{}m", s / 60) } else if s < 86400 { format!("{}h", s / 3600) } else if s < 86400 * 7 { format!("{}d", s / 86400) } else { format!("{}w", s / (86400 * 7)) }
}

fn rename(tab: Entity<Tab>, window: &mut Window, cx: &mut App) {
    use gpui_kit::component::WindowExt;
    let cur = tab.read(cx).title();
    let input = cx.new(|cx| InputState::new(window, cx).default_value(cur));
    let (i2, t2) = (input.clone(), tab.clone());
    window.open_dialog(cx, move |d, _, _| {
        let (i, t) = (i2.clone(), t2.clone());
        d.title("Rename session").w(px(420.)).child(Input::new(&i2)).footer(
            h_flex().gap_2().justify_end().w_full().child(Button::new("rn-ok").label("Rename").primary().on_click(move |_, window, cx| {
                let name = i.read(cx).value().trim().to_string();
                if !name.is_empty() { t.update(cx, |t, cx| t.rename(name, cx)); }
                window.close_dialog(cx);
            })),
        )
    });
    input.read(cx).focus_handle(cx).focus(window, cx);
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
        h_flex().id("workspace").key_context("Workspace").track_focus(&self.focus).size_full().bg(cx.theme().background).text_color(cx.theme().foreground)
            .font_family(cx.theme().font_family.clone()).text_size(cx.theme().font_size)
            .on_action(cx.listener(|this, _: &NewSession, window, cx| this.new_session(window, cx)))
            .on_action(cx.listener(|this, _: &Palette, window, cx| this.palette(window, cx)))
            .on_action(cx.listener(|_, _: &SwitchModel, window, cx| picker::models(window, cx)))
            .on_action(cx.listener(|this, _: &OpenSettings, _, cx| this.app.update(cx, |a, cx| a.show(View::Settings, cx))))
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| this.app.update(cx, |a, cx| { let s = !a.sidebar; a.save_settings(serde_json::json!({ "sidebar": s }), cx) })))
            .on_action(cx.listener(|this, _: &CloseTab, _, cx| { let t = this.app.read(cx).active.clone(); if let Some(t) = t { this.app.update(cx, |a, cx| a.close(&t, cx)); } }))
            .on_action(cx.listener(|this, _: &StopRun, _, cx| { if let Some(t) = this.app.read(cx).active.clone() { t.update(cx, |t, cx| t.abort(cx)); } }))
            .on_action(cx.listener(|this, _: &FocusComposer, window, cx| { if let Some(t) = this.app.read(cx).active.clone() { if let Some(c) = this.chats.get(&t.entity_id()).cloned() { ChatView::focus_composer(&c, window, cx); } } }))
            .on_action(cx.listener(|this, _: &TogglePanel, _, cx| this.app.update(cx, |a, cx| { a.side = !a.side; cx.notify(); })))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| this.drop_paths(paths, window, cx)))
            .on_action(cx.listener(|this, _: &NextTab, _, cx| cycle(&this.app, 1, cx)))
            .on_action(cx.listener(|this, _: &PrevTab, _, cx| cycle(&this.app, -1, cx)))
            .when(sidebar, |d| d.child(self.sidebar(cx)))
            .child(v_flex().flex_1().min_w_0().h_full().relative().child(self.header(cx)).child(div().flex_1().min_h_0().child(content)).child(self.toasts(cx)))
            .when(side_open, |d| {
                let side = self.side.get_or_insert_with(|| cx.new(|cx| SidePanel::new(window, cx))).clone();
                d.child(div().w(px(side_w)).h_full().flex_none().border_l_1().border_color(cx.theme().border).child(side))
            })
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
