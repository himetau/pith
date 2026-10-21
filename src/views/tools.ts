// How each tool call shows up in the transcript. One line by default, the useful part on click
import { act } from "../actions";
import { h, type Child } from "../lib/dom";
import { cost, dur, tilde, tokens } from "../lib/fmt";
import { icon } from "../lib/icons";
import { highlight, langOf, md, mdInline } from "../lib/md";
import type { Tab, ToolBlock, ToolResult } from "../tab";

export const opened = new Set<string>();
export const closed = new Set<string>();

const textOf = (r?: ToolResult) => (r?.content ?? []).filter((c) => c.type === "text").map((c) => (c as { text: string }).text).join("\n");
const imagesOf = (r?: ToolResult) => (r?.content ?? []).filter((c) => c.type === "image") as { data: string; mimeType: string }[];
const lines = (s: string) => (s ? s.replace(/\n$/, "").split("\n").length : 0);
const nLines = (s: string, word = "line") => { const n = lines(s); return `${n} ${n === 1 ? word : word + "s"}`; };
const one = (s: string, n = 120) => { const l = String(s ?? "").replace(/\s+/g, " ").trim(); return l.length > n ? l.slice(0, n - 1) + "…" : l; };

// args arrive as a growing json string while the model writes them. good enough to show the path early
function partialArgs(tb: ToolBlock): any {
  if (tb.args && Object.keys(tb.args).length) return tb.args;
  const out: any = {};
  for (const k of ["path", "command", "pattern", "url", "action", "prompt", "question"]) {
    const m = tb.argsText.match(new RegExp(`"${k}"\\s*:\\s*"((?:[^"\\\\]|\\\\.)*)`));
    if (m) { try { out[k] = JSON.parse(`"${m[1]}"`); } catch { out[k] = m[1]; } }
  }
  return out;
}

type Kind = { verb: string; cls: string; target: Child; meta?: Child; body?: () => Child; auto?: boolean; path?: string; line?: number };

function pathTarget(tab: Tab, p: string, line?: number, extra?: string) {
  if (!p) return h("span.dim", "…");
  return h("span.tpath", { onclick: (e: Event) => { e.stopPropagation(); act.openFile(tab, p, line); }, title: p }, tilde(p), extra ? h("span.dim", extra) : null);
}

function codeBlock(text: string, lang?: string, start = 1) {
  const pre = h("pre.tool-code");
  const ls = text.split("\n");
  const gutter = ls.map((_, i) => String(start + i)).join("\n");
  pre.append(h("code.gutter", gutter), h("code.hljs", { html: highlight(text, lang) }));
  return pre;
}

export function diffView(diff: string) {
  const box = h("div.diff");
  for (const raw of diff.split("\n")) {
    const m = raw.match(/^([+\- ])\s*(\d+)\s(.*)$/);
    if (!m) { box.append(h("div.dl.gap", raw.trim() || "…")); continue; }
    const [, sign, num, code] = m;
    box.append(h(`div.dl${sign === "+" ? ".add" : sign === "-" ? ".del" : ""}`, h("span.dn", num), h("span.ds", sign === " " ? " " : sign), h("span.dc", code.replace(/\t/g, "  "))));
  }
  return box;
}

// before pi sends its numbered diff (streaming, or a failed edit) the edit args are all we have
export function editsView(edits: { oldText: string; newText: string }[]) {
  const box = h("div.diff");
  edits.forEach((e, i) => {
    if (i) box.append(h("div.dl.gap", "…"));
    for (const l of String(e.oldText ?? "").split("\n")) box.append(h("div.dl.del", h("span.dn", ""), h("span.ds", "-"), h("span.dc", l)));
    for (const l of String(e.newText ?? "").split("\n")) box.append(h("div.dl.add", h("span.dn", ""), h("span.ds", "+"), h("span.dc", l)));
  });
  return box;
}

function diffCounts(diff: string) {
  let a = 0, d = 0;
  for (const l of diff.split("\n")) { if (/^\+\s*\d/.test(l)) a++; else if (/^-\s*\d/.test(l)) d++; }
  return { a, d };
}

