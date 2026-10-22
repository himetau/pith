// How each tool call shows up in the transcript: one quiet line, the useful part on click.
// a port of the old ui's tools.ts: reads show numbered code, edits a diff, bash its output (live while it runs)
use super::theme::pal;
use super::transcript::{code, md, media};
use crate::state::tab::{Tab, TabEvent, ToolBlock, ToolResult, ToolStatus};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Icon, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::Value;
use std::collections::HashSet;
use std::rc::Rc;

// a body shows this many lines until you ask for all of them
const PREVIEW: usize = 40;
// a running command shows its last few lines
const LIVE_TAIL: usize = 14;

pub fn dur(ms: u64) -> String {
    let s = ms / 1000;
    if ms < 1000 { format!("{ms}ms") } else if s < 60 { format!("{}s", s) } else if s < 3600 { format!("{}m {}s", s / 60, s % 60) } else { format!("{}h {}m", s / 3600, (s % 3600) / 60) }
}
pub fn tokens(n: u64) -> String {
    if n >= 1_000_000 { format!("{:.1}M tokens", n as f64 / 1e6) } else if n >= 1000 { format!("{}k tokens", n / 1000) } else { format!("{n} tokens") }
}
fn short_tokens(n: u64) -> String { if n >= 1_000_000 { format!("{:.1}M", n as f64 / 1e6) } else if n >= 1000 { format!("{:.1}k", n as f64 / 1e3) } else { n.to_string() } }
pub fn tilde(p: &str) -> String {
    let h = crate::core::sys::home().display().to_string();
    match p.strip_prefix(&h) { Some(r) => format!("~{r}"), None => p.to_string() }
}
fn rel(p: &str, cwd: &str) -> String {
    match p.strip_prefix(cwd.trim_end_matches('/')) { Some(r) if r.starts_with('/') => r[1..].to_string(), _ => tilde(p) }
}
fn one(s: &str, n: usize) -> String {
    let l = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if l.chars().count() > n { format!("{}…", l.chars().take(n - 1).collect::<String>()) } else { l }
}
fn plural(n: usize, w: &str) -> String { format!("{n} {w}{}", if n == 1 { "" } else { "s" }) }

// args arrive as a growing json string while the model writes them. good enough to show the path early
fn arg(tb: &ToolBlock, k: &str) -> String {
    if let Some(v) = tb.args[k].as_str() { return v.to_string(); }
    let key = format!("\"{k}\"");
    let Some(at) = tb.args_text.find(&key) else { return String::new() };
    let rest = &tb.args_text[at + key.len()..];
    let Some(q) = rest.find('"') else { return String::new() };
    let mut out = String::new();
    let mut esc = false;
    for c in rest[q + 1..].chars() {
        if esc { out.push(match c { 'n' => '\n', 't' => '\t', c => c }); esc = false; continue; }
        match c { '\\' => esc = true, '"' => break, c => out.push(c) }
    }
    out
}

fn images_of(r: Option<&ToolResult>) -> Vec<(String, String)> {
    r.and_then(|r| r.content.as_array()).into_iter().flatten().filter(|c| c["type"] == "image")
        .map(|c| (c["data"].as_str().unwrap_or("").to_string(), c["mimeType"].as_str().unwrap_or("image/png").to_string())).collect()
}

fn edit_pairs(a: &Value) -> Vec<(String, String)> {
    match a["edits"].as_array() {
        Some(e) => e.iter().map(|e| (e["oldText"].as_str().unwrap_or("").into(), e["newText"].as_str().unwrap_or("").into())).collect(),
        None if a["oldText"].is_string() || a["newText"].is_string() => vec![(a["oldText"].as_str().unwrap_or("").into(), a["newText"].as_str().unwrap_or("").into())],
        None => vec![],
    }
}

