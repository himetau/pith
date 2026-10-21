// Loaded into every pi that pith starts (-e). Gives the agent the app's panes as tools and asks pith
// before edits and commands, depending on the session's mode. Without PITH_BRIDGE set it does nothing
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { StringEnum } from "@earendil-works/pi-ai";
import { Type } from "typebox";
import { existsSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { isAbsolute, join, resolve } from "node:path";
import { generateImage } from "./imagegen.ts";

const BRIDGE = process.env.PITH_BRIDGE, TOKEN = process.env.PITH_TOKEN ?? "", TAB = process.env.PITH_TAB ?? "";

async function call<T = any>(op: string, args: object = {}): Promise<T> {
  const res = await fetch(`${BRIDGE}/call`, { method: "POST", headers: { "content-type": "application/json", "x-pith-token": TOKEN }, body: JSON.stringify({ tab: TAB, op, args }) });
  const j: any = await res.json().catch(() => ({ error: `bridge said ${res.status}` }));
  if (!res.ok || j.error) throw new Error(j.error ?? `bridge said ${res.status}`);
  return j.result as T;
}

const text = (t: string) => ({ type: "text" as const, text: t });

// read-only shell for plan mode, same idea as pi's plan-mode example
const WRITES = [/\b(rm|rmdir|mv|cp|mkdir|touch|chmod|chown|ln|tee|truncate|dd|shred|kill|pkill|killall|sudo|su|reboot|shutdown)\b/, /(^|[^<2&])>(?!&)/, />>/, /\b(npm|pnpm|yarn|bun)\s+(i|install|add|remove|uninstall|update|ci|link|publish|run)\b/, /\bpip\s+(install|uninstall)/, /\bgit\s+(add|commit|push|pull|merge|rebase|reset|checkout|switch|stash|cherry-pick|revert|tag|init|clone|clean|restore|apply|am)\b/, /\bsed\s+-i/, /\b(vim?|nano|emacs)\b/, /\bsystemctl\s+(start|stop|restart|enable|disable)/];
const readOnly = (cmd: string) => !WRITES.some((r) => r.test(cmd));
const WRITE_TOOLS = new Set(["edit", "write", "generate_image"]);

export default function (pi: ExtensionAPI) {
  if (!BRIDGE) return;
  const allowed = new Set<string>();

  pi.on("tool_call", async (event, ctx) => {
    const tool = event.toolName, input = event.input as Record<string, any>;
    let mode = "ask";
    try { mode = (await call<{ mode: string }>("mode")).mode; } catch { /* app gone, keep asking */ }
    const isWrite = WRITE_TOOLS.has(tool), isShell = tool === "bash";
    if (!isWrite && !isShell) return undefined;
    if (mode === "auto") return undefined;
    if (mode === "plan") {
      if (isShell && readOnly(String(input.command ?? ""))) return undefined;
      return { block: true, reason: "Plan mode is on, so nothing gets changed yet. Look around with read-only tools and finish with a concrete plan; the user switches modes when they want it carried out." };
    }
    if (mode === "edits" && isWrite) return undefined;
    if (allowed.has(tool)) return undefined;
    if (isShell && readOnly(String(input.command ?? "")) && /^\s*(ls|cat|head|tail|wc|pwd|git (status|diff|log|show)|rg|grep|find|fd|tree|file|stat|which|echo)\b/.test(String(input.command ?? ""))) return undefined;
    const pick = await ctx.ui.select(`pith:perm${JSON.stringify({ tool, input })}`, ["Allow", `Allow ${tool} for this session`, "Deny"]);
    if (pick === "Allow") return undefined;
    if (pick?.startsWith("Allow ")) { allowed.add(tool); return undefined; }
    return { block: true, reason: pick === undefined ? "The user didn't answer the permission prompt." : "The user denied this. Ask what they'd like instead, or try another way." };
  });

  pi.on("before_agent_start", async (event) => {
    if (event.messageType !== "user") return undefined;
    let mode = "ask";
    try { mode = (await call<{ mode: string }>("mode")).mode; } catch { return undefined; }
    if (mode !== "plan") return undefined;
    return { message: { customType: "pith-plan-mode", display: false, content: "[plan mode] Read-only for now: edit, write and changing shell commands are blocked. Explore, ask with ask_user if something is unclear, then write a numbered plan. Don't try to make the changes." } };
  });

  pi.registerTool({
    name: "browser",
    label: "Browser",
    description: "Drive the browser pane the user sees next to the chat. snapshot lists interactive elements as [eN] refs plus the page text; click/type take a ref (or a CSS selector, or x/y). Use screenshot when layout matters. Always snapshot again after the page changes before using refs.",
    parameters: Type.Object({
      action: StringEnum(["navigate", "snapshot", "screenshot", "click", "type", "key", "scroll", "back", "forward", "reload", "eval", "wait"] as const),
      url: Type.Optional(Type.String({ description: "for navigate" })),
      ref: Type.Optional(Type.String({ description: "element ref from the last snapshot, like e12" })),
      selector: Type.Optional(Type.String({ description: "CSS selector, when there's no ref" })),
      text: Type.Optional(Type.String({ description: "text to type" })),
      submit: Type.Optional(Type.Boolean({ description: "press enter after typing" })),
      clear: Type.Optional(Type.Boolean({ description: "clear the field before typing" })),
      key: Type.Optional(Type.String({ description: "key to press, like Enter, Escape, ctrl+a" })),
      x: Type.Optional(Type.Number()),
      y: Type.Optional(Type.Number()),
      dy: Type.Optional(Type.Number({ description: "pixels to scroll, negative is up" })),
      script: Type.Optional(Type.String({ description: "javascript to evaluate in the page, for eval" })),
      ms: Type.Optional(Type.Number({ description: "for wait" })),
    }),
    async execute(_id, params) {
      const r = await call<{ text: string; image?: { data: string; mimeType: string } }>("browser", params);
      return { content: r.image ? [text(r.text), { type: "image" as const, ...r.image }] : [text(r.text)], details: { url: (r.text.match(/^(?:url:|now at) (\S+)/m) ?? [])[1] } };
    },
  });

  pi.registerTool({
    name: "preview",
    label: "Preview",
    description: "Open an HTML file (or a URL, like a dev server) in the preview pane next to the chat. It reloads by itself when the file changes, so call this once per design, not after every edit. The user can click elements there to comment on them.",
    parameters: Type.Object({
      path: Type.Optional(Type.String({ description: "HTML file to show" })),
      url: Type.Optional(Type.String({ description: "URL to show instead of a file" })),
      title: Type.Optional(Type.String()),
    }),
    async execute(_id, params, _signal, _onUpdate, ctx) {
      if (!params.path && !params.url) throw new Error("give a path or a url");
      const path = params.path ? (isAbsolute(params.path) ? params.path : resolve(ctx.cwd, params.path)) : undefined;
      if (path && !existsSync(path)) throw new Error(`${path} doesn't exist yet, write it first`);
      await call("preview", { path, url: params.url, title: params.title });
      return { content: [text(`showing ${path ?? params.url} in the preview pane`)], details: { path, url: params.url } };
    },
  });

  pi.registerTool({
    name: "ask_user",
    label: "Ask",
    description: "Ask the user a question with a few options and wait for the answer. Use it when you're blocked on a decision that's theirs to make, not for confirmation of things you can decide. Put the recommended option first.",
    parameters: Type.Object({
      question: Type.String(),
      options: Type.Array(Type.Object({ label: Type.String(), description: Type.Optional(Type.String()) }), { minItems: 2, maxItems: 6 }),
      multi: Type.Optional(Type.Boolean({ description: "let them pick several" })),
    }),
    executionMode: "sequential",
    async execute(_id, params, _signal, _onUpdate, ctx) {
      const answer = await ctx.ui.input(`pith:ask${JSON.stringify(params)}`, "");
      if (answer === undefined) return { content: [text("The user dismissed the question without answering.")], details: { answer: null } };
      return { content: [text(`The user answered: ${answer}`)], details: { answer } };
    },
  });

  pi.registerTool({
    name: "todo_write",
    label: "Tasks",
    description: "Keep a visible task list for multi-step work. Send the whole list every time with each item's status; keep exactly one in_progress while working. Skip it for small one-step requests.",
    parameters: Type.Object({
      todos: Type.Array(Type.Object({ content: Type.String(), status: StringEnum(["pending", "in_progress", "completed"] as const) })),
    }),
    async execute(_id, params) {
      const done = params.todos.filter((t) => t.status === "completed").length;
      return { content: [text(`task list updated, ${done}/${params.todos.length} done`)], details: undefined };
    },
  });

  pi.registerTool({
    name: "report_findings",
    label: "Findings",
    description: "Report code review findings so the app can show them as cards linked to the diff. Only include findings you verified. Rank most severe first. Call it once at the end of a review, with an empty list if nothing held up.",
    parameters: Type.Object({
      summary: Type.Optional(Type.String({ description: "one or two sentences on the overall state" })),
      findings: Type.Array(Type.Object({
        file: Type.String({ description: "repo-relative path" }),
        line: Type.Optional(Type.Number()),
        severity: StringEnum(["high", "medium", "low"] as const),
        category: Type.Optional(Type.String({ description: "short kebab-case slug, like correctness or security" })),
        title: Type.String({ description: "the defect in under 60 characters" }),
        detail: Type.String({ description: "what goes wrong, with the concrete input or state that triggers it" }),
        fix: Type.Optional(Type.String({ description: "the suggested change" })),
      })),
    }),
    async execute(_id, params) {
      return { content: [text(`reported ${params.findings.length} finding${params.findings.length === 1 ? "" : "s"}`)], details: undefined };
    },
  });

  pi.registerTool({
    name: "generate_image",
    label: "Image",
    description: "Generate an image from a prompt, or edit/combine existing images by passing their paths. Saves to disk and shows it in the chat. Write a specific prompt: subject, style, composition, colours, any text that has to appear.",
    parameters: Type.Object({
      prompt: Type.String(),
      images: Type.Optional(Type.Array(Type.String(), { description: "paths of input images to edit or use as reference" })),
      aspect: Type.Optional(Type.String({ description: "like 1:1, 16:9, 9:16" })),
      save_to: Type.Optional(Type.String({ description: "folder to save into, defaults to pith's image library" })),
    }),
    async execute(_id, params, _signal, _onUpdate, ctx) {
      const s = await call<{ model: string; provider: string; dir: string; pi: string }>("image-settings");
      const inputs = (params.images ?? []).map((p) => {
        const file = p.startsWith("~") ? join(homedir(), p.slice(1)) : isAbsolute(p) ? p : resolve(ctx.cwd, p);
        const ext = file.split(".").pop()!.toLowerCase();
        return { data: readFileSync(file).toString("base64"), mimeType: ext === "png" ? "image/png" : ext === "webp" ? "image/webp" : "image/jpeg" };
      });
      const outDir = params.save_to ? resolve(ctx.cwd, params.save_to) : s.dir;
      const r = await generateImage({ prompt: params.prompt, inputs, model: s.model, provider: s.provider, outDir, library: s.dir, pi: s.pi, aspect: params.aspect });
      return {
        content: [text(`saved ${r.files.join(", ")}${r.text ? `\n${r.text}` : ""}`), ...r.images.map((im) => ({ type: "image" as const, ...im }))],
        details: { files: r.files, model: s.model },
      };
    },
  });
}
