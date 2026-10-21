// Everything read off the machine that isn't a pi session: settings, matugen colours, kitty's font,
// hyprland's gaps, git, files, agent definitions, the subagent hub, the image library
use base64::Engine;
use serde_json::{json, Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}
fn xdg(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var).map(PathBuf::from).unwrap_or_else(|| home().join(fallback))
}
pub fn config_dir() -> PathBuf { xdg("XDG_CONFIG_HOME", ".config").join("pith") }
pub fn data_dir() -> PathBuf { xdg("XDG_DATA_HOME", ".local/share").join("pith") }
pub fn images_dir() -> PathBuf { data_dir().join("images") }
pub fn theme_file() -> PathBuf { config_dir().join("colors.css") }
pub fn agent_dir() -> PathBuf {
    std::env::var_os("PI_CODING_AGENT_DIR").map(PathBuf::from).unwrap_or_else(|| home().join(".pi/agent"))
}
// ---------- settings ----------
fn defaults() -> Value {
    let h = home();
    json!({
        "piPath": "pi", "extraArgs": "", "defaultCwd": h, "mode": "ask", "font": "", "proseFont": "", "fontSize": 14,
        "notify": true, "budget": { "window5h": 0, "week": 0, "month": 0 },
        "imageModel": "gemini-3.1-flash-image", "imageCost": 0, "imageProvider": "my-endpoint",
        "browserHome": "https://duckduckgo.com", "designDir": h.join("Designs"), "pinned": [], "maxLive": 6,
        "sidebar": true, "paneWidth": 520
    })
}
fn settings_file() -> PathBuf { config_dir().join("settings.json") }

// unknown keys survive a save, so an older build reading a newer file doesn't eat settings
pub fn load_settings() -> Value {
    let mut out = defaults();
    if let Ok(text) = fs::read_to_string(settings_file()) {
        if let Ok(Value::Object(saved)) = serde_json::from_str::<Value>(&text) {
            let o = out.as_object_mut().unwrap();
            for (k, v) in saved { o.insert(k, v); }
        }
    }
    out
}
pub fn save_settings(patch: Value) -> Value {
    let mut cur = load_settings();
    if let (Some(o), Value::Object(p)) = (cur.as_object_mut(), patch) { for (k, v) in p { o.insert(k, v); } }
    let _ = fs::create_dir_all(config_dir());
    let _ = fs::write(settings_file(), serde_json::to_string_pretty(&cur).unwrap() + "\n");
    cur
}
pub fn setting_str(key: &str) -> String { load_settings().get(key).and_then(|v| v.as_str()).unwrap_or("").to_string() }

// ---------- look ----------
pub fn read_theme() -> Value {
    let f = theme_file();
    match fs::read_to_string(&f) {
        Ok(css) => json!({ "css": css, "found": true, "file": f }),
        Err(_) => json!({ "css": "", "found": false, "file": f }),
    }
}

// kitty writes "Maple Mono medium", the weight word isn't part of the family
pub fn terminal_font() -> Value {
    let Ok(conf) = fs::read_to_string(home().join(".config/kitty/kitty.conf")) else { return Value::Null };
    for line in conf.lines() {
        let l = line.trim();
        let Some(rest) = l.strip_prefix("font_family") else { continue };
        let words: Vec<&str> = rest.split_whitespace().collect();
        if words.is_empty() { continue; }
        let weight = match words.last().unwrap().to_lowercase().as_str() {
            "thin" => 100, "light" => 300, "regular" => 400, "medium" => 500, "semibold" => 600, "bold" => 700, _ => 0,
        };
        if weight > 0 && words.len() > 1 { return json!({ "family": words[..words.len() - 1].join(" "), "weight": weight }); }
        return json!({ "family": words.join(" "), "weight": 400 });
    }
    Value::Null
}

// pane gaps, borders and corners match the compositor's windows
pub fn hypr_look() -> Value {
    let get = |opt: &str| -> Option<i64> {
        let out = Command::new("hyprctl").args(["-j", "getoption", opt]).output().ok()?;
        let j: Value = serde_json::from_slice(&out.stdout).ok()?;
        j.get("int").and_then(|v| v.as_i64())
            .or_else(|| j.get("custom").and_then(|v| v.as_str()).and_then(|s| s.split_whitespace().next()?.parse().ok()))
    };
    json!({ "rounding": get("decoration:rounding").unwrap_or(8), "gap": get("general:gaps_in").unwrap_or(5), "border": get("general:border_size").unwrap_or(2) })
}

pub fn pi_version(bin: &str) -> String {
    Command::new(bin).arg("--version").output().ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty()).unwrap_or_else(|| "not found".into())
}

// ---------- git and files ----------
fn run(cmd: &str, args: &[&str], cwd: &str) -> String {
    Command::new(cmd).args(args).current_dir(cwd).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
}