// the transcript's open/closed memory and how to flip it, handed down from the chat view
pub struct Ui<'a> {
    pub open: &'a HashSet<String>,
    pub closed: &'a HashSet<String>,
    pub toggle: Rc<dyn Fn(String, bool, &mut App)>,
    pub tab: &'a Entity<Tab>,
    pub cwd: &'a str,
}
impl Ui<'_> {
    // a row that opens itself (a live command, a small diff) stays open until you close it
    pub fn is_open(&self, key: &str, auto: bool) -> bool { self.open.contains(key) || (auto && !self.closed.contains(key)) }
    pub fn on(&self, key: String, auto: bool) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
        let was = self.is_open(&key, auto);
        let t = self.toggle.clone();
        move |_, _, cx| { cx.stop_propagation(); t(key.clone(), was, cx) }
    }
}

enum Body { None, Read, Write, Edit, Bash, Output, Browser, Image, Ask, Todos, Findings, Agents, Other }

struct Desc {
    icon: IconName,
    verb: String,
    target: String,
    mono: bool,
    extra: Option<String>,
    meta: Vec<(String, Hsla)>,
    auto: bool,
    body: Body,
}

fn describe(tb: &ToolBlock, cwd: &str, cx: &App) -> Desc {
    let p = pal(cx);
    let r = tb.result.as_ref().or(tb.partial.as_ref());
    let out = r.map(|r| r.text()).unwrap_or_default();
    let err = tb.result.as_ref().is_some_and(|r| r.is_error) || tb.status == ToolStatus::Error;
    let ok = tb.result.is_some() && !err;
    let d = |icon, verb: &str, target: String, body| Desc { icon, verb: verb.into(), target, mono: false, extra: None, meta: vec![], auto: false, body };
    match tb.name.as_str() {
        "read" => {
            let (off, lim) = (tb.args["offset"].as_u64(), tb.args["limit"].as_u64());
            let range = match (off, lim) { (None, None) => None, (o, Some(l)) => Some(format!(":{}-{}", o.unwrap_or(1), o.unwrap_or(1) + l - 1)), (Some(o), None) => Some(format!(":{o}")) };
            let imgs = !images_of(r).is_empty();
            Desc { meta: if imgs { vec![("image".into(), p.dim)] } else if ok { vec![(plural(code::count_lines(&out), "line"), p.dim)] } else { vec![] }, ..d(IconName::FileText, "Read", rel(&arg(tb, "path"), cwd) + range.as_deref().unwrap_or(""), Body::Read) }
        }
        "write" => {
            let c = arg(tb, "content");
            let meta = if !c.is_empty() { vec![(format!("+{}", code::count_lines(&c)), p.green)] } else if tb.status == ToolStatus::Streaming { vec![("writing…".into(), p.dim)] } else { vec![] };
            Desc { meta, ..d(IconName::FilePlus, "Wrote", rel(&arg(tb, "path"), cwd), Body::Write) }
        }
        "edit" => {
            let diff = tb.result.as_ref().and_then(|r| r.details["diff"].as_str());
            let n = edit_pairs(&tb.args).len();
            let meta = match diff {
                Some(df) => { let (a, x) = code::diff_counts(df); [(a, "+", p.green), (x, "−", p.red)].into_iter().filter(|(n, ..)| *n > 0).map(|(n, s, c)| (format!("{s}{n}"), c)).collect() }
                None if n > 1 => vec![(format!("{n} edits"), p.dim)],
                None => vec![],
            };
            Desc { meta, auto: diff.is_some_and(|d| code::diff_len(d) <= 40) && !err, ..d(IconName::FilePen, "Edited", rel(&arg(tb, "path"), cwd), Body::Edit) }
        }
        "bash" => {
            let exit = tb.result.as_ref().and_then(|r| r.details["exitCode"].as_i64());
            let meta = if err { vec![(exit.map(|e| format!("exit {e}")).unwrap_or("failed".into()), p.red)] } else if !out.trim().is_empty() { vec![(plural(code::count_lines(&out), "line"), p.dim)] } else { vec![] };
            let running = matches!(tb.status, ToolStatus::Running);
            Desc { mono: true, meta, auto: running && !out.is_empty(), ..d(IconName::SquareTerminal, if running { "Running" } else { "Ran" }, one(&arg(tb, "command"), 140), Body::Bash) }
        }
        "grep" | "find" | "glob" | "ls" => {
            let (verb, target) = if tb.name == "ls" { ("Listed", rel(&[arg(tb, "path")].concat(), cwd)) } else { ("Searched", one(&arg(tb, "pattern"), 80)) };
            let path = arg(tb, "path");
            let extra = (tb.name != "ls" && !path.is_empty()).then(|| format!("in {}", rel(&path, cwd)));
            let n = code::count_lines(out.trim());
            let word = match (tb.name == "grep", n == 1) { (true, true) => "1 match".to_string(), (true, false) => format!("{n} matches"), _ => plural(n, "result") };
            let meta = if ok { vec![(word, p.dim)] } else { vec![] };
            Desc { mono: tb.name != "ls", extra, meta, ..d(if tb.name == "ls" { IconName::FolderOpen } else { IconName::Search }, verb, if target.is_empty() { ".".into() } else { target }, Body::Output) }
        }
        "todo_write" => {
            let todos = tb.args["todos"].as_array().cloned().unwrap_or_default();
            let done = todos.iter().filter(|t| t["status"] == "completed").count();
            let cur = todos.iter().find(|t| t["status"] == "in_progress").and_then(|t| t["content"].as_str()).map(|s| one(s, 80));
            Desc { extra: cur, meta: vec![(format!("{done} of {}", todos.len()), p.dim)], ..d(IconName::ListTodo, "Updated the plan", String::new(), Body::Todos) }
        }
        "report_findings" => {
            let n = tb.args["findings"].as_array().map(|a| a.len()).unwrap_or(0);
            Desc { auto: true, ..d(IconName::ShieldCheck, "Reviewed", plural(n, "finding"), Body::Findings) }
        }
        "subagent" | "summon_agent" | "task" => {
            let a = &tb.args;
            let planned = planned(a);
            let mode = r.and_then(|r| r.details["mode"].as_str()).map(String::from)
                .unwrap_or_else(|| if a["chain"].is_array() { "chain".into() } else if a["tasks"].is_array() { "parallel".into() } else { "single".into() });
            let names = planned.iter().map(|(g, _)| g.as_str()).filter(|g| !g.is_empty()).collect::<Vec<_>>().join(", ");
            let n = planned.len().max(r.and_then(|r| r.details["results"].as_array()).map(|a| a.len()).unwrap_or(0));
            let target = if n <= 1 { names.clone() } else { format!("{n} agents, {mode}") };
            Desc { extra: (n > 1 && !names.is_empty()).then_some(names), auto: true, ..d(IconName::Bot, "Delegated to", target, Body::Agents) }
        }
        "browser" => {
            let action = arg(tb, "action");
            let what = match action.as_str() { "navigate" => arg(tb, "url"), "type" => format!("“{}”", arg(tb, "text")), _ => ["ref", "selector", "key", "script"].iter().map(|k| arg(tb, k)).find(|s| !s.is_empty()).unwrap_or_default() };
            let shots = !images_of(r).is_empty();
            Desc { extra: (!what.is_empty()).then(|| one(&what, 90)), meta: if shots { vec![("screenshot".into(), p.dim)] } else { vec![] }, auto: shots, ..d(IconName::Globe, "Browser", if action.is_empty() { "…".into() } else { action }, Body::Browser) }
        }
        "preview" => {
            let (path, url) = (arg(tb, "path"), arg(tb, "url"));
            d(IconName::Eye, "Previewed", if path.is_empty() { url } else { rel(&path, cwd) }, Body::None)
        }
        "generate_image" => {
            let file = tb.result.as_ref().and_then(|r| r.details["files"][0].as_str()).map(|f| f.rsplit('/').next().unwrap_or(f).to_string());
            Desc { meta: file.map(|f| vec![(f, p.dim)]).unwrap_or_default(), auto: true, ..d(IconName::Image, "Generated an image", one(&arg(tb, "prompt"), 100), Body::Image) }
        }
        "ask_user" => Desc { auto: tb.result.is_some(), ..d(IconName::MessageCircleQuestionMark, "Asked", one(&arg(tb, "question"), 110), Body::Ask) },
        n => {
            let name = n.strip_prefix("mcp__").unwrap_or(n).replace("__", ":");
            let args = if tb.args.is_object() { one(&tb.args.to_string(), 100) } else { String::new() };
            let meta = if ok && !out.trim().is_empty() { vec![(plural(code::count_lines(&out), "line"), p.dim)] } else { vec![] };
            Desc { mono: true, extra: (!args.is_empty() && args != "{}").then_some(args), meta, ..d(IconName::Wrench, "", name, Body::Other) }
        }
    }
}

