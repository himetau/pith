// How each tool call shows up in the transcript (tools.ts, chat.css .trow): one line by default, the useful part on
// click. a status dot, a coloured verb, the target, and on the right what came back and how long it took
use super::theme::{em, geo, mix, pal, u, Pal};
use super::transcript::text::{sel, OnClick};
use super::transcript::{code, md, media};
use crate::state::app::Pith;
use crate::state::tab::{Tab, TabEvent, ToolBlock, ToolResult, ToolStatus};
use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::Value;
use std::collections::HashSet;
use std::ops::Range;
use std::rc::Rc;

// lib/fmt.ts dur: 299ms, 4.2s, 12s, 1m05s, 1h02m
pub fn dur(ms: u64) -> String {
    if ms < 1000 { return format!("{ms}ms"); }
    let s = ms as f64 / 1000.;
    if s < 60. { return if s < 10. { format!("{s:.1}s") } else { format!("{}s", s.round() as u64) }; }
    let m = (s / 60.).floor() as u64;
    if m < 60 { return format!("{m}m{:02}s", (s % 60.).round() as u64); }
    format!("{}h{:02}m", m / 60, m % 60)
}
// lib/fmt.ts tokens: 980, 1.2k, 34k, 1.20M
pub fn tok(n: u64) -> String {
    match n { 0 => "0".into(), 1..=999 => n.to_string(), 1000..=9_999 => format!("{:.1}k", n as f64 / 1e3), 10_000..=999_999 => format!("{}k", (n as f64 / 1e3).round()), 1_000_000..=9_999_999 => format!("{:.2}M", n as f64 / 1e6), _ => format!("{:.1}M", n as f64 / 1e6) }
}
// lib/fmt.ts cost
pub fn cost(n: f64) -> String {
    if n <= 0. { "$0".into() } else if n < 0.01 { format!("${n:.4}") } else if n < 10. { format!("${n:.2}") } else if n < 1000. { format!("${n:.1}") } else { format!("${}", n.round()) }
}
pub fn tilde(p: &str) -> String {
    let h = crate::core::sys::home().display().to_string();
    match p.strip_prefix(&h) { Some(r) => format!("~{r}"), None => p.to_string() }
}
pub fn one(s: &str, n: usize) -> String {
    let l = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if l.chars().count() > n { format!("{}…", l.chars().take(n - 1).collect::<String>()) } else { l }
}
fn n_lines(s: &str, word: &str) -> String { let n = code::count_lines(s); format!("{n} {word}{}", if n == 1 { "" } else { "s" }) }

