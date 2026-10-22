// Settings: what pith runs and how it looks. every field saves as you leave it or press enter
use super::page::{self, group, heading, row};
use super::theme::pal;
use super::tools::tilde;
use crate::core::sys;
use crate::state::app::Pith;
use crate::state::tab::Mode;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Sizable};
use gpui_kit::*;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq)]
enum Kind { Text, Dir, Int(i64, i64), Size, Money }

// key, label, help, kind. "budget.x" keys live inside the budget object
const FIELDS: &[(&str, &str, &str, Kind)] = &[
    ("piPath", "pi command", "The pi binary every session runs", Kind::Text),
    ("extraArgs", "Extra arguments", "Added to every pi pith starts, like --no-skills", Kind::Text),
    ("defaultCwd", "Default folder", "Where new sessions start", Kind::Dir),
    ("designDir", "Design folder", "Where new design sessions start", Kind::Dir),
    ("browserHome", "Browser home", "The first page in a session's browser pane", Kind::Text),
    ("maxLive", "Open sessions", "Idle sessions past this give their process back until you open them again", Kind::Int(1, 32)),
    ("uiFont", "Interface font", "Empty uses the system font", Kind::Text),
    ("font", "Code font", "Empty uses your terminal's font, then the platform's monospace", Kind::Text),
    ("textSize", "Text size", "In points, 10 to 28", Kind::Size),
    ("budget.window5h", "5 hour window", "", Kind::Money),
    ("budget.week", "7 days", "", Kind::Money),
    ("budget.month", "Month", "", Kind::Money),
    ("imageProvider", "Provider", "A provider from pi's models.json or auth", Kind::Text),
    ("imageModel", "Model", "Any model that answers with images, like gemini-3.1-flash-image", Kind::Text),
    ("imageCost", "Price per image", "Endpoints don't report image prices, so usage counts this for each one. 0 leaves it out", Kind::Money),
];

fn spec(key: &str) -> (&'static str, &'static str, &'static str, Kind) { *FIELDS.iter().find(|f| f.0 == key).unwrap() }

fn get<'a>(s: &'a Value, key: &str) -> &'a Value {
    match key.split_once('.') { Some((a, b)) => &s[a][b], None => &s[key] }
}

fn show(v: &Value) -> String {
    match v { Value::String(x) => x.clone(), Value::Number(n) => n.to_string(), _ => String::new() }
}

pub struct SettingsPage {
    fields: HashMap<&'static str, Entity<InputState>>,
    pi_version: Option<String>,
    _subs: Vec<Subscription>,
}

