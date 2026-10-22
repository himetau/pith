// The browser and preview panes are real webviews laid over the side window, one each per session.
// The ui tells us where its pane is (web_layout) and everything not in that list gets hidden.
// Screenshots go through the platform's webview directly (WebKitGTK, WKWebView), tauri has no api for them
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::webview::{NewWindowResponse, PageLoadEvent};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Url, Webview, WebviewBuilder, WebviewUrl};

static LOADING: Mutex<Option<HashMap<String, bool>>> = Mutex::new(None);
static REQUESTED: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

fn set_loading(label: &str, v: bool) { LOADING.lock().unwrap().get_or_insert_with(HashMap::new).insert(label.into(), v); }
fn is_loading(label: &str) -> bool { LOADING.lock().unwrap().as_ref().and_then(|m| m.get(label).copied()).unwrap_or(false) }

pub fn label(kind: &str, tab: &str) -> String { format!("{kind}-{tab}") }
fn tab_of(label: &str) -> String { label.split_once('-').map(|(_, t)| t.to_string()).unwrap_or_default() }

// the preview's comment mode. picks go back through the document title, which tauri reports for any page
const PICKER: &str = r#"
(() => {
  let on = false, box = null, tag = null, hovered = null;
  const sel = (el) => { const parts = []; for (let e = el; e && e !== document.body && parts.length < 5; e = e.parentElement) {
      if (e.id) { parts.unshift('#' + CSS.escape(e.id)); break; }
      let s = e.tagName.toLowerCase(); const cls = [...e.classList].slice(0, 2); if (cls.length) s += '.' + cls.map(CSS.escape).join('.');
      const sib = e.parentElement ? [...e.parentElement.children].filter((c) => c.tagName === e.tagName) : []; if (sib.length > 1) s += ':nth-of-type(' + (sib.indexOf(e) + 1) + ')';
      parts.unshift(s); } return parts.join(' > '); };
  const make = () => { if (box) return; box = document.createElement('div'); box.style.cssText = 'position:fixed;z-index:2147483646;pointer-events:none;outline:2px solid #f0c;outline-offset:-1px;background:rgba(255,0,204,.08);display:none';
    tag = document.createElement('div'); tag.style.cssText = 'position:fixed;z-index:2147483647;pointer-events:none;font:11px/1.6 ui-monospace,monospace;padding:0 6px;background:#f0c;color:#fff;border-radius:3px;display:none;white-space:nowrap;max-width:60vw;overflow:hidden;text-overflow:ellipsis';
    document.documentElement.append(box, tag); };
  const paint = (el) => { if (!box) return; if (!el) { box.style.display = tag.style.display = 'none'; return; } const r = el.getBoundingClientRect();
    Object.assign(box.style, { display: 'block', left: r.left + 'px', top: r.top + 'px', width: r.width + 'px', height: r.height + 'px' });
    tag.textContent = sel(el); Object.assign(tag.style, { display: 'block', left: Math.max(0, r.left) + 'px', top: (r.top > 22 ? r.top - 22 : r.bottom + 2) + 'px' }); };
  const send = (t) => { const was = document.title; document.title = t; setTimeout(() => { document.title = was; }, 30); };
  const move = (e) => { hovered = e.target; paint(hovered); };
  const click = (e) => { e.preventDefault(); e.stopPropagation(); const el = e.target; const html = el.outerHTML.length > 700 ? el.outerHTML.slice(0, 700) + '…' : el.outerHTML;
    send('pith-pick:' + JSON.stringify({ selector: sel(el), html, text: (el.innerText || '').slice(0, 200) })); };
  const key = (e) => { if (e.key === 'Escape') { window.__pithPick(false); send('pith-pick-off'); } };
  window.__pithPick = (v) => { on = v; make(); const o = { capture: true };
    if (on) { addEventListener('mousemove', move, o); addEventListener('click', click, o); addEventListener('keydown', key, o); }
    else { removeEventListener('mousemove', move, o); removeEventListener('click', click, o); removeEventListener('keydown', key, o); paint(null); } };
  addEventListener('scroll', () => { if (on) paint(hovered); }, true);
})();
"#;