fn planned(a: &Value) -> Vec<(String, String)> {
    let list = a["tasks"].as_array().or(a["chain"].as_array());
    match list {
        Some(l) => l.iter().map(|t| (t["agent"].as_str().unwrap_or("").into(), t["task"].as_str().unwrap_or("").into())).collect(),
        None if a["agent"].is_string() || a["task"].is_string() => vec![(a["agent"].as_str().unwrap_or("").into(), a["task"].as_str().unwrap_or("").into())],
        None => vec![],
    }
}

fn lines_cut(s: &str, n: usize) -> (String, usize) {
    let total = code::count_lines(s);
    if total <= n { return (s.to_string(), total); }
    (s.split('\n').take(n).collect::<Vec<_>>().join("\n"), total)
}
fn last_lines(s: &str, n: usize) -> String {
    let t = s.trim_end_matches('\n');
    let v: Vec<&str> = t.split('\n').collect();
    if v.len() <= n { t.to_string() } else { v[v.len() - n..].join("\n") }
}

// "show all 312 lines" under a body that was cut short
fn more(id: &str, total: usize, shown: usize, all: bool, ui: &Ui, cx: &App) -> Option<AnyElement> {
    if total <= shown && !all { return None; }
    if all && total <= PREVIEW { return None; }
    let p = pal(cx);
    let key = format!("{id}:all");
    let label = if all { "Show less".to_string() } else { format!("Show all {total} lines") };
    Some(h_flex().id(SharedString::from(format!("{id}-more"))).px_1().text_xs().text_color(p.dim).cursor_pointer().hover(|s| s.text_color(cx.theme().foreground))
        .child(label).on_click(ui.on(key, false)).into_any_element())
}

