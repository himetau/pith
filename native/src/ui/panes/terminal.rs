// A shell in the session's folder. core::term owns the pty, alacritty_terminal keeps the screen and scrollback,
// and the visible rows are shaped and painted on a canvas in the mono font, one cell per column
use crate::core::term;
use crate::ui::theme::{is_dark, pal};
use alacritty_terminal::event::{Event as TEvent, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point as GPoint, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, TermMode};
use alacritty_terminal::vte::ansi::{Color as AColor, CursorShape, NamedColor, Processor, Rgb};
use alacritty_terminal::Term;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{h_flex, ActiveTheme, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub fn bind_keys(cx: &mut App) {
    // the app's ctrl shortcuts belong to the shell while it has focus. ctrl-j still closes the panel, ctrl-tab still cycles
    let mut keys: Vec<KeyBinding> = ('a'..='z').filter(|c| *c != 'j').map(|c| KeyBinding::new(&format!("ctrl-{c}"), NoAction {}, Some(CONTEXT))).collect();
    for k in ["escape", "tab", "shift-tab", "ctrl-space"] {
        keys.push(KeyBinding::new(k, NoAction {}, Some(CONTEXT)));
    }
    cx.bind_keys(keys);
}

const CONTEXT: &str = "PithTerminal";

struct Grid { cols: usize, rows: usize }
impl Dimensions for Grid {
    fn total_lines(&self) -> usize { self.rows }
    fn screen_lines(&self) -> usize { self.rows }
    fn columns(&self) -> usize { self.cols }
}

type ColorFmt = Arc<dyn Fn(Rgb) -> String + Sync + Send + 'static>;
enum Ev { Wake, Write(String), Copy(String), Color(usize, ColorFmt), Exit(u64) }

// the terminal reports back on the reader thread; everything that touches the ui or the pty goes through the channel
#[derive(Clone)]
struct Listener(async_channel::Sender<Ev>);
impl EventListener for Listener {
    fn send_event(&self, ev: TEvent) {
        let _ = match ev {
            TEvent::PtyWrite(s) => self.0.try_send(Ev::Write(s)),
            TEvent::ClipboardStore(_, s) => self.0.try_send(Ev::Copy(s)),
            TEvent::ColorRequest(i, f) => self.0.try_send(Ev::Color(i, f)),
            _ => Ok(()),
        };
    }
}

// where the grid sits on screen, written while painting and read by the mouse handlers
#[derive(Clone, Copy, Default)]
struct Geom { origin: Point<Pixels>, cell: Size<Pixels>, cols: usize, rows: usize }

pub struct TermView {
    id: String,
    cwd: String,
    term: Arc<Mutex<Term<Listener>>>,
    geom: Arc<Mutex<Geom>>,
    tx: async_channel::Sender<Ev>,
    focus: FocusHandle,
    gen: u64,
    exited: bool,
    blink: bool,
    scroll_px: f32,
    selecting: bool,
    _tasks: Vec<Task<()>>,
}

static NEXT: AtomicU64 = AtomicU64::new(1);

impl TermView {
    pub fn new(cwd: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (tx, rx) = async_channel::unbounded::<Ev>();
        let term = Term::new(Config::default(), &Grid { cols: 80, rows: 24 }, Listener(tx.clone()));
        let id = format!("pane-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::SeqCst));
        // a burst of output arrives as many wakes, the ui takes them all in one go
        let pump = cx.spawn(async move |this, cx| {
            while let Ok(ev) = rx.recv().await {
                let mut evs = vec![ev];
                while let Ok(e) = rx.try_recv() { evs.push(e); }
                if this.update(cx, |v, cx| { for e in evs { v.handle(e, cx); } cx.notify(); }).is_err() { break; }
            }
        });
        // only a focused cursor blinks, so an idle terminal doesn't keep the window repainting
        let blink = cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(530)).await;
            let alive = this.update_in(cx, |v, window, cx| {
                if v.focus.is_focused(window) { v.blink = !v.blink; cx.notify(); } else { v.blink = true; }
            });
            if alive.is_err() { break; }
        });
        let mut v = TermView {
            id, cwd, term: Arc::new(Mutex::new(term)), geom: Arc::new(Mutex::new(Geom::default())), tx, focus: cx.focus_handle(),
            gen: 0, exited: false, blink: true, scroll_px: 0., selecting: false, _tasks: vec![pump, blink],
        };
        v.start(cx);
        v
    }

    pub fn focus_handle(&self) -> FocusHandle { self.focus.clone() }

    fn start(&mut self, cx: &mut Context<Self>) {
        self.gen += 1;
        let gen = self.gen;
        let (cols, rows) = { let g = self.geom.lock().unwrap(); (if g.cols > 0 { g.cols } else { 80 }, if g.rows > 0 { g.rows } else { 24 }) };
        let (term, tx, tx2) = (self.term.clone(), self.tx.clone(), self.tx.clone());
        let parser: Mutex<Processor> = Mutex::new(Processor::new());
        let res = term::open(&self.id, &self.cwd, cols as u16, rows as u16, move |text| {
            let mut p = parser.lock().unwrap();
            p.advance(&mut *term.lock().unwrap(), text.as_bytes());
            let _ = tx.try_send(Ev::Wake);
        }, move |_| { let _ = tx2.try_send(Ev::Exit(gen)); });
        self.exited = res.is_err();
        cx.notify();
    }

    fn restart(&mut self, cx: &mut Context<Self>) {
        let (cols, rows) = { let g = self.geom.lock().unwrap(); (g.cols.max(2), g.rows.max(2)) };
        *self.term.lock().unwrap() = Term::new(Config::default(), &Grid { cols, rows }, Listener(self.tx.clone()));
        self.start(cx);
    }

    fn handle(&mut self, ev: Ev, cx: &mut Context<Self>) {
        match ev {
            Ev::Wake => {}
            Ev::Write(s) => term::write(&self.id, &s),
            Ev::Copy(s) => cx.write_to_clipboard(ClipboardItem::new_string(s)),
            Ev::Color(i, f) => {
                let c = Palette::new(cx);
                let h = match i { 256 => c.fg, 257 => c.bg, 258 => c.cursor, i if i < 16 => c.ansi[i], _ => c.fg };
                let r = Rgba::from(h);
                let rgb = Rgb { r: (r.r * 255.) as u8, g: (r.g * 255.) as u8, b: (r.b * 255.) as u8 };
                term::write(&self.id, &f(rgb));
            }
            Ev::Exit(g) => if g == self.gen { self.exited = true; },
        }
    }

    fn write(&mut self, s: &str, cx: &mut Context<Self>) {
        {
            let mut t = self.term.lock().unwrap();
            t.selection = None;
            t.scroll_display(Scroll::Bottom);
        }
        term::write(&self.id, s);
        self.blink = true;
        cx.notify();
    }

    fn mode(&self) -> TermMode { *self.term.lock().unwrap().mode() }

    fn paste(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) else { return };
        let text = text.replace("\r\n", "\r").replace('\n', "\r");
        let s = if self.mode().contains(TermMode::BRACKETED_PASTE) { format!("\x1b[200~{}\x1b[201~", text.replace('\x1b', "")) } else { text };
        self.write(&s, cx);
    }

    fn copy(&mut self, cx: &mut Context<Self>) {
        let s = self.term.lock().unwrap().selection_to_string();
        if let Some(s) = s.filter(|s| !s.is_empty()) { cx.write_to_clipboard(ClipboardItem::new_string(s)); }
    }

    fn key(&mut self, e: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let ks = &e.keystroke;
        let m = ks.modifiers;
        if self.exited {
            if ks.key == "enter" { self.restart(cx); cx.stop_propagation(); }
            return;
        }
        let mac = cfg!(target_os = "macos");
        let copy = (m.control && m.shift && ks.key == "c") || (mac && m.platform && ks.key == "c");
        let paste = (m.control && m.shift && ks.key == "v") || (mac && m.platform && ks.key == "v") || (m.shift && ks.key == "insert");
        if copy { self.copy(cx); cx.stop_propagation(); return; }
        if paste { self.paste(cx); cx.stop_propagation(); return; }
        if m.shift && !m.control && (ks.key == "pageup" || ks.key == "pagedown") {
            self.term.lock().unwrap().scroll_display(if ks.key == "pageup" { Scroll::PageUp } else { Scroll::PageDown });
            cx.notify();
            cx.stop_propagation();
            return;
        }
        if let Some(s) = to_esc(ks, self.mode().contains(TermMode::APP_CURSOR)) {
            self.write(&s, cx);
            cx.stop_propagation();
        }
    }

    fn wheel(&mut self, e: &ScrollWheelEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let lh = self.geom.lock().unwrap().cell.height;
        if lh <= px(0.) { return; }
        self.scroll_px += e.delta.pixel_delta(lh).y / lh;
        let lines = self.scroll_px.trunc() as i32;
        if lines == 0 { return; }
        self.scroll_px -= lines as f32;
        let mode = self.mode();
        // full screen apps (less, vim) get arrows for the wheel, the way other terminals do it
        if mode.contains(TermMode::ALT_SCREEN) && mode.contains(TermMode::ALTERNATE_SCROLL) {
            let app = mode.contains(TermMode::APP_CURSOR);
            let k = match (lines > 0, app) { (true, true) => "\x1bOA", (true, false) => "\x1b[A", (false, true) => "\x1bOB", (false, false) => "\x1b[B" };
            term::write(&self.id, &k.repeat(lines.unsigned_abs() as usize));
        } else {
            self.term.lock().unwrap().scroll_display(Scroll::Delta(lines));
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn cell_at(&self, pos: Point<Pixels>) -> Option<(GPoint, Side)> {
        let g = *self.geom.lock().unwrap();
        if g.cols == 0 || g.cell.width <= px(0.) { return None; }
        let x = ((pos.x - g.origin.x) / g.cell.width).max(0.);
        let y = ((pos.y - g.origin.y) / g.cell.height).max(0.);
        let col = (x as usize).min(g.cols - 1);
        let row = (y as usize).min(g.rows.saturating_sub(1));
        let side = if x.fract() > 0.5 { Side::Right } else { Side::Left };
        let offset = self.term.lock().unwrap().grid().display_offset() as i32;
        Some((GPoint::new(Line(row as i32 - offset), Column(col)), side))
    }

    fn mouse_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
        let Some((pt, side)) = self.cell_at(e.position) else { return };
        let ty = match e.click_count { 2 => SelectionType::Semantic, 3 => SelectionType::Lines, _ => SelectionType::Simple };
        self.term.lock().unwrap().selection = Some(Selection::new(ty, pt, side));
        self.selecting = true;
        cx.notify();
    }

    fn mouse_move(&mut self, e: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.selecting || e.pressed_button != Some(MouseButton::Left) { return; }
        let Some((pt, side)) = self.cell_at(e.position) else { return };
        if let Some(s) = self.term.lock().unwrap().selection.as_mut() { s.update(pt, side); }
        cx.notify();
    }

    fn mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.selecting = false;
        let mut t = self.term.lock().unwrap();
        if t.selection.as_ref().is_some_and(|s| s.is_empty()) { t.selection = None; }
        cx.notify();
    }
}

