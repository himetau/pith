// pith's backend. Starts pi processes for the ui, serves the bridge their extension calls, drives the
// pane webviews, and watches the files the ui follows live (matugen colours, subagent state, previews)
mod bridge;
mod browser;
mod imagegen;
mod pi;
mod sessions;
mod sys;
mod term;
mod webviews;

use pi::PiProc;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewBuilder, WebviewUrl};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

static PROCS: Mutex<Option<HashMap<String, Arc<PiProc>>>> = Mutex::new(None);
static MODES: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);
static BRIDGE: OnceLock<(String, String)> = OnceLock::new();
static RES: OnceLock<PathBuf> = OnceLock::new();
static PREVIEWS: Mutex<Option<HashMap<String, notify::RecommendedWatcher>>> = Mutex::new(None);

fn procs<T>(f: impl FnOnce(&mut HashMap<String, Arc<PiProc>>) -> T) -> T { f(PROCS.lock().unwrap().get_or_insert_with(HashMap::new)) }
fn mode_of(tab: &str) -> String {
    MODES.lock().unwrap().as_ref().and_then(|m| m.get(tab).cloned()).unwrap_or_else(|| sys::setting_str("mode"))
}
fn set_mode(tab: &str, m: &str) { MODES.lock().unwrap().get_or_insert_with(HashMap::new).insert(tab.into(), m.into()); }
fn res() -> &'static Path { RES.get().unwrap() }
fn err(e: impl std::fmt::Display) -> String { e.to_string() }

// ---------- pi ----------
#[derive(Deserialize)]
struct StartOpts { tab: String, cwd: String, session: Option<String>, kind: Option<String>, model: Option<String>, thinking: Option<String>, mode: Option<String>, name: Option<String> }

#[tauri::command]
async fn pi_start(app: AppHandle, o: StartOpts) -> Value {
    if let Some(old) = procs(|m| m.remove(&o.tab)) { old.stop(); }
    let settings = sys::load_settings();
    if let Some(m) = &o.mode { set_mode(&o.tab, m); }
    let p = |s: &str| res().join(s).display().to_string();
    let mut args = vec!["-e".into(), p("ext/pith-bridge.ts"), "--append-system-prompt".into(), p("prompts/pith.md"), "--prompt-template".into(), p("prompts/commands")];
    if o.kind.as_deref() == Some("design") { args.extend(["--append-system-prompt".into(), p("prompts/design-system.md")]); }
    for (flag, v) in [("--session", &o.session), ("--model", &o.model), ("--thinking", &o.thinking), ("--name", &o.name)] {
        if let Some(v) = v { args.extend([flag.to_string(), v.clone()]); }
    }
    let extra = settings["extraArgs"].as_str().unwrap_or("").to_string();
    args.extend(split_args(&extra));
    let _ = std::fs::create_dir_all(&o.cwd);
    let (url, token) = BRIDGE.get().unwrap().clone();
    let env = vec![("PITH_BRIDGE".into(), url), ("PITH_TOKEN".into(), token), ("PITH_TAB".into(), o.tab.clone()),
        ("PITH_KIND".into(), o.kind.clone().unwrap_or_else(|| "chat".into()))];
    let bin = settings["piPath"].as_str().unwrap_or("pi").to_string();
    let (a1, a2, t1, t2) = (app.clone(), app.clone(), o.tab.clone(), o.tab.clone());
    let pid_cell: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));
    let pc = pid_cell.clone();
    let proc = match PiProc::spawn(&bin, &args, &o.cwd, &env,
        move |rec| { let _ = a1.emit("pi:event", json!([t1, rec])); },
        move |code, stderr| {
            // a replaced or stopped process exiting isn't news
            let mine = *pc.lock().unwrap();
            let current = procs(|m| m.get(&t2).map(|p| p.pid));
            if current != Some(mine) { return; }
            procs(|m| m.remove(&t2));
            let _ = a2.emit("pi:exit", json!([t2, code, stderr]));
        }) {
        Ok(p) => p,
        Err(e) => return json!({ "ok": false, "error": format!("couldn't run {bin}: {e}") }),
    };
    *pid_cell.lock().unwrap() = proc.pid;
    procs(|m| m.insert(o.tab.clone(), proc.clone()));
    let st = proc.send(json!({ "type": "get_state" })).await;
    if st["success"] != true {
        let tail = proc.stderr.lock().unwrap().trim().to_string();
        return json!({ "ok": false, "error": if tail.is_empty() { st["error"].clone() } else { json!(tail) } });
    }
    json!({ "ok": true, "state": st["data"], "pid": proc.pid })
}

