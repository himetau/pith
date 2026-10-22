// pith's look, the same as the webview build: matugen's palette, the terminal's mono font everywhere,
// window gaps, borders and corners off hyprland. matugen's file (~/.config/pith/colors.css) wins over the
// plain dark neutral below, and pith rethemes when it changes
use crate::core::sys;
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::*;
use std::collections::HashMap;

fn hex(s: &str) -> Option<Hsla> {
    let s = s.trim().trim_start_matches('#');
    let v = u32::from_str_radix(s.get(..6)?, 16).ok()?;
    Some(rgb(v).into())
}

// base.css's fallback for a machine without matugen
const DEFAULTS: &[(&str, &str)] = &[
    ("surface-lowest", "#0e0f11"), ("surface-low", "#16171a"), ("surface", "#131416"), ("surface-container", "#1b1c1f"), ("surface-high", "#24262a"),
    ("surface-highest", "#2e3035"), ("surface-bright", "#383a40"), ("outline", "#8b8f98"), ("outline-variant", "#3a3d44"), ("on-surface", "#e3e4e8"),
    ("on-surface-variant", "#c3c6ce"), ("primary", "#a9c7ff"), ("on-primary", "#08305f"), ("primary-container", "#224777"), ("on-primary-container", "#d6e3ff"),
    ("secondary", "#bdc7dc"), ("secondary-container", "#3e4759"), ("on-secondary-container", "#d9e3f8"), ("tertiary", "#dcbce1"), ("tertiary-container", "#573e5c"),
    ("on-tertiary-container", "#f9d8fe"), ("error", "#ffb4ab"), ("error-container", "#93000a"),
    ("red", "#f2838f"), ("green", "#87ee86"), ("gold", "#efc186"), ("orange", "#f09d84"), ("cyan", "#87edcd"), ("blue", "#8f86ef"), ("magenta", "#c785f0"),
];

// every `--name: #hex;` in matugen's file
fn matugen() -> (HashMap<String, Hsla>, bool) {
    let mut m: HashMap<String, Hsla> = DEFAULTS.iter().filter_map(|(k, v)| Some((k.to_string(), hex(v)?))).collect();
    let Ok(css) = std::fs::read_to_string(sys::theme_file()) else { return (m, false) };
    for line in css.lines() {
        let Some(rest) = line.trim().strip_prefix("--") else { continue };
        let Some((k, v)) = rest.split_once(':') else { continue };
        if let Some(c) = hex(v.trim().trim_end_matches(';')) { m.insert(k.trim().to_string(), c); }
    }
    (m, true)
}

// css color-mix(in srgb, a p, b)
pub fn mix(a: Hsla, p: f32, b: Hsla) -> Hsla {
    let (a, b) = (a.to_rgb(), b.to_rgb());
    let l = |x: f32, y: f32| x * p + y * (1. - p);
    Rgba { r: l(a.r, b.r), g: l(a.g, b.g), b: l(a.b, b.b), a: l(a.a, b.a) }.into()
}
// color-mix with transparent is the colour at that opacity
pub fn fade(a: Hsla, p: f32) -> Hsla { a.opacity(p) }

#[derive(Clone)]
pub struct Pal {
    // matugen's names
    pub surface_lowest: Hsla, pub surface_low: Hsla, pub surface: Hsla, pub surface_container: Hsla, pub surface_high: Hsla,
    pub surface_highest: Hsla, pub surface_bright: Hsla, pub outline: Hsla, pub outline_variant: Hsla, pub on_surface: Hsla,
    pub on_surface_variant: Hsla, pub primary: Hsla, pub on_primary: Hsla, pub primary_container: Hsla, pub on_primary_container: Hsla,
    pub secondary: Hsla, pub secondary_container: Hsla, pub on_secondary_container: Hsla, pub tertiary: Hsla, pub tertiary_container: Hsla,
    pub on_tertiary_container: Hsla, pub error: Hsla, pub error_container: Hsla,
    pub red: Hsla, pub green: Hsla, pub gold: Hsla, pub orange: Hsla, pub cyan: Hsla, pub blue: Hsla, pub magenta: Hsla,
    // roles, as base.css builds them
    pub gutter: Hsla, pub bg: Hsla, pub raised: Hsla, pub high: Hsla, pub line: Hsla, pub rule: Hsla,
    pub text: Hsla, pub mid: Hsla, pub dim: Hsla, pub accent: Hsla,
    pub md_head: Hsla, pub md_strong: Hsla, pub md_em: Hsla, pub md_code: Hsla, pub md_mark: Hsla,
    pub win_on: Hsla, pub hover: Hsla, pub sel: Hsla, pub selection: Hsla,
    pub add: Hsla, pub del: Hsla, pub add_bg: Hsla, pub del_bg: Hsla,
    // older names the native views used
    pub hairline: Hsla, pub code_bg: Hsla,
    pub found: bool,
}