impl Drop for TermView {
    fn drop(&mut self) { term::close(&self.id); }
}

// xterm's encoding for a keystroke
fn to_esc(ks: &Keystroke, app_cursor: bool) -> Option<String> {
    let m = ks.modifiers;
    if m.platform { return None; }
    let n = 1 + m.shift as u8 + 2 * m.alt as u8 + 4 * m.control as u8;
    let csi = |c: char| if n > 1 { format!("\x1b[1;{n}{c}") } else if app_cursor { format!("\x1bO{c}") } else { format!("\x1b[{c}") };
    let tilde = |k: u8| if n > 1 { format!("\x1b[{k};{n}~") } else { format!("\x1b[{k}~") };
    let ss3 = |c: char| if n > 1 { format!("\x1b[1;{n}{c}") } else { format!("\x1bO{c}") };
    let alt = |s: &str| if m.alt { format!("\x1b{s}") } else { s.to_string() };
    Some(match ks.key.as_str() {
        "up" => csi('A'), "down" => csi('B'), "right" => csi('C'), "left" => csi('D'),
        "home" => csi('H'), "end" => csi('F'),
        "pageup" => tilde(5), "pagedown" => tilde(6), "delete" => tilde(3), "insert" => tilde(2),
        "f1" => ss3('P'), "f2" => ss3('Q'), "f3" => ss3('R'), "f4" => ss3('S'),
        "f5" => tilde(15), "f6" => tilde(17), "f7" => tilde(18), "f8" => tilde(19),
        "f9" => tilde(20), "f10" => tilde(21), "f11" => tilde(23), "f12" => tilde(24),
        "enter" => alt("\r"),
        "backspace" => if m.control { "\x08".into() } else { alt("\x7f") },
        "tab" => if m.shift { "\x1b[Z".into() } else { "\t".into() },
        "escape" => "\x1b".into(),
        "space" if m.control => "\0".into(),
        k if m.control => {
            let c = k.chars().next()?;
            if k.chars().count() != 1 { return None; }
            let b = match c.to_ascii_lowercase() {
                c @ 'a'..='z' => c as u8 & 0x1f,
                '@' | '2' => 0, '[' | '3' => 0x1b, '\\' | '4' => 0x1c, ']' | '5' => 0x1d, '^' | '6' => 0x1e, '_' | '-' | '7' => 0x1f, '8' => 0x7f,
                _ => return None,
            };
            alt(&(b as char).to_string())
        }
        k => {
            // alt sends the plain key with an escape in front; otherwise whatever the layout typed
            if m.alt && k.chars().count() == 1 { format!("\x1b{}", if m.shift { k.to_uppercase() } else { k.to_string() }) }
            else { ks.key_char.clone().filter(|s| !s.is_empty())? }
        }
    })
}