pub fn git_status(cwd: &str) -> Value {
    if run("git", &["rev-parse", "--is-inside-work-tree"], cwd).trim() != "true" { return json!({ "repo": false }); }
    let branch = run("git", &["rev-parse", "--abbrev-ref", "HEAD"], cwd);
    let status = run("git", &["status", "--porcelain=v1", "-z", "-uall"], cwd);
    let stat = run("git", &["diff", "HEAD", "--numstat"], cwd);
    let mut nums = std::collections::HashMap::new();
    for l in stat.lines() {
        let p: Vec<&str> = l.split('\t').collect();
        if p.len() == 3 { nums.insert(p[2].to_string(), (p[0].parse::<i64>().unwrap_or(0), p[1].parse::<i64>().unwrap_or(0))); }
    }
    let parts: Vec<&str> = status.split('\0').collect();
    let mut files = vec![];
    let mut i = 0;
    while i < parts.len() {
        let p = parts[i];
        i += 1;
        if p.len() < 4 { continue; }
        let (code, path) = (&p[..2], &p[3..]);
        // a rename carries the old path as the next field
        if code.starts_with('R') || code.starts_with('C') { i += 1; }
        let (mut add, del) = nums.get(path).copied().unwrap_or((0, 0));
        if code == "??" { add = fs::read_to_string(Path::new(cwd).join(path)).map(|t| t.lines().count() as i64).unwrap_or(0); }
        files.push(json!({ "path": path, "code": code, "add": add, "del": del }));
    }
    json!({ "repo": true, "branch": branch.trim(), "files": files })
}

pub fn git_diff(cwd: &str, file: &str, untracked: bool) -> String {
    if untracked { run("git", &["diff", "--no-index", "--no-color", "--", "/dev/null", file], cwd) }
    else { run("git", &["diff", "HEAD", "--no-color", "-U4", "--", file], cwd) }
}

pub fn git_branch(cwd: &str) -> String { run("git", &["rev-parse", "--abbrev-ref", "HEAD"], cwd).trim().to_string() }

// for @ mentions. git knows what's ignored, outside a repo fall back to a shallow walk
pub fn list_files(cwd: &str) -> Vec<String> {
    let out = run("git", &["ls-files", "--cached", "--others", "--exclude-standard"], cwd);
    if !out.trim().is_empty() { return out.lines().filter(|l| !l.is_empty()).take(20000).map(String::from).collect(); }
    let mut files = vec![];
    fn walk(root: &Path, dir: &Path, depth: usize, files: &mut Vec<String>) {
        if depth > 4 || files.len() > 4000 { return; }
        let Ok(rd) = fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || name == "node_modules" { continue; }
            let p = e.path();
            if p.is_dir() { walk(root, &p, depth + 1, files); }
            else if let Ok(r) = p.strip_prefix(root) { files.push(r.display().to_string()); }
        }
    }
    walk(Path::new(cwd), Path::new(cwd), 0, &mut files);
    files
}

pub fn mime_of(p: &Path) -> Option<&'static str> {
    match p.extension()?.to_str()?.to_lowercase().as_str() {
        "png" => Some("image/png"), "jpg" | "jpeg" => Some("image/jpeg"), "gif" => Some("image/gif"),
        "webp" => Some("image/webp"), "svg" => Some("image/svg+xml"), "avif" => Some("image/avif"), _ => None,
    }
}

pub fn read_any(path: &str) -> Value {
    let p = Path::new(path);
    let Ok(meta) = fs::metadata(p) else { return json!({ "kind": "missing", "size": 0 }) };
    let size = meta.len();
    if let Some(mime) = mime_of(p) {
        if size > 25 * 1024 * 1024 { return json!({ "kind": "big", "size": size }); }
        let bytes = fs::read(p).unwrap_or_default();
        return json!({ "kind": "image", "data": base64::engine::general_purpose::STANDARD.encode(bytes), "mimeType": mime, "size": size });
    }
    if size > 3 * 1024 * 1024 { return json!({ "kind": "big", "size": size }); }
    let bytes = fs::read(p).unwrap_or_default();
    if bytes.iter().take(8000).any(|b| *b == 0) { return json!({ "kind": "binary", "size": size }); }
    json!({ "kind": "text", "text": String::from_utf8_lossy(&bytes), "size": size })
}

// ---------- agents ----------
fn parse_agent(file: &Path, scope: &str) -> Option<Value> {
    let text = fs::read_to_string(file).ok()?;
    let (fm, body) = match text.strip_prefix("---\n").and_then(|r| r.split_once("\n---")) {
        Some((fm, rest)) => (fm.to_string(), rest.trim_start_matches('\n').to_string()),
        None => (String::new(), text.clone()),
    };
    let mut m = Map::new();
    for l in fm.lines() {
        if let Some((k, v)) = l.split_once(':') { m.insert(k.trim().into(), Value::String(v.trim().trim_matches(|c| c == '"' || c == '\'').into())); }
    }
    let g = |k: &str| m.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let name = if g("name").is_empty() { file.file_stem()?.to_string_lossy().into_owned() } else { g("name") };
    Some(json!({ "name": name, "description": g("description"), "tools": g("tools"), "model": g("model"), "file": file, "scope": scope, "body": body.trim() }))
}