impl Pal {
    fn build(m: &HashMap<String, Hsla>, found: bool) -> Pal {
        let c = |k: &str| m.get(k).copied().unwrap_or(hsla(0., 0., 0.5, 1.));
        let (on_surface, outline_variant, primary) = (c("on-surface"), c("outline-variant"), c("primary"));
        let (green, red, dim) = (c("green"), c("red"), c("outline"));
        Pal {
            surface_lowest: c("surface-lowest"), surface_low: c("surface-low"), surface: c("surface"), surface_container: c("surface-container"),
            surface_high: c("surface-high"), surface_highest: c("surface-highest"), surface_bright: c("surface-bright"), outline: dim,
            outline_variant, on_surface, on_surface_variant: c("on-surface-variant"), primary, on_primary: c("on-primary"),
            primary_container: c("primary-container"), on_primary_container: c("on-primary-container"), secondary: c("secondary"),
            secondary_container: c("secondary-container"), on_secondary_container: c("on-secondary-container"), tertiary: c("tertiary"),
            tertiary_container: c("tertiary-container"), on_tertiary_container: c("on-tertiary-container"), error: c("error"), error_container: c("error-container"),
            red, green, gold: c("gold"), orange: c("orange"), cyan: c("cyan"), blue: c("blue"), magenta: c("magenta"),
            gutter: c("surface-lowest"), bg: c("surface"), raised: c("surface-container"), high: c("surface-high"), line: outline_variant,
            rule: fade(outline_variant, 0.6), text: on_surface, mid: c("on-surface-variant"), dim, accent: primary,
            md_head: c("magenta"), md_strong: c("orange"), md_em: c("gold"), md_code: green, md_mark: c("blue"),
            win_on: mix(on_surface, 0.8, outline_variant), hover: fade(on_surface, 0.06), sel: fade(primary, 0.16), selection: fade(primary, 0.38),
            add: green, del: red, add_bg: fade(green, 0.11), del_bg: fade(red, 0.11),
            hairline: fade(outline_variant, 0.6), code_bg: c("surface-low"),
            found,
        }
    }
    // thinking and summaries keep the markdown colours, only hinted
    pub fn quiet(&self) -> Pal {
        let mut p = self.clone();
        let q = |c: Hsla| mix(c, 0.5, self.dim);
        p.md_head = q(self.magenta); p.md_strong = q(self.orange); p.md_em = q(self.gold); p.md_code = q(self.green); p.md_mark = q(self.blue);
        p
    }
}

// sizes, from hyprland and settings. css px go through u(), so the scale setting zooms everything like the webview did
#[derive(Clone)]
pub struct Geo { pub gap: f32, pub bw: f32, pub round: f32, pub r2: f32, pub fs: f32, pub scale: f32, pub side_w: f32, pub mono: SharedString, pub prose: SharedString, pub weight: FontWeight }

struct Look { pal: Pal, geo: Geo }
impl Global for Look {}

pub fn pal(cx: &App) -> Pal { cx.try_global::<Look>().map(|l| l.pal.clone()).unwrap_or_else(|| { let (m, f) = matugen(); Pal::build(&m, f) }) }
pub fn geo(cx: &App) -> Geo { cx.global::<Look>().geo.clone() }

// a css pixel at the current scale. the kit's Root sets the window's rem size to the theme's font size (fontSize * scale)
// every frame, so a css pixel is 1/fontSize of a rem
static BASE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x41600000); // 14.0
pub fn u(v: f32) -> Rems { rems(v / f32::from_bits(BASE.load(std::sync::atomic::Ordering::Relaxed))) }
// the body text size, and a fraction of it (css .9em)
pub fn fs(cx: &App) -> Rems { u(geo(cx).fs) }
pub fn em(cx: &App, f: f32) -> Rems { u(geo(cx).fs * f) }