impl SettingsPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let s = Pith::global(cx).read(cx).settings.clone();
        let mono_default = sys::terminal_font()["family"].as_str().map(String::from);
        let mut fields = HashMap::new();
        let mut subs = vec![];
        for &(key, _, _, kind) in FIELDS {
            let placeholder = match key {
                "piPath" => "pi".to_string(),
                "extraArgs" => "--flag value".into(),
                "uiFont" => "System font".into(),
                "font" => mono_default.clone().unwrap_or_else(|| "Monospace".into()),
                "textSize" => "15".into(),
                _ if kind == Kind::Money => "0".into(),
                _ => String::new(),
            };
            let input = cx.new(|cx| InputState::new(window, cx).default_value(show(get(&s, key))).placeholder(placeholder));
            subs.push(cx.subscribe_in(&input, window, move |this, st, ev: &InputEvent, window, cx| {
                if !matches!(ev, InputEvent::Blur | InputEvent::PressEnter { .. }) { return; }
                let raw = st.read(cx).value().trim().to_string();
                this.commit(key, raw, window, cx);
            }));
            fields.insert(key, input);
        }
        let mut page = SettingsPage { fields, pi_version: None, _subs: subs };
        page.check_pi(cx);
        page
    }

    // `pi --version` takes a few hundred ms, so it runs on a worker
    fn check_pi(&mut self, cx: &mut Context<Self>) {
        self.pi_version = None;
        let bin = sys::setting_str("piPath");
        let bin = if bin.is_empty() { "pi".to_string() } else { bin };
        cx.spawn(async move |this, cx| {
            let v = cx.background_spawn(async move { sys::pi_version(&bin) }).await;
            let _ = this.update(cx, |this, cx| { this.pi_version = Some(v); cx.notify(); });
        }).detach();
        cx.notify();
    }

    fn commit(&mut self, key: &'static str, raw: String, window: &mut Window, cx: &mut Context<Self>) {
        let (_, _, _, kind) = spec(key);
        let val = match kind {
            Kind::Int(lo, hi) => json!(raw.parse::<f64>().map(|v| v.round() as i64).unwrap_or(6).clamp(lo, hi)),
            Kind::Size => json!(raw.parse::<f64>().unwrap_or(15.).clamp(10., 28.)),
            Kind::Money => json!(raw.trim_start_matches('$').parse::<f64>().unwrap_or(0.).max(0.)),
            Kind::Text | Kind::Dir => json!(if key == "piPath" && raw.is_empty() { "pi".to_string() } else { raw }),
        };
        // what the field shows is what got saved, so a clamp or a bad number is visible straight away
        let shown = show(&val);
        if let Some(input) = self.fields.get(key) {
            if input.read(cx).value().as_ref() != shown { input.update(cx, |s, cx| s.set_value(shown, window, cx)); }
        }
        let app = Pith::global(cx);
        let cur = app.read(cx).settings.clone();
        if *get(&cur, key) == val { return; }
        let patch = match key.split_once('.') {
            Some((obj, sub)) => { let mut o = cur[obj].clone(); if !o.is_object() { o = json!({}); } o[sub] = val; json!({ obj: o }) }
            None => json!({ key: val }),
        };
        app.update(cx, |a, cx| a.save_settings(patch, cx));
        if matches!(key, "uiFont" | "font" | "textSize") { super::theme::apply(Some(window), cx); }
        if key == "defaultCwd" { if let Some(d) = app.read(cx).settings["defaultCwd"].as_str().map(String::from) { app.update(cx, |a, cx| { a.home_cwd = d; cx.notify(); }); } }
        if key == "piPath" { self.check_pi(cx); }
        if key == "maxLive" { app.update(cx, |a, cx| a.trim(cx)); }
    }

    fn pick_dir(&mut self, key: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Choose".into()) });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let Some(p) = paths.into_iter().next() else { return };
            let _ = this.update_in(cx, |this, window, cx| this.commit(key, p.display().to_string(), window, cx));
        }).detach();
    }

    fn input(&self, key: &'static str, cx: &mut Context<Self>) -> AnyElement {
        let (_, _, _, kind) = spec(key);
        let input = &self.fields[key];
        match kind {
            Kind::Dir => h_flex().w_full().gap_1()
                .child(div().flex_1().min_w_0().child(Input::new(input).small()))
                .child(Button::new(SharedString::from(format!("pick-{key}"))).icon(IconName::FolderOpen).outline().small().tooltip("Choose a folder…")
                    .on_click(cx.listener(move |this, _, window, cx| this.pick_dir(key, window, cx))))
                .into_any_element(),
            Kind::Money => div().w(px(120.)).child(Input::new(input).small().prefix(div().text_color(pal(cx).dim).child("$"))).into_any_element(),
            Kind::Int(..) | Kind::Size => div().w(px(120.)).child(Input::new(input).small()).into_any_element(),
            Kind::Text => div().w_full().child(Input::new(input).small()).into_any_element(),
        }
    }

    fn field(&self, key: &'static str, cx: &mut Context<Self>) -> AnyElement {
        let (_, label, hint, _) = spec(key);
        let ctl = self.input(key, cx);
        row(label, hint, ctl, cx)
    }
}

fn cap(s: &str) -> String { let mut c = s.chars(); c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default() }

fn mono(text: impl Into<SharedString>, cx: &App) -> Div {
    div().min_w_0().font_family(cx.theme().mono_font_family.clone()).text_xs().text_color(pal(cx).mid).whitespace_nowrap().overflow_hidden().text_ellipsis_start().child(text.into())
}

fn reveal_button(id: &'static str, path: PathBuf) -> Button {
    Button::new(id).label("Show").ghost().small().on_click(move |_, _, cx| {
        // reveal wants something that exists, so fall back to the nearest folder that does
        let mut p = path.clone();
        while !p.exists() { if !p.pop() { break; } }
        cx.reveal_path(&p);
    })
}

const KEYS: &[(&str, &str)] = &[
    ("New session", "secondary-n"), ("Command palette", "secondary-k"), ("Switch model", "secondary-m"), ("Focus the composer", "secondary-l"),
    ("Toggle sidebar", "secondary-b"), ("Toggle side panel", "secondary-j"), ("Settings", "secondary-,"), ("Close session", "secondary-w"),
    ("Next session", "ctrl-tab"), ("Previous session", "ctrl-shift-tab"), ("Stop the run", "escape"),
];

