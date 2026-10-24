// Loopback http server for ext/pith.js, which runs inside each pi process. Random port and a random token in the
// path, handed to pi as one url (PITH_BRIDGE): pi keeps env vars that look like secrets away from extensions
use serde_json::{json, Value};

pub fn start(handle: impl Fn(String, String, Value) -> Result<Value, String> + Send + Sync + 'static) -> String {
    use rand::Rng;
    let token: String = rand::rng().sample_iter(rand::distr::Alphanumeric).take(48).map(char::from).collect();
    let server = tiny_http::Server::http("127.0.0.1:0").expect("bridge couldn't bind");
    let port = server.server_addr().to_ip().map(|a| a.port()).unwrap_or(0);
    let handle = std::sync::Arc::new(handle);
    let tok = token.clone();
    std::thread::spawn(move || {
        for mut req in server.incoming_requests() {
            let (handle, tok) = (handle.clone(), tok.clone());
            // browser calls take seconds, so each request gets its own thread
            std::thread::spawn(move || {
                let reply = |code: u16, body: Value| tiny_http::Response::from_string(body.to_string()).with_status_code(code)
                    .with_header("content-type: application/json".parse::<tiny_http::Header>().unwrap());
                if req.method() != &tiny_http::Method::Post || req.url() != format!("/{tok}/call") { let _ = req.respond(reply(404, json!({ "error": "nope" }))); return; }
                let mut body = String::new();
                let _ = req.as_reader().read_to_string(&mut body);
                let Ok(v) = serde_json::from_str::<Value>(&body) else { let _ = req.respond(reply(400, json!({ "error": "bad json" }))); return; };
                let tab = v["tab"].as_str().unwrap_or("").to_string();
                let op = v["op"].as_str().unwrap_or("").to_string();
                let res = match handle(tab, op, v["args"].clone()) {
                    Ok(r) => reply(200, json!({ "ok": true, "result": r })),
                    Err(e) => reply(500, json!({ "error": e })),
                };
                let _ = req.respond(res);
            });
        }
    });
    format!("http://127.0.0.1:{port}/{token}")
}