// pane gaps, borders and corners match the compositor's windows
fn hypr() -> (f32, f32, f32) {
    let get = |opt: &str| -> Option<f32> {
        let out = std::process::Command::new("hyprctl").args(["-j", "getoption", opt]).output().ok()?;
        let j: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
        j.get("int").and_then(|v| v.as_f64())
            .or_else(|| j.get("custom").and_then(|v| v.as_str()).and_then(|s| s.split_whitespace().next()?.parse().ok()))
            .map(|v| v as f32)
    };
    thread_local! { static CACHE: std::cell::OnceCell<(f32, f32, f32)> = const { std::cell::OnceCell::new() }; }
    CACHE.with(|c| *c.get_or_init(|| (get("general:gaps_in").unwrap_or(5.), get("general:border_size").unwrap_or(2.), get("decoration:rounding").unwrap_or(8.))))
}

pub fn apply(window: Option<&mut Window>, cx: &mut App) {
    let settings = sys::load_settings();
    let (m, found) = matugen();
    let p = Pal::build(&m, found);
    let term = sys::terminal_font();
    let user_font = settings["font"].as_str().filter(|s| !s.is_empty()).map(String::from);
    let names = cx.text_system().all_font_names();
    let find = |f: String| -> Option<String> { names.iter().find(|n| n.eq_ignore_ascii_case(f.trim().trim_matches('"'))).cloned() };
    let user_font = user_font.and_then(|f| find(f));
    let term_font = term["family"].as_str().and_then(|f| find(f.to_string()));
    let mono: SharedString = user_font.clone().or(term_font).or_else(|| ["JetBrains Mono", "IBM Plex Mono", "DejaVu Sans Mono"].into_iter().find_map(|f| find(f.into()))).unwrap_or_else(|| "monospace".into()).into();
    let prose: SharedString = settings["proseFont"].as_str().filter(|s| !s.is_empty()).and_then(|s| find(s.to_string())).map(SharedString::from).unwrap_or_else(|| mono.clone());
    let weight = if user_font.is_some() { 400. } else { term["weight"].as_f64().unwrap_or(400.) as f32 };
    let fsize = settings["fontSize"].as_f64().unwrap_or(14.) as f32;
    let scale = settings["scale"].as_f64().filter(|s| *s > 0.).unwrap_or(1.) as f32;
    let (gaps_in, border, rounding) = hypr();
    let geo = Geo {
        gap: gaps_in * 2., bw: border.max(1.), round: rounding, r2: rounding * 0.6, fs: fsize, scale,
        side_w: settings["paneWidth"].as_f64().unwrap_or(520.) as f32, mono: mono.clone(), prose, weight: FontWeight(weight),
    };
    // the palette decides light or dark: matugen's surfaces are dark unless it was asked for a light scheme
    let dark = p.bg.l < 0.5;
    BASE.store(fsize.max(6.).to_bits(), std::sync::atomic::Ordering::Relaxed);
    Theme::change(if dark { ThemeMode::Dark } else { ThemeMode::Light }, window, cx);
    Theme::update(cx, |t| {
        let c = &mut t.colors;
        c.background = p.bg;
        c.foreground = p.text;
        c.muted = p.high;
        c.muted_foreground = p.dim;
        c.border = p.line;
        c.input = p.line;
        c.ring = p.primary;
        c.caret = p.primary;
        c.selection = p.selection;
        c.primary = p.primary;
        c.primary_hover = mix(p.primary, 0.88, gpui_kit::white());
        c.primary_active = p.primary;
        c.primary_foreground = p.on_primary;
        c.secondary = p.raised;
        c.secondary_hover = p.high;
        c.secondary_active = p.high;
        c.secondary_foreground = p.text;
        c.accent = p.high;
        c.accent_foreground = p.text;
        c.popover = p.raised;
        c.popover_foreground = p.text;
        c.list = p.bg;
        c.list_hover = p.hover;
        c.list_active = p.high;
        c.list_active_border = p.high.opacity(0.);
        c.sidebar = p.bg;
        c.sidebar_foreground = p.mid;
        c.sidebar_accent = p.high;
        c.sidebar_accent_foreground = p.text;
        c.sidebar_border = p.line;
        c.sidebar_primary = p.primary;
        c.sidebar_primary_foreground = p.on_primary;
        c.title_bar = p.gutter;
        c.title_bar_border = p.line;
        c.window_border = p.line;
        c.scrollbar = p.bg.opacity(0.);
        c.scrollbar_thumb = p.line;
        c.scrollbar_thumb_hover = p.dim;
        c.tab_bar = p.bg;
        c.overlay = fade(p.surface_lowest, 0.55);
        c.link = p.accent;
        c.link_hover = p.accent;
        c.link_active = p.accent;
        c.button_primary = p.primary;
        c.button_primary_hover = c.primary_hover;
        c.button_primary_active = p.primary;
        c.button_primary_foreground = p.on_primary;
        c.button = p.raised;
        c.button_hover = p.high;
        c.button_active = p.high;
        c.button_foreground = p.text;
        c.switch = mix(p.primary, 0.3, p.high);
        c.slider_bar = p.primary;
        c.slider_thumb = p.primary;
        c.progress_bar = p.primary;
        c.chart_1 = p.primary;
        c.tab_active = p.high;
        c.tab_active_foreground = p.text;
        c.tab_foreground = p.dim;
        c.green = p.green;
        c.green_light = p.add_bg;
        c.red = p.red;
        c.red_light = p.del_bg;
        c.yellow = p.gold;
        c.yellow_light = fade(p.gold, 0.14);
        c.success = p.green;
        c.danger = p.red;
        c.warning = p.gold;
        c.description_list_label = p.raised;
        c.description_list_label_foreground = p.mid;
        // the kit sizes its own controls off these, in real pixels
        t.font_size = px(fsize * scale);
        t.mono_font_size = px(fsize * scale);
        t.radius = px(geo.r2 * scale);
        t.radius_lg = px(geo.round * scale);
        t.shadow = true;
        t.font_family = mono.clone();
        t.mono_font_family = mono.clone();
        t.highlight_theme = syntax(&p);
    });
    cx.set_global(Look { pal: p, geo });
    cx.refresh_windows();
}

