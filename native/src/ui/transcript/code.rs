// Code in the transcript outside markdown: tool bodies with tree-sitter colours and a line-number gutter, diffs with
// tinted rows, plain output. highlighting runs once per text and is kept, the list redraws visible rows every frame
use crate::ui::theme::{is_dark, pal};
use gpui_kit::base::input::Rope;
use gpui_kit::component::clipboard::Clipboard;
use gpui_kit::component::highlighter::{LanguageRegistry, SyntaxHighlighter};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::rc::Rc;

// past this a body stops growing, the rest is a count
pub const MAX_SHOWN: usize = 1500;
// highlighting a huge file is wasted work, nobody scrolls a tool body that far
const MAX_HIGHLIGHT: usize = 200_000;

pub type Runs = Vec<(Range<usize>, HighlightStyle)>;
pub struct Line { pub text: SharedString, pub runs: Runs }

// tree-sitter's names for the extensions models touch most. anything else is plain text
pub fn lang_of(path: &str) -> Option<&'static str> {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    if name == "makefile" || name == "dockerfile" { return None; }
    let ext = name.rsplit_once('.').map(|(_, e)| e)?;
    Some(match ext {
        "rs" => "rust",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "js" | "mjs" | "cjs" | "jsx" | "qml" => "javascript",
        "py" | "pyi" => "python",
        "sh" | "bash" | "zsh" => "bash",
        "json" | "jsonc" | "jsonl" => "json",
        "toml" => "toml",
        "yml" | "yaml" => "yaml",
        "go" => "go",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" | "hh" => "cpp",
        "html" | "htm" | "vue" | "svelte" => "html",
        "css" | "scss" | "rasi" => "css",
        "md" | "markdown" => "markdown",
        "diff" | "patch" => "diff",
        _ => return None,
    })
}

fn known(lang: &str) -> bool { LanguageRegistry::singleton().language(lang).is_some() }

thread_local! {
    static CACHE: RefCell<HashMap<u64, Rc<Vec<Line>>>> = RefCell::new(HashMap::new());
}

// the text split into lines, each with its colour runs. cached by content, language and light/dark
pub fn lines(text: &str, lang: Option<&str>, cx: &App) -> Rc<Vec<Line>> {
    let lang = lang.filter(|l| known(l));
    let theme = cx.theme().highlight_theme.clone();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (text, lang, is_dark(cx), std::sync::Arc::as_ptr(&theme) as usize).hash(&mut h);
    let key = h.finish();
    if let Some(l) = CACHE.with(|c| c.borrow().get(&key).cloned()) { return l; }

    let text = text.replace('\t', "    ");
    let runs: Runs = match lang {
        Some(l) if text.len() <= MAX_HIGHLIGHT => {
            let mut hl = SyntaxHighlighter::new(l);
            hl.update(None, &Rope::from_str(&text), None);
            hl.styles(&(0..text.len()), theme.as_ref())
        }
        _ => vec![],
    };
    let mut out = Vec::new();
    let mut at = 0;
    let mut ri = 0;
    for (n, line) in text.split('\n').enumerate() {
        if n >= MAX_SHOWN { break; }
        let r = at..at + line.len();
        let mut mine = vec![];
        while ri < runs.len() && runs[ri].0.end <= r.start { ri += 1; }
        let mut j = ri;
        while j < runs.len() && runs[j].0.start < r.end {
            let (rr, st) = &runs[j];
            let (s, e) = (rr.start.max(r.start) - r.start, rr.end.min(r.end) - r.start);
            if s < e && line.is_char_boundary(s) && line.is_char_boundary(e) { mine.push((s..e, *st)); }
            j += 1;
        }
        out.push(Line { text: line.to_string().into(), runs: mine });
        at = r.end + 1;
    }
    // a trailing newline isn't a line of its own
    if out.len() > 1 && out.last().is_some_and(|l| l.text.is_empty()) { out.pop(); }
    let out = Rc::new(out);
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() > 400 { c.clear(); }
        c.insert(key, out.clone());
    });
    out
}

pub fn count_lines(s: &str) -> usize { if s.is_empty() { 0 } else { s.trim_end_matches('\n').split('\n').count() } }