// the 16 ansi colours come off pith's palette so the shell matches the rest of the app
struct Palette { fg: Hsla, bg: Hsla, cursor: Hsla, sel: Hsla, ansi: [Hsla; 16] }
impl Palette {
    fn new(cx: &App) -> Self {
        let p = pal(cx);
        let t = cx.theme();
        let dark = is_dark(cx);
        let h = |deg: f32, s: f32, l: f32| hsla(deg / 360., s, l, 1.);
        let (blue, magenta, cyan) = if dark { (h(214., 0.8, 0.68), h(300., 0.45, 0.7), h(186., 0.5, 0.6)) } else { (h(214., 0.7, 0.45), h(300., 0.4, 0.45), h(186., 0.65, 0.33)) };
        let (black, white, bblack, bwhite) = if dark { (h(240., 0.04, 0.3), p.mid, p.dim, t.foreground) } else { (t.foreground, p.dim, p.mid, h(240., 0.04, 0.7)) };
        let bright = |c: Hsla| hsla(c.h, c.s, (c.l + if dark { 0.08 } else { -0.06 }).clamp(0., 1.), 1.);
        Palette {
            fg: t.foreground, bg: t.background, cursor: t.primary, sel: t.selection,
            ansi: [black, p.red, p.green, p.gold, blue, magenta, cyan, white, bblack, bright(p.red), bright(p.green), bright(p.gold), bright(blue), bright(magenta), bright(cyan), bwhite],
        }
    }

