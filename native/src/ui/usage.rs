// Usage: limits against your own budgets, totals, 26 weeks of days, the last 30 days, and where it went (usage.ts).
// the report is read and bucketed by local day on a worker
use super::page::{self, base, btn, cost, group_digits, head, ic, tokens, wide, Btn};
use super::theme::{em, geo, mix, pal, u};
use super::tools::tilde;
use crate::core::sessions;
use crate::state::app::{Pith, View};
use crate::state::tab::now_ms;
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;
use serde_json::Value;
use std::collections::HashMap;
use std::rc::Rc;

const HOUR: f64 = 3_600_000.;
const DAY: f64 = 24. * HOUR;
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const WDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

#[derive(Clone, Copy, Default)]
struct Tot { cost: f64, input: f64, output: f64, cache_read: f64, cache_write: f64, n: u64 }
impl Tot {
    fn add(&mut self, r: &Row) {
        self.cost += r.cost; self.input += r.input; self.output += r.output; self.cache_read += r.cache_read; self.cache_write += r.cache_write; self.n += 1;
    }
    fn tokens(&self) -> f64 { self.input + self.output + self.cache_read + self.cache_write }
}

struct Row { ts: f64, model: String, sess: usize, input: f64, output: f64, cache_read: f64, cache_write: f64, cost: f64 }

#[derive(Clone)]
struct Day { label: SharedString, full: SharedString, month: u8, tot: Tot }

#[derive(Clone)]
struct Sess { file: String, title: String, cwd: String, tokens: f64, cost: f64 }

struct Report {
    calls: usize,
    sessions: Vec<Sess>,
    win5h: Tot, week: Tot, month: Tot, today: Tot, d30: Tot, all: Tot,
    // minutes until the oldest call in the 5 hour window ages out
    ages_out: Option<i64>,
    resets_in: i64,
    days30: Vec<Day>,
    heat: Vec<Day>,
    by_model: Vec<(String, Tot)>,
    by_folder: Vec<(String, Tot)>,
}

// ---------- local time, through libc so days split at the user's midnight ----------
fn tm(ms: f64) -> libc::tm {
    let t = (ms / 1000.).floor() as libc::time_t;
    let mut out: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&t, &mut out) };
    out
}
// local midnight `days` days on from the day `ms` falls in. mktime folds overflowing days into months and minds dst
fn midnight(ms: f64, days: i32) -> f64 {
    let mut t = tm(ms);
    t.tm_mday += days; t.tm_hour = 0; t.tm_min = 0; t.tm_sec = 0; t.tm_isdst = -1;
    unsafe { libc::mktime(&mut t) as f64 * 1000. }
}
fn month_start(ms: f64, months: i32) -> f64 {
    let mut t = tm(ms);
    t.tm_mon += months; t.tm_mday = 1; t.tm_hour = 0; t.tm_min = 0; t.tm_sec = 0; t.tm_isdst = -1;
    unsafe { libc::mktime(&mut t) as f64 * 1000. }
}
fn day_key(ms: f64) -> i64 { let t = tm(ms); t.tm_year as i64 * 400 + t.tm_yday as i64 }
fn day(ms: f64, by_day: &HashMap<i64, Tot>) -> Day {
    let t = tm(ms);
    let m = MONTHS[t.tm_mon.clamp(0, 11) as usize];
    Day {
        label: format!("{m} {}", t.tm_mday).into(),
        full: format!("{}, {m} {}", WDAYS[t.tm_wday.clamp(0, 6) as usize], t.tm_mday).into(),
        month: t.tm_mon as u8,
        tot: by_day.get(&day_key(ms)).copied().unwrap_or_default(),
    }
}