fn body(tb: &ToolBlock, desc: &Desc, ui: &Ui, cx: &App) -> Option<AnyElement> {
    let p = pal(cx);
    let id = format!("tb-{}", tb.id);
    let all = ui.open.contains(&format!("{id}:all"));
    let limit = if all { code::MAX_SHOWN } else { PREVIEW };
    let r = tb.result.as_ref().or(tb.partial.as_ref());
    let out = r.map(|r| r.text()).unwrap_or_default();
    let err = tb.result.as_ref().is_some_and(|r| r.is_error);
    let col = v_flex().w_full().gap_1();
    let plain = |text: &str, cx: &App| -> AnyElement {
        let (t, total) = lines_cut(text, limit);
        v_flex().gap_1().child(code::output(SharedString::from(id.clone()), &t, cx)).children(more(&id, total, limit.min(total), all, ui, cx)).into_any_element()
    };
    Some(match desc.body {
        Body::None => return None,
        Body::Read => {
            let imgs = images_of(r);
            if !imgs.is_empty() { media::strip(imgs.iter().map(|(d, m)| (d.as_str(), m.as_str())), px(220.), cx).into_any_element() }
            else if r.is_none() { return None }
            else if err { plain(&out, cx) }
            else {
                let path = arg(tb, "path");
                let total = code::count_lines(&out);
                col.child(code::code(id.clone(), &out, code::lang_of(&path), Some(tb.args["offset"].as_u64().unwrap_or(1) as usize), limit, cx))
                    .children(more(&id, total, limit.min(total), all, ui, cx)).into_any_element()
            }
        }
        Body::Write => {
            if err { plain(&out, cx) } else {
                let c = arg(tb, "content");
                let total = code::count_lines(&c);
                col.child(code::code(id.clone(), &c, code::lang_of(&arg(tb, "path")), Some(1), limit, cx)).children(more(&id, total, limit.min(total), all, ui, cx)).into_any_element()
            }
        }
        Body::Edit => {
            let lang = code::lang_of(&arg(tb, "path"));
            let pairs = edit_pairs(&tb.args);
            let diff = tb.result.as_ref().filter(|_| !err).and_then(|r| r.details["diff"].as_str());
            match diff {
                Some(df) => {
                    let total = code::diff_len(df);
                    col.child(code::diff(id.clone(), df, lang, limit, cx)).children(more(&id, total, limit.min(total), all, ui, cx)).into_any_element()
                }
                None => col.when(err, |c| c.child(code::output(SharedString::from(format!("{id}-err")), &out, cx)))
                    .when(!pairs.is_empty(), |c| c.child(code::edits(id.clone(), &pairs, lang, limit, cx))).into_any_element(),
            }
        }
        Body::Bash => {
            let cmd = arg(tb, "command");
            let running = tb.status == ToolStatus::Running;
            col.when(cmd.contains('\n') || cmd.chars().count() > 140, |c| c.child(code::code(format!("{id}-cmd"), &cmd, Some("bash"), None, code::MAX_SHOWN, cx)))
                .child(if running { code::output(SharedString::from(id.clone()), &last_lines(&out, LIVE_TAIL), cx).into_any_element() } else { plain(&out, cx) })
                .into_any_element()
        }
        Body::Output => plain(&out, cx),
        Body::Browser => {
            let imgs = images_of(r);
            col.when(!imgs.is_empty(), |c| c.child(media::strip(imgs.iter().map(|(d, m)| (d.as_str(), m.as_str())), px(240.), cx)))
                .when(!out.trim().is_empty() && arg(tb, "action") != "screenshot", |c| c.child(plain(&out, cx))).into_any_element()
        }
        Body::Image => {
            let imgs = images_of(tb.result.as_ref());
            if !imgs.is_empty() { media::strip(imgs.iter().map(|(d, m)| (d.as_str(), m.as_str())), px(260.), cx).into_any_element() }
            else if err { plain(&out, cx) }
            else if matches!(tb.status, ToolStatus::Running | ToolStatus::Queued | ToolStatus::Streaming) {
                h_flex().gap_2().items_center().text_sm().text_color(p.dim).child(Spinner::new().xsmall().color(p.dim)).child("Painting").into_any_element()
            } else { return None }
        }
        Body::Ask => {
            let res = tb.result.as_ref()?;
            let answer = res.details["answer"].as_str().map(String::from).unwrap_or_else(|| if res.is_error { res.text() } else { "Dismissed".into() });
            v_flex().gap_1().pl_6().text_sm()
                .when(arg(tb, "question").chars().count() > 110, |d| d.child(div().text_color(p.mid).child(arg(tb, "question"))))
                .child(h_flex().gap_2().child(div().text_color(p.dim).child("Answer")).child(div().text_color(cx.theme().foreground).child(answer)))
                .into_any_element()
        }
        Body::Todos => {
            let todos = tb.args["todos"].as_array().cloned().unwrap_or_default();
            div().pl_6().child(todo_list(todos.iter().map(|t| (t["content"].as_str().unwrap_or(""), t["status"].as_str().unwrap_or(""))), cx)).into_any_element()
        }
        Body::Findings => findings(tb, ui, cx),
        Body::Agents => agents(tb, ui, cx),
        Body::Other => {
            let pretty = serde_json::to_string_pretty(&tb.args).unwrap_or_default();
            let imgs = images_of(r);
            col.when(pretty != "{}" && pretty != "null", |c| c.child(code::code(format!("{id}-args"), &pretty, Some("json"), None, 60, cx)))
                .when(!imgs.is_empty(), |c| c.child(media::strip(imgs.iter().map(|(d, m)| (d.as_str(), m.as_str())), px(220.), cx)))
                .when(!out.trim().is_empty(), |c| c.child(plain(&out, cx))).into_any_element()
        }
    })
}

