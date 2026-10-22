// The preview pane's page ("pv-<tab>"): the agent's design, reloaded when anything in its folder changes.
// the pane just shows snapshots of it, see core/web.rs
use super::web;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

pub fn page_id(tab: &str) -> String { format!("pv-{tab}") }

// one watcher per tab; a new preview replaces the old one's target
static WATCHED: Mutex<Option<HashMap<String, PathBuf>>> = Mutex::new(None);

pub fn show(tab: &str, path: Option<&str>, url: Option<&str>) {
    let id = page_id(tab);
    let target = path.map(|p| format!("file://{p}")).or(url.map(String::from)).unwrap_or_else(|| "about:blank".into());
    web::open(&id, &target, 1100, 800);
    let Some(p) = path else { WATCHED.lock().unwrap().get_or_insert_with(HashMap::new).remove(tab); return };
    let dir = Path::new(p).parent().map(Path::to_path_buf).unwrap_or_default();
    let fresh = WATCHED.lock().unwrap().get_or_insert_with(HashMap::new).insert(tab.to_string(), dir.clone()).is_none();
    if !fresh { return; }
    let tab = tab.to_string();
    // polling the folder's newest mtime is plenty for a person watching a design, and needs no watcher per platform
    std::thread::spawn(move || {
        let mut last = newest(&dir);
        loop {
            std::thread::sleep(Duration::from_millis(400));
            let Some(dir) = WATCHED.lock().unwrap().as_ref().and_then(|m| m.get(&tab).cloned()) else { break };
            if web::page(&page_id(&tab)).is_none() { WATCHED.lock().unwrap().get_or_insert_with(HashMap::new).remove(&tab); break; }
            let now = newest(&dir);
            if now != last { last = now; web::go(&page_id(&tab), "reload"); }
        }
    });
}

pub fn close(tab: &str) {
    WATCHED.lock().unwrap().get_or_insert_with(HashMap::new).remove(tab);
    web::close(&page_id(tab));
}

// the newest modification time among the folder's files, one level down, which covers html + css + assets/
fn newest(dir: &Path) -> Option<SystemTime> {
    let mut best = None;
    let mut look = |p: &Path| if let Ok(m) = std::fs::metadata(p).and_then(|m| m.modified()) { if best.is_none_or(|b| m > b) { best = Some(m); } };
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.') || n == "node_modules") { continue; }
            for f in std::fs::read_dir(&p).into_iter().flatten().flatten() { look(&f.path()); }
        } else { look(&p); }
    }
    best
}