function imgs(list: { data: string; mimeType: string }[], cls = "tool-imgs") {
  return h(`div.${cls}`, list.map((im) => {
    const src = `data:${im.mimeType};base64,${im.data}`;
    return h("img", { src, onclick: (e: Event) => { e.stopPropagation(); act.lightbox(src); } });
  }));
}

function outputBlock(text: string, tail = false) {
  const t = tail && text.length > 6000 ? "…" + text.slice(-6000) : text.length > 60000 ? text.slice(0, 60000) + "\n…" : text;
  return h("pre.tool-out", t || "(no output)");
}

function describe(tb: ToolBlock, tab: Tab): Kind {
  const a = partialArgs(tb), r = tb.result ?? tb.partial, out = textOf(r);
  switch (tb.name) {
    case "read": {
      const range = a.offset || a.limit ? `:${a.offset ?? 1}${a.limit ? `-${(a.offset ?? 1) + a.limit - 1}` : ""}` : "";
      const im = imagesOf(r);
      return {
        verb: "read", cls: "t-read", target: pathTarget(tab, a.path, a.offset, range),
        meta: im.length ? "image" : r && !tb.result?.isError ? nLines(out) : undefined,
        body: () => im.length ? imgs(im) : codeBlock(out, langOf(a.path ?? ""), a.offset ?? 1),
      };
    }
    case "write": {
      const content = String(a.content ?? "");
      return {
        verb: "write", cls: "t-write", target: pathTarget(tab, a.path), meta: content ? h("span.add", `+${lines(content)}`) : tb.status === "streaming" ? "writing…" : undefined,
        body: () => tb.result?.isError ? outputBlock(out) : codeBlock(content, langOf(a.path ?? "")),
      };
    }
    case "edit": {
      const diff: string | undefined = tb.result?.details?.diff;
      const c = diff ? diffCounts(diff) : null;
      const edits = Array.isArray(a.edits) ? a.edits : a.oldText != null ? [{ oldText: a.oldText, newText: a.newText }] : [];
      return {
        verb: "edit", cls: "t-edit", target: pathTarget(tab, a.path),
        meta: c ? h("span", h("span.add", `+${c.a}`), " ", h("span.del", `-${c.d}`)) : edits.length > 1 ? `${edits.length} edits` : undefined,
        auto: !!diff && diff.split("\n").length <= 40,
        body: () => tb.result?.isError ? h("div", outputBlock(out), edits.length ? editsView(edits) : null) : diff ? diffView(diff) : editsView(edits),
      };
    }
    case "bash": {
      const exit = tb.result?.details?.exitCode;
      return {
        verb: "$", cls: "t-bash", target: h("span.tcmd", one(a.command ?? "", 140)),
        meta: tb.result?.isError ? h("span.del", exit != null ? `exit ${exit}` : "failed") : out ? nLines(out) : undefined,
        auto: tb.status === "running" && !!out,
        body: () => h("div", lines(a.command ?? "") > 1 || (a.command ?? "").length > 140 ? h("pre.tool-cmd", { html: highlight(a.command, "bash") }) : null, outputBlock(out, tb.status === "running")),
      };
    }
    case "grep": case "find": case "ls": {
      const target = tb.name === "ls" ? pathTarget(tab, a.path ?? ".") : h("span", h("span.tpat", a.pattern ?? ""), a.path ? h("span.dim", ` in ${tilde(a.path)}`) : null);
      return { verb: tb.name, cls: "t-search", target, meta: r ? nLines(out.trim(), "result") : undefined, body: () => outputBlock(out) };
    }
    case "browser": {
      const what = a.action === "navigate" ? a.url : a.action === "type" ? JSON.stringify(a.text ?? "") : a.ref ?? a.selector ?? a.key ?? a.script ?? "";
      const im = imagesOf(r);
      return {
        verb: "web", cls: "t-web", target: h("span", h("span.tact", a.action ?? "…"), " ", h("span.dim", one(String(what ?? ""), 90))),
        meta: im.length ? "screenshot" : undefined, auto: im.length > 0,
        body: () => h("div", im.length ? imgs(im, "tool-shot") : null, out && a.action !== "screenshot" ? outputBlock(out) : null),
      };
    }
    case "preview":
      return {
        verb: "preview", cls: "t-preview", target: a.path ? pathTarget(tab, a.path) : h("span", a.url ?? "…"),
        meta: h("button.mini", { onclick: (e: Event) => { e.stopPropagation(); tab.preview = { path: a.path, url: a.url, title: a.title }; act.openPane(tab, "preview"); } }, "show"),
      };
    case "generate_image": {
      const im = imagesOf(tb.result);
      const files: string[] = tb.result?.details?.files ?? [];
      return {
        verb: "image", cls: "t-image", target: h("span.dim", one(a.prompt ?? "", 110)), auto: true,
        meta: files[0] ? h("span.tpath", { title: files[0], onclick: (e: Event) => { e.stopPropagation(); act.openFile(tab, files[0]); } }, files[0].split("/").pop()) : undefined,
        body: () => im.length ? imgs(im, "gen-imgs") : tb.status === "running" || tb.status === "queued" ? h("div.gen-wait", h("span", { "data-spin": "" }), " painting") : tb.result?.isError ? outputBlock(out) : null,
      };
    }
    case "ask_user":
      return {
        verb: "ask", cls: "t-ask", target: h("span", one(a.question ?? "", 120)), auto: !!tb.result,
        body: () => tb.result ? h("div.ask-done", h("span.dim", "answer "), h("span", tb.result.details?.answer ?? "dismissed")) : null,
      };
    case "todo_write": {
      const todos: { content: string; status: string }[] = a.todos ?? [];
      const done = todos.filter((t) => t.status === "completed").length;
      const cur = todos.find((t) => t.status === "in_progress");
      return { verb: "tasks", cls: "t-todo", target: h("span", `${done}/${todos.length}`, cur ? h("span.dim", ` · ${one(cur.content, 80)}`) : null), body: () => todoList(todos) };
    }
    case "report_findings":
      return { verb: "review", cls: "t-review", target: h("span", `${(a.findings ?? []).length} findings`), auto: true, body: () => findingsCard(tab, a) };
    case "subagent": case "summon_agent":
      return agentsKind(tb, a);
    default: {
      const argStr = one(JSON.stringify(a ?? {}), 110);
      return {
        verb: tb.name.replace(/^mcp__/, "").replace(/__/g, ":"), cls: "t-other", target: h("span.dim", argStr), meta: r && out ? nLines(out) : undefined,
        body: () => h("div", h("pre.tool-cmd", JSON.stringify(a, null, 2)), imagesOf(r).length ? imgs(imagesOf(r)) : null, out ? outputBlock(out) : null),
      };
    }
  }
}