pub fn todo_list<'a>(todos: impl IntoIterator<Item = (&'a str, &'a str)>, cx: &App) -> Div {
    let p = pal(cx);
    v_flex().gap_1().text_sm().children(todos.into_iter().map(|(text, status)| {
        let (icon, ic, tc) = match status {
            "completed" => (IconName::CircleCheck, p.green, p.dim),
            "in_progress" => (IconName::CircleDot, cx.theme().primary, cx.theme().foreground),
            _ => (IconName::Circle, p.dim, p.mid),
        };
        h_flex().gap_2().items_start()
            .child(div().flex_none().pt(px(2.)).child(Icon::new(icon).xsmall().text_color(ic)))
            .child(div().min_w_0().text_color(tc).when(status == "completed", |d| d.line_through()).when(status == "in_progress", |d| d.font_weight(FontWeight::MEDIUM)).child(text.to_string()))
    }))
}

fn findings(tb: &ToolBlock, ui: &Ui, cx: &App) -> AnyElement {
    let p = pal(cx);
    let a = &tb.args;
    let list = a["findings"].as_array().cloned().unwrap_or_default();
    let mut col = v_flex().w_full().gap_3().pl_6();
    if let Some(s) = a["summary"].as_str() { col = col.child(md::markdown(SharedString::from(format!("{}-sum", tb.id)), s.to_string(), cx)); }
    if list.is_empty() { col = col.child(div().text_sm().text_color(p.dim).child("Nothing held up")); }
    for (i, f) in list.iter().enumerate() {
        let sev = f["severity"].as_str().unwrap_or("").to_string();
        let sc = match sev.as_str() { "critical" | "high" => p.red, "medium" => p.gold, _ => p.dim };
        let file = f["file"].as_str().unwrap_or("");
        let loc = format!("{}{}", rel(file, ui.cwd), f["line"].as_u64().map(|l| format!(":{l}")).unwrap_or_default());
        let title = f["title"].as_str().unwrap_or("").to_string();
        let ask = format!("Fix finding {}: {title} ({loc})", i + 1);
        let tab = ui.tab.clone();
        col = col.child(v_flex().gap_1().p_3().rounded(cx.theme().radius_lg).border_1().border_color(p.hairline)
            .child(h_flex().gap_2().items_center().text_sm()
                .child(div().flex_none().text_xs().text_color(p.dim).font_family(cx.theme().mono_font_family.clone()).child(format!("{:02}", i + 1)))
                .child(div().flex_none().text_xs().font_weight(FontWeight::MEDIUM).text_color(sc).child(sev))
                .when_some(f["category"].as_str(), |d, c| d.child(div().flex_none().text_xs().text_color(p.dim).child(c.to_string())))
                .child(div().min_w_0().font_weight(FontWeight::SEMIBOLD).text_color(cx.theme().foreground).child(title)))
            .when(!file.is_empty(), |d| d.child(div().text_xs().text_color(p.dim).font_family(cx.theme().mono_font_family.clone()).child(loc.clone())))
            .when_some(f["detail"].as_str(), |d, t| d.child(md::markdown(SharedString::from(format!("{}-f{i}", tb.id)), t.to_string(), cx).text_sm()))
            .when_some(f["fix"].as_str(), |d, t| d.child(h_flex().gap_2().items_start().text_sm().child(div().flex_none().text_color(p.dim).child("Fix")).child(md::markdown(SharedString::from(format!("{}-x{i}", tb.id)), t.to_string(), cx))))
            .child(h_flex().child(Button::new(SharedString::from(format!("{}-fix{i}", tb.id))).label("Fix this").xsmall().ghost()
                .on_click(move |_, _, cx| tab.update(cx, |_, cx| cx.emit(TabEvent::Draft(ask.clone())))))));
    }
    col.into_any_element()
}