pub fn ensure(app: &AppHandle, kind: &str, tab: &str, url: &str) -> Result<Webview, String> {
    let l = label(kind, tab);
    if let Some(w) = app.get_webview(&l) { return Ok(w); }
    let win = app.get_window("main").ok_or("no main window")?;
    let parsed: Url = url.parse().map_err(|e| format!("bad url {url}: {e}"))?;
    let (a1, a2, a3) = (app.clone(), app.clone(), app.clone());
    let (l1, l2) = (l.clone(), l.clone());
    let is_web = kind == "web";
    let first = if cfg!(target_os = "macos") && parsed.scheme() == "file" { "about:blank".parse().unwrap() } else { parsed.clone() };
    let mut b = WebviewBuilder::new(&l, WebviewUrl::External(first))
        .on_page_load(move |wv, p| {
            let label = wv.label().to_string();
            set_loading(&label, matches!(p.event(), PageLoadEvent::Started));
            if is_web { let _ = a1.emit("browser:url", json!([tab_of(&label), p.url().as_str()])); }
            else if matches!(p.event(), PageLoadEvent::Finished) { let _ = a1.emit("preview:loaded", json!([tab_of(&label)])); }
        })
        // popups and target=_blank open in the same pane
        .on_new_window(move |url, _| {
            if let Some(w) = a2.get_webview(&l1) { set_loading(&l1, true); let _ = w.navigate(url); }
            NewWindowResponse::Deny
        });
    if !is_web {
        b = b.initialization_script(PICKER).on_document_title_changed(move |_wv, title| {
            if let Some(j) = title.strip_prefix("pith-pick:") {
                if let Ok(v) = serde_json::from_str::<Value>(j) { let _ = a3.emit("preview:pick", json!([tab_of(&l2), v])); }
            } else if title == "pith-pick-off" { let _ = a3.emit("preview:pick-off", json!([tab_of(&l2)])); }
        });
    }
    set_loading(&l, true);
    REQUESTED.lock().unwrap().get_or_insert_with(HashMap::new).insert(l.clone(), url.into());
    let wv = win.add_child(b, LogicalPosition::new(-20000.0, 0.0), LogicalSize::new(1100.0, 760.0)).map_err(|e| e.to_string())?;
    let _ = wv.set_auto_resize(false);
    let _ = wv.hide();
    #[cfg(target_os = "macos")]
    if parsed.scheme() == "file" { go(&wv, parsed)?; }
    #[cfg(target_os = "linux")]
    overlay(&wv, is_web);
    Ok(wv)
}

// navigate, but a file url on macos goes through loadFileURL so the page can read the files next to it.
// wkwebview's plain loadRequest only lets the webcontent process read the one file
pub fn go(wv: &Webview, url: Url) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    if url.scheme() == "file" {
        let Ok(path) = url.to_file_path() else { return Err(format!("bad file url {url}")) };
        let dir = path.parent().map(|d| d.to_path_buf()).unwrap_or(path.clone());
        return wv.with_webview(move |pv| unsafe {
            use objc2_foundation::{NSString, NSURL};
            let view = &*(pv.inner() as *const objc2_web_kit::WKWebView);
            let file = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
            let read = NSURL::fileURLWithPath(&NSString::from_str(&dir.to_string_lossy()));
            view.loadFileURL_allowingReadAccessToURL(&file, &read);
        }).map_err(|e| e.to_string());
    }
    wv.navigate(url).map_err(|e| e.to_string())
}

#[cfg(target_os = "linux")]
fn overlay(wv: &Webview, is_web: bool) {
    // tauri's window.add_child packs into a vertical GtkBox on linux, which splits the window vertically
    // and ignores set_position/set_size. move the widget into a GtkOverlay over the main webview instead
    let _ = wv.with_webview(move |pv| {
        use gtk::prelude::*;
        use webkit2gtk::{SettingsExt, WebViewExt};
        native_refresh(&pv.inner());
        if !is_web {
            if let Some(s) = WebViewExt::settings(&pv.inner()) {
                s.set_allow_file_access_from_file_urls(true);
                s.set_allow_universal_access_from_file_urls(true);
            }
        }
        let widget: gtk::Widget = pv.inner().clone().upcast();
        let Some(parent) = widget.parent() else { return };
        if let Some(vbox) = parent.downcast_ref::<gtk::Box>() {
            let children = vbox.children();
            let overlay = if let Some(ov) = children.iter().find_map(|c| c.downcast_ref::<gtk::Overlay>().cloned()) {
                ov
            } else if let Some(first) = children.first().cloned() {
                vbox.remove(&first);
                let ov = gtk::Overlay::new();
                ov.add(&first);
                ov.connect_get_child_position(|_, w| {
                    let x = unsafe { *w.data::<i32>("pith-x").map(|p| p.as_ref()).unwrap_or(&0) };
                    let y = unsafe { *w.data::<i32>("pith-y").map(|p| p.as_ref()).unwrap_or(&0) };
                    let width = unsafe { *w.data::<i32>("pith-w").map(|p| p.as_ref()).unwrap_or(&1) };
                    let height = unsafe { *w.data::<i32>("pith-h").map(|p| p.as_ref()).unwrap_or(&1) };
                    Some(gtk::gdk::Rectangle::new(x, y, width, height))
                });
                ov.show();
                vbox.pack_start(&ov, true, true, 0);
                ov
            } else { return };
            vbox.remove(&widget);
            overlay.add_overlay(&widget);
        }
    });
}

