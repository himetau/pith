// Image generation over an OpenAI-style chat endpoint (gemini image models answer with message.images).
// The images page calls it, and so does the agent's generate_image tool through the bridge
use base64::Engine;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn base_url(provider: &str) -> Result<String, String> {
    let models = fs::read_to_string(super::sys::agent_dir().join("models.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
    if let Some(u) = models.as_ref().and_then(|m| m["providers"][provider]["baseUrl"].as_str()) { return Ok(u.trim_end_matches('/').into()); }
    match provider {
        "openrouter" => Ok("https://openrouter.ai/api/v1".into()),
        "openai" => Ok("https://api.openai.com/v1".into()),
        _ => Err(format!("don't know where {provider} lives, add it to models.json")),
    }
}

// the key pi would use, looked up the way pi does: auth.json, then the provider's apiKey in models.json, then
// <PROVIDER>_API_KEY. a value is a command when it starts with !, an env var when one has that name, else the key
fn api_key(provider: &str) -> Result<String, String> {
    let read = |f: &str| fs::read_to_string(super::sys::agent_dir().join(f)).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).unwrap_or(Value::Null);
    let auth = read("auth.json");
    let a = &auth[provider];
    let found = match a["type"].as_str() {
        Some("oauth") => a["access"].as_str().or(a["access_token"].as_str()).map(String::from),
        Some("bearer_token") => a["token"].as_str().map(String::from),
        _ => a["key"].as_str().and_then(resolve),
    };
    found.or_else(|| read("models.json")["providers"][provider]["apiKey"].as_str().and_then(resolve))
        .or_else(|| std::env::var(format!("{}_API_KEY", provider.to_uppercase().replace('-', "_"))).ok())
        .filter(|k| !k.trim().is_empty())
        .ok_or_else(|| format!("no api key for {provider}"))
}

fn resolve(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if let Some(cmd) = raw.strip_prefix('!') {
        let out = Command::new("sh").args(["-c", cmd]).output().ok()?;
        return Some(String::from_utf8_lossy(&out.stdout).trim().to_string());
    }
    let var = raw.trim_start_matches('$');
    std::env::var(var).ok().or_else(|| Some(raw.to_string()))
}

fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() { out.push(c) } else if !out.ends_with('-') { out.push('-') }
        if out.len() >= 40 { break; }
    }
    let t = out.trim_matches('-').to_string();
    if t.is_empty() { "image".into() } else { t }
}

#[allow(dead_code)] // the images page only needs the files and the spend
pub struct Out { pub files: Vec<String>, pub images: Vec<(String, String)>, pub text: String, pub input: f64, pub output: f64 }

