// Highlighted lines for the files pane and the changes diff: the whole text split into lines with their colour runs,
// unlike the transcript's helper, which stops at a tool body's worth. one text cached per caller
use crate::ui::theme::is_dark;
use gpui_kit::base::input::Rope;
use gpui_kit::component::highlighter::{LanguageRegistry, SyntaxHighlighter};
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::rc::Rc;

pub use crate::ui::transcript::code::lang_of;

// highlighting more than this on the ui thread stalls a frame; the rest of a huge file is plain
const MAX_HIGHLIGHT: usize = 400_000;

pub struct Line { pub text: SharedString, pub runs: Vec<(Range<usize>, HighlightStyle)> }

#[derive(Default)]
pub struct Cache { key: u64, lines: Rc<Vec<Line>> }

impl Cache {
    // `version` names the text, so a redraw doesn't hash a whole file to find out it's the same one
    pub fn lines(&mut self, version: u64, text: &str, lang: Option<&str>, cx: &App) -> Rc<Vec<Line>> {
        let lang = lang.filter(|l| LanguageRegistry::singleton().language(l).is_some());
        let theme = cx.theme().highlight_theme.clone();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (version, lang, is_dark(cx), std::sync::Arc::as_ptr(&theme) as usize).hash(&mut h);
        let key = h.finish();
        if key != self.key || self.lines.is_empty() { self.key = key; self.lines = Rc::new(split(text, lang, cx)); }
        self.lines.clone()
    }
}

pub fn split(text: &str, lang: Option<&str>, cx: &App) -> Vec<Line> {
    let theme = cx.theme().highlight_theme.clone();
    let text = text.replace('\t', "    ");
    let runs = match lang {
        Some(l) if text.len() <= MAX_HIGHLIGHT => {
            let mut hl = SyntaxHighlighter::new(l);
            hl.update(None, &Rope::from_str(&text), None);
            hl.styles(&(0..text.len()), theme.as_ref())
        }
        _ => vec![],
    };
    let mut out = Vec::new();
    let (mut at, mut ri) = (0, 0);
    for line in text.split('\n') {
        let r = at..at + line.len();
        let mut mine = vec![];
        while ri < runs.len() && runs[ri].0.end <= r.start { ri += 1; }
        let mut j = ri;
        while j < runs.len() && runs[j].0.start < r.end {
            let (rr, st) = &runs[j];
            let (s, e) = (rr.start.max(r.start) - r.start, rr.end.min(r.end) - r.start);
            if s < e && line.is_char_boundary(s) && line.is_char_boundary(e) { mine.push((s..e, *st)); }
            j += 1;
        }
        out.push(Line { text: line.to_string().into(), runs: mine });
        at = r.end + 1;
    }
    out
}

pub fn styled(l: &Line) -> StyledText {
    let t: SharedString = if l.text.is_empty() { " ".into() } else { l.text.clone() };
    StyledText::new(t).with_highlights(l.runs.iter().cloned())
}