fn agents(tb: &ToolBlock, ui: &Ui, cx: &App) -> AnyElement {
    let p = pal(cx);
    let r = tb.result.as_ref().or(tb.partial.as_ref());
    let results = r.and_then(|r| r.details["results"].as_array()).cloned().unwrap_or_default();
    let planned = planned(&tb.args);
    let running = matches!(tb.status, ToolStatus::Running | ToolStatus::Queued);
    let n = results.len().max(planned.len());
    let mut col = v_flex().w_full().gap_1p5().pl_6();
    for i in 0..n {
        let res = results.get(i).cloned().unwrap_or(Value::Null);
        let agent = res["agent"].as_str().map(String::from).or_else(|| planned.get(i).map(|x| x.0.clone())).unwrap_or_default();
        let task = res["task"].as_str().map(String::from).or_else(|| planned.get(i).map(|x| x.1.clone())).unwrap_or_default();
        let (mut last, mut doing) = (String::new(), String::new());
        for m in res["messages"].as_array().into_iter().flatten().filter(|m| m["role"] == "assistant") {
            for c in m["content"].as_array().into_iter().flatten() {
                if c["type"] == "text" { if let Some(t) = c["text"].as_str().filter(|t| !t.is_empty()) { last = t.to_string(); } }
                if c["type"] == "toolCall" { doing = format!("{} {}", c["name"].as_str().unwrap_or(""), one(&super::transcript::dock::gist(&c["arguments"]), 60)); }
            }
        }
        let failed = res["exitCode"].as_i64().is_some_and(|e| e > 0) || matches!(res["stopReason"].as_str(), Some("error" | "aborted"));
        let done = !running || (res["exitCode"].as_i64().is_some_and(|e| e >= 0) && (!last.is_empty() || failed));
        let u = &res["usage"];
        let meta = (!u.is_null()).then(|| format!("{} · {} tokens · ${:.2}", plural(u["turns"].as_u64().unwrap_or(0) as usize, "turn"), short_tokens(u["input"].as_u64().unwrap_or(0) + u["output"].as_u64().unwrap_or(0)), u["cost"].as_f64().unwrap_or(0.)));
        let key = format!("{}:agent{i}", tb.id);
        let open = ui.is_open(&key, false);
        let status: AnyElement = if !done { Spinner::new().xsmall().color(p.dim).into_any_element() }
            else { div().size(px(7.)).mx(px(3.)).rounded_full().bg(if failed { p.red } else { p.green }).into_any_element() };
        col = col.child(v_flex().gap_1().px_3().py_2().rounded(cx.theme().radius_lg).border_1().border_color(p.hairline)
            .child(h_flex().id(SharedString::from(key.clone())).gap_2().items_center().min_w_0().text_sm().when(!last.is_empty(), |d| d.cursor_pointer()).child(status)
                .child(div().flex_none().font_weight(FontWeight::MEDIUM).text_color(cx.theme().foreground).child(if agent.is_empty() { "agent".to_string() } else { agent }))
                .child(div().min_w_0().truncate().text_color(p.mid).child(one(&task, 120)))
                .child(div().flex_1())
                .when_some(meta, |d, m| d.child(div().flex_none().text_xs().text_color(p.dim).child(m)))
                .when(!last.is_empty(), |d| d.child(Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronRight }).xsmall().text_color(p.dim)).on_click(ui.on(key.clone(), false))))
            .when(!done && !doing.is_empty(), |d| d.child(div().text_xs().text_color(p.dim).font_family(cx.theme().mono_font_family.clone()).child(format!("› {doing}"))))
            .when(failed, |d| d.when_some(res["errorMessage"].as_str(), |d, e| d.child(div().text_xs().text_color(p.red).child(one(e, 300)))))
            .when(open && !last.is_empty(), |d| d.child(div().pt_1().child(md::markdown(SharedString::from(format!("{key}-out")), last.clone(), cx)))));
    }
    col.into_any_element()
}

