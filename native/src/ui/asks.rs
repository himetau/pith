// Cards for what pi asks (asks.ts, chat.css .ask): a tool waiting on approval, and the ask tool's questions.
// every pending one is shown, stacked; the top one takes digits, y/n, enter, esc and ↑↓ from the composer
// while the composer is empty
use super::theme::{geo, mix, pal, u};
use super::transcript::code;
use crate::state::tab::{Answer, Ask, AskKind, Tab};
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};

#[derive(Clone, Copy, Debug)]
pub enum AskKey { Char(char), Enter, Escape, Up, Down }


// a card's own state: the highlighted row, what's ticked on a pick-any, its text field
#[derive(Default)]
struct St { sel: usize, picked: BTreeSet<usize>, field: Option<Entity<InputState>>, _sub: Option<Subscription> }

pub struct Asks {
    tab: Entity<Tab>,
    st: HashMap<String, St>,
    _sub: Subscription,
}

fn perm_opts(tool: &str) -> [(String, Answer); 3] {
    [("Allow".into(), Answer::Allow), (format!("Allow {tool} for this session"), Answer::AllowTool), ("Deny".into(), Answer::Deny)]
}

fn q_label(q: &Value, i: usize) -> String { q["options"][i]["label"].as_str().unwrap_or("").to_string() }

impl Asks {
    pub fn new(tab: Entity<Tab>, cx: &mut Context<Self>) -> Self {
        let sub = cx.observe(&tab, |this, tab, cx| {
            // cards that went (answered, timed out) take their state with them
            let ids: Vec<String> = tab.read(cx).asks.iter().map(|a| a.id.clone()).collect();
            this.st.retain(|k, _| ids.contains(k));
            cx.notify();
        });
        Asks { tab, st: HashMap::new(), _sub: sub }
    }

    fn answer(&self, id: &str, v: Answer, cx: &mut Context<Self>) {
        let id = id.to_string();
        self.tab.update(cx, |t, cx| t.answer(&id, v, cx));
    }

    fn submit_question(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(a) = self.tab.read(cx).asks.iter().find(|a| a.id == id).cloned() else { return };
        let AskKind::Question(q) = &a.kind else { return };
        let st = self.st.entry(id.to_string()).or_default();
        let other = st.field.as_ref().map(|f| f.read(cx).value().trim().to_string()).unwrap_or_default();
        let v = if !other.is_empty() { Answer::Other(other) }
            else if q["multi"] == true { if st.picked.is_empty() { return; } Answer::Picked(st.picked.iter().map(|i| q_label(q, *i)).collect()) }
            else { Answer::Picked(vec![q_label(q, st.sel)]) };
        self.answer(id, v, cx);
    }

    fn pick_row(&mut self, id: &str, i: usize, multi: bool, label: String, cx: &mut Context<Self>) {
        let st = self.st.entry(id.to_string()).or_default();
        st.sel = i;
        if multi { if !st.picked.remove(&i) { st.picked.insert(i); } cx.notify(); } else { self.answer(id, Answer::Picked(vec![label]), cx); }
    }

    // the top card's keys. true when it took the key
    pub fn key(&mut self, k: AskKey, _window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(a) = self.tab.read(cx).asks.first().cloned() else { return false };
        let digit = |n: usize| match k { AskKey::Char(c) => c.to_digit(10).map(|d| d as usize).filter(|d| *d >= 1 && *d <= n), _ => None };
        let ch = |c: char| matches!(k, AskKey::Char(x) if x == c);
        let esc = matches!(k, AskKey::Escape);
        match &a.kind {
            AskKind::Perm { tool, .. } => {
                let opts = perm_opts(tool);
                let i = if let Some(n) = digit(opts.len()) { n - 1 } else if ch('y') { 0 } else if ch('n') || esc { opts.len() - 1 } else { return false };
                self.answer(&a.id, opts[i].1.clone(), cx);
            }
            AskKind::Question(q) => {
                let n = q["options"].as_array().map(|o| o.len()).unwrap_or(0);
                let multi = q["multi"] == true;
                if let Some(d) = digit(n) { self.pick_row(&a.id, d - 1, multi, q_label(q, d - 1), cx); return true; }
                match k {
                    AskKey::Down | AskKey::Up if n > 0 => {
                        let st = self.st.entry(a.id.clone()).or_default();
                        st.sel = if matches!(k, AskKey::Down) { (st.sel + 1) % n } else { (st.sel + n - 1) % n };
                        cx.notify();
                    }
                    AskKey::Enter => self.submit_question(&a.id, cx),
                    AskKey::Escape => self.answer(&a.id, Answer::Dismiss, cx),
                    _ => return false,
                }
            }
        }
        true
    }

