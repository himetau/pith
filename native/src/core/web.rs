// Web pages without a webview in the window: each page lives offscreen in the platform's engine (WebKitGTK on
// linux, WKWebView on macos) and the panes show snapshots of it. the agent's browser tool and the design preview
// both drive pages through here. every call is fine from any thread; the engine's own thread does the work
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone, Default, Debug)]
pub struct Page { pub url: String, pub title: String, pub loading: bool }

static PAGES: Mutex<Option<HashMap<String, Page>>> = Mutex::new(None);
fn set_page(id: &str, f: impl FnOnce(&mut Page)) { f(PAGES.lock().unwrap().get_or_insert_with(HashMap::new).entry(id.to_string()).or_default()); }
pub fn page(id: &str) -> Option<Page> { PAGES.lock().unwrap().as_ref().and_then(|m| m.get(id).cloned()) }

// what the engine hands back to js callers: the body runs as an async function, its return value comes back as json
fn wrap(body: &str) -> String {
    format!("const __r = await (async () => {{ {body}\n }})(); return JSON.stringify(__r === undefined ? null : __r);")
}
fn parse(raw: Result<String, String>) -> Result<Value, String> {
    let s = raw?;
    serde_json::from_str(&s).map_err(|e| format!("bad result from the page: {e}"))
}

pub fn normalize(url: &str) -> String {
    if url.contains("://") || url.starts_with("about:") { url.to_string() }
    else if url.starts_with('/') { format!("file://{url}") }
    else { format!("https://{url}") }
}

pub async fn sleep(d: Duration) { async_io::Timer::after(d).await; }

pub async fn wait_loaded(id: &str, max: Duration) {
    let end = Instant::now() + max;
    sleep(Duration::from_millis(150)).await;
    while Instant::now() < end {
        refresh(id).await;
        if !page(id).is_some_and(|p| p.loading) { break; }
        sleep(Duration::from_millis(100)).await;
    }
    sleep(Duration::from_millis(120)).await;
}

