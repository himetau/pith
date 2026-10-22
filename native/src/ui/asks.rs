// Cards for what an extension asks (asks.ts, chat.css .ask): pith's own permission prompts and ask_user questions, plus
// plain select / confirm / input / editor from any other extension. every pending one is shown, stacked; the top one
// takes digits, y/n, enter, esc and ↑↓ from the composer while the composer is empty
use super::theme::{geo, mix, pal, u};
use super::transcript::code;
use crate::state::tab::{Ask, Tab};
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::{h_flex, v_flex, Icon, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};

#[derive(Clone, Copy, Debug)]
pub enum AskKey { Char(char), Enter, Escape, Up, Down }

enum Field { Line(Entity<InputState>), Area(Entity<TextareaState>) }
impl Field {
    fn value(&self, cx: &App) -> String { match self { Field::Line(s) => s.read(cx).value().to_string(), Field::Area(s) => s.read(cx).value().to_string() } }
}

// a card's own state: the highlighted row, what's ticked on a pick-any, its text field
#[derive(Default)]
struct St { sel: usize, picked: BTreeSet<usize>, field: Option<Field>, _sub: Option<Subscription> }

pub struct Asks {
    tab: Entity<Tab>,
    st: HashMap<String, St>,
    _sub: Subscription,
}

enum Card { Perm(Value), Question(Value), Generic }

fn parse(title: &str, tag: &str) -> Option<Value> { serde_json::from_str(title.strip_prefix(tag)?).ok() }

fn card_of(a: &Ask) -> Card {
    if a.method == "select" { if let Some(v) = parse(&a.title, "pith:perm") { return Card::Perm(v); } }
    if a.method == "input" { if let Some(v) = parse(&a.title, "pith:ask") { return Card::Question(v); } }
    Card::Generic
}