fn styled(l: &Line) -> StyledText {
    let t: SharedString = if l.text.is_empty() { " ".into() } else { l.text.clone() };
    StyledText::new(t).with_highlights(l.runs.iter().cloned())
}

// the shared frame of every code surface: code background, hairline, mono, a copy button in the corner
pub fn frame(id: impl Into<SharedString>, copy: Option<String>, cx: &App) -> Div {
    let p = pal(cx);
    let id: SharedString = id.into();
    div().relative().w_full().min_w_0().rounded(cx.theme().radius).bg(p.code_bg).border_1().border_color(p.hairline).overflow_hidden()
        .font_family(cx.theme().mono_font_family.clone()).text_size(cx.theme().mono_font_size * 0.9).line_height(relative(1.5)).text_color(cx.theme().foreground)
        .when_some(copy.filter(|c| !c.is_empty()), |d, c| d.child(
            div().absolute().top_1().right_1().rounded(cx.theme().radius).bg(p.code_bg)
                .child(Clipboard::new(SharedString::from(format!("{id}-copy"))).value(c).tooltip("Copy")),
        ))
}

// wide enough for the longest number, so the code column lines up
fn gutter_w(digits: usize, cx: &App) -> Pixels { cx.theme().mono_font_size * 0.9 * (0.62 * digits.max(2) as f32) + px(4.) }

// coloured lines, numbered from `start` when there is one. `limit` cuts it short, the caller says what was left out
pub fn code(id: impl Into<SharedString>, text: &str, lang: Option<&str>, start: Option<usize>, limit: usize, cx: &App) -> Div {
    let p = pal(cx);
    let ls = lines(text, lang, cx);
    let shown = ls.len().min(limit);
    let gw = start.map(|s| gutter_w((s + shown).to_string().len(), cx));
    let mut col = v_flex().w_full().py_1p5().when(gw.is_none(), |d| d.px_3());
    for (i, l) in ls.iter().take(shown).enumerate() {
        col = col.child(h_flex().items_start().w_full().pr_8()
            .when_some(gw.zip(start), |d, (w, s)| d.child(div().flex_none().w(w).mr_3().ml_2().text_right().text_color(p.dim.opacity(0.7)).child((s + i).to_string())))
            .child(div().flex_1().min_w_0().child(styled(l))));
    }
    frame(id, Some(text.to_string()), cx).child(col)
}

// plain output: no gutter, no colours, the tail when it's live
pub fn output(id: impl Into<SharedString>, text: &str, cx: &App) -> Div {
    let p = pal(cx);
    let body = if text.trim().is_empty() { div().text_color(p.dim).child("No output") } else { div().text_color(p.mid).child(text.trim_end_matches('\n').replace('\t', "    ")) };
    frame(id, Some(text.to_string()), cx).child(div().px_3().py_2().pr_8().child(body))
}

#[derive(Clone, Copy, PartialEq)]
enum Kind { Add, Del, Same, Gap }

// pi's edit diff: "+ 12 code", "- 12 code", "  12 code", and gap lines between hunks
fn parse_diff(diff: &str) -> Vec<(Kind, String, String)> {
    diff.lines().map(|raw| {
        let (sign, rest) = raw.split_at(raw.char_indices().nth(1).map(|(i, _)| i).unwrap_or(raw.len()));
        let kind = match sign { "+" => Kind::Add, "-" => Kind::Del, " " => Kind::Same, _ => Kind::Gap };
        let t = rest.trim_start();
        let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
        if kind == Kind::Gap || digits == 0 { return (Kind::Gap, String::new(), String::new()); }
        let code = t[digits..].strip_prefix(' ').unwrap_or(&t[digits..]);
        (kind, t[..digits].to_string(), code.to_string())
    }).collect()
}

pub fn diff_counts(diff: &str) -> (usize, usize) {
    parse_diff(diff).iter().fold((0, 0), |(a, d), (k, ..)| match k { Kind::Add => (a + 1, d), Kind::Del => (a, d + 1), _ => (a, d) })
}

