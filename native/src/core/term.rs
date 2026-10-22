// Shells for the terminal pane, each on its own pty
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Mutex;

struct Term { master: Box<dyn MasterPty + Send>, writer: Box<dyn Write + Send>, child: Box<dyn Child + Send + Sync> }
static TERMS: Mutex<Option<HashMap<String, Term>>> = Mutex::new(None);

fn with<T>(f: impl FnOnce(&mut HashMap<String, Term>) -> T) -> T {
    let mut g = TERMS.lock().unwrap();
    f(g.get_or_insert_with(HashMap::new))
}

pub fn open(id: &str, cwd: &str, cols: u16, rows: u16, on_data: impl Fn(String) + Send + 'static, on_exit: impl FnOnce(Option<u32>) + Send + 'static) -> Result<(), String> {
    close(id);
    let pair = native_pty_system().openpty(PtySize { rows: rows.max(2), cols: cols.max(2), pixel_width: 0, pixel_height: 0 }).map_err(|e| e.to_string())?;
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    let mut cmd = CommandBuilder::new(&shell);
    cmd.arg("-l");
    cmd.cwd(cwd);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("PITH_TERMINAL", "1");
    let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
    let pid = child.process_id();
    std::thread::spawn(move || {
        // utf-8 can split across reads, hold the unfinished tail for the next one
        let mut buf = [0u8; 16384];
        let mut carry: Vec<u8> = vec![];
        loop {
            let n = match reader.read(&mut buf) { Ok(0) | Err(_) => break, Ok(n) => n };
            carry.extend_from_slice(&buf[..n]);
            let valid = match std::str::from_utf8(&carry) { Ok(_) => carry.len(), Err(e) if e.error_len().is_none() => e.valid_up_to(), Err(_) => carry.len() };
            let text = String::from_utf8_lossy(&carry[..valid]).into_owned();
            carry.drain(..valid);
            if !text.is_empty() { on_data(text); }
        }
        on_exit(pid);
    });
    with(|m| m.insert(id.into(), Term { master: pair.master, writer, child }));
    Ok(())
}

pub fn write(id: &str, data: &str) {
    with(|m| if let Some(t) = m.get_mut(id) { let _ = t.writer.write_all(data.as_bytes()); let _ = t.writer.flush(); });
}

pub fn resize(id: &str, cols: u16, rows: u16) {
    with(|m| if let Some(t) = m.get(id) { let _ = t.master.resize(PtySize { rows: rows.max(2), cols: cols.max(2), pixel_width: 0, pixel_height: 0 }); });
}

pub fn close(id: &str) {
    if let Some(mut t) = with(|m| m.remove(id)) { let _ = t.child.kill(); }
}

pub fn close_all() {
    let all: Vec<Term> = with(|m| m.drain().map(|(_, t)| t).collect());
    for mut t in all { let _ = t.child.kill(); }
}
