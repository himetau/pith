// Markdown as the webview showed it (chat.css .prose): opencode's colours off the palette, "##" markers before
// headings, "–" bullets, tables with rule lines, code blocks with a bar (language, copy, preview for html), inline
// code that looks like a path opens the file. parsed once per text and kept, the list redraws visible rows every frame
use super::code;
use super::text::{sel, OnClick};
use crate::state::tab::{Tab, TabEvent};
use crate::ui::theme::{geo, pal, u, Pal};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use markdown::mdast::Node;
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::rc::Rc;
use std::time::Instant;

const STRONG: u8 = 1;
const EM: u8 = 2;
const CODE: u8 = 4;
const PATH: u8 = 8;
const LINK: u8 = 16;
const DEL: u8 = 32;

#[derive(Clone)]
enum Target { Path(String), Url(String) }

#[derive(Default, Clone)]
pub struct Inl { text: String, segs: Vec<(Range<usize>, u8)>, targets: Vec<(Range<usize>, Target)> }

impl Inl {
    fn push(&mut self, s: &str, f: u8) {
        if s.is_empty() { return; }
        let at = self.text.len();
        self.text.push_str(s);
        if f != 0 { self.segs.push((at..self.text.len(), f)); }
    }
    pub fn plain(s: &str) -> Inl { let mut i = Inl::default(); i.push(s, 0); i }
}

#[derive(Clone)]
enum Blk {
    P(Inl),
    H(u8, Inl),
    Code { lang: String, text: String },
    List { ordered: bool, start: u32, tight: bool, items: Vec<Vec<Blk>> },
    Quote(Vec<Blk>),
    Rule,
    Table { head: Vec<Inl>, rows: Vec<Vec<Inl>> },
}