fn build(v: Value) -> Report {
    let now = now_ms();
    let f = |x: &Value| x.as_f64().unwrap_or(0.);
    let sessions: Vec<Sess> = v["sessions"].as_array().into_iter().flatten().map(|s| {
        let u = &s["usage"];
        Sess {
            file: s["file"].as_str().unwrap_or("").into(), title: s["title"].as_str().unwrap_or("").lines().next().unwrap_or("").chars().take(200).collect(),
            cwd: s["cwd"].as_str().unwrap_or("").into(),
            tokens: f(&u["input"]) + f(&u["output"]) + f(&u["cacheRead"]), cost: f(&u["cost"]),
        }
    }).collect();
    let mut rows = vec![];
    for s in v["spend"].as_array().into_iter().flatten() {
        let sess = s["s"].as_u64().unwrap_or(0) as usize;
        for r in s["rows"].as_array().into_iter().flatten() {
            rows.push(Row { ts: f(&r[0]), model: r[1].as_str().unwrap_or("?").into(), sess, input: f(&r[2]), output: f(&r[3]), cache_read: f(&r[4]), cache_write: f(&r[5]), cost: f(&r[6]) });
        }
    }

    let (today0, month0) = (midnight(now, 0), month_start(now, 0));
    let (mut win5h, mut week, mut month, mut today, mut d30, mut all) = Default::default();
    let mut oldest5h = f64::MAX;
    let mut by_day: HashMap<i64, Tot> = HashMap::new();
    let mut by_model: HashMap<String, Tot> = HashMap::new();
    let mut by_folder: HashMap<String, Tot> = HashMap::new();
    for r in &rows {
        let add = |t: &mut Tot| t.add(r);
        add(&mut all);
        if r.ts >= now - 5. * HOUR { add(&mut win5h); oldest5h = oldest5h.min(r.ts); }
        if r.ts >= now - 7. * DAY { add(&mut week); }
        if r.ts >= now - 30. * DAY { add(&mut d30); }
        if r.ts >= month0 { add(&mut month); }
        if r.ts >= today0 { add(&mut today); }
        add(by_day.entry(day_key(r.ts)).or_default());
        add(by_model.entry(r.model.clone()).or_default());
        add(by_folder.entry(sessions.get(r.sess).map(|s| s.cwd.clone()).unwrap_or_else(|| "?".into())).or_default());
    }
    let days30 = (0..30).rev().map(|i| day(midnight(now, -i), &by_day)).collect();
    // 26 weeks back from today, the first one from its monday
    let first = midnight(now, -(26 * 7 - 1));
    let first = midnight(first, -((tm(first).tm_wday + 6) % 7));
    let mut heat = vec![];
    for i in 0.. {
        let d = midnight(first, i);
        if d > today0 + HOUR { break; }
        heat.push(day(d, &by_day));
    }
    let top = |m: HashMap<String, Tot>| {
        let mut v: Vec<(String, Tot)> = m.into_iter().collect();
        v.sort_by(|a, b| b.1.cost.total_cmp(&a.1.cost).then(b.1.tokens().total_cmp(&a.1.tokens())));
        v.truncate(10);
        v
    };
    Report {
        calls: rows.len(), sessions, win5h, week, month, today, d30, all,
        ages_out: (oldest5h < f64::MAX).then(|| ((oldest5h + 5. * HOUR - now) / 60_000.).round().max(1.) as i64),
        resets_in: ((month_start(now, 1) - now) / DAY).ceil() as i64,
        days30, heat, by_model: top(by_model), by_folder: top(by_folder),
    }
}

// the hover card over the heatmap and the bars (.tip)
#[derive(Clone, PartialEq)]
struct Tip { key: usize, title: SharedString, body: SharedString, at: Point<Pixels> }

pub struct UsagePage {
    report: Option<Rc<Report>>,
    loading: bool,
    tokens: bool,
    tip: Option<Tip>,
    last_view: View,
    _subs: Vec<Subscription>,
}