fn split_args(s: &str) -> Vec<String> {
    let (mut out, mut cur, mut q) = (vec![], String::new(), false);
    for c in s.chars() {
        match c { '"' => q = !q, c if c.is_whitespace() && !q => { if !cur.is_empty() { out.push(std::mem::take(&mut cur)); } } c => cur.push(c) }
    }
    if !cur.is_empty() { out.push(cur); }
    out
}

#[tauri::command]
async fn pi_cmd(tab: String, cmd: Value) -> Value {
    match procs(|m| m.get(&tab).cloned()) {
        Some(p) => p.send(cmd).await,
        None => json!({ "type": "response", "success": false, "error": "this session isn't running" }),
    }
}
#[tauri::command]
fn pi_ui(tab: String, id: String, payload: Value) { if let Some(p) = procs(|m| m.get(&tab).cloned()) { p.ui_response(&id, payload); } }
#[tauri::command]
fn pi_stop(app: AppHandle, tab: String) { if let Some(p) = procs(|m| m.remove(&tab)) { p.stop(); } webviews::close_tab(&app, &tab); }
#[tauri::command]
fn pi_mode(tab: String, mode: String) { set_mode(&tab, &mode); }
#[tauri::command]
fn pi_live() -> Vec<String> { procs(|m| m.keys().cloned().collect()) }

// ---------- sessions, settings, system ----------
#[tauri::command]
fn sessions_list() -> Vec<sessions::SessionInfo> { sessions::list_sessions() }
#[tauri::command]
fn sessions_trash(file: String) -> Result<(), String> { trash::delete(&file).map_err(err) }
#[tauri::command]
fn meta_all() -> Value { sys::load_meta() }
#[tauri::command]
fn meta_set(file: String, patch: Value) { sys::save_meta(&file, patch) }
#[tauri::command]
fn usage_get() -> Value { sessions::usage_report() }
#[tauri::command]
fn settings_get() -> Value { sys::load_settings() }
#[tauri::command]
fn settings_set(patch: Value) -> Value { sys::save_settings(patch) }
#[tauri::command]
fn sys_info() -> Value {
    json!({
        "home": sys::home(), "piVersion": sys::pi_version(&sys::setting_str("piPath")), "font": sys::terminal_font(), "hypr": sys::hypr_look(),
        "theme": sys::read_theme(), "imagesDir": sys::images_dir(), "agentDir": sys::agent_dir(), "configDir": sys::config_dir(), "platform": "linux",
    })
}
#[tauri::command]
fn git_status(cwd: String) -> Value { sys::git_status(&cwd) }
#[tauri::command]
fn git_diff(cwd: String, file: String, untracked: bool) -> String { sys::git_diff(&cwd, &file, untracked) }
#[tauri::command]
fn git_branch(cwd: String) -> String { sys::git_branch(&cwd) }
#[tauri::command]
fn fs_list(cwd: String) -> Vec<String> { sys::list_files(&cwd) }
#[tauri::command]
fn fs_read(path: String) -> Value { sys::read_any(&path) }
#[tauri::command]
fn fs_write(path: String, text: String) -> Result<(), String> {
    if let Some(d) = Path::new(&path).parent() { std::fs::create_dir_all(d).map_err(err)?; }
    std::fs::write(&path, text).map_err(err)
}
#[tauri::command]
fn fs_exists(path: String) -> bool { Path::new(&path).exists() }
#[tauri::command]
async fn fs_pick_dir(app: AppHandle, def: Option<String>) -> Option<String> {
    let mut d = app.dialog().file();
    if let Some(p) = def { d = d.set_directory(p); }
    d.blocking_pick_folder().and_then(|f| f.into_path().ok()).map(|p| p.display().to_string())
}
#[tauri::command]
async fn fs_pick_files(app: AppHandle) -> Vec<String> {
    app.dialog().file().blocking_pick_files().unwrap_or_default().into_iter().filter_map(|f| f.into_path().ok()).map(|p| p.display().to_string()).collect()
}