#[cfg(target_os = "linux")]
pub use linux::*;
#[cfg(target_os = "macos")]
pub use mac::*;

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use futures::channel::oneshot;
    use gtk::prelude::*;
    use std::cell::RefCell;
    use std::sync::OnceLock;
    use webkit2gtk::{SnapshotOptions, SnapshotRegion, WebView, WebViewExt};

    type Job = Box<dyn FnOnce() + Send>;
    thread_local! { static VIEWS: RefCell<HashMap<String, (gtk::OffscreenWindow, WebView)>> = RefCell::new(HashMap::new()); }

    // gtk lives on its own thread with its own main loop. gpui doesn't use glib, so nothing else claims it
    fn post(job: impl FnOnce() + Send + 'static) {
        static TX: OnceLock<Option<std::sync::mpsc::Sender<Job>>> = OnceLock::new();
        let tx = TX.get_or_init(|| {
            let (tx, rx) = std::sync::mpsc::channel::<Job>();
            let (ok_tx, ok_rx) = std::sync::mpsc::channel::<bool>();
            std::thread::Builder::new().name("pith-web".into()).spawn(move || {
                if gtk::init().is_err() { let _ = ok_tx.send(false); return; }
                let _ = ok_tx.send(true);
                gtk::glib::timeout_add_local(Duration::from_millis(8), move || {
                    while let Ok(job) = rx.try_recv() { job(); }
                    gtk::glib::ControlFlow::Continue
                });
                gtk::main();
            }).ok()?;
            ok_rx.recv().ok().filter(|ok| *ok).map(|_| tx)
        });
        if let Some(tx) = tx { let _ = tx.send(Box::new(job)); }
    }

    fn with(id: &str, f: impl FnOnce(&WebView) + 'static) {
        let id = id.to_string();
        let f: Box<dyn FnOnce(&WebView)> = Box::new(f);
        // the closure is only ever run on the gtk thread, which is where WebView may be touched
        let f = SendBox(f);
        post(move || { let f = f; let f = f.0; VIEWS.with(|v| { if let Some((_, w)) = v.borrow().get(&id) { f(w); } }); });
    }
    struct SendBox<T>(T);
    unsafe impl<T> Send for SendBox<T> {}

    pub fn open(id: &str, url: &str, w: i32, h: i32) {
        let (id, url) = (id.to_string(), normalize(url));
        set_page(&id, |p| { p.loading = true; p.url = url.clone(); });
        post(move || VIEWS.with(|v| {
            let mut v = v.borrow_mut();
            let (_, view) = v.entry(id.clone()).or_insert_with(|| {
                let win = gtk::OffscreenWindow::new();
                let view = WebView::new();
                // an offscreen window has no gl, and webkit aborts if it tries; software paints into the snapshot fine
                if let Some(st) = WebViewExt::settings(&view) {
                    use webkit2gtk::SettingsExt;
                    st.set_hardware_acceleration_policy(webkit2gtk::HardwareAccelerationPolicy::Never);
                }
                win.set_default_size(w.max(200), h.max(200));
                view.set_size_request(w.max(200), h.max(200));
                win.add(&view);
                win.show_all();
                (win, view)
            });
            view.load_uri(&url);
        }));
    }

    pub fn resize(id: &str, w: i32, h: i32) {
        let id = id.to_string();
        let (w, h) = (w.max(200), h.max(200));
        // an offscreen window never shrinks below its default size, so that moves too
        post(move || VIEWS.with(|v| { if let Some((win, view)) = v.borrow().get(&id) { view.set_size_request(w, h); win.set_default_size(w, h); win.resize(w, h); } }));
    }

    pub fn go(id: &str, what: &str) {
        let what = what.to_string();
        set_page(id, |p| p.loading = true);
        with(id, move |w| match what.as_str() { "back" => w.go_back(), "forward" => w.go_forward(), _ => w.reload() });
    }

    pub fn close(id: &str) {
        let id2 = id.to_string();
        PAGES.lock().unwrap().get_or_insert_with(HashMap::new).remove(id);
        post(move || VIEWS.with(|v| { if let Some((win, _)) = v.borrow_mut().remove(&id2) { unsafe { win.destroy(); } } }));
    }

    pub async fn refresh(id: &str) {
        let (tx, rx) = oneshot::channel();
        let tx = SendBox(tx);
        with(id, move |w| { let _ = tx.0.send((w.uri().map(|u| u.to_string()).unwrap_or_default(), w.title().map(|t| t.to_string()).unwrap_or_default(), w.is_loading())); });
        if let Ok((url, title, loading)) = rx.await { set_page(id, |p| { p.url = url; p.title = title; p.loading = loading; }); }
    }

    pub async fn eval(id: &str, body: &str) -> Result<Value, String> {
        let (tx, rx) = oneshot::channel::<Result<String, String>>();
        let tx = SendBox(tx);
        let js = wrap(body);
        with(id, move |w| {
            w.call_async_javascript_function(&js, None, None, None, None::<&gtk::gio::Cancellable>, move |res| {
                let out = res.map_err(|e| e.message().to_string()).map(|v| {
                    use javascriptcore::ValueExt;
                    v.to_str().to_string()
                });
                let _ = tx.0.send(out);
            });
        });
        match futures::future::select(rx, Box::pin(sleep(Duration::from_secs(40)))).await {
            futures::future::Either::Left((Ok(v), _)) => parse(v),
            futures::future::Either::Left((Err(_), _)) => Err("the page went away".into()),
            _ => Err("the page didn't answer".into()),
        }
    }

    pub async fn snapshot(id: &str) -> Result<Vec<u8>, String> {
        let (tx, rx) = oneshot::channel::<Result<Vec<u8>, String>>();
        let tx = SendBox(tx);
        with(id, move |w| {
            w.snapshot(SnapshotRegion::Visible, SnapshotOptions::NONE, None::<&gtk::gio::Cancellable>, move |res| {
                let out = res.map_err(|e| e.message().to_string()).and_then(|surface| {
                    let img = cairo::ImageSurface::try_from(surface).map_err(|_| "snapshot wasn't an image".to_string())?;
                    let mut png = vec![];
                    img.write_to_png(&mut png).map_err(|e| e.to_string())?;
                    Ok(png)
                });
                let _ = tx.0.send(out);
            });
        });
        match futures::future::select(rx, Box::pin(sleep(Duration::from_secs(15)))).await {
            futures::future::Either::Left((Ok(v), _)) => v,
            futures::future::Either::Left((Err(_), _)) => Err("the page went away".into()),
            _ => Err("screenshot timed out".into()),
        }
    }

}

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use dispatch2::DispatchQueue;
    use futures::channel::oneshot;
    use objc2::rc::Retained;
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{NSBackingStoreType, NSBitmapImageFileType, NSBitmapImageRep, NSImage, NSWindow, NSWindowStyleMask};
    use objc2_foundation::{NSDictionary, NSError, NSPoint, NSRect, NSSize, NSString, NSURLRequest, NSURL};
    use objc2_web_kit::{WKContentWorld, WKWebView, WKWebViewConfiguration};
    use std::cell::RefCell;

    thread_local! { static VIEWS: RefCell<HashMap<String, (Retained<NSWindow>, Retained<WKWebView>)>> = RefCell::new(HashMap::new()); }
    struct SendBox<T>(T);
    unsafe impl<T> Send for SendBox<T> {}

    // webkit wants the main thread on macos, which is also gpui's; jobs queue behind whatever it's doing
    fn main(job: impl FnOnce(MainThreadMarker) + 'static) {
        let job = SendBox(job);
        DispatchQueue::main().exec_async(move || { let job = job; job.0(MainThreadMarker::new().expect("main queue runs on the main thread")); });
    }
    fn with(id: &str, f: impl FnOnce(&WKWebView) + 'static) {
        let id = id.to_string();
        main(move |_| VIEWS.with(|v| { if let Some((_, w)) = v.borrow().get(&id) { f(w); } }));
    }

    fn url_of(s: &str) -> Option<Retained<NSURL>> { NSURL::URLWithString(&NSString::from_str(s)) }

    pub fn open(id: &str, url: &str, w: i32, h: i32) {
        let (id, url) = (id.to_string(), normalize(url));
        set_page(&id, |p| { p.loading = true; p.url = url.clone(); });
        main(move |mtm| VIEWS.with(|v| unsafe {
            let mut v = v.borrow_mut();
            let (_, view) = v.entry(id.clone()).or_insert_with(|| {
                // a borderless window far off screen: webkit only paints, and snapshots, views that are in a window
                let frame = NSRect::new(NSPoint::new(-20000., -20000.), NSSize::new(w.max(200) as f64, h.max(200) as f64));
                let win = NSWindow::initWithContentRect_styleMask_backing_defer(NSWindow::alloc(mtm), frame, NSWindowStyleMask::Borderless, NSBackingStoreType::Buffered, false);
                win.setReleasedWhenClosed(false);
                win.setIgnoresMouseEvents(true);
                let conf = WKWebViewConfiguration::new(mtm);
                let view = WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), NSRect::new(NSPoint::new(0., 0.), frame.size), &conf);
                win.setContentView(Some(&view));
                win.orderBack(None);
                (win, view)
            });
            if let Some(u) = url_of(&url) {
                if url.starts_with("file://") {
                    // file pages may read what's next to them, a design's css and images
                    let dir = u.URLByDeletingLastPathComponent().unwrap_or_else(|| u.clone());
                    view.loadFileURL_allowingReadAccessToURL(&u, &dir);
                } else {
                    view.loadRequest(&NSURLRequest::requestWithURL(&u));
                }
            }
        }));
    }

    pub fn resize(id: &str, w: i32, h: i32) {
        let id = id.to_string();
        main(move |_| VIEWS.with(|v| {
            if let Some((win, _)) = v.borrow().get(&id) {
                win.setContentSize(NSSize::new(w.max(200) as f64, h.max(200) as f64));
            }
        }));
    }

    pub fn go(id: &str, what: &str) {
        let what = what.to_string();
        set_page(id, |p| p.loading = true);
        with(id, move |w| unsafe { match what.as_str() { "back" => { w.goBack(); } "forward" => { w.goForward(); } _ => { w.reload(); } } });
    }

    pub fn close(id: &str) {
        let id2 = id.to_string();
        PAGES.lock().unwrap().get_or_insert_with(HashMap::new).remove(id);
        main(move |_| VIEWS.with(|v| { if let Some((win, _)) = v.borrow_mut().remove(&id2) { win.close(); } }));
    }

    pub async fn refresh(id: &str) {
        let (tx, rx) = oneshot::channel();
        let tx = SendBox(tx);
        with(id, move |w| unsafe {
            let url = w.URL().and_then(|u| u.absoluteString()).map(|s| s.to_string()).unwrap_or_default();
            let title = w.title().map(|s| s.to_string()).unwrap_or_default();
            let _ = tx.0.send((url, title, w.isLoading()));
        });
        if let Ok((url, title, loading)) = rx.await { set_page(id, |p| { p.url = url; p.title = title; p.loading = loading; }); }
    }

    pub async fn eval(id: &str, body: &str) -> Result<Value, String> {
        let (tx, rx) = oneshot::channel::<Result<String, String>>();
        let tx = std::sync::Mutex::new(Some(tx));
        let js = wrap(body);
        with(id, move |w| unsafe {
            let done = block2::RcBlock::new(move |res: *mut objc2::runtime::AnyObject, err: *mut NSError| {
                let out = if let Some(e) = err.as_ref() { Err(e.localizedDescription().to_string()) }
                    else if let Some(r) = res.as_ref() { Ok(r.downcast_ref::<NSString>().map(|s| s.to_string()).unwrap_or_else(|| "null".into())) }
                    else { Ok("null".into()) };
                if let Some(t) = tx.lock().unwrap().take() { let _ = t.send(out); }
            });
            w.callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(&NSString::from_str(&js), None, None, &WKContentWorld::pageWorld(MainThreadMarker::new().unwrap()), Some(&done));
        });
        let r = futures::future::select(rx, Box::pin(sleep(Duration::from_secs(40)))).await;
        match r { futures::future::Either::Left((Ok(v), _)) => parse(v), futures::future::Either::Left((Err(_), _)) => Err("the page went away".into()), _ => Err("the page didn't answer".into()) }
    }

    pub async fn snapshot(id: &str) -> Result<Vec<u8>, String> {
        let (tx, rx) = oneshot::channel::<Result<Vec<u8>, String>>();
        let tx = std::sync::Mutex::new(Some(tx));
        with(id, move |w| unsafe {
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
            w.takeSnapshotWithConfiguration_completionHandler(None, &done);
        });
        let r = futures::future::select(rx, Box::pin(sleep(Duration::from_secs(15)))).await;
        match r { futures::future::Either::Left((Ok(v), _)) => v, futures::future::Either::Left((Err(_), _)) => Err("the page went away".into()), _ => Err("screenshot timed out".into()) }
    }
}
