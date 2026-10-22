// Slash commands pith runs itself. anything else starting with / goes to pi as typed (templates, skills, extension commands)
use super::picker::{self, PickItem};
use crate::state::app::{res, Pith, View};
use crate::state::tab::{Block, ItemKind, Level, Mode, Tab};
use gpui_kit::*;
use serde_json::json;
use std::sync::OnceLock;

#[derive(Clone, Debug)]
pub struct Cmd { pub name: String, pub desc: String, pub arg: Option<String>, pub source: String }

impl Cmd {
    fn app(name: &str, desc: &str, arg: Option<&str>) -> Self { Cmd { name: name.into(), desc: desc.into(), arg: arg.map(String::from), source: "app".into() } }
    // what the menu shows next to the name: nothing for pith's own, otherwise where it comes from
    pub fn tag(&self) -> Option<&'static str> {
        match self.source.as_str() { "app" => None, "prompt" => Some("template"), "skill" => Some("skill"), "extension" => Some("extension"), _ => None }
    }
}

fn builtin() -> Vec<Cmd> {
    vec![
        Cmd::app("new", "new session in this folder", None),
        Cmd::app("compact", "summarise older context to free room", Some("[what to keep]")),
        Cmd::app("model", "switch model", None),
        Cmd::app("effort", "how hard the model thinks", None),
        Cmd::app("mode", "ask · edits · plan · auto", Some("[mode]")),
        Cmd::app("plan", "switch to plan mode (read-only)", None),
        Cmd::app("name", "rename this session", Some("<title>")),
        Cmd::app("clone", "copy this session into a new one", None),
        Cmd::app("export", "save the session as an html page", None),
        Cmd::app("copy", "copy the last reply", None),
        Cmd::app("image", "generate an image", Some("<prompt>")),
        Cmd::app("browser", "open the browser pane", Some("[url]")),
        Cmd::app("preview", "open the preview pane", None),
        Cmd::app("diff", "open the changes pane", None),
        Cmd::app("files", "open the files pane", None),
        Cmd::app("agents", "live subagents", None),
        Cmd::app("terminal", "a shell in this folder", None),
        Cmd::app("usage", "tokens, cost, limits", None),
        Cmd::app("settings", "settings", None),
        Cmd::app("stop", "stop the current run", None),
    ]
}

// the templates pith ships, for before pi is up and has said what it has
fn bundled() -> &'static Vec<Cmd> {
    static B: OnceLock<Vec<Cmd>> = OnceLock::new();
    B.get_or_init(|| {
        let mut out = vec![];
        let Ok(rd) = std::fs::read_dir(res().join("prompts/commands")) else { return out };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("md") { continue; }
            let Some(name) = p.file_stem().map(|s| s.to_string_lossy().to_string()) else { continue };
            let text = std::fs::read_to_string(&p).unwrap_or_default();
            let fm = text.strip_prefix("---\n").and_then(|r| r.split_once("\n---")).map(|x| x.0).unwrap_or("");
            let field = |k: &str| fm.lines().find_map(|l| l.strip_prefix(k).map(|v| v.trim().trim_matches('"').to_string())).filter(|s| !s.is_empty());
            out.push(Cmd { name, desc: field("description:").unwrap_or_default(), arg: field("argument-hint:"), source: "prompt".into() });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    })
}

// commands.ts commandsFor: pith's own (unless pi has one by that name), pi's, then the bundled templates until pi
// has said what it has
pub fn commands_for(tab: Option<&Tab>) -> Vec<Cmd> {
    let pi: Vec<Cmd> = tab.map(|t| t.commands.iter().map(|c| Cmd { name: c.name.clone(), desc: c.description.clone(), arg: None, source: c.source.clone() }).collect()).unwrap_or_default();
    let mut out: Vec<Cmd> = builtin().into_iter().filter(|b| !pi.iter().any(|c| c.name == b.name)).collect();
    let none = pi.is_empty();
    out.extend(pi);
    if none { out.extend(bundled().iter().cloned()); }
    out
}

pub fn is_builtin(text: &str) -> bool {
    let name = text.strip_prefix('/').and_then(|r| r.split_whitespace().next()).unwrap_or("");
    builtin().iter().any(|b| b.name == name)
}

pub fn mode_label(m: Mode) -> &'static str { match m { Mode::Ask => "Ask", Mode::Edits => "Edits", Mode::Plan => "Plan", Mode::Auto => "Auto" } }

pub fn level_label(l: &str) -> String {
    match l { "off" => "Off".into(), "xhigh" => "Extra high".into(), _ => { let mut c = l.chars(); c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default() } }
}

pub fn level_hint(l: &str) -> &'static str {
    match l { "off" => "Answers straight away", "minimal" => "A quick look first", "low" => "Some thought", "medium" => "Thinks it through", "high" => "Thinks hard", "xhigh" => "As hard as it can", _ => "" }
}