// a model's `src/foo.rs:12` in backticks (lib/md.ts's codespan rule)
pub fn is_path(t: &str) -> bool {
    if t.is_empty() || t.starts_with(|c: char| c.is_ascii_digit()) { return false; }
    let mut s = t;
    // :12 or :12-20 at the end
    if let Some((a, b)) = s.rsplit_once(':') {
        let (n1, n2) = b.split_once('-').unwrap_or((b, "1"));
        if !n1.is_empty() && n1.chars().all(|c| c.is_ascii_digit()) && !n2.is_empty() && n2.chars().all(|c| c.is_ascii_digit()) { s = a; }
    }
    for p in ["../", "./", "/", "~"] { if let Some(r) = s.strip_prefix(p) { s = r; break; } }
    let ok = |c: char| c.is_ascii_alphanumeric() || "_@.+-".contains(c);
    if s.is_empty() || s.split('/').any(|seg| seg.is_empty() || !seg.chars().all(ok)) { return false; }
    let last = s.rsplit('/').next().unwrap_or(s);
    // ends in something, a dot, then 1-8 word chars
    last.rsplit_once('.').is_some_and(|(a, e)| !a.is_empty() && (1..=8).contains(&e.len()) && e.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
}

// "path:12-20" → ("path", Some(12))
pub fn split_line(t: &str) -> (String, Option<usize>) {
    let mut s = t;
    if let Some((a, b)) = s.rsplit_once('-') { if !b.is_empty() && b.chars().all(|c| c.is_ascii_digit()) && a.contains(':') { s = a; } }
    match s.rsplit_once(':') {
        Some((a, b)) if !b.is_empty() && b.chars().all(|c| c.is_ascii_digit()) => (a.to_string(), b.parse().ok()),
        _ => (s.to_string(), None),
    }
}

// opens a path from the chat in the files pane, at a line when it has one
pub fn open_path(path: &str, line: Option<usize>, cx: &mut App) {
    let p = match path.strip_prefix("~/") { Some(r) => crate::core::sys::home().join(r).display().to_string(), None => path.to_string() };
    crate::ui::panes::reveal(&p, line, cx);
}

fn inline(n: &Node, f: u8, out: &mut Inl) {
    match n {
        Node::Text(t) => out.push(&t.value, f),
        Node::Strong(x) => x.children.iter().for_each(|c| inline(c, f | STRONG, out)),
        Node::Emphasis(x) => x.children.iter().for_each(|c| inline(c, f | EM, out)),
        Node::Delete(x) => x.children.iter().for_each(|c| inline(c, f | DEL, out)),
        Node::InlineCode(c) => {
            let at = out.text.len();
            if f & LINK == 0 && is_path(&c.value) {
                out.push(&c.value, f | PATH);
                out.targets.push((at..out.text.len(), Target::Path(c.value.clone())));
            } else { out.push(&c.value, f | CODE); }
        }
        Node::Link(l) => {
            let at = out.text.len();
            l.children.iter().for_each(|c| inline(c, f | LINK, out));
            out.targets.push((at..out.text.len(), Target::Url(l.url.clone())));
        }
        Node::Break(_) => out.push("\n", f),
        Node::Image(i) => out.push(&i.alt, f | EM),
        Node::Html(h) => out.push(&h.value, f),
        Node::FootnoteReference(r) => out.push(&format!("[{}]", r.identifier), f),
        n => if let Some(ch) = n.children() { ch.iter().for_each(|c| inline(c, f, out)) },
    }
}

fn inl_of(children: &[Node]) -> Inl { let mut i = Inl::default(); children.iter().for_each(|c| inline(c, 0, &mut i)); i }

fn blocks(nodes: &[Node]) -> Vec<Blk> {
    let mut out = vec![];
    for n in nodes {
        match n {
            Node::Paragraph(p) => out.push(Blk::P(inl_of(&p.children))),
            Node::Heading(h) => out.push(Blk::H(h.depth, inl_of(&h.children))),
            Node::Code(c) => out.push(Blk::Code { lang: c.lang.clone().unwrap_or_default().split_whitespace().next().unwrap_or("").to_string(), text: c.value.clone() }),
            Node::List(l) => {
                let tight = !l.spread && l.children.iter().all(|i| !matches!(i, Node::ListItem(li) if li.spread));
                out.push(Blk::List { ordered: l.ordered, start: l.start.unwrap_or(1), tight, items: l.children.iter().map(|i| blocks(i.children().map(|c| c.as_slice()).unwrap_or(&[]))).collect() });
            }
            Node::Blockquote(q) => out.push(Blk::Quote(blocks(&q.children))),
            Node::ThematicBreak(_) => out.push(Blk::Rule),
            Node::Table(t) => {
                let mut rows: Vec<Vec<Inl>> = t.children.iter().map(|r| r.children().map(|cs| cs.iter().map(|c| inl_of(c.children().map(|x| x.as_slice()).unwrap_or(&[]))).collect()).unwrap_or_default()).collect();
                let head = if rows.is_empty() { vec![] } else { rows.remove(0) };
                out.push(Blk::Table { head, rows });
            }
            Node::Html(h) => out.push(Blk::P(Inl::plain(h.value.trim_end()))),
            Node::FootnoteDefinition(d) => out.extend(blocks(&d.children)),
            n => if let Some(ch) = n.children() { out.extend(blocks(ch)) },
        }
    }
    out
}

thread_local! {
    static CACHE: RefCell<HashMap<u64, Rc<Vec<Blk>>>> = RefCell::new(HashMap::new());
    static COPIED: RefCell<HashMap<String, Instant>> = RefCell::new(HashMap::new());
    static CH: RefCell<HashMap<String, f32>> = RefCell::new(HashMap::new());
}

fn parse(text: &str) -> Rc<Vec<Blk>> {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    let key = h.finish();
    if let Some(b) = CACHE.with(|c| c.borrow().get(&key).cloned()) { return b; }
    let mut o = markdown::ParseOptions::gfm();
    o.constructs.gfm_strikethrough = true;
    o.gfm_strikethrough_single_tilde = false;
    let b = Rc::new(markdown::to_mdast(text, &o).ok().map(|n| blocks(n.children().map(|c| c.as_slice()).unwrap_or(&[]))).unwrap_or_else(|| vec![Blk::P(Inl::plain(text))]));
    CACHE.with(|c| { let mut c = c.borrow_mut(); if c.len() > 600 { c.clear(); } c.insert(key, b.clone()); });
    b
}

// css ch of the reading font, as a fraction of its size
pub fn ch(cx: &App) -> f32 {
    let fam = geo(cx).prose;
    if let Some(v) = CH.with(|c| c.borrow().get(fam.as_ref()).copied()) { return v; }
    let ts = cx.text_system();
    let v = ts.ch_width(ts.resolve_font(&font(fam.clone())), px(100.)).map(|w| f32::from(w) / 100.).unwrap_or(0.6);
    CH.with(|c| c.borrow_mut().insert(fam.to_string(), v));
    v
}

// how a run of inline flags looks
fn style(f: u8, p: &Pal) -> HighlightStyle {
    let mut s = HighlightStyle::default();
    if f & STRONG != 0 { s.color = Some(p.md_strong); s.font_weight = Some(FontWeight::BOLD); }
    if f & EM != 0 { s.font_style = Some(FontStyle::Italic); if f & STRONG == 0 { s.color = Some(p.md_em); } }
    if f & CODE != 0 { s.color = Some(p.md_code); }
    if f & DEL != 0 { s.strikethrough = Some(StrikethroughStyle { thickness: px(1.), color: None }); }
    if f & LINK != 0 { s.color = Some(p.accent); s.underline = Some(UnderlineStyle { thickness: px(1.), color: Some(p.accent.opacity(0.45)), wavy: false }); }
    if f & PATH != 0 { s.color = Some(p.cyan); s.background_color = Some(p.cyan.opacity(0.09)); }
    s
}

// what's needed to draw one document
#[derive(Clone)]
pub struct Ctx { pub pal: Pal, pub tab: Option<Entity<Tab>>, pub caret: bool }

fn runs(i: &Inl, p: &Pal) -> Vec<(Range<usize>, HighlightStyle)> { i.segs.iter().map(|(r, f)| (r.clone(), style(*f, p))).collect() }

fn clicks(i: &Inl) -> Vec<(Range<usize>, OnClick)> {
    i.targets.iter().map(|(r, t)| {
        let f: OnClick = match t.clone() {
            Target::Path(p) => Rc::new(move |_, cx| { let (p, l) = split_line(&p); open_path(&p, l, cx); }),
            Target::Url(u) => Rc::new(move |_, cx| cx.open_url(&u)),
        };
        (r.clone(), f)
    }).collect()
}

fn text_el(id: String, i: &Inl, c: &Ctx, caret: bool) -> AnyElement {
    sel(SharedString::from(id), i.text.clone()).runs(runs(i, &c.pal)).clicks(clicks(i)).caret(caret.then_some(c.pal.primary)).into_any_element()
}

// margins in px, top and bottom, collapsing like css: a container takes its first and last child's when bigger
fn margins(b: &Blk, tight: bool, fs: f32) -> (f32, f32) {
    match b {
        Blk::P(_) if tight => (0., 0.),
        Blk::P(_) => (0.55 * fs, 0.55 * fs),
        Blk::H(d, _) => (1.1 * fs * hsize(*d), 0.4 * fs * hsize(*d)),
        Blk::Code { .. } => (0.7 * fs, 0.7 * fs),
        Blk::Table { .. } => (0.7 * fs * 0.92, 0.7 * fs * 0.92),
        Blk::List { .. } => (0.45 * fs, 0.45 * fs),
        Blk::Quote(k) => { let (a, z) = edges(k, false, fs); (a.max(0.6 * fs), z.max(0.6 * fs)) }
        Blk::Rule => (1.2 * fs, 1.2 * fs),
    }
}
fn edges(k: &[Blk], tight: bool, fs: f32) -> (f32, f32) { (k.first().map(|b| margins(b, tight, fs).0).unwrap_or(0.), k.last().map(|b| margins(b, tight, fs).1).unwrap_or(0.)) }

// a heading's size in em
fn hsize(d: u8) -> f32 { match d { 1 => 1.25, 2 => 1.12, 3 => 1.02, _ => 1. } }

// blocks one under the other; the first's top and the last's bottom margin belong to whoever holds them
fn flow(id: &str, bs: &[Blk], c: &Ctx, tight: bool, caret_last: bool, fs: f32, cx: &App) -> Div {
    let mut col = div().flex().flex_col().w_full().min_w_0();
    let mut prev: Option<f32> = None;
    let n = bs.len();
    for (i, b) in bs.iter().enumerate() {
        let (top, bottom) = margins(b, tight, fs);
        let gap = prev.map(|pb| pb.max(top)).unwrap_or(0.);
        prev = Some(bottom);
        col = col.child(div().w_full().min_w_0().when(gap > 0., |d| d.mt(u(gap))).child(block(&format!("{id}.{i}"), b, c, caret_last && i + 1 == n, fs, cx)));
    }
    col
}

fn block(id: &str, b: &Blk, c: &Ctx, caret: bool, fs: f32, cx: &App) -> AnyElement {
    let p = &c.pal;
    match b {
        Blk::P(i) => text_el(id.into(), i, c, caret && c.caret),
        Blk::H(d, i) => {
            // the "##" before it is faded and not bold, with half a ch after it
            let mark = match d { 1 => "#", 2 => "##", 3 => "###", _ => "" };
            let half = ch(cx) * fs * hsize(*d) * 0.5;
            div().flex().text_size(u(fs * hsize(*d))).line_height(relative(1.3)).font_weight(FontWeight::BOLD).text_color(p.md_head)
                .when(!mark.is_empty(), |d| d.child(div().flex_none().mr(u(half)).font_weight(FontWeight::NORMAL).text_color(p.md_head.opacity(0.45)).child(mark)))
                .child(div().flex_1().min_w_0().child(sel(SharedString::from(id.to_string()), i.text.clone()).runs(runs(i, p)).clicks(clicks(i)).caret((caret && c.caret).then_some(p.primary))))
                .into_any_element()
        }
        Blk::Code { lang, text } => code_block(id, lang, text, c, caret, fs, cx),
        Blk::List { ordered, start, tight, items } => {
            let chv = ch(cx);
            let n = items.len();
            let mut col = div().flex().flex_col().w_full();
            for (k, it) in items.iter().enumerate() {
                let marker = if *ordered { format!("{}.", start + k as u32) } else { "–".into() };
                let mk = div().flex_none().w(u(fs * chv * 2.2)).text_color(p.md_mark)
                    .when(*ordered, |d| d.flex().justify_end().pr(u(fs * chv)).whitespace_nowrap())
                    .when(!*ordered, |d| d.pl(u(fs * chv * 0.2)))
                    .child(marker);
                // li margins: .18em, more when its first or last child wants it
                let gap = if k == 0 { 0. } else { (0.18 * fs).max(edges(&items[k - 1], *tight, fs).1).max(edges(it, *tight, fs).0) };
                col = col.child(div().flex().w_full().min_w_0().when(gap > 0., |d| d.mt(u(gap))).child(mk)
                    .child(flow(&format!("{id}.{k}"), it, c, *tight, caret && k + 1 == n, fs, cx).flex_1()));
            }
            col.into_any_element()
        }
        Blk::Quote(k) => div().w_full().pl(u(12.)).border_l(u(2.)).border_color(p.line).text_color(p.mid)
            .child(flow(id, k, c, false, caret, fs, cx)).into_any_element(),
        Blk::Rule => div().w_full().child(crate::ui::tools::dashed(p.line)).into_any_element(),
        Blk::Table { head, rows } => {
            let chv = ch(cx) * fs * 0.92;
            let ncol = head.len().max(rows.iter().map(|r| r.len()).max().unwrap_or(0));
            // columns as wide as their longest cell, like a css auto table, within reason
            let widths: Vec<f32> = (0..ncol).map(|j| {
                let w = std::iter::once(head.get(j)).chain(rows.iter().map(|r| r.get(j))).flatten().map(|c| c.text.lines().map(|l| l.chars().count()).max().unwrap_or(0)).max().unwrap_or(0);
                w.min(48) as f32 * chv + 12. + 1.
            }).collect();
            let row = |r: &[Inl], rid: String, th: bool| {
                let mut h = div().flex();
                for (j, w) in widths.iter().enumerate() {
                    let cell = r.get(j).cloned().unwrap_or_default();
                    h = h.child(div().flex_none().w(u(*w)).pt(u(4.)).pb(u(4.)).pr(u(12.)).border_b(u(1.)).border_color(if th { p.line } else { p.rule })
                        .when(th, |d| d.text_color(p.mid))
                        .child(text_el(format!("{rid}.{j}"), &cell, c, false)));
                }
                h
            };
            let mut t = div().id(SharedString::from(format!("{id}-tbl"))).flex().flex_col().max_w_full().overflow_x_scroll().text_size(u(fs * 0.92)).child(row(head, format!("{id}.h"), true));
            for (k, r) in rows.iter().enumerate() { t = t.child(row(r, format!("{id}.r{k}"), false)); }
            t.into_any_element()
        }
    }
}

fn copied(id: &str) -> bool { COPIED.with(|c| c.borrow().get(id).is_some_and(|t| t.elapsed().as_millis() < 1200)) }

// .code: the bar with the language, copy and (html) preview, then the highlighted code
fn code_block(id: &str, lang: &str, text: &str, c: &Ctx, caret: bool, fs: f32, cx: &App) -> AnyElement {
    let p = &c.pal;
    let g = geo(cx);
    let tl = code::fence_lang(lang);
    let (joined, runs) = code::joined(text, tl, cx);
    let bar_btn = |bid: String, label: &str| {
        let (dim, txt, hov) = (p.dim, p.text, p.hover);
        div().id(SharedString::from(bid)).px(u(6.)).py(u(1.)).rounded(u(4.)).cursor_pointer().text_color(dim).hover(move |s| s.text_color(txt).bg(hov)).child(label.to_string())
    };
    let (cid, code_text) = (id.to_string(), text.to_string());
    let copy = bar_btn(format!("{id}-copy"), if copied(id) { "copied" } else { "copy" }).on_click(move |_, _, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string(code_text.clone()));
        COPIED.with(|m| m.borrow_mut().insert(cid.clone(), Instant::now()));
        cx.refresh_windows();
        cx.spawn(async |cx| { cx.background_executor().timer(std::time::Duration::from_millis(1250)).await; cx.update(|cx| cx.refresh_windows()); }).detach();
    });
    let preview = (lang == "html").then(|| {
        let (tab, html) = (c.tab.clone(), text.to_string());
        bar_btn(format!("{id}-pv"), "preview").on_click(move |_, _, cx| { if let Some(t) = &tab { preview_html(t, &html, cx); } })
    });
    div().w_full().border(u(1.)).border_color(p.line).rounded(u(g.r2)).overflow_hidden().bg(p.surface_low)
        .child(div().flex().items_center().gap(u(10.)).pl(u(12.)).pr(u(6.)).py(u(2.)).text_size(u(fs * 0.78)).text_color(p.dim).border_b(u(1.)).border_color(p.rule)
            .child(div().flex_1().child(if lang.is_empty() { "text".to_string() } else { lang.to_string() }))
            .child(copy).children(preview))
        .child(div().id(SharedString::from(format!("{id}-pre"))).w_full().overflow_x_scroll().px(u(14.)).py(u(10.)).text_size(u(fs * 0.9)).line_height(relative(1.5)).whitespace_nowrap().font_family(g.mono.clone()).text_color(p.text)
            .child(sel(SharedString::from(format!("{id}-code")), joined).runs(runs).caret((caret && c.caret).then_some(p.primary))))
        .into_any_element()
}

