// Cards for what an extension asks the user: pith's own permission prompts and ask_user questions,
// plus plain select / confirm / input / editor from any other extension
import { h, mod } from "../lib/dom";
import { tilde } from "../lib/fmt";
import { icon } from "../lib/icons";
import { highlight, langOf } from "../lib/md";
import type { Ask, Tab } from "../tab";
import { editsView } from "./tools";

function parse(title: string, tag: string): any | null {
  if (!title.startsWith(tag)) return null;
  try { return JSON.parse(title.slice(tag.length)); } catch { return null; }
}

// keyboard for the top card: digits pick, enter takes the selection, esc cancels
export type KeyHandler = (e: KeyboardEvent) => boolean;

export function askCard(ask: Ask, tab: Tab): { el: HTMLElement; key: KeyHandler } {
  const perm = ask.method === "select" ? parse(ask.title, "pith:perm") : null;
  if (perm) return permCard(ask, tab, perm);
  const q = ask.method === "input" ? parse(ask.title, "pith:ask") : null;
  if (q) return questionCard(ask, tab, q);
  return genericCard(ask, tab);
}

function preview(tool: string, input: any): HTMLElement | null {
  if (tool === "bash") return h("pre.ask-code", { html: highlight(String(input.command ?? ""), "bash") });
  if (tool === "edit") {
    const edits = Array.isArray(input.edits) ? input.edits : [{ oldText: input.oldText, newText: input.newText }];
    return h("div.ask-diff", editsView(edits));
  }
  if (tool === "write") {
    const c = String(input.content ?? "");
    const ls = c.split("\n");
    return h("pre.ask-code", { html: highlight(ls.slice(0, 40).join("\n") + (ls.length > 40 ? `\n… ${ls.length - 40} more lines` : ""), langOf(input.path ?? "")) });
  }
  if (tool === "generate_image") return h("div.ask-q", input.prompt);
  return h("pre.ask-code", JSON.stringify(input, null, 2));
}

function permCard(ask: Ask, tab: Tab, p: { tool: string; input: any }) {
  const opts = ask.options ?? ["Allow", "Deny"];
  const what = p.tool === "bash" ? "run this command" : p.tool === "edit" ? `edit ${tilde(p.input.path ?? "")}` : p.tool === "write" ? `write ${tilde(p.input.path ?? "")}` : p.tool === "generate_image" ? "generate an image" : `use ${p.tool}`;
  const pickI = (i: number) => tab.answer(ask.id, { value: opts[i] });
  const el = h("div.ask.perm",
    h("div.ask-legend", h("span.ask-tag", "permission"), h("span", `pi wants to ${what}`)),
    preview(p.tool, p.input),
    h("div.ask-opts", opts.map((o, i) => h(`button.ask-opt${i === opts.length - 1 ? ".deny" : i === 0 ? ".yes" : ""}`, { onclick: () => pickI(i) }, h("kbd", String(i + 1)), o))),
  );
  return {
    el,
    key: (e: KeyboardEvent) => {
      const n = Number(e.key);
      if (n >= 1 && n <= opts.length) { pickI(n - 1); return true; }
      if (e.key === "y") { pickI(0); return true; }
      if (e.key === "n" || e.key === "Escape") { pickI(opts.length - 1); return true; }
      return false;
    },
  };
}