pub fn is_dark(cx: &App) -> bool { Theme::global(cx).mode == ThemeMode::Dark }

// tree-sitter colours, highlight.js's mapping from the webview build in the palette
pub fn syntax(p: &Pal) -> std::sync::Arc<gpui_kit::component::highlighter::HighlightTheme> {
    use gpui_kit::component::highlighter::HighlightTheme;
    thread_local! { static CACHE: std::cell::RefCell<Option<(Hsla, Hsla, std::sync::Arc<HighlightTheme>)>> = const { std::cell::RefCell::new(None) }; }
    if let Some(t) = CACHE.with(|c| c.borrow().as_ref().filter(|(a, b, _)| *a == p.primary && *b == p.bg).map(|(_, _, t)| t.clone())) { return t; }
    let st = |c: Hsla| serde_json::json!({ "color": c });
    let comment = serde_json::json!({ "color": p.dim, "font_style": "italic" });
    let syntax = serde_json::json!({
        "attribute": st(p.tertiary), "boolean": st(p.orange), "comment": comment, "comment.doc": comment,
        "constant": st(p.orange), "constructor": st(p.gold), "embedded": st(p.text), "emphasis": { "font_style": "italic" }, "emphasis.strong": { "font_weight": 700 },
        "enum": st(p.gold), "function": st(p.cyan), "keyword": st(p.primary), "label": st(p.tertiary), "link_text": st(p.cyan), "link_uri": st(p.green), "number": st(p.orange),
        "operator": st(p.text), "preproc": st(p.mid), "property": st(p.tertiary), "punctuation": st(p.text), "punctuation.bracket": st(p.text),
        "punctuation.delimiter": st(p.text), "punctuation.list_marker": st(p.md_mark), "punctuation.special": st(p.primary), "string": st(p.green),
        "string.escape": st(p.green), "string.regex": st(p.green), "string.special": st(p.green), "string.special.symbol": st(p.orange), "tag": st(p.magenta),
        "tag.doctype": st(p.primary), "text.literal": st(p.green), "title": { "color": p.cyan, "font_weight": 700 }, "type": st(p.gold), "variable": st(p.text),
        "variable.special": st(p.tertiary), "variant": st(p.gold),
    });
    let theme = serde_json::json!({ "name": "pith", "appearance": if p.bg.l < 0.5 { "dark" } else { "light" }, "style": { "syntax": syntax } });
    let t = match serde_json::from_value::<HighlightTheme>(theme) {
        Ok(t) => std::sync::Arc::new(t),
        Err(e) => { eprintln!("pith: syntax theme didn't parse, using the default: {e}"); HighlightTheme::default_dark() }
    };
    CACHE.with(|c| *c.borrow_mut() = Some((p.primary, p.bg, t.clone())));
    t
}