// args arrive as a growing json string while the model writes them. good enough to show the path early
fn arg(tb: &ToolBlock, k: &str) -> String {
    if let Some(v) = tb.args[k].as_str() { return v.to_string(); }
    if tb.args.as_object().is_some_and(|o| !o.is_empty()) { return String::new(); }
    let key = format!("\"{k}\"");
    let Some(at) = tb.args_text.find(&key) else { return String::new() };
    let rest = tb.args_text[at + key.len()..].trim_start();
    let Some(rest) = rest.strip_prefix(':').map(str::trim_start).and_then(|r| r.strip_prefix('"')) else { return String::new() };
    let mut out = String::new();
    let mut esc = false;
    for c in rest.chars() {
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

// one line of mixed text: its runs and the bits you can click (a path)
#[derive(Default)]
pub struct Line { text: String, runs: Vec<(Range<usize>, HighlightStyle)>, clicks: Vec<(Range<usize>, OnClick)> }
impl Line {
    fn push(&mut self, s: &str, c: Option<Hsla>) -> Range<usize> {
        let at = self.text.len();
        self.text.push_str(s);
        if let Some(c) = c { self.runs.push((at..self.text.len(), HighlightStyle { color: Some(c), ..Default::default() })); }
        at..self.text.len()
    }
    // tools.ts pathTarget: the path, clickable into the files pane, then a dim extra (":12-40")
    fn path(&mut self, p: &str, line: Option<usize>, extra: &str, pal: &Pal) {
        if p.is_empty() { self.push("…", Some(pal.dim)); return; }
        let r = self.push(&tilde(p), None);
        let (path, l) = (p.to_string(), line);
        self.clicks.push((r, Rc::new(move |_, cx| md::open_path(&path, l, cx))));
        if !extra.is_empty() { self.push(extra, Some(pal.dim)); }
    }
    fn el(self, id: String) -> AnyElement { sel(SharedString::from(id), self.text).runs(self.runs).clicks(self.clicks).into_any_element() }
}

#[derive(Clone, Copy)]
enum Body { None, Read, Write, Edit, Bash, Output, Browser, Image, Ask, Todos, Findings, Agents, Other }

struct Desc { verb: String, verb_c: Hsla, bold: bool, target: Line, meta: Vec<AnyElement>, auto: bool, body: Body }

fn mini(id: String, label: &str, size: Rems, p: &Pal) -> Stateful<Div> {
    let (txt, hov) = (p.text, p.hover);
    div().id(SharedString::from(id)).text_size(size).px(u(7.)).rounded(u(4.)).border(u(1.)).border_color(p.line).text_color(p.mid).cursor_pointer().whitespace_nowrap()
        .hover(move |s| s.text_color(txt).bg(hov)).child(label.to_string())
}

fn describe(tb: &ToolBlock, ui: &Ui, cx: &App) -> Desc {
    let p = pal(cx);
    let r = tb.result.as_ref().or(tb.partial.as_ref());
    let out = r.map(|r| r.text()).unwrap_or_default();
    let is_err = tb.result.as_ref().is_some_and(|r| r.is_error);
    let txt = |s: String| div().child(s).into_any_element();
    let colored = |s: String, c: Hsla| div().text_color(c).child(s).into_any_element();
    let mut t = Line::default();
    let d = |verb: &str, c: Hsla, t: Line, body| Desc { verb: verb.into(), verb_c: c, bold: false, target: t, meta: vec![], auto: false, body };
    match tb.name.as_str() {
        "read" => {
            let (off, lim) = (tb.args["offset"].as_u64(), tb.args["limit"].as_u64());
            let range = if off.is_some() || lim.is_some() { format!(":{}{}", off.unwrap_or(1), lim.map(|l| format!("-{}", off.unwrap_or(1) + l - 1)).unwrap_or_default()) } else { String::new() };
            t.path(&arg(tb, "path"), off.map(|o| o as usize), &range, &p);
            let meta = if !images_of(r).is_empty() { vec![txt("image".into())] } else if r.is_some() && !is_err { vec![txt(n_lines(&out, "line"))] } else { vec![] };
            Desc { meta, ..d("read", p.cyan, t, Body::Read) }
        }
        "write" => {
            let c = arg(tb, "content");
            t.path(&arg(tb, "path"), None, "", &p);
            let meta = if !c.is_empty() { vec![colored(format!("+{}", code::count_lines(&c)), p.add)] } else if tb.status == ToolStatus::Streaming { vec![txt("writing…".into())] } else { vec![] };
            Desc { meta, ..d("write", p.gold, t, Body::Write) }
        }
        "edit" => {
            let diff = tb.result.as_ref().and_then(|r| r.details["diff"].as_str());
            let n = edit_pairs(&tb.args).len();
            t.path(&arg(tb, "path"), None, "", &p);
            let meta = match diff {
                Some(df) => { let (a, x) = code::diff_counts(df); vec![div().flex().child(div().text_color(p.add).child(format!("+{a}"))).child(" ").child(div().text_color(p.del).child(format!("-{x}"))).into_any_element()] }
                None if n > 1 => vec![txt(format!("{n} edits"))],
                None => vec![],
            };
            Desc { meta, auto: diff.is_some_and(|d| d.split('\n').count() <= 40), ..d("edit", p.gold, t, Body::Edit) }
        }
        "bash" => {
            let exit = tb.result.as_ref().and_then(|r| r.details["exitCode"].as_i64());
            t.push(&one(&arg(tb, "command"), 140), Some(p.text));
            let meta = if is_err { vec![colored(exit.map(|e| format!("exit {e}")).unwrap_or("failed".into()), p.del)] } else if !out.is_empty() { vec![txt(n_lines(&out, "line"))] } else { vec![] };
            Desc { meta, bold: true, auto: tb.status == ToolStatus::Running && !out.is_empty(), ..d("$", p.green, t, Body::Bash) }
        }
        "grep" | "find" | "ls" | "glob" => {
            let path = arg(tb, "path");
            if tb.name == "ls" { t.path(if path.is_empty() { "." } else { &path }, None, "", &p); }
            else {
                t.push(&arg(tb, "pattern"), Some(p.orange));
                if !path.is_empty() { t.push(&format!(" in {}", tilde(&path)), Some(p.dim)); }
            }
            let meta = if r.is_some() { vec![txt(n_lines(out.trim(), "result"))] } else { vec![] };
            Desc { meta, ..d(&tb.name, p.blue, t, Body::Output) }
        }
        "browser" => {
            let action = arg(tb, "action");
            let what = match action.as_str() { "navigate" => arg(tb, "url"), "type" => serde_json::to_string(&arg(tb, "text")).unwrap_or_default(), _ => ["ref", "selector", "key", "script"].iter().map(|k| arg(tb, k)).find(|s| !s.is_empty()).unwrap_or_default() };
            let shots = !images_of(r).is_empty();
            t.push(if action.is_empty() { "…" } else { &action }, Some(p.mid));
            t.push(" ", None);
            t.push(&one(&what, 90), Some(p.dim));
            Desc { meta: if shots { vec![txt("screenshot".into())] } else { vec![] }, auto: shots, ..d("web", p.magenta, t, Body::Browser) }
        }
        "preview" => {
            let (path, url) = (arg(tb, "path"), arg(tb, "url"));
            if path.is_empty() { t.push(if url.is_empty() { "…" } else { &url }, None); } else { t.path(&path, None, "", &p); }
            // a preview row's one action: put it back in the side pane
            let (tab, args) = (ui.tab.clone(), tb.args.clone());
            let show = mini(format!("show-{}", tb.id), "show", em(cx, 0.9 * 0.92 * 0.8), &p).on_click(move |_, _, cx| {
                cx.stop_propagation();
                tab.update(cx, |t, cx| {
                    let mut a = args.clone();
                    if let Some(path) = a["path"].as_str().filter(|p| !p.starts_with('/')) { a["path"] = Value::from(format!("{}/{}", t.cwd.trim_end_matches('/'), path.trim_start_matches("./"))); }
                    crate::core::preview::show(&t.id, a["path"].as_str(), a["url"].as_str());
                    t.preview = Some(a);
                    cx.emit(TabEvent::Preview);
                });
                super::panes::open(super::panes::Pane::Preview, cx);
            });
            Desc { meta: vec![show.into_any_element()], ..d("preview", p.tertiary, t, Body::None) }
        }
        "generate_image" => {
            t.push(&one(&arg(tb, "prompt"), 110), Some(p.dim));
            let file = tb.result.as_ref().and_then(|r| r.details["files"][0].as_str()).map(String::from);
            let meta = file.map(|f| {
                let name = f.rsplit('/').next().unwrap_or(&f).to_string();
                let mut l = Line::default();
                let r = l.push(&name, None);
                l.clicks.push((r, Rc::new(move |_, cx| md::open_path(&f, None, cx))));
                vec![l.el(format!("gif-{}", tb.id))]
            }).unwrap_or_default();
            Desc { meta, auto: true, ..d("image", p.magenta, t, Body::Image) }
        }
        "ask_user" => { t.push(&one(&arg(tb, "question"), 120), None); Desc { auto: tb.result.is_some(), ..d("ask", p.gold, t, Body::Ask) } }
        "todo_write" => {
            let todos = tb.args["todos"].as_array().cloned().unwrap_or_default();
            let done = todos.iter().filter(|t| t["status"] == "completed").count();
            t.push(&format!("{done}/{}", todos.len()), None);
            if let Some(c) = todos.iter().find(|t| t["status"] == "in_progress").and_then(|t| t["content"].as_str()) { t.push(&format!(" · {}", one(c, 80)), Some(p.dim)); }
            d("tasks", p.mid, t, Body::Todos)
        }
        "report_findings" => {
            t.push(&format!("{} findings", tb.args["findings"].as_array().map(|a| a.len()).unwrap_or(0)), None);
            Desc { auto: true, ..d("review", p.primary, t, Body::Findings) }
        }
        "subagent" | "summon_agent" => {
            let planned = planned(&tb.args);
            let det = r.map(|r| &r.details);
            let mode = det.and_then(|d| d["mode"].as_str()).map(String::from)
                .unwrap_or_else(|| if tb.args["chain"].is_array() { "chain".into() } else if tb.args["tasks"].is_array() { "parallel".into() } else { "single".into() });
            let n = planned.len().max(det.and_then(|d| d["results"].as_array()).map(|a| a.len()).unwrap_or(0));
            t.push(&format!("{} {mode}", if n == 0 { "?".into() } else { n.to_string() }), None);
            t.push(&format!(" · {}", planned.iter().map(|(g, _)| g.as_str()).collect::<Vec<_>>().join(", ")), Some(p.dim));
            Desc { auto: true, ..d("agents", p.tertiary, t, Body::Agents) }
        }
        n => {
            let name = n.strip_prefix("mcp__").unwrap_or(n).replace("__", ":");
            t.push(&one(&tb.args.to_string(), 110), Some(p.dim));
            let meta = if r.is_some() && !out.is_empty() { vec![txt(n_lines(&out, "line"))] } else { vec![] };
            Desc { meta, ..d(&name, p.mid, t, Body::Other) }
        }
    }
}

fn planned(a: &Value) -> Vec<(String, String)> {
    let list = a["tasks"].as_array().or(a["chain"].as_array());
    match list {
        Some(l) => l.iter().map(|t| (t["agent"].as_str().unwrap_or("").into(), t["task"].as_str().unwrap_or("").into())).collect(),
        None if a["agent"].is_string() => vec![(a["agent"].as_str().unwrap_or("").into(), a["task"].as_str().unwrap_or("").into())],
        None => vec![],
    }
}

// .tool-code: a line-number gutter and the coloured code, scrolling past 420px
fn code_view(id: String, text: &str, lang: Option<&str>, start: usize, cx: &App) -> AnyElement {
    let p = pal(cx);
    let (joined, runs) = code::joined(text, lang, cx);
    let n = joined.split('\n').count();
    let gutter = (start..start + n).map(|i| i.to_string()).collect::<Vec<_>>().join("\n");
    div().id(SharedString::from(format!("{id}-sc"))).max_h(u(420.)).overflow_scroll().text_size(em(cx, 0.9 * 0.92)).line_height(relative(1.5)).whitespace_nowrap()
        .child(div().flex()
            .child(div().flex_none().text_right().pr(u(14.)).text_color(p.dim.opacity(0.7)).child(gutter))
            .child(div().flex_none().text_color(p.text).child(sel(SharedString::from(id), joined).runs(runs))))
        .into_any_element()
}

// .tool-out: plain output, wrapped, scrolling past 380px. a live one shows its tail
fn output(id: String, text: &str, tail: bool, cx: &App) -> AnyElement {
    let p = pal(cx);
    let n = text.chars().count();
    let t = if tail && n > 6000 { format!("…{}", text.chars().skip(n - 6000).collect::<String>()) } else if n > 60000 { format!("{}\n…", text.chars().take(60000).collect::<String>()) } else { text.to_string() };
    let t = if t.is_empty() { "(no output)".to_string() } else { t.replace('\t', "    ") };
    div().id(SharedString::from(format!("{id}-sc"))).max_h(u(380.)).overflow_y_scroll().text_size(em(cx, 0.9 * 0.92)).text_color(p.mid)
        .child(sel(SharedString::from(id), t.trim_end_matches('\n').to_string()))
        .into_any_element()
}

// .tool-cmd: a long or multi-line command, coloured as shell
fn cmd_view(id: String, text: &str, lang: Option<&str>, cx: &App) -> AnyElement {
    let (joined, runs) = code::joined(text, lang, cx);
    div().mb(u(6.)).text_size(em(cx, 0.9 * 0.92)).text_color(pal(cx).text).child(sel(SharedString::from(id), joined).runs(runs)).into_any_element()
}

#[derive(Clone, Copy, PartialEq)]
enum DK { Add, Del, Same, Gap }

// pi's edit diff: "+ 12 code", "- 12 code", "  12 code", and gap lines between hunks (tools.ts diffView)
fn parse_diff(diff: &str) -> Vec<(DK, String, String)> {
    diff.split('\n').map(|raw| {
        let mut cs = raw.chars();
        let sign = cs.next();
        let rest = cs.as_str();
        let kind = match sign { Some('+') => DK::Add, Some('-') => DK::Del, Some(' ') => DK::Same, _ => DK::Gap };
        let t = rest.trim_start();
        let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
        if kind == DK::Gap || digits == 0 || !t[digits..].starts_with(' ') && !t[digits..].is_empty() { return (DK::Gap, String::new(), if raw.trim().is_empty() { "…".into() } else { raw.trim().into() }); }
        (kind, t[..digits].to_string(), t[digits..].strip_prefix(' ').unwrap_or("").replace('\t', "  "))
    }).collect()
}

fn diff_view(id: String, rows: Vec<(DK, String, String)>, cx: &App) -> AnyElement {
    let p = pal(cx);
    let ch = md::ch(cx) * geo(cx).fs * 0.9 * 0.9;
    let mut col = div().flex().flex_col().min_w_full();
    for (i, (k, num, code)) in rows.into_iter().enumerate() {
        if k == DK::Gap { col = col.child(div().pl(u(ch)).text_color(p.dim).child(code)); continue; }
        let (bg, sc, sign) = match k { DK::Add => (Some(p.add_bg), p.add, "+"), DK::Del => (Some(p.del_bg), p.del, "-"), _ => (None, p.text, " ") };
        col = col.child(div().flex().w_full().when_some(bg, |d, b| d.bg(b))
            .child(div().flex_none().w(u(ch * 4.5)).pr(u(8.)).text_right().text_color(p.dim.opacity(0.7)).child(num))
            .child(div().flex_none().w(u(ch * 2.)).text_color(sc).child(sign))
            .child(div().flex_none().child(sel(SharedString::from(format!("{id}-{i}")), code))));
    }
    div().id(SharedString::from(format!("{id}-sc"))).overflow_x_scroll().text_size(em(cx, 0.9 * 0.9)).line_height(relative(1.5)).whitespace_nowrap().child(col).into_any_element()
}


fn edits_rows(edits: &[(String, String)]) -> Vec<(DK, String, String)> {
    let mut rows = vec![];
    for (i, (o, n)) in edits.iter().enumerate() {
        if i > 0 { rows.push((DK::Gap, String::new(), "…".into())); }
        rows.extend(o.split('\n').map(|l| (DK::Del, String::new(), l.to_string())));
        rows.extend(n.split('\n').map(|l| (DK::Add, String::new(), l.to_string())));
    }
    rows
}

fn body(tb: &ToolBlock, kind: Body, ui: &Ui, cx: &App) -> Option<AnyElement> {
    let p = pal(cx);
    let id = format!("tb-{}", tb.id);
    let r = tb.result.as_ref().or(tb.partial.as_ref());
    let out = r.map(|r| r.text()).unwrap_or_default();
    let err = tb.result.as_ref().is_some_and(|r| r.is_error);
    let col = div().flex().flex_col().w_full();
    let imgs = |list: Vec<(String, String)>, h: f32, w: f32, rad: f32, border: bool| media::strip(&format!("{id}-im"), list.iter().map(|(d, m)| (d.as_str(), m.as_str())), h, w, rad, border, 8., cx).into_any_element();
    Some(match kind {
        Body::None => return None,
        Body::Read => {
            let im = images_of(r);
            if !im.is_empty() { imgs(im, 220., 1200., 5., true) } else { code_view(id, &out, code::lang_of(&arg(tb, "path")), tb.args["offset"].as_u64().unwrap_or(1) as usize, cx) }
        }
        Body::Write => if err { output(id, &out, false, cx) } else { code_view(id, &arg(tb, "content"), code::lang_of(&arg(tb, "path")), 1, cx) },
        Body::Edit => {
            let pairs = edit_pairs(&tb.args);
            let diff = tb.result.as_ref().and_then(|r| r.details["diff"].as_str());
            if err { col.child(output(format!("{id}-err"), &out, false, cx)).when(!pairs.is_empty(), |c| c.child(diff_view(id.clone(), edits_rows(&pairs), cx))).into_any_element() }
            else if let Some(df) = diff { diff_view(id, parse_diff(df), cx) }
            else { diff_view(id, edits_rows(&pairs), cx) }
        }
        Body::Bash => {
            let cmd = arg(tb, "command");
            col.when(cmd.contains('\n') || cmd.chars().count() > 140, |c| c.child(cmd_view(format!("{id}-cmd"), &cmd, Some("bash"), cx)))
                .child(output(id.clone(), &out, tb.status == ToolStatus::Running, cx)).into_any_element()
        }
        Body::Output => output(id, &out, false, cx),
        Body::Browser => {
            let im = images_of(r);
            col.when(!im.is_empty(), |c| c.child(imgs(im, 1200., 520., 5., true)))
                .when(!out.is_empty() && arg(tb, "action") != "screenshot", |c| c.child(output(id.clone(), &out, false, cx))).into_any_element()
        }
        Body::Image => {
            let im = images_of(tb.result.as_ref());
            if !im.is_empty() { imgs(im, 1200., 440., geo(cx).r2, false) }
            else if matches!(tb.status, ToolStatus::Running | ToolStatus::Queued) {
                // .gen-wait: a striped placeholder while it paints
                div().w(u(260.)).h(u(180.)).flex().items_center().justify_center().gap(u(4.)).rounded(u(geo(cx).r2)).bg(mix(p.raised, 0.5, p.surface_low)).text_color(p.dim)
                    .child(super::frame::spin(SharedString::from(format!("{id}-sp")))).child(" painting").into_any_element()
            }
            else if err { output(id, &out, false, cx) } else { return None }
        }
        Body::Ask => {
            let res = tb.result.as_ref()?;
            let answer = res.details["answer"].as_str().map(String::from).unwrap_or_else(|| "dismissed".into());
            div().py(u(2.)).flex().child(div().text_color(p.dim).child("answer ")).child(div().min_w_0().child(sel(SharedString::from(id), answer))).into_any_element()
        }
        Body::Todos => {
            let todos: Vec<(String, String)> = tb.args["todos"].as_array().into_iter().flatten().map(|t| (t["content"].as_str().unwrap_or("").to_string(), t["status"].as_str().unwrap_or("").to_string())).collect();
            todo_list_for(&id, todos.iter().map(|(a, b)| (a.as_str(), b.as_str())), Some(ui.tab), cx).into_any_element()
        }
        Body::Findings => findings(tb, ui, cx),
        Body::Agents => agents(tb, ui.tab, cx),
        Body::Other => {
            let im = images_of(r);
            col.child(cmd_view(format!("{id}-args"), &serde_json::to_string_pretty(&tb.args).unwrap_or_default(), None, cx))
                .when(!im.is_empty(), |c| c.child(imgs(im, 220., 1200., 5., true)))
                .when(!out.is_empty(), |c| c.child(output(id.clone(), &out, false, cx))).into_any_element()
        }
    })
}

// ---------- subagent runs you can open ----------

fn open_run(run: crate::state::hub::AgentRun) -> OnClick { Rc::new(move |_, cx| { let r = run.clone(); Pith::global(cx).update(cx, |a, cx| { a.open_agent(r, cx); }); }) }

// every subagent run this session started that has a transcript to open: (task, run)
fn runs_in(tab: &Tab, cx: &App) -> Vec<(String, crate::state::hub::AgentRun)> {
    tab.subruns.iter().filter_map(|r| Some((r["task"].as_str()?.to_string(), crate::state::hub::AgentRun::of(r, cx)?))).collect()
}

// a todo and a delegated task are the same work when most of the todo's words are in the task. no match, no link
fn words(s: &str) -> HashSet<String> {
    let s = s.to_lowercase();
    s.split(|c: char| !(c.is_ascii_alphanumeric() || "_./-".contains(c))).filter(|w| w.len() >= 3).map(String::from).collect()
}
fn agent_for_todo(todo: &str, runs: &[(String, crate::state::hub::AgentRun)]) -> Option<OnClick> {
    let tw = words(todo);
    if tw.is_empty() { return None; }
    let mut best = None;
    let mut score = 0.;
    for r in runs {
        let rw = words(&r.0);
        let s = tw.iter().filter(|w| rw.contains(*w)).count() as f32 / tw.len() as f32;
        if s >= score { best = Some(r); score = s; }
    }
    best.filter(|_| score >= 0.6).map(|r| open_run(r.1.clone()))
}

// ul.todos: a box per todo (a tick, a spinner, or empty) and its text; one that matches a subagent run opens it
pub fn todo_list<'a>(todos: impl IntoIterator<Item = (&'a str, &'a str)>, cx: &App) -> Div { todo_list_for("todos", todos, None, cx) }
pub fn todo_list_for<'a>(key: &str, todos: impl IntoIterator<Item = (&'a str, &'a str)>, tab: Option<&Entity<Tab>>, cx: &App) -> Div {
    let p = pal(cx);
    let runs = tab.map(|t| runs_in(t.read(cx), cx)).unwrap_or_default();
    div().flex().flex_col().children(todos.into_iter().enumerate().map(|(i, (text, status))| {
        let tc = match status { "completed" => p.dim, "in_progress" => p.text, _ => p.mid };
        let bx = div().flex_none().size(u(14.)).flex().items_center().justify_center().rounded(u(3.)).text_size(em(cx, 0.8)).text_color(p.accent)
            .when(status == "completed", |d| d.bg(p.sel).child(Icon::new(IconName::Check).size(u(11.))))
            .when(status != "completed", |d| d.border(u(1.)).border_color(p.line))
            .when(status == "in_progress", |d| d.child(super::frame::spin(SharedString::from(format!("{key}-sp{i}")))));
        let link = (!runs.is_empty()).then(|| agent_for_todo(text, &runs)).flatten();
        let mut st = HighlightStyle::default();
        if status == "completed" { st.strikethrough = Some(StrikethroughStyle { thickness: px(1.), color: Some(p.dim.opacity(0.6)) }); }
        if link.is_some() { st.underline = Some(UnderlineStyle { thickness: px(1.), color: Some(tc.opacity(0.4)), wavy: false }); }
        let len = text.len();
        let t = sel(SharedString::from(format!("{key}-t{i}")), text.to_string()).runs(vec![(0..len, st)]).clicks(link.map(|l| vec![(0..len, l)]).unwrap_or_default());
        div().flex().items_center().gap(u(8.)).py(u(1.)).child(bx).child(div().min_w_0().text_color(tc).child(t))
    }))
}

// .findings: a card per finding with its severity on the left edge; the location opens the file, "fix this" quotes it
fn findings(tb: &ToolBlock, ui: &Ui, cx: &App) -> AnyElement {
    let p = pal(cx);
    let g = geo(cx);
    let a = &tb.args;
    let list = a["findings"].as_array().cloned().unwrap_or_default();
    let mut col = div().flex().flex_col().gap(u(8.)).py(u(4.));
    if let Some(s) = a["summary"].as_str() { col = col.child(div().text_color(p.mid).child(md::inline_text(format!("{}-sum", tb.id), s, cx))); }
    if list.is_empty() { col = col.child(div().text_color(p.green).child("nothing held up")); }
    for (i, f) in list.iter().enumerate() {
        let sev = f["severity"].as_str().unwrap_or("").to_string();
        let sc = match sev.as_str() { "high" | "critical" => Some(p.red), "medium" => Some(p.gold), "low" => Some(p.dim), _ => None };
        let file = f["file"].as_str().unwrap_or("").to_string();
        let line = f["line"].as_u64().map(|l| l as usize);
        let title = f["title"].as_str().unwrap_or("").to_string();
        let ask = format!("Fix finding {}: {title} ({file}{})", i + 1, line.map(|l| format!(":{l}")).unwrap_or_default());
        let tab = ui.tab.clone();
        let mut loc = Line::default();
        loc.path(&file, line, &line.map(|l| format!(":{l}")).unwrap_or_default(), &p);
        let fix = mini(format!("{}-fix{i}", tb.id), "fix this", em(cx, 0.9 * 0.8), &p).on_click(move |_, _, cx| {
            let label = if ask.chars().count() > 140 { format!("{}…", ask.chars().take(140).collect::<String>()) } else { ask.clone() };
            tab.update(cx, |_, cx| cx.emit(TabEvent::Quote { label, text: ask.clone() }));
        });
        col = col.child(div().relative().border(u(1.)).border_color(p.line).rounded(u(g.r2)).pl(u(12.)).pr(u(12.)).py(u(8.)).bg(p.surface_low).overflow_hidden()
            // the inset 3px severity edge
            .when_some(sc, |d, c| d.child(div().absolute().left_0().top_0().bottom_0().w(u(3.)).bg(c)))
            // panes.css's .f-head (the files pane's bar) lands on these too: a padded bar with a rule under it, .88em
            .child(div().flex().items_center().gap(u(4.)).pl(u(12.)).pr(u(8.)).py(u(4.)).border_b(u(1.)).border_color(p.rule).text_size(em(cx, 0.9 * 0.88))
                .child(div().text_color(p.dim).child(format!("{:02}", i + 1)))
                .child(div().text_size(em(cx, 0.9 * 0.88 * 0.8)).px(u(6.)).rounded(u(3.)).bg(p.high).when_some(sc.filter(|_| sev != "low"), |d, c| d.text_color(c)).child(sev.clone()))
                .when_some(f["category"].as_str(), |d, c| d.child(div().text_color(p.dim).text_size(em(cx, 0.9 * 0.88 * 0.85)).child(c.to_string())))
                .child(div().min_w_0().text_color(p.text).font_weight(FontWeight::BOLD).child(md::inline_text(format!("{}-ti{i}", tb.id), &title, cx))))
            .child(div().mt(u(2.)).mb(u(4.)).text_color(p.cyan).text_size(em(cx, 0.9 * 0.9)).child(loc.el(format!("{}-loc{i}", tb.id))))
            .when_some(f["detail"].as_str(), |d, t| d.child(div().text_color(p.mid).child(md::plain(format!("{}-f{i}", tb.id), t, cx))))
            .when_some(f["fix"].as_str(), |d, t| d.child(div().mt(u(4.)).flex().gap(u(6.)).text_color(p.mid).child(div().flex_none().text_color(p.dim).child("fix ")).child(md::plain(format!("{}-x{i}", tb.id), t, cx))))
            .child(div().mt(u(6.)).flex().child(fix)));
    }
    col.into_any_element()
}

// .agent-cards: one per delegated task, live from what pi reports on the call (Tab::subruns); a head opens the
// run's transcript
fn agents(tb: &ToolBlock, tab: &Entity<Tab>, cx: &App) -> AnyElement {
    let p = pal(cx);
    let g = geo(cx);
    let prefix = format!("{}/{}/", tab.read(cx).id, tb.id);
    let runs: Vec<Value> = tab.read(cx).subruns.iter().filter(|r| r["key"].as_str().is_some_and(|k| k.starts_with(&prefix))).cloned().collect();
    let planned = planned(&tb.args);
    let n = runs.len().max(planned.len());
    let mut col = div().flex().flex_col().gap(u(6.)).py(u(2.));
    for i in 0..n {
        let res = runs.get(i).cloned().unwrap_or(Value::Null);
        let agent = res["agent"].as_str().map(String::from).or_else(|| planned.get(i).map(|x| x.0.clone())).unwrap_or_default();
        let task = res["task"].as_str().map(String::from).or_else(|| planned.get(i).map(|x| x.1.clone())).unwrap_or_default();
        let output = res["output"].as_str().unwrap_or("").to_string();
        let failed = matches!(res["status"].as_str(), Some("failed" | "cancelled"));
        let done = failed || res["status"] == "completed" || !matches!(tb.status, ToolStatus::Running | ToolStatus::Queued | ToolStatus::Streaming);
        let meta = [res["model"].as_str().map(String::from), res["pid"].as_u64().filter(|_| !done).map(|p| format!("pid {p}"))].into_iter().flatten().collect::<Vec<_>>().join(" · ");
        let open = (!res.is_null()).then(|| crate::state::hub::AgentRun::of(&res, cx)).flatten().map(open_run);
        let key = format!("{}-ag{i}", tb.id);
        let dot: AnyElement = if done { div().text_size(em(cx, 0.9 * 0.7)).text_color(if failed { p.red } else { p.green }).child("●").into_any_element() }
            else { div().text_color(p.accent).child(super::frame::spin(SharedString::from(format!("{key}-sp")))).into_any_element() };
        let text = p.tertiary;
        let doing = (!done).then(|| output.lines().rev().find(|l| !l.trim().is_empty()).map(String::from)).flatten();
        let head = div().id(SharedString::from(format!("{key}-h"))).flex().items_baseline().gap(u(8.)).min_w_0()
            .child(dot)
            .child(div().flex_none().text_color(p.tertiary).when(open.is_some(), |d| d.hover(move |s| s.text_color(text).underline())).child(agent))
            .child(div().flex_1().min_w_0().truncate().text_color(p.mid).child(one(&task, 100)))
            .when(!meta.is_empty(), |d| d.child(div().flex_none().text_color(p.dim).text_size(em(cx, 0.9 * 0.85)).whitespace_nowrap().child(meta)))
            .when_some(open, |d, o| d.cursor_pointer().on_click(move |_, w, cx| { cx.stop_propagation(); o(w, cx) }));
        let out_key = format!("{key}-out");
        col = col.child(div().border(u(1.)).border_color(if failed { mix(p.red, 0.5, p.line) } else { p.line }).rounded(u(g.r2)).px(u(10.)).py(u(6.)).bg(p.surface_low)
            .child(head)
            .when_some(doing, |d, s| d.child(div().mt(u(2.)).text_size(em(cx, 0.9 * 0.9)).text_color(p.mid).truncate().child(format!("› {}", one(&s, 120)))))
            .when(failed, |d| d.when_some(res["error"].as_str(), |d, e| d.child(div().mt(u(2.)).text_size(em(cx, 0.9 * 0.9)).text_color(p.red).child(one(e, 300)))))
            .when(done && !output.is_empty(), |d| d.child(details(out_key, "output", &output, cx))));
    }
    col.into_any_element()
}

// <details> with a summary line: .ac-out. its open state lives with the chat's other rows
thread_local! { static DETAILS: std::cell::RefCell<HashSet<String>> = std::cell::RefCell::new(HashSet::new()); }
fn details(key: String, summary: &str, text: &str, cx: &App) -> AnyElement {
    let p = pal(cx);
    let open = DETAILS.with(|d| d.borrow().contains(&key));
    let k = key.clone();
    let mid = p.mid;
    div().flex().flex_col()
        .child(div().id(SharedString::from(format!("{key}-sum"))).mt(u(4.)).flex().gap(u(4.)).cursor_pointer().text_color(p.dim).text_size(em(cx, 0.9 * 0.88)).hover(move |s| s.text_color(mid))
            .child(if open { "▼" } else { "▶" }).child(summary.to_string())
            .on_click(move |_, _, cx| { cx.stop_propagation(); DETAILS.with(|d| { let mut d = d.borrow_mut(); if !d.remove(&k) { d.insert(k.clone()); } }); cx.refresh_windows(); }))
        .when(open, |d| d.child(div().id(SharedString::from(format!("{key}-body"))).mt(u(6.)).mb(u(2.)).max_h(u(360.)).overflow_y_scroll().child(md::prose(format!("{key}-md"), text, false, false, None, cx))))
        .into_any_element()
}

// one call: .tool > .trow (+ .tbody when open). also the bottom margin its body pushes through (10 when open)
pub fn row(tb: &ToolBlock, ui: &Ui, cx: &App) -> (AnyElement, f32) {
    let p = pal(cx);
    let desc = describe(tb, ui, cx);
    let open = ui.is_open(&tb.id, desc.auto);
    let failed = tb.status == ToolStatus::Error || tb.result.as_ref().is_some_and(|r| r.is_error);
    let busy = matches!(tb.status, ToolStatus::Running | ToolStatus::Streaming | ToolStatus::Queued);
    let can = !matches!(desc.body, Body::None);
    let kind = desc.body;
    let fs = 0.9;
    let glyph: AnyElement = if busy { div().text_color(p.accent).child(super::frame::spin(SharedString::from(format!("tg-{}", tb.id)))).into_any_element() }
        else if failed { div().text_color(p.red).font_weight(FontWeight::BOLD).child("×").into_any_element() }
        else { div().relative().top(em(cx, -fs * 0.6 * 0.35)).text_size(em(cx, fs * 0.6)).text_color(p.dim.opacity(0.7)).child("●").into_any_element() };
    // a finished call's time when it took a while, a running one's ticking
    let time = match (tb.ms, tb.started) {
        (Some(ms), _) if ms > 400 => Some(dur(ms)),
        (None, Some(st)) if tb.status == ToolStatus::Running => Some(dur(st.elapsed().as_millis() as u64)),
        _ => None,
    };
    let chv = md::ch(cx) * geo(cx).fs * fs;
    let hover = p.hover;
    let head = div().id(SharedString::from(format!("tool-{}", tb.id))).flex().items_baseline().gap(u(10.)).pl(u(2.)).pr(u(8.)).py(u(1.)).rounded(u(4.))
        .child(div().flex_none().w(u(14.)).flex().justify_center().child(glyph))
        .child(div().flex_none().min_w(u(chv * 6.)).text_color(desc.verb_c).when(desc.bold, |d| d.font_weight(FontWeight::BOLD)).child(desc.verb.clone()))
        .child(div().flex_1().min_w_0().truncate().text_color(if failed { p.mid } else { p.text }).child(desc.target.el(format!("tt-{}", tb.id))))
        .child(div().flex_none().max_w(u(chv * 40.)).overflow_hidden().flex().items_baseline().gap(u(10.)).text_color(p.dim).text_size(em(cx, fs * 0.92)).whitespace_nowrap()
            .children(desc.meta).when_some(time, |d, t| d.child(div().child(t))))
        .when(can, |s| s.cursor_pointer().hover(move |s| s.bg(hover)).on_click(ui.on(tb.id.clone(), desc.auto)));
    let b = if open && can { body(tb, kind, ui, cx) } else { None };
    let bottom = if b.is_some() { 10. } else { 0. };
    (div().w_full().text_size(em(cx, fs))
        .child(head)
        .children(b.map(|b| div().mt(u(3.)).ml(u(26.)).pl(u(12.)).border_l(u(1.)).border_color(p.rule).child(b)))
        .into_any_element(), bottom)
}

// a 1px dashed rule (.rule, .prose hr): dashes painted by hand, gpui's dashed border skips a box this thin
pub fn dashed(color: Hsla) -> impl IntoElement {
    canvas(|_, _, _| (), move |b, _, window, _| {
        let (dash, gap) = (px(3.), px(3.));
        let mut x = b.left();
        while x < b.right() {
            let w = dash.min(b.right() - x);
            window.paint_quad(fill(Bounds::new(point(x, b.top()), size(w, px(1.))), color));
            x += dash + gap;
        }
    }).w_full().h(px(1.))
}

// .msg.bash: a command you ran yourself with ! in the composer
pub fn shell(id: u64, command: &str, output: &str, exit: Option<i64>, running: bool, cancelled: bool, excluded: bool, cx: &App) -> impl IntoElement {
    let p = pal(cx);
    let g = geo(cx);
    let bad = exit.is_some_and(|e| e != 0);
    let out = if output.is_empty() && !running { "(no output)".to_string() } else { output.trim_end_matches('\n').replace('\t', "    ") };
    div().w_full().px(u(12.)).py(u(8.)).rounded(u(g.r2)).bg(p.surface_low).border(u(1.)).border_color(if bad { mix(p.red, 0.4, p.rule) } else { p.rule })
        .child(div().flex().items_baseline().gap(u(8.)).mb(u(4.))
            .child(div().flex_none().text_color(p.green).font_weight(FontWeight::BOLD).child("$"))
            .child(div().min_w_0().child(sel(SharedString::from(format!("sh-{id}-c")), command.to_string())))
            .when(running, |d| d.child(div().text_color(p.dim).child(super::frame::spin(SharedString::from(format!("sh-{id}-sp"))))))
            .when(!running && exit.is_some(), |d| d.child(div().ml_auto().flex_none().text_color(if bad { p.red } else { p.dim }).text_size(em(cx, 0.85)).child(if cancelled { "cancelled".to_string() } else { format!("exit {}", exit.unwrap_or(0)) })))
            .when(excluded, |d| d.child(div().flex_none().text_color(p.dim).child(" · not sent to the model"))))
        .child(div().id(SharedString::from(format!("sh-{id}-sc"))).max_h(u(380.)).overflow_y_scroll().text_size(em(cx, 0.92)).text_color(p.mid)
            .child(sel(SharedString::from(format!("sh-{id}-o")), out)))
}
