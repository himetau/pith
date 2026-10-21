import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
// The backend as the views see it (tauri commands and events), and the pi shapes the ui reads
export type Content = { type: "text"; text: string } | { type: "image"; data: string; mimeType: string };
export type Usage = { input: number; output: number; cacheRead: number; cacheWrite: number; totalTokens?: number; cost?: { total: number } };
export type Model = { id: string; name?: string; provider: string; reasoning?: boolean; input?: string[]; contextWindow?: number; maxTokens?: number; cost?: { input: number; output: number; cacheRead: number; cacheWrite: number } };
export type PiState = { model?: Model; thinkingLevel?: string; isStreaming?: boolean; isCompacting?: boolean; sessionFile?: string; sessionId?: string; sessionName?: string; messageCount?: number; pendingMessageCount?: number; autoCompactionEnabled?: boolean };
export type Stats = { tokens: { input: number; output: number; cacheRead: number; cacheWrite: number; total: number }; cost: number; contextUsage?: { tokens: number | null; contextWindow: number; percent: number | null }; toolCalls: number; userMessages: number; assistantMessages: number };
export type Rec = { type: string; [k: string]: any };
export type Resp = { type: "response"; success: boolean; data?: any; error?: string };
export type SessionInfo = { file: string; id: string; cwd: string; name?: string; firstPrompt: string; started: number; updated: number; messages: number; parent?: string; model?: string; usage: { input: number; output: number; cacheRead: number; cacheWrite: number; cost: number } };
export type Spend = [ts: number, model: string, input: number, output: number, cacheRead: number, cacheWrite: number, cost: number];
export type UsageReport = { spend: { s: number; rows: Spend[] }[]; sessions: { file: string; title: string; cwd: string; updated: number; usage: SessionInfo["usage"] }[] };
export type Settings = {
  piPath: string; extraArgs: string; defaultCwd: string; mode: Mode; font: string; proseFont: string; fontSize: number; scale: number; notify: boolean;
  budget: { window5h: number; week: number; month: number }; imageModel: string; imageCost: number; imageProvider: string; browserHome: string; designDir: string;
  pinned: string[]; maxLive: number; sidebar: boolean; paneWidth: number;
};
export type Mode = "ask" | "edits" | "plan" | "auto";
export type SysInfo = { home: string; piVersion: string; font: { family: string; weight: number } | null; hypr: { rounding: number; gap: number; border: number }; theme: { css: string; found: boolean; file: string }; imagesDir: string; agentDir: string; configDir: string; platform: string };
export type FileRead = { kind: "text"; text: string; size: number } | { kind: "image"; data: string; mimeType: string; size: number } | { kind: "binary" | "missing" | "big"; size: number };
export type GitStatus = { repo: false } | { repo: true; branch: string; files: { path: string; code: string; add: number; del: number }[] };
export type AgentDef = { name: string; description: string; tools: string; model: string; file: string; scope: "user" | "project"; body: string };

type Fn<A extends unknown[], R> = (...a: A) => Promise<R>;
export type WebSlot = { kind: "web" | "pv"; tab: string; url?: string; x: number; y: number; w: number; h: number };
export type Api = {
  start: Fn<[o: { tab: string; cwd: string; session?: string; kind?: string; model?: string; thinking?: string; mode?: string; name?: string }], { ok: boolean; state?: PiState; error?: string; pid?: number }>;
  cmd: Fn<[tab: string, cmd: Rec], Resp>;
  ui: Fn<[tab: string, id: string, payload: object], void>;
  stop: Fn<[tab: string], void>;
  setMode: Fn<[tab: string, mode: Mode], void>;
  live: Fn<[], string[]>;
  sessions: Fn<[], SessionInfo[]>;
  trashSession: Fn<[file: string], void>;
  meta: Fn<[], Record<string, { kind?: "chat" | "design"; mode?: Mode }>>;
  setMeta: Fn<[file: string, patch: { kind?: string; mode?: string }], void>;
  usage: Fn<[], UsageReport>;
  settings: Fn<[], Settings>;
  saveSettings: Fn<[patch: Partial<Settings>], Settings>;
  sys: Fn<[], SysInfo>;
  gitStatus: Fn<[cwd: string], GitStatus>;
  gitDiff: Fn<[cwd: string, file: string, untracked: boolean], string>;
  gitBranch: Fn<[cwd: string], string>;
  listFiles: Fn<[cwd: string], string[]>;
  read: Fn<[path: string], FileRead>;
  write: Fn<[path: string, text: string], void>;
  exists: Fn<[path: string], boolean>;
  pickDir: Fn<[def?: string], string | null>;
  pickFiles: Fn<[], string[]>;
  watchPreview: Fn<[tab: string, path: string], void>;
  unwatchPreview: Fn<[tab: string], void>;
  agentDefs: Fn<[cwd?: string], AgentDef[]>;
  saveAgent: Fn<[def: Partial<AgentDef>, cwd?: string, scope?: string], string>;
  agentHub: Fn<[], any>;
  agentSession: Fn<[file: string], { entries: any[]; leafId: string | null }>;
  images: Fn<[], { file: string; mtime: number; prompt: string }[]>;
  generateImage: Fn<[prompt: string, inputs?: { data: string; mimeType: string }[]], { files: string[]; images: { data: string; mimeType: string }[]; text: string }>;
  openExternal: Fn<[url: string], void>;
  openPath: Fn<[p: string], void>;
  reveal: Fn<[p: string], void>;
  copy: Fn<[t: string], void>;
  copyImage: Fn<[data: string], void>;
  notify: Fn<[title: string, body: string], void>;
  focused: Fn<[], boolean>;
  termOpen: Fn<[id: string, cwd: string, cols: number, rows: number], void>;
  termWrite: Fn<[id: string, data: string], void>;
  termResize: Fn<[id: string, cols: number, rows: number], void>;
  termClose: Fn<[id: string], void>;
  webLayout: Fn<[slots: WebSlot[]], void>;
  webNavigate: Fn<[tab: string, url: string], void>;
  webGo: Fn<[kind: "web" | "pv", tab: string, what: "back" | "forward" | "reload"], void>;
  webUrl: Fn<[kind: "web" | "pv", tab: string], string>;
  webEval: Fn<[kind: "web" | "pv", tab: string, js: string], void>;
  webClose: Fn<[tab: string], void>;
  minimize: Fn<[], void>;
  maximize: Fn<[], void>;
  close: Fn<[], void>;
  zoom: Fn<[scale: number], void>;
  // a file the ui can put in an <img>, through tauri's asset protocol
  fileSrc(path: string): string;
  on(ch: string, fn: (...a: any[]) => void): () => void;
  onDrop(fn: (paths: string[]) => void): void;
};