// runs /name if it's one of pith's own. false means it should go to pi as typed
pub fn run_builtin(tab: Option<Entity<Tab>>, text: &str, window: &mut Window, cx: &mut App) -> bool {
    let Some(rest) = text.strip_prefix('/') else { return false };
    let (name, arg) = match rest.split_once(char::is_whitespace) { Some((n, a)) => (n, a.trim().to_string()), None => (rest, String::new()) };
    if !builtin().iter().any(|b| b.name == name) { return false; }
    let app = Pith::global(cx);
    match name {
        "usage" => { app.update(cx, |a, cx| a.show(View::Usage, cx)); return true; }
        "settings" => { app.update(cx, |a, cx| a.show(View::Settings, cx)); return true; }
        _ => {}
    }
    let Some(tab) = tab else {
        app.update(cx, |a, cx| a.toast(Level::Info, "open a session first", cx));
        return true;
    };
    match name {
        "new" => { let (cwd, kind) = { let t = tab.read(cx); (t.cwd.clone(), t.kind) }; app.update(cx, |a, cx| { a.fresh(cwd, kind, cx); }); }
        "compact" => tab.update(cx, |t, cx| {
            if arg.is_empty() { return t.compact(cx); }
            t.compacting = true;
            cx.notify();
            t.with_pi(json!({ "type": "compact", "customInstructions": arg }), cx, |t, r, cx| {
                t.compacting = false;
                if r["success"] != true { t.note(Level::Error, r["error"].as_str().unwrap_or("Compaction failed").to_string(), cx); }
                cx.notify();
            });
        }),
        "model" => picker::models(window, cx),
        "effort" => if arg.is_empty() { pick_effort(tab, window, cx) } else { tab.update(cx, |t, cx| t.set_thinking(arg, cx)) },
        "mode" => match Mode::ALL.into_iter().find(|m| m.as_str() == arg) {
            Some(m) => tab.update(cx, |t, cx| t.set_mode(m, cx)),
            None => pick_mode(Some(tab), window, cx),
        },
        "plan" => tab.update(cx, |t, cx| t.set_mode(Mode::Plan, cx)),
        "name" => if arg.is_empty() { app.update(cx, |a, cx| a.toast(Level::Info, "/name needs a title", cx)) } else { tab.update(cx, |t, cx| t.rename(arg, cx)) },
        "clone" => tab.update(cx, |t, cx| t.clone_session(cx)),
        // saved next to the session, then opened
        "export" => tab.update(cx, |t, cx| t.export_html(cx, |p, cx| {
            Pith::global(cx).update(cx, |a, cx| a.toast(Level::Info, format!("saved {p}"), cx));
            cx.open_with_system(std::path::Path::new(&p));
        })),
        "copy" => match last_reply(tab.read(cx)) {
            Some(t) => { cx.write_to_clipboard(ClipboardItem::new_string(t)); app.update(cx, |a, cx| a.toast(Level::Info, "copied", cx)); }
            None => {}
        },
        "image" if arg.is_empty() => app.update(cx, |a, cx| a.show(View::Images, cx)),
        "image" => tab.update(cx, |t, cx| t.send(format!("Generate this image with generate_image, then show it: {arg}"), vec![], crate::state::tab::How::Auto, cx)),
        // the agent's browser page, made or sent somewhere new, then the pane on it
        "browser" => {
            if !arg.is_empty() {
                let url = if arg.split_once(':').is_some_and(|(s, _)| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphabetic())) { arg.clone() } else { format!("https://{arg}") };
                let id = crate::core::browser::page_id(&tab.read(cx).id);
                crate::core::web::open(&id, &url, 1280, 860);
            }
            super::panes::open(super::panes::Pane::Browser, cx)
        }
        "preview" => super::panes::open(super::panes::Pane::Preview, cx),
        "diff" => super::panes::open(super::panes::Pane::Changes, cx),
        "files" => super::panes::open(super::panes::Pane::Files, cx),
        "agents" => super::panes::open(super::panes::Pane::Agents, cx),
        "terminal" => super::panes::open(super::panes::Pane::Terminal, cx),
        "stop" => tab.update(cx, |t, cx| t.abort(cx)),
        _ => return false,
    }
    true
}

fn last_reply(t: &Tab) -> Option<String> {
    t.items.iter().rev().find_map(|i| match &i.kind {
        ItemKind::Assistant(a) => {
            let s = a.blocks.iter().filter_map(|b| match b { Some(Block::Text(t)) => Some(t.as_str()), _ => None }).collect::<Vec<_>>().join("\n\n");
            Some(s).filter(|s| !s.trim().is_empty())
        }
        _ => None,
    })
}

pub fn pick_mode(tab: Option<Entity<Tab>>, window: &mut Window, cx: &mut App) {
    let cur = match &tab { Some(t) => t.read(cx).mode, None => Pith::global(cx).read(cx).mode_default() };
    let items = Mode::ALL.into_iter().map(|m| {
        let t = tab.clone();
        PickItem::new(mode_label(m), move |_, cx| match &t {
            Some(t) => t.update(cx, |t, cx| t.set_mode(m, cx)),
            None => Pith::global(cx).update(cx, |a, cx| a.save_settings(json!({ "mode": m.as_str() }), cx)),
        }).hint(super::composer::sentence(m.hint())).on(m == cur).icon(mode_icon(m))
    }).collect();
    picker::open(items, "Mode", window, cx);
}

pub fn pick_effort(tab: Entity<Tab>, window: &mut Window, cx: &mut App) {
    let (levels, cur) = { let t = tab.read(cx); (t.levels.clone(), t.state["thinkingLevel"].as_str().unwrap_or("").to_string()) };
    let items = levels.into_iter().map(|l| {
        let (t, l2) = (tab.clone(), l.clone());
        PickItem::new(level_label(&l), move |_, cx| t.update(cx, |t, cx| t.set_thinking(l2.clone(), cx))).hint(level_hint(&l)).on(l == cur)
    }).collect();
    picker::open(items, "Thinking", window, cx);
}

pub fn mode_icon(m: Mode) -> gpui_kit::assets::IconName {
    use gpui_kit::assets::IconName;
    match m { Mode::Ask => IconName::ShieldQuestionMark, Mode::Edits => IconName::FilePen, Mode::Plan => IconName::NotebookPen, Mode::Auto => IconName::Zap }
}
