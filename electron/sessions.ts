// Reads pi's session files straight off disk: the sidebar list and every number on the usage page.
// Parsed once per file and kept until its size or mtime changes
import { readdirSync, readFileSync, statSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

export type Usage = { input: number; output: number; cacheRead: number; cacheWrite: number; cost: number };
// one assistant reply (or tool/compaction usage) worth of spend, compact so all of them fit in memory
export type Spend = [ts: number, model: string, input: number, output: number, cacheRead: number, cacheWrite: number, cost: number];

export type SessionInfo = {
  file: string;
  id: string;
  cwd: string;
  name?: string;
  firstPrompt: string;
  started: number;
  updated: number;
  messages: number;
  parent?: string;
  model?: string;
  usage: Usage;
};

type Parsed = { info: SessionInfo; spend: Spend[]; size: number; mtime: number };

export const agentDir = process.env.PI_CODING_AGENT_DIR || join(homedir(), ".pi", "agent");

export function sessionsRoot() {
  if (process.env.PI_CODING_AGENT_SESSION_DIR) return process.env.PI_CODING_AGENT_SESSION_DIR;
  try {
    const s = JSON.parse(readFileSync(join(agentDir, "settings.json"), "utf8"));
    if (typeof s.sessionDir === "string") return s.sessionDir.replace(/^~/, homedir());
  } catch { /* defaults */ }
  return join(agentDir, "sessions");
}

const cache = new Map<string, Parsed>();
const zero = (): Usage => ({ input: 0, output: 0, cacheRead: 0, cacheWrite: 0, cost: 0 });

function textOf(content: unknown): string {
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return "";
  return content.filter((c) => c?.type === "text").map((c) => c.text).join(" ");
}

function parse(file: string, size: number, mtime: number): Parsed {
  const info: SessionInfo = { file, id: "", cwd: "", firstPrompt: "", started: mtime, updated: mtime, messages: 0, usage: zero() };
  const spend: Spend[] = [];
  const add = (ts: number, model: string, u: any) => {
    if (!u) return;
    const row: Spend = [ts, model, u.input || 0, u.output || 0, u.cacheRead || 0, u.cacheWrite || 0, u.cost?.total || 0];
    if (!row[2] && !row[3] && !row[4] && !row[5] && !row[6]) return;
    spend.push(row);
    info.usage.input += row[2]; info.usage.output += row[3]; info.usage.cacheRead += row[4]; info.usage.cacheWrite += row[5]; info.usage.cost += row[6];
  };
  let text = "", last = 0;
  try { text = readFileSync(file, "utf8"); } catch { return { info, spend, size, mtime }; }
  for (const line of text.split("\n")) {
    if (!line) continue;
    let e: any;
    try { e = JSON.parse(line); } catch { continue; }
    const ts = Date.parse(e.timestamp) || mtime;
    switch (e.type) {
      case "session":
        info.id = e.id; info.cwd = e.cwd ?? ""; info.started = ts; info.parent = e.parentSession;
        break;
      case "session_info":
        info.name = e.name || undefined;
        break;
      case "model_change":
        info.model = `${e.provider}/${e.modelId}`;
        break;
      case "usage":
        add(ts, `${e.provider}/${e.model}`, e.usage);
        break;
      case "compaction":
      case "branch_summary":
        add(ts, info.model ?? "?", e.usage);
        break;
      case "message": {
        const m = e.message;
        if (!m || m.role === "system") break;
        info.messages++;
        const mts = m.timestamp || ts;
        if (m.role === "user" && !info.firstPrompt) info.firstPrompt = textOf(m.content).slice(0, 300);
        if (m.role === "assistant") {
          info.model = `${m.provider}/${m.model}`;
          add(mts, info.model, m.usage);
        }
        if (m.role === "toolResult") add(mts, info.model ?? "?", m.usage);
        last = Math.max(last, mts);
        break;
      }
    }
  }
  info.updated = last || mtime;
  return { info, spend, size, mtime };
}

function scan(): Parsed[] {
  const root = sessionsRoot(), seen = new Set<string>(), out: Parsed[] = [];
  let dirs: string[] = [];
  try { dirs = readdirSync(root); } catch { return out; }
  for (const d of dirs) {
    let files: string[] = [];
    try { files = readdirSync(join(root, d)).filter((f) => f.endsWith(".jsonl")); } catch { continue; }
    for (const f of files) {
      const file = join(root, d, f);
      seen.add(file);
      let st;
      try { st = statSync(file); } catch { continue; }
      let p = cache.get(file);
      if (!p || p.size !== st.size || p.mtime !== st.mtimeMs) {
        p = parse(file, st.size, st.mtimeMs);
        cache.set(file, p);
      }
      out.push(p);
    }
  }
  for (const k of cache.keys()) if (!seen.has(k)) cache.delete(k);
  return out;
}

export function listSessions(): SessionInfo[] {
  return scan().map((p) => p.info).filter((i) => i.id).sort((a, b) => b.updated - a.updated);
}

export function sessionSpend(file: string): Spend[] {
  return cache.get(file)?.spend ?? [];
}

export type UsageReport = {
  spend: { s: number; rows: Spend[] }[];
  sessions: { file: string; title: string; cwd: string; updated: number; usage: Usage }[];
};

// the renderer does the bucketing (local days, windows), main just hands over the rows per session
export function usageReport(): UsageReport {
  const parsed = scan().filter((p) => p.info.id);
  return {
    spend: parsed.map((p, s) => ({ s, rows: p.spend })),
    sessions: parsed.map((p) => ({ file: p.info.file, title: p.info.name || p.info.firstPrompt || "untitled", cwd: p.info.cwd, updated: p.info.updated, usage: p.info.usage })),
  };
}
