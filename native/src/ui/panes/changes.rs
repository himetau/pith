// What changed in the session's folder: git status up top, the picked file's unified diff below (panes.ts Changes).
// git runs on a worker; a refresh that's overtaken by a newer one is dropped
use super::code::{self, Cache};
use super::{btn_sm, dn_color, empty, pb, pb_icon, pbar};
use crate::core::sys;
use crate::state::tab::{How, Tab};
use crate::ui::theme::{em, fade, geo, pal, u};
use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::collections::HashMap;

#[derive(Clone)]
pub struct GitFile { pub path: String, code: String, add: i64, del: i64 }

enum Status { Loading, NoRepo, Repo { branch: String, files: Vec<GitFile> } }

#[derive(Clone, Copy, PartialEq)]
enum Row { Hunk, Add, Del, Ctx }
// one line of the unified view: the old and new numbers, and the code (or the hunk's "line N · context")
struct DLine { kind: Row, old: Option<usize>, new: Option<usize>, text: String, code: Option<usize> }

pub enum ChangesEvent { OpenFile(String, usize) }
impl EventEmitter<ChangesEvent> for Changes {}

pub struct Changes {
    cwd: String,
    tab: Option<Entity<Tab>>,
    status: Status,
    // the file picked in each session, so switching sessions comes back to it
    sel: HashMap<EntityId, String>,
    cur: Option<String>,
    lines: Vec<DLine>,
    code: String,
    hl: Cache,
    hl_lines: std::rc::Rc<Vec<code::Line>>,
    version: u64,
    list: ListState,
    gen: u64,
    pub busy: bool,
}

