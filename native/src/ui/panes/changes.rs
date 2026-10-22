// What changed in the session's folder: git status up top, the picked file's diff below.
// git runs on a worker; a refresh that's overtaken by a newer one is dropped
use crate::core::sys;
use crate::ui::theme::pal;
use gpui_kit::component::diff::{Diff, DiffChangeIndicator, DiffFile, DiffHunkSeparator, DiffState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Icon, Sizable};
use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::collections::HashMap;

#[derive(Clone)]
pub struct GitFile { pub path: String, code: String, add: i64, del: i64 }

enum Status { Loading, NoRepo, Repo { branch: String, files: Vec<GitFile> } }

pub enum ChangesEvent { OpenFile(String, usize) }
impl EventEmitter<ChangesEvent> for Changes {}

pub struct Changes {
    cwd: String,
    tab: Option<EntityId>,
    status: Status,
    // the file picked in each session, so switching sessions comes back to it
    sel: HashMap<EntityId, String>,
    diff: Entity<DiffState>,
    diff_empty: bool,
    gen: u64,
    pub busy: bool,
}

impl Changes {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let diff = cx.new(|cx| DiffState::new([], cx));
        Changes { cwd: String::new(), tab: None, status: Status::Loading, sel: HashMap::new(), diff, diff_empty: true, gen: 0, busy: false }
    }

    pub fn show(&mut self, tab: EntityId, cwd: &str, cx: &mut Context<Self>) {
        if self.tab != Some(tab) || self.cwd != cwd {
            self.tab = Some(tab);
            self.cwd = cwd.to_string();
            self.status = Status::Loading;
            self.diff_empty = true;
            self.diff.update(cx, |d, cx| d.set_files([], cx));
        }
        self.refresh(cx);
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
                if st["repo"] != true { this.status = Status::NoRepo; this.busy = false; cx.notify(); return None; }
                let files: Vec<GitFile> = st["files"].as_array().into_iter().flatten().map(|f| GitFile {
                    path: f["path"].as_str().unwrap_or("").into(), code: f["code"].as_str().unwrap_or("").into(),
                    add: f["add"].as_i64().unwrap_or(0), del: f["del"].as_i64().unwrap_or(0),
                }).collect();
                let tab = this.tab?;
                let cur = this.sel.get(&tab).filter(|p| files.iter().any(|f| &f.path == *p)).cloned().or_else(|| files.first().map(|f| f.path.clone()));
                let pick = cur.and_then(|p| files.iter().find(|f| f.path == p).cloned());
                this.status = Status::Repo { branch: st["branch"].as_str().unwrap_or("").into(), files };
                if pick.is_none() { this.busy = false; this.diff_empty = true; this.diff.update(cx, |d, cx| d.set_files([], cx)); }
                cx.notify();
                pick
            }) else { return };
            let Some(f) = pick else { return };
            let text = cx.background_spawn(async move {
                let mut t = sys::git_diff(&cwd, &f.path, f.code == "??");
                // a huge diff isn't worth reading in a side panel, and it'd stall parsing
                if t.len() > 2_000_000 { t.truncate(t.floor_char_boundary(2_000_000)); }
                DiffFile::parse(&t).unwrap_or_default()
            }).await;
            let _ = this.update(cx, |this, cx| {
                if this.gen != gen { return; }
                this.busy = false;
                this.diff_empty = text.iter().all(|f| !f.has_changes());
                this.diff.update(cx, |d, cx| d.set_files(text, cx));
                cx.notify();
            });
        }).detach();
    }

    fn pick(&mut self, path: String, cx: &mut Context<Self>) {
        let Some(tab) = self.tab else { return };
        self.sel.insert(tab, path);
        self.refresh(cx);
    }

    pub fn forget(&mut self, tab: EntityId) { self.sel.remove(&tab); }

    // the summary for the panel's header row
    pub fn summary(&self) -> Option<(String, usize, i64, i64)> {
        match &self.status {
            Status::Repo { branch, files } => Some((branch.clone(), files.len(), files.iter().map(|f| f.add).sum(), files.iter().map(|f| f.del).sum())),
            _ => None,
        }
    }
}