    // a question's "something else…" field
    fn ensure_field(&mut self, a: &Ask, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(a.kind, AskKind::Question(_)) || self.st.get(&a.id).is_some_and(|s| s.field.is_some()) { return; }
        let id = a.id.clone();
        let s = cx.new(|cx| InputState::new(window, cx).placeholder("something else…"));
        let i2 = id.clone();
        let sub = cx.subscribe_in(&s, window, move |this, _, ev: &InputEvent, _, cx| if let InputEvent::PressEnter { .. } = ev { this.submit_question(&i2, cx) });
        let st = self.st.entry(id).or_default();
        st.field = Some(s);
        st._sub = Some(sub);
        // the model's recommended option starts highlighted
        if let AskKind::Question(q) = &a.kind { st.sel = q["recommended"].as_u64().map(|r| r as usize).unwrap_or(0); }
    }
}

// kbd from base.css
pub fn kbd(text: impl Into<SharedString>, cx: &App) -> Div {
    let (p, g) = (pal(cx), geo(cx));
    let fs = g.fs * 0.78;
    div().relative().flex_none().pt(u(2.)).pb(u(3.)).px(u(5.)).rounded(u(4.)).bg(p.high).text_color(p.mid).text_size(u(fs)).line_height(u(fs))
        .child(div().absolute().left_0().right_0().bottom_0().h(u(1.)).rounded_b(u(4.)).bg(p.line))
        .child(text.into())
}

// .btn.pri / .btn.ghost
fn btn(id: impl Into<ElementId>, label: &str, primary: bool, cx: &App) -> Stateful<Div> {
    let (p, g) = (pal(cx), geo(cx));
    let hover = if primary { mix(p.primary, 0.88, gpui_kit::white()) } else { p.high };
    div().id(id).flex().items_center().gap(u(6.)).py(u(5.)).px(u(12.)).rounded(u(g.r2)).border_1().cursor_pointer().whitespace_nowrap()
        .when(primary, |d| d.bg(p.primary).text_color(p.on_primary).border_color(p.primary))
        .when(!primary, |d| d.border_color(p.line).text_color(p.text))
        .hover(move |s| s.bg(hover))
        .child(label.to_string())
}

impl Render for Asks {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let asks = self.tab.read(cx).asks.clone();
        for a in &asks { self.ensure_field(a, window, cx); }
        let mut col = v_flex().w_full().gap(u(14.)).when(!asks.is_empty(), |d| d.pt(u(14.)));
        for a in &asks { col = col.child(self.card(a, window, cx)); }
        col
    }
}

