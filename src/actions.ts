// Cross-view actions. Filled in by whoever owns them at startup so views don't import each other in circles
import type { Tab, Img } from "./tab";

export const act = {
  openFile: (_tab: Tab, _path: string, _line?: number) => {},
  openPane: (_tab: Tab, _kind: string) => {},
  togglePane: (_tab: Tab, _kind: string) => {},
  previewHtml: (_tab: Tab, _html: string) => {},
  quote: (_tab: Tab, _text: string, _label?: string) => {},
  attach: (_tab: Tab, _img: Img) => {},
  lightbox: (_src: string, _caption?: string) => {},
  palette: () => {},
  pickModel: (_tab: Tab) => {},
  pickEffort: (_tab: Tab) => {},
  pickMode: (_tab: Tab) => {},
  runCommand: async (_tab: Tab | null, _text: string): Promise<boolean> => false,
  refreshDiff: (_tab: Tab) => {},
  // a subagent run, by whatever the caller knows about it, to something that opens its session. null when there's no session
  agentRun: (_q: AgentRef): (() => void) | null => null,
};

export type AgentRef = { sessionFile?: string; hubId?: number; task?: string; agent?: string; cwd?: string };

export const MODES = [
  { id: "ask", label: "ask", hint: "asks before edits and commands" },
  { id: "edits", label: "edits", hint: "edits files freely, asks before commands" },
  { id: "plan", label: "plan", hint: "read-only, ends with a plan" },
  { id: "auto", label: "auto", hint: "never asks" },
] as const;
