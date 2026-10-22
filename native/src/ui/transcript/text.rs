// Text in the transcript: one styled run that takes part in the window's selection like the webview's did, with
// clickable ranges (paths, links) and the blinking stream caret drawn at its end
use gpui_kit::base::{TextSelection, TextSelectionHandle, TextSelectionRegistration, TextSelectionRun};
use gpui_kit::*;
use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;

pub type OnClick = Rc<dyn Fn(&mut Window, &mut App)>;

thread_local! {
    // reading order across every run on screen: they prepaint top to bottom, so a counter is the order
    static ORDER: Cell<u64> = const { Cell::new(0) };
}

pub struct Sel {
    id: ElementId,
    text: SharedString,
    styled: StyledText,
    clicks: Vec<(Range<usize>, OnClick)>,
    caret: Option<Hsla>,
}

pub fn sel(id: impl Into<ElementId>, text: impl Into<SharedString>) -> Sel {
    let mut text: SharedString = text.into();
    // an empty run lays out no line at all
    if text.is_empty() { text = " ".into(); }
    Sel { id: id.into(), styled: StyledText::new(text.clone()), text, clicks: vec![], caret: None }
}

impl Sel {
    pub fn runs(mut self, runs: Vec<(Range<usize>, HighlightStyle)>) -> Self {
        let len = self.text.len();
        let runs: Vec<_> = runs.into_iter().filter(|(r, _)| r.start < r.end && r.end <= len).collect();
        self.styled = StyledText::new(self.text.clone()).with_highlights(runs);
        self
    }
    pub fn clicks(mut self, c: Vec<(Range<usize>, OnClick)>) -> Self { self.clicks = c; self }
    // the stream caret after the last character
    pub fn caret(mut self, c: Option<Hsla>) -> Self { self.caret = c; self }
}

impl IntoElement for Sel {
    type Element = Self;
    fn into_element(self) -> Self { self }
}

impl Element for Sel {
    type RequestLayoutState = TextSelectionHandle;
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> { Some(self.id.clone()) }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> { None }

    fn request_layout(&mut self, gid: Option<&GlobalElementId>, iid: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, TextSelectionHandle) {
        let text = self.text.clone();
        let handle = window.with_element_state(gid.expect("sel needs an id"), |kept: Option<TextSelectionHandle>, _| {
            let h = kept.unwrap_or_else(|| TextSelectionHandle::new(text.to_string(), cx));
            (h.clone(), h)
        });
        let (id, ()) = self.styled.request_layout(gid, iid, window, cx);
        (id, handle)
    }

    fn prepaint(&mut self, gid: Option<&GlobalElementId>, iid: Option<&InspectorElementId>, bounds: Bounds<Pixels>, handle: &mut TextSelectionHandle, window: &mut Window, cx: &mut App) -> Hitbox {
        self.styled.prepaint(gid, iid, bounds, &mut (), window, cx);
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        let order = ORDER.with(|o| { let v = o.get() + 1; o.set(v); v });
        let reg = TextSelectionRegistration::new(hitbox.clone(), bounds).with_document_order(order).with_text_bounds(vec![bounds]).with_rendered_element(handle, window, cx);
        handle.register(reg, window, cx);
        hitbox
    }

    fn paint(&mut self, gid: Option<&GlobalElementId>, iid: Option<&InspectorElementId>, bounds: Bounds<Pixels>, handle: &mut TextSelectionHandle, hitbox: &mut Hitbox, window: &mut Window, cx: &mut App) {
        let layout = self.styled.layout().clone();
        let before = TextSelection::selected_text(window, cx);
        let proj = handle.update_runs(&[TextSelectionRun::new(self.text.clone(), layout.clone(), bounds)], cx);
        if before != TextSelection::selected_text(window, cx) { window.refresh(); }
        let color = crate::ui::theme::pal(cx).selection;
        for r in proj.ranges().iter().flatten().cloned() { paint_sel(&layout, r, color, window); }
        self.styled.paint(gid, iid, bounds, &mut (), &mut (), window, cx);

        if let Some(c) = self.caret {
            // css: .58em wide, 1.1em tall, 2px after the text, blinking in two steps a second
            let on = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() / 500 % 2 == 0).unwrap_or(true);
            let fs = window.text_style().font_size.to_pixels(window.rem_size());
            if let Some(at) = layout.position_for_index(self.text.len()) {
                let lh = layout.line_height();
                let h = fs * 1.1;
                let top = at.y + (lh - h) / 2. + fs * 0.08;
                if on { window.paint_quad(fill(Bounds::new(point(at.x + px(2.), top), size(fs * 0.58, h)), c).corner_radii(px(1.))); }
            }
            window.request_animation_frame();
        }

        if self.clicks.is_empty() { return; }
        let over = |pos: Point<Pixels>, l: &TextLayout, clicks: &[(Range<usize>, OnClick)]| -> Option<usize> {
            let ix = l.index_for_position(pos).ok()?;
            clicks.iter().position(|(r, _)| r.contains(&ix))
        };
        if hitbox.is_hovered(window) && over(window.mouse_position(), &layout, &self.clicks).is_some() {
            window.set_cursor_style(CursorStyle::PointingHand, hitbox);
        }
        let clicks: Rc<Vec<(Range<usize>, OnClick)>> = Rc::new(std::mem::take(&mut self.clicks));
        let down: Rc<Cell<Option<Point<Pixels>>>> = Rc::new(Cell::new(None));
        let (h1, h2, d1, d2) = (hitbox.clone(), hitbox.clone(), down.clone(), down);
        window.on_mouse_event(move |e: &MouseDownEvent, phase, window, _| {
            if phase == DispatchPhase::Bubble && e.button == MouseButton::Left && h1.is_hovered(window) { d1.set(Some(e.position)); }
        });
        window.on_mouse_event(move |e: &MouseUpEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || e.button != MouseButton::Left { return; }
            let Some(at) = d2.take() else { return };
            // a drag is a selection, not a click
            if !h2.is_hovered(window) || (e.position - at).magnitude() > 4. { return; }
            if let Some(i) = over(e.position, &layout, &clicks) { cx.stop_propagation(); let f = clicks[i].1.clone(); f(window, cx); }
        });
    }
}

fn paint_sel(layout: &TextLayout, r: Range<usize>, color: Hsla, window: &mut Window) {
    let (Some(a), Some(b)) = (layout.position_for_index(r.start), layout.position_for_index(r.end)) else { return };
    let (lh, bb) = (layout.line_height(), layout.bounds());
    let mut quads = vec![];
    if a.y == b.y { quads.push(Bounds::from_corners(a, point(b.x, b.y + lh))); } else {
        quads.push(Bounds::from_corners(a, point(bb.right(), a.y + lh)));
        if b.y > a.y + lh { quads.push(Bounds::from_corners(point(bb.left(), a.y + lh), point(bb.right(), b.y))); }
        quads.push(Bounds::from_corners(point(bb.left(), b.y), point(b.x, b.y + lh)));
    }
    for q in quads { window.paint_quad(fill(q, color)); }
}