export function todoList(todos: { content: string; status: string }[]) {
  return h("ul.todos", todos.map((t) => h(`li.todo.${t.status}`, h("span.tbox", t.status === "completed" ? icon("check") : t.status === "in_progress" ? h("span", { "data-spin": "" }) : ""), h("span.tt", t.content))));
}

function findingsCard(tab: Tab, a: any) {
  const f: any[] = a.findings ?? [];
  return h("div.findings",
    a.summary ? h("div.f-sum", { html: mdInline(a.summary) }) : null,
    f.length ? null : h("div.f-none", "nothing held up"),
    f.map((x, i) => h(`div.finding.sev-${x.severity}`,
      h("div.f-head", h("span.f-n", String(i + 1).padStart(2, "0")), h("span.f-sev", x.severity), x.category ? h("span.f-cat", x.category) : null, h("span.f-title", { html: mdInline(x.title ?? "") })),
      h("div.f-loc", pathTarget(tab, x.file, x.line, x.line ? `:${x.line}` : "")),
      h("div.f-detail", { html: md(x.detail ?? "") }),
      x.fix ? h("div.f-fix", h("span.dim", "fix "), h("span", { html: md(x.fix) })) : null,
      h("div.f-acts", h("button.mini", { onclick: () => act.quote(tab, `Fix finding ${i + 1}: ${x.title} (${x.file}${x.line ? `:${x.line}` : ""})`) }, "fix this")),
    )),
  );
}