fn perm_opts(a: &Ask) -> Vec<String> { if a.options.is_empty() { vec!["Allow".into(), "Deny".into()] } else { a.options.clone() } }

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

    fn answer(&self, id: &str, v: Value, cx: &mut Context<Self>) {
        let id = id.to_string();
        self.tab.update(cx, |t, cx| t.answer(&id, v, cx));
    }

    fn submit_question(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(a) = self.tab.read(cx).asks.iter().find(|a| a.id == id).cloned() else { return };
        let Card::Question(q) = card_of(&a) else { return };
        let st = self.st.entry(id.to_string()).or_default();
        let other = st.field.as_ref().map(|f| f.value(cx).trim().to_string()).unwrap_or_default();
        let v = if !other.is_empty() { other }
            else if q["multi"] == true { if st.picked.is_empty() { return; } st.picked.iter().map(|i| q_label(&q, *i)).collect::<Vec<_>>().join(", ") }
            else { q_label(&q, st.sel) };
        self.answer(id, json!({ "value": v }), cx);
    }

    fn submit_field(&mut self, id: &str, cx: &mut Context<Self>) {
        let v = self.st.get(id).and_then(|s| s.field.as_ref()).map(|f| f.value(cx)).unwrap_or_default();
        self.answer(id, json!({ "value": v }), cx);
    }

    fn pick_row(&mut self, id: &str, i: usize, multi: bool, label: String, cx: &mut Context<Self>) {
        let st = self.st.entry(id.to_string()).or_default();
        st.sel = i;
        if multi { if !st.picked.remove(&i) { st.picked.insert(i); } cx.notify(); } else { self.answer(id, json!({ "value": label }), cx); }
    }

    // the top card's keys. true when it took the key
    pub fn key(&mut self, k: AskKey, _window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(a) = self.tab.read(cx).asks.first().cloned() else { return false };
        let digit = |n: usize| match k { AskKey::Char(c) => c.to_digit(10).map(|d| d as usize).filter(|d| *d >= 1 && *d <= n), _ => None };
        let ch = |c: char| matches!(k, AskKey::Char(x) if x == c);
        let esc = matches!(k, AskKey::Escape);
        match card_of(&a) {
            Card::Perm(_) => {
                let opts = perm_opts(&a);
                let i = if let Some(n) = digit(opts.len()) { n - 1 } else if ch('y') { 0 } else if ch('n') || esc { opts.len() - 1 } else { return false };
                self.answer(&a.id, json!({ "value": opts[i] }), cx);
            }
            Card::Question(q) => {
                let n = q["options"].as_array().map(|o| o.len()).unwrap_or(0);
                let multi = q["multi"] == true;
                if let Some(d) = digit(n) { self.pick_row(&a.id, d - 1, multi, q_label(&q, d - 1), cx); return true; }
                match k {
                    AskKey::Down | AskKey::Up if n > 0 => {
                        let st = self.st.entry(a.id.clone()).or_default();
                        st.sel = if matches!(k, AskKey::Down) { (st.sel + 1) % n } else { (st.sel + n - 1) % n };
                        cx.notify();
                    }
                    AskKey::Enter => self.submit_question(&a.id, cx),
                    AskKey::Escape => self.answer(&a.id, json!({ "cancelled": true }), cx),
                    _ => return false,
                }
            }
            Card::Generic => match a.method.as_str() {
                "confirm" if ch('y') => self.answer(&a.id, json!({ "confirmed": true }), cx),
                "confirm" if ch('n') || esc => self.answer(&a.id, json!({ "confirmed": false }), cx),
                "select" => {
                    if let Some(d) = digit(a.options.len()) { self.answer(&a.id, json!({ "value": a.options[d - 1] }), cx); }
                    else if esc { self.answer(&a.id, json!({ "cancelled": true }), cx); }
                    else { return false; }
                }
                _ => return false,
            },
        }
        true
    }

    // the text fields cards carry: "something else…" on a question, an extension's input or editor
    fn ensure_field(&mut self, a: &Ask, window: &mut Window, cx: &mut Context<Self>) {
        let want = match card_of(a) { Card::Question(_) => 1, Card::Generic if a.method == "input" => 2, Card::Generic if a.method == "editor" => 3, _ => 0 };
        if want == 0 || self.st.get(&a.id).is_some_and(|s| s.field.is_some()) { return; }
        let id = a.id.clone();
        let prefill = a.prefill.clone().unwrap_or_default();
        let (field, sub, focus) = if want == 3 {
            let s = cx.new(|cx| { let mut s = TextareaState::new(window, cx).rows(8).submit_on_enter(false); s.set_value(prefill, window, cx); s });
            let i2 = id.clone();
            // ctrl+enter sends an editor's text
            let sub = cx.subscribe_in(&s, window, move |this, _, ev: &InputEvent, _, cx| if let InputEvent::PressEnter { secondary: true, .. } = ev { this.submit_field(&i2, cx) });
            let f = s.read(cx).focus_handle(cx);
            (Field::Area(s), sub, f)
        } else {
            let ph = if want == 1 { "something else…".to_string() } else { a.placeholder.clone().unwrap_or_default() };
            let s = cx.new(|cx| { let mut s = InputState::new(window, cx).placeholder(ph); s.set_value(prefill, window, cx); s });
            let i2 = id.clone();
            let sub = cx.subscribe_in(&s, window, move |this, _, ev: &InputEvent, _, cx| if let InputEvent::PressEnter { .. } = ev {
                if want == 1 { this.submit_question(&i2, cx) } else { this.submit_field(&i2, cx) }
            });
            let f = s.read(cx).focus_handle(cx);
            (Field::Line(s), sub, f)
        };
        let st = self.st.entry(id).or_default();
        st.field = Some(field);
        st._sub = Some(sub);
        // an extension's own field takes the focus when it shows up (asks.ts focuses it after 30ms)
        if want != 1 { cx.defer_in(window, move |_, window, cx| focus.focus(window, cx)); }
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
        let kind = card_of(a);
        let border = match kind { Card::Perm(_) => mix(p.gold, 0.75, p.line), Card::Question(_) => mix(p.primary, 0.7, p.line), Card::Generic => p.line };
        let (tag, tag_bg, tag_fg, what) = match &kind {
            Card::Perm(v) => {
                let tool = v["tool"].as_str().unwrap_or("");
                let path = super::tools::tilde(v["input"]["path"].as_str().unwrap_or(""));
                let what = match tool { "bash" => "run this command".into(), "edit" => format!("edit {path}"), "write" => format!("write {path}"), "generate_image" => "generate an image".into(), t => format!("use {t}") };
                ("permission", p.gold, p.surface_lowest, format!("pi wants to {what}"))
            }
            Card::Question(q) => ("question", p.primary, p.on_primary, if q["multi"] == true { "pick any".into() } else { "pick one".into() }),
            Card::Generic => ("extension", p.gold, p.surface_lowest, a.title.clone()),
        };
        let legend = super::frame::legend(super::frame::Side::L, p.bg, cx).px(u(6.)).gap(u(8.)).text_color(p.text)
            .child(div().px(u(6.)).rounded(u(3.)).bg(tag_bg).text_color(tag_fg).text_size(u(g.fs * 0.84 * 0.9)).child(tag))
            .child(div().min_w_0().truncate().child(what));
        let mut body = v_flex().w_full().gap(u(10.)).pt(u(18.)).px(u(14.)).pb(u(12.));
        let ask_q = |t: String| div().font_family(g.prose.clone()).text_color(p.text).child(t);
        let me = cx.weak_entity();
        let ans = move |id: String, v: Value| { let me = me.clone(); move |_: &ClickEvent, _: &mut Window, cx: &mut App| { let _ = me.update(cx, |this, cx| this.answer(&id, v.clone(), cx)); } };

        match &kind {
            Card::Perm(v) => {
                body = body.child(preview(&id, v["tool"].as_str().unwrap_or(""), &v["input"], cx));
                let opts = perm_opts(a);
                let last = opts.len() - 1;
                body = body.child(h_flex().flex_wrap().gap(u(8.)).children(opts.iter().enumerate().map(|(i, o)| {
                    opt(("perm-opt", i), &(i + 1).to_string(), o, i == 0, i == last && i > 0, cx).on_click(ans(id.clone(), json!({ "value": o })))
                })));
            }
            Card::Question(q) => {
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
                        .child(v_flex().min_w_0()
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
                    .child(btn("ask-skip", "skip", false, cx).on_click(ans(id.clone(), json!({ "cancelled": true })))));
            }
            Card::Generic => {
                match a.method.as_str() {
                    "confirm" => {
                        body = body.when_some(a.message.clone(), |d, m| d.child(ask_q(m)))
                            .child(h_flex().flex_wrap().gap(u(8.))
                                .child(opt("ask-yes", "y", "yes", true, false, cx).on_click(ans(id.clone(), json!({ "confirmed": true }))))
                                .child(opt("ask-no", "n", "no", false, true, cx).on_click(ans(id.clone(), json!({ "confirmed": false })))));
                    }
                    "select" => {
                        let hi = p.high;
                        body = body.child(v_flex().gap(u(2.)).children(a.options.iter().enumerate().map(|(i, o)| {
                            h_flex().id(("ask-opt", i)).items_start().gap(u(10.)).py(u(4.)).px(u(8.)).rounded(u(5.)).cursor_pointer().hover(move |s| s.bg(hi))
                                .child(div().pt(u(2.)).child(kbd((i + 1).to_string(), cx))).child(div().min_w_0().text_color(p.text).child(o.clone()))
                                .on_click(ans(id.clone(), json!({ "value": o })))
                        })));
                    }
                    _ => {
                        let me = cx.weak_entity();
                        let (i2, i3) = (id.clone(), id.clone());
                        let cancel = ans(id.clone(), json!({ "cancelled": true }));
                        body = body
                            .children(self.st.get(&id).and_then(|s| s.field.as_ref()).map(|f| div().w_full()
                                .capture_action(move |_: &Escape, _, cx| { let _ = me.update(cx, |this, cx| this.answer(&i3, json!({ "cancelled": true }), cx)); })
                                .child(field(f, window, cx))))
                            .child(h_flex().gap(u(8.)).items_center()
                                .child(btn("ask-send", "send", true, cx).on_click({ let me = cx.weak_entity(); move |_, _, cx| { let _ = me.update(cx, |this, cx| this.submit_field(&i2, cx)); } }))
                                .child(btn("ask-cancel", "cancel", false, cx).on_click(cancel)));
                    }
                }
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

// .ask-other / .ask-edit: raised, a line border, the accent while typing
fn field(f: &Field, window: &Window, cx: &App) -> AnyElement {
    let (p, g) = (pal(cx), geo(cx));
    let (focus, el) = match f {
        Field::Line(s) => (s.read(cx).focus_handle(cx), Input::new(s).appearance(false).p(u(0.)).h_auto().into_any_element()),
        Field::Area(s) => (s.read(cx).focus_handle(cx), Textarea::new(s).appearance(false).xsmall().mx(u(-4.)).into_any_element()),
    };
    div().flex_1().min_w_0().py(u(5.)).px(u(10.)).rounded(u(g.r2)).bg(p.raised).border_1()
        .border_color(if focus.is_focused(window) { p.primary } else { p.line })
        .child(el).into_any_element()
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