// ---------- previews reload when their folder changes ----------
#[tauri::command]
fn preview_watch(app: AppHandle, tab: String, path: String) {
    use notify::Watcher;
    let Some(dir) = Path::new(&path).parent().map(Path::to_path_buf) else { return };
    let label = webviews::label("pv", &tab);
    let last = Arc::new(Mutex::new(std::time::Instant::now() - std::time::Duration::from_secs(5)));
    let w = notify::recommended_watcher(move |ev: notify::Result<notify::Event>| {
        let Ok(ev) = ev else { return };
        if !matches!(ev.kind, notify::EventKind::Modify(_) | notify::EventKind::Create(_)) { return; }
        // editors and the agent write in bursts; one reload per burst
        let mut l = last.lock().unwrap();
        if l.elapsed() < std::time::Duration::from_millis(250) { return; }
        *l = std::time::Instant::now();
        let (app, label) = (app.clone(), label.clone());
        std::thread::spawn(move || { std::thread::sleep(std::time::Duration::from_millis(150)); if let Some(wv) = app.get_webview(&label) { let _ = wv.reload(); } });
    });
    if let Ok(mut w) = w {
        if w.watch(&dir, notify::RecursiveMode::Recursive).is_ok() { PREVIEWS.lock().unwrap().get_or_insert_with(HashMap::new).insert(tab, w); }
    }
}
#[tauri::command]
fn preview_unwatch(tab: String) { PREVIEWS.lock().unwrap().get_or_insert_with(HashMap::new).remove(&tab); }

// ---------- agents, images ----------
#[tauri::command]
fn agents_defs(cwd: Option<String>) -> Vec<Value> { sys::agent_defs(cwd.as_deref()) }
#[tauri::command]
fn agents_save(def: Value, cwd: Option<String>, scope: Option<String>) -> String { sys::save_agent(&def, cwd.as_deref(), scope.as_deref().unwrap_or("user")) }
#[tauri::command]
fn agents_hub() -> Value { sys::read_agent_hub() }
#[tauri::command]
fn agents_session(file: String) -> Value { sys::agent_session(&file) }
#[tauri::command]
fn images_list() -> Vec<Value> { sys::list_images() }

#[derive(Deserialize)]
struct ImgIn { data: String, #[serde(rename = "mimeType")] mime_type: String }

#[tauri::command]
async fn images_generate(prompt: String, inputs: Option<Vec<ImgIn>>) -> Result<Value, String> {
    let s = sys::load_settings();
    let (model, provider, pi) = (s["imageModel"].as_str().unwrap_or("").to_string(), s["imageProvider"].as_str().unwrap_or("").to_string(), s["piPath"].as_str().unwrap_or("pi").to_string());
    let price = s["imageCost"].as_f64().unwrap_or(0.0);
    let inputs: Vec<(String, String)> = inputs.unwrap_or_default().into_iter().map(|i| (i.mime_type, i.data)).collect();
    let out = tauri::async_runtime::spawn_blocking(move || imagegen::generate(&prompt, &inputs, &model, &provider, &sys::images_dir(), &pi).map(|o| (o, model, provider)))
        .await.map_err(err)??;
    let (o, model, provider) = out;
    // not part of any pi session, so it goes in pith's own log for the usage page
    sessions::log_spend(&format!("{provider}/{model}"), o.input, o.output, price * o.files.len() as f64);
    Ok(json!({ "files": o.files, "images": o.images.iter().map(|(m, d)| json!({ "mimeType": m, "data": d })).collect::<Vec<_>>(), "text": o.text }))
}

// ---------- os ----------
#[tauri::command]
fn open_external(app: AppHandle, url: String) -> Result<(), String> { app.opener().open_url(url, None::<&str>).map_err(err) }
#[tauri::command]
fn open_path(app: AppHandle, path: String) -> Result<(), String> { app.opener().open_path(path, None::<&str>).map_err(err) }
#[tauri::command]
fn open_reveal(app: AppHandle, path: String) -> Result<(), String> { app.opener().reveal_item_in_dir(path).map_err(err) }
#[tauri::command]
fn clip_write(app: AppHandle, text: String) -> Result<(), String> { app.clipboard().write_text(text).map_err(err) }
#[tauri::command]
fn clip_image(app: AppHandle, data: String) -> Result<(), String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(data).map_err(err)?;
    let img = image::load_from_memory(&bytes).map_err(err)?.to_rgba8();
    let (w, h) = img.dimensions();
    app.clipboard().write_image(&tauri::image::Image::new_owned(img.into_raw(), w, h)).map_err(err)
}
#[tauri::command]
fn notify(app: AppHandle, title: String, body: String) {
    if app.get_window("main").and_then(|w| w.is_focused().ok()).unwrap_or(false) { return; }
    let _ = app.notification().builder().title(title).body(body).show();
}
#[tauri::command]
fn win_focused(app: AppHandle) -> bool { app.get_window("main").and_then(|w| w.is_focused().ok()).unwrap_or(false) }
#[tauri::command]
fn win_minimize(app: AppHandle) { if let Some(w) = app.get_window("main") { let _ = w.minimize(); } }
#[tauri::command]
fn win_maximize(app: AppHandle) { if let Some(w) = app.get_window("main") { if w.is_maximized().unwrap_or(false) { let _ = w.unmaximize(); } else { let _ = w.maximize(); } } }
#[tauri::command]
fn win_close(app: AppHandle) { if let Some(w) = app.get_window("main") { let _ = w.close(); } }
// the ui's scale is the main webview's zoom. the pane webviews keep 1, pages should look like they do in a browser
#[tauri::command]
fn win_zoom(app: AppHandle, scale: f64) { if let Some(w) = app.get_webview("main") { let _ = w.set_zoom(scale.clamp(0.5, 3.0)); } }

