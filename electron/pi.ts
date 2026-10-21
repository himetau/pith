// One `pi --mode rpc` child per open session. JSONL both ways, responses matched back by id
import { spawn, type ChildProcess } from "node:child_process";

export type Rec = { type: string; [k: string]: any };

export class PiProc {
  private proc: ChildProcess;
  private pending = new Map<string, (r: Rec) => void>();
  private seq = 0;
  private buf: Buffer[] = [];
  stderr = "";
  exited = false;

  constructor(bin: string, args: string[], cwd: string, env: NodeJS.ProcessEnv, private onRec: (r: Rec) => void, onExit: (code: number | null, stderr: string) => void) {
    this.proc = spawn(bin, ["--mode", "rpc", ...args], { cwd, env, stdio: ["pipe", "pipe", "pipe"] });
    this.proc.stdout!.on("data", (d: Buffer) => this.read(d));
    // keep the tail only, it's for the error message when pi dies
    this.proc.stderr!.on("data", (d: Buffer) => { this.stderr = (this.stderr + d.toString()).slice(-8000); });
    this.proc.on("error", (e) => { this.stderr += `\n${e.message}`; });
    this.proc.on("exit", (code) => {
      this.exited = true;
      for (const done of this.pending.values()) done({ type: "response", success: false, error: "pi exited" });
      this.pending.clear();
      onExit(code, this.stderr);
    });
  }

  get pid() { return this.proc.pid ?? null; }

  // split on LF bytes only. readline would also break on U+2028 inside json strings
  private read(d: Buffer) {
    let start = 0;
    for (let i = 0; i < d.length; i++) {
      if (d[i] !== 10) continue;
      this.buf.push(d.subarray(start, i));
      start = i + 1;
      let line = Buffer.concat(this.buf).toString("utf8");
      this.buf = [];
      if (line.endsWith("\r")) line = line.slice(0, -1);
      if (!line.trim()) continue;
      let rec: Rec;
      try { rec = JSON.parse(line); } catch { continue; }
      if (rec.type === "response" && rec.id && this.pending.has(rec.id)) {
        this.pending.get(rec.id)!(rec);
        this.pending.delete(rec.id);
      } else this.onRec(rec);
    }
    if (start < d.length) this.buf.push(d.subarray(start));
  }

  private write(obj: object) {
    if (this.exited || !this.proc.stdin?.writable) return false;
    this.proc.stdin.write(JSON.stringify(obj) + "\n");
    return true;
  }

  send(cmd: Rec): Promise<Rec> {
    const id = cmd.id ?? `p${++this.seq}`;
    return new Promise((resolve) => {
      if (!this.write({ ...cmd, id })) return resolve({ type: "response", success: false, error: "pi isn't running" });
      this.pending.set(id, resolve);
    });
  }

  uiResponse(id: string, payload: object) { this.write({ type: "extension_ui_response", id, ...payload }); }

  stop() {
    if (this.exited) return;
    this.proc.stdin?.end();
    setTimeout(() => { if (!this.exited) this.proc.kill("SIGTERM"); }, 3000).unref();
  }
}