impl UsagePage {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let app = Pith::global(cx);
        // read again every time the page comes back into view
        let sub = cx.observe(&app, |this: &mut Self, app, cx| {
            let v = app.read(cx).view;
            if v == View::Usage && this.last_view != View::Usage { this.load(cx); }
            this.last_view = v;
        });
        let mut page = UsagePage { report: None, loading: false, tokens: false, tip: None, last_view: View::Usage, _subs: vec![sub] };
        page.load(cx);
        page
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        if self.loading { return; }
        self.loading = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let r = cx.background_spawn(async { build(sessions::usage_report()) }).await;
            let _ = this.update(cx, |this, cx| { this.report = Some(Rc::new(r)); this.loading = false; cx.notify(); });
        }).detach();
    }

    // a hover target that shows the tip by the pointer, and hides it when left
    fn tipped(&self, el: Stateful<Div>, key: usize, title: SharedString, body: SharedString, cx: &mut Context<Self>) -> Stateful<Div> {
        el.on_mouse_move(cx.listener(move |this, e: &MouseMoveEvent, _, cx| {
            let t = Tip { key, title: title.clone(), body: body.clone(), at: e.position };
            if this.tip.as_ref() != Some(&t) { this.tip = Some(t); cx.notify(); }
        }))
        .on_hover(cx.listener(move |this, h: &bool, _, cx| {
            if !*h && this.tip.as_ref().is_some_and(|t| t.key == key) { this.tip = None; cx.notify(); }
        }))
    }

    // budgets are the user's own, pi has no limits to report. state gets a word, never just a colour
    fn limit(&self, id: &'static str, label: &str, spent: f64, budget: f64, note: String, cx: &mut Context<Self>) -> AnyElement {
        let (p, g) = (pal(cx), geo(cx));
        let pct = if budget > 0. { spent / budget * 100. } else { 0. };
        let state = if budget <= 0. { "" } else if pct >= 100. { "over" } else if pct >= 80. { "close" } else { "ok" };
        let (word, color) = match state { "over" => ("over budget", p.red), "close" => ("near budget", p.gold), _ => ("within budget", p.green) };
        let on_color = match state { "over" => p.red, "close" => p.gold, _ => p.primary };
        let border = match state { "over" => mix(p.red, 0.6, p.line), "close" => mix(p.gold, 0.6, p.line), _ => p.line };
        let on = if budget > 0. { ((pct / 100. * 24.).round() as usize).min(24) } else { 0 };
        let app = Pith::global(cx);
        div().border(u(1.)).border_color(border).rounded(u(g.round)).py(u(12.)).px(u(14.)).bg(p.surface_low)
            .child(div().flex().justify_between().items_baseline()
                .child(div().text_size(em(cx, 0.82)).text_color(p.dim).child(label.to_string()))
                .when(!state.is_empty(), |d| d.child(div().text_size(em(cx, 0.8)).px(u(6.)).rounded(u(3.)).bg(p.high).text_color(color).child(word))))
            .child(div().mt(u(4.)).mb(u(8.)).flex().items_baseline().gap(u(6.))
                .child(big(cost(spent), cx))
                .child(if budget > 0. { div().text_color(p.dim).child(format!(" of {}", cost(budget))).into_any_element() } else {
                    page::link(id, "set a budget", cx).on_click(move |_, _, cx| app.update(cx, |a, cx| a.show(View::Settings, cx))).into_any_element()
                }))
            .child(div().flex().gap(u(2.)).when(budget <= 0., |d| d.opacity(0.5))
                .children((0..24).map(|i| div().flex_1().h(u(10.)).rounded(u(2.)).bg(if i < on { on_color } else { p.high }))))
            .child(div().mt(u(6.)).text_size(em(cx, 0.82)).text_color(p.dim).child(if budget > 0. { format!("{}% · {note}", pct.round()) } else { note }))
            .into_any_element()
    }

    fn current(&self, narrow: bool, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = pal(cx);
        let t = Pith::global(cx).read(cx).active.clone()?;
        let t = t.read(cx);
        let st = t.stats.clone();
        if st.is_null() { return None; }
        let f = |v: &Value| v.as_f64().unwrap_or(0.);
        let cu = &st["contextUsage"];
        let pct = f(&cu["percent"]);
        let ctx_line = if cu["tokens"].is_number() { format!("{} of {} · {}%", tokens(f(&cu["tokens"])), tokens(f(&cu["contextWindow"])), pct.round()) } else { "known after the next reply".into() };
        let kv = |k: &str, v: String, cx: &App| fr(1.).min_w_0().child(div().text_size(em(cx, 0.82)).text_color(p.dim).child(k.to_string())).child(div().child(v));
        let mut grid = div().flex().flex_wrap().items_end().gap(u(14.));
        let cells = vec![
            fr(2.).min_w_0().child(div().text_size(em(cx, 0.82)).text_color(p.dim).child("context"))
                .child(div().h(u(8.)).mt(u(6.)).mb(u(4.)).rounded(u(4.)).bg(p.high).overflow_hidden()
                    .child(div().h_full().rounded(u(4.)).bg(p.primary).w(relative((pct.min(100.) / 100.) as f32))))
                .child(div().text_color(p.dim).child(ctx_line)),
            kv("input", tokens(f(&st["tokens"]["input"])), cx), kv("output", tokens(f(&st["tokens"]["output"])), cx),
            kv("cache read", tokens(f(&st["tokens"]["cacheRead"])), cx), kv("cache write", tokens(f(&st["tokens"]["cacheWrite"])), cx),
            kv("cost", cost(f(&st["cost"])), cx), kv("tool calls", format!("{}", f(&st["toolCalls"]) as u64), cx),
        ];
        // at narrow widths it's three to a row
        if narrow {
            grid = div().grid().grid_cols(3).items_end().gap(u(14.)).children(cells.into_iter().map(|c| c.flex_none()));
        } else {
            grid = grid.flex_nowrap().children(cells);
        }
        Some(block(cx).mb(u(22.))
            .child(sec(cx).child(div().child("this session")).child(div().text_color(p.dim).min_w_0().truncate().child(t.title())))
            .child(grid).into_any_element())
    }

    fn heat(&self, r: &Report, cx: &mut Context<Self>) -> AnyElement {
        let p = pal(cx);
        // levels by quartile of the days that had any spend, so one huge day doesn't wash the rest out
        let mut vals: Vec<f64> = r.heat.iter().map(|d| d.tot.cost).filter(|v| *v > 0.).collect();
        vals.sort_by(|a, b| a.total_cmp(b));
        let q = |f: f64| vals.get(((f * vals.len() as f64).floor() as usize).min(vals.len().saturating_sub(1))).copied().unwrap_or(0.);
        let steps = [q(0.25), q(0.5), q(0.75), q(0.9)];
        let level = |v: f64| if v <= 0. { 0 } else { 1 + steps.iter().filter(|s| v > **s).count() };
        const CELL: f32 = 13.;
        const GAP: f32 = 3.;
        let weeks = r.heat.len().div_ceil(7);
        let (vw, vh) = (weeks as f32 * (CELL + GAP) + 30., 7. * (CELL + GAP) + 18.);
        let hl = |cx: &App| div().absolute().text_size(u(10.)).line_height(u(13.)).text_color(pal(cx).dim).whitespace_nowrap();
        let mut svg = div().relative().flex_none().w(u(vw)).h(u(vh));
        let mut last_month = 255u8;
        let text = p.text;
        for (i, d) in r.heat.iter().enumerate() {
            let (w, dow) = ((i / 7) as f32, i % 7);
            let x = 30. + w * (CELL + GAP);
            let body: SharedString = if d.tot.n > 0 { format!("{} · {} tokens · {} calls", cost(d.tot.cost), tokens(d.tot.tokens()), d.tot.n).into() } else { "nothing".into() };
            let cell = div().id(("hc", i)).absolute().left(u(x)).top(u(18. + dow as f32 * (CELL + GAP))).size(u(CELL)).rounded(u(3.))
                .bg(heat_color(level(d.tot.cost), cx)).border(u(1.5)).border_color(text.opacity(0.)).hover(move |s| s.border_color(text));
            svg = svg.child(self.tipped(cell, i, d.full.clone(), body, cx));
            if dow == 0 && d.month != last_month {
                last_month = d.month;
                svg = svg.child(hl(cx).left(u(x)).top(u(1.)).child(MONTHS[d.month as usize]));
            }
        }
        for (i, l) in ["mon", "", "wed", "", "fri", "", ""].into_iter().enumerate() {
            if !l.is_empty() { svg = svg.child(hl(cx).left_0().top(u(18. + i as f32 * (CELL + GAP))).child(l)); }
        }
        let key = div().flex().gap(u(3.)).items_center().justify_end().text_size(em(cx, 0.78)).mt(u(4.)).max_w(u(900.))
            .child(div().mx(u(4.)).text_color(p.dim).child("less"))
            .children((0..6).map(|l| div().size(u(11.)).rounded(u(2.)).bg(heat_color(l, cx))))
            .child(div().mx(u(4.)).text_color(p.dim).child("more"));
        div().child(svg).child(key).into_any_element()
    }

    // the 30 day bars, laid out in the reference's 1000 wide viewbox and scaled to the block
    fn bars(&self, r: &Report, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let p = pal(cx);
        let by_tokens = self.tokens;
        let val = |t: &Tot| if by_tokens { t.tokens() } else { t.cost };
        let max = r.days30.iter().map(|d| val(&d.tot)).fold(if by_tokens { 1. } else { 0.01 }, f64::max);
        let (w, h, pad, top) = (1000f32, 180f32, 46f32, 12f32);
        let vh = h + 24. + top;
        let bw = (w - pad) / 30.;
        let s = width / w;
        let x = |v: f32| relative(v / w);
        let y = |v: f32| relative((v + top) / vh);
        let fmt = |v: f64| if by_tokens { tokens(v) } else { cost(v) };
        let hl = |cx: &App| div().absolute().text_size(u(10. * s)).line_height(u(12. * s)).text_color(pal(cx).dim).whitespace_nowrap();
        let mut svg = div().relative().w_full().aspect_ratio(w / vh).max_h(u(240.));
        for f in [0.5f32, 1.] {
            let ly = h - f * h;
            svg = svg.child(dashes(p.rule, s).absolute().left(x(pad)).right_0().top(y(ly)).h(u(1.)))
                .child(hl(cx).left_0().top(y(ly + 4. - 10.)).child(fmt(max * f as f64)));
        }
        svg = svg.child(div().absolute().left(x(pad)).right_0().top(y(h)).h(u(1.)).bg(p.line));
        let hover = p.hover;
        for (i, d) in r.days30.iter().enumerate() {
            let v = val(&d.tot);
            let bh = if v > 0. { ((v / max) as f32 * (h - 6.)).max(3.) } else { 0. };
            let bx = pad + i as f32 * bw + 1.;
            if bh > 0. {
                svg = svg.child(div().absolute().left(x(bx)).w(x(bw - 2.)).top(y(h - bh)).h(relative(bh / vh)).bg(p.primary).rounded_t(u(4f32.min((bw - 2.) / 2.).min(bh) * s)));
            }
            let body: SharedString = if d.tot.n > 0 { format!("{} · {} in · {} out · {} cached", cost(d.tot.cost), tokens(d.tot.input), tokens(d.tot.output), tokens(d.tot.cache_read)).into() } else { "nothing".into() };
            let hit = div().id(("bar", i)).absolute().left(x(pad + i as f32 * bw)).w(x(bw)).top(y(0.)).h(relative(h / vh)).hover(move |st| st.bg(hover));
            svg = svg.child(self.tipped(hit, 1000 + i, d.full.clone(), body, cx));
            if i % 5 == 4 || i == 0 {
                let (label, cx0) = if i == 29 { ("today".to_string(), w) } else { (d.label.to_string(), pad + i as f32 * bw + bw / 2.) };
                // a 100 unit box centred on the tick (or ending at it for "today") stands in for svg's text-anchor
                let lb = hl(cx).top(y(h + 16. - 10.)).w(x(100.)).flex();
                svg = svg.child(if i == 29 { lb.left(x(cx0 - 100.)).justify_end() } else { lb.left(x(cx0 - 50.)).justify_center() }.child(label));
            }
        }
        svg.into_any_element()
    }

    // by model / by folder: name, where, a magnitude bar, then in, out, cached and cost columns
    fn table(&self, list: &[(String, Tot)], name: impl Fn(&str) -> (String, String), cx: &mut Context<Self>) -> AnyElement {
        let p = pal(cx);
        if list.is_empty() { return div().text_color(p.dim).child("nothing yet").into_any_element(); }
        let max = list.iter().map(|x| x.1.cost).fold(0.0001, f64::max);
        let rows: Vec<(String, String, Tot)> = list.iter().map(|(k, t)| { let (a, b) = name(k); (a, b, *t) }).collect();
        // auto table layout: columns as wide as their content, the spare width shared out in proportion
        let ch = geo(cx).fs * 0.9 * 0.6;
        let colw = |f: &dyn Fn(&Tot) -> String, h: &str| rows.iter().map(|r| f(&r.2).chars().count()).max().unwrap_or(0).max(h.len()) as f32 * ch + 14.;
        let cols: [(&str, Box<dyn Fn(&Tot) -> String>); 4] = [
            ("in", Box::new(|t: &Tot| tokens(t.input))), ("out", Box::new(|t: &Tot| tokens(t.output))),
            ("cached", Box::new(|t: &Tot| tokens(t.cache_read))), ("cost", Box::new(|t: &Tot| cost(t.cost))),
        ];
        let ws: Vec<f32> = cols.iter().map(|(h, f)| colw(f.as_ref(), h)).collect();
        let namew = rows.iter().map(|r| (r.0.chars().count() as f32 * ch).max((r.1.chars().count().min(34) as f32) * ch * 0.82 / 0.9)).fold(0., f32::max);
        let col = |w: f32| fr(w).flex_basis(u(w)).min_w(u(w));
        let header = div().flex().text_size(em(cx, 0.9 * 0.85)).text_color(p.dim).pb(u(4.))
            .child(col(namew))
            .children(cols.iter().zip(&ws).map(|((h, _), w)| col(*w).items_end().child(h.to_string())));
        let body = rows.iter().map(|(n, sub, t)| {
            div().flex().border_t(u(1.)).border_color(p.rule)
                .child(col(namew).min_w_0().py(u(5.))
                    .child(div().text_color(p.text).child(n.clone()))
                    .child(div().text_size(em(cx, 0.9 * 0.82)).text_color(p.dim).max_w(u(34. * ch * 0.82 / 0.9)).whitespace_nowrap().overflow_hidden().text_ellipsis_start().child(sub.clone()))
                    .child(div().h(u(4.)).mt(u(4.)).max_w(u(220.)).rounded(u(2.)).bg(p.high)
                        .child(div().h_full().min_w(u(2.)).rounded(u(2.)).bg(p.primary).w(relative((t.cost / max) as f32)))))
                .children(cols.iter().zip(&ws).enumerate().map(|(i, ((_, f), w))| col(*w).py(u(5.)).items_end().whitespace_nowrap()
                    .text_color(if i == 3 { p.text } else { p.mid }).child(f(t))))
        });
        div().w_full().text_size(em(cx, 0.9)).child(header).children(body).into_any_element()
    }

    fn top_sessions(&self, r: &Report, cx: &mut Context<Self>) -> AnyElement {
        let p = pal(cx);
        let mut list: Vec<&Sess> = r.sessions.iter().collect();
        list.sort_by(|a, b| b.cost.total_cmp(&a.cost));
        list.truncate(8);
        if list.is_empty() { return div().text_color(p.dim).child("nothing yet").into_any_element(); }
        let ch = geo(cx).fs * 0.92 * 0.6;
        let app = Pith::global(cx);
        let hover = p.hover;
        div().flex().flex_col().children(list.into_iter().enumerate().map(|(i, s)| {
            let (a, file) = (app.clone(), s.file.clone());
            div().id(("u-trow", i)).flex().gap(u(12.)).py(u(4.)).px(u(6.)).rounded(u(5.)).text_size(em(cx, 0.92)).cursor_pointer().hover(move |st| st.bg(hover))
                .child(div().flex_1().min_w_0().truncate().child(s.title.clone()))
                .child(div().flex_none().w(u(14. * ch)).truncate().text_color(p.dim).child(base(&s.cwd)))
                .child(div().flex_none().w(u(8. * ch)).flex().justify_end().text_color(p.mid).child(tokens(s.tokens)))
                .child(div().flex_none().w(u(9. * ch)).flex().justify_end().text_color(p.text).child(cost(s.cost)))
                .on_click(move |_, _, cx| {
                    let known = a.read(cx).sessions.iter().any(|x| x.file == file);
                    if known && !file.is_empty() { a.update(cx, |a, cx| a.open(&file, cx)); }
                })
        })).into_any_element()
    }
}