    fn resolve(&self, c: AColor, over: &alacritty_terminal::term::color::Colors) -> Hsla {
        let from_rgb = |r: Rgb| Hsla::from(Rgba { r: r.r as f32 / 255., g: r.g as f32 / 255., b: r.b as f32 / 255., a: 1. });
        match c {
            AColor::Spec(rgb) => from_rgb(rgb),
            AColor::Indexed(i) => {
                if let Some(rgb) = over[i as usize] { return from_rgb(rgb); }
                let i = i as usize;
                if i < 16 { return self.ansi[i]; }
                if i < 232 {
                    let i = i - 16;
                    let v = |x: usize| if x == 0 { 0. } else { (55 + x * 40) as f32 / 255. };
                    return Hsla::from(Rgba { r: v(i / 36), g: v((i / 6) % 6), b: v(i % 6), a: 1. });
                }
                let l = (8 + (i - 232) * 10) as f32 / 255.;
                Hsla::from(Rgba { r: l, g: l, b: l, a: 1. })
            }
            AColor::Named(n) => {
                if let Some(rgb) = over[n as usize] { return from_rgb(rgb); }
                match n {
                    NamedColor::Foreground | NamedColor::BrightForeground => self.fg,
                    NamedColor::DimForeground => self.fg.opacity(0.66),
                    NamedColor::Background => self.bg,
                    NamedColor::Cursor => self.cursor,
                    n => {
                        let i = n as usize;
                        if i < 16 { self.ansi[i] }
                        else if (NamedColor::DimBlack as usize..=NamedColor::DimWhite as usize).contains(&i) { self.ansi[i - NamedColor::DimBlack as usize].opacity(0.7) }
                        else { self.fg }
                    }
                }
            }
        }
    }
}

