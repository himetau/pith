// Settings, saved as you change them (~/.config/pith/settings.json), laid out as settings.ts does: bordered groups with
// their title in the border, a row per field with the label and a dim hint on the left and the control on the right
use super::page::{self, head, kbd, s_in, segs, toggle, wide};
use super::theme::{em, geo, pal, u};
use super::tools::tilde;
use crate::core::sys;
use crate::state::app::Pith;
use crate::state::tab::Mode;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;
use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq)]
enum Kind { Text, Dir, Num(f64, f64), Money }

// key and kind of every typed field. "budget.x" keys live inside the budget object
const FIELDS: &[(&str, Kind)] = &[
    ("piPath", Kind::Text), ("extraArgs", Kind::Text), ("maxLive", Kind::Num(1., 32.)),
    ("defaultCwd", Kind::Dir), ("designDir", Kind::Dir), ("browserHome", Kind::Text),
    ("font", Kind::Text), ("proseFont", Kind::Text), ("fontSize", Kind::Num(10., 22.)),
    ("budget.window5h", Kind::Money), ("budget.week", Kind::Money), ("budget.month", Kind::Money),
    ("imageProvider", Kind::Text), ("imageModel", Kind::Text), ("imageCost", Kind::Money),
];
// look.ts SCALES
const SCALES: [f64; 8] = [0.9, 1., 1.1, 1.25, 1.4, 1.5, 1.75, 2.];

fn kind(key: &str) -> Kind { FIELDS.iter().find(|f| f.0 == key).map(|f| f.1).unwrap_or(Kind::Text) }

fn get<'a>(s: &'a Value, key: &str) -> &'a Value {
    match key.split_once('.') { Some((a, b)) => &s[a][b], None => &s[key] }
}

fn show(v: &Value) -> String {
    match v { Value::String(x) => x.clone(), Value::Number(n) => n.to_string(), _ => String::new() }
}

pub struct SettingsPage {
    fields: HashMap<&'static str, Entity<InputState>>,
    pi_version: String,
    _subs: Vec<Subscription>,
}

impl SettingsPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let s = Pith::global(cx).read(cx).settings.clone();
        let term = sys::terminal_font()["family"].as_str().map(String::from);
        let mut fields = HashMap::new();
        let mut subs = vec![];
        for &(key, _) in FIELDS {
            let placeholder = match key {
                "extraArgs" => "--flag value".to_string(),
                "font" => term.clone().unwrap_or_else(|| "monospace".into()),
                "proseFont" => "same as above".into(),
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
        // the focus border follows the fields
        for f in fields.values() { subs.push(cx.observe(f, |_, _, cx| cx.notify())); }
        let mut page = SettingsPage { fields, pi_version: String::new(), _subs: subs };
        page.check_pi(cx);
        page
    }

    // `pi --version` takes a few hundred ms, so it runs on a worker
    fn check_pi(&mut self, cx: &mut Context<Self>) {
        let bin = sys::setting_str("piPath");
        let bin = if bin.is_empty() { "pi".to_string() } else { bin };
        cx.spawn(async move |this, cx| {
            let v = cx.background_spawn(async move { sys::pi_version(&bin) }).await;
            let _ = this.update(cx, |this, cx| { this.pi_version = v; cx.notify(); });
        }).detach();
    }

    fn save(&mut self, patch: Value, window: &mut Window, cx: &mut Context<Self>) {
        let look = patch.as_object().is_some_and(|o| o.keys().any(|k| matches!(k.as_str(), "font" | "proseFont" | "fontSize" | "scale")));
        Pith::global(cx).update(cx, |a, cx| a.save_settings(patch, cx));
        // like applyLook() after every save: fonts, size and scale take straight away
        if look { super::theme::apply(Some(window), cx); }
        cx.notify();
    }

    fn commit(&mut self, key: &'static str, raw: String, window: &mut Window, cx: &mut Context<Self>) {
        let num = || raw.trim_start_matches('$').parse::<f64>().ok().filter(|v| v.is_finite());
        let val = match kind(key) {
            Kind::Num(lo, hi) => json!((num().unwrap_or(lo).clamp(lo, hi)).round() as i64),
            Kind::Money => json!(num().unwrap_or(0.).max(0.)),
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
        self.save(patch, window, cx);
        if key == "defaultCwd" { if let Some(d) = app.read(cx).settings["defaultCwd"].as_str().map(String::from) { app.update(cx, |a, cx| { a.home_cwd = d; cx.notify(); }); } }
        if key == "piPath" { self.check_pi(cx); }
        if key == "maxLive" { app.update(cx, |a, cx| a.trim(cx)); }
    }

    fn pick_dir(&mut self, key: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("choose".into()) });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let Some(p) = paths.into_iter().next() else { return };
            let _ = this.update_in(cx, |this, window, cx| this.commit(key, p.display().to_string(), window, cx));
        }).detach();
    }

    // a number field's up and down arrows, like the webview's
    fn step(&mut self, key: &'static str, dir: f64, window: &mut Window, cx: &mut Context<Self>) {
        let st = if kind(key) == Kind::Money { 0.5 } else { 1. };
        let cur = self.fields[key].read(cx).value().trim().trim_start_matches('$').parse::<f64>().unwrap_or(0.);
        let v = cur + dir * st;
        self.commit(key, format!("{v}"), window, cx);
    }

    fn control(&self, key: &'static str, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let p = pal(cx);
        let input = &self.fields[key];
        match kind(key) {
            Kind::Text => s_in(input, window, cx).into_any_element(),
            Kind::Dir => div().flex().gap(u(4.)).w_full().items_center()
                .child(div().flex_1().min_w_0().child(s_in(input, window, cx)))
                .child(pb(SharedString::from(format!("pick-{key}")), "folder", cx).on_click(cx.listener(move |this, _, window, cx| this.pick_dir(key, window, cx))))
                .into_any_element(),
            Kind::Num(..) | Kind::Money => {
                let (text, hover) = (p.text, p.hover);
                let arrow = |id: String, icon: IconName, dir: f64, cx: &mut Context<Self>| div().id(SharedString::from(id)).flex_1().flex().items_center().justify_center().px(u(2.)).rounded(u(2.))
                    .text_color(p.dim).hover(move |s| s.text_color(text).bg(hover)).cursor_pointer()
                    .child(Icon::new(icon).size(u(10.)))
                    .on_click(cx.listener(move |this, _, window, cx| this.step(key, dir, window, cx)));
                let field = div().w(u(110.)).flex_none().relative()
                    .child(s_in(input, window, cx).pr(u(24.)))
                    .child(div().absolute().top(u(5.)).bottom(u(5.)).right(u(6.)).flex().flex_col()
                        .child(arrow(format!("up-{key}"), IconName::ChevronUp, 1., cx)).child(arrow(format!("dn-{key}"), IconName::ChevronDown, -1., cx)));
                if kind(key) == Kind::Money {
                    div().flex().items_center().gap(u(6.)).child(div().text_color(p.dim).child("$")).child(field).into_any_element()
                } else { field.into_any_element() }
            }
        }
    }
}

