// App-wide state: open sessions, which view is up, settings, the session list from disk.
// pi processes and the extension's bridge report here, and get routed to their tab
use super::tab::{Kind, Level, Mode, Spawner, Tab, TabEvent};
use crate::core::{bridge, pi::PiProc, sessions, sys};
use gpui_kit::*;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View { Home, Chat, Settings, Usage, Agents, Images }

pub enum Msg {
    Rec(String, Value),
    Exit(String, Option<i32>, String),
    Preview(String, Value),
}

#[derive(Clone, Debug)]
pub struct Toast { pub id: u64, pub level: Level, pub text: SharedString }

#[derive(Clone, Debug)]
pub struct Model { pub id: String, pub name: String, pub provider: String, pub reasoning: bool, pub sees: bool, pub context: u64 }

pub enum AppEvent { Focus }
impl EventEmitter<AppEvent> for Pith {}

pub struct Pith {
    pub tabs: Vec<Entity<Tab>>,
    pub active: Option<Entity<Tab>>,
    pub view: View,
    pub settings: Value,
    pub sessions: Arc<Vec<sessions::SessionInfo>>,
    pub meta: Value,
    pub models: Vec<Model>,
    pub toasts: Vec<Toast>,
    pub home_cwd: String,
    pub home_kind: Kind,
    pub sidebar: bool,
    // the panel on the right, its width in window pixels
    pub side: bool,
    pub side_w: f32,
    spawner: Spawner,
    _tasks: Vec<Task<()>>,
    _subs: HashMap<EntityId, Subscription>,
}

struct Global_(Entity<Pith>);
impl Global for Global_ {}

static MODES: Mutex<Option<HashMap<String, Mode>>> = Mutex::new(None);
pub fn set_mode(tab: &str, m: Mode) { MODES.lock().unwrap().get_or_insert_with(HashMap::new).insert(tab.into(), m); }
fn mode_of(tab: &str) -> Mode { MODES.lock().unwrap().as_ref().and_then(|m| m.get(tab).copied()).unwrap_or(Mode::Ask) }

// ext/ and prompts/ ship next to the binary, or in ../lib/pith for the nix and deb layouts. a dev build reads the repo
pub fn res() -> &'static PathBuf {
    static RES: OnceLock<PathBuf> = OnceLock::new();
    RES.get_or_init(|| {
        let exe = std::env::current_exe().ok().and_then(|e| e.parent().map(PathBuf::from));
        let lib = exe.as_ref().map(|d| d.join("../lib/pith"));
        let mac = exe.as_ref().map(|d| d.join("../Resources"));
        [exe, lib, mac].into_iter().flatten().find(|d| d.join("ext/pith-bridge.ts").exists())
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".."))
    })
}

impl Pith {
    pub fn global(cx: &App) -> Entity<Pith> { cx.global::<Global_>().0.clone() }

    pub fn init(cx: &mut App) {
        let (tx, rx) = async_channel::unbounded::<Msg>();
        let btx = tx.clone();
        let (url, token) = bridge::start(move |tab, op, args| match op.as_str() {
            "mode" => Ok(json!({ "mode": mode_of(&tab).as_str() })),
            "preview" => { let _ = btx.send_blocking(Msg::Preview(tab, args)); Ok(json!({})) }
            "image-settings" => {
                let s = sys::load_settings();
                Ok(json!({ "model": s["imageModel"], "provider": s["imageProvider"], "dir": sys::images_dir(), "pi": s["piPath"], "cost": s["imageCost"] }))
            }
            "browser" => Err("the browser pane isn't in this build yet".into()),
            _ => Err(format!("unknown op {op}")),
        });
        let spawner: Spawner = Arc::new(move |tab: &str, cwd: &str, session: Option<&str>, kind: Kind, mode: Mode| {
            set_mode(tab, mode);
            let settings = sys::load_settings();
            let p = |s: &str| res().join(s).display().to_string();
            let mut args = vec!["-e".into(), p("ext/pith-bridge.ts"), "--append-system-prompt".into(), p("prompts/pith.md"), "--prompt-template".into(), p("prompts/commands")];
            if kind == Kind::Design { args.extend(["--append-system-prompt".into(), p("prompts/design-system.md")]); }
            if let Some(f) = session { args.extend(["--session".into(), f.into()]); }
            args.extend(split_args(settings["extraArgs"].as_str().unwrap_or("")));
            let _ = std::fs::create_dir_all(cwd);
            let env = vec![("PITH_BRIDGE".into(), url.clone()), ("PITH_TOKEN".into(), token.clone()), ("PITH_TAB".into(), tab.into()), ("PITH_KIND".into(), kind.as_str().into())];
            let bin = settings["piPath"].as_str().filter(|s| !s.is_empty()).unwrap_or("pi").to_string();
            let (t1, t2, tx1, tx2) = (tab.to_string(), tab.to_string(), tx.clone(), tx.clone());
            PiProc::spawn(&bin, &args, cwd, &env,
                move |rec| { let _ = tx1.send_blocking(Msg::Rec(t1.clone(), rec)); },
                move |code, err| { let _ = tx2.send_blocking(Msg::Exit(t2, code, err)); })
                .map_err(|e| format!("couldn't run {bin}: {e}"))
        });

        let settings = sys::load_settings();
        let home_cwd = settings["defaultCwd"].as_str().map(String::from).unwrap_or_else(|| sys::home().display().to_string());
        let sidebar = settings["sidebar"].as_bool().unwrap_or(true);
        let settings_w = settings["paneWidth"].as_f64().unwrap_or(520.) as f32;
        let pith = cx.new(|cx| {
            // everything pi says, in order, on the ui thread
            let route = cx.spawn(async move |this, cx| {
                while let Ok(msg) = rx.recv().await {
                    let _ = this.update(cx, |app: &mut Pith, cx| app.route(msg, cx));
                }
            });
            // the session list is read off disk on a worker, never in the way of the first frame
            let poll = cx.spawn(async move |this, cx| loop {
                let list = cx.background_spawn(async { (sessions::list_sessions(), sys::load_meta()) }).await;
                if this.update(cx, |app, cx| { app.sessions = Arc::new(list.0); app.meta = list.1; cx.notify(); }).is_err() { break; }
                cx.background_executor().timer(std::time::Duration::from_secs(30)).await;
            });
            Pith {
                tabs: vec![], active: None, view: View::Home, settings, sessions: Arc::new(vec![]), meta: json!({}), models: vec![], toasts: vec![],
                home_cwd, home_kind: Kind::Chat, sidebar, side: false, side_w: settings_w, spawner, _tasks: vec![route, poll], _subs: HashMap::new(),
            }
        });
        cx.set_global(Global_(pith));
    }

