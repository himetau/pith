// Usage: spend against your own budgets, the last 30 days, a year of days, where it went, and the
// sessions that cost most. the report is read and bucketed by local day on a worker
use super::page::{self, cost, empty, group_digits, heading, tokens};
use super::theme::pal;
use super::tools::tilde;
use crate::core::sessions;
use crate::state::app::{Pith, View};
use crate::state::tab::now_ms;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::chart::BarChart;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Icon, Sizable};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;
use serde_json::Value;
use std::collections::HashMap;
use std::rc::Rc;

const HOUR: f64 = 3_600_000.;
const DAY: f64 = 24. * HOUR;
const HEAT_WEEKS: i32 = 53;
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
struct Day { label: SharedString, full: SharedString, month: u8, mday: u8, tot: Tot }

#[derive(Clone)]
struct Sess { file: String, title: String, cwd: String, updated: f64, calls: u64, tokens: f64, cost: f64 }

struct Report {
    calls: usize,
    sessions: Vec<Sess>,
    win5h: Tot, today: Tot, week: Tot, month: Tot, all: Tot,
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
        month: t.tm_mon as u8, mday: t.tm_mday as u8,
        tot: by_day.get(&day_key(ms)).copied().unwrap_or_default(),
    }
}

fn build(v: Value) -> Report {
    let now = now_ms();
    let f = |x: &Value| x.as_f64().unwrap_or(0.);
    let mut sessions: Vec<Sess> = v["sessions"].as_array().into_iter().flatten().map(|s| {
        let u = &s["usage"];
        let title = s["title"].as_str().unwrap_or("").lines().next().unwrap_or("").chars().take(140).collect::<String>();
        Sess {
            file: s["file"].as_str().unwrap_or("").into(), title: if title.trim().is_empty() { "Untitled".into() } else { title },
            cwd: s["cwd"].as_str().unwrap_or("").into(), updated: f(&s["updated"]), calls: 0,
            tokens: f(&u["input"]) + f(&u["output"]) + f(&u["cacheRead"]) + f(&u["cacheWrite"]), cost: f(&u["cost"]),
        }
    }).collect();
    let mut rows = vec![];
    for s in v["spend"].as_array().into_iter().flatten() {
        let sess = s["s"].as_u64().unwrap_or(0) as usize;
        for r in s["rows"].as_array().into_iter().flatten() {
            rows.push(Row { ts: f(&r[0]), model: r[1].as_str().unwrap_or("?").into(), sess, input: f(&r[2]), output: f(&r[3]), cache_read: f(&r[4]), cache_write: f(&r[5]), cost: f(&r[6]) });
        }
    }
    for r in &rows { if let Some(s) = sessions.get_mut(r.sess) { s.calls += 1; } }

    let (today0, month0) = (midnight(now, 0), month_start(now, 0));
    let (mut win5h, mut today, mut week, mut month, mut all) = Default::default();
    let mut oldest5h = f64::MAX;
    let mut by_day: HashMap<i64, Tot> = HashMap::new();
    let mut by_model: HashMap<String, Tot> = HashMap::new();
    let mut by_folder: HashMap<String, Tot> = HashMap::new();
    for r in &rows {
        let add = |t: &mut Tot| t.add(r);
        add(&mut all);
        if r.ts >= now - 5. * HOUR { add(&mut win5h); oldest5h = oldest5h.min(r.ts); }
        if r.ts >= today0 { add(&mut today); }
        if r.ts >= now - 7. * DAY { add(&mut week); }
        if r.ts >= month0 { add(&mut month); }
        add(by_day.entry(day_key(r.ts)).or_default());
        add(by_model.entry(r.model.clone()).or_default());
        add(by_folder.entry(sessions.get(r.sess).map(|s| s.cwd.clone()).unwrap_or_else(|| "?".into())).or_default());
    }
    let days30 = (0..30).rev().map(|i| day(midnight(now, -i), &by_day)).collect();
    // whole weeks, monday first, ending with today
    let first = midnight(now, -(HEAT_WEEKS * 7 - 1));
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
        v.truncate(8);
        v
    };
    Report {
        calls: rows.len(), sessions, win5h, today, week, month, all,
        ages_out: (oldest5h < f64::MAX).then(|| ((oldest5h + 5. * HOUR - now) / 60_000.).ceil().max(1.) as i64),
        resets_in: ((month_start(now, 1) - now) / DAY).ceil() as i64,
        days30, heat, by_model: top(by_model), by_folder: top(by_folder),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Sort { Title, Calls, Tokens, Cost, Updated }

pub struct UsagePage {
    report: Option<Rc<Report>>,
    loading: bool,
    tokens: bool,
    sort: (Sort, bool),
    all_sessions: bool,
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
        let mut page = UsagePage { report: None, loading: false, tokens: false, sort: (Sort::Cost, true), all_sessions: false, last_view: View::Usage, _subs: vec![sub] };
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

    // one cell of the strip up top: what went out in a window, against its budget when there is one
    fn stat(&self, id: &'static str, label: &'static str, t: Tot, budget: Option<f64>, note: String, cx: &mut Context<Self>) -> AnyElement {
        let p = pal(cx);
        let mut cell = v_flex().flex_1().min_w_0().px_4().py_3().gap_1()
            .child(div().text_xs().text_color(p.dim).child(label))
            .child(div().text_size(px(22.)).font_weight(FontWeight::SEMIBOLD).text_color(cx.theme().foreground).child(cost(t.cost)))
            .child(div().text_xs().text_color(p.dim).truncate().child(format!("{} tokens · {} calls", tokens(t.tokens()), group_digits(t.n))));
        match budget {
            Some(b) if b > 0. => {
                let pct = t.cost / b * 100.;
                let (word, color) = if pct >= 100. { ("Over budget", p.red) } else if pct >= 80. { ("Near budget", p.gold) } else { ("Within budget", p.green) };
                cell = cell
                    .child(div().mt_2().child(Progress::new(id).value(pct as f32).color(if pct >= 80. { color } else { cx.theme().primary })))
                    .child(h_flex().gap_1().text_xs().child(div().text_color(color).child(word)).child(div().text_color(p.dim).truncate().child(format!("· {}% of {}", pct.round(), cost(b)))))
                    .child(div().text_xs().text_color(p.dim).truncate().child(note));
            }
            Some(_) => {
                let app = Pith::global(cx);
                cell = cell
                    .child(div().mt_2().h(px(6.)).rounded_full().bg(cx.theme().muted))
                    .child(h_flex().text_xs().child(div().id(id).text_color(cx.theme().link).cursor_pointer().hover(|s| s.underline()).child("Set a budget")
                        .on_click(move |_, _, cx| app.update(cx, |a, cx| a.show(View::Settings, cx)))))
                    .child(div().text_xs().text_color(p.dim).truncate().child(note));
            }
            None => { cell = cell.child(div().mt_2().h(px(6.))).child(div().text_xs().child(" ")).child(div().text_xs().text_color(p.dim).child(note)); }
        }
        cell.into_any_element()
    }

    fn chart(&self, r: &Report, cx: &mut Context<Self>) -> AnyElement {
        let accent = cx.theme().primary;
        let by_tokens = self.tokens;
        let chart = BarChart::new(r.days30.clone())
            .band(|d: &Day| d.label.clone())
            .value(move |d: &Day| if by_tokens { d.tot.tokens() } else { d.tot.cost })
            .name(if by_tokens { "Tokens" } else { "Spend" })
            .fill(move |_, _, _, _| accent)
            .corner_radii(Corners { top_left: px(3.), top_right: px(3.), bottom_left: px(0.), bottom_right: px(0.) })
            .value_axis(true)
            .value_tick_count(3)
            .value_tick_format(move |v| if by_tokens { tokens(v) } else { cost(v) })
            .grid_dashed(false)
            .band_tick_count(6)
            .padding_inner(0.3)
            .tooltip_title(|d: &Day| d.full.clone())
            .tooltip_value(move |d: &Day, _| if d.tot.n == 0 { "Nothing".into() } else {
                format!("{} · {} tokens · {} calls", cost(d.tot.cost), tokens(d.tot.tokens()), d.tot.n).into()
            })
            .id(if by_tokens { "usage-days-tokens" } else { "usage-days-cost" });
        div().h(px(220.)).w_full().child(chart).into_any_element()
    }

    fn heat(&self, r: &Report, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let p = pal(cx);
        let accent = cx.theme().primary;
        let empty_cell = cx.theme().muted;
        // levels by quartile of the days that had any spend, so one huge day doesn't wash the rest out
        let mut vals: Vec<f64> = r.heat.iter().map(|d| d.tot.cost).filter(|v| *v > 0.).collect();
        vals.sort_by(|a, b| a.total_cmp(b));
        let q = |f: f64| vals.get(((f * vals.len() as f64) as usize).min(vals.len().saturating_sub(1))).copied().unwrap_or(0.);
        let steps = [q(0.25), q(0.5), q(0.75), q(0.9)];
        let shade = move |v: f64| -> Hsla {
            if v <= 0. { return empty_cell; }
            let lvl = steps.iter().filter(|s| v > **s).count();
            accent.opacity([0.3, 0.48, 0.66, 0.84, 1.][lvl])
        };
        const CELL: f32 = 12.;
        const GAP: f32 = 3.;
        // as many recent weeks as fit, so today is always on screen
        let weeks: Vec<&[Day]> = r.heat.chunks(7).collect();
        let fit = (((width - 28.) / (CELL + GAP)).floor() as usize).max(8);
        let weeks = weeks[weeks.len().saturating_sub(fit)..].to_vec();
        let mut last_month = 255u8;
        let cols = weeks.iter().enumerate().map(|(w, days)| {
            let label = if days[0].month != last_month && days[0].mday <= 7 { last_month = days[0].month; MONTHS[days[0].month as usize] } else { "" };
            v_flex().gap(px(GAP)).w(px(CELL)).flex_none()
                .child(div().h(px(14.)).text_size(px(10.)).text_color(p.dim).whitespace_nowrap().child(label))
                .children(days.iter().enumerate().map(|(i, d)| {
                    let tip = if d.tot.n == 0 { format!("{}\nNothing", d.full) } else { format!("{}\n{} · {} tokens · {} calls", d.full, cost(d.tot.cost), tokens(d.tot.tokens()), d.tot.n) };
                    div().id(("heat", w * 7 + i)).size(px(CELL)).rounded(px(3.)).bg(shade(d.tot.cost))
                        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                }))
        }).collect::<Vec<_>>();
        let rows = v_flex().gap(px(GAP)).w(px(28.)).flex_none().text_size(px(10.)).text_color(p.dim)
            .child(div().h(px(14.)))
            .children(["Mon", "", "Wed", "", "Fri", "", ""].into_iter().map(|l| div().h(px(CELL)).flex().items_center().child(l)));
        let legend = h_flex().gap(px(3.)).items_center().text_xs().text_color(p.dim)
            .child(div().mr_1().child("Less"))
            .child(div().size(px(10.)).rounded(px(2.)).bg(empty_cell))
            .children([0.3, 0.48, 0.66, 0.84, 1.].into_iter().map(move |o| div().size(px(10.)).rounded(px(2.)).bg(accent.opacity(o))))
            .child(div().ml_1().child("More"));
        v_flex().gap_2()
            .child(h_flex().items_start().gap(px(GAP)).child(rows).children(cols))
            .child(h_flex().child(div().flex_1()).child(legend))
            .into_any_element()
    }

    fn breakdown(&self, id: &'static str, list: &[(String, Tot)], name: impl Fn(&str) -> (String, String), cx: &mut Context<Self>) -> AnyElement {
        let p = pal(cx);
        if list.is_empty() { return div().p_4().text_sm().text_color(p.dim).child("Nothing yet").into_any_element(); }
        let max = list.iter().map(|x| x.1.cost).fold(0.0001, f64::max);
        let accent = cx.theme().primary;
        let rows = list.iter().enumerate().map(|(i, (k, t))| {
            let (title, sub) = name(k);
            h_flex().id((id, i)).px_4().py_2().gap_4().items_center()
                .child(v_flex().flex_1().min_w_0().gap_0p5()
                    .child(div().text_sm().truncate().text_color(cx.theme().foreground).child(title))
                    .child(div().text_xs().truncate().text_color(p.dim).child(sub))
                    .child(div().mt_1().h(px(3.)).w_full().rounded_full().bg(cx.theme().muted)
                        .child(div().h_full().rounded_full().bg(accent).w(relative((t.cost / max).clamp(0., 1.) as f32)))))
                .child(v_flex().flex_none().w(px(72.)).items_end()
                    .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(cost(t.cost)))
                    .child(div().text_xs().text_color(p.dim).child(tokens(t.tokens()))))
                .into_any_element()
        }).collect();
        page::group(rows, cx).into_any_element()
    }

    fn sessions_table(&self, r: &Report, cx: &mut Context<Self>) -> AnyElement {
        let p = pal(cx);
        let mut list: Vec<&Sess> = r.sessions.iter().filter(|s| s.cost > 0. || s.tokens > 0.).collect();
        let (by, desc) = self.sort;
        list.sort_by(|a, b| {
            let o = match by {
                Sort::Title => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
                Sort::Calls => a.calls.cmp(&b.calls),
                Sort::Tokens => a.tokens.total_cmp(&b.tokens),
                Sort::Cost => a.cost.total_cmp(&b.cost),
                Sort::Updated => a.updated.total_cmp(&b.updated),
            };
            if desc { o.reverse() } else { o }
        });
        if list.is_empty() { return div().p_4().text_sm().text_color(p.dim).child("Nothing yet").into_any_element(); }
        let total = list.len();
        if !self.all_sessions { list.truncate(12); }

        let head = |label: &'static str, s: Sort, w: Option<f32>, cx: &mut Context<Self>| {
            let on = by == s;
            h_flex().id(label).gap_1().items_center().cursor_pointer()
                .map(|d| match w { Some(w) => d.w(px(w)).flex_none().justify_end(), None => d.flex_1().min_w_0() })
                .text_color(if on { cx.theme().foreground } else { p.dim }).hover(|st| st.text_color(cx.theme().foreground))
                .child(label)
                .when(on, |d| d.child(Icon::new(if desc { IconName::ArrowDown } else { IconName::ArrowUp }).xsmall()))
                .on_click(cx.listener(move |this, _, _, cx| {
                    // a new column starts out biggest first, except names which read a to z
                    this.sort = if this.sort.0 == s { (s, !this.sort.1) } else { (s, s != Sort::Title) };
                    cx.notify();
                }))
        };
        let header = h_flex().px_4().h(px(34.)).gap_4().items_center().text_xs().font_weight(FontWeight::MEDIUM)
            .child(head("Session", Sort::Title, None, cx))
            .child(head("Calls", Sort::Calls, Some(56.), cx))
            .child(head("Tokens", Sort::Tokens, Some(72.), cx))
            .child(head("Cost", Sort::Cost, Some(72.), cx))
            .child(head("Last active", Sort::Updated, Some(84.), cx));
        let app = Pith::global(cx);
        let rows = list.into_iter().map(|s| {
            let file = s.file.clone();
            let clickable = !file.is_empty();
            let folder = std::path::Path::new(&s.cwd).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| tilde(&s.cwd));
            let a = app.clone();
            h_flex().id(SharedString::from(format!("us-{}-{}", s.file, s.title))).px_4().h(px(48.)).gap_4().items_center().border_t_1().border_color(p.hairline)
                .when(clickable, |d| d.cursor_pointer().hover(|st| st.bg(cx.theme().muted.opacity(0.6))).on_click(move |_, _, cx| a.update(cx, |a, cx| a.open(&file, cx))))
                .child(v_flex().flex_1().min_w_0()
                    .child(div().text_sm().truncate().text_color(cx.theme().foreground).child(s.title.clone()))
                    .child(div().text_xs().truncate().text_color(p.dim).child(folder)))
                .child(div().w(px(56.)).flex_none().text_right().text_sm().text_color(p.mid).child(group_digits(s.calls)))
                .child(div().w(px(72.)).flex_none().text_right().text_sm().text_color(p.mid).child(tokens(s.tokens)))
                .child(div().w(px(72.)).flex_none().text_right().text_sm().font_weight(FontWeight::MEDIUM).text_color(cx.theme().foreground).child(cost(s.cost)))
                .child(div().w(px(84.)).flex_none().text_right().text_sm().text_color(p.dim).child(if s.updated > 0. { super::workspace::ago(s.updated) } else { String::new() }))
        });
        let more = (total > 12).then(|| {
            let label = if self.all_sessions { "Show fewer".to_string() } else { format!("Show all {total}") };
            h_flex().px_4().py_2().border_t_1().border_color(p.hairline).justify_center()
                .child(Button::new("us-more").label(label).ghost().small().on_click(cx.listener(|this, _, _, cx| { this.all_sessions = !this.all_sessions; cx.notify(); })))
        });
        v_flex().rounded(cx.theme().radius_lg).border_1().border_color(p.hairline).overflow_hidden()
            .child(header).children(rows).children(more)
            .into_any_element()
    }
}