pub fn generate(prompt: &str, inputs: &[(String, String)], model: &str, provider: &str, out_dir: &Path, library: Option<&Path>) -> Result<Out, String> {
    let key = api_key(provider)?;
    let content = if inputs.is_empty() { json!(prompt) } else {
        let mut parts = vec![json!({ "type": "text", "text": prompt })];
        for (mime, data) in inputs { parts.push(json!({ "type": "image_url", "image_url": { "url": format!("data:{mime};base64,{data}") } })); }
        json!(parts)
    };
    let body = json!({ "model": model, "messages": [{ "role": "user", "content": content }], "modalities": ["image", "text"] });
    let mut res = ureq::post(&format!("{}/chat/completions", base_url(provider)?))
        .header("authorization", &format!("Bearer {key}"))
        .config().http_status_as_error(false).build()
        .send_json(&body).map_err(|e| e.to_string())?;
    let status = res.status();
    let text = res.body_mut().read_to_string().map_err(|e| e.to_string())?;
    if !status.is_success() { return Err(format!("{model}: {} {}", status.as_u16(), text.chars().take(300).collect::<String>())); }
    let j: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let msg = &j["choices"][0]["message"];
    let mut urls: Vec<String> = msg["images"].as_array().into_iter().flatten().filter_map(|i| i["image_url"]["url"].as_str().map(String::from)).collect();
    // some gateways put the image in content parts instead
    if let Some(parts) = msg["content"].as_array() {
        urls.extend(parts.iter().filter(|p| p["type"] == "image_url").filter_map(|p| p["image_url"]["url"].as_str().map(String::from)));
    }
    let b64 = base64::engine::general_purpose::STANDARD;
    let mut images = vec![];
    for u in urls {
        if let Some(rest) = u.strip_prefix("data:") {
            if let Some((mime, data)) = rest.split_once(";base64,") { images.push((mime.to_string(), data.to_string())); }
        } else if let Ok(mut r) = ureq::get(&u).call() {
            let mime = r.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("image/png").to_string();
            if let Ok(bytes) = r.body_mut().read_to_vec() { images.push((mime, b64.encode(bytes))); }
        }
    }
    let said = msg["content"].as_str().unwrap_or("").to_string();
    if images.is_empty() { return Err(format!("{model} didn't return an image{}", if said.is_empty() { String::new() } else { format!(": {}", said.chars().take(300).collect::<String>()) })); }
    fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;
    let stamp = chrono_stamp();
    let mut files = vec![];
    for (i, (mime, data)) in images.iter().enumerate() {
        let ext = if mime.contains("png") { "png" } else if mime.contains("webp") { "webp" } else { "jpg" };
        let n = if images.len() > 1 { format!("-{}", i + 1) } else { String::new() };
        let file = out_dir.join(format!("{stamp}-{}{n}.{ext}", slug(prompt)));
        fs::write(&file, b64.decode(data).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let _ = fs::write(file.with_extension("txt"), format!("{prompt}\n\nmodel: {model}\n"));
        // saved somewhere else (a design folder): linked into the library so it shows up there too
        if let Some(lib) = library.filter(|l| fs::canonicalize(l).ok() != fs::canonicalize(out_dir).ok()) {
            let _ = fs::create_dir_all(lib);
            for f in [file.clone(), file.with_extension("txt")] {
                if let Some(name) = f.file_name() { let _ = std::os::unix::fs::symlink(&f, lib.join(name)); }
            }
        }
        files.push(file.display().to_string());
    }
    let u = &j["usage"];
    Ok(Out { files, images, text: said, input: u["prompt_tokens"].as_f64().unwrap_or(0.0), output: u["completion_tokens"].as_f64().unwrap_or(0.0) })
}

// 2026-10-21T16-55-31, utc
fn chrono_stamp() -> String {
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
    iso_ms(ms)[..19].replace(':', "-")
}

// ms since the epoch as 2026-10-21T16:55:31.123Z
pub fn iso_ms(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z", rem / 3600, rem % 3600 / 60, rem % 60, ms.rem_euclid(1000))
}

// the agent's generate_image: paths relative to the session's folder, inputs read off disk, spend in pith's log
pub fn for_agent(a: &Value) -> Result<Value, String> {
    let s = super::sys::load_settings();
    let (model, provider) = (s["imageModel"].as_str().unwrap_or(""), s["imageProvider"].as_str().unwrap_or(""));
    if model.is_empty() || provider.is_empty() { return Err("no image model set, pick one in pith's settings".into()); }
    let cwd = PathBuf::from(a["cwd"].as_str().unwrap_or("."));
    let at = |p: &str| if let Some(rest) = p.strip_prefix("~/") { super::sys::home().join(rest) } else { cwd.join(p) };
    let b64 = base64::engine::general_purpose::STANDARD;
    let mut inputs = vec![];
    for p in a["images"].as_array().into_iter().flatten().filter_map(|p| p.as_str()) {
        let f = at(p);
        let bytes = fs::read(&f).map_err(|e| format!("{}: {e}", f.display()))?;
        let mime = match f.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase().as_str() { "png" => "image/png", "webp" => "image/webp", _ => "image/jpeg" };
        inputs.push((mime.to_string(), b64.encode(bytes)));
    }
    let prompt = a["prompt"].as_str().unwrap_or("");
    let prompt = match a["aspect"].as_str() { Some(r) => format!("{prompt}\n\n(aspect ratio {r})"), None => prompt.to_string() };
    let lib = super::sys::images_dir();
    let out_dir = a["save_to"].as_str().map(at).unwrap_or_else(|| lib.clone());
    let out = generate(&prompt, &inputs, model, provider, &out_dir, Some(&lib))?;
    super::sessions::log_spend(&format!("{provider}/{model}"), out.input, out.output, s["imageCost"].as_f64().unwrap_or(0.) * out.files.len() as f64);
    Ok(json!({
        "files": out.files, "text": out.text, "model": model,
        "images": out.images.iter().map(|(m, d)| json!({ "type": "image", "mimeType": m, "data": d })).collect::<Vec<_>>(),
    }))
}
