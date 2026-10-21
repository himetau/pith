// Markdown for model output. Sanitised: the renderer has the whole app api, model text is untrusted
import { Marked } from "marked";
import DOMPurify from "dompurify";
import hljs from "highlight.js/lib/common";

const esc = (s: string) => s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);

export function highlight(code: string, lang?: string): string {
  try {
    if (lang && hljs.getLanguage(lang)) return hljs.highlight(code, { language: lang, ignoreIllegals: true }).value;
    if (code.length < 20000) return hljs.highlightAuto(code).value;
  } catch { /* plain */ }
  return esc(code);
}

export function langOf(path: string) {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  const map: Record<string, string> = { ts: "typescript", tsx: "typescript", mts: "typescript", cts: "typescript", js: "javascript", mjs: "javascript", cjs: "javascript", jsx: "javascript", py: "python", rs: "rust", go: "go", rb: "ruby", sh: "bash", bash: "bash", zsh: "bash", fish: "bash", nu: "bash", md: "markdown", json: "json", jsonc: "json", jsonl: "json", toml: "ini", ini: "ini", conf: "ini", yml: "yaml", yaml: "yaml", css: "css", scss: "scss", html: "xml", xml: "xml", svg: "xml", vue: "xml", c: "c", h: "c", cpp: "cpp", cc: "cpp", hpp: "cpp", java: "java", kt: "kotlin", swift: "swift", lua: "lua", sql: "sql", diff: "diff", rasi: "css", qml: "javascript", zig: "zig" };
  return map[ext];
}

const marked = new Marked({ gfm: true, breaks: false });
marked.use({
  renderer: {
    code({ text, lang }) {
      const l = (lang ?? "").split(/\s/)[0];
      return `<div class="code" data-lang="${esc(l)}"><div class="code-bar"><span>${esc(l || "text")}</span><button class="code-copy" data-act="copy">copy</button>${l === "html" ? '<button data-act="preview">preview</button>' : ""}</div><pre><code class="hljs">${highlight(text, l)}</code></pre></div>`;
    },
    // inline code that looks like a path becomes a link to the file viewer. marked hands over raw text here
    codespan({ text }) {
      if (/^(~|\.{0,2}\/)?[\w@.+-]+(\/[\w@.+-]+)*\.\w{1,8}(:\d+(-\d+)?)?$/.test(text) && !/^\d/.test(text)) return `<code class="path" data-path="${esc(text)}">${esc(text)}</code>`;
      return `<code>${esc(text)}</code>`;
    },
  },
});

DOMPurify.addHook("afterSanitizeAttributes", (node) => {
  if (node.tagName === "A") { node.setAttribute("target", "_blank"); node.setAttribute("rel", "noreferrer"); }
});

export function md(text: string): string {
  const html = marked.parse(text, { async: false }) as string;
  return DOMPurify.sanitize(html, { ADD_ATTR: ["data-act", "data-path", "data-lang", "target"], FORBID_TAGS: ["style", "form", "input", "iframe"] });
}

// one line of markdown (titles, summaries) without the paragraph wrapper
export function mdInline(text: string): string {
  return DOMPurify.sanitize(marked.parseInline(text, { async: false }) as string, { ADD_ATTR: ["data-path"], FORBID_TAGS: ["style", "form", "input", "iframe"] });
}

export { esc };
