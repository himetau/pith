// Agents: the subagent definitions pi's subagent extension reads (~/.pi/agent/agents, <project>/.pi/agents).
// a list on the left, the one you picked in an editor on the right
use super::page::{self, empty};
use super::theme::pal;
use super::tools::tilde;
use crate::core::sys;
use crate::state::app::{Pith, View};
use crate::state::tab::{How, Level};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;
use serde_json::{json, Value};

const TOOLS: &str = "read, grep, find, ls, bash";

pub struct AgentsPage {
    defs: Vec<Value>,
    loaded: bool,
    // the file being edited, empty for a new one. None shows the empty state
    open: Option<String>,
    scope: &'static str,
    cwd: Option<String>,
    saving: bool,
    name: Entity<InputState>,
    description: Entity<InputState>,
    tools: Entity<InputState>,
    model: Entity<InputState>,
    body: Entity<TextareaState>,
    task: Entity<InputState>,
    last_view: View,
    _subs: Vec<Subscription>,
}

impl AgentsPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = |ph: &str, window: &mut Window, cx: &mut Context<Self>| { let ph = ph.to_string(); cx.new(|cx| InputState::new(window, cx).placeholder(ph)) };
        let name = input("reviewer", window, cx);
        let description = input("When the main agent should hand work to it", window, cx);
        let tools = input("read, grep, find, ls, bash, edit, write", window, cx);
        let model = input("Inherit", window, cx);
        let task = input("A task for it", window, cx);
        let body = cx.new(|cx| TextareaState::new(window, cx).placeholder("The agent's system prompt: who it is, what it does, how it reports back"));
        let app = Pith::global(cx);
        let mut subs = vec![
            cx.observe(&app, |this: &mut Self, app, cx| {
                let v = app.read(cx).view;
                if v == View::Agents && this.last_view != View::Agents { this.load(cx); }
                this.last_view = v;
            }),
            cx.subscribe_in(&task, window, |this, _, ev: &InputEvent, window, cx| if let InputEvent::PressEnter { .. } = ev { this.summon(window, cx) }),
        ];
        for i in [&name, &description, &tools, &model] {
            subs.push(cx.subscribe_in(i, window, |this, _, ev: &InputEvent, window, cx| if let InputEvent::PressEnter { .. } = ev { this.save(window, cx) }));
        }
        let mut page = AgentsPage { defs: vec![], loaded: false, open: None, scope: "user", cwd: None, saving: false, name, description, tools, model, body, task, last_view: View::Agents, _subs: subs };
        page.load(cx);
        page
    }

    // project agents belong to the session you're in, or the default folder when there isn't one
    fn project_cwd(cx: &App) -> String {
        let a = Pith::global(cx).read(cx);
        a.active.as_ref().map(|t| t.read(cx).cwd.clone()).unwrap_or_else(|| a.home_cwd.clone())
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let cwd = Self::project_cwd(cx);
        self.cwd = Some(cwd.clone());
        cx.spawn(async move |this, cx| {
            let defs = cx.background_spawn(async move { sys::agent_defs(Some(&cwd)) }).await;
            let _ = this.update(cx, |this, cx| {
                // an agent deleted on disk while it was open goes back to the empty state
                if let Some(f) = &this.open { if !f.is_empty() && !defs.iter().any(|d| d["file"].as_str() == Some(f)) { this.open = None; } }
                this.defs = defs;
                this.loaded = true;
                cx.notify();
            });
        }).detach();
    }

    fn fill(&mut self, d: &Value, window: &mut Window, cx: &mut Context<Self>) {
        let s = |k: &str| d[k].as_str().unwrap_or("").to_string();
        for (input, k) in [(&self.name, "name"), (&self.description, "description"), (&self.tools, "tools"), (&self.model, "model")] {
            let v = s(k);
            input.update(cx, |st, cx| st.set_value(v, window, cx));
        }
        let body = s("body");
        self.body.update(cx, |st, cx| st.set_value(body, window, cx));
        self.open = Some(s("file"));
        self.scope = if d["scope"] == "project" { "project" } else { "user" };
        cx.notify();
    }

    fn new_agent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.fill(&json!({ "tools": TOOLS }), window, cx);
        self.name.update(cx, |s, cx| s.focus(window, cx));
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(file) = self.open.clone() else { return };
        if self.saving { return; }
        let v = |i: &Entity<InputState>, cx: &App| i.read(cx).value().trim().to_string();
        let name = v(&self.name, cx);
        let app = Pith::global(cx);
        if name.is_empty() {
            app.update(cx, |a, cx| a.toast(Level::Warn, "An agent needs a name", cx));
            self.name.update(cx, |s, cx| s.focus(window, cx));
            return;
        }
        let def = json!({ "name": name, "description": v(&self.description, cx), "tools": v(&self.tools, cx), "model": v(&self.model, cx), "body": self.body.read(cx).value().to_string(), "file": file });
        let (cwd, scope) = (self.cwd.clone(), self.scope);
        self.saving = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let saved = cx.background_spawn(async move { sys::save_agent(&def, cwd.as_deref(), scope) }).await;
            let _ = app.update(cx, |a, cx| a.toast(Level::Info, format!("Saved {}", tilde(&saved)), cx));
            let _ = this.update(cx, |this, cx| { this.saving = false; this.open = Some(saved); this.load(cx); });
        }).detach();
    }

    fn summon(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let app = Pith::global(cx);
        let Some(tab) = app.read(cx).active.clone() else { return };
        let task = self.task.read(cx).value().trim().to_string();
        let name = self.name.read(cx).value().trim().to_string();
        if task.is_empty() || name.is_empty() { self.task.update(cx, |s, cx| s.focus(window, cx)); return; }
        tab.update(cx, |t, cx| t.send(format!("/summon {name} {task}"), vec![], How::Auto, cx));
        self.task.update(cx, |s, cx| s.set_value("", window, cx));
        app.update(cx, |a, cx| a.focus(tab, cx));
    }

    fn list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = pal(cx);
        let mut rows: Vec<AnyElement> = vec![];
        let project = self.cwd.as_deref().map(|c| std::path::Path::new(c).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| tilde(c))).unwrap_or_default();
        for (scope, title, sub) in [("user", "Personal".to_string(), tilde(&sys::agent_dir().join("agents").display().to_string())), ("project", format!("Project · {project}"), ".pi/agents".to_string())] {
            let defs: Vec<&Value> = self.defs.iter().filter(|d| d["scope"] == scope).collect();
            if defs.is_empty() && scope == "project" { continue; }
            rows.push(v_flex().px_2().pt_4().pb_1()
                .child(div().text_xs().font_weight(FontWeight::MEDIUM).text_color(p.mid).child(title))
                .child(div().text_xs().text_color(p.dim).truncate().child(sub)).into_any_element());
            if defs.is_empty() { rows.push(div().px_2().py_1().text_sm().text_color(p.dim).child("None yet").into_any_element()); }
            for d in defs {
                let file = d["file"].as_str().unwrap_or("").to_string();
                let on = self.open.as_deref() == Some(file.as_str());
                let d2 = d.clone();
                rows.push(v_flex().id(SharedString::from(format!("ag-{file}"))).px_2().py_2().gap_0p5().rounded(cx.theme().radius).cursor_pointer()
                    .when(on, |e| e.bg(cx.theme().secondary_active)).when(!on, |e| e.hover(|s| s.bg(cx.theme().muted.opacity(0.6))))
                    .child(div().text_sm().font_weight(FontWeight::MEDIUM).text_color(cx.theme().foreground).truncate().child(d["name"].as_str().unwrap_or("").to_string()))
                    .when(!d["description"].as_str().unwrap_or("").is_empty(), |e| e.child(div().text_xs().text_color(p.dim).line_clamp(2).child(d["description"].as_str().unwrap_or("").to_string())))
                    .on_click(cx.listener(move |this, _, window, cx| this.fill(&d2, window, cx)))
                    .into_any_element());
            }
        }
        v_flex().w(px(300.)).h_full().flex_none().border_r_1().border_color(p.hairline)
            .child(h_flex().h(px(48.)).flex_none().px_4().items_center().gap_2()
                .child(div().flex_1().text_sm().text_color(p.dim).child(if self.loaded { format!("{} defined", self.defs.len()) } else { String::new() }))
                .child(Button::new("ag-new").icon(IconName::Plus).label("New agent").outline().small().on_click(cx.listener(|this, _, window, cx| this.new_agent(window, cx)))))
            .child(div().id("ag-list").flex_1().min_h_0().overflow_y_scroll().px_2().pb_3().children(rows))
    }

    fn editor(&self, file: &str, cx: &mut Context<Self>) -> AnyElement {
        let p = pal(cx);
        let field = |label: &'static str, hint: &'static str, ctl: AnyElement, cx: &App| {
            v_flex().gap_1p5().flex_1().min_w_0()
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).text_color(cx.theme().foreground).child(label))
                .child(ctl)
                .when(!hint.is_empty(), |d| d.child(div().text_xs().text_color(p.dim).child(hint)))
        };
        let is_new = file.is_empty();
        let name = self.name.read(cx).value().to_string();
        let has_project = self.cwd.is_some();
        let dest = if self.scope == "project" { self.cwd.as_deref().map(|c| tilde(&format!("{c}/.pi/agents"))).unwrap_or_default() } else { tilde(&sys::agent_dir().join("agents").display().to_string()) };
        let has_tab = Pith::global(cx).read(cx).active.is_some();

        let title = h_flex().gap_3().items_center()
            .child(div().flex_1().min_w_0().truncate().text_xl().font_weight(FontWeight::SEMIBOLD).text_color(cx.theme().foreground)
                .child(if is_new { "New agent".to_string() } else if name.trim().is_empty() { "Untitled".into() } else { name }))
            .child(Button::new("ag-save").label("Save").primary().small().loading(self.saving).on_click(cx.listener(|this, _, window, cx| this.save(window, cx))));
        let location: AnyElement = if is_new {
            let this = cx.entity().downgrade();
            h_flex().gap_3().items_center()
                .child(page::segmented("ag-scope", if has_project { &["Personal", "Project"] } else { &["Personal"] }, (self.scope == "project") as usize, move |i, _, cx| {
                    let _ = this.update(cx, |t, cx| { t.scope = if i == 1 { "project" } else { "user" }; cx.notify(); });
                }, cx))
                .child(div().min_w_0().text_xs().text_color(p.dim).whitespace_nowrap().overflow_hidden().text_ellipsis_start().child(format!("Saves to {dest}")))
                .into_any_element()
        } else {
            div().text_xs().text_color(p.dim).font_family(cx.theme().mono_font_family.clone()).whitespace_nowrap().overflow_hidden().text_ellipsis_start().child(tilde(file)).into_any_element()
        };

        let form = v_flex().gap_5()
            .child(h_flex().gap_4().items_start()
                .child(field("Name", "", Input::new(&self.name).into_any_element(), cx))
                .child(div().w(px(220.)).flex_none().child(field("Model", "Empty uses the session's", Input::new(&self.model).into_any_element(), cx))))
            .child(field("Description", "The main agent picks subagents by this, so say plainly what it's for", Input::new(&self.description).into_any_element(), cx))
            .child(field("Tools", "Comma separated", Input::new(&self.tools).into_any_element(), cx))
            .child(field("Prompt", "", Textarea::new(&self.body).h(px(340.)).into_any_element(), cx));

        let summon = v_flex().gap_2().mt_8().pt_6().border_t_1().border_color(p.hairline)
            .child(div().text_sm().font_weight(FontWeight::MEDIUM).text_color(cx.theme().foreground).child("Summon"))
            .child(div().text_xs().text_color(p.dim).child(if has_tab { "Hands a task to this agent in the session you're in. Needs a subagent extension in ~/.pi/agent/extensions." } else { "Open a session to hand this agent a task from there." }))
            .child(h_flex().gap_2().mt_1()
                .child(div().flex_1().min_w_0().child(Input::new(&self.task).disabled(!has_tab || is_new)))
                .child(Button::new("ag-summon").icon(IconName::Bot).label("Summon").outline().disabled(!has_tab || is_new).on_click(cx.listener(|this, _, window, cx| this.summon(window, cx)))));

        div().id("ag-edit").flex_1().min_w_0().h_full().overflow_y_scroll().child(
            h_flex().w_full().justify_center().px_8().pt_6().pb(px(64.)).child(
                v_flex().w_full().max_w(px(720.)).gap_1()
                    .child(title)
                    .child(location)
                    .child(div().mt_6().child(form))
                    .child(summon)))
            .into_any_element()
    }
}

impl Render for AgentsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = pal(cx);
        if !self.loaded {
            return v_flex().size_full().items_center().justify_center().gap_2().text_sm().text_color(p.dim).child(Spinner::new().small()).into_any_element();
        }
        let right: AnyElement = match self.open.clone() {
            Some(f) => self.editor(&f, cx),
            None => {
                let new = Button::new("ag-new-empty").icon(IconName::Plus).label("New agent").outline().small().on_click(cx.listener(|this, _, window, cx| this.new_agent(window, cx)));
                let (title, body) = if self.defs.is_empty() {
                    ("No agents yet", "Subagents are separate pi processes the main agent hands work to. It picks them by name and description. pi reads them through a subagent extension in ~/.pi/agent/extensions.")
                } else {
                    ("Pick an agent to edit", "Or make a new one. The main agent picks subagents by name and description, so say plainly what each is for.")
                };
                v_flex().flex_1().h_full().justify_center().child(empty(IconName::Bot, title, body, Some(new.into_any_element()), cx)).into_any_element()
            }
        };
        h_flex().size_full().items_start().child(self.list(cx)).child(right).into_any_element()
    }
}
