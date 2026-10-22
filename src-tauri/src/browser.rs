// The agent's `browser` tool. Drives the session's browser pane webview: navigation through tauri,
// everything else as JS in the page. Clicks and typing are DOM events, not OS input
use crate::webviews::{ensure, eval_json, label, mark_loading, snapshot, wait_loaded};
use base64::Engine;
use serde_json::{json, Value};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, Url};

// numbers every visible interactive element as data-pith-ref so later calls can point at them
const SNAPSHOT: &str = r#"
  const sel = 'a[href],button,input,select,textarea,summary,[role=button],[role=link],[role=tab],[role=menuitem],[role=checkbox],[role=option],[contenteditable=""],[contenteditable=true],[onclick],[tabindex]:not([tabindex="-1"])';
  let n = 0; const lines = [];
  for (const el of document.querySelectorAll('[data-pith-ref]')) el.removeAttribute('data-pith-ref');
  for (const el of document.querySelectorAll(sel)) {
    const r = el.getBoundingClientRect(), cs = getComputedStyle(el);
    if (r.width < 2 || r.height < 2 || cs.visibility === 'hidden' || cs.display === 'none') continue;
    if (lines.length >= 250) break;
    const ref = 'e' + (++n); el.setAttribute('data-pith-ref', ref);
    const tag = el.tagName.toLowerCase(), role = el.getAttribute('role') || (tag === 'a' ? 'link' : tag);
    const name = (el.getAttribute('aria-label') || el.innerText || el.value || el.placeholder || el.title || el.alt || '').trim().replace(/\s+/g, ' ').slice(0, 80);
    let extra = '';
    if (tag === 'input') extra = ' type=' + (el.type || 'text') + (el.value ? ' value="' + String(el.value).slice(0, 40) + '"' : '');
    if (tag === 'a') extra = ' -> ' + (el.getAttribute('href') || '').slice(0, 80);
    const off = r.bottom < 0 || r.top > innerHeight ? ' (offscreen)' : '';
    lines.push('[' + ref + '] ' + role + (name ? ' "' + name + '"' : '') + extra + off);
  }
  const heads = [...document.querySelectorAll('h1,h2,h3,[role=heading]')].filter((e) => e.getBoundingClientRect().height > 0).slice(0, 30)
    .map((e) => (e.tagName[0] === 'H' ? e.tagName.toLowerCase() : 'heading') + ' ' + e.innerText.trim().replace(/\s+/g, ' ').slice(0, 100));
  const text = (document.body ? document.body.innerText : '').replace(/\n{3,}/g, '\n\n').slice(0, 7000);
  return 'url: ' + location.href + '\ntitle: ' + document.title + '\nscroll: ' + Math.round(scrollY) + '/' + Math.max(0, document.documentElement.scrollHeight - innerHeight) +
    (heads.length ? '\n\n# headings\n' + heads.join('\n') : '') + '\n\n# interactive\n' + lines.join('\n') + '\n\n# text\n' + text;
"#;

fn target(a: &Value) -> String {
    if let Some(r) = a["ref"].as_str() { return format!("document.querySelector('[data-pith-ref=\"{}\"]')", r.replace(['"', '\'', '\\'], "")); }
    if let Some(s) = a["selector"].as_str() { return format!("document.querySelector({})", json!(s)); }
    "null".into()
}

