// Agent definitions the subagent extension reads (~/.pi/agent/agents, .pi/agents), and summoning them (agents.ts).
// a 300px list on the left, the one you picked in an editor on the right
use super::page::{self, btn, head, ic, legend_box, s_box, s_in, wide, Btn};
use super::panes::Pane;
use super::theme::{em, geo, pal, u};
use super::tools::tilde;
use crate::core::sys;
use crate::state::app::{Pith, View};
use crate::state::tab::{How, Level};
use gpui_kit::component::input::{InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;
use serde_json::{json, Value};

const TOOLS: &str = "read, grep, find, ls, bash";
const RUNNING: [&str; 4] = ["spawning", "thinking", "tool", "streaming"];

pub struct AgentsPage {
    defs: Vec<Value>,
    running: usize,
    // the definition being edited: its file, empty for a new one, and where it saves. None shows the empty state
    sel: Option<(String, &'static str)>,
    cwd: Option<String>,
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
        let name = input("name, like reviewer", window, cx);
        let description = input("when the main agent should use it", window, cx);
        let tools = input("read, grep, find, ls, bash, edit, write", window, cx);
        let model = input("inherit", window, cx);
        let task = input("a task for it", window, cx);
        let body = cx.new(|cx| TextareaState::new(window, cx).placeholder("the agent's system prompt"));
        let app = Pith::global(cx);
        let mut subs = vec![
            cx.observe(&app, |this: &mut Self, app, cx| {
                let v = app.read(cx).view;
                if v == View::Agents && this.last_view != View::Agents { this.load(cx); }
                this.last_view = v;
            }),
            cx.subscribe_in(&task, window, |this, _, ev: &InputEvent, window, cx| if let InputEvent::PressEnter { .. } = ev { this.summon(window, cx) }),
            cx.observe(&body, |_, _, cx| cx.notify()),
        ];
        // the focus border follows the fields
        for i in [&name, &description, &tools, &model, &task] { subs.push(cx.observe(i, |_, _, cx| cx.notify())); }
        let mut page = AgentsPage { defs: vec![], running: 0, sel: None, cwd: None, name, description, tools, model, body, task, last_view: View::Agents, _subs: subs };
        page.load(cx);
        page
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let cwd = Pith::global(cx).read(cx).active.as_ref().map(|t| t.read(cx).cwd.clone());
        self.cwd = cwd.clone();
        cx.spawn(async move |this, cx| {
            let (defs, hub) = cx.background_spawn(async move { (sys::agent_defs(cwd.as_deref()), sys::read_agent_hub()) }).await;
            let _ = this.update(cx, |this, cx| {
                // an agent deleted on disk while it was open goes back to the empty state
                if let Some((f, _)) = &this.sel { if !f.is_empty() && !defs.iter().any(|d| d["file"].as_str() == Some(f)) { this.sel = None; } }
                this.defs = defs;
                this.running = hub["agents"].as_array().map(|a| a.iter().filter(|x| RUNNING.contains(&x["status"].as_str().unwrap_or(""))).count()).unwrap_or(0);
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
        self.sel = Some((s("file"), if d["scope"] == "project" { "project" } else { "user" }));
        cx.notify();
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((file, scope)) = self.sel.clone() else { return };
        let v = |i: &Entity<InputState>, cx: &App| i.read(cx).value().trim().to_string();
        let name = v(&self.name, cx);
        let app = Pith::global(cx);
        if name.is_empty() { app.update(cx, |a, cx| a.toast(Level::Warn, "it needs a name", cx)); return; }
        let def = json!({ "name": name, "description": v(&self.description, cx), "tools": v(&self.tools, cx), "model": v(&self.model, cx), "body": self.body.read(cx).value().to_string(), "file": file });
        let cwd = self.cwd.clone();
        let _ = window;
        cx.spawn(async move |this, cx| {
            let saved = cx.background_spawn(async move { sys::save_agent(&def, cwd.as_deref(), scope) }).await;
            let _ = app.update(cx, |a, cx| a.toast(Level::Info, format!("saved {}", tilde(&saved)), cx));
            let _ = this.update(cx, |this, cx| { this.sel = Some((saved, scope)); this.load(cx); });
        }).detach();
    }

    fn summon(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let app = Pith::global(cx);
        let Some(tab) = app.read(cx).active.clone() else { app.update(cx, |a, cx| a.toast(Level::Warn, "open a session to summon into", cx)); return };
        let task = self.task.read(cx).value().trim().to_string();
        if task.is_empty() { self.task.update(cx, |s, cx| s.focus(window, cx)); return; }
        let name = self.name.read(cx).value().trim().to_string();
        tab.update(cx, |t, cx| t.send(format!("/summon {name} {task}"), vec![], How::Auto, cx));
        live_view(cx);
    }

    fn editor(&self, file: &str, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let p = pal(cx);
        // .lab: a small dim label over its field
        let lab = |l: &str, el: AnyElement, cx: &App| div().flex().flex_col().gap(u(4.)).min_w_0().child(div().text_size(em(cx, 0.86)).text_color(p.dim).child(l.to_string())).child(el);
        let focused = self.body.read(cx).focus_handle(cx).is_focused(window);
        let fs = geo(cx).fs;
        let body = s_box(focused, cx).items_start().child(Textarea::new(&self.body).appearance(false).p_0().h(u(16. * fs * 1.5 + 10.)).text_size(u(fs)).line_height(u(fs * 1.5)).w_full()
            // the kit pads its editor inside; the css textarea only has .s-in's
            .mx(u(-10.)).mt(u(-8.)).mb(u(-2.)));
        let row2 = |a: AnyElement, b: AnyElement| div().flex().gap(u(10.)).child(div().flex_1().min_w_0().child(a)).child(div().w(u(220.)).flex_none().child(b));
        div().flex().flex_col().gap(u(12.)).min_w_0()
            .child(row2(lab("name", s_in(&self.name, window, cx).into_any_element(), cx).into_any_element(), lab("model", s_in(&self.model, window, cx).into_any_element(), cx).into_any_element()))
            .child(lab("description", s_in(&self.description, window, cx).into_any_element(), cx))
            .child(lab("tools", s_in(&self.tools, window, cx).into_any_element(), cx))
            .child(lab("prompt", body.into_any_element(), cx))
            .child(div().flex().gap(u(12.)).items_center().text_size(em(cx, 0.9))
                .child(btn("ag-save", Btn::Pri, false, cx).child("save").on_click(cx.listener(|this, _, window, cx| this.save(window, cx))))
                .when(!file.is_empty(), |d| d.child(div().min_w_0().truncate().text_color(p.dim).child(tilde(file)))))
            .child(div().mt(u(10.)).child(legend_box("summon into the current session",
                div().pt(u(18.)).px(u(14.)).pb(u(12.)).flex().gap(u(10.)).items_center()
                    .child(div().flex_1().min_w_0().child(s_in(&self.task, window, cx)))
                    .child(btn("ag-summon", Btn::Plain, false, cx).child(ic("agents", cx)).child("summon").on_click(cx.listener(|this, _, window, cx| this.summon(window, cx)))), cx)))
            .into_any_element()
    }
}

// "live view": the agents pane, on the session you're in
fn live_view(cx: &mut App) {
    let app = Pith::global(cx);
    let Some(t) = app.read(cx).active.clone() else { return };
    app.update(cx, |a, cx| a.focus(t, cx));
    super::panes::open(Pane::Agents, cx);
}

impl Render for AgentsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (p, g) = (pal(cx), geo(cx));
        let has_tab = Pith::global(cx).read(cx).active.is_some();
        let mut actions = vec![btn("ag-new", Btn::Plain, true, cx).child(ic("plus", cx)).child("new agent")
            .on_click(cx.listener(|this, _, window, cx| this.fill(&json!({ "tools": TOOLS, "scope": "user" }), window, cx))).into_any_element()];
        if has_tab { actions.push(btn("ag-live", Btn::Ghost, true, cx).child("live view").on_click(|_, _, cx| live_view(cx)).into_any_element()); }
        let sub = format!("{} defined · {} running", self.defs.len(), self.running);

        let sel = self.sel.as_ref().map(|s| s.0.clone());
        let (hover, tertiary) = (p.hover, p.tertiary);
        let list = div().flex().flex_col().gap(u(4.)).children(self.defs.iter().enumerate().map(|(i, d)| {
            let on = sel.as_deref().is_some_and(|f| !f.is_empty() && d["file"].as_str() == Some(f));
            let s = |k: &str| d[k].as_str().unwrap_or("").to_string();
            let d2 = d.clone();
            div().id(("ag-def", i)).py(u(8.)).px(u(10.)).rounded(u(g.r2)).border(u(1.)).cursor_pointer()
                .border_color(if on { p.line } else { p.line.opacity(0.) }).when(on, |e| e.bg(p.raised)).when(!on, |e| e.hover(move |st| st.bg(hover)))
                .child(div().flex().gap(u(8.)).items_baseline().text_color(tertiary).child(s("name"))
                    .when(d["scope"] == "project", |e| e.child(div().text_size(em(cx, 0.75)).text_color(p.dim).border(u(1.)).border_color(p.line).rounded(u(3.)).px(u(4.)).child("project"))))
                .when(!s("description").is_empty(), |e| e.child(div().text_size(em(cx, 0.88)).text_color(p.mid).child(s("description"))))
                .when(!s("tools").is_empty(), |e| e.child(div().mt(u(2.)).text_size(em(cx, 0.8)).text_color(p.dim).child(s("tools"))))
                .on_click(cx.listener(move |this, _, window, cx| this.fill(&d2, window, cx)))
        }));
        let right = match sel {
            Some(f) => self.editor(&f, window, cx),
            None => page::empty("pick an agent to edit, or summon one", cx).into_any_element(),
        };
        // pages.css's @media (max-width: 1100px) stacks the two
        let narrow = f32::from(window.viewport_size().width) / g.scale < 1100.;
        let grid = wide().flex().gap(u(18.)).when(narrow, |d| d.flex_col())
            .child(div().when(!narrow, |d| d.w(u(300.)).flex_none()).child(list))
            .child(div().flex_1().min_w_0().flex().flex_col().child(right));
        page::page("agents", vec![
            head("agents", Some(sub.into()), actions, cx).into_any_element(),
            page::note("subagents are separate pi processes the main agent hands work to. it picks them by name and description, so say plainly what each is for. this needs a subagent extension in ~/.pi/agent/extensions.", cx).self_start().into_any_element(),
            grid.into_any_element(),
        ])
    }
}
