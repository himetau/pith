// Settings: what pith runs and how it looks. every field saves as you leave it
use super::chat::MAX_W;
use super::theme::pal;
use crate::state::app::Pith;
use crate::state::tab::Mode;
use gpui_kit::component::button::{Button, ButtonGroup, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Selectable, Sizable};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;
use serde_json::{json, Value};

const TEXT: [(&str, &str, &str); 6] = [
    ("piPath", "pi command", "The pi binary pith runs for each session."),
    ("extraArgs", "Extra arguments", "Added to every pi it starts."),
    ("defaultCwd", "Default folder", "Where new sessions start."),
    ("uiFont", "Interface font", "Empty uses the system font."),
    ("font", "Code font", "Empty uses your terminal's font, then the platform's monospace."),
    ("textSize", "Text size", "In points."),
];

pub struct SettingsPage {
    fields: Vec<(&'static str, Entity<InputState>)>,
    _subs: Vec<Subscription>,
}

impl SettingsPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let s = Pith::global(cx).read(cx).settings.clone();
        let mut fields = vec![];
        let mut subs = vec![];
        for (key, _, _) in TEXT {
            let v = match &s[key] { Value::String(x) => x.clone(), Value::Number(n) => n.to_string(), _ => String::new() };
            let input = cx.new(|cx| InputState::new(window, cx).default_value(v));
            subs.push(cx.subscribe_in(&input, window, move |_, st, ev: &InputEvent, window, cx| {
                if !matches!(ev, InputEvent::Blur | InputEvent::PressEnter { .. }) { return; }
                let raw = st.read(cx).value().trim().to_string();
                let val = if key == "textSize" { json!(raw.parse::<f64>().unwrap_or(15.).clamp(10., 28.)) } else { json!(raw) };
                let app = Pith::global(cx);
                if app.read(cx).settings[key] == val { return; }
                app.update(cx, |a, cx| a.save_settings(json!({ key: val }), cx));
                if matches!(key, "uiFont" | "font" | "textSize") { super::theme::apply(Some(window), cx); }
            }));
            fields.push((key, input));
        }
        SettingsPage { fields, _subs: subs }
    }
}

impl Render for SettingsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = pal(cx);
        let app = Pith::global(cx);
        let s = app.read(cx).settings.clone();
        let mode = Mode::parse(s["mode"].as_str().unwrap_or("ask"));
        let notify = s["notify"].as_bool().unwrap_or(true);
        let row = |label: &'static str, hint: &'static str, ctl: AnyElement| {
            h_flex().gap_6().py_3().border_b_1().border_color(p.hairline).items_center()
                .child(v_flex().w(px(240.)).flex_none().gap_0p5().child(div().text_color(cx.theme().foreground).child(label)).child(div().text_xs().text_color(p.dim).child(hint)))
                .child(div().flex_1().min_w_0().child(ctl))
        };
        let mut col = v_flex().w_full().max_w(px(MAX_W)).pb_8();
        for (i, (key, input)) in self.fields.iter().enumerate() {
            let (_, label, hint) = TEXT[i];
            debug_assert_eq!(*key, TEXT[i].0);
            col = col.child(row(label, hint, Input::new(input).small().into_any_element()));
        }
        let (a1, a2) = (app.clone(), app.clone());
        col = col.child(row("Default mode", "What a new session may do without asking.", ButtonGroup::new("mode").small().outline()
            .children(Mode::ALL.iter().map(|m| Button::new(m.as_str()).label(match m { Mode::Ask => "Ask", Mode::Edits => "Edits", Mode::Plan => "Plan", Mode::Auto => "Auto" }).selected(*m == mode)))
            .on_click(move |sel: &Vec<usize>, _, cx| { if let Some(i) = sel.first() { a1.update(cx, |a, cx| a.save_settings(json!({ "mode": Mode::ALL[*i].as_str() }), cx)); } })
            .into_any_element()));
        col = col.child(row("Notifications", "When a session finishes while pith is in the background.", Switch::new("notify").checked(notify)
            .on_change(move |v: &bool, _, cx| a2.update(cx, |a, cx| a.save_settings(json!({ "notify": *v }), cx))).into_any_element()));
        div().id("settings").size_full().overflow_y_scroll().child(h_flex().w_full().justify_center().px_6().pt_6().child(col))
    }
}
