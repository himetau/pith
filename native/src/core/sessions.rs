// Reads pi's session files straight off disk: the sidebar list and every number on the usage page.
// Parsed once per file and kept until its size or mtime changes
use super::sys::{agent_dir, data_dir, home};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Usage { pub input: f64, pub output: f64, pub cache_read: f64, pub cache_write: f64, pub cost: f64 }

// one model call worth of spend: [ts, model, input, output, cacheRead, cacheWrite, cost]
pub type Spend = (f64, String, f64, f64, f64, f64, f64);

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub file: String,
    pub id: String,
    pub cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub first_prompt: String,
    pub started: f64,
    pub updated: f64,
    pub messages: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub usage: Usage,
}

struct Parsed { info: SessionInfo, spend: Vec<Spend>, size: u64, mtime: f64 }

static CACHE: Mutex<Option<HashMap<PathBuf, Parsed>>> = Mutex::new(None);

pub fn sessions_root() -> PathBuf {
    if let Some(d) = std::env::var_os("PI_CODING_AGENT_SESSION_DIR") { return d.into(); }
    if let Ok(t) = fs::read_to_string(agent_dir().join("settings.json")) {
        if let Some(d) = serde_json::from_str::<Value>(&t).ok().and_then(|v| v.get("sessionDir")?.as_str().map(String::from)) {
            return PathBuf::from(d.replacen('~', &home().display().to_string(), 1));
        }
    }
    agent_dir().join("sessions")
}

fn text_of(c: &Value) -> String {
    match c {
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().filter(|x| x["type"] == "text").filter_map(|x| x["text"].as_str()).collect::<Vec<_>>().join(" "),
        _ => String::new(),
    }
}

fn parse(file: &PathBuf, size: u64, mtime: f64) -> Parsed {
    let mut info = SessionInfo { file: file.display().to_string(), started: mtime, updated: mtime, ..Default::default() };
    let mut spend: Vec<Spend> = vec![];
    let mut last: f64 = 0.0;
    let mut add = |info: &mut SessionInfo, ts: f64, model: &str, u: &Value| {
        if !u.is_object() { return; }
        let n = |k: &str| u.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0);
        let row: Spend = (ts, model.to_string(), n("input"), n("output"), n("cacheRead"), n("cacheWrite"), u["cost"]["total"].as_f64().unwrap_or(0.0));
        if row.2 == 0.0 && row.3 == 0.0 && row.4 == 0.0 && row.5 == 0.0 && row.6 == 0.0 { return; }
        info.usage.input += row.2; info.usage.output += row.3; info.usage.cache_read += row.4; info.usage.cache_write += row.5; info.usage.cost += row.6;
        spend.push(row);
    };
    let Ok(bytes) = fs::read(file) else { return Parsed { info, spend, size, mtime } };
    let text = String::from_utf8_lossy(&bytes);
    for line in text.lines() {
        if line.is_empty() { continue; }
        let Ok(e) = serde_json::from_str::<Value>(line) else { continue };
        let ts = e["timestamp"].as_str().and_then(parse_iso).unwrap_or(mtime);
        match e["type"].as_str().unwrap_or("") {
            "session" => {
                info.id = e["id"].as_str().unwrap_or("").into();
                info.cwd = e["cwd"].as_str().unwrap_or("").into();
                info.started = ts;
                info.parent = e["parentSession"].as_str().map(String::from);
            }
            "session_info" => info.name = e["name"].as_str().filter(|s| !s.is_empty()).map(String::from),
            "model_change" => info.model = Some(format!("{}/{}", e["provider"].as_str().unwrap_or(""), e["modelId"].as_str().unwrap_or(""))),
            "usage" => { let m = format!("{}/{}", e["provider"].as_str().unwrap_or(""), e["model"].as_str().unwrap_or("")); add(&mut info, ts, &m, &e["usage"]); }
            "compaction" | "branch_summary" => { let m = info.model.clone().unwrap_or_else(|| "?".into()); add(&mut info, ts, &m, &e["usage"]); }
            "message" => {
                let m = &e["message"];
                let role = m["role"].as_str().unwrap_or("");
                if role == "system" || role.is_empty() { continue; }
                info.messages += 1;
                let mts = m["timestamp"].as_f64().unwrap_or(ts);
                if role == "user" && info.first_prompt.is_empty() { info.first_prompt = text_of(&m["content"]).chars().take(300).collect(); }
                if role == "assistant" {
                    let model = format!("{}/{}", m["provider"].as_str().unwrap_or(""), m["model"].as_str().unwrap_or(""));
                    info.model = Some(model.clone());
                    add(&mut info, mts, &model, &m["usage"]);
                }
                if role == "toolResult" { let model = info.model.clone().unwrap_or_else(|| "?".into()); add(&mut info, mts, &model, &m["usage"]); }
                last = last.max(mts);
            }
            _ => {}
        }
    }
    info.updated = if last > 0.0 { last } else { mtime };
    Parsed { info, spend, size, mtime }
}