// webkit holds page rendering to ~60fps even on faster screens (PreferPageRenderingUpdatesNear60FPS, on by default).
// that caps scrolling and animation at 60 on a 180hz monitor, so every webview pith makes turns it off.
// the gtk bindings don't wrap webkit's feature api, so it's called straight from the library they link
#[cfg(target_os = "linux")]
pub fn native_refresh(view: &webkit2gtk::WebView) {
    use gtk::glib::object::ObjectType;
    use std::ffi::{c_char, c_void, CStr};
    extern "C" {
        fn webkit_settings_get_all_features() -> *mut c_void;
        fn webkit_feature_list_get_length(list: *mut c_void) -> usize;
        fn webkit_feature_list_get(list: *mut c_void, index: usize) -> *mut c_void;
        fn webkit_feature_list_unref(list: *mut c_void);
        fn webkit_feature_get_identifier(feature: *mut c_void) -> *const c_char;
        fn webkit_settings_set_feature_enabled(settings: *mut c_void, feature: *mut c_void, enabled: i32);
    }
    let Some(settings) = webkit2gtk::WebViewExt::settings(view) else { return };
    unsafe {
        let list = webkit_settings_get_all_features();
        if list.is_null() { return; }
        for i in 0..webkit_feature_list_get_length(list) {
            let f = webkit_feature_list_get(list, i);
            let id = webkit_feature_get_identifier(f);
            if !id.is_null() && CStr::from_ptr(id).to_bytes() == b"PreferPageRenderingUpdatesNear60FPS" {
                webkit_settings_set_feature_enabled(settings.as_ptr() as *mut c_void, f, 0);
            }
        }
        webkit_feature_list_unref(list);
    }
}

// gtk3 paces its frame clock off the previous frame's timings, and only fills those in for frames it commits itself.
// webkit paints through gl, which gtk3 swaps without asking wayland for the frame callback, so refresh_interval stays
// 0 and the clock falls back to its built-in 16667us: every frame of the app capped at 60fps on a 180hz monitor.
// after each paint this fills the interval in from the monitor the window is on, if gtk left it empty.
// GdkFrameTimings is private, so the field is found by offset and checked against the public getter before writing;
// if gtk ever moves it they won't agree and this does nothing
#[cfg(target_os = "linux")]
pub fn pace_frame_clock(win: &gtk::ApplicationWindow) {
    use gtk::prelude::*;
    use gtk::glib::translate::ToGlibPtr;
    const REFRESH_INTERVAL_OFFSET: usize = 56; // ref_count + pad, then frame_counter, cookie, frame_time, smoothed_frame_time, drawn_time, presentation_time
    let hook = |w: &gtk::ApplicationWindow| {
        let Some(clock) = w.frame_clock() else { return };
        let w = w.clone();
        clock.connect_after_paint(move |clock| {
            let Some(t) = clock.current_timings() else { return };
            if t.refresh_interval().is_some() { return; }
            let Some(mhz) = w.window().and_then(|gw| w.display().monitor_at_window(&gw)).map(|m| m.refresh_rate()).filter(|r| *r > 0) else { return };
            unsafe {
                let p: *mut gtk::gdk::ffi::GdkFrameTimings = t.to_glib_none().0;
                let field = (p as *mut u8).add(REFRESH_INTERVAL_OFFSET) as *mut i64;
                if *field != 0 { return; } // the getter said empty, so the offset has to read 0 too
                *field = 1_000_000_000 / mhz as i64;
                if t.refresh_interval().map(|v| v.get() as i64) != Some(*field) { *field = 0; } // wrong field: put it back
            }
        });
    };
    if win.frame_clock().is_some() { hook(win); } else { win.connect_realize(move |w| hook(w)); }
}

