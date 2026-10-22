// New session (home.ts, pages.css .home): pick a folder (or a design), type, go. recent sessions and a few ways in below
use super::composer::{self, Composer, ComposerEvent};
use super::theme::{geo, mix, pal, u};
use super::workspace::{ago, session_title};
use crate::state::app::{Pith, View};
use crate::state::tab::{How, Kind};
use gpui_kit::assets::IconName;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

pub struct Home {
    composer: Entity<Composer>,
    _subs: Vec<Subscription>,
}

impl Home {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let composer = cx.new(|cx| Composer::new(None, window, cx));
        let app = Pith::global(cx);
        let subs = vec![
            cx.subscribe_in(&composer, window, |_, composer, ev: &ComposerEvent, window, cx| {
                let ComposerEvent::Submit { text, images, .. } = ev else { return };
                let (text, images) = (text.clone(), images.clone());
                let mode = composer.read(cx).pending_mode();
                let app = Pith::global(cx);
                let (cwd, kind) = { let a = app.read(cx); (a.home_cwd.clone(), a.home_kind) };
                let tab = app.update(cx, |a, cx| a.fresh_in(cwd, kind, mode, cx));
                // a design shows its preview from the start
                if kind == Kind::Design { super::panes::open(super::panes::Pane::Preview, cx); }
                window.defer(cx, move |window, cx| composer::dispatch(tab, text, images, How::Auto, window, cx));
            }),
            cx.observe(&app, |_, _, cx| cx.notify()),
        ];
        Home { composer, _subs: subs }
    }

    pub fn composer(&self) -> Entity<Composer> { self.composer.clone() }

    pub fn focus(this: &Entity<Self>, window: &mut Window, cx: &mut App) { let f = this.read(cx).composer.read(cx).focus_handle(cx); f.focus(window, cx); }

    fn browse(composer: Entity<Composer>, window: &mut Window, cx: &mut App) {
        let app = Pith::global(cx);
        let rx = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("open".into()) });
        let w = window.window_handle();
        cx.spawn(async move |cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let Some(p) = paths.into_iter().next() else { return };
            let _ = app.update(cx, |a, cx| { a.home_cwd = p.display().to_string(); cx.notify(); });
            let _ = w.update(cx, |_, window, cx| { let f = composer.read(cx).focus_handle(cx); f.focus(window, cx); });
        }).detach();
    }

    fn set_kind(k: Kind, cx: &mut App) {
        Pith::global(cx).update(cx, |a, cx| {
            a.home_kind = k;
            // designs have a home of their own, unless a folder was picked on purpose
            let default = a.settings["defaultCwd"].as_str().map(String::from).unwrap_or_else(|| crate::core::sys::home().display().to_string());
            if let Some(d) = a.settings["designDir"].as_str().filter(|d| !d.is_empty()) {
                if k == Kind::Design && a.home_cwd == default { a.home_cwd = d.to_string(); }
                if k == Kind::Chat && a.home_cwd == d { a.home_cwd = default; }
            }
            cx.notify();
        });
    }

    fn fill(composer: &Entity<Composer>, text: &str, window: &mut Window, cx: &mut App) {
        composer.update(cx, |c, cx| c.set_text(text, window, cx));
        let f = composer.read(cx).focus_handle(cx);
        f.focus(window, cx);
    }
}

// lib/fmt.ts base()
fn base(d: &str) -> String { d.trim_end_matches('/').rsplit('/').next().filter(|s| !s.is_empty()).map(String::from).unwrap_or_else(|| d.to_string()) }

// .h-sec: a small label with a dashed rule after it
fn section(label: &str, cx: &App) -> Div {
    let (p, g) = (pal(cx), geo(cx));
    h_flex().items_center().gap(u(10.)).mb(u(8.)).text_size(u(g.fs * 0.84)).text_color(p.dim)
        .child(label.to_string()).child(dashed(p.line, g.scale).flex_1())
}