impl Asks {
    fn card(&self, a: &Ask, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let (p, g) = (pal(cx), geo(cx));
        let id = a.id.clone();
        let border = match &a.kind { AskKind::Perm { .. } => mix(p.gold, 0.75, p.line), AskKind::Question(_) => mix(p.primary, 0.7, p.line) };
        let (tag, tag_bg, tag_fg, what) = match &a.kind {
            AskKind::Perm { tool, input } => {
                let path = super::tools::tilde(input["path"].as_str().unwrap_or(""));
                let what = match tool.as_str() { "bash" => "run this command".into(), "edit" | "hashline_edit" | "ast_edit" => format!("edit {path}"), "write" => format!("write {path}"), "generate_image" => "generate an image".into(), t => format!("use {t}") };
                ("permission", p.gold, p.surface_lowest, format!("pi wants to {what}"))
            }
            AskKind::Question(q) => ("question", p.primary, p.on_primary, q["header"].as_str().map(String::from).unwrap_or_else(|| if q["multi"] == true { "pick any".into() } else { "pick one".into() })),
        };
        let legend = super::frame::legend(super::frame::Side::L, p.bg, cx).px(u(6.)).gap(u(8.)).text_color(p.text)
            .child(div().px(u(6.)).rounded(u(3.)).bg(tag_bg).text_color(tag_fg).text_size(u(g.fs * 0.84 * 0.9)).child(tag))
            .child(div().min_w_0().truncate().child(what));
        let mut body = v_flex().w_full().gap(u(10.)).pt(u(18.)).px(u(14.)).pb(u(12.));
        let ask_q = |t: String| div().font_family(g.prose.clone()).text_color(p.text).child(t);
        let me = cx.weak_entity();
        let ans = move |id: String, v: Answer| { let me = me.clone(); move |_: &ClickEvent, _: &mut Window, cx: &mut App| { let _ = me.update(cx, |this, cx| this.answer(&id, v.clone(), cx)); } };

        match &a.kind {
            AskKind::Perm { tool, input } => {
                body = body.child(preview(&id, tool, input, cx));
                let opts = perm_opts(tool);
                let last = opts.len() - 1;
                body = body.child(h_flex().flex_wrap().gap(u(8.)).children(opts.into_iter().enumerate().map(|(i, (label, v))| {
                    opt(("perm-opt", i), &(i + 1).to_string(), &label, i == 0, i == last, cx).on_click(ans(id.clone(), v))
                })));
            }
            AskKind::Question(q) => {
                let multi = q["multi"] == true;
                let st = self.st.get(&id);
                let sel = st.map(|s| s.sel).unwrap_or(0);
                body = body.child(ask_q(q["question"].as_str().unwrap_or("").to_string()));
                let mut list = v_flex().gap(u(2.));
                for (i, o) in q["options"].as_array().cloned().unwrap_or_default().iter().enumerate() {
                    let label = o["label"].as_str().unwrap_or("").to_string();
                    let on = st.is_some_and(|s| s.picked.contains(&i));
                    let (hi, l2, i2, me) = (p.high, label.clone(), id.clone(), cx.weak_entity());
                    list = list.child(h_flex().id(("ask-row", i)).items_start().gap(u(10.)).py(u(4.)).px(u(8.)).rounded(u(5.)).cursor_pointer()
                        .when(i == sel, |d| d.bg(p.high)).hover(move |s| s.bg(hi))
                        .child(div().pt(u(2.)).child(kbd((i + 1).to_string(), cx)))
                        .when(multi, |d| d.child(div().flex_none().mt(u(3.)).size(u(14.)).rounded(u(3.)).border_1().border_color(p.line).flex().items_center().justify_center().text_color(p.accent)
                            .when(on, |d| d.child(Icon::new(IconName::Check).size(u(11.))))))
                        .child(v_flex().min_w_0().gap(u(8.))
                            .child(div().text_color(if on { p.accent } else { p.text }).child(label))
                            .when_some(o["description"].as_str(), |d, s| d.child(div().text_size(u(g.fs * 0.88)).font_family(g.prose.clone()).text_color(p.dim).child(s.to_string()))))
                        .on_click(move |_, _, cx| { let _ = me.update(cx, |this, cx| this.pick_row(&i2, i, multi, l2.clone(), cx)); }));
                }
                body = body.child(list);
                let me = cx.weak_entity();
                let i2 = id.clone();
                body = body.child(h_flex().gap(u(8.)).items_center()
                    .children(st.and_then(|s| s.field.as_ref()).map(|f| field(f, window, cx)))
                    .when(multi, |d| d.child(btn("ask-answer", "answer", true, cx).on_click(move |_, _, cx| { let _ = me.update(cx, |this, cx| this.submit_question(&i2, cx)); })))
                    .child(btn("ask-skip", "skip", false, cx).on_click(ans(id.clone(), Answer::Dismiss))));
            }
        }
        super::frame::boxed(cx).id(SharedString::from(format!("ask-{id}"))).w_full()
            .child(body)
            .child(super::frame::edge(border, cx))
            .child(legend)
            .into_any_element()
    }
}

