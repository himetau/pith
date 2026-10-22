// The working line, opened: every thinking stretch, tool call and reply since the last prompt with how long each took,
// then a live tail of whatever is happening now (chat.ts runSteps, chat.css .run-steps)
use crate::state::tab::{Block, ItemKind, Tab, ToolStatus};
use crate::ui::frame::spin;
use crate::ui::theme::{em, geo, pal};
use crate::ui::theme::u;
use crate::ui::tools::{dur, one};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::time::Instant;

fn first_line(s: &str) -> String {
    let l = s.trim().lines().next().unwrap_or("").trim_start_matches(|c: char| c == '#' || c == '*' || c.is_whitespace());
    l.chars().take(140).collect()
}

fn gist(a: &serde_json::Value) -> String {
    let v = ["command", "path", "file_path", "pattern", "url", "query", "task", "action", "prompt"].iter().find_map(|k| a[*k].as_str());
    match v { Some(s) => s.lines().next().unwrap_or("").chars().take(140).collect(), None => a.to_string().chars().take(140).collect() }
}

fn tail(s: &str) -> String {
    let n = s.chars().count();
    let t = if n > 900 { format!("…{}", s.chars().skip(n - 900).collect::<String>()) } else { s.to_string() };
    t.trim().to_string()
}

struct Step { live: bool, kind: String, what: String, ms: Option<u64>, bad: bool, started: Option<Instant> }

pub fn run_steps(t: &Tab, cx: &App) -> AnyElement {
    let p = pal(cx);
    let from = t.items.iter().rposition(|i| matches!(i.kind, ItemKind::User { .. })).map(|i| i + 1).unwrap_or(0);
    let mut steps: Vec<Step> = vec![];
    let mut now: Option<(String, String)> = None;
    for it in &t.items[from..] {
        match &it.kind {
            ItemKind::Bash { command, running, .. } => steps.push(Step { live: *running, kind: "$".into(), what: command.clone(), ms: None, bad: false, started: None }),
            ItemKind::Assistant(a) => {
                let last = a.blocks.iter().rposition(|b| b.is_some());
                for (bi, b) in a.blocks.iter().enumerate() {
                    let Some(b) = b else { continue };
                    let is_last = Some(bi) == last;
                    match b {
                        Block::Thinking { text, ms, started, .. } => {
                            let live = a.live && ms.is_none() && is_last;
                            let w = first_line(text);
                            steps.push(Step { live, kind: "think".into(), what: if w.is_empty() { "thinking".into() } else { w }, ms: *ms, bad: false, started: if live { *started } else { None } });
                            if live { now = Some(("thinking".into(), tail(text))); }
                        }
                        Block::Tool(id) => if let Some(tb) = t.tools.get(id) {
                            let live = matches!(tb.status, ToolStatus::Running | ToolStatus::Streaming | ToolStatus::Queued);
                            let bad = tb.status == ToolStatus::Error || tb.result.as_ref().is_some_and(|r| r.is_error);
                            steps.push(Step { live, kind: tb.name.clone(), what: gist(&tb.args), ms: tb.ms, bad, started: if live { tb.started } else { None } });
                            if live {
                                let out = tb.partial.as_ref().map(|r| r.text()).unwrap_or_default();
                                let shown = if !out.is_empty() { out } else if !tb.args_text.is_empty() { tb.args_text.clone() } else { serde_json::to_string_pretty(&tb.args).unwrap_or_default() };
                                now = Some((format!("{} {}", tb.name, if tb.status == ToolStatus::Streaming { "being written" } else { "running" }), tail(&shown)));
                            }
                        },
                        Block::Text(text) => steps.push(Step { live: a.live && is_last, kind: "reply".into(), what: first_line(text), ms: None, bad: false, started: None }),
                    }
                }
            }
            _ => {}
        }
    }
    let chv = crate::ui::transcript::md::ch(cx) * geo(cx).fs * 0.9;
    let mut col = div().w_full().mt(u(4.)).pt(u(6.)).pb(u(2.)).pl(u(12.)).border_l(u(1.)).border_color(p.rule).flex().flex_col().gap(u(2.));
    if steps.is_empty() { col = col.child(div().text_color(p.dim).child("waiting on the model")); }
    for (i, s) in steps.into_iter().enumerate() {
        let time = s.started.map(|st| dur(st.elapsed().as_millis() as u64)).or(s.ms.map(dur));
        col = col.child(div().flex().items_baseline().gap(u(10.)).min_w_0().text_color(if s.live { p.text } else { p.mid })
            .child(div().flex_none().w(u(chv)).text_color(if s.bad { p.red } else { p.accent })
                .child(if s.live { spin(SharedString::from(format!("rs-{i}"))).into_any_element() } else { div().child(if s.bad { "✗" } else { "·" }).into_any_element() }))
            .child(div().flex_none().min_w(u(chv * 6.)).text_color(p.md_mark).child(s.kind))
            .child(div().flex_1().min_w_0().truncate().child(one(&s.what, 400)))
            .when_some(time, |d, t| d.child(div().flex_none().text_color(p.dim).text_size(em(cx, 0.9 * 0.9)).child(t))));
    }
    if let Some((label, text)) = now {
        col = col.child(div().mt(u(8.)).child(div().text_color(p.dim).text_size(em(cx, 0.9 * 0.9)).mb(u(2.)).child(label))
            .when(!text.is_empty(), |d| d.child(div().text_color(p.mid).text_size(em(cx, 0.9 * 0.92)).child(text))));
    }
    col.into_any_element()
}
