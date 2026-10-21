// Preload for the design preview webview. With picking on, hovering outlines an element and clicking
// sends it to the chat as something to comment on
import { ipcRenderer } from "electron";

let on = false, box: HTMLDivElement | null = null, label: HTMLDivElement | null = null, hovered: Element | null = null;

function selectorOf(el: Element): string {
  const parts: string[] = [];
  for (let e: Element | null = el; e && e !== document.body && parts.length < 5; e = e.parentElement) {
    if (e.id) { parts.unshift(`#${CSS.escape(e.id)}`); break; }
    let s = e.tagName.toLowerCase();
    const cls = [...e.classList].filter((c) => !c.startsWith("pith-")).slice(0, 2);
    if (cls.length) s += "." + cls.map((c) => CSS.escape(c)).join(".");
    const sibs = e.parentElement ? [...e.parentElement.children].filter((c) => c.tagName === e!.tagName) : [];
    if (sibs.length > 1) s += `:nth-of-type(${sibs.indexOf(e) + 1})`;
    parts.unshift(s);
  }
  return parts.join(" > ");
}

function paint(el: Element | null) {
  if (!box || !label) return;
  if (!el) { box.style.display = label.style.display = "none"; return; }
  const r = el.getBoundingClientRect();
  Object.assign(box.style, { display: "block", left: `${r.left}px`, top: `${r.top}px`, width: `${r.width}px`, height: `${r.height}px` });
  label.textContent = selectorOf(el);
  Object.assign(label.style, { display: "block", left: `${Math.max(0, r.left)}px`, top: `${r.top > 22 ? r.top - 22 : r.bottom + 2}px` });
}

function ensure() {
  if (box) return;
  box = document.createElement("div");
  box.style.cssText = "position:fixed;z-index:2147483646;pointer-events:none;outline:2px solid #f0c;outline-offset:-1px;background:rgba(255,0,204,.08);display:none";
  label = document.createElement("div");
  label.style.cssText = "position:fixed;z-index:2147483647;pointer-events:none;font:11px/1.6 ui-monospace,monospace;padding:0 6px;background:#f0c;color:#fff;border-radius:3px;display:none;white-space:nowrap;max-width:60vw;overflow:hidden;text-overflow:ellipsis";
  document.documentElement.append(box, label);
}

const move = (e: MouseEvent) => { hovered = e.target as Element; paint(hovered); };
const click = (e: MouseEvent) => {
  e.preventDefault(); e.stopPropagation();
  const el = e.target as Element;
  const html = el.outerHTML.length > 700 ? el.outerHTML.slice(0, 700) + "…" : el.outerHTML;
  ipcRenderer.sendToHost("pick", { selector: selectorOf(el), html, text: (el as HTMLElement).innerText?.slice(0, 200) ?? "" });
};
const key = (e: KeyboardEvent) => { if (e.key === "Escape") { set(false); ipcRenderer.sendToHost("pick-off"); } };

function set(v: boolean) {
  on = v;
  ensure();
  const opts = { capture: true };
  if (on) { addEventListener("mousemove", move, opts); addEventListener("click", click, opts); addEventListener("keydown", key, opts); }
  else { removeEventListener("mousemove", move, opts); removeEventListener("click", click, opts); removeEventListener("keydown", key, opts); paint(null); }
}

ipcRenderer.on("pick", (_e, v: boolean) => set(v));
addEventListener("scroll", () => { if (on) paint(hovered); }, true);