// a key as the page sees it: KeyboardEvent key/code, with modifiers from "ctrl+a"
fn key_js(key: &str) -> String {
    let mut parts: Vec<&str> = key.split('+').collect();
    let base = parts.pop().unwrap_or("Enter");
    let named = match base.to_lowercase().as_str() {
        "enter" | "return" => "Enter", "esc" | "escape" => "Escape", "tab" => "Tab", "backspace" => "Backspace", "delete" => "Delete",
        "up" => "ArrowUp", "down" => "ArrowDown", "left" => "ArrowLeft", "right" => "ArrowRight", "space" => " ", "pageup" => "PageUp", "pagedown" => "PageDown",
        "home" => "Home", "end" => "End", _ => base,
    };
    let m = |n: &str| parts.iter().any(|p| p.eq_ignore_ascii_case(n));
    format!(r#"
      const el = document.activeElement || document.body;
      const o = {{ key: {k}, bubbles: true, cancelable: true, ctrlKey: {c}, altKey: {a}, shiftKey: {s}, metaKey: {m} }};
      const go = el.dispatchEvent(new KeyboardEvent('keydown', o));
      el.dispatchEvent(new KeyboardEvent('keyup', o));
      if (go && o.key === 'Enter' && el.form && !o.ctrlKey) el.form.requestSubmit ? el.form.requestSubmit() : el.form.submit();
      return true;"#, k = json!(named), c = m("ctrl"), a = m("alt"), s = m("shift"), m = m("meta") || m("super"))
}

pub async fn run(app: &AppHandle, tab: &str, a: Value, home: &str) -> Result<Value, String> {
    let l = label("web", tab);
    if app.get_webview(&l).is_none() { ensure(app, "web", tab, home)?; wait_loaded(&l, Duration::from_secs(10)).await; }
    // the user should see the agent drive it
    let _ = app.emit("pane:open", json!([tab, "browser"]));
    let wv = app.get_webview(&l).ok_or("browser pane went away")?;
    let text = |t: String| Ok(json!({ "text": t }));
    let after = |what: String| { let l = l.clone(); async move {
        wait_loaded(&l, Duration::from_secs(8)).await;
        let at = eval_json(app, &l, "return location.href + ' (' + document.title + ')'").await.unwrap_or(json!("?"));
        Ok::<Value, String>(json!({ "text": format!("{what}\nnow at {}", at.as_str().unwrap_or("?")) }))
    } };
    let action = a["action"].as_str().unwrap_or("");
    match action {
        "navigate" => {
            let raw = a["url"].as_str().ok_or("navigate needs a url")?;
            let url = if raw.contains("://") || raw.starts_with("about:") { raw.to_string() } else { format!("https://{raw}") };
            mark_loading(&l);
            crate::webviews::go(&wv, url.parse::<Url>().map_err(|e| e.to_string())?)?;
            wait_loaded(&l, Duration::from_secs(15)).await;
            text(eval_json(app, &l, SNAPSHOT).await?.as_str().unwrap_or("").to_string())
        }
        "snapshot" => text(eval_json(app, &l, SNAPSHOT).await?.as_str().unwrap_or("").to_string()),
        "screenshot" => {
            let png = snapshot(app, &l).await?;
            let img = image::load_from_memory(&png).map_err(|e| e.to_string())?;
            let img = if img.width() > 1280 { img.resize(1280, 100_000, image::imageops::FilterType::Triangle) } else { img };
            let mut jpg = std::io::Cursor::new(vec![]);
            img.to_rgb8().write_to(&mut jpg, image::ImageFormat::Jpeg).map_err(|e| e.to_string())?;
            let url = eval_json(app, &l, "return location.href").await.unwrap_or(json!(""));
            Ok(json!({ "text": format!("screenshot of {}", url.as_str().unwrap_or("")), "image": { "data": base64::engine::general_purpose::STANDARD.encode(jpg.into_inner()), "mimeType": "image/jpeg" } }))
        }
        "click" => {
            let pick = if let (Some(x), Some(y)) = (a["x"].as_f64(), a["y"].as_f64()) { format!("document.elementFromPoint({x}, {y})") } else { target(&a) };
            let r = eval_json(app, &l, &format!(r#"
                const el = {pick};
                if (!el) return null;
                el.scrollIntoView({{ block: 'center', inline: 'center' }});
                const r = el.getBoundingClientRect(), x = r.left + r.width / 2, y = r.top + r.height / 2;
                const o = {{ bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, button: 0, view: window }};
                el.dispatchEvent(new PointerEvent('pointerdown', o)); el.dispatchEvent(new MouseEvent('mousedown', o));
                if (el.focus) el.focus();
                el.dispatchEvent(new PointerEvent('pointerup', o)); el.dispatchEvent(new MouseEvent('mouseup', o));
                el.click();
                return el.tagName.toLowerCase() + ' ' + (el.innerText || el.value || el.getAttribute('aria-label') || '').trim().slice(0, 60);"#)).await?;
            if r.is_null() { return Err("no element there, take a new snapshot".into()); }
            after(format!("clicked {}", r.as_str().unwrap_or(""))).await
        }
        "type" => {
            let t = a["text"].as_str().unwrap_or("");
            let focus = if a["ref"].is_string() || a["selector"].is_string() { target(&a) } else { "document.activeElement".into() };
            // execCommand goes through the editing path, so react-style inputs notice; the setter is the fallback
            let ok = eval_json(app, &l, &format!(r#"
                const el = {focus};
                if (!el) return false;
                el.scrollIntoView({{ block: 'center' }}); el.focus();
                if ({clear}) {{ if ('value' in el) {{ el.select && el.select(); }} else if (el.isContentEditable) {{ document.execCommand('selectAll'); }} }}
                const t = {t};
                if (!document.execCommand('insertText', false, t)) {{
                  const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
                  const set = Object.getOwnPropertyDescriptor(proto, 'value')?.set;
                  if (set) set.call(el, ({clear} ? '' : el.value) + t); else el.textContent += t;
                  el.dispatchEvent(new Event('input', {{ bubbles: true }}));
                }}
                el.dispatchEvent(new Event('change', {{ bubbles: true }}));
                return true;"#, clear = a["clear"].as_bool().unwrap_or(false), t = json!(t))).await?;
            if ok != json!(true) { return Err("no element to type into, take a new snapshot".into()); }
            if a["submit"].as_bool().unwrap_or(false) { eval_json(app, &l, &key_js("Enter")).await?; }
            after(format!("typed {}{}", json!(t.chars().take(60).collect::<String>()), if a["submit"].as_bool().unwrap_or(false) { " and pressed enter" } else { "" })).await
        }
        "key" => {
            let k = a["key"].as_str().unwrap_or("Enter");
            eval_json(app, &l, &key_js(k)).await?;
            after(format!("pressed {k}")).await
        }
        "scroll" => {
            eval_json(app, &l, &format!("scrollBy(0, {}); return true", a["dy"].as_f64().unwrap_or(600.0))).await?;
            tokio::time::sleep(Duration::from_millis(250)).await;
            text(eval_json(app, &l, SNAPSHOT).await?.as_str().unwrap_or("").to_string())
        }
        "back" | "forward" => {
            eval_json(app, &l, if action == "back" { "history.back(); return true" } else { "history.forward(); return true" }).await?;
            after(format!("went {action}")).await
        }
        "reload" => { mark_loading(&l); wv.reload().map_err(|e| e.to_string())?; after("reloaded".into()).await }
        "eval" => {
            let s = a["script"].as_str().unwrap_or("undefined");
            // scripts come as expressions or statements; try as an expression first
            let v = match eval_json(app, &l, &format!("return await ({s}\n)")).await { Ok(v) => v, Err(_) => eval_json(app, &l, s).await? };
            let out = if let Some(t) = v.as_str() { t.to_string() } else { serde_json::to_string_pretty(&v).unwrap_or_default() };
            text(out.chars().take(20000).collect())
        }
        "wait" => {
            tokio::time::sleep(Duration::from_millis(a["ms"].as_u64().unwrap_or(1000).min(15000))).await;
            after("waited".into()).await
        }
        _ => Err(format!("unknown browser action {action}")),
    }
}
