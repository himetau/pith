// Drives the browser pane's <webview> for the agent's `browser` tool. Each session has its own
// webview, the renderer tells us its webContents id once it's attached
import { webContents, type WebContents } from "electron";

const attached = new Map<string, number>();
const waiters = new Map<string, ((id: number) => void)[]>();

export function attachBrowser(tab: string, wcId: number) {
  attached.set(tab, wcId);
  for (const w of waiters.get(tab) ?? []) w(wcId);
  waiters.delete(tab);
}

export function detachBrowser(tab: string) { attached.delete(tab); }

// pane might not be open yet, ask for it and wait for the webview to show up
async function contents(tab: string, open: () => void): Promise<WebContents> {
  let id = attached.get(tab);
  let wc = id != null ? webContents.fromId(id) : undefined;
  if (wc && !wc.isDestroyed()) return wc;
  open();
  id = await new Promise<number>((resolve, reject) => {
    const t = setTimeout(() => reject(new Error("the browser pane didn't open")), 15000);
    waiters.set(tab, [...(waiters.get(tab) ?? []), (i) => { clearTimeout(t); resolve(i); }]);
  });
  wc = webContents.fromId(id);
  if (!wc) throw new Error("the browser pane went away");
  return wc;
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function settle(wc: WebContents, ms = 8000) {
  const end = Date.now() + ms;
  await sleep(150);
  while (wc.isLoading() && Date.now() < end) await sleep(100);
  await sleep(120);
}

// numbers every visible interactive element as data-pith-ref so later calls can point at them
const SNAPSHOT = `(() => {
  const sel = 'a[href],button,input,select,textarea,summary,[role=button],[role=link],[role=tab],[role=menuitem],[role=checkbox],[role=option],[contenteditable=""],[contenteditable=true],[onclick],[tabindex]:not([tabindex="-1"])';
  let n = 0; const lines = [];
  for (const el of document.querySelectorAll('[data-pith-ref]')) el.removeAttribute('data-pith-ref');
  for (const el of document.querySelectorAll(sel)) {
    const r = el.getBoundingClientRect(), cs = getComputedStyle(el);
    if (r.width < 2 || r.height < 2 || cs.visibility === 'hidden' || cs.display === 'none') continue;
    if (lines.length >= 250) break;
    const ref = 'e' + (++n); el.setAttribute('data-pith-ref', ref);
    const tag = el.tagName.toLowerCase(), role = el.getAttribute('role') || (tag === 'a' ? 'link' : tag);
    let name = (el.getAttribute('aria-label') || el.innerText || el.value || el.placeholder || el.title || el.alt || '').trim().replace(/\\s+/g, ' ').slice(0, 80);
    let extra = '';
    if (tag === 'input') extra = ' type=' + (el.type || 'text') + (el.value ? ' value="' + String(el.value).slice(0, 40) + '"' : '');
    if (tag === 'a') extra = ' -> ' + (el.getAttribute('href') || '').slice(0, 80);
    const off = r.bottom < 0 || r.top > innerHeight ? ' (offscreen)' : '';
    lines.push('[' + ref + '] ' + role + (name ? ' "' + name + '"' : '') + extra + off);
  }
  const heads = [...document.querySelectorAll('h1,h2,h3,[role=heading]')].filter((e) => e.getBoundingClientRect().height > 0).slice(0, 30).map((e) => (e.tagName[0] === 'H' ? e.tagName.toLowerCase() : 'heading') + ' ' + e.innerText.trim().replace(/\\s+/g, ' ').slice(0, 100));
  const text = (document.body?.innerText || '').replace(/\\n{3,}/g, '\\n\\n').slice(0, 7000);
  return 'url: ' + location.href + '\\ntitle: ' + document.title + '\\nscroll: ' + Math.round(scrollY) + '/' + Math.max(0, document.documentElement.scrollHeight - innerHeight) +
    (heads.length ? '\\n\\n# headings\\n' + heads.join('\\n') : '') + '\\n\\n# interactive\\n' + lines.join('\\n') + '\\n\\n# text\\n' + text;
})()`;

const target = (a: Args) => a.ref ? `document.querySelector('[data-pith-ref="${String(a.ref).replace(/"/g, "")}"]')` : a.selector ? `document.querySelector(${JSON.stringify(a.selector)})` : "null";

async function centerOf(wc: WebContents, a: Args): Promise<{ x: number; y: number; desc: string }> {
  if (typeof a.x === "number" && typeof a.y === "number") return { x: a.x, y: a.y, desc: `${a.x},${a.y}` };
  const r = await wc.executeJavaScript(`(() => { const el = ${target(a)}; if (!el) return null; el.scrollIntoView({ block: 'center', inline: 'center' });
    const r = el.getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2, desc: el.tagName.toLowerCase() + ' ' + (el.innerText || el.value || el.getAttribute('aria-label') || '').trim().slice(0, 60) }; })()`);
  if (!r) throw new Error(`no element for ${a.ref ? `ref ${a.ref}` : `selector ${a.selector}`}, take a new snapshot`);
  return { x: Math.round(r.x), y: Math.round(r.y), desc: r.desc };
}

type Args = { action: string; url?: string; ref?: string; selector?: string; text?: string; submit?: boolean; clear?: boolean; key?: string; x?: number; y?: number; dy?: number; script?: string; ms?: number; full?: boolean };
export type BrowserResult = { text: string; image?: { data: string; mimeType: string } };

async function shot(wc: WebContents): Promise<BrowserResult["image"]> {
  let img = await wc.capturePage();
  const { width } = img.getSize();
  if (width > 1280) img = img.resize({ width: 1280, quality: "good" });
  return { data: img.toJPEG(80).toString("base64"), mimeType: "image/jpeg" };
}

const keyCode = (k: string) => ({ enter: "Return", return: "Return", esc: "Escape", escape: "Escape", tab: "Tab", backspace: "Backspace", delete: "Delete", up: "Up", down: "Down", left: "Left", right: "Right", space: "Space", pageup: "PageUp", pagedown: "PageDown", home: "Home", end: "End" } as Record<string, string>)[k.toLowerCase()] ?? k;

function press(wc: WebContents, key: string) {
  const parts = key.split("+"), base = keyCode(parts.pop()!);
  const modifiers = parts.map((m) => m.toLowerCase()) as any[];
  wc.sendInputEvent({ type: "keyDown", keyCode: base, modifiers });
  if (base.length === 1 && !modifiers.length) wc.sendInputEvent({ type: "char", keyCode: base });
  wc.sendInputEvent({ type: "keyUp", keyCode: base, modifiers });
}

export async function runBrowser(tab: string, a: Args, open: () => void, onUrl: (url: string) => void): Promise<BrowserResult> {
  const wc = await contents(tab, open);
  const after = async (what: string) => { await settle(wc); onUrl(wc.getURL()); return { text: `${what}\nnow at ${wc.getURL()} (${wc.getTitle()})` }; };
  switch (a.action) {
    case "navigate": {
      if (!a.url) throw new Error("navigate needs a url");
      const url = /^[a-z]+:/i.test(a.url) ? a.url : `https://${a.url}`;
      await wc.loadURL(url).catch(() => { /* loadURL rejects on redirects and aborted subframes, the page still loads */ });
      await settle(wc);
      onUrl(wc.getURL());
      return { text: await wc.executeJavaScript(SNAPSHOT) };
    }
    case "snapshot":
      return { text: await wc.executeJavaScript(SNAPSHOT) };
    case "screenshot":
      return { text: `screenshot of ${wc.getURL()}`, image: await shot(wc) };
    case "click": {
      const c = await centerOf(wc, a);
      await sleep(60);
      wc.sendInputEvent({ type: "mouseMove", x: c.x, y: c.y });
      wc.sendInputEvent({ type: "mouseDown", x: c.x, y: c.y, button: "left", clickCount: 1 });
      wc.sendInputEvent({ type: "mouseUp", x: c.x, y: c.y, button: "left", clickCount: 1 });
      return after(`clicked ${c.desc}`);
    }
    case "type": {
      if (a.ref || a.selector) {
        const ok = await wc.executeJavaScript(`(() => { const el = ${target(a)}; if (!el) return false; el.scrollIntoView({ block: 'center' }); el.focus();
          ${a.clear ? "if ('value' in el) { el.value = ''; el.dispatchEvent(new Event('input', { bubbles: true })); } else if (el.isContentEditable) el.textContent = '';" : ""} return true; })()`);
        if (!ok) throw new Error("no element to type into, take a new snapshot");
      }
      if (a.text) wc.insertText(a.text);
      if (a.submit) { await sleep(50); press(wc, "Enter"); }
      return after(`typed ${JSON.stringify((a.text ?? "").slice(0, 60))}${a.submit ? " and pressed enter" : ""}`);
    }
    case "key":
      press(wc, a.key || "Enter");
      return after(`pressed ${a.key}`);
    case "scroll":
      await wc.executeJavaScript(`scrollBy(0, ${Number(a.dy) || 600})`);
      await sleep(250);
      return { text: await wc.executeJavaScript(SNAPSHOT) };
    case "back":
      wc.navigationHistory.goBack();
      return after("went back");
    case "forward":
      wc.navigationHistory.goForward();
      return after("went forward");
    case "reload":
      wc.reload();
      return after("reloaded");
    case "eval": {
      const v = await wc.executeJavaScript(a.script ?? "undefined", true);
      let s: string;
      try { s = typeof v === "string" ? v : JSON.stringify(v, null, 2); } catch { s = String(v); }
      return { text: (s ?? "undefined").slice(0, 20000) };
    }
    case "wait":
      await sleep(Math.min(Number(a.ms) || 1000, 15000));
      await settle(wc);
      return { text: `waited, at ${wc.getURL()}` };
    default:
      throw new Error(`unknown browser action ${a.action}`);
  }
}