// ---------- terminal ----------
#[tauri::command]
fn term_open(app: AppHandle, id: String, cwd: String, cols: u16, rows: u16) -> Result<(), String> {
    let cwd = if Path::new(&cwd).exists() { cwd } else { sys::home().display().to_string() };
    let (a1, a2, i1, i2) = (app.clone(), app.clone(), id.clone(), id.clone());
    term::open(&id, &cwd, cols, rows, move |d| { let _ = a1.emit("term:data", json!([i1, d])); }, move |_| { let _ = a2.emit("term:exit", json!([i2, Value::Null])); })
}
#[tauri::command]
fn term_write(id: String, data: String) { term::write(&id, &data) }
#[tauri::command]
fn term_resize(id: String, cols: u16, rows: u16) { term::resize(&id, cols, rows) }
#[tauri::command]
fn term_close(id: String) { term::close(&id) }

// ---------- pane webviews ----------
#[tauri::command]
fn web_layout(app: AppHandle, slots: Vec<webviews::Slot>) -> Result<(), String> {
    webviews::layout(&app, slots, sys::load_settings()["browserHome"].as_str().unwrap_or("about:blank"))
}
#[tauri::command]
fn web_navigate(app: AppHandle, tab: String, url: String) -> Result<(), String> {
    let l = webviews::label("web", &tab);
    let wv = app.get_webview(&l).ok_or("no browser yet")?;
    webviews::mark_loading(&l);
    wv.navigate(url.parse().map_err(err)?).map_err(err)
}
#[tauri::command]
fn web_go(app: AppHandle, kind: String, tab: String, what: String) -> Result<(), String> {
    let wv = app.get_webview(&webviews::label(&kind, &tab)).ok_or("no webview")?;
    match what.as_str() { "reload" => wv.reload().map_err(err), "back" => wv.eval("history.back()").map_err(err), _ => wv.eval("history.forward()").map_err(err) }
}
#[tauri::command]
fn web_url(app: AppHandle, kind: String, tab: String) -> String {
    app.get_webview(&webviews::label(&kind, &tab)).and_then(|w| w.url().ok()).map(|u| u.to_string()).unwrap_or_default()
}
#[tauri::command]
fn web_eval(app: AppHandle, kind: String, tab: String, js: String) { if let Some(w) = app.get_webview(&webviews::label(&kind, &tab)) { let _ = w.eval(js); } }
#[tauri::command]
fn web_close(app: AppHandle, tab: String) { webviews::close_tab(&app, &tab) }