// .ask-opt: a key hint and the option. the first is the yes, the last the deny
fn opt(id: impl Into<ElementId>, key: &str, label: &str, yes: bool, deny: bool, cx: &App) -> Stateful<Div> {
    let (p, g) = (pal(cx), geo(cx));
    let (hover, red) = (p.hover, p.red);
    div().id(id).flex().items_center().gap(u(8.)).py(u(4.)).pr(u(12.)).pl(u(6.)).rounded(u(g.r2)).border_1().cursor_pointer().text_color(p.text)
        .border_color(if yes { mix(p.primary, 0.5, p.line) } else { p.line })
        .hover(move |s| { let s = s.bg(hover); if deny { s.text_color(red) } else { s } })
        .child(kbd(key.to_string(), cx)).child(label.to_string())
}

// .ask-other: raised, a line border, the accent while typing
fn field(f: &Entity<InputState>, window: &Window, cx: &App) -> AnyElement {
    let (p, g) = (pal(cx), geo(cx));
    let focused = f.read(cx).focus_handle(cx).is_focused(window);
    div().flex_1().min_w_0().py(u(6.)).px(u(10.)).rounded(u(g.r2)).bg(p.raised).border_1()
        .border_color(if focused { p.primary } else { p.line })
        .child(Input::new(f).appearance(false).p(u(0.)).h_auto().text_size(u(g.fs))).into_any_element()
}

// what pi wants to do, shown before it does it (asks.ts preview)
fn preview(id: &str, tool: &str, input: &Value, cx: &App) -> AnyElement {
    let p = pal(cx);
    let path = input["path"].as_str().unwrap_or("");
    match tool {
        "edit" => {
            let pairs: Vec<(String, String)> = match input["edits"].as_array() {
                Some(e) => e.iter().map(|e| (e["oldText"].as_str().unwrap_or("").into(), e["newText"].as_str().unwrap_or("").into())).collect(),
                None => vec![(input["oldText"].as_str().unwrap_or("").into(), input["newText"].as_str().unwrap_or("").into())],
            };
            div().id(SharedString::from(format!("askd-{id}"))).max_h(u(300.)).overflow_y_scroll().rounded(u(5.))
                .child(code::edits(format!("perm-{id}"), &pairs, code::lang_of(path), 400, cx)).into_any_element()
        }
        "generate_image" => div().font_family(geo(cx).prose).text_color(p.text).child(input["prompt"].as_str().unwrap_or("").to_string()).into_any_element(),
        "bash" => ask_code(id, input["command"].as_str().unwrap_or(""), Some("bash"), cx),
        "write" => {
            let c = input["content"].as_str().unwrap_or("");
            let ls: Vec<&str> = c.split('\n').collect();
            let shown = if ls.len() > 40 { format!("{}\n… {} more lines", ls[..40].join("\n"), ls.len() - 40) } else { c.to_string() };
            ask_code(id, &shown, code::lang_of(path), cx)
        }
        _ => ask_code(id, &serde_json::to_string_pretty(input).unwrap_or_default(), None, cx),
    }
}

// .ask-code: highlighted, wrapped, on the low surface
fn ask_code(id: &str, text: &str, lang: Option<&str>, cx: &App) -> AnyElement {
    let (p, g) = (pal(cx), geo(cx));
    let ls = code::lines(text, lang, cx);
    div().id(SharedString::from(format!("askc-{id}"))).w_full().max_h(u(280.)).overflow_y_scroll().py(u(8.)).px(u(10.)).rounded(u(5.)).bg(p.surface_low)
        .text_size(u(g.fs * 0.9)).text_color(p.text)
        .children(ls.iter().map(|l| {
            let t: SharedString = if l.text.is_empty() { " ".into() } else { l.text.clone() };
            div().w_full().child(StyledText::new(t).with_highlights(l.runs.iter().cloned()))
        }))
        .into_any_element()
}
