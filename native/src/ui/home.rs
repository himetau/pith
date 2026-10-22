// Where a new session starts: pick a folder, say what to do. recent sessions below for picking one back up
use super::chat::MAX_W;
use super::composer::{Composer, ComposerEvent};
use super::theme::pal;
use super::workspace::{ago, session_title};
use crate::state::app::Pith;
use crate::state::tab::{How, Kind};
use gpui_kit::component::button::{Button, ButtonGroup, ButtonVariants};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Selectable, Sizable};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::assets::IconName;

pub struct Home {
    composer: Entity<Composer>,
    _subs: Vec<Subscription>,
}

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

    pub fn focus(this: &Entity<Self>, window: &mut Window, cx: &mut App) { let f = this.read(cx).composer.read(cx).focus_handle(cx); f.focus(window, cx); }

    fn pick_dir(&mut self, cx: &mut Context<Self>) {
        let app = Pith::global(cx);
        let rx = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Open".into()) });
        cx.spawn(async move |_, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let Some(p) = paths.into_iter().next() else { return };
            let _ = app.update(cx, |a, cx| { a.home_cwd = p.display().to_string(); cx.notify(); });
        }).detach();
    }
}

impl Render for Home {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = pal(cx);
        let app = Pith::global(cx);
        let a = app.read(cx);
        let cwd = a.home_cwd.clone();
        let kind = a.home_kind;
        // folders you've worked in lately, one click to switch
        let mut recent_dirs: Vec<String> = vec![];
        for s in a.sessions.iter() { if !recent_dirs.contains(&s.cwd) && s.cwd != cwd { recent_dirs.push(s.cwd.clone()); } if recent_dirs.len() >= 4 { break; } }
        let recent: Vec<_> = a.sessions.iter().take(6).cloned().collect();
        let (a1, a2) = (app.clone(), app.clone());
        div().id("home").size_full().overflow_y_scroll().child(
            h_flex().w_full().justify_center().px_6().pt(px(96.)).pb_8().child(
                v_flex().w_full().max_w(px(MAX_W)).gap_4()
                    .child(div().text_size(px(28.)).font_weight(FontWeight::SEMIBOLD).text_color(cx.theme().foreground).child(if kind == Kind::Design { "What should we design?" } else { "What should we work on?" }))
                    .child(
                        h_flex().gap_2().items_center().flex_wrap()
                            .child(Button::new("cwd").label(super::tools::tilde(&cwd)).icon(IconName::Folder).outline().small().on_click(cx.listener(|this, _, _, cx| this.pick_dir(cx))))
                            .children(recent_dirs.into_iter().enumerate().map(|(i, d)| {
                                let a = app.clone();
                                let label = std::path::Path::new(&d).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| d.clone());
                                Button::new(("rdir", i)).label(label).ghost().small().tooltip(super::tools::tilde(&d)).on_click(move |_, _, cx| a.update(cx, |a, cx| { a.home_cwd = d.clone(); cx.notify(); }))
                            }))
                            .child(div().flex_1())
                            .child(ButtonGroup::new("kind").small().outline()
                                .child(Button::new("k-chat").label("Chat").selected(kind == Kind::Chat))
                                .child(Button::new("k-design").label("Design").selected(kind == Kind::Design))
                                .on_click(move |sel: &Vec<usize>, _, cx| { let k = if sel.first() == Some(&1) { Kind::Design } else { Kind::Chat }; a1.update(cx, |a, cx| { a.home_kind = k; cx.notify(); }) })),
                    )
                    .child(self.composer.clone())
                    .when(!recent.is_empty(), |d| d.child(
                        v_flex().mt_6().gap_1()
                            .child(div().text_xs().font_weight(FontWeight::MEDIUM).text_color(p.dim).mb_1().child("Recent"))
                            .children(recent.into_iter().map(|s| {
                                let (a, f) = (a2.clone(), s.file.clone());
                                h_flex().id(SharedString::from(format!("recent-{}", s.file))).h(px(34.)).px_2().mx(px(-8.)).gap_3().items_center().rounded(cx.theme().radius).cursor_pointer()
                                    .hover(|st| st.bg(cx.theme().muted))
                                    .child(div().flex_1().min_w_0().truncate().child(session_title(&s)))
                                    .child(div().flex_none().text_xs().text_color(p.dim).child(super::tools::tilde(&s.cwd)))
                                    .child(div().flex_none().w(px(32.)).text_right().text_xs().text_color(p.dim).child(ago(s.updated)))
                                    .on_click(move |_, _, cx| a.update(cx, |a, cx| a.open(&f, cx)))
                            })),
                    )),
            ),
        )
    }
}