fn diff_rows(id: SharedString, rows: Vec<(Kind, String, String)>, lang: Option<&str>, limit: usize, copy: String, cx: &App) -> Div {
    let p = pal(cx);
    let joined = rows.iter().map(|r| r.2.as_str()).collect::<Vec<_>>().join("\n");
    let ls = lines(&joined, lang, cx);
    let widest = rows.iter().map(|r| r.1.len()).max().unwrap_or(1);
    let gw = gutter_w(widest, cx);
    let mut col = v_flex().w_full().py_1();
    for (i, (k, num, _)) in rows.iter().enumerate().take(limit) {
        if *k == Kind::Gap {
            col = col.child(div().w_full().px_3().text_color(p.dim.opacity(0.7)).child("⋯"));
            continue;
        }
        let (bg, sign_c, sign) = match k { Kind::Add => (p.green.opacity(0.13), p.green, "+"), Kind::Del => (p.red.opacity(0.13), p.red, "−"), _ => (transparent_black(), p.dim, " ") };
        let line = ls.get(i);
        col = col.child(h_flex().items_start().w_full().pr_8().bg(bg)
            .child(div().flex_none().w(gw).ml_2().text_right().text_color(p.dim.opacity(0.7)).child(num.clone()))
            .child(div().flex_none().w(px(18.)).text_center().text_color(sign_c).child(sign))
            .child(div().flex_1().min_w_0().when_some(line, |d, l| d.child(styled(l)))));
    }
    frame(id, Some(copy), cx).child(col)
}

pub fn diff(id: impl Into<SharedString>, diff: &str, lang: Option<&str>, limit: usize, cx: &App) -> Div {
    let rows = parse_diff(diff);
    // copying a diff gives the new side, the thing you'd paste somewhere
    let copy = rows.iter().filter(|r| r.0 != Kind::Del && r.0 != Kind::Gap).map(|r| r.2.as_str()).collect::<Vec<_>>().join("\n");
    diff_rows(id.into(), rows, lang, limit, copy, cx)
}
pub fn diff_len(diff: &str) -> usize { diff.lines().count() }

// before pi sends its numbered diff (streaming, or a failed edit) the edit args are all there is
pub fn edits(id: impl Into<SharedString>, edits: &[(String, String)], lang: Option<&str>, limit: usize, cx: &App) -> Div {
    let mut rows = vec![];
    for (i, (o, n)) in edits.iter().enumerate() {
        if i > 0 { rows.push((Kind::Gap, String::new(), String::new())); }
        if !o.is_empty() { rows.extend(o.split('\n').map(|l| (Kind::Del, String::new(), l.to_string()))); }
        if !n.is_empty() { rows.extend(n.split('\n').map(|l| (Kind::Add, String::new(), l.to_string()))); }
    }
    let copy = edits.iter().map(|e| e.1.as_str()).collect::<Vec<_>>().join("\n");
    diff_rows(id.into(), rows, lang, limit, copy, cx)
}

// a fence's language ("rs", "rust", "sh", "tsx") as tree-sitter's name
pub fn fence_lang(lang: &str) -> Option<&'static str> {
    let l = lang.to_ascii_lowercase();
    match l.as_str() {
        "" | "text" | "txt" | "plain" => None,
        "rust" => Some("rust"), "typescript" => Some("typescript"), "javascript" | "js" | "jsx" => Some("javascript"), "python" | "py" => Some("python"),
        "shell" | "sh" | "bash" | "zsh" | "console" | "fish" => Some("bash"), "json" | "jsonc" => Some("json"), "toml" => Some("toml"), "yaml" | "yml" => Some("yaml"),
        "go" | "golang" => Some("go"), "c" => Some("c"), "cpp" | "c++" => Some("cpp"), "html" | "xml" | "svg" => Some("html"), "css" | "scss" => Some("css"),
        "markdown" | "md" => Some("markdown"), "diff" | "patch" => Some("diff"),
        e => lang_of(&format!("x.{e}")),
    }
}

// the whole text with its colour runs, for one selectable block (code blocks, tool bodies)
pub fn joined(text: &str, lang: Option<&str>, cx: &App) -> (SharedString, Runs) {
    let ls = lines(text, lang, cx);
    let mut out = String::with_capacity(text.len());
    let mut runs = vec![];
    for (i, l) in ls.iter().enumerate() {
        if i > 0 { out.push('\n'); }
        let at = out.len();
        out.push_str(&l.text);
        runs.extend(l.runs.iter().map(|(r, s)| (r.start + at..r.end + at, *s)));
    }
    (out.into(), runs)
}
