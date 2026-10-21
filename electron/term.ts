// Shells for the terminal pane, each behind shell-pty.py
import { spawn, type ChildProcess } from "node:child_process";
import { StringDecoder } from "node:string_decoder";
import type { Writable } from "node:stream";

type Term = { proc: ChildProcess; ctl: Writable };
const terms = new Map<string, Term>();

export function openTerm(id: string, helper: string, cwd: string, cols: number, rows: number, onData: (s: string) => void, onExit: (code: number | null) => void) {
  closeTerm(id);
  // -I: isolated, so nothing next to the helper can shadow a stdlib module
  const proc = spawn("python3", ["-I", helper, cwd], {
    cwd,
    env: { ...process.env, TERM: "xterm-256color", COLORTERM: "truecolor", PITH_TERMINAL: "1" },
    stdio: ["pipe", "pipe", "pipe", "pipe"],
  });
  const dec = new StringDecoder("utf8");
  proc.stdout!.on("data", (d: Buffer) => onData(dec.write(d)));
  proc.stderr!.on("data", (d: Buffer) => onData(d.toString()));
  proc.on("exit", (code) => { if (terms.get(id)?.proc === proc) terms.delete(id); onExit(code); });
  proc.on("error", (e) => onData(`\r\ncouldn't start a shell: ${e.message}\r\n`));
  const t = { proc, ctl: proc.stdio[3] as Writable };
  terms.set(id, t);
  resizeTerm(id, cols, rows);
}

export function writeTerm(id: string, data: string) { terms.get(id)?.proc.stdin?.write(data); }

export function resizeTerm(id: string, cols: number, rows: number) {
  const t = terms.get(id);
  if (t && cols > 0 && rows > 0) t.ctl.write(`${Math.floor(cols)} ${Math.floor(rows)}\n`);
}

export function closeTerm(id: string) {
  const t = terms.get(id);
  if (!t) return;
  terms.delete(id);
  t.proc.stdin?.end();
  t.proc.kill("SIGTERM");
}

export function closeAllTerms() { for (const id of [...terms.keys()]) closeTerm(id); }
