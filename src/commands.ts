// Slash commands the app handles itself. Anything else that starts with / goes to pi as typed
// (prompt templates like /review, skills, extension commands)
import { act, MODES } from "./actions";
import { app } from "./app";
import { api, type Mode } from "./lib/api";
import type { Tab } from "./tab";

export type Cmd = { name: string; desc: string; arg?: string; source: "app" | "prompt" | "skill" | "extension" };

export const BUILTIN: Cmd[] = [
  { name: "new", desc: "new session in this folder", source: "app" },
  { name: "compact", desc: "summarise older context to free room", arg: "[what to keep]", source: "app" },
  { name: "model", desc: "switch model", source: "app" },
  { name: "effort", desc: "how hard the model thinks", source: "app" },
  { name: "mode", desc: "ask · edits · plan · auto", arg: "[mode]", source: "app" },
  { name: "plan", desc: "switch to plan mode (read-only)", source: "app" },
  { name: "name", desc: "rename this session", arg: "<title>", source: "app" },
  { name: "clone", desc: "copy this session into a new one", source: "app" },
  { name: "export", desc: "save the session as an html page", source: "app" },
  { name: "copy", desc: "copy the last reply", source: "app" },
  { name: "image", desc: "generate an image", arg: "<prompt>", source: "app" },
  { name: "browser", desc: "open the browser pane", arg: "[url]", source: "app" },
  { name: "preview", desc: "open the preview pane", source: "app" },
  { name: "diff", desc: "open the changes pane", source: "app" },
  { name: "files", desc: "open the files pane", source: "app" },
  { name: "agents", desc: "live subagents", source: "app" },
  { name: "terminal", desc: "a shell in this folder", source: "app" },
  { name: "usage", desc: "tokens, cost, limits", source: "app" },
  { name: "settings", desc: "settings", source: "app" },
  { name: "stop", desc: "stop the current run", source: "app" },
];

export function commandsFor(tab: Tab | null): Cmd[] {
  const pi: Cmd[] = (tab?.commands ?? []).map((c) => ({ name: c.name, desc: c.description ?? "", source: c.source as Cmd["source"] }));
  const names = new Set(pi.map((c) => c.name));
  // before a session is running there's no command list from pi yet, the bundled templates are still there
  const bundled: Cmd[] = tab?.commands.length ? [] : [
    { name: "review", desc: "review the current changes for bugs", source: "prompt" },
    { name: "security-review", desc: "security review of the pending changes", source: "prompt" },
    { name: "simplify", desc: "clean up the changed code", source: "prompt" },
    { name: "design", desc: "design something visual", source: "prompt" },
    { name: "init", desc: "write an AGENTS.md", source: "prompt" },
    { name: "explain", desc: "explain part of the codebase", source: "prompt" },
  ];
  return [...BUILTIN.filter((b) => !names.has(b.name)), ...pi, ...bundled];
}

export async function runBuiltin(tab: Tab | null, text: string): Promise<boolean> {
  const m = text.match(/^\/(\S+)\s*([\s\S]*)$/);
  if (!m) return false;
  const [, name, rest] = m;
  const arg = rest.trim();
  if (!BUILTIN.some((b) => b.name === name)) return false;
  switch (name) {
    case "usage": app.show("usage"); return true;
    case "settings": app.show("settings"); return true;
  }
  if (!tab) { app.toast("open a session first"); return true; }
  switch (name) {
    case "new": app.fresh(tab.cwd, tab.kind); break;
    case "compact": void tab.compact(arg || undefined); break;
    case "model": act.pickModel(tab); break;
    case "effort": if (arg) void tab.setThinking(arg); else act.pickEffort(tab); break;
    case "mode":
      if (MODES.some((x) => x.id === arg)) tab.setMode(arg as Mode); else act.pickMode(tab);
      break;
    case "plan": tab.setMode("plan"); break;
    case "name": if (arg) void tab.rename(arg); else app.toast("/name needs a title"); break;
    case "clone": void tab.clone(); break;
    case "export": {
      const p = await tab.exportHtml();
      if (p) { app.toast(`saved ${p}`); void api.openPath(p); }
      break;
    }
    case "copy": { const t = await tab.lastText(); if (t) { void api.copy(t); app.toast("copied"); } break; }
    case "image":
      if (!arg) { app.show("images"); break; }
      void tab.send(`Generate this image with generate_image, then show it: ${arg}`);
      break;
    case "browser":
      if (arg) tab.browserUrl = /^[a-z]+:/i.test(arg) ? arg : `https://${arg}`;
      act.openPane(tab, "browser");
      break;
    case "preview": case "diff": case "files": case "agents": case "terminal": act.openPane(tab, name); break;
    case "stop": void tab.abort(); break;
  }
  return true;
}
