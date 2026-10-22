// The box you type into. Enter sends, shift+enter is a new line. while pi works, Enter steers and alt+enter queues a follow up.
// the footer holds what the next message runs with: mode, model, thinking, plus how full the context is
use super::theme::pal;
use crate::state::app::Pith;
use crate::state::tab::{How, Img, Mode, Status, Tab};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::assets::IconName;

pub enum ComposerEvent {
    Submit { text: String, images: Vec<Img>, how: How },
    Stop,
}
impl EventEmitter<ComposerEvent> for Composer {}

pub struct Composer {
    pub tab: Option<Entity<Tab>>,
    input: Entity<TextareaState>,
    pub images: Vec<Img>,
    _subs: Vec<Subscription>,
}

impl Composer {
    pub fn new(tab: Option<Entity<Tab>>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let placeholder = if tab.is_some() { "Reply to pi…" } else { "What should we work on?" };
        let input = cx.new(|cx| TextareaState::new(window, cx).auto_grow(1, 14).submit_on_enter(true).placeholder(placeholder));
        let mut subs = vec![cx.subscribe_in(&input, window, |this, _, ev: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { secondary, shift } = ev {
                if *shift { return; }
                this.submit(if *secondary { How::FollowUp } else { How::Auto }, window, cx);
            }
        })];
        if let Some(t) = &tab { subs.push(cx.observe(t, |_, _, cx| cx.notify())); }
        Composer { tab, input, images: vec![], _subs: subs }
    }

    pub fn focus_handle(&self, cx: &App) -> FocusHandle { self.input.read(cx).focus_handle(cx) }

    pub fn set_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |s, cx| s.set_value(text.to_string(), window, cx));
    }

    pub fn text(&self, cx: &App) -> String { self.input.read(cx).value().to_string() }

    fn submit(&mut self, how: How, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.text(cx).trim().to_string();
        if text.is_empty() && self.images.is_empty() { return; }
        let images = std::mem::take(&mut self.images);
        self.input.update(cx, |s, cx| s.set_value("", window, cx));
        cx.emit(ComposerEvent::Submit { text, images, how });
        cx.notify();
    }
}

fn chip(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    Button::new(id).label(label).ghost().xsmall()
}

impl Render for Composer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = pal(cx);
        let focused = self.input.read(cx).focus_handle(cx).is_focused(window);
        let tab = self.tab.clone();
        let (running, mode, model, level, ctx, cost, queued) = match &tab {
            Some(t) => {
                let t = t.read(cx);
                (t.running(), t.mode, t.model_label(), t.state["thinkingLevel"].as_str().map(String::from), t.context_percent(), t.cost(), t.queue.0.len() + t.queue.1.len())
            }
            None => (false, Pith::global(cx).read(cx).mode_default(), None, None, None, 0.0, 0),
        };
        let empty = self.text(cx).trim().is_empty();
        let border = if focused { cx.theme().ring } else { cx.theme().border };

        let mut footer = h_flex().gap_1().items_center().px_2().pb_2();
        // mode cycles on click: the four of them fit in your head, a menu would be slower
        let t1 = tab.clone();
        footer = footer.child(chip("mode", match mode { Mode::Ask => "Ask", Mode::Edits => "Edits", Mode::Plan => "Plan", Mode::Auto => "Auto" })
            .icon(match mode { Mode::Ask => IconName::ShieldQuestionMark, Mode::Edits => IconName::FilePen, Mode::Plan => IconName::NotebookPen, Mode::Auto => IconName::Zap })
            .tooltip(mode.hint())
            .on_click(move |_, _, cx| {
                let next = Mode::ALL[(Mode::ALL.iter().position(|m| *m == mode).unwrap_or(0) + 1) % 4];
                match &t1 {
                    Some(t) => t.update(cx, |t, cx| t.set_mode(next, cx)),
                    None => Pith::global(cx).update(cx, |a, cx| a.save_settings(serde_json::json!({ "mode": next.as_str() }), cx)),
                }
            }));
        if let Some(m) = model {
            footer = footer.child(chip("model", m).on_click(|_, window, cx| super::picker::models(window, cx)));
        }
        if let (Some(l), Some(t)) = (level, tab.clone()) {
            footer = footer.child(chip("think", if l == "off" { "No thinking".to_string() } else { format!("Thinking: {l}") }).on_click(move |_, _, cx| {
                t.update(cx, |t, cx| {
                    let levels = t.levels.clone();
                    if levels.is_empty() { return; }
                    let cur = t.state["thinkingLevel"].as_str().unwrap_or("").to_string();
                    let next = levels[(levels.iter().position(|x| *x == cur).map(|i| i + 1).unwrap_or(0)) % levels.len()].clone();
                    t.set_thinking(next, cx);
                })
            }));
        }
        footer = footer.child(div().flex_1());
        if queued > 0 { footer = footer.child(div().text_xs().text_color(p.dim).mr_2().child(format!("{queued} queued"))); }
        if let Some(pct) = ctx {
            footer = footer.child(div().text_xs().text_color(if pct > 80. { p.gold } else { p.dim }).mr_2().child(format!("{:.0}% context", pct)));
        }
        if cost > 0.0 { footer = footer.child(div().text_xs().text_color(p.dim).mr_2().child(format!("${cost:.2}"))); }
        let starting = tab.as_ref().is_some_and(|t| t.read(cx).status == Status::Starting);
        footer = footer.child(if running && empty {
            Button::new("stop").icon(IconName::Square).small().rounded_full().tooltip("Stop").on_click(cx.listener(|_, _, _, cx| cx.emit(ComposerEvent::Stop)))
        } else {
            Button::new("send").icon(IconName::ArrowUp).small().primary().rounded_full().disabled(empty && self.images.is_empty() || starting && empty)
                .tooltip(if running { "Steer (Enter) · queue (Alt+Enter)" } else { "Send" })
                .on_click(cx.listener(|this, _, window, cx| this.submit(How::Auto, window, cx)))
        });

        v_flex().w_full().rounded(px(14.)).border_1().border_color(border).bg(p.raised).shadow_sm()
            .when(!self.images.is_empty(), |d| d.child(div().px_3().pt_2().text_xs().text_color(p.dim).child(format!("{} image(s) attached", self.images.len()))))
            .child(div().px_1().pt_1().child(Textarea::new(&self.input).appearance(false)))
            .child(footer)
    }
}