pub fn row(tb: &ToolBlock, ui: &Ui, cx: &App) -> AnyElement {
    let p = pal(cx);
    let desc = describe(tb, ui.cwd, cx);
    let open = ui.is_open(&tb.id, desc.auto);
    let failed = tb.status == ToolStatus::Error || tb.result.as_ref().is_some_and(|r| r.is_error);
    let busy = matches!(tb.status, ToolStatus::Running | ToolStatus::Streaming | ToolStatus::Queued);
    let has_body = !matches!(desc.body, Body::None);
    let lead: AnyElement = if busy { Spinner::new().xsmall().color(p.dim).into_any_element() }
        else { Icon::new(desc.icon.clone()).xsmall().text_color(if failed { p.red } else { p.dim }).into_any_element() };
    let mono = cx.theme().mono_font_family.clone();
    let msz = cx.theme().mono_font_size * 0.92;
    let time = tb.ms.filter(|ms| !busy && *ms >= 400).map(dur);
    // a preview row's one action: put it back in the side pane
    let show = (tb.name == "preview").then(|| {
        let (tab, args) = (ui.tab.clone(), tb.args.clone());
        Button::new(SharedString::from(format!("show-{}", tb.id))).label("Show").xsmall().ghost().on_click(move |_, _, cx| {
            cx.stop_propagation();
            tab.update(cx, |t, cx| {
                let mut a = args.clone();
                if let Some(path) = a["path"].as_str().filter(|p| !p.starts_with('/')) { a["path"] = Value::from(format!("{}/{}", t.cwd.trim_end_matches('/'), path.trim_start_matches("./"))); }
                t.preview = Some(a);
                cx.emit(TabEvent::Preview);
            })
        })
    });
    let head = h_flex().id(SharedString::from(format!("tool-{}", tb.id))).gap_2().items_center().min_w_0().h(px(26.)).text_sm()
        .rounded(cx.theme().radius).px_1p5().mx(px(-6.)).when(has_body, |s| s.hover(|s| s.bg(cx.theme().muted)))
        .child(div().flex_none().w(px(14.)).flex().justify_center().child(lead))
        .when(!desc.verb.is_empty(), |s| s.child(div().flex_none().text_color(cx.theme().foreground).child(desc.verb.clone())))
        .when(!desc.target.is_empty(), |s| s.child(div().min_w_0().truncate().text_color(if desc.verb.is_empty() { cx.theme().foreground } else { p.mid })
            .when(desc.mono, |s| s.font_family(mono.clone()).text_size(msz)).child(desc.target.clone())))
        .when_some(desc.extra.clone(), |s, e| s.child(div().min_w_0().truncate().text_color(p.dim).when(desc.mono && tb.name != "grep" && tb.name != "find", |s| s.font_family(mono.clone()).text_size(msz)).child(e)))
        .children(desc.meta.iter().map(|(m, c)| div().flex_none().text_xs().text_color(*c).child(m.clone())))
        .child(div().flex_1())
        .children(show)
        .when_some(time, |s, t| s.child(div().flex_none().text_xs().text_color(p.dim).child(t)))
        .when(has_body, |s| s.cursor_pointer().on_click(ui.on(tb.id.clone(), desc.auto)));
    v_flex().w_full().gap_1()
        .child(head)
        .when(open && has_body, |s| s.children(body(tb, &desc, ui, cx).map(|b| div().w_full().pb_1().child(b))))
        .into_any_element()
}