// tauri commands take named arguments; this keeps the positional api the views were written against
const call = <R,>(cmd: string, names: string[] = []) => (...a: unknown[]): Promise<R> => {
  const args: Record<string, unknown> = {};
  names.forEach((n, i) => { if (a[i] !== undefined) args[n] = a[i]; });
  return invoke<R>(cmd, args);
};

export const api: Api = {
  start: call("pi_start", ["o"]), cmd: call("pi_cmd", ["tab", "cmd"]), ui: call("pi_ui", ["tab", "id", "payload"]), stop: call("pi_stop", ["tab"]),
  setMode: call("pi_mode", ["tab", "mode"]), live: call("pi_live"),
  sessions: call("sessions_list"), trashSession: call("sessions_trash", ["file"]), meta: call("meta_all"), setMeta: call("meta_set", ["file", "patch"]), usage: call("usage_get"),
  settings: call("settings_get"), saveSettings: call("settings_set", ["patch"]), sys: call("sys_info"),
  gitStatus: call("git_status", ["cwd"]), gitDiff: call("git_diff", ["cwd", "file", "untracked"]), gitBranch: call("git_branch", ["cwd"]),
  listFiles: call("fs_list", ["cwd"]), read: call("fs_read", ["path"]), write: call("fs_write", ["path", "text"]), exists: call("fs_exists", ["path"]),
  pickDir: call("fs_pick_dir", ["def"]), pickFiles: call("fs_pick_files"),
  watchPreview: call("preview_watch", ["tab", "path"]), unwatchPreview: call("preview_unwatch", ["tab"]),
  agentDefs: call("agents_defs", ["cwd"]), saveAgent: call("agents_save", ["def", "cwd", "scope"]), agentHub: call("agents_hub"), agentSession: call("agents_session", ["file"]),
  images: call("images_list"), generateImage: call("images_generate", ["prompt", "inputs"]),
  openExternal: call("open_external", ["url"]), openPath: call("open_path", ["path"]), reveal: call("open_reveal", ["path"]),
  copy: call("clip_write", ["text"]), copyImage: call("clip_image", ["data"]), notify: call("notify", ["title", "body"]), focused: call("win_focused"),
  termOpen: call("term_open", ["id", "cwd", "cols", "rows"]), termWrite: call("term_write", ["id", "data"]), termResize: call("term_resize", ["id", "cols", "rows"]), termClose: call("term_close", ["id"]),
  webLayout: call("web_layout", ["slots"]), webNavigate: call("web_navigate", ["tab", "url"]), webGo: call("web_go", ["kind", "tab", "what"]),
  webUrl: call("web_url", ["kind", "tab"]), webEval: call("web_eval", ["kind", "tab", "js"]), webClose: call("web_close", ["tab"]),
  minimize: call("win_minimize"), maximize: call("win_maximize"), close: call("win_close"), zoom: call("win_zoom", ["scale"]),
  fileSrc: (p) => convertFileSrc(p),
  // events carry their arguments as an array, same order the views were written against
  on(ch, fn) {
    const off = listen<unknown>(ch, (e) => (Array.isArray(e.payload) ? fn(...e.payload) : fn(e.payload)));
    return () => { void off.then((u) => u()); };
  },
  onDrop(fn) {
    void getCurrentWebview().onDragDropEvent((e) => { if (e.payload.type === "drop") fn(e.payload.paths); });
  },
};
