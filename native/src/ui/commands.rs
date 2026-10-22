// Slash commands pith runs itself. anything else starting with / goes to pi as typed (templates, skills, extension commands)
use super::picker::{self, PickItem};
use crate::state::app::{res, Pith, View};
use crate::state::tab::{Block, ItemKind, Level, Mode, Tab};
use gpui_kit::*;
use serde_json::json;
use std::sync::OnceLock;

#[derive(Clone, Debug)]
pub struct Cmd { pub name: String, pub desc: String, pub arg: Option<String>, pub source: String }

// pith's own that still mean something before there's a session
const NO_TAB: [&str; 6] = ["mode", "plan", "image", "agents", "usage", "settings"];

impl Cmd {
    fn app(name: &str, desc: &str, arg: Option<&str>) -> Self { Cmd { name: name.into(), desc: desc.into(), arg: arg.map(String::from), source: "app".into() } }
    // what the menu shows next to the name: nothing for pith's own, otherwise where it comes from
    pub fn tag(&self) -> Option<&'static str> {
        match self.source.as_str() { "app" => None, "prompt" => Some("template"), "skill" => Some("skill"), "extension" => Some("extension"), _ => None }
    }
}

fn builtin() -> Vec<Cmd> {
    vec![
        Cmd::app("new", "New session in this folder", None),
        Cmd::app("compact", "Summarise older context to free room", Some("[what to keep]")),
        Cmd::app("model", "Switch model", None),
        Cmd::app("effort", "How hard the model thinks", Some("[level]")),
        Cmd::app("mode", "Ask, edits, plan or auto", Some("[mode]")),
        Cmd::app("plan", "Switch to plan mode, read only", None),
        Cmd::app("name", "Rename this session", Some("<title>")),
        Cmd::app("copy", "Copy the last reply", None),
        Cmd::app("image", "Generate an image", Some("<prompt>")),
        Cmd::app("diff", "Show what changed", None),
        Cmd::app("files", "Browse the project's files", None),
        Cmd::app("terminal", "A shell in this folder", None),
        Cmd::app("agents", "Subagents", None),
        Cmd::app("usage", "Tokens, cost, limits", None),
        Cmd::app("settings", "Settings", None),
        Cmd::app("stop", "Stop the current run", None),
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

pub fn commands_for(tab: Option<&Tab>) -> Vec<Cmd> {
    let pi: Vec<Cmd> = tab.map(|t| t.commands.iter().map(|c| Cmd { name: c.name.clone(), desc: c.description.clone(), arg: None, source: c.source.clone() }).collect()).unwrap_or_default();
    let own = builtin().into_iter().filter(|b| !pi.iter().any(|c| c.name == b.name) && (tab.is_some() || NO_TAB.contains(&b.name.as_str())));
    // before pi has said what it has, the templates pith ships are what there is, and they lead
    if tab.is_none() { return bundled().iter().cloned().chain(own).collect(); }
    let mut out: Vec<Cmd> = own.collect();
    if pi.is_empty() { out.extend(bundled().iter().cloned()); } else { out.extend(pi); }
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
        "agents" => { app.update(cx, |a, cx| a.show(View::Agents, cx)); return true; }
        "image" if arg.is_empty() => { app.update(cx, |a, cx| a.show(View::Images, cx)); return true; }
        "mode" | "plan" if tab.is_none() => {
            let m = if name == "plan" { Some(Mode::Plan) } else { Mode::ALL.into_iter().find(|m| m.as_str() == arg) };
            match m {
                Some(m) => app.update(cx, |a, cx| a.save_settings(json!({ "mode": m.as_str() }), cx)),
                None => pick_mode(None, window, cx),
            }
            return true;
        }
        _ => {}
    }
    let Some(tab) = tab else {
        app.update(cx, |a, cx| a.toast(Level::Info, "Start a session first", cx));
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
        "copy" => match last_reply(tab.read(cx)) {
            Some(t) => { cx.write_to_clipboard(ClipboardItem::new_string(t)); app.update(cx, |a, cx| a.toast(Level::Info, "Copied the last reply", cx)); }
            None => app.update(cx, |a, cx| a.toast(Level::Info, "Nothing to copy yet", cx)),
        },
        "image" => tab.update(cx, |t, cx| t.send(format!("Generate this image with generate_image, then show it: {arg}"), vec![], crate::state::tab::How::Auto, cx)),
        // the side panel decides which of its views to show
        "diff" | "files" | "terminal" => app.update(cx, |a, cx| { a.side = true; cx.notify(); }),
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