// a command you ran yourself with ! in the composer
pub fn shell(id: u64, command: &str, output: &str, exit: Option<i64>, running: bool, cancelled: bool, excluded: bool, cx: &App) -> impl IntoElement {
    let p = pal(cx);
    let bad = exit.is_some_and(|e| e != 0);
    v_flex().w_full().gap_1()
        .child(h_flex().gap_2().items_center().min_w_0().text_sm()
            .child(div().flex_none().w(px(14.)).flex().justify_center().child(if running { Spinner::new().xsmall().color(p.dim).into_any_element() } else { Icon::new(IconName::SquareTerminal).xsmall().text_color(if bad { p.red } else { p.dim }).into_any_element() }))
            .child(div().min_w_0().truncate().font_family(cx.theme().mono_font_family.clone()).text_size(cx.theme().mono_font_size * 0.92).text_color(cx.theme().foreground).child(format!("$ {command}")))
            .when(excluded, |s| s.child(div().flex_none().text_xs().text_color(p.dim).child("not sent to the model")))
            .child(div().flex_1())
            .when(cancelled, |s| s.child(div().flex_none().text_xs().text_color(p.dim).child("cancelled")))
            .when_some(exit.filter(|e| *e != 0 && !cancelled), |s, e| s.child(div().flex_none().text_xs().text_color(p.red).child(format!("exit {e}")))))
        .when(!output.is_empty() || !running, |d| {
            let t = if running { last_lines(output, LIVE_TAIL) } else { lines_cut(output, code::MAX_SHOWN).0 };
            d.child(code::output(SharedString::from(format!("sh-{id}")), &t, cx))
        })
}