    fn route(&mut self, msg: Msg, cx: &mut Context<Self>) {
        let id = match &msg { Msg::Rec(t, _) | Msg::Exit(t, _, _) | Msg::Preview(t, _) => t.clone() };
        let Some(tab) = self.tabs.iter().find(|t| t.read(cx).id == id).cloned() else { return };
        match msg {
            Msg::Rec(_, rec) => {
                let background = self.active.as_ref() != Some(&tab) || self.view != View::Chat;
                tab.update(cx, |t, cx| {
                    t.on_event(&rec, cx);
                    if background && rec["type"] == "agent_settled" { t.unread = true; }
                    if rec["type"] == "agent_settled" && rec["aborted"] != true { crate::platform::notify(&t.title(), "Done", cx); }
                });
            }
            Msg::Exit(_, code, err) => tab.update(cx, |t, cx| t.exited(code, err, cx)),
            Msg::Preview(_, args) => tab.update(cx, |t, cx| { t.preview = Some(args); cx.emit(TabEvent::Preview); }),
        }
        cx.notify();
    }

    pub fn mode_default(&self) -> Mode { Mode::parse(self.settings["mode"].as_str().unwrap_or("ask")) }

    fn add(&mut self, t: Tab, cx: &mut Context<Self>) -> Entity<Tab> {
        let tab = cx.new(|_| t);
        let sub = cx.subscribe(&tab, |app, _tab, ev: &TabEvent, cx| match ev {
            TabEvent::Toast(l, t) => app.toast(*l, t.clone(), cx),
            TabEvent::Settled => { app.refresh_sessions(cx); app.trim(cx); }
            _ => {}
        });
        self._subs.insert(tab.entity_id(), sub);
        // tabs repaint the sidebar too: titles, the running dot, unread
        cx.observe(&tab, |_, _, cx| cx.notify()).detach();
        self.tabs.push(tab.clone());
        tab
    }

    pub fn focus(&mut self, tab: Entity<Tab>, cx: &mut Context<Self>) {
        tab.update(cx, |t, cx| {
            t.unread = false;
            t.last_used = Instant::now();
            if t.status == super::tab::Status::Cold { let _ = t.ensure(cx); }
            cx.notify();
        });
        self.active = Some(tab);
        self.view = View::Chat;
        cx.emit(AppEvent::Focus);
        cx.notify();
        self.trim(cx);
    }

