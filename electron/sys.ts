// Everything main reads off the machine that isn't a pi session: matugen colours, kitty's font,
// hyprland's gaps, git, files, agent definitions, settings
import { execFile, execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { basename, extname, join, relative } from "node:path";
import { agentDir } from "./sessions";

const home = homedir();
export const configDir = join(process.env.XDG_CONFIG_HOME || join(home, ".config"), "pith");
export const dataDir = join(process.env.XDG_DATA_HOME || join(home, ".local", "share"), "pith");
export const themeFile = join(configDir, "colors.css");
export const imagesDir = join(dataDir, "images");
mkdirSync(configDir, { recursive: true });
mkdirSync(imagesDir, { recursive: true });

export type Settings = {
  piPath: string;
  extraArgs: string;
  defaultCwd: string;
  mode: "ask" | "edits" | "plan" | "auto";
  font: string;
  proseFont: string;
  fontSize: number;
  notify: boolean;
  budget: { window5h: number; week: number; month: number };
  imageModel: string;
  imageCost: number;
  imageProvider: string;
  browserHome: string;
  designDir: string;
  pinned: string[];
  maxLive: number;
  sidebar: boolean;
  paneWidth: number;
};

const defaults: Settings = {
  piPath: "pi",
  extraArgs: "",
  defaultCwd: home,
  mode: "ask",
  font: "",
  proseFont: "",
  fontSize: 14,
  notify: true,
  budget: { window5h: 0, week: 0, month: 0 },
  imageModel: "gemini-3.1-flash-image",
  imageCost: 0,
  imageProvider: "my-endpoint",
  browserHome: "https://duckduckgo.com",
  designDir: join(home, "Designs"),
  pinned: [],
  maxLive: 6,
  sidebar: true,
  paneWidth: 520,
};

const settingsFile = join(configDir, "settings.json");
export function loadSettings(): Settings {
  try { return { ...defaults, ...JSON.parse(readFileSync(settingsFile, "utf8")) }; } catch { return { ...defaults }; }
}
export function saveSettings(patch: Partial<Settings>): Settings {
  const next = { ...loadSettings(), ...patch };
  writeFileSync(settingsFile, JSON.stringify(next, null, 2) + "\n");
  return next;
}

export function readTheme(): { css: string; found: boolean; file: string } {
  try { return { css: readFileSync(themeFile, "utf8"), found: true, file: themeFile }; } catch { return { css: "", found: false, file: themeFile }; }
}

// the terminal's font is the app's font unless settings say otherwise. kitty writes "Maple Mono medium",
// the weight word isn't part of the family
export function terminalFont(): { family: string; weight: number } | null {
  try {
    const conf = readFileSync(join(home, ".config", "kitty", "kitty.conf"), "utf8");
    const m = conf.match(/^\s*font_family\s+(.+)$/m);
    if (!m) return null;
    const weights: Record<string, number> = { thin: 100, light: 300, regular: 400, medium: 500, semibold: 600, bold: 700 };
    const words = m[1].trim().split(/\s+/);
    const last = words[words.length - 1].toLowerCase();
    if (last in weights && words.length > 1) return { family: words.slice(0, -1).join(" "), weight: weights[last] };
    return { family: words.join(" "), weight: 400 };
  } catch { return null; }
}

// pane gaps, borders and corners match the compositor's windows
export function hyprLook(): { rounding: number; gap: number; border: number } {
  const get = (opt: string) => {
    try {
      const j = JSON.parse(execFileSync("hyprctl", ["-j", "getoption", opt], { encoding: "utf8", timeout: 1500 }));
      return typeof j.int === "number" ? j.int : parseInt(String(j.custom ?? "").split(" ")[0]) || null;
    } catch { return null; }
  };
  return { rounding: get("decoration:rounding") ?? 8, gap: get("general:gaps_in") ?? 5, border: get("general:border_size") ?? 2 };
}

const run = (cmd: string, args: string[], cwd: string, max = 20 * 1024 * 1024) => new Promise<string>((resolve) => {
  execFile(cmd, args, { cwd, maxBuffer: max, timeout: 15000 }, (_e, out) => resolve(out ?? ""));
});

export async function gitStatus(cwd: string) {
  const inside = (await run("git", ["rev-parse", "--is-inside-work-tree"], cwd)).trim() === "true";
  if (!inside) return { repo: false as const };
  const [branch, status, stat] = await Promise.all([
    run("git", ["rev-parse", "--abbrev-ref", "HEAD"], cwd),
    run("git", ["status", "--porcelain=v1", "-z", "-uall"], cwd),
    run("git", ["diff", "HEAD", "--numstat"], cwd),
  ]);
  const nums = new Map<string, [number, number]>();
  for (const l of stat.split("\n")) {
    const [a, d, f] = l.split("\t");
    if (f) nums.set(f, [parseInt(a) || 0, parseInt(d) || 0]);
  }
  const files: { path: string; code: string; add: number; del: number }[] = [];
  const parts = status.split("\0");
  for (let i = 0; i < parts.length; i++) {
    const p = parts[i];
    if (p.length < 4) continue;
    const code = p.slice(0, 2), path = p.slice(3);
    if (code[0] === "R" || code[0] === "C") i++; // rename carries the old path as the next field
    let [add, del] = nums.get(path) ?? [0, 0];
    if (code === "??") { try { add = readFileSync(join(cwd, path), "utf8").split("\n").length; } catch { /* binary or gone */ } }
    files.push({ path, code, add, del });
  }
  return { repo: true as const, branch: branch.trim(), files };
}

export async function gitDiff(cwd: string, file: string, untracked: boolean) {
  if (untracked) return run("git", ["diff", "--no-index", "--no-color", "--", "/dev/null", file], cwd);
  return run("git", ["diff", "HEAD", "--no-color", "-U4", "--", file], cwd);
}

export async function gitBranch(cwd: string) {
  return (await run("git", ["rev-parse", "--abbrev-ref", "HEAD"], cwd)).trim();
}

// for @ mentions. git knows what's ignored, outside a repo fall back to a shallow walk
export async function listFiles(cwd: string): Promise<string[]> {
  const out = await run("git", ["ls-files", "--cached", "--others", "--exclude-standard"], cwd);
  if (out.trim()) return out.split("\n").filter(Boolean).slice(0, 20000);
  const files: string[] = [];
  const walk = (dir: string, depth: number) => {
    if (depth > 4 || files.length > 4000) return;
    let ents;
    try { ents = readdirSync(dir, { withFileTypes: true }); } catch { return; }
    for (const e of ents) {
      if (e.name.startsWith(".") || e.name === "node_modules") continue;
      const p = join(dir, e.name);
      if (e.isDirectory()) walk(p, depth + 1);
      else files.push(relative(cwd, p));
    }
  };
  walk(cwd, 0);
  return files;
}

const imageTypes: Record<string, string> = { ".png": "image/png", ".jpg": "image/jpeg", ".jpeg": "image/jpeg", ".gif": "image/gif", ".webp": "image/webp", ".svg": "image/svg+xml", ".avif": "image/avif" };
export const mimeOf = (p: string) => imageTypes[extname(p).toLowerCase()];

export function readAny(path: string): { kind: "text"; text: string; size: number } | { kind: "image"; data: string; mimeType: string; size: number } | { kind: "binary" | "missing" | "big"; size: number } {
  let st;
  try { st = statSync(path); } catch { return { kind: "missing", size: 0 }; }
  const mime = mimeOf(path);
  if (mime) {
    if (st.size > 25 * 1024 * 1024) return { kind: "big", size: st.size };
    return { kind: "image", data: readFileSync(path).toString("base64"), mimeType: mime, size: st.size };
  }
  if (st.size > 3 * 1024 * 1024) return { kind: "big", size: st.size };
  const buf = readFileSync(path);
  if (buf.subarray(0, 8000).includes(0)) return { kind: "binary", size: st.size };
  return { kind: "text", text: buf.toString("utf8"), size: st.size };
}

export type AgentDef = { name: string; description: string; tools: string; model: string; file: string; scope: "user" | "project"; body: string };

function parseAgent(file: string, scope: AgentDef["scope"]): AgentDef | null {
  try {
    const text = readFileSync(file, "utf8");
    const m = text.match(/^---\n([\s\S]*?)\n---\n?([\s\S]*)$/);
    const fm: Record<string, string> = {};
    for (const l of (m?.[1] ?? "").split("\n")) {
      const i = l.indexOf(":");
      if (i > 0) fm[l.slice(0, i).trim()] = l.slice(i + 1).trim().replace(/^["']|["']$/g, "");
    }
    return { name: fm.name || basename(file, ".md"), description: fm.description ?? "", tools: fm.tools ?? "", model: fm.model ?? "", file, scope, body: (m ? m[2] : text).trim() };
  } catch { return null; }
}

export function agentDefs(cwd?: string): AgentDef[] {
  const out: AgentDef[] = [];
  const read = (dir: string, scope: AgentDef["scope"]) => {
    try { for (const f of readdirSync(dir)) if (f.endsWith(".md")) { const a = parseAgent(join(dir, f), scope); if (a) out.push(a); } } catch { /* none */ }
  };
  read(join(agentDir, "agents"), "user");
  if (cwd) read(join(cwd, ".pi", "agents"), "project");
  return out;
}

export function saveAgent(def: Omit<AgentDef, "file" | "scope"> & { file?: string }, cwd?: string, scope: AgentDef["scope"] = "user"): string {
  const dir = scope === "project" && cwd ? join(cwd, ".pi", "agents") : join(agentDir, "agents");
  mkdirSync(dir, { recursive: true });
  const file = def.file && existsSync(def.file) ? def.file : join(dir, `${def.name.replace(/[^\w.-]+/g, "-")}.md`);
  const fm = [`name: ${def.name}`, `description: ${def.description}`, def.tools && `tools: ${def.tools}`, def.model && `model: ${def.model}`].filter(Boolean).join("\n");
  writeFileSync(file, `---\n${fm}\n---\n\n${def.body.trim()}\n`);
  return file;
}

const RUNNING = new Set(["spawning", "thinking", "tool", "streaming"]);
const alive = (pid: number) => { try { process.kill(pid, 0); return true; } catch (e) { return (e as NodeJS.ErrnoException).code === "EPERM"; } };

// the extension can die mid-run and leave agents marked running forever, so check their pid
export function readAgentHub(): unknown {
  try {
    const hub = JSON.parse(readFileSync(join(agentDir, "runtime", "agents-state.json"), "utf8"));
    for (const a of hub.agents ?? []) if (RUNNING.has(a.status) && (!a.pid || !alive(a.pid))) a.status = "aborted";
    return hub;
  } catch { return null; }
}

export function piVersion(bin: string): string {
  try { return execFileSync(bin, ["--version"], { encoding: "utf8", timeout: 5000 }).trim(); } catch { return "not found"; }
}

export function listImages(): { file: string; mtime: number; prompt: string }[] {
  const out: { file: string; mtime: number; prompt: string }[] = [];
  const walk = (dir: string) => {
    let ents;
    try { ents = readdirSync(dir, { withFileTypes: true }); } catch { return; }
    for (const e of ents) {
      const p = join(dir, e.name);
      if (e.isDirectory()) walk(p);
      else if (mimeOf(p)) {
        let prompt = "";
        try { prompt = readFileSync(p.replace(/\.\w+$/, ".txt"), "utf8"); } catch { /* no sidecar */ }
        out.push({ file: p, mtime: statSync(p).mtimeMs, prompt });
      }
    }
  };
  walk(imagesDir);
  return out.sort((a, b) => b.mtime - a.mtime);
}

// what pith knows about a session that pi doesn't keep: design or chat, and its mode
const metaFile = join(configDir, "sessions.json");
export type SessionMeta = { kind?: "chat" | "design"; mode?: string };
export function loadMeta(): Record<string, SessionMeta> {
  try { return JSON.parse(readFileSync(metaFile, "utf8")); } catch { return {}; }
}
export function saveMeta(file: string, patch: SessionMeta) {
  const all = loadMeta();
  all[file] = { ...all[file], ...patch };
  writeFileSync(metaFile, JSON.stringify(all, null, 1));
}