// gtk ignores tauri's set_position/set_size for child webviews (see overlay), everywhere else they work
#[cfg(not(target_os = "linux"))]
fn place(wv: &Webview, x: f64, y: f64, w: f64, h: f64) {
    let _ = wv.set_position(LogicalPosition::new(x, y));
    let _ = wv.set_size(LogicalSize::new(w.max(1.0), h.max(1.0)));
}
#[cfg(target_os = "linux")]
fn place(wv: &Webview, x: f64, y: f64, w: f64, h: f64) {
    let _ = wv.with_webview(move |pv| {
        use gtk::prelude::*;
        let widget: gtk::Widget = pv.inner().clone().upcast();
        unsafe {
            widget.set_data("pith-x", x.round() as i32);
            widget.set_data("pith-y", y.round() as i32);
            widget.set_data("pith-w", w.round().max(1.0) as i32);
            widget.set_data("pith-h", h.round().max(1.0) as i32);
        }
        widget.queue_resize();
    });
}

#[derive(Deserialize)]
pub struct Slot { kind: String, tab: String, url: Option<String>, x: f64, y: f64, w: f64, h: f64 }

// show exactly these, hide every other pane webview
pub fn layout(app: &AppHandle, slots: Vec<Slot>, home: &str) -> Result<(), String> {
    let mut keep = vec![];
    for s in slots {
        let url = s.url.clone().unwrap_or_else(|| home.to_string());
        let wv = ensure(app, &s.kind, &s.tab, &url)?;
        let l = label(&s.kind, &s.tab);
        // the preview follows whatever file the session last pointed it at
        if let Some(u) = &s.url {
            let mut req = REQUESTED.lock().unwrap();
            let m = req.get_or_insert_with(HashMap::new);
            if m.get(&l) != Some(u) {
                m.insert(l.clone(), u.clone());
                if let Ok(parsed) = u.parse::<Url>() { set_loading(&l, true); let _ = go(&wv, parsed); }
            }
        }
        place(&wv, s.x, s.y, s.w, s.h);
        let _ = wv.show();
        keep.push(l);
    }
    for (l, wv) in app.webviews() {
        if l == "main" || keep.contains(&l) { continue; }
        let _ = wv.hide();
    }
    Ok(())
}

pub fn close_tab(app: &AppHandle, tab: &str) {
    for k in ["web", "pv"] { if let Some(w) = app.get_webview(&label(k, tab)) { let _ = w.close(); } }
}

pub async fn wait_loaded(label: &str, max: Duration) {
    let end = Instant::now() + max;
    tokio::time::sleep(Duration::from_millis(150)).await;
    while is_loading(label) && Instant::now() < end { tokio::time::sleep(Duration::from_millis(100)).await; }
    tokio::time::sleep(Duration::from_millis(120)).await;
}
pub fn mark_loading(label: &str) { set_loading(label, true); }