    // open a session file, reusing its tab when it's already open. history comes off disk right away, pi starts behind it
    pub fn open(&mut self, file: &str, cx: &mut Context<Self>) {
        if let Some(t) = self.tabs.iter().find(|t| t.read(cx).file.as_deref() == Some(file)).cloned() { return self.focus(t, cx); }
        let info = self.sessions.iter().find(|s| s.file == file).cloned().unwrap_or_default();
        let m = &self.meta[file];
        let kind = Kind::parse(m["kind"].as_str().unwrap_or("chat"));
        let mode = m["mode"].as_str().map(Mode::parse).unwrap_or(self.mode_default());
        let cwd = if info.cwd.is_empty() { self.home_cwd.clone() } else { info.cwd.clone() };
        let tab = self.add(Tab::new(cwd, Some(file.to_string()), kind, mode, info.name.clone(), self.spawner.clone()), cx);
        let f = file.to_string();
        let disk = cx.background_spawn(async move { sys::agent_session(&f) });
        let weak = tab.downgrade();
        cx.spawn(async move |_, cx| {
            let d = disk.await;
            let _ = weak.update(cx, |t, cx| t.fill_from_disk(&d, cx));
        }).detach();
        self.focus(tab, cx);
    }

    pub fn fresh(&mut self, cwd: String, kind: Kind, cx: &mut Context<Self>) -> Entity<Tab> {
        let tab = self.add(Tab::new(cwd, None, kind, self.mode_default(), None, self.spawner.clone()), cx);
        self.focus(tab.clone(), cx);
        tab
    }

    pub fn close(&mut self, tab: &Entity<Tab>, cx: &mut Context<Self>) {
        tab.update(cx, |t, cx| t.stop(cx));
        self.tabs.retain(|t| t != tab);
        self._subs.remove(&tab.entity_id());
        if self.active.as_ref() == Some(tab) {
            self.active = None;
            self.view = View::Home;
        }
        cx.notify();
    }

    pub fn show(&mut self, v: View, cx: &mut Context<Self>) { self.view = v; cx.notify(); }

    // idle sessions past the limit give their process back, oldest first. they come back on click
    pub fn trim(&mut self, cx: &mut Context<Self>) {
        let max = self.settings["maxLive"].as_u64().unwrap_or(6) as usize;
        let live = self.tabs.iter().filter(|t| t.read(cx).live_process()).count();
        if live <= max { return; }
        let mut idle: Vec<Entity<Tab>> = self.tabs.iter().filter(|t| Some(*t) != self.active.as_ref()).filter(|t| { let t = t.read(cx); t.status == super::tab::Status::Idle && t.asks.is_empty() }).cloned().collect();
        idle.sort_by_key(|t| t.read(cx).last_used);
        for t in idle.into_iter().take(live - max) { t.update(cx, |t, cx| t.stop(cx)); }
    }

    pub fn refresh_sessions(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let list = cx.background_spawn(async { (sessions::list_sessions(), sys::load_meta()) }).await;
            let _ = this.update(cx, |app, cx| { app.sessions = Arc::new(list.0); app.meta = list.1; cx.notify(); });
        }).detach();
    }

    pub fn load_models(&mut self, cx: &mut Context<Self>) {
        let tab = self.active.clone().or_else(|| self.tabs.iter().find(|t| t.read(cx).live_process()).cloned());
        let Some(tab) = tab else { return };
        tab.update(cx, |t, cx| t.with_pi(json!({ "type": "get_available_models" }), cx, |_, r, cx| {
            if r["success"] != true { return; }
            let models = r["data"]["models"].as_array().into_iter().flatten().map(|m| Model {
                id: m["id"].as_str().unwrap_or("").into(), name: m["name"].as_str().unwrap_or("").into(), provider: m["provider"].as_str().unwrap_or("").into(),
                reasoning: m["reasoning"] == true, sees: m["input"].as_array().is_some_and(|a| a.iter().any(|x| x == "image")), context: m["contextWindow"].as_u64().unwrap_or(0),
            }).collect();
            let app = Pith::global(cx);
            app.update(cx, |a, cx| { a.models = models; cx.notify(); });
        }));
    }

    pub fn save_settings(&mut self, patch: Value, cx: &mut Context<Self>) {
        self.settings = sys::save_settings(patch);
        if let Some(b) = self.settings["sidebar"].as_bool() { self.sidebar = b; }
        cx.notify();
    }

    pub fn toast(&mut self, level: Level, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let id = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.toasts.push(Toast { id, level, text: text.into() });
        cx.notify();
        let ms = if level == Level::Error { 7000 } else { 3800 };
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(ms)).await;
            let _ = this.update(cx, |a, cx| { a.toasts.retain(|t| t.id != id); cx.notify(); });
        }).detach();
    }

    pub fn stop_all(&mut self, cx: &mut Context<Self>) {
        for t in self.tabs.clone() { t.update(cx, |t, cx| t.stop(cx)); }
    }
}

fn split_args(s: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut q: Option<char> = None;
    for c in s.chars() {
        match (q, c) {
            (Some(x), c) if c == x => q = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => q = Some(c),
            (None, c) if c.is_whitespace() => { if !cur.is_empty() { out.push(std::mem::take(&mut cur)); } }
            (None, c) => cur.push(c),
        }
    }
    if !cur.is_empty() { out.push(cur); }
    out
}