fn code_label(code: &str) -> (&'static str, char) {
    let c = code.trim();
    if c == "??" { return ("new", 'A'); }
    let k = c.chars().next().unwrap_or('M');
    (match k { 'A' => "A", 'D' => "D", 'R' => "R", 'C' => "C", 'U' => "U", _ => "M" }, k)
}

impl Render for Changes {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = pal(cx);
        let t = cx.theme();
        let empty = |title: &str, sub: String| v_flex().flex_1().items_center().justify_center().gap_1().p_6().text_sm()
            .child(div().text_color(t.foreground).child(title.to_string())).child(div().text_color(p.dim).text_center().child(sub)).into_any_element();
        let body: AnyElement = match &self.status {
            Status::Loading => v_flex().flex_1().items_center().justify_center().child(Spinner::new().small().color(p.dim)).into_any_element(),
            Status::NoRepo => empty("Not a git repository", crate::ui::tools::tilde(&self.cwd)),
            Status::Repo { files, .. } if files.is_empty() => empty("No changes", "The working tree is clean".into()),
            Status::Repo { files, .. } => {
                let cur = self.tab.and_then(|t| self.sel.get(&t).cloned()).filter(|c| files.iter().any(|f| &f.path == c)).or_else(|| files.first().map(|f| f.path.clone()));
                let rows = files.iter().map(|f| {
                    let on = cur.as_deref() == Some(f.path.as_str());
                    let (label, k) = code_label(&f.code);
                    let color = match k { 'A' => p.green, 'D' => p.red, _ => p.gold };
                    let (dir, base) = match f.path.rfind('/') { Some(i) => (f.path[..=i].to_string(), f.path[i + 1..].to_string()), None => (String::new(), f.path.clone()) };
                    let path = f.path.clone();
                    h_flex().id(SharedString::from(format!("ch-{}", f.path))).h(px(26.)).px_2().gap_2().items_center().rounded(t.radius).text_sm()
                        .when(on, |d| d.bg(t.list_active)).hover(|s| s.bg(if on { t.list_active } else { t.list_hover }))
                        .child(div().w(px(26.)).flex_none().text_xs().font_weight(FontWeight::MEDIUM).text_color(color).child(label))
                        .child(h_flex().flex_1().min_w_0().overflow_hidden()
                            .child(div().flex_none().text_color(t.foreground).child(base))
                            .child(div().min_w_0().ml_2().truncate().text_xs().text_color(p.dim).child(dir.trim_end_matches('/').to_string())))
                        .child(h_flex().flex_none().gap_1p5().text_xs().font_family(t.mono_font_family.clone())
                            .when(f.add > 0, |d| d.child(div().text_color(p.green).child(format!("+{}", f.add))))
                            .when(f.del > 0, |d| d.child(div().text_color(p.red).child(format!("−{}", f.del)))))
                        .on_click(cx.listener(move |this, _, _, cx| this.pick(path.clone(), cx)))
                });
                let rows: Vec<_> = rows.collect();
                let n = rows.len();
                let open = cx.entity().downgrade();
                v_flex().flex_1().min_h_0()
                    .child(div().id("ch-files").flex_none().max_h(px((n.min(9) as f32) * 26. + 8.)).overflow_y_scroll().p_1().children(rows))
                    .child(div().h_px().flex_none().bg(t.border))
                    .child(if self.diff_empty && !self.busy {
                        empty("No text changes", "A binary file, or only its mode changed".into())
                    } else {
                        div().flex_1().min_h_0().child(
                            Diff::new(&self.diff).header_visible(false).hunk_separator(DiffHunkSeparator::LineInfo).change_indicator(DiffChangeIndicator::Bars).size_full()
                                // double click a line to see it in the whole file
                                .on_line_click(move |pos, ev, _, cx| {
                                    if ev.click_count() < 2 { return; }
                                    let (path, line) = (pos.path().to_string(), pos.line());
                                    let _ = open.update(cx, |_, cx| cx.emit(ChangesEvent::OpenFile(path, line)));
                                }),
                        ).into_any_element()
                    })
                    .into_any_element()
            }
        };
        v_flex().size_full().child(body)
    }
}

pub fn branch_icon() -> Icon { Icon::new(IconName::GitBranch).xsmall() }
