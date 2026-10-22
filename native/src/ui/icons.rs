// the webview build's own 16px line icons (lib/icons.ts), served as svg assets so they draw exactly as they did.
// ic("globe") is the .ic element: 1.2em, the text colour. anything not under pith/ falls through to the kit's icons
use gpui_kit::*;
use std::borrow::Cow;

const P: &[(&str, &str)] = &[
    ("plus", "M8 3v10M3 8h10"),
    ("close", "M4 4l8 8M12 4l-8 8"),
    ("search", "M7 12A5 5 0 1 0 7 2a5 5 0 0 0 0 10zM10.6 10.6L14 14"),
    ("folder", "M2 4.5V12a1 1 0 0 0 1 1h10a1 1 0 0 0 1-1V6a1 1 0 0 0-1-1H8L6.5 3.5H3A1 1 0 0 0 2 4.5z"),
    ("globe", "M8 14A6 6 0 1 0 8 2a6 6 0 0 0 0 12zM2 8h12M8 2c-2 2-2 10 0 12M8 2c2 2 2 10 0 12"),
    ("frame", "M2.5 3.5h11v9h-11zM2.5 6h11M4.5 4.75h.01M6 4.75h.01"),
    ("diff", "M5 2v7M1.5 5.5h7M10 11h4.5M10.5 3l3 3-3 3"),
    ("file", "M4 1.5h5l3 3V14a.5.5 0 0 1-.5.5h-7.5A.5.5 0 0 1 3.5 14V2a.5.5 0 0 1 .5-.5zM9 1.5V5h3"),
    ("agents", "M5 7a2 2 0 1 0 0-4 2 2 0 0 0 0 4zM11.5 7a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3zM1.5 13c0-2 1.5-4 3.5-4s3.5 2 3.5 4M9.5 9.5c.5-.3 1.2-.5 2-.5 1.6 0 3 1.5 3 3.5"),
    ("image", "M2 3h12v10H2zM2 11l3.5-3.5 3 3 2-2L14 12M10.5 6.5h.01"),
    ("gear", "M8 10a2 2 0 1 0 0-4 2 2 0 0 0 0 4zM8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.4 3.4l1.4 1.4M11.2 11.2l1.4 1.4M3.4 12.6l1.4-1.4M11.2 4.8l1.4-1.4"),
    ("bars", "M2 14h12M3.5 11V8M6.5 11V4M9.5 11V6.5M12.5 11V2.5"),
    ("up", "M8 13V3M3.5 7.5L8 3l4.5 4.5"),
    ("stop", "M4.5 4.5h7v7h-7z"),
    ("clip", "M12.5 7.5l-5 5a3 3 0 0 1-4.25-4.25l5.5-5.5a2 2 0 0 1 2.8 2.8L6.2 11a1 1 0 0 1-1.4-1.4L9.5 5"),
    ("chevron", "M6 4l4 4-4 4"),
    ("down", "M4 6l4 4 4-4"),
    ("copy", "M5.5 5.5h7v8h-7zM3.5 10.5v-8h7"),
    ("branch", "M4.5 2v12M4.5 10c0-3 7-2 7-5.5M11.5 4.5V2"),
    ("refresh", "M13 8a5 5 0 1 1-1.5-3.5M13 2.5v3h-3"),
    ("external", "M9 2.5h4.5V7M13.5 2.5L7.5 8.5M11.5 9.5v3.5h-9v-9H6"),
    ("pin", "M6 2.5h4l-.5 4 2 2v1h-7v-1l2-2zM8 9.5V14"),
    ("trash", "M2.5 4h11M6 4V2.5h4V4M4 4l.7 9.5h6.6L12 4"),
    ("more", "M3.5 8h.01M8 8h.01M12.5 8h.01"),
    ("check", "M3 8.5l3 3 7-7"),
    ("back", "M10 3.5L5.5 8l4.5 4.5"),
    ("fwd", "M6 3.5l4.5 4.5L6 12.5"),
    ("pick", "M2.5 2.5l4.5 11 1.5-4.5 4.5-1.5zM9 9l4 4"),
    ("sidebar", "M2 3h12v10H2zM6 3v10"),
    ("pane", "M2 3h12v10H2zM10 3v10"),
    ("terminal", "M2.5 3.5h11v9h-11zM4.5 6.5l2 1.5-2 1.5M8 10h3"),
    ("phone", "M5 1.5h6v13H5zM7.5 12.5h1"),
    ("tablet", "M3 2h10v12H3zM7.5 12h1"),
    ("desktop", "M1.5 2.5h13v8.5h-13zM6 14h4M8 11v3"),
    ("edit", "M10.5 2.5l3 3-8 8h-3v-3z"),
    ("sparkle", "M8 2v3M8 11v3M2 8h3M11 8h3M4 4l1.5 1.5M10.5 10.5L12 12M4 12l1.5-1.5M10.5 5.5L12 4"),
    ("review", "M2.5 2.5h8l3 3v8h-11zM5 8l2 2 4-4"),
    ("list", "M5.5 4h8M5.5 8h8M5.5 12h8M2.5 4h.01M2.5 8h.01M2.5 12h.01"),
    ("window", "M3 8h10"),
    ("max", "M3.5 3.5h9v9h-9z"),
];

// the app's asset source: pith/<name>.svg is one of the icons above, everything else is the kit's
pub struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(name) = path.strip_prefix("pith/").and_then(|p| p.strip_suffix(".svg")) {
            let d = P.iter().find(|(n, _)| *n == name).or_else(|| P.iter().find(|(n, _)| *n == "more")).map(|x| x.1).unwrap_or("");
            let svg = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="black" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="{d}"/></svg>"#);
            return Ok(Some(Cow::Owned(svg.into_bytes())));
        }
        gpui_kit::assets::AllAssets.load(path)
    }
    fn list(&self, path: &str) -> Result<Vec<SharedString>> { gpui_kit::assets::AllAssets.list(path) }
}

// .ic: 1.2em square in the colour of the text around it (an svg doesn't inherit it, so it's read when drawn)
#[derive(IntoElement)]
pub struct Ic { name: &'static str, size: Option<Rems>, color: Option<Hsla> }
pub fn ic(name: &'static str) -> Ic { Ic { name, size: None, color: None } }
impl Ic {
    pub fn size(mut self, s: Rems) -> Self { self.size = Some(s); self }
    pub fn text_color(mut self, c: Hsla) -> Self { self.color = Some(c); self }
}
impl RenderOnce for Ic {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        let ts = window.text_style();
        let size = self.size.map(|s| s.to_pixels(window.rem_size())).unwrap_or(ts.font_size.to_pixels(window.rem_size()) * 1.2);
        svg().path(SharedString::from(format!("pith/{}.svg", self.name))).flex_none().size(size).text_color(self.color.unwrap_or(ts.color))
    }
}