impl Render for UsagePage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = pal(cx);
        let Some(r) = self.report.clone() else {
            return v_flex().size_full().items_center().justify_center().gap_2().text_sm().text_color(p.dim)
                .child(Spinner::new().small()).child("Reading sessions…").into_any_element();
        };
        let reload = Button::new("us-reload").icon(IconName::RefreshCw).label("Reload").ghost().small().loading(self.loading)
            .on_click(cx.listener(|this, _, _, cx| this.load(cx)));
        if r.calls == 0 {
            return page::column("usage", 980., v_flex()
                .child(page::intro("", reload, cx))
                .child(empty(IconName::ChartColumn, "No usage yet", "Spend and tokens show up here once a session has talked to a model. Budgets are set in Settings.", None, cx)))
                .into_any_element();
        }
        let b = Pith::global(cx).read(cx).settings["budget"].clone();
        let budget = |k: &str| b[k].as_f64().unwrap_or(0.);
        let summary = format!("{} sessions · {} model calls · {} all time", group_digits(r.sessions.len() as u64), group_digits(r.calls as u64), cost(r.all.cost));

        let strip = h_flex().rounded(cx.theme().radius_lg).border_1().border_color(p.hairline)
            .child(self.stat("st-5h", "Last 5 hours", r.win5h, Some(budget("window5h")), r.ages_out.map(|m| if m >= 60 { format!("Oldest call ages out in {}h {}m", m / 60, m % 60) } else { format!("Oldest call ages out in {m}m") }).unwrap_or_else(|| "Nothing in the window".into()), cx))
            .child(div().w(px(1.)).self_stretch().bg(p.hairline))
            .child(self.stat("st-today", "Today", r.today, None, "Since midnight".into(), cx))
            .child(div().w(px(1.)).self_stretch().bg(p.hairline))
            .child(self.stat("st-week", "Last 7 days", r.week, Some(budget("week")), "Rolling".into(), cx))
            .child(div().w(px(1.)).self_stretch().bg(p.hairline))
            .child(self.stat("st-month", "This month", r.month, Some(budget("month")), format!("Resets in {}d", r.resets_in), cx));

        let by_tokens = self.tokens;
        let metric = page::segmented("us-metric", &["Cost", "Tokens"], by_tokens as usize, {
            let this = cx.entity().downgrade();
            move |i, _, cx| { let _ = this.update(cx, |t, cx| { t.tokens = i == 1; cx.notify(); }); }
        }, cx);
        let chart = self.chart(&r, cx);
        let sidebar = if Pith::global(cx).read(cx).sidebar { 264. } else { 0. };
        let width = (f32::from(window.viewport_size().width) - sidebar - 64.).min(980.);
        let heat = self.heat(&r, width, cx);
        let models = self.breakdown("bm", &r.by_model, |k| match k.split_once('/') { Some((prov, m)) => (m.to_string(), prov.to_string()), None => (k.to_string(), String::new()) }, cx);
        let folders = self.breakdown("bf", &r.by_folder, |k| {
            let name = std::path::Path::new(k).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "/".into());
            (name, tilde(k))
        }, cx);
        let table = self.sessions_table(&r, cx);

        page::column("usage", 980., v_flex()
            .child(page::intro(summary, reload, cx))
            .child(div().mt_3().child(strip))
            .child(h_flex().mt_8().mb_3().items_center()
                .child(div().flex_1().text_sm().font_weight(FontWeight::SEMIBOLD).child(if by_tokens { "Tokens, last 30 days" } else { "Spend, last 30 days" }))
                .child(metric))
            .child(chart)
            .child(heading("Every day", cx)).child(heat)
            .child(h_flex().gap_6().items_start()
                .child(v_flex().flex_1().min_w_0().child(heading("By model", cx)).child(models))
                .child(v_flex().flex_1().min_w_0().child(heading("By folder", cx)).child(folders)))
            .child(heading("Sessions", cx)).child(table))
            .into_any_element()
    }
}