// what one frame paints: background runs, shaped text segments, the cursor
struct Frame { bgs: Vec<(Bounds<Pixels>, Hsla)>, text: Vec<(Point<Pixels>, ShapedLine)>, cursor: Option<(Bounds<Pixels>, bool)>, line_h: Pixels }

fn layout(bounds: Bounds<Pixels>, term: &Mutex<Term<Listener>>, geom: &Mutex<Geom>, id: &str, base: &Font, size: Pixels, pal: &Palette, cursor_on: bool, focused: bool, window: &mut Window) -> Frame {
    let ts = window.text_system().clone();
    let fid = ts.resolve_font(base);
    let cell_w = ts.advance(fid, size, 'm').map(|s| s.width).unwrap_or(size * 0.6);
    let line_h = (size * 1.35).round();
    let cols = ((bounds.size.width / cell_w).floor() as usize).max(2);
    let rows = ((bounds.size.height / line_h).floor() as usize).max(2);
    let mut t = term.lock().unwrap();
    {
        let mut g = geom.lock().unwrap();
        if g.cols != cols || g.rows != rows {
            t.resize(Grid { cols, rows });
            term::resize(id, cols as u16, rows as u16);
        }
        *g = Geom { origin: bounds.origin, cell: gpui_kit::size(cell_w, line_h), cols, rows };
    }
    let content = t.renderable_content();
    let offset = content.display_offset as i32;
    let sel = content.selection;
    let cur = content.cursor;
    let cur_row = cur.point.line.0 + offset;
    let show_cursor = cur.shape != CursorShape::Hidden && (cursor_on || !focused) && offset == 0;
    let block = focused && matches!(cur.shape, CursorShape::Block | CursorShape::HollowBlock);

    let mut frame = Frame { bgs: vec![], text: vec![], cursor: None, line_h };
    let mut seg = String::new();
    let mut runs: Vec<TextRun> = vec![];
    let mut seg_at = (0i32, 0usize);
    let at = |row: i32, col: usize| point(bounds.origin.x + cell_w * col as f32, bounds.origin.y + line_h * row as f32);
    let flush = |seg: &mut String, runs: &mut Vec<TextRun>, seg_at: (i32, usize), frame: &mut Frame| {
        if seg.trim_end().is_empty() { seg.clear(); runs.clear(); return; }
        let line = ts.shape_line(SharedString::from(std::mem::take(seg)), size, runs, Some(cell_w));
        runs.clear();
        frame.text.push((at(seg_at.0, seg_at.1), line));
    };

    for cell in content.display_iter {
        let row = cell.point.line.0 + offset;
        let col = cell.point.column.0;
        if row != seg_at.0 {
            flush(&mut seg, &mut runs, seg_at, &mut frame);
            seg_at = (row, col);
        } else if seg.is_empty() {
            seg_at = (row, col);
        }
        if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) { continue; }
        let wide = cell.flags.contains(Flags::WIDE_CHAR);
        let mut fg = pal.resolve(cell.fg, content.colors);
        let mut bg = match cell.bg { AColor::Named(NamedColor::Background) => None, c => Some(pal.resolve(c, content.colors)) };
        if cell.flags.contains(Flags::INVERSE) { let f = fg; fg = bg.unwrap_or(pal.bg); bg = Some(f); }
        if cell.flags.contains(Flags::DIM) { fg = fg.opacity(0.7); }
        let gp = GPoint::new(cell.point.line, cell.point.column);
        if sel.is_some_and(|s| s.contains(gp)) { bg = Some(pal.sel); }
        let is_cursor = show_cursor && row == cur_row && col == cur.point.column.0;
        let w = if wide { 2. } else { 1. };
        if is_cursor {
            let b = Bounds::new(at(row, col), gpui_kit::size(cell_w * w, line_h));
            if block { bg = Some(pal.cursor); fg = pal.bg; }
            frame.cursor = Some((b, block));
        }
        if let Some(bg) = bg {
            let b = Bounds::new(at(row, col), gpui_kit::size(cell_w * w, line_h));
            match frame.bgs.last_mut() {
                Some((lb, lc)) if *lc == bg && lb.origin.y == b.origin.y && (lb.origin.x + lb.size.width - b.origin.x).abs() < px(0.5) => lb.size.width += b.size.width,
                _ => frame.bgs.push((b, bg)),
            }
        }
        let c = if cell.flags.contains(Flags::HIDDEN) || cell.c == '\t' { ' ' } else { cell.c };
        let mut font = base.clone();
        if cell.flags.contains(Flags::BOLD) { font.weight = FontWeight::BOLD; }
        if cell.flags.contains(Flags::ITALIC) { font.style = FontStyle::Italic; }
        let underline = cell.flags.intersects(Flags::ALL_UNDERLINES).then(|| UnderlineStyle { thickness: px(1.), color: Some(fg), wavy: cell.flags.contains(Flags::UNDERCURL) });
        let strike = cell.flags.contains(Flags::STRIKEOUT).then(|| StrikethroughStyle { thickness: px(1.), color: Some(fg) });
        let start = seg.len();
        seg.push(c);
        if let Some(zw) = cell.zerowidth() { seg.extend(zw.iter()); }
        let len = seg.len() - start;
        match runs.last_mut() {
            Some(r) if r.font == font && r.color == fg && r.underline == underline && r.strikethrough == strike => r.len += len,
            _ => runs.push(TextRun { len, font, color: fg, background_color: None, underline, strikethrough: strike }),
        }
        // a wide glyph is forced to one cell's advance, so the text after it starts a fresh segment two cells on
        if wide {
            flush(&mut seg, &mut runs, seg_at, &mut frame);
            seg_at = (row, col + 2);
        }
    }
    flush(&mut seg, &mut runs, seg_at, &mut frame);
    frame
}