// .s-group: a bordered box with its title in the border
fn group(title: &str, kids: Vec<AnyElement>, cx: &App) -> AnyElement {
    let body = div().pt(u(16.)).px(u(16.)).pb(u(8.)).children(kids);
    wide().mb(u(22.)).child(page::legend_box(title.to_string(), body, cx)).into_any_element()
}

// .s-field: label and hint on the left, the control on the right
fn field(label: &str, hint: &str, first: bool, ctl: impl IntoElement, cx: &App) -> AnyElement {
    let p = pal(cx);
    div().flex().gap(u(18.)).py(u(8.)).items_center().when(!first, |d| d.border_t(u(1.)).border_color(p.rule))
        .child(div().flex_basis(relative(0.)).flex_grow(1.).min_w_0()
            .child(div().child(label.to_string()))
            .when(!hint.is_empty(), |d| d.child(div().text_size(em(cx, 0.84)).text_color(p.dim).child(hint.to_string()))))
        .child(div().flex_basis(relative(0.)).flex_grow(1.1).min_w_0().flex().justify_end().child(ctl))
        .into_any_element()
}

// .pb: a small icon button beside a field
fn pb(id: impl Into<ElementId>, icon: &str, cx: &App) -> Stateful<Div> {
    let p = pal(cx);
    let (text, hover) = (p.text, p.hover);
    div().id(id).flex().flex_none().items_center().h(u(26.)).px(u(6.)).rounded(u(5.)).text_color(p.dim).cursor_pointer()
        .hover(move |s| s.text_color(text).bg(hover)).child(page::ic(icon, cx))
}

fn mono(text: impl Into<SharedString>) -> Div { div().min_w_0().whitespace_nowrap().overflow_hidden().text_ellipsis_start().child(text.into()) }

const KEYS: [(&str, &str); 16] = [
    ("ctrl+n", "new session"), ("ctrl+k", "commands"), ("ctrl+p", "find a session"), ("ctrl+b", "sidebar"), ("ctrl+= / ctrl+-", "scale"), ("ctrl+\\", "side window"),
    ("alt+1…6", "web · preview · changes · files · agents · terminal"), ("shift+tab", "cycle mode"), ("ctrl+m", "model"), ("ctrl+e", "effort"),
    ("esc", "stop the run"), ("enter while running", "steer"), ("alt+enter", "queue for after"), ("↑", "last prompt"), ("!cmd", "run in shell, output goes to context"), ("!!cmd", "shell, kept out of context"),
];