impl Changes {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Changes { cwd: String::new(), tab: None, status: Status::Loading, sel: HashMap::new(), cur: None, lines: vec![], code: String::new(), hl: Cache::default(), hl_lines: Default::default(), version: 0,
            list: ListState::new(0, ListAlignment::Top, px(800.)), gen: 0, busy: false }
    }

    pub fn show(&mut self, tab: &Entity<Tab>, cx: &mut Context<Self>) {
        let cwd = tab.read(cx).cwd.clone();
        if self.tab.as_ref() != Some(tab) || self.cwd != cwd {
            self.tab = Some(tab.clone());
            self.cwd = cwd;
            self.status = Status::Loading;
            self.set_diff(None, String::new());
        }
        self.refresh(cx);
    }

    fn set_diff(&mut self, cur: Option<String>, text: String) {
        self.cur = cur;
        self.lines = unified(&text);
        let code: Vec<&str> = self.lines.iter().filter(|l| l.code.is_some()).map(|l| l.text.as_str()).collect();
        self.code = code.join("\n");
        self.version += 1;
        self.list.reset(self.lines.len());
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.cwd.is_empty() { return; }
        self.gen += 1;
        let gen = self.gen;
        let cwd = self.cwd.clone();
        self.busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let c2 = cwd.clone();
            let st = cx.background_spawn(async move { sys::git_status(&c2) }).await;
            let Ok(pick) = this.update(cx, |this, cx| {
                if this.gen != gen { return None; }
                if st["repo"] != true { this.status = Status::NoRepo; this.busy = false; this.set_diff(None, String::new()); cx.notify(); return None; }
                let files: Vec<GitFile> = st["files"].as_array().into_iter().flatten().map(|f| GitFile {
                    path: f["path"].as_str().unwrap_or("").into(), code: f["code"].as_str().unwrap_or("").into(),
                    add: f["add"].as_i64().unwrap_or(0), del: f["del"].as_i64().unwrap_or(0),
                }).collect();
                let tab = this.tab.as_ref()?.entity_id();
                let cur = this.sel.get(&tab).filter(|p| files.iter().any(|f| &f.path == *p)).cloned().or_else(|| files.first().map(|f| f.path.clone()));
                let pick = cur.and_then(|p| files.iter().find(|f| f.path == p).cloned());
                this.status = Status::Repo { branch: st["branch"].as_str().unwrap_or("").into(), files };
                if pick.is_none() { this.busy = false; this.set_diff(None, String::new()); }
                cx.notify();
                pick
            }) else { return };
            let Some(f) = pick else { return };
            let path = f.path.clone();
            let text = cx.background_spawn(async move {
                let mut t = sys::git_diff(&cwd, &f.path, f.code == "??");
                // a huge diff isn't worth reading in a side panel
                if t.len() > 2_000_000 { t.truncate(t.floor_char_boundary(2_000_000)); }
                t
            }).await;
            let _ = this.update(cx, |this, cx| {
                if this.gen != gen { return; }
                this.busy = false;
                this.set_diff(Some(path), text);
                cx.notify();
            });
        }).detach();
    }

    fn pick(&mut self, path: String, cx: &mut Context<Self>) {
        let Some(tab) = &self.tab else { return };
        self.sel.insert(tab.entity_id(), path);
        self.refresh(cx);
    }

    pub fn forget(&mut self, tab: EntityId) { self.sel.remove(&tab); }

    // act.runCommand: the session runs it, as if typed
    fn run(&self, text: &str, cx: &mut Context<Self>) {
        let Some(t) = self.tab.clone() else { return };
        let app = crate::state::app::Pith::global(cx);
        app.update(cx, |a, cx| a.focus(t.clone(), cx));
        t.update(cx, |t, cx| t.send(text.into(), vec![], How::Auto, cx));
    }

    fn bar(&self, cx: &mut Context<Self>) -> Div {
        let p = pal(cx);
        let bar = pbar(cx).gap(u(8.));
        match &self.status {
            Status::Loading => bar.child(div().h(u(26.))),
            Status::NoRepo => bar.child(div().h(u(26.)).flex().items_center().text_color(p.dim).child("not a git repository")),
            Status::Repo { branch, files } => {
                let (add, del): (i64, i64) = (files.iter().map(|f| f.add).sum(), files.iter().map(|f| f.del).sum());
                let none = files.is_empty();
                bar.child(h_flex().min_w_0().gap(u(5.)).text_color(p.mid).child(pb_icon(IconName::GitBranch, cx)).child(div().truncate().child(branch.clone())))
                    .child(h_flex().flex_none().text_color(p.dim).child(format!("{} files\u{a0}", files.len()))
                        .child(div().text_color(p.add).child(format!("+{add}"))).child("\u{a0}").child(div().text_color(p.red).child(format!("-{del}"))))
                    .child(div().flex_1())
                    .child(pb("ch-refresh", false, cx).child(pb_icon(IconName::RotateCw, cx)).tooltip(|w, cx| gpui_kit::component::tooltip::Tooltip::new("refresh").build(w, cx))
                        .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))))
                    .child(btn_sm("ch-review", false, none, cx).child(Icon::new(IconName::SquareCheck).size(em(cx, 0.9 * 0.88 * 1.2))).child("review")
                        .when(!none, |d| d.on_click(cx.listener(|this, _, _, cx| this.run("/review", cx)))))
                    .child(btn_sm("ch-security", true, none, cx).child("security")
                        .when(!none, |d| d.on_click(cx.listener(|this, _, _, cx| this.run("/security-review", cx)))))
            }
        }
    }

    fn row(&mut self, ix: usize, cw: Pixels, cx: &mut Context<Self>) -> AnyElement {
        let p = pal(cx);
        let lines = self.hl_lines.clone();
        let Some(l) = self.lines.get(ix) else { return div().into_any_element() };
        let num = |n: Option<usize>| div().w(cw * 5.).flex_none().flex().justify_end().pr(u(8.)).text_color(dn_color(cx)).child(n.map(|n| n.to_string()).unwrap_or_default());
        let fs = geo(cx).fs * 0.9;
        let base = h_flex().id(ix).w_full().whitespace_nowrap().overflow_hidden().text_size(u(fs)).line_height(u(fs * 1.5)).font_family(geo(cx).mono.clone());
        match l.kind {
            Row::Hunk => base.mt(u(6.)).bg(fade(p.blue, 0.08)).text_color(p.blue)
                .child(div().w(cw * 5.).flex_none().flex().justify_end().pr(u(8.)).text_color(dn_color(cx)).child("⋯")).child(num(None))
                .child(div().child(l.text.clone())).into_any_element(),
            kind => {
                let text: AnyElement = match l.code.and_then(|i| lines.get(i)) { Some(hl) => code::styled(hl).into_any_element(), None => div().child(l.text.clone()).into_any_element() };
                let target = l.new.filter(|_| kind != Row::Del);
                let path = self.cur.clone().unwrap_or_default();
                base.when(kind == Row::Add, |d| d.bg(p.add_bg)).when(kind == Row::Del, |d| d.bg(p.del_bg))
                    .child(num(l.old)).child(num(l.new)).child(div().min_w_0().child(text))
                    // double click a line to see it in the whole file
                    .when_some(target, |d, n| d.on_click(cx.listener(move |_, e: &ClickEvent, _, cx| if e.click_count() >= 2 { cx.emit(ChangesEvent::OpenFile(path.clone(), n)); })))
                    .into_any_element()
            }
        }
    }
}