// a code block's html into a scratch file, shown in the preview pane (panes.ts previewHtml)
pub fn preview_html(tab: &Entity<Tab>, html: &str, cx: &mut App) {
    let dir = crate::core::sys::data_dir().join("scratch");
    let _ = std::fs::create_dir_all(&dir);
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let path = dir.join(format!("snippet-{stamp:x}.html"));
    if std::fs::write(&path, html).is_err() { return; }
    let path = path.display().to_string();
    tab.update(cx, |t, cx| {
        crate::core::preview::show(&t.id, Some(&path), None);
        t.preview = Some(serde_json::json!({ "path": path, "title": "code block" }));
        cx.emit(TabEvent::Preview);
    });
    crate::ui::panes::open(crate::ui::panes::Pane::Preview, cx);
}

// .prose: the reading font at 1.66, markdown below. `quiet` is thinking and summaries, the colours only hinted
pub fn prose(id: impl Into<SharedString>, text: &str, quiet: bool, caret: bool, tab: Option<&Entity<Tab>>, cx: &App) -> Div {
    let p = if quiet { pal(cx).quiet() } else { pal(cx) };
    let g = geo(cx);
    let id: SharedString = id.into();
    let bs = parse(text);
    let c = Ctx { pal: p, tab: tab.cloned(), caret };
    let fs = g.fs;
    let body = if bs.is_empty() && caret { div().child(sel(SharedString::from(format!("{id}.c")), "").caret(Some(c.pal.primary))) } else { flow(&id, &bs, &c, false, true, fs, cx) };
    div().w_full().min_w_0().font_family(g.prose.clone()).line_height(relative(1.66)).child(body)
}

// one line of markdown without the paragraph (titles, summaries)
pub fn inline_text(id: impl Into<SharedString>, text: &str, cx: &App) -> AnyElement {
    let bs = parse(text);
    let c = Ctx { pal: pal(cx), tab: None, caret: false };
    match bs.first() { Some(Blk::P(i)) | Some(Blk::H(_, i)) => text_el(id.into().to_string(), i, &c, false), _ => sel(id.into(), text.to_string()).into_any_element() }
}