fn paint(frame: Frame, focused: bool, pal: &Palette, window: &mut Window, cx: &mut App) {
    for (b, c) in frame.bgs { window.paint_quad(fill(b, c)); }
    for (o, line) in frame.text { let _ = line.paint(o, frame.line_h, TextAlign::Left, None, window, cx); }
    if let Some((b, block)) = frame.cursor {
        if block { return; }
        if focused {
            // bar and underline shapes, as the shell asked for
            window.paint_quad(fill(Bounds::new(b.origin, gpui_kit::size(px(2.), b.size.height)), pal.cursor));
        } else {
            window.paint_quad(outline(b, pal.cursor.opacity(0.7), BorderStyle::default()));
        }
    }
}

impl Render for TermView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus.is_focused(window);
        let colors = Palette::new(cx);
        let (term, geom, id) = (self.term.clone(), self.geom.clone(), self.id.clone());
        let base = font(cx.theme().mono_font_family.clone());
        let size = cx.theme().mono_font_size;
        let cursor_on = self.blink;
        let dim = pal(cx).dim;
        let restart = cx.listener(|v, _: &ClickEvent, window, cx| { v.restart(cx); v.focus.focus(window, cx); });
        div().id("term").key_context(CONTEXT).track_focus(&self.focus).size_full().relative().bg(colors.bg).pl_3().pr_1().pt_2().pb_1()
            .cursor(CursorStyle::IBeam)
            .on_key_down(cx.listener(Self::key))
            .on_scroll_wheel(cx.listener(Self::wheel))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .child(canvas(
                move |bounds, window, _cx| layout(bounds, &term, &geom, &id, &base, size, &colors, cursor_on, focused, window),
                move |_bounds, frame, window, cx| { let pal = Palette::new(cx); paint(frame, focused, &pal, window, cx) },
            ).size_full())
            .when(self.exited, |d| d.child(
                h_flex().absolute().bottom_0().left_0().right_0().h(px(36.)).px_3().gap_2().items_center().bg(cx.theme().background).border_t_1().border_color(cx.theme().border)
                    .text_sm().text_color(dim).child("The shell exited")
                    .child(div().flex_1())
                    .child(Button::new("term-restart").label("Restart").small().ghost().on_click(restart)),
            ))
    }
}
