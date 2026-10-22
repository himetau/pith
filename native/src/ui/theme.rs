// pith's look: neutral greys, one accent, the platform's own UI font. code and tool output get the mono font.
// light and dark follow the system. on linux the accent follows matugen when ~/.config/pith/colors.css exists
use crate::core::sys;
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::*;

fn hex(s: &str) -> Option<Hsla> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 { return None; }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some(rgb(v).into())
}

// matugen's --primary, if the file is there
fn matugen_accent() -> Option<Hsla> {
    let css = std::fs::read_to_string(sys::theme_file()).ok()?;
    let line = css.lines().find(|l| l.trim_start().starts_with("--primary:"))?;
    hex(line.split(':').nth(1)?.trim_end_matches(';').trim())
}

fn g(l: f32) -> Hsla { hsla(240. / 360., 0.04, l, 1.) }
fn with_l(c: Hsla, l: f32) -> Hsla { hsla(c.h, c.s, l, 1.) }

pub fn apply(window: Option<&mut Window>, cx: &mut App) {
    Theme::sync_system_appearance(window, cx);
    let settings = sys::load_settings();
    let ui_font = settings["uiFont"].as_str().filter(|s| !s.is_empty()).map(String::from);
    let mono = settings["font"].as_str().filter(|s| !s.is_empty()).map(String::from)
        .or_else(|| sys::terminal_font()["family"].as_str().map(String::from));
    // textSize is the native build's own key: the old fontSize was for a page that was all monospace
    let size = settings["textSize"].as_f64().unwrap_or(15.) as f32;
    let accent = matugen_accent();
    Theme::update(cx, |t| {
        let dark = t.is_dark();
        // the accent has to read on both grounds: lighter on dark, deeper on light
        let a = match accent {
            Some(c) => if dark { with_l(c, c.l.max(0.62)) } else { with_l(c, c.l.min(0.45)) },
            None => if dark { hsla(211. / 360., 1., 0.62, 1.) } else { hsla(211. / 360., 1., 0.47, 1.) },
        };
        let (bg, side, raised, hover, active, border, fg, mid, dim) = if dark {
            (g(0.105), g(0.082), g(0.14), g(0.165), g(0.2), g(0.2), g(0.93), g(0.68), g(0.5))
        } else {
            (g(1.), g(0.965), g(1.), g(0.94), g(0.91), g(0.885), g(0.12), g(0.36), g(0.52))
        };
        let c = &mut t.colors;
        c.background = bg;
        c.foreground = fg;
        c.muted = hover;
        c.muted_foreground = dim;
        c.border = border;
        c.input = border;
        c.ring = a.opacity(0.55);
        c.caret = a;
        c.selection = a.opacity(0.28);
        c.primary = a;
        c.primary_hover = with_l(a, a.l + if dark { 0.05 } else { -0.05 });
        c.primary_active = with_l(a, a.l + if dark { -0.05 } else { 0.05 });
        c.primary_foreground = if dark { g(0.08) } else { g(1.) };
        c.secondary = hover;
        c.secondary_hover = active;
        c.secondary_active = active;
        c.secondary_foreground = fg;
        c.accent = hover;
        c.accent_foreground = fg;
        c.popover = raised;
        c.popover_foreground = fg;
        c.list = bg;
        c.list_hover = hover;
        c.list_active = a.opacity(0.14);
        c.list_active_border = a.opacity(0.0);
        c.sidebar = side;
        c.sidebar_foreground = mid;
        c.sidebar_accent = active;
        c.sidebar_accent_foreground = fg;
        c.sidebar_border = border;
        c.sidebar_primary = a;
        c.sidebar_primary_foreground = c.primary_foreground;
        c.title_bar = bg;
        c.title_bar_border = border;
        c.window_border = border;
        c.scrollbar = bg.opacity(0.);
        c.scrollbar_thumb = fg.opacity(0.18);
        c.scrollbar_thumb_hover = fg.opacity(0.3);
        c.tab_bar = side;
        c.overlay = hsla(0., 0., 0., if dark { 0.45 } else { 0.18 });
        c.link = a;
        c.link_hover = c.primary_hover;
        c.link_active = c.primary_active;
        // buttons and the controls that take the accent: the kit's defaults are its own greys
        c.button_primary = a;
        c.button_primary_hover = c.primary_hover;
        c.button_primary_active = c.primary_active;
        c.button_primary_foreground = c.primary_foreground;
        c.button = raised;
        c.button_hover = hover;
        c.button_active = active;
        c.button_foreground = fg;
        c.switch = a;
        c.slider_bar = a;
        c.slider_thumb = g(1.);
        c.progress_bar = a;
        c.chart_1 = a;
        c.tab_active = bg;
        c.tab_active_foreground = fg;
        c.tab_foreground = mid;
        c.description_list_label = hover;
        c.description_list_label_foreground = mid;
        t.font_size = px(size);
        t.mono_font_size = px((size - 1.).max(10.));
        t.radius = px(6.);
        t.radius_lg = px(10.);
        t.shadow = true;
        if let Some(f) = &ui_font { t.font_family = f.clone().into(); }
        if let Some(f) = &mono { t.mono_font_family = f.clone().into(); }
    });
}

pub fn is_dark(cx: &App) -> bool { Theme::global(cx).mode == ThemeMode::Dark }

// colours the component theme doesn't name
pub struct Pal { pub raised: Hsla, pub mid: Hsla, pub dim: Hsla, pub hairline: Hsla, pub green: Hsla, pub red: Hsla, pub gold: Hsla, pub code_bg: Hsla }
pub fn pal(cx: &App) -> Pal {
    let dark = is_dark(cx);
    if dark {
        Pal { raised: g(0.14), mid: g(0.72), dim: g(0.5), hairline: g(0.18), green: hsla(140. / 360., 0.5, 0.6, 1.), red: hsla(0., 0.75, 0.66, 1.), gold: hsla(38. / 360., 0.8, 0.62, 1.), code_bg: g(0.13) }
    } else {
        Pal { raised: g(1.), mid: g(0.32), dim: g(0.52), hairline: g(0.9), green: hsla(140. / 360., 0.55, 0.35, 1.), red: hsla(0., 0.65, 0.48, 1.), gold: hsla(32. / 360., 0.85, 0.42, 1.), code_bg: g(0.965) }
    }
}
