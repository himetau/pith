// Tiny element builder. h("div.a.b", { onclick }, child, "text")
type Attrs = Record<string, any> | null | undefined;
export type Child = Node | string | number | false | null | undefined | Child[];

export function h<K extends keyof HTMLElementTagNameMap>(tag: K | `${K}.${string}` | `${K}#${string}`, attrs?: Attrs | Child, ...kids: Child[]): HTMLElementTagNameMap[K];
export function h(tag: string, attrs?: Attrs | Child, ...kids: Child[]): HTMLElement;
export function h(tag: string, attrs?: Attrs | Child, ...kids: Child[]): HTMLElement {
  // the second argument is attributes only when it's a plain object, otherwise it's the first child
  if (attrs != null && (typeof attrs !== "object" || attrs instanceof Node || Array.isArray(attrs))) { kids.unshift(attrs as Child); attrs = null; }
  const [name, ...rest] = tag.split(/(?=[.#])/);
  const el = document.createElement(name || "div");
  for (const r of rest) {
    if (r[0] === ".") el.classList.add(r.slice(1));
    else el.id = r.slice(1);
  }
  if (attrs) for (const [k, v] of Object.entries(attrs as Record<string, any>)) {
    if (v == null || v === false) continue;
    if (k.startsWith("on") && typeof v === "function") el.addEventListener(k.slice(2), v);
    else if (k === "class") el.className += (el.className ? " " : "") + v;
    else if (k === "style" && typeof v === "object") Object.assign(el.style, v);
    else if (k === "html") el.innerHTML = v;
    else if (k in el && k !== "list" && typeof v !== "string") (el as any)[k] = v;
    else el.setAttribute(k, v === true ? "" : String(v));
  }
  add(el, kids);
  return el;
}

export function add(el: Node, kids: Child[]) {
  for (const k of kids.flat(Infinity as 1) as Child[]) {
    if (k == null || k === false) continue;
    el.appendChild(k instanceof Node ? k : document.createTextNode(String(k)));
  }
}

export function swap(el: Element, ...kids: Child[]) {
  el.replaceChildren();
  add(el, kids);
}

export const svgNS = "http://www.w3.org/2000/svg";
export function s<K extends keyof SVGElementTagNameMap>(tag: K, attrs: Record<string, string | number> = {}, ...kids: (SVGElement | string)[]): SVGElementTagNameMap[K] {
  const el = document.createElementNS(svgNS, tag);
  for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, String(v));
  for (const k of kids) el.append(k);
  return el;
}

// one listener per key, for things that re-render and re-subscribe
export function onKey(el: HTMLElement, map: Record<string, (e: KeyboardEvent) => void>) {
  el.addEventListener("keydown", (e) => {
    const k = [e.ctrlKey && "ctrl", e.altKey && "alt", e.shiftKey && "shift", e.key.toLowerCase()].filter(Boolean).join("+");
    const fn = map[k];
    if (fn) { e.preventDefault(); fn(e); }
  });
}

export function debounce<A extends unknown[]>(fn: (...a: A) => void, ms: number) {
  let t: ReturnType<typeof setTimeout> | null = null;
  return (...a: A) => { if (t) clearTimeout(t); t = setTimeout(() => fn(...a), ms); };
}

export function autosize(ta: HTMLTextAreaElement, max = 320) {
  ta.style.height = "auto";
  ta.style.height = `${Math.min(ta.scrollHeight, max)}px`;
}