// a 1px dashed rule like webkit draws one (3 on, 3 off). gpui's dashed borders don't show on a 1px line
pub fn dashed(color: Hsla, scale: f32) -> Canvas<()> {
    canvas(|_, _, _| (), move |b, _, window, _| {
        let (on, step) = (px(3. * scale), px(6. * scale));
        let mut x = b.left();
        while x < b.right() {
            window.paint_quad(fill(Bounds::new(point(x, b.top()), size(on.min(b.right() - x), px(scale.max(1.)))), color));
            x += step;
        }
    }).h(px(scale.max(1.)))
}

// what each starter does: fill the box, or do something else first
#[derive(Clone, Copy)]
enum Start { Fill(&'static str), Design, Images }
const STARTS: [(&str, &str, Start); 6] = [
    ("/review", "review what changed for bugs", Start::Fill("/review")),
    ("/design", "make something visual, live preview", Start::Design),
    ("/image", "generate or edit an image", Start::Images),
    ("/explain", "how does a part of this work", Start::Fill("/explain ")),
    ("/init", "write an AGENTS.md for this repo", Start::Fill("/init")),
    ("/security-review", "look for holes in the pending changes", Start::Fill("/security-review")),
];

impl Render for Home {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (p, g) = (pal(cx), geo(cx));
        let app = Pith::global(cx);
        let a = app.read(cx);
        let cwd = a.home_cwd.clone();
        let kind = a.home_kind;
        // folders sessions were in, newest first
        let mut dirs: Vec<String> = vec![];
        for s in a.sessions.iter() { if !s.cwd.is_empty() && !dirs.contains(&s.cwd) { dirs.push(s.cwd.clone()); } if dirs.len() >= 8 { break; } }
        let recent: Vec<_> = a.sessions.iter().take(9).cloned().collect();
        let composer = self.composer.clone();
        let (hover, high, text, dim) = (p.hover, p.high, p.text, p.dim);

        // .h-top: the title and chat | design
        let seg = |id: &'static str, label: &'static str, k: Kind| {
            let on = kind == k;
            div().id(id).py(u(2.)).px(u(10.)).rounded(u(g.r2 - 2.)).cursor_pointer().text_color(if on { text } else { dim })
                .when(on, |d| d.bg(high)).when(!on, |d| d.hover(move |s| s.text_color(text)))
                .child(label).on_click(move |_, _, cx| Home::set_kind(k, cx))
        };
        let top = h_flex().items_center().justify_between().mb(u(18.))
            .child(h_flex().text_size(u(g.fs * 1.5)).font_weight(FontWeight::BOLD).text_color(p.text)
                .child(div().text_color(p.accent).child("❯")).child(" new ").child(if kind == Kind::Design { "design" } else { "session" }))
            .child(h_flex().border_1().border_color(p.line).rounded(u(g.r2)).p(u(2.)).gap(u(2.))
                .child(seg("k-chat", "chat", Kind::Chat)).child(seg("k-design", "design", Kind::Design)));

        // .h-where: in <cwd> [browse]
        let (c1, c2) = (composer.clone(), composer.clone());
        let where_ = h_flex().items_center().gap(u(8.)).mb(u(8.))
            .child(div().text_color(p.dim).child("in"))
            .child(div().id("h-cwd").py(u(1.)).px(u(8.)).rounded(u(5.)).bg(p.raised).text_size(u(g.fs * 1.05)).text_color(p.text).cursor_pointer()
                .hover(move |s| s.bg(high)).child(super::tools::tilde(&cwd))
                .on_click(move |_, window, cx| Home::browse(c1.clone(), window, cx)))
            .child(div().id("h-browse").flex().items_center().gap(u(6.)).py(u(3.)).px(u(9.)).rounded(u(g.r2)).border_1().border_color(p.line).text_size(u(g.fs * 0.88)).text_color(p.text).cursor_pointer()
                .hover(move |s| s.bg(high))
                .child(Icon::new(IconName::Folder).size(u(g.fs * 0.88 * 1.2))).child("browse")
                .on_click(move |_, window, cx| Home::browse(c2.clone(), window, cx)));

        // .h-folders: the folders sessions were in
        let folders = h_flex().flex_wrap().gap(u(6.)).mb(u(18.)).children(dirs.iter().enumerate().map(|(i, d)| {
            let on = *d == cwd;
            let (d1, d2, c) = (d.clone(), d.clone(), composer.clone());
            let on_c = mix(p.accent, 0.5, p.line);
            div().id(("fchip", i)).py(u(1.)).px(u(10.)).rounded(u(12.)).border_1().text_size(u(g.fs * 0.86)).cursor_pointer()
                .border_color(if on { on_c } else { p.line }).text_color(if on { p.accent } else { p.mid })
                .when(!on, |d| d.hover(move |s| s.text_color(text).border_color(dim)))
                .child(base(d))
                .tooltip(move |w, cx| Tooltip::new(d1.clone()).build(w, cx))
                .on_click(move |_, window, cx| {
                    Pith::global(cx).update(cx, |a, cx| { a.home_cwd = d2.clone(); cx.notify(); });
                    let f = c.read(cx).focus_handle(cx);
                    f.focus(window, cx);
                })
        }));

        // .h-cols: recent | start from
        let rows = if recent.is_empty() { vec![div().text_color(p.dim).child("nothing yet").into_any_element()] } else {
            recent.into_iter().map(|s| {
                let (a, f) = (app.clone(), s.file.clone());
                h_flex().id(SharedString::from(format!("recent-{}", s.file))).w_full().gap(u(10.)).py(u(3.)).px(u(8.)).rounded(u(5.)).cursor_pointer()
                    .hover(move |st| st.bg(hover))
                    .child(div().flex_1().min_w_0().truncate().text_color(p.text).child(session_title(&s)))
                    .child(div().flex_none().whitespace_nowrap().text_size(u(g.fs * 0.86)).text_color(p.dim).child(base(&s.cwd)))
                    .child(div().flex_none().w(u(g.fs * 0.86 * 0.62 * 4.)).whitespace_nowrap().text_right().text_size(u(g.fs * 0.86)).text_color(p.dim).child(ago(s.updated)))
                    .on_click(move |_, _, cx| a.update(cx, |a, cx| a.open(&f, cx)))
                    .into_any_element()
            }).collect()
        };
        let starts = STARTS.iter().enumerate().map(|(i, (cmd, desc, what))| {
            let (c, what) = (composer.clone(), *what);
            h_flex().id(("start", i)).w_full().items_start().gap(u(10.)).py(u(3.)).px(u(8.)).rounded(u(5.)).cursor_pointer()
                .hover(move |st| st.bg(hover))
                .child(div().flex_none().w(u(g.fs * 0.6 * 17.)).text_color(p.accent).child(*cmd))
                .child(div().flex_1().min_w_0().pt(u(g.fs * 0.05)).text_size(u(g.fs * 0.9)).text_color(p.dim).child(*desc))
                .on_click(move |_, window, cx| match what {
                    Start::Fill(t) => Home::fill(&c, t, window, cx),
                    Start::Design => { Home::set_kind(Kind::Design, cx); Home::fill(&c, "/design ", window, cx); }
                    Start::Images => Pith::global(cx).update(cx, |a, cx| a.show(View::Images, cx)),
                })
        });
        // one column on a narrow window (pages.css max-width 1100px)
        let wide = window.viewport_size().width >= px(1100. * g.scale);
        let col_l = v_flex().min_w_0().child(section("recent", cx)).children(rows);
        let col_r = v_flex().min_w_0().child(section("start from", cx)).children(starts);
        let cols = if wide {
            h_flex().items_start().gap(u(26.)).mt(u(30.)).child(col_l.flex_grow(1.2).flex_basis(relative(0.)).flex_shrink(1.))
                .child(col_r.flex_grow(1.).flex_basis(relative(0.)).flex_shrink(1.)).into_any_element()
        } else { v_flex().gap(u(26.)).mt(u(30.)).child(col_l).child(col_r).into_any_element() };

        div().id("home").size_full().overflow_y_scroll().flex().child(
            v_flex().w_full().max_w(u(860.)).m_auto().pt(u(40.)).px(u(28.)).pb(u(60.))
                .child(top)
                .child(where_)
                .when(!dirs.is_empty(), |d| d.child(folders))
                .when(dirs.is_empty(), |d| d.child(div().mb(u(18.))))
                .child(self.composer.clone())
                .child(cols),
        )
    }
}