pub fn agent_defs(cwd: Option<&str>) -> Vec<Value> {
    let mut out = vec![];
    let mut read = |dir: PathBuf, scope: &str| {
        let Ok(rd) = fs::read_dir(dir) else { return };
        let mut paths: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "md")).collect();
        paths.sort();
        for p in paths { if let Some(a) = parse_agent(&p, scope) { out.push(a); } }
    };
    read(agent_dir().join("agents"), "user");
    if let Some(c) = cwd { read(Path::new(c).join(".pi/agents"), "project"); }
    out
}

pub fn save_agent(def: &Value, cwd: Option<&str>, scope: &str) -> String {
    let s = |k: &str| def.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let dir = match (scope, cwd) { ("project", Some(c)) => Path::new(c).join(".pi/agents"), _ => agent_dir().join("agents") };
    let _ = fs::create_dir_all(&dir);
    let existing = s("file");
    let file = if !existing.is_empty() && Path::new(&existing).exists() { PathBuf::from(existing) } else {
        let safe: String = s("name").chars().map(|c| if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '-' }).collect();
        dir.join(format!("{safe}.md"))
    };
    let mut fm = vec![format!("name: {}", s("name")), format!("description: {}", s("description"))];
    if !s("tools").is_empty() { fm.push(format!("tools: {}", s("tools"))); }
    if !s("model").is_empty() { fm.push(format!("model: {}", s("model"))); }
    let _ = fs::write(&file, format!("---\n{}\n---\n\n{}\n", fm.join("\n"), s("body").trim()));
    file.display().to_string()
}

fn alive(pid: i64) -> bool {
    // signal 0 only checks; EPERM still means it exists
    unsafe { libc::kill(pid as i32, 0) == 0 || *libc::__errno_location() == libc::EPERM }
}

// the extension can die mid-run and leave agents marked running forever, so check their pid
pub fn read_agent_hub() -> Value {
    let Ok(text) = fs::read_to_string(agent_dir().join("runtime/agents-state.json")) else { return Value::Null };
    let Ok(mut hub) = serde_json::from_str::<Value>(&text) else { return Value::Null };
    if let Some(agents) = hub.get_mut("agents").and_then(|a| a.as_array_mut()) {
        for a in agents {
            let running = matches!(a.get("status").and_then(|s| s.as_str()), Some("spawning" | "thinking" | "tool" | "streaming"));
            let pid = a.get("pid").and_then(|p| p.as_i64()).unwrap_or(0);
            // no pid while spawning is normal, and it's how auto-continue waits between attempts
            let spawning = a.get("status").and_then(|s| s.as_str()) == Some("spawning");
            if running && if pid == 0 { !spawning } else { !alive(pid) } { a["status"] = json!("aborted"); }
        }
    }
    hub
}

// a subagent's session straight off disk, shaped like pi's get_entries, since its own process owns it while it runs
pub fn agent_session(file: &str) -> Value {
    let bytes = fs::read(file).unwrap_or_default();
    let entries: Vec<Value> = String::from_utf8_lossy(&bytes).lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|e| e.get("type").and_then(|t| t.as_str()) != Some("session") && e.get("id").is_some())
        .collect();
    let leaf = entries.last().and_then(|e| e.get("id")).cloned().unwrap_or(Value::Null);
    json!({ "entries": entries, "leafId": leaf })
}

// ---------- images ----------
pub fn list_images() -> Vec<Value> {
    let mut out = vec![];
    fn walk(dir: &Path, out: &mut Vec<Value>) {
        let Ok(rd) = fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() { walk(&p, out); continue; }
            if mime_of(&p).is_none() { continue; }
            let prompt = fs::read_to_string(p.with_extension("txt")).unwrap_or_default();
            let mtime = fs::metadata(&p).and_then(|m| m.modified()).ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as f64).unwrap_or(0.0);
            out.push(json!({ "file": p, "mtime": mtime, "prompt": prompt }));
        }
    }
    walk(&images_dir(), &mut out);
    out.sort_by(|a, b| b["mtime"].as_f64().partial_cmp(&a["mtime"].as_f64()).unwrap_or(std::cmp::Ordering::Equal));
    out
}

// ---------- per-session meta pi doesn't keep: design or chat, and its mode ----------
fn meta_file() -> PathBuf { config_dir().join("sessions.json") }
pub fn load_meta() -> Value {
    fs::read_to_string(meta_file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| json!({}))
}
pub fn save_meta(file: &str, patch: Value) {
    let mut all = load_meta();
    let entry = all.as_object_mut().unwrap().entry(file.to_string()).or_insert_with(|| json!({}));
    if let (Some(e), Value::Object(p)) = (entry.as_object_mut(), patch) { for (k, v) in p { e.insert(k, v); } }
    let _ = fs::write(meta_file(), serde_json::to_string(&all).unwrap());
}