// .bars .grid: a 1px rule, dashed 2 on 4 off in viewbox units
fn dashes(color: Hsla, s: f32) -> Canvas<()> {
    canvas(|_, _, _| (), move |b, _, window, _| {
        let (on, step) = (px(2. * s), px(6. * s));
        let mut x = b.origin.x;
        while x < b.origin.x + b.size.width {
            window.paint_quad(fill(Bounds { origin: point(x, b.origin.y), size: size(on.min(b.origin.x + b.size.width - x), b.size.height) }, color));
            x += step;
        }
    })
}
// .u-big
fn big(text: String, cx: &App) -> Div { div().text_size(em(cx, 1.45)).font_weight(FontWeight::BOLD).child(text) }
// a grid track of `n` fr: grows in proportion from nothing, like grid-template-columns does
fn fr(n: f32) -> Div { div().flex().flex_col().flex_basis(relative(0.)).flex_shrink(1.).flex_grow(n) }
// .u-block and its .u-sec heading
fn block(cx: &App) -> Div { let (p, g) = (pal(cx), geo(cx)); div().relative().border(u(1.)).border_color(p.line).rounded(u(g.round)).pt(u(12.)).px(u(16.)).pb(u(14.)) }
fn sec(cx: &App) -> Div { div().flex().items_baseline().gap(u(12.)).mb(u(10.)).text_size(em(cx, 0.92)) }
// .hc.l0 … .l5: one hue, light to dark
fn heat_color(level: usize, cx: &App) -> Hsla {
    let p = pal(cx);
    match level { 0 => p.high, 5 => p.primary, l => mix(p.primary, [0.22, 0.42, 0.62, 0.82][l - 1], p.high) }
}
impl Render for UsagePage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (p, g) = (pal(cx), geo(cx));
        let Some(r) = self.report.clone() else {
            return page::page("usage", vec![head("usage", None, vec![], cx).into_any_element(), wide().text_color(p.dim).child("reading sessions…").into_any_element()]).into_any_element();
        };
        let vw = f32::from(window.viewport_size().width) / g.scale;
        let narrow = vw < 1100.;
        let reload = btn("us-reload", Btn::Ghost, true, cx).child(ic("refresh", cx)).child("reload").on_click(cx.listener(|this, _, _, cx| this.load(cx)));
        let summary: SharedString = format!("{} sessions · {} model calls on record", r.sessions.len(), group_digits(r.calls as u64)).into();
        let b = Pith::global(cx).read(cx).settings["budget"].clone();
        let budget = |k: &str| b[k].as_f64().unwrap_or(0.);

        let ages = r.ages_out.map(|m| format!("oldest call ages out in {m}m")).unwrap_or_else(|| "nothing in the window".into());
        let limits = div().grid().grid_cols(if narrow { 2 } else { 3 }).gap(u(12.))
            .child(self.limit("ul-5h", "5 hour window", r.win5h.cost, budget("window5h"), ages, cx))
            .child(self.limit("ul-week", "last 7 days", r.week.cost, budget("week"), "rolling".into(), cx))
            .child(self.limit("ul-month", "this month", r.month.cost, budget("month"), format!("resets in {}d", r.resets_in), cx));

        let tile = |i: usize, label: &str, t: Tot, cx: &App| div().py(u(12.)).px(u(16.)).when(i > 0, |d| d.border_l(u(1.)).border_color(p.rule))
            .child(div().text_size(em(cx, 0.82)).text_color(p.dim).child(label.to_string()))
            .child(big(cost(t.cost), cx))
            .child(div().text_size(em(cx, 0.84)).text_color(p.dim).child(format!("{} tokens · {} calls", tokens(t.tokens()), group_digits(t.n))));
        let tiles = div().grid().grid_cols(if narrow { 2 } else { 4 }).border(u(1.)).border_color(p.line).rounded(u(g.round))
            .child(tile(0, "today", r.today, cx)).child(tile(1, "7 days", r.week, cx)).child(tile(2, "30 days", r.d30, cx)).child(tile(3, "all time", r.all, cx));

        let tokens_on = self.tokens;
        let this = cx.entity().downgrade();
        let metric = page::segs("us-metric", &["cost", "tokens"], tokens_on as usize, move |i, _, cx| { let _ = this.update(cx, |t, cx| { t.tokens = i == 1; cx.notify(); }); }, cx);
        // the block's inner width: the main window less the page's padding and the block's own
        let main_w = vw - 2. * g.gap - if Pith::global(cx).read(cx).sidebar { 264. + g.gap } else { 0. } - 2. * g.bw;
        let inner = (main_w - 68.).min(page::MAX_W) - 34.;
        let models = self.table(&r.by_model, |k| (k.split_once('/').map(|x| x.1).unwrap_or(k).to_string(), k.split('/').next().unwrap_or("").to_string()), cx);
        let folders = self.table(&r.by_folder, |k| { let b = base(k); (if b.is_empty() { "/".into() } else { b }, tilde(k)) }, cx);

        let mut kids: Vec<AnyElement> = vec![
            head("usage", Some(summary), vec![reload.into_any_element()], cx).into_any_element(),
            wide().mb(u(22.)).child(limits).into_any_element(),
            wide().mb(u(22.)).child(tiles).into_any_element(),
        ];
        if let Some(c) = self.current(narrow, cx) { kids.push(wide().child(c).into_any_element()); }
        kids.push(wide().child(block(cx).mb(u(22.))
            .child(sec(cx).child(div().child("every day, last 26 weeks")).child(div().text_color(p.dim).child("darker is more spend")))
            .child(self.heat(&r, cx))).into_any_element());
        kids.push(wide().child(block(cx).mb(u(22.))
            .child(sec(cx).items_center().child(div().child("last 30 days")).child(div().flex_1()).child(metric))
            .child(self.bars(&r, inner, cx))).into_any_element());
        let col = |title: &str, body: AnyElement, cx: &App| block(cx).min_w_0().child(sec(cx).child(title.to_string())).child(body);
        kids.push(wide().mb(u(22.)).child(div().grid().grid_cols(if narrow { 1 } else { 2 }).gap(u(14.))
            .child(col("by model", models, cx)).child(col("by folder", folders, cx))).into_any_element());
        kids.push(wide().child(block(cx).mb(u(22.)).child(sec(cx).child("most expensive sessions")).child(self.top_sessions(&r, cx))).into_any_element());

        let tip = self.tip.clone().map(|t| deferred(anchored().position(t.at + point(px(14.), px(14.))).snap_to_window_with_margin(px(8.)).child(
            div().py(u(6.)).px(u(10.)).bg(p.raised).border(u(1.)).border_color(p.line).rounded(u(g.r2)).text_size(em(cx, 0.84)).max_w(u(280.)).shadow_lg()
                .child(div().font_weight(FontWeight::BOLD).mb(u(2.)).child(t.title)).child(div().child(t.body)))).with_priority(1));
        div().size_full().child(page::page("usage", kids)).children(tip).into_any_element()
    }
}