function agentsKind(tb: ToolBlock, a: any): Kind {
  const det = (tb.result ?? tb.partial)?.details;
  const results: any[] = det?.results ?? [];
  const planned: { agent: string; task: string }[] = a.tasks ?? a.chain ?? (a.agent ? [{ agent: a.agent, task: a.task }] : []);
  const n = Math.max(results.length, planned.length);
  const running = tb.status === "running" || tb.status === "queued";
  return {
    verb: "agents", cls: "t-agents", auto: true,
    target: h("span", `${n || "?"} ${det?.mode ?? (a.chain ? "chain" : a.tasks ? "parallel" : "single")}`, h("span.dim", ` · ${planned.map((p) => p.agent).join(", ")}`)),
    body: () => h("div.agent-cards", (results.length ? results : planned).map((r: any, i: number) => {
      const msgs: any[] = r.messages ?? [];
      let final = "", lastTool = "";
      for (const m of msgs) if (m.role === "assistant") for (const c of m.content ?? []) {
        if (c.type === "text" && c.text) final = c.text;
        if (c.type === "toolCall") lastTool = `${c.name} ${one(c.arguments?.path ?? c.arguments?.command ?? c.arguments?.pattern ?? "", 60)}`;
      }
      const failed = r.exitCode > 0 || r.stopReason === "error" || r.stopReason === "aborted";
      const done = !running || (r.exitCode != null && r.exitCode >= 0 && (final || failed));
      const u = r.usage;
      return h(`div.agent-card${failed ? ".bad" : ""}`,
        h("div.ac-head", done ? h(`span.ac-dot${failed ? ".bad" : ""}`) : h("span.ac-spin", { "data-spin": "" }), h("span.ac-name", r.agent ?? planned[i]?.agent), h("span.ac-task", one(r.task ?? planned[i]?.task ?? "", 100)),
          u ? h("span.ac-meta", `${u.turns ?? 0}t · ${tokens((u.input ?? 0) + (u.output ?? 0))} · ${cost(u.cost ?? 0)}`) : null),
        !done && lastTool ? h("div.ac-now", h("span.dim", "› "), lastTool) : null,
        failed && r.errorMessage ? h("div.ac-err", one(r.errorMessage, 300)) : null,
        final ? h("details.ac-out", h("summary", "output"), h("div.prose", { html: md(final) })) : null,
      );
    })),
  };
}

export function toolNode(tb: ToolBlock, tab: Tab): HTMLElement {
  const k = describe(tb, tab);
  const isOpen = opened.has(tb.id) || (k.auto && !closed.has(tb.id));
  const st = tb.status;
  const glyph = st === "running" || st === "streaming" || st === "queued" ? h("span.tg.run", { "data-spin": "" }) : h(`span.tg.${st === "error" || tb.result?.isError ? "err" : "ok"}`);
  const time = tb.ms != null && tb.ms > 400 ? dur(tb.ms) : st === "running" && tb.started ? h("span", { "data-since": String(tb.started) }) : null;
  const row = h(`div.trow.${k.cls}${k.body ? ".can" : ""}${isOpen ? ".open" : ""}`,
    { onclick: () => {
      if (!k.body) return;
      if (isOpen) { opened.delete(tb.id); closed.add(tb.id); } else { opened.add(tb.id); closed.delete(tb.id); }
      tb.v++;
      tab.emit("items");
    } },
    glyph, h("span.tverb", k.verb), h("span.ttarget", k.target), h("span.tmeta", k.meta ?? null, time ? h("span.ttime", time) : null),
  );
  const box = h(`div.tool${tb.result?.isError ? ".err" : ""}`, row);
  if (isOpen && k.body) { const b = k.body(); if (b) box.append(h("div.tbody", b)); }
  return box;
}
