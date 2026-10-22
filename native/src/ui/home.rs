// Where a new session starts: pick a folder, say what to do. recent sessions below for picking one back up
use super::chat::MAX_W;
use super::composer::{Composer, ComposerEvent};
use super::theme::pal;
use super::workspace::{ago, session_title};
use crate::state::app::Pith;
use crate::state::tab::{How, Kind};
use gpui_kit::component::button::{Button, ButtonGroup, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Icon, Selectable, Side, Sizable};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::assets::IconName;

pub struct Home {
    composer: Entity<Composer>,
    _subs: Vec<Subscription>,
}

// a few ways in, each fills the box rather than sending, so there's room to add to it
const STARTS: [(&str, &str, &str, IconName); 4] = [
    ("Review changes", "/review ", "Look over what changed for bugs", IconName::GitCompare),
    ("Explain", "/explain ", "How a part of this works", IconName::BookOpen),
    ("Write AGENTS.md", "/init", "Notes for future sessions", IconName::FileText),
    ("Simplify", "/simplify ", "Clean up the changed code", IconName::Sparkles),
];

impl Home {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let composer = cx.new(|cx| Composer::new(None, window, cx));
        let app = Pith::global(cx);
        let subs = vec![
            cx.subscribe_in(&composer, window, |_, _, ev: &ComposerEvent, _, cx| {
                let ComposerEvent::Submit { text, images, how } = ev else { return };
                let (text, images, how) = (text.clone(), images.clone(), *how);
                let app = Pith::global(cx);
                let (cwd, kind) = { let a = app.read(cx); (a.home_cwd.clone(), a.home_kind) };
                let tab = app.update(cx, |a, cx| a.fresh(cwd, kind, cx));
                tab.update(cx, |t, cx| t.send(text, images, if how == How::FollowUp { How::Auto } else { how }, cx));
            }),
            cx.observe(&app, |_, _, cx| cx.notify()),
        ];
        Home { composer, _subs: subs }
    }

    pub fn composer(&self) -> Entity<Composer> { self.composer.clone() }

    pub fn focus(this: &Entity<Self>, window: &mut Window, cx: &mut App) { let f = this.read(cx).composer.read(cx).focus_handle(cx); f.focus(window, cx); }

    fn pick_dir(composer: Entity<Composer>, window: &mut Window, cx: &mut App) {
        let app = Pith::global(cx);
        let rx = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Open".into()) });
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
}

fn base(d: &str) -> String {
    std::path::Path::new(d).file_name().map(|n| n.to_string_lossy().to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| super::tools::tilde(d))
}

impl Render for Home {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = pal(cx);
        let app = Pith::global(cx);
        let a = app.read(cx);
        let cwd = a.home_cwd.clone();
        let kind = a.home_kind;
        // folders you've worked in lately, for the folder menu
        let mut dirs: Vec<String> = vec![cwd.clone()];
        for s in a.sessions.iter() { if !s.cwd.is_empty() && !dirs.contains(&s.cwd) { dirs.push(s.cwd.clone()); } if dirs.len() >= 8 { break; } }
        let recent: Vec<_> = a.sessions.iter().take(8).cloned().collect();
        let composer = self.composer.clone();

        let (c1, c2) = (composer.clone(), composer.clone());
        let cur = cwd.clone();
        let folder = Button::new("cwd").label(super::tools::tilde(&cwd)).icon(IconName::Folder).ghost().small().dropdown_caret(true)
            .dropdown_menu_with_anchor(Anchor::TopLeft, move |menu, _, cx| {
                let p = pal(cx);
                let mut menu = menu.check_side(Side::Right).min_w(px(300.)).max_w(px(460.)).label("Recent folders");
                for d in &dirs {
                    let (d1, d2, c) = (d.clone(), d.clone(), c1.clone());
                    let name = base(d);
                    menu = menu.item(PopupMenuItem::element(move |_, cx| {
                        h_flex().gap_2().items_center().min_w_0()
                            .child(div().flex_none().text_sm().text_color(cx.theme().foreground).child(name.clone()))
                            .child(div().min_w_0().truncate().text_xs().text_color(p.dim).child(super::tools::tilde(&d1)))
                    }).checked(*d == cur).on_click(move |_, window, cx| {
                        Pith::global(cx).update(cx, |a, cx| { a.home_cwd = d2.clone(); cx.notify(); });
                        let f = c.read(cx).focus_handle(cx);
                        f.focus(window, cx);
                    }));
                }
                let c = c1.clone();
                menu.separator().item(PopupMenuItem::new("Choose folder…").icon(Icon::new(IconName::FolderOpen)).on_click(move |_, window, cx| Home::pick_dir(c.clone(), window, cx)))
            });

        div().id("home").size_full().overflow_y_scroll().child(
            h_flex().w_full().justify_center().px_6().pt(px(112.)).pb_8().child(
                v_flex().w_full().max_w(px(MAX_W)).gap_3()
                    .child(div().text_size(px(26.)).font_weight(FontWeight::SEMIBOLD).text_color(cx.theme().foreground).mb_2()
                        .child(if kind == Kind::Design { "What should we design?" } else { "What should we work on?" }))
                    .child(
                        h_flex().gap_2().items_center().ml(px(-8.))
                            .child(folder)
                            .child(div().flex_1())
                            .child(ButtonGroup::new("kind").small().outline()
                                .child(Button::new("k-chat").label("Chat").icon(IconName::MessageSquare).selected(kind == Kind::Chat))
                                .child(Button::new("k-design").label("Design").icon(IconName::Palette).selected(kind == Kind::Design))
                                .on_click(move |sel: &Vec<usize>, _, cx| Home::set_kind(if sel.first() == Some(&1) { Kind::Design } else { Kind::Chat }, cx))),
                    )
                    .child(self.composer.clone())
                    .child(h_flex().gap_1().flex_wrap().ml(px(-8.)).children(STARTS.iter().enumerate().map(|(i, (label, fill, hint, icon))| {
                        let c = c2.clone();
                        Button::new(("start", i)).label(*label).icon(*icon).ghost().small().tooltip(*hint).on_click(move |_, window, cx| {
                            c.update(cx, |c, cx| c.set_text(fill, window, cx));
                            let f = c.read(cx).focus_handle(cx);
                            f.focus(window, cx);
                        })
                    })))
                    .when(!recent.is_empty(), |d| d.child(
                        v_flex().mt_8()
                            .child(div().text_xs().font_weight(FontWeight::MEDIUM).text_color(p.dim).pb_1p5().child("Recent sessions"))
                            .children(recent.into_iter().map(|s| {
                                let (a, f) = (app.clone(), s.file.clone());
                                h_flex().id(SharedString::from(format!("recent-{}", s.file))).h(px(36.)).px_2().mx(px(-8.)).gap_3().items_center().rounded(cx.theme().radius)
                                    .hover(|st| st.bg(cx.theme().muted))
                                    .child(Icon::new(IconName::MessageSquare).small().text_color(p.dim))
                                    .child(div().flex_1().min_w_0().truncate().text_sm().text_color(cx.theme().foreground).child(session_title(&s)))
                                    .child(div().flex_none().max_w(px(200.)).truncate().text_xs().text_color(p.dim).child(base(&s.cwd)))
                                    .child(div().flex_none().w(px(28.)).text_right().text_xs().text_color(p.dim).child(ago(s.updated)))
                                    .on_click(move |_, _, cx| a.update(cx, |a, cx| a.open(&f, cx)))
                            })),
                    )),
            ),
        )
    }
}
