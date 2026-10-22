// How each tool call shows up in the transcript: one quiet line, the useful part on click
use super::theme::pal;
use crate::state::tab::{ToolBlock, ToolStatus};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Icon, Sizable};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::assets::IconName;
use serde_json::Value;

pub fn dur(ms: u64) -> String {
    let s = ms / 1000;
    if ms < 1000 { format!("{ms}ms") } else if s < 60 { format!("{}s", s) } else if s < 3600 { format!("{}m {}s", s / 60, s % 60) } else { format!("{}h {}m", s / 3600, (s % 3600) / 60) }
}
pub fn tokens(n: u64) -> String {
    if n >= 1_000_000 { format!("{:.1}M tokens", n as f64 / 1e6) } else if n >= 1000 { format!("{}k tokens", n / 1000) } else { format!("{n} tokens") }
}
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
fn lines(s: &str) -> usize { if s.is_empty() { 0 } else { s.trim_end_matches('\n').split('\n').count() } }
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

struct Desc { icon: IconName, verb: &'static str, target: String, meta: Option<String>, mono_target: bool }

fn describe(tb: &ToolBlock, cwd: &str) -> Desc {
    let r = tb.result.as_ref().or(tb.partial.as_ref());
    let out = r.map(|r| r.text()).unwrap_or_default();
    let ok = tb.result.as_ref().is_some_and(|r| !r.is_error);
    match tb.name.as_str() {
        "read" => Desc { icon: IconName::FileText, verb: "Read", target: rel(&arg(tb, "path"), cwd), meta: ok.then(|| plural(lines(&out), "line")), mono_target: false },
        "write" => {
            let c = arg(tb, "content");
            Desc { icon: IconName::FilePlus, verb: "Wrote", target: rel(&arg(tb, "path"), cwd), meta: (!c.is_empty()).then(|| format!("+{}", lines(&c))), mono_target: false }
        }
        "edit" => {
            let diff = tb.result.as_ref().and_then(|r| r.details["diff"].as_str().map(String::from));
            let meta = diff.map(|d| {
                let (mut a, mut x) = (0, 0);
                for l in d.lines() { if l.starts_with('+') { a += 1 } else if l.starts_with('-') { x += 1 } }
                format!("+{a} −{x}")
            });
            Desc { icon: IconName::FilePen, verb: "Edited", target: rel(&arg(tb, "path"), cwd), meta, mono_target: false }
        }
        "bash" => {
            let exit = tb.result.as_ref().and_then(|r| r.details["exitCode"].as_i64());
            let meta = if tb.result.as_ref().is_some_and(|r| r.is_error) { Some(exit.map(|e| format!("exit {e}")).unwrap_or("failed".into())) } else if !out.is_empty() { Some(plural(lines(&out), "line")) } else { None };
            Desc { icon: IconName::SquareTerminal, verb: "Ran", target: one(&arg(tb, "command"), 110), meta, mono_target: true }
        }
        "grep" => Desc { icon: IconName::Search, verb: "Searched", target: one(&arg(tb, "pattern"), 80), meta: ok.then(|| plural(lines(&out), "match")), mono_target: true },
        "find" | "glob" | "ls" => Desc { icon: IconName::FolderSearch, verb: "Listed", target: one(&[arg(tb, "pattern"), arg(tb, "path")].concat(), 80), meta: ok.then(|| plural(lines(&out), "entry")), mono_target: true },
        "todo_write" => {
            let n = tb.args["todos"].as_array().map(|a| a.len()).unwrap_or(0);
            Desc { icon: IconName::ListTodo, verb: "Updated todos", target: String::new(), meta: (n > 0).then(|| plural(n, "item")), mono_target: false }
        }
        "browser" => Desc { icon: IconName::Globe, verb: "Browser", target: one(&[arg(tb, "action"), " ".into(), arg(tb, "url")].concat(), 80), meta: None, mono_target: false },
        "preview" => Desc { icon: IconName::Eye, verb: "Previewed", target: rel(&[arg(tb, "path"), arg(tb, "url")].concat(), cwd), meta: None, mono_target: false },
        "generate_image" => Desc { icon: IconName::Image, verb: "Generated image", target: one(&arg(tb, "prompt"), 80), meta: None, mono_target: false },
        "ask_user" => Desc { icon: IconName::MessageCircleQuestionMark, verb: "Asked", target: one(&arg(tb, "question"), 90), meta: None, mono_target: false },
        "subagent" | "task" => Desc { icon: IconName::Bot, verb: "Agent", target: one(&[arg(tb, "agent"), " · ".into(), arg(tb, "task")].concat(), 90), meta: None, mono_target: false },
        n => Desc { icon: IconName::Wrench, verb: "Tool", target: n.to_string(), meta: None, mono_target: true },
    }
}

pub fn code_block(text: &str, cx: &App) -> Div {
    let p = pal(cx);
    let t = if text.len() > 40_000 { format!("{}\n…", &text[..text.floor_char_boundary(40_000)]) } else { text.to_string() };
    div().w_full().max_h(px(360.)).overflow_hidden().rounded(cx.theme().radius).bg(p.code_bg).border_1().border_color(p.hairline)
        .px_3().py_2().font_family(cx.theme().mono_font_family.clone()).text_size(cx.theme().mono_font_size * 0.92).text_color(p.mid)
        .child(if t.is_empty() { "(no output)".to_string() } else { t })
}

fn diff_block(diff: &str, cx: &App) -> Div {
    let p = pal(cx);
    let mut col = v_flex().w_full().rounded(cx.theme().radius).bg(p.code_bg).border_1().border_color(p.hairline).py_1()
        .font_family(cx.theme().mono_font_family.clone()).text_size(cx.theme().mono_font_size * 0.92);
    for l in diff.lines().take(400) {
        let (bg, fg) = if l.starts_with('+') { (p.green.opacity(0.12), p.green) } else if l.starts_with('-') { (p.red.opacity(0.12), p.red) } else { (transparent_black(), p.mid) };
        col = col.child(div().px_3().bg(bg).text_color(fg).child(if l.is_empty() { " ".to_string() } else { l.replace('\t', "  ") }));
    }
    col
}

fn body(tb: &ToolBlock, cx: &App) -> Div {
    let r = tb.result.as_ref().or(tb.partial.as_ref());
    let out = r.map(|r| r.text()).unwrap_or_default();
    let err = tb.result.as_ref().is_some_and(|r| r.is_error);
    match tb.name.as_str() {
        "edit" if !err => match tb.result.as_ref().and_then(|r| r.details["diff"].as_str()) {
            Some(d) => diff_block(d, cx),
            None => code_block(&edits_text(&tb.args), cx),
        },
        "write" if !err => code_block(&arg(tb, "content"), cx),
        "bash" => {
            let cmd = arg(tb, "command");
            v_flex().gap_1().when(cmd.contains('\n') || cmd.len() > 110, |d| d.child(code_block(&cmd, cx))).child(code_block(&tail(&out, 6000), cx))
        }
        _ => code_block(&out, cx),
    }
}

fn edits_text(a: &Value) -> String {
    let edits: Vec<(String, String)> = match a["edits"].as_array() {
        Some(e) => e.iter().map(|e| (e["oldText"].as_str().unwrap_or("").into(), e["newText"].as_str().unwrap_or("").into())).collect(),
        None => vec![(a["oldText"].as_str().unwrap_or("").into(), a["newText"].as_str().unwrap_or("").into())],
    };
    edits.iter().map(|(o, n)| o.lines().map(|l| format!("- {l}")).chain(n.lines().map(|l| format!("+ {l}"))).collect::<Vec<_>>().join("\n")).collect::<Vec<_>>().join("\n…\n")
}

fn tail(s: &str, n: usize) -> String {
    if s.len() <= n { return s.to_string(); }
    let mut at = s.len() - n;
    while !s.is_char_boundary(at) { at += 1; }
    format!("…{}", &s[at..])
}

pub fn row(tb: &ToolBlock, cwd: &str, open: bool, cx: &App, on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> impl IntoElement {
    let p = pal(cx);
    let d = describe(tb, cwd);
    let failed = tb.status == ToolStatus::Error;
    let busy = matches!(tb.status, ToolStatus::Running | ToolStatus::Streaming | ToolStatus::Queued);
    let status: AnyElement = if busy { Spinner::new().xsmall().color(p.dim).into_any_element() }
        else if failed { Icon::new(IconName::CircleX).xsmall().text_color(p.red).into_any_element() }
        else { div().into_any_element() };
    let meta = d.meta.clone().unwrap_or_default();
    let meta_color = if meta.starts_with('+') { p.green } else if failed { p.red } else { p.dim };
    v_flex().w_full().gap_1()
        .child(
            h_flex().id(SharedString::from(format!("tool-{}", tb.id))).gap_2().items_center().min_w_0().text_sm().text_color(p.mid).cursor_pointer()
                .rounded(cx.theme().radius).px_1().mx(px(-4.)).hover(|s| s.bg(cx.theme().muted))
                .child(Icon::new(d.icon).xsmall().text_color(p.dim))
                .child(div().flex_none().text_color(cx.theme().foreground).child(d.verb))
                .child(div().min_w_0().truncate().text_color(p.mid).when(d.mono_target, |s| s.font_family(cx.theme().mono_font_family.clone()).text_size(cx.theme().mono_font_size * 0.92)).child(d.target))
                .when(!meta.is_empty(), |s| s.child(div().flex_none().text_xs().text_color(meta_color).child(meta)))
                .child(div().flex_1())
                .when_some(tb.ms.filter(|_| !busy), |s, ms| s.child(div().flex_none().text_xs().text_color(p.dim).child(dur(ms))))
                .child(status)
                .on_click(on_click),
        )
        .when(open, |s| s.child(body(tb, cx)))
}

pub fn shell(command: &str, output: &str, exit: Option<i64>, running: bool, cx: &App) -> impl IntoElement {
    let p = pal(cx);
    v_flex().w_full().gap_1()
        .child(h_flex().gap_2().text_sm().child(Icon::new(IconName::SquareTerminal).xsmall().text_color(p.dim))
            .child(div().font_family(cx.theme().mono_font_family.clone()).text_color(cx.theme().foreground).child(format!("$ {command}")))
            .child(div().flex_1())
            .when(running, |s| s.child(Spinner::new().xsmall().color(p.dim)))
            .when_some(exit.filter(|e| *e != 0), |s, e| s.child(div().text_xs().text_color(p.red).child(format!("exit {e}")))))
        .child(code_block(&tail(output, 8000), cx))
}
