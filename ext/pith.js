// Loaded into every pi that pith starts (-e): the tools only the app can do, each one a call back into pith over its
// loopback bridge. Approvals, questions, todos and plan mode are pi's own now, pith answers those over rpc.
// Plain json schema and no imports, so pi's js runtime has nothing to resolve. Without PITH_BRIDGE set it does nothing
// PITH_BRIDGE is http://127.0.0.1:<port>/<token>, the token in the path since pi hides secret-looking env vars
const BRIDGE = process.env.PITH_BRIDGE, TAB = process.env.PITH_TAB ?? "";
// design sessions never generate images, the design has to carry itself, so they don't get the tool at all
const DESIGN = process.env.PITH_KIND === "design";

async function call(op, args = {}) {
  const res = await fetch(`${BRIDGE}/call`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ tab: TAB, op, args }) });
  const j = await res.json().catch(() => ({ error: `bridge said ${res.status}` }));
  if (!res.ok || j.error) throw new Error(j.error ?? `bridge said ${res.status}`);
  return j.result;
}

const text = (t) => ({ type: "text", text: t });
const str = (description) => ({ type: "string", ...(description ? { description } : {}) });
const obj = (properties, required = []) => ({ type: "object", properties, required, additionalProperties: false });

export default function (pi) {
  if (!BRIDGE) return;

  pi.registerTool({
    name: "browser",
    label: "Browser",
    description: "Drive the browser pane the user sees next to the chat. snapshot lists interactive elements as [eN] refs plus the page text; click/type take a ref (or a CSS selector, or x/y). Use screenshot when layout matters. Always snapshot again after the page changes before using refs.",
    parameters: obj({
      action: { type: "string", enum: ["navigate", "snapshot", "screenshot", "click", "type", "key", "scroll", "back", "forward", "reload", "eval", "wait"] },
      url: str("for navigate"),
      ref: str("element ref from the last snapshot, like e12"),
      selector: str("CSS selector, when there's no ref"),
      text: str("text to type"),
      submit: { type: "boolean", description: "press enter after typing" },
      clear: { type: "boolean", description: "clear the field before typing" },
      key: str("key to press, like Enter, Escape, ctrl+a"),
      x: { type: "number" },
      y: { type: "number" },
      dy: { type: "number", description: "pixels to scroll, negative is up" },
      script: str("javascript to evaluate in the page, for eval"),
      ms: { type: "number", description: "for wait" },
    }, ["action"]),
    async execute(_id, params) {
      const r = await call("browser", params);
      return { content: r.image ? [text(r.text), { type: "image", ...r.image }] : [text(r.text)], details: { url: (r.text.match(/^(?:url:|now at) (\S+)/m) ?? [])[1] } };
    },
  });

  pi.registerTool({
    name: "preview",
    label: "Preview",
    description: "Open an HTML file (or a URL, like a dev server) in the preview pane next to the chat. It reloads by itself when the file changes, so call this once per design, not after every edit. The user can click elements there to comment on them.",
    parameters: obj({ path: str("HTML file to show"), url: str("URL to show instead of a file"), title: str() }),
    async execute(_id, params, _signal, _onUpdate, ctx) {
      const r = await call("preview", { ...params, cwd: ctx?.cwd });
      return { content: [text(`showing ${r.shown} in the preview pane`)], details: { path: r.path, url: params.url } };
    },
  });

  pi.registerTool({
    name: "report_findings",
    label: "Findings",
    description: "Report code review findings so the app can show them as cards linked to the diff. Only include findings you verified. Rank most severe first. Call it once at the end of a review, with an empty list if nothing held up.",
    parameters: obj({
      summary: str("one or two sentences on the overall state"),
      findings: { type: "array", items: obj({
        file: str("repo-relative path"),
        line: { type: "number" },
        severity: { type: "string", enum: ["high", "medium", "low"] },
        category: str("short kebab-case slug, like correctness or security"),
        title: str("the defect in under 60 characters"),
        detail: str("what goes wrong, with the concrete input or state that triggers it"),
        fix: str("the suggested change"),
      }, ["file", "severity", "title", "detail"]) },
    }, ["findings"]),
    async execute(_id, params) {
      const n = params.findings?.length ?? 0;
      return { content: [text(`reported ${n} finding${n === 1 ? "" : "s"}`)] };
    },
  });

  if (!DESIGN) pi.registerTool({
    name: "generate_image",
    label: "Image",
    description: "Generate an image from a prompt, or edit/combine existing images by passing their paths. Saves to disk and shows it in the chat. Write a specific prompt: subject, style, composition, colours, any text that has to appear.",
    parameters: obj({
      prompt: str(),
      images: { type: "array", items: { type: "string" }, description: "paths of input images to edit or use as reference" },
      aspect: str("like 1:1, 16:9, 9:16"),
      save_to: str("folder to save into, defaults to pith's image library"),
    }, ["prompt"]),
    async execute(_id, params, _signal, _onUpdate, ctx) {
      const r = await call("generate-image", { ...params, cwd: ctx?.cwd });
      return {
        content: [text(`saved ${r.files.join(", ")}${r.text ? `\n${r.text}` : ""}`), ...r.images],
        details: { files: r.files, model: r.model },
      };
    },
  });
}