// git's patch as the reference's unified view: headers dropped, hunks as "line N · context", both line numbers
fn unified(text: &str) -> Vec<DLine> {
    let (mut o, mut n, mut code) = (0usize, 0usize, 0usize);
    let mut out = vec![];
    for l in text.split('\n') {
        if ["diff --git", "index ", "--- ", "+++ ", "new file", "deleted file", "similarity", "rename "].iter().any(|h| l.starts_with(h)) { continue; }
        if let Some(rest) = l.strip_prefix("@@ -") {
            let Some((nums, ctx)) = rest.split_once(" @@") else { continue };
            let mut it = nums.split(" +");
            o = it.next().and_then(|a| a.split(',').next()?.parse().ok()).unwrap_or(0);
            n = it.next().and_then(|a| a.split(',').next()?.parse().ok()).unwrap_or(0);
            let ctx = ctx.trim();
            out.push(DLine { kind: Row::Hunk, old: None, new: None, text: if ctx.is_empty() { format!("line {n}") } else { format!("line {n}  ·  {ctx}") }, code: None });
            continue;
        }
        if l.starts_with('\\') || l.is_empty() { continue; }
        let (sign, rest) = l.split_at(l.ceil_char_boundary(1));
        let (kind, old, new) = match sign {
            "+" => { n += 1; (Row::Add, None, Some(n - 1)) }
            "-" => { o += 1; (Row::Del, Some(o - 1), None) }
            " " => { o += 1; n += 1; (Row::Ctx, Some(o - 1), Some(n - 1)) }
            _ => continue,
        };
        out.push(DLine { kind, old, new, text: rest.replace('\t', "    "), code: Some(code) });
        code += 1;
    }
    out
}

fn code_of(code: &str) -> (String, char) {
    let c = code.trim();
    if c == "??" { return ("new".into(), 'N'); }
    (c.to_string(), c.chars().next().unwrap_or('M'))
}

impl Render for Changes {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (p, g) = (pal(cx), geo(cx));
        let bar = self.bar(cx);
        self.hl_lines = self.hl.lines(self.version, &self.code, self.cur.as_deref().and_then(code::lang_of), cx);
        let body: AnyElement = match &self.status {
            Status::Loading => div().flex_1().into_any_element(),
            Status::NoRepo => empty(Some("no git here"), crate::ui::tools::tilde(&self.cwd), cx).into_any_element(),
            Status::Repo { files, .. } => {
                let fs = g.fs * 0.9;
                let rows: Vec<AnyElement> = files.iter().map(|f| {
                    let on = self.cur.as_deref() == Some(f.path.as_str());
                    let (label, k) = code_of(&f.code);
                    let color = match k { 'N' | 'A' => p.green, 'D' => p.red, _ => p.gold };
                    let (dir, base) = match f.path.rfind('/') { Some(i) => (f.path[..=i].to_string(), f.path[i + 1..].to_string()), None => (String::new(), f.path.clone()) };
                    let path = f.path.clone();
                    let hover = p.hover;
                    h_flex().id(SharedString::from(format!("ch-{}", f.path))).gap(u(8.)).items_baseline().py(u(2.)).px(u(8.)).rounded(u(4.)).cursor_pointer().text_size(u(fs))
                        .when(on, |d| d.bg(p.high)).when(!on, |d| d.hover(move |s| s.bg(hover)))
                        .child(div().w(u(fs * 0.6 * 4.)).flex_none().text_size(u(fs * 0.8)).text_color(color).child(label))
                        .child(h_flex().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().child(div().text_color(p.dim).child(dir)).child(div().child(base)))
                        .child(h_flex().flex_none().text_size(u(fs * 0.85)).child(div().text_color(p.add).child(format!("+{}", f.add))).child("\u{a0}").child(div().text_color(p.red).child(format!("-{}", f.del))))
                        .on_click(cx.listener(move |this, _, _, cx| this.pick(path.clone(), cx)))
                        .into_any_element()
                }).collect();
                let none = rows.is_empty();
                // the code column's character width, for the 5ch gutters
                let mono = font(g.mono.clone());
                let ts = window.text_system().clone();
                let dfs = u(fs).to_pixels(window.rem_size());
                let cw = ts.advance(ts.resolve_font(&mono), dfs, '0').map(|s| s.width).unwrap_or(dfs * 0.6);
                let diff: AnyElement = if self.cur.is_none() { div().flex_1().into_any_element() }
                    else if self.lines.is_empty() && !self.busy { div().flex_1().child(div().p(u(14.)).text_color(p.dim).child("binary file, or no textual change")).into_any_element() }
                    else {
                        // the list ignores its own padding, so the first and last rows carry .ch-diff's
                        list(self.list.clone(), cx.processor(move |this, ix, _, cx| { let n = this.lines.len(); div().when(ix == 0, |d| d.pt(u(6.))).when(ix + 1 == n, |d| d.pb(u(20.))).child(this.row(ix, cw, cx)).into_any_element() }))
                            .flex_1().min_h_0().text_size(u(fs)).line_height(relative(1.5)).font_family(g.mono.clone()).into_any_element()
                    };
                v_flex().flex_1().min_h_0()
                    .child(div().id("ch-files").flex_none().max_h(relative(0.34)).overflow_y_scroll().p(u(4.)).border_b(u(1.)).border_color(p.rule)
                        .when(none, |d| d.child(div().p(u(14.)).text_color(p.dim).child("working tree is clean"))).children(rows))
                    .child(diff)
                    .into_any_element()
            }
        };
        v_flex().size_full().child(bar).child(body)
    }
}