// iso 8601 to ms. pi writes UTC with a Z, nothing fancier
fn parse_iso(s: &str) -> Option<f64> {
    let b = s.as_bytes();
    if b.len() < 19 { return None; }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, se) = (n(0..4)?, n(5..7)?, n(8..10)?, n(11..13)?, n(14..16)?, n(17..19)?);
    let ms = if b.len() > 20 && b[19] == b'.' { s[20..].trim_end_matches('Z').get(0..3).and_then(|x| x.parse::<i64>().ok()).unwrap_or(0) } else { 0 };
    // days from civil, howard hinnant's
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = if y2 >= 0 { y2 } else { y2 - 399 } / 400;
    let yoe = y2 - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(((days * 86400 + h * 3600 + mi * 60 + se) * 1000 + ms) as f64)
}

fn scan<T>(f: impl FnOnce(Vec<&Parsed>) -> T) -> T {
    let root = sessions_root();
    let mut guard = CACHE.lock().unwrap();
    let cache = guard.get_or_insert_with(HashMap::new);
    let mut seen = vec![];
    if let Ok(dirs) = fs::read_dir(&root) {
        for d in dirs.flatten() {
            let Ok(files) = fs::read_dir(d.path()) else { continue };
            for f in files.flatten() {
                let p = f.path();
                if p.extension().is_none_or(|e| e != "jsonl") { continue; }
                let Ok(meta) = f.metadata() else { continue };
                let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as f64).unwrap_or(0.0);
                let fresh = cache.get(&p).is_some_and(|c| c.size == meta.len() && c.mtime == mtime);
                if !fresh { let parsed = parse(&p, meta.len(), mtime); cache.insert(p.clone(), parsed); }
                seen.push(p);
            }
        }
    }
    cache.retain(|k, _| seen.contains(k));
    f(seen.iter().filter_map(|p| cache.get(p)).filter(|p| !p.info.id.is_empty()).collect())
}

pub fn list_sessions() -> Vec<SessionInfo> {
    let mut v = scan(|ps| ps.into_iter().map(|p| p.info.clone()).collect::<Vec<_>>());
    v.sort_by(|a, b| b.updated.partial_cmp(&a.updated).unwrap_or(std::cmp::Ordering::Equal));
    v
}

// spend outside pi sessions (the images page), one Spend row per line
fn spend_log() -> PathBuf { data_dir().join("usage.jsonl") }
pub fn log_spend(model: &str, input: f64, output: f64, cost: f64) {
    let _ = fs::create_dir_all(data_dir());
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0);
    let row: Spend = (now, model.into(), input, output, 0.0, 0.0, cost);
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(spend_log()) {
        let _ = writeln!(f, "{}", serde_json::to_string(&row).unwrap());
    }
}

// the renderer does the bucketing (local days, windows), this just hands over the rows per session
pub fn usage_report() -> Value {
    let (mut spend, mut sessions) = scan(|ps| {
        let spend: Vec<Value> = ps.iter().enumerate().map(|(s, p)| serde_json::json!({ "s": s, "rows": p.spend })).collect();
        let sessions: Vec<Value> = ps.iter().map(|p| serde_json::json!({
            "file": p.info.file,
            "title": p.info.name.clone().unwrap_or_else(|| if p.info.first_prompt.is_empty() { "untitled".into() } else { p.info.first_prompt.clone() }),
            "cwd": p.info.cwd, "updated": p.info.updated, "usage": p.info.usage,
        })).collect();
        (spend, sessions)
    });
    let logged: Vec<Spend> = fs::read_to_string(spend_log()).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    if let Some(last) = logged.last() {
        let mut u = Usage::default();
        for r in &logged { u.input += r.2; u.output += r.3; u.cost += r.6; }
        spend.push(serde_json::json!({ "s": sessions.len(), "rows": logged }));
        sessions.push(serde_json::json!({ "file": "", "title": "images page", "cwd": data_dir().join("images"), "updated": last.0, "usage": u }));
    }
    serde_json::json!({ "spend": spend, "sessions": sessions })
}