function questionCard(ask: Ask, tab: Tab, q: { question: string; options: { label: string; description?: string }[]; multi?: boolean }) {
  const picked = new Set<number>();
  let sel = 0;
  const other = h("input.ask-other", { placeholder: "something else…" }) as HTMLInputElement;
  const list = h("div.ask-list");
  const done = (v: string) => tab.answer(ask.id, { value: v });
  const submit = () => {
    if (other.value.trim()) return done(other.value.trim());
    if (q.multi) { if (picked.size) done([...picked].sort().map((i) => q.options[i].label).join(", ")); return; }
    done(q.options[sel].label);
  };
  const draw = () => {
    list.replaceChildren(...q.options.map((o, i) => h(`button.ask-row${i === sel ? ".sel" : ""}${picked.has(i) ? ".on" : ""}`, {
      onclick: () => { sel = i; if (q.multi) { if (picked.has(i)) picked.delete(i); else picked.add(i); draw(); } else done(o.label); },
    }, h("kbd", String(i + 1)), q.multi ? h("span.cb", picked.has(i) ? icon("check") : "") : null, h("span.ar-l", o.label), o.description ? h("span.ar-d", o.description) : null)));
  };
  draw();
  other.addEventListener("keydown", (e) => { if (e.key === "Enter") { e.preventDefault(); submit(); } e.stopPropagation(); });
  const el = h("div.ask.question",
    h("div.ask-legend", h("span.ask-tag.q", "question"), h("span", q.multi ? "pick any" : "pick one")),
    h("div.ask-q", q.question),
    list,
    h("div.ask-foot", other, q.multi ? h("button.btn.pri", { onclick: submit }, "answer") : null, h("button.btn.ghost", { onclick: () => tab.answer(ask.id, { cancelled: true }) }, "skip")),
  );
  return {
    el,
    key: (e: KeyboardEvent) => {
      const n = Number(e.key);
      if (n >= 1 && n <= q.options.length) { sel = n - 1; if (q.multi) { if (picked.has(sel)) picked.delete(sel); else picked.add(sel); draw(); } else done(q.options[sel].label); return true; }
      if (e.key === "ArrowDown") { sel = (sel + 1) % q.options.length; draw(); return true; }
      if (e.key === "ArrowUp") { sel = (sel - 1 + q.options.length) % q.options.length; draw(); return true; }
      if (e.key === "Enter") { submit(); return true; }
      if (e.key === "Escape") { tab.answer(ask.id, { cancelled: true }); return true; }
      return false;
    },
  };
}

function genericCard(ask: Ask, tab: Tab) {
  const cancel = () => tab.answer(ask.id, { cancelled: true });
  const legend = h("div.ask-legend", h("span.ask-tag", "extension"), h("span", ask.title));
  if (ask.method === "confirm") {
    const el = h("div.ask", legend, ask.message ? h("div.ask-q", ask.message) : null,
      h("div.ask-opts", h("button.ask-opt.yes", { onclick: () => tab.answer(ask.id, { confirmed: true }) }, h("kbd", "y"), "yes"), h("button.ask-opt.deny", { onclick: () => tab.answer(ask.id, { confirmed: false }) }, h("kbd", "n"), "no")));
    return { el, key: (e: KeyboardEvent) => { if (e.key === "y") { tab.answer(ask.id, { confirmed: true }); return true; } if (e.key === "n" || e.key === "Escape") { tab.answer(ask.id, { confirmed: false }); return true; } return false; } };
  }
  if (ask.method === "select") {
    const opts = ask.options ?? [];
    const el = h("div.ask", legend, h("div.ask-list", opts.map((o, i) => h("button.ask-row", { onclick: () => tab.answer(ask.id, { value: o }) }, h("kbd", String(i + 1)), h("span.ar-l", o)))));
    return { el, key: (e: KeyboardEvent) => { const n = Number(e.key); if (n >= 1 && n <= opts.length) { tab.answer(ask.id, { value: opts[n - 1] }); return true; } if (e.key === "Escape") { cancel(); return true; } return false; } };
  }
  const field = ask.method === "editor" ? h("textarea.ask-edit", { rows: 8 }) as HTMLTextAreaElement : h("input.ask-other", { placeholder: ask.placeholder ?? "" }) as HTMLInputElement;
  field.value = ask.prefill ?? "";
  const ok = () => tab.answer(ask.id, { value: field.value });
  field.addEventListener("keydown", (e: Event) => { const k = e as KeyboardEvent; if (k.key === "Enter" && (ask.method === "input" || mod(k))) { e.preventDefault(); ok(); } if (k.key === "Escape") cancel(); e.stopPropagation(); });
  const el = h("div.ask", legend, field, h("div.ask-foot", h("button.btn.pri", { onclick: ok }, "send"), h("button.btn.ghost", { onclick: cancel }, "cancel")));
  setTimeout(() => field.focus(), 30);
  return { el, key: () => false };
}