// ---------- what the pi extension asks for ----------
fn bridge_call(app: &AppHandle, tab: String, op: String, args: Value) -> Result<Value, String> {
    match op.as_str() {
        "mode" => Ok(json!({ "mode": mode_of(&tab) })),
        "browser" => {
            let home = sys::load_settings()["browserHome"].as_str().unwrap_or("about:blank").to_string();
            tauri::async_runtime::block_on(browser::run(app, &tab, args, &home))
        }
        "preview" => { let _ = app.emit("preview:open", json!([tab, args])); Ok(json!({})) }
        "image-settings" => {
            let s = sys::load_settings();
            Ok(json!({ "model": s["imageModel"], "provider": s["imageProvider"], "dir": sys::images_dir(), "pi": s["piPath"], "cost": s["imageCost"] }))
        }
        _ => Err(format!("unknown op {op}")),
    }
}

// dev only: drop {"js": "<expression>"} or {"size": [w, h]} as *.cmd.json in $PITH_CTL to drive the app from scripts
fn dev_control(app: AppHandle, dir: PathBuf) {
    use notify::Watcher;
    let _ = std::fs::create_dir_all(&dir);
    let d2 = dir.clone();
    let mut w = notify::recommended_watcher(move |ev: notify::Result<notify::Event>| {
        let Ok(ev) = ev else { return };
        if !matches!(ev.kind, notify::EventKind::Create(_) | notify::EventKind::Modify(_)) { return };
        for p in ev.paths {
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if !name.ends_with(".cmd.json") { continue; }
            let Ok(text) = std::fs::read_to_string(&p) else { continue };
            let _ = std::fs::remove_file(&p);
            let out = d2.join(name.replace(".cmd.json", ".out.json"));
            let app = app.clone();
            std::thread::spawn(move || {
                let cmd: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                if let (Some(w), Some(s)) = (app.get_window("main"), cmd["size"].as_array()) {
                    let _ = w.set_size(LogicalSize::new(s[0].as_f64().unwrap_or(1480.0), s[1].as_f64().unwrap_or(940.0)));
                }
                let res = if let Some(js) = cmd["js"].as_str() {
                    match tauri::async_runtime::block_on(async {
                        match webviews::eval_json(&app, "main", &format!("return await ({js}\n)")).await {
                            Ok(v) => Ok(v),
                            Err(_) => webviews::eval_json(&app, "main", js).await,
                        }
                    }) {
                        Ok(v) => json!({ "ok": true, "value": v }), Err(e) => json!({ "ok": false, "error": e }),
                    }
                } else if let Some(shot) = cmd["shot"].as_str() {
                    match tauri::async_runtime::block_on(webviews::snapshot(&app, "main")).and_then(|png| std::fs::write(shot, png).map_err(err)) {
                        Ok(()) => json!({ "ok": true }), Err(e) => json!({ "ok": false, "error": e }),
                    }
                } else {
                    json!({ "ok": true })
                };
                let _ = std::fs::write(&out, res.to_string());
            });
        }
    }).expect("ctl watcher");
    w.watch(&dir, notify::RecursiveMode::NonRecursive).expect("ctl watch");
    std::mem::forget(w);
}

fn watch_dir(dir: PathBuf, test: impl Fn(&str) -> bool + Send + 'static, fire: impl Fn() + Send + 'static) {
    use notify::Watcher;
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut w) = notify::recommended_watcher(move |ev: notify::Result<notify::Event>| {
        let Ok(ev) = ev else { return };
        // fire() reads the file it's watching, so an Access event here loops into itself
        if !matches!(ev.kind, notify::EventKind::Create(_) | notify::EventKind::Modify(_)) { return; }
        if ev.paths.iter().any(|p| p.file_name().is_some_and(|n| test(&n.to_string_lossy()))) { fire(); }
    }) {
        if w.watch(&dir, notify::RecursiveMode::NonRecursive).is_ok() { std::mem::forget(w); }
    }
}