// runs `body` as an async function in the page and hands back its return value as json. the result is
// parked on window and collected with tauri's eval_with_callback, which only sees synchronous values
pub async fn eval_json(app: &AppHandle, label: &str, body: &str) -> Result<Value, String> {
    use rand::Rng;
    let wv = app.get_webview(label).ok_or_else(|| format!("no webview {label}"))?;
    let id: String = rand::rng().sample_iter(rand::distr::Alphanumeric).take(12).map(char::from).collect();
    let run = format!(r#"(() => {{ const box = (window.__pithEval ||= {{}}); box["_{id}"] = 1; const k = "{id}";
      try {{
        (async () => {{ {body}
        }})().then((v) => {{ box[k] = JSON.stringify({{ ok: true, v: v === undefined ? null : v }}); }},
                   (e) => {{ box[k] = JSON.stringify({{ ok: false, e: String((e && e.message) || e) }}); }});
      }} catch (e) {{ box[k] = JSON.stringify({{ ok: false, e: String((e && e.message) || e) }}); }}
      return 1; }})()"#);
    wv.eval(run).map_err(|e| e.to_string())?;
    let take = format!(r#"(() => {{ const box = window.__pithEval || {{}}; if (!box["_{id}"]) return "noparse"; const r = box["{id}"]; if (r !== undefined) {{ delete box["{id}"]; delete box["_{id}"]; }} return r === undefined ? null : r; }})()"#);
    let end = Instant::now() + Duration::from_secs(40);
    loop {
        let (tx, rx) = tokio::sync::oneshot::channel::<String>();
        let tx = Mutex::new(Some(tx));
        wv.eval_with_callback(take.clone(), move |r| { if let Some(t) = tx.lock().unwrap().take() { let _ = t.send(r); } }).map_err(|e| e.to_string())?;
        let raw = tokio::time::timeout(Duration::from_secs(5), rx).await.ok().and_then(|r| r.ok()).unwrap_or_default();
        // the callback gets the json of what the expression returned: our json string, or null
        if let Ok(Value::String(inner)) = serde_json::from_str::<Value>(&raw) {
            if inner == "noparse" { return Err("script failed to parse".into()); }
            let out: Value = serde_json::from_str(&inner).map_err(|e| e.to_string())?;
            return if out["ok"] == true { Ok(out["v"].clone()) } else { Err(out["e"].as_str().unwrap_or("script failed").to_string()) };
        }
        if Instant::now() > end { return Err("the page didn't answer".into()); }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
}

// png of what the webview shows
#[cfg(target_os = "linux")]
pub async fn snapshot(app: &AppHandle, label: &str) -> Result<Vec<u8>, String> {
    let wv = app.get_webview(label).ok_or_else(|| format!("no webview {label}"))?;
    let (tx, rx) = tokio::sync::oneshot::channel::<Result<Vec<u8>, String>>();
    wv.with_webview(move |pv| {
        use webkit2gtk::WebViewExt;
        pv.inner().snapshot(webkit2gtk::SnapshotRegion::Visible, webkit2gtk::SnapshotOptions::NONE, None::<&webkit2gtk::gio::Cancellable>, move |res| {
            let out = res.map_err(|e| e.message().to_string()).and_then(|surface| {
                let img = cairo::ImageSurface::try_from(surface).map_err(|_| "snapshot wasn't an image".to_string())?;
                let mut png = vec![];
                img.write_to_png(&mut png).map_err(|e| e.to_string())?;
                Ok(png)
            });
            let _ = tx.send(out);
        });
    }).map_err(|e| e.to_string())?;
    tokio::time::timeout(Duration::from_secs(15), rx).await.map_err(|_| "screenshot timed out".to_string())?.map_err(|_| "webview went away".to_string())?
}

#[cfg(target_os = "macos")]
pub async fn snapshot(app: &AppHandle, label: &str) -> Result<Vec<u8>, String> {
    let wv = app.get_webview(label).ok_or_else(|| format!("no webview {label}"))?;
    let (tx, rx) = tokio::sync::oneshot::channel::<Result<Vec<u8>, String>>();
    let tx = Mutex::new(Some(tx));
    wv.with_webview(move |pv| unsafe {
        use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
        use objc2_foundation::{NSDictionary, NSError};
        let view = &*(pv.inner() as *const objc2_web_kit::WKWebView);
        let done = block2::RcBlock::new(move |img: *mut NSImage, err: *mut NSError| {
            let out = if let Some(img) = img.as_ref() {
                // the snapshot is at the screen's scale, a png comes out of a bitmap rep of its tiff
                img.TIFFRepresentation()
                    .and_then(|tiff| NSBitmapImageRep::imageRepWithData(&tiff))
                    .and_then(|rep| rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()))
                    .map(|png| png.to_vec())
                    .ok_or_else(|| "couldn't encode the snapshot".to_string())
            } else {
                Err(err.as_ref().map(|e| e.localizedDescription().to_string()).unwrap_or_else(|| "snapshot failed".into()))
            };
            if let Some(t) = tx.lock().unwrap().take() { let _ = t.send(out); }
        });
        view.takeSnapshotWithConfiguration_completionHandler(None, &done);
    }).map_err(|e| e.to_string())?;
    tokio::time::timeout(Duration::from_secs(15), rx).await.map_err(|_| "screenshot timed out".to_string())?.map_err(|_| "webview went away".to_string())?
}
