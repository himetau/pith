// One `pi --mode rpc` child per open session. JSONL both ways, responses matched back by id
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

pub struct PiProc {
    stdin: Mutex<Option<ChildStdin>>,
    pending: Mutex<HashMap<String, oneshot::Sender<Value>>>,
    seq: AtomicU64,
    stderr: Arc<Mutex<String>>,
    // pi's own word on why it's exiting ({"type":"error"} on stdout), better than stderr's last lines
    fatal: Arc<Mutex<Option<String>>>,
    pub exited: Arc<AtomicBool>,
    pub pid: u32,
}

impl PiProc {
    pub fn spawn(
        bin: &str, args: &[String], cwd: &str, env: &[(String, String)],
        on_rec: impl Fn(Value) + Send + 'static,
        on_exit: impl FnOnce(Option<i32>, String) + Send + 'static,
    ) -> std::io::Result<Arc<Self>> {
        let mut child = Command::new(bin).arg("--mode").arg("rpc").args(args).current_dir(cwd)
            .envs(env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
            .spawn()?;
        let stdout = child.stdout.take().unwrap();
        let mut stderr = child.stderr.take().unwrap();
        let proc = Arc::new(PiProc {
            stdin: Mutex::new(child.stdin.take()),
            pending: Mutex::new(HashMap::new()),
            seq: AtomicU64::new(0),
            stderr: Arc::new(Mutex::new(String::new())),
            fatal: Arc::new(Mutex::new(None)),
            exited: Arc::new(AtomicBool::new(false)),
            pid: child.id(),
        });

        // split on LF only. a line reader that also breaks on U+2028 would cut json strings in half
        let me = proc.clone();
        std::thread::spawn(move || {
            let mut r = BufReader::new(stdout);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match r.read_until(b'\n', &mut buf) { Ok(0) | Err(_) => break, Ok(_) => {} }
                while buf.last().is_some_and(|b| *b == b'\n' || *b == b'\r') { buf.pop(); }
                if buf.is_empty() { continue; }
                let Ok(rec) = serde_json::from_slice::<Value>(&buf) else { continue };
                if rec["type"] == "error" { *me.fatal.lock().unwrap() = rec["message"].as_str().map(String::from); continue; }
                if rec["type"] == "response" {
                    if let Some(id) = rec["id"].as_str() {
                        if let Some(tx) = me.pending.lock().unwrap().remove(id) { let _ = tx.send(rec); continue; }
                    }
                }
                on_rec(rec);
            }
        });

        // keep the tail only, it's for the error message when pi dies
        let tail = proc.stderr.clone();
        std::thread::spawn(move || {
            let mut chunk = [0u8; 4096];
            while let Ok(n) = stderr.read(&mut chunk) {
                if n == 0 { break; }
                let mut t = tail.lock().unwrap();
                t.push_str(&String::from_utf8_lossy(&chunk[..n]));
                if t.len() > 8000 { let cut = t.len() - 8000; let cut = (cut..t.len()).find(|i| t.is_char_boundary(*i)).unwrap_or(0); t.drain(..cut); }
            }
        });

        let me = proc.clone();
        std::thread::spawn(move || {
            let code = child.wait().ok().and_then(|s| s.code());
            me.exited.store(true, Ordering::SeqCst);
            for (_, tx) in me.pending.lock().unwrap().drain() { let _ = tx.send(json!({ "type": "response", "success": false, "error": "pi exited" })); }
            let err = me.fatal.lock().unwrap().clone().unwrap_or_else(|| me.stderr.lock().unwrap().clone());
            on_exit(code, err);
        });
        Ok(proc)
    }

    // why it died: pi's error record if it sent one, else the end of stderr
    pub fn last_words(&self) -> String {
        self.fatal.lock().unwrap().clone().unwrap_or_else(|| self.stderr.lock().unwrap().trim().to_string())
    }

    fn write(&self, v: &Value) -> bool {
        if self.exited.load(Ordering::SeqCst) { return false; }
        let mut g = self.stdin.lock().unwrap();
        let Some(stdin) = g.as_mut() else { return false };
        let mut line = serde_json::to_vec(v).unwrap();
        line.push(b'\n');
        stdin.write_all(&line).and_then(|_| stdin.flush()).is_ok()
    }

    pub async fn send(&self, mut cmd: Value) -> Value {
        let id = cmd.get("id").and_then(|v| v.as_str()).map(String::from)
            .unwrap_or_else(|| format!("p{}", self.seq.fetch_add(1, Ordering::SeqCst) + 1));
        cmd["id"] = json!(id);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id.clone(), tx);
        if !self.write(&cmd) {
            self.pending.lock().unwrap().remove(&id);
            return json!({ "type": "response", "success": false, "error": "pi isn't running" });
        }
        rx.await.unwrap_or_else(|_| json!({ "type": "response", "success": false, "error": "pi went away" }))
    }

    // answers an ask_request (a tool's approval, the ask tool). pi's {resolved} reply isn't needed
    pub fn ask_response(&self, mut v: Value) {
        v["type"] = json!("ask_response");
        v["id"] = json!(format!("a{}", self.seq.fetch_add(1, Ordering::SeqCst) + 1));
        self.write(&v);
    }

    // closing stdin asks pi to shut down cleanly; if it hasn't in 3s it gets a SIGTERM
    pub fn stop(&self) {
        if self.exited.load(Ordering::SeqCst) { return; }
        self.stdin.lock().unwrap().take();
        let (pid, exited) = (self.pid, self.exited.clone());
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(3));
            if !exited.load(Ordering::SeqCst) { unsafe { libc::kill(pid as i32, libc::SIGTERM); } }
        });
    }
}