pub fn run() {
    // the wayland app_id, so the compositor and launchers match the window to pith.desktop
    gtk::glib::set_prgname(Some("pith"));
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let h = app.handle().clone();
            // pi and the shell read the extension and prompts off disk. bundled builds carry them as resources,
            // the launcher's copy (scripts/install-desktop.sh) keeps them next to the binary, a dev build reads the repo
            let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(PathBuf::from));
            let bundled = [app.path().resource_dir().ok(), exe_dir].into_iter().flatten().find(|d| d.join("ext/pith-bridge.ts").exists());
            let _ = RES.set(bundled.unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")));
            let hb = h.clone();
            let _ = BRIDGE.set(bridge::start(move |tab, op, args| bridge_call(&hb, tab, op, args)));

            let hidden = std::env::var_os("PITH_HIDDEN").is_some();
            let win = tauri::window::WindowBuilder::new(app, "main")
                .title("pith").inner_size(1480.0, 940.0).min_inner_size(760.0, 480.0).decorations(false).visible(!hidden)
                .build()?;
            let size = win.inner_size()?.to_logical::<f64>(win.scale_factor()?);
            let main = win.add_child(WebviewBuilder::new("main", WebviewUrl::App("index.html".into())).auto_resize(), LogicalPosition::new(0.0, 0.0), size)?;
            let _ = main.with_webview(|pv| webviews::native_refresh(&pv.inner()));
            if let Ok(gw) = win.gtk_window() { webviews::pace_frame_clock(&gw); }
            // PITH_FRAME_LOG=1: logs gtk's frame clock once a second (it keeps the clock ticking to measure it).
            // webkit hands a frame back only after gtk paints it, so this is the real ceiling on fps
            if std::env::var_os("PITH_FRAME_LOG").is_some() {
                if let Ok(gw) = win.gtk_window() {
                    use gtk::prelude::*;
                    let n = std::rc::Rc::new(std::cell::Cell::new((0u32, 0i64)));
                    // it redraws every tick: gtk only gets real timings back from the compositor for frames it commits
                    gw.add_tick_callback(move |w, clock| {
                        w.queue_draw();
                        let (count, since) = n.get();
                        let now = clock.frame_time();
                        if since == 0 { n.set((0, now)); return gtk::glib::ControlFlow::Continue; }
                        if now - since >= 1_000_000 {
                            let (interval, _) = clock.refresh_info(now);
                            let timings = clock.current_timings().and_then(|t| t.refresh_interval()).map(|v| v.get() as i64).unwrap_or(-1);
                            let mon = w.window().and_then(|gw| w.display().monitor_at_window(&gw)).map(|m| format!("{:?} {}mHz", m.model(), m.refresh_rate())).unwrap_or_else(|| "none".into());
                            eprintln!("frame clock: {} fps, refresh interval {}us, last timings {}us, on monitor {}", count + 1, interval, timings, mon);
                            n.set((0, now));
                        } else { n.set((count + 1, since)); }
                        gtk::glib::ControlFlow::Continue
                    });
                }
            }

            let ht = h.clone();
            watch_dir(sys::config_dir(), |f| f == "colors.css", move || { let _ = ht.emit("theme", sys::read_theme()); });
            let ha = h.clone();
            watch_dir(sys::agent_dir().join("runtime"), |f| f.starts_with("agents-state"), move || { let _ = ha.emit("agents:hub", json!([sys::read_agent_hub()])); });
            if let Some(dir) = std::env::var_os("PITH_CTL") { dev_control(h.clone(), dir.into()); }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            pi_start, pi_cmd, pi_ui, pi_stop, pi_mode, pi_live,
            sessions_list, sessions_trash, meta_all, meta_set, usage_get, settings_get, settings_set, sys_info,
            git_status, git_diff, git_branch, fs_list, fs_read, fs_write, fs_exists, fs_pick_dir, fs_pick_files,
            preview_watch, preview_unwatch, agents_defs, agents_save, agents_hub, agents_session, images_list, images_generate,
            open_external, open_path, open_reveal, clip_write, clip_image, notify, win_focused, win_minimize, win_maximize, win_close, win_zoom,
            term_open, term_write, term_resize, term_close,
            web_layout, web_navigate, web_go, web_url, web_eval, web_close,
        ])
        .build(tauri::generate_context!())
        .expect("pith failed to start")
        .run(|_app, ev| {
            if let tauri::RunEvent::Exit = ev {
                for p in procs(|m| m.drain().map(|(_, p)| p).collect::<Vec<_>>()) { p.stop(); }
                term::close_all();
            }
        });
}