impl Render for SettingsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = pal(cx);
        let app = Pith::global(cx);
        let s = app.read(cx).settings.clone();
        let mode = Mode::parse(s["mode"].as_str().unwrap_or("ask"));
        let notify = s["notify"].as_bool().unwrap_or(true);
        let (a1, a2) = (app.clone(), app.clone());

        let version: AnyElement = match &self.pi_version {
            None => h_flex().gap_2().items_center().text_xs().text_color(p.dim).child(Spinner::new().xsmall()).child("Checking…").into_any_element(),
            Some(v) if v == "not found" => div().text_xs().text_color(p.red).child("Not found. Check the pi command above").into_any_element(),
            Some(v) => mono(v.clone(), cx).into_any_element(),
        };
        let general = group(vec![
            self.field("piPath", cx),
            row("pi version", "", version, cx),
            self.field("extraArgs", cx),
            self.field("defaultCwd", cx),
            self.field("designDir", cx),
            self.field("browserHome", cx),
        ], cx);

        let mi = Mode::ALL.iter().position(|m| *m == mode).unwrap_or(0);
        let modes = page::segmented("mode", &["Ask", "Edits", "Plan", "Auto"], mi, move |i, _, cx| a1.update(cx, |a, cx| a.save_settings(json!({ "mode": Mode::ALL[i].as_str() }), cx)), cx);
        let sessions = group(vec![
            row("Default mode", cap(mode.hint()), modes, cx),
            self.field("maxLive", cx),
            row("Notifications", "When a session in the background finishes or needs an answer", Switch::new("notify").checked(notify)
                .on_change(move |v: &bool, _, cx| a2.update(cx, |a, cx| a.save_settings(json!({ "notify": *v }), cx))), cx),
        ], cx);

        let ai = match s["appearance"].as_str() { Some("light") => 1, Some("dark") => 2, _ => 0 };
        let a3 = app.clone();
        let looks = page::segmented("appearance", &["System", "Light", "Dark"], ai, move |i, window, cx| {
            a3.update(cx, |a, cx| a.save_settings(json!({ "appearance": (["system", "light", "dark"][i]) }), cx));
            super::theme::apply(Some(window), cx);
        }, cx);
        let mut look = vec![row("Appearance", "Light or dark, or whatever the system is set to", looks, cx), self.field("uiFont", cx), self.field("font", cx), self.field("textSize", cx)];
        if cfg!(target_os = "linux") {
            let theme = sys::theme_file();
            let found = theme.exists();
            look.push(row("Accent", if found { "From matugen, follows your wallpaper" } else { "The default blue. matugen hasn't written a colour file yet" },
                mono(tilde(&theme.display().to_string()), cx), cx));
        }
        let appearance = group(look, cx);

        let budget = group(vec![self.field("budget.window5h", cx), self.field("budget.week", cx), self.field("budget.month", cx)], cx);

        let images_dir = sys::images_dir();
        let images = group(vec![
            self.field("imageProvider", cx),
            self.field("imageModel", cx),
            self.field("imageCost", cx),
            row("Library", "Where generated images are kept", h_flex().gap_2().items_center().min_w_0()
                .child(mono(tilde(&images_dir.display().to_string()), cx)).child(reveal_button("reveal-images", images_dir.clone())), cx),
        ], cx);

        let cfg_file = sys::config_dir().join("settings.json");
        let files = group(vec![
            row("Settings file", "", h_flex().gap_2().items_center().min_w_0().child(mono(tilde(&cfg_file.display().to_string()), cx)).child(reveal_button("reveal-cfg", cfg_file.clone())), cx),
            row("pi's folder", "Sessions, agents and models are read from here", h_flex().gap_2().items_center().min_w_0()
                .child(mono(tilde(&sys::agent_dir().display().to_string()), cx)).child(reveal_button("reveal-agent", sys::agent_dir())), cx),
        ], cx);

        let keys = group(KEYS.iter().map(|(label, k)| {
            h_flex().px_4().py_2().items_center().text_sm()
                .child(div().flex_1().text_color(cx.theme().foreground).child(*label))
                .children(Keystroke::parse(k).ok().map(Kbd::new))
                .into_any_element()
        }).collect(), cx);

        page::column("settings", 760., v_flex()
            .child(heading("General", cx).mt_2()).child(general)
            .child(heading("Sessions", cx)).child(sessions)
            .child(heading("Appearance", cx)).child(appearance)
            .child(heading("Budget", cx))
            .child(div().mb_2().text_xs().text_color(p.dim).child("pi doesn't know your plan's limits, so these are yours. The usage page measures spend against them. 0 turns one off."))
            .child(budget)
            .child(heading("Images", cx)).child(images)
            .child(heading("Files", cx)).child(files)
            .child(heading("Keyboard", cx)).child(keys))
    }
}