impl Render for SettingsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (p, g) = (pal(cx), geo(cx));
        let s = Pith::global(cx).read(cx).settings.clone();
        let this = cx.entity().downgrade();
        let f = |this: &Self, label: &str, hint: &str, first: bool, key: &'static str, window: &Window, cx: &mut Context<Self>| { let c = this.control(key, window, cx); field(label, hint, first, c, cx) };

        let pi = group("pi", vec![
            f(self, "pi binary", "what pith runs for every session", true, "piPath", window, cx),
            field("version", "", false, mono(self.pi_version.clone()), cx),
            f(self, "extra arguments", "added to every pi it starts, like --no-skills", false, "extraArgs", window, cx),
            f(self, "open sessions", "idle sessions past this give their process back until you open them again", false, "maxLive", window, cx),
        ], cx);

        let mode = Mode::parse(s["mode"].as_str().unwrap_or("ask"));
        let mi = Mode::ALL.iter().position(|m| *m == mode).unwrap_or(0);
        let t1 = this.clone();
        let modes = segs("mode", &["ask", "edits", "plan", "auto"], mi, move |i, w, cx| { let _ = t1.update(cx, |t, cx| t.save(json!({ "mode": Mode::ALL[i].as_str() }), w, cx)); }, cx);
        let t2 = this.clone();
        let notify = toggle("notify", s["notify"].as_bool().unwrap_or(true), move |v, w, cx| { let _ = t2.update(cx, |t, cx| t.save(json!({ "notify": v }), w, cx)); }, cx);
        let defaults = group("defaults", vec![
            field("mode for new sessions", "", true, modes, cx),
            f(self, "default folder", "where new sessions start", false, "defaultCwd", window, cx),
            f(self, "design folder", "where new design sessions start", false, "designDir", window, cx),
            f(self, "browser home", "first page in a session's browser pane", false, "browserHome", window, cx),
            field("notifications", "when a session in the background finishes or needs an answer", false, notify, cx),
        ], cx);

        let theme = sys::theme_file();
        let term = sys::terminal_font()["family"].as_str().map(String::from);
        let scale = s["scale"].as_f64().filter(|v| *v > 0.).unwrap_or(1.);
        let labels: Vec<String> = SCALES.iter().map(|x| format!("{}%", (x * 100.).round())).collect();
        let label_refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
        let si = SCALES.iter().position(|x| (x - scale).abs() < 0.001).unwrap_or(usize::MAX);
        let t3 = this.clone();
        let scales = segs("scale", &label_refs, si, move |i, w, cx| { let _ = t3.update(cx, |t, cx| t.save(json!({ "scale": SCALES[i] }), w, cx)); }, cx);
        let look = group("look", vec![
            field("theme", if p.found { "from matugen, follows your wallpaper live" } else { "matugen hasn't written it yet. run your wallpaper script once" }, true, mono(tilde(&theme.display().to_string())), cx),
            f(self, "font", &format!("empty uses your terminal's{}", term.map(|t| format!(" ({t})")).unwrap_or_default()), false, "font", window, cx),
            f(self, "reading font", "for replies. empty keeps them in the same font", false, "proseFont", window, cx),
            field("scale", "everything in the window. ctrl+= and ctrl+- step it, ctrl+0 resets", false, scales, cx),
            f(self, "text size", "", false, "fontSize", window, cx),
            field("windows", "gaps, borders and corners come from hyprland", false,
                mono(format!("gap {} · border {} · rounding {}", g.gap / 2., g.bw, g.round)), cx),
        ], cx);

        let budgets = group("budgets", vec![
            page::note("pi doesn't know your plan's limits, so these are yours. the usage page measures spend against them. 0 turns one off.", cx).mt(u(0.)).into_any_element(),
            f(self, "5 hour window", "", true, "budget.window5h", window, cx),
            f(self, "7 days", "", false, "budget.week", window, cx),
            f(self, "month", "", false, "budget.month", window, cx),
        ], cx);

        let dir = sys::images_dir();
        let d2 = dir.clone();
        let images = group("images", vec![
            f(self, "provider", "a provider from pi's models.json or auth", true, "imageProvider", window, cx),
            f(self, "model", "any model that answers with images, like gemini-3.1-flash-image or gemini-3-pro-image", false, "imageModel", window, cx),
            f(self, "price per image", "your endpoint doesn't report image prices, so usage counts this for each one. 0 leaves it out", false, "imageCost", window, cx),
            field("library", "", false, page::link("im-lib", tilde(&dir.display().to_string()), cx).on_click(move |_, _, cx| { let _ = std::fs::create_dir_all(&d2); cx.open_with_system(&d2); }), cx),
        ], cx);

        let ch = g.fs * 0.9 * 0.78 * 0.6;
        let keys = group("keys", vec![
            div().grid().grid_cols(2).gap_y(u(4.)).gap_x(u(20.)).pt(u(4.)).pb(u(8.))
                .children(KEYS.iter().map(|(k, v)| div().flex().gap(u(10.)).items_center().text_size(em(cx, 0.9))
                    .child(kbd(*k, cx).text_size(em(cx, 0.9 * 0.78)).min_w(u(11. * ch)).flex().justify_center()).child(div().child(*v))))
                .into_any_element(),
        ], cx);

        let file = tilde(&sys::config_dir().join("settings.json").display().to_string());
        page::page("settings", vec![head("settings", Some(file.into()), vec![], cx).into_any_element(), pi, defaults, look, budgets, images, keys])
    }
}
