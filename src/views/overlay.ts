// Things that float: context menus, the picker (palette, model, effort, mode), text prompts, lightbox, toasts
import { app } from "../app";
import { api } from "../lib/api";
import { h, swap } from "../lib/dom";
import { icon } from "../lib/icons";
import { fuzzy } from "./composer";

const layer = () => document.getElementById("overlay")!;

export type MenuItem = { label: string; hint?: string; danger?: boolean; run: () => void } | false | null | undefined | "";

export function menu(x: number, y: number, items: MenuItem[]) {
  const close = () => { el.remove(); removeEventListener("mousedown", away, true); };
  const away = (e: MouseEvent) => { if (!el.contains(e.target as Node)) close(); };
  const el = h("div.ctx-menu", items.filter(Boolean).map((it) => {
    const i = it as Exclude<MenuItem, false | null | undefined | "">;
    return h(`button${i.danger ? ".danger" : ""}`, { onclick: () => { close(); i.run(); } }, h("span", i.label), i.hint ? h("span.dim", i.hint) : null);
  }));
  layer().append(el);
  const r = el.getBoundingClientRect();
  el.style.left = `${Math.min(x, innerWidth - r.width - 8)}px`;
  el.style.top = `${Math.min(y, innerHeight - r.height - 8)}px`;
  setTimeout(() => addEventListener("mousedown", away, true));
}

export function prompt(title: string, value = "", opts: { multiline?: boolean; placeholder?: string } = {}): Promise<string | null> {
  return new Promise((resolve) => {
    const field = (opts.multiline ? h("textarea.dlg-field", { rows: 8 }) : h("input.dlg-field")) as HTMLInputElement;
    field.value = value;
    if (opts.placeholder) field.placeholder = opts.placeholder;
    const done = (v: string | null) => { scrim.remove(); resolve(v); };
    const scrim = h("div.scrim", { onmousedown: (e: Event) => { if (e.target === scrim) done(null); } },
      h("div.dlg", h("div.dlg-legend", title), field, h("div.dlg-foot", h("button.btn.ghost", { onclick: () => done(null) }, "cancel"), h("button.btn.pri", { onclick: () => done(field.value.trim()) }, "ok"))));
    field.addEventListener("keydown", (e) => {
      if (e.key === "Escape") done(null);
      if (e.key === "Enter" && (!opts.multiline || (e as KeyboardEvent).ctrlKey)) { e.preventDefault(); done(field.value.trim()); }
    });
    layer().append(scrim);
    field.focus();
    field.select();
  });
}

export type PickItem = { label: string; hint?: string; group?: string; right?: string; on?: boolean; keys?: string; run: () => void };

// one picker for everything: palette, models, effort, mode
export function picker(o: { title: string; items: PickItem[]; placeholder?: string; initial?: string }) {
  const input = h("input.pk-in", { placeholder: o.placeholder ?? "type to filter", spellcheck: false }) as HTMLInputElement;
  const list = h("div.pk-list");
  let sel = 0, shown: PickItem[] = [];
  const close = () => scrim.remove();
  const draw = () => {
    const q = input.value.trim();
    shown = q ? o.items.map((it) => ({ it, s: Math.max(fuzzy(q, it.label), fuzzy(q, `${it.group ?? ""} ${it.hint ?? ""}`) * 0.5) })).filter((x) => x.s > 0).sort((a, b) => b.s - a.s).map((x) => x.it) : o.items;
    sel = Math.min(sel, Math.max(0, shown.length - 1));
    let lastGroup = "";
    const els: HTMLElement[] = [];
    shown.forEach((it, i) => {
      if (!q && it.group && it.group !== lastGroup) { els.push(h("div.pk-grp", it.group)); lastGroup = it.group; }
      els.push(h(`div.pk-row${i === sel ? ".sel" : ""}${it.on ? ".on" : ""}`, { onmousedown: (e: Event) => { e.preventDefault(); close(); it.run(); }, onmousemove: () => { if (sel !== i) { sel = i; draw(); } } },
        h("span.pk-mark", it.on ? "●" : ""), h("span.pk-l", it.label), it.hint ? h("span.pk-h", it.hint) : null, it.right ? h("span.pk-r", it.right) : null, it.keys ? h("kbd", it.keys) : null));
    });
    if (!shown.length) els.push(h("div.pk-none", "nothing matches"));
    swap(list, els);
    list.querySelector(".sel")?.scrollIntoView({ block: "nearest" });
  };
  input.addEventListener("input", () => { sel = 0; draw(); });
  input.addEventListener("keydown", (e) => {
    if (e.key === "Escape") { e.preventDefault(); close(); }
    else if (e.key === "ArrowDown" || (e.key === "n" && e.ctrlKey)) { e.preventDefault(); sel = Math.min(sel + 1, shown.length - 1); draw(); }
    else if (e.key === "ArrowUp" || (e.key === "p" && e.ctrlKey)) { e.preventDefault(); sel = Math.max(sel - 1, 0); draw(); }
    else if (e.key === "Enter") { e.preventDefault(); const it = shown[sel]; if (it) { close(); it.run(); } }
  });
  const scrim = h("div.scrim.top", { onmousedown: (e: Event) => { if (e.target === scrim) close(); } }, h("div.picker", h("div.pk-legend", o.title), h("div.pk-top", icon("search"), input), list));
  layer().append(scrim);
  if (o.initial) { const i = o.items.findIndex((x) => x.label === o.initial || x.on); if (i >= 0) sel = i; }
  else { const i = o.items.findIndex((x) => x.on); if (i >= 0) sel = i; }
  draw();
  input.focus();
}

export function lightbox(src: string, caption?: string) {
  const close = () => { scrim.remove(); removeEventListener("keydown", key); };
  const key = (e: KeyboardEvent) => { if (e.key === "Escape") close(); };
  const data = src.startsWith("data:") ? src.slice(src.indexOf(",") + 1) : "";
  const scrim = h("div.scrim.lb", { onclick: (e: Event) => { if (e.target === scrim || (e.target as HTMLElement).tagName === "IMG") close(); } },
    h("img", { src }),
    h("div.lb-bar", caption ? h("span.lb-cap", caption) : null, data ? h("button.btn.ghost", { onclick: () => { void api.copyImage(data); app.toast("image copied"); } }, icon("copy"), "copy") : null, h("button.btn.ghost", { onclick: close }, icon("close"), "close")));
  layer().append(scrim);
  addEventListener("keydown", key);
}

export class Toasts {
  el = h("div#toasts");
  constructor() {
    app.on("toast", () => swap(this.el, app.toasts.map((t) => h(`div.toast.${t.level}`, h("span.tk", t.level === "error" ? "!" : t.level === "warning" ? "~" : "›"), h("span", t.text)))));
  }
}
