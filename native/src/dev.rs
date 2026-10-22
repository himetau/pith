// dev only: PITH_CTL=<dir> lets scripts drive the app. drop {"...": ...} as <id>.cmd.json, the answer lands in <id>.out.json.
//   {"keys": "ctrl-k"}            a keystroke, as if typed
//   {"type": "hello"}             text into whatever has focus
//   {"click": [x, y]}             left click at window coordinates
//   {"move": [x, y]}              the pointer, for hover states
//   {"scroll": [x, y, dy]}        a wheel scroll, dy in pixels (negative scrolls up)
//   {"open": "<session file>"}    open a session
//   {"view": "home|settings|usage|agents|images"}  switch page
//   {"dump": true}                the app's state as json
//   {"resize": [w, h]}
use crate::state::app::{Pith, View};
use crate::state::tab::ItemKind;
use gpui_kit::*;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;

pub fn start(window: AnyWindowHandle, cx: &mut App) {
    let Some(dir) = std::env::var_os("PITH_CTL").map(PathBuf::from) else { return };
    let _ = std::fs::create_dir_all(&dir);
    cx.spawn(async move |cx| loop {
        cx.background_executor().timer(Duration::from_millis(60)).await;
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut cmds: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().ends_with(".cmd.json")).collect();
        cmds.sort();
        for p in cmds {
            let Ok(text) = std::fs::read_to_string(&p) else { continue };
            let _ = std::fs::remove_file(&p);
            let cmd: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            let out = window.update(cx, |_, window, cx| run(&cmd, window, cx)).unwrap_or_else(|e| json!({ "ok": false, "error": e.to_string() }));
            let name = p.file_name().unwrap().to_string_lossy().replace(".cmd.json", ".out.json");
            let tmp = dir.join(format!("{name}.tmp"));
            let _ = std::fs::write(&tmp, out.to_string());
            let _ = std::fs::rename(&tmp, dir.join(name));
        }
    }).detach();
}

fn pt(v: &Value, i: usize) -> Pixels { px(v[i].as_f64().unwrap_or(0.) as f32) }

fn run(cmd: &Value, window: &mut Window, cx: &mut App) -> Value {
    let app = Pith::global(cx);
    if let Some(k) = cmd["keys"].as_str() {
        for part in k.split_whitespace() {
            match Keystroke::parse(part) { Ok(ks) => { window.dispatch_keystroke(ks, cx); } Err(e) => return json!({ "ok": false, "error": e.to_string() }) }
        }
    } else if let Some(t) = cmd["type"].as_str() {
        for c in t.chars() {
            let key = if c == ' ' { "space".to_string() } else if c == '\n' { "shift-enter".to_string() } else { c.to_string() };
            if let Ok(mut ks) = Keystroke::parse(&key) { if c != '\n' { ks.key_char = Some(c.to_string()); } window.dispatch_keystroke(ks, cx); }
        }
    } else if let Some(c) = cmd["click"].as_array() {
        let pos = point(pt(&cmd["click"], 0), pt(&cmd["click"], 1));
        let _ = c;
        window.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position: pos, pressed_button: None, modifiers: Modifiers::default() }), cx);
        window.dispatch_event(PlatformInput::MouseDown(MouseDownEvent { button: MouseButton::Left, position: pos, modifiers: Modifiers::default(), click_count: 1, first_mouse: false }), cx);
        window.dispatch_event(PlatformInput::MouseUp(MouseUpEvent { button: MouseButton::Left, position: pos, modifiers: Modifiers::default(), click_count: 1 }), cx);
    } else if cmd["move"].is_array() {
        let pos = point(pt(&cmd["move"], 0), pt(&cmd["move"], 1));
        window.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position: pos, pressed_button: None, modifiers: Modifiers::default() }), cx);
    } else if cmd["scroll"].is_array() {
        let pos = point(pt(&cmd["scroll"], 0), pt(&cmd["scroll"], 1));
        let dy = pt(&cmd["scroll"], 2);
        window.dispatch_event(PlatformInput::ScrollWheel(ScrollWheelEvent { position: pos, delta: ScrollDelta::Pixels(point(px(0.), -dy)), modifiers: Modifiers::default(), touch_phase: TouchPhase::Moved }), cx);
    } else if let Some(f) = cmd["open"].as_str() {
        app.update(cx, |a, cx| a.open(f, cx));
    } else if let Some(v) = cmd["view"].as_str() {
        let v = match v { "settings" => View::Settings, "usage" => View::Usage, "agents" => View::Agents, "images" => View::Images, "chat" => View::Chat, _ => View::Home };
        app.update(cx, |a, cx| a.show(v, cx));
    } else if cmd["resize"].is_array() {
        window.resize(size(pt(&cmd["resize"], 0), pt(&cmd["resize"], 1)));
    } else if cmd["dump"].as_bool() == Some(true) {
        let a = app.read(cx);
        let tabs: Vec<Value> = a.tabs.iter().map(|t| {
            let t = t.read(cx);
            let items: Vec<Value> = t.items.iter().map(|i| match &i.kind {
                ItemKind::User { text, .. } => json!({ "k": "user", "text": text }),
                ItemKind::Assistant(x) => json!({ "k": "assistant", "blocks": x.blocks.len(), "live": x.live, "error": x.error }),
                ItemKind::Bash { command, .. } => json!({ "k": "bash", "command": command }),
                ItemKind::Note { text, .. } => json!({ "k": "note", "text": text }),
                other => json!({ "k": format!("{:?}", std::mem::discriminant(other)) }),
            }).collect();
            json!({ "id": t.id, "title": t.title(), "status": format!("{:?}", t.status), "file": t.file, "cwd": t.cwd, "error": t.error, "items": items, "asks": t.asks.len(), "model": t.model_label() })
        }).collect();
        return json!({ "ok": true, "view": format!("{:?}", a.view), "active": a.active.as_ref().map(|t| t.read(cx).id.clone()), "tabs": tabs, "sessions": a.sessions.len(), "models": a.models.len(), "toasts": a.toasts.iter().map(|t| t.text.to_string()).collect::<Vec<_>>() });
    } else {
        return json!({ "ok": false, "error": "unknown command" });
    }
    window.refresh();
    json!({ "ok": true })
}
