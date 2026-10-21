// Image library and a quick generator that doesn't need a session
import { act } from "../actions";
import { app } from "../app";
import { api } from "../lib/api";
import { autosize, h, swap } from "../lib/dom";
import { ago, tilde } from "../lib/fmt";
import { icon } from "../lib/icons";

type Ref = { data: string; mimeType: string };

export class ImagesPage {
  el = h("div.page.images");
  private grid = h("div.im-grid");
  private refs: Ref[] = [];
  private refsEl = h("div.im-refs");
  private busy = 0;
  private ta = h("textarea.s-in.im-prompt", { rows: 2, placeholder: "describe the image: subject, style, composition, colours, any text in it" }) as HTMLTextAreaElement;

  constructor() {
    app.on("view", () => { if (app.view === "images") void this.load(); });
    this.ta.addEventListener("input", () => autosize(this.ta, 200));
    this.ta.addEventListener("keydown", (e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); void this.gen(); } });
    this.ta.addEventListener("paste", (e) => {
      for (const it of [...(e.clipboardData?.items ?? [])]) if (it.type.startsWith("image/")) { const f = it.getAsFile(); if (f) void readImg(f).then((r) => { this.refs.push(r); this.drawRefs(); }); }
    });
    this.el.append(
      h("div.page-head", h("h1", "images"), h("span.grow"), h("button.btn.sm.ghost", { onclick: () => void api.openPath(app.sys.imagesDir) }, icon("folder"), "folder")),
      h("div.im-gen",
        h("div.s-legend", "generate"),
        this.ta,
        this.refsEl,
        h("div.im-bar",
          h("button.btn.sm.ghost", { onclick: () => void this.addRef() }, icon("clip"), "reference image"),
          h("span.dim.im-model"),
          h("span.grow"),
          h("button.btn.pri", { onclick: () => void this.gen() }, icon("sparkle"), "generate")),
      ),
      this.grid,
    );
  }

  private drawRefs() {
    swap(this.refsEl, this.refs.map((r, i) => h("div.chip.img", h("img", { src: `data:${r.mimeType};base64,${r.data}` }), h("button", { onclick: () => { this.refs.splice(i, 1); this.drawRefs(); } }, icon("close")))));
  }

  private async addRef() {
    for (const f of await api.pickFiles()) {
      const r = await api.read(f);
      if (r.kind === "image") this.refs.push({ data: r.data, mimeType: r.mimeType });
    }
    this.drawRefs();
  }

  async load() {
    swap(this.el.querySelector(".im-model")!, `${app.settings.imageProvider} / ${app.settings.imageModel}`);
    const list = await api.images();
    const pending = Array.from({ length: this.busy }, () => h("div.im-card.wait", h("div.im-ph", h("span", { "data-spin": "" }), " painting")));
    swap(this.grid, pending, list.map((x) => h("div.im-card",
      h("img", { src: `file://${x.file}`, loading: "lazy", onclick: () => act.lightbox(`file://${x.file}`, x.prompt.split("\n")[0]) }),
      h("div.im-meta",
        h("div.im-p", x.prompt.split("\n")[0] || tilde(x.file)),
        h("div.im-acts", h("span.dim", ago(x.mtime)), h("span.grow"),
          h("button.ia", { title: "use in the current chat", onclick: () => void this.useInChat(x.file) }, icon("clip")),
          h("button.ia", { title: "use as reference", onclick: async () => { const r = await api.read(x.file); if (r.kind === "image") { this.refs.push({ data: r.data, mimeType: r.mimeType }); this.drawRefs(); this.ta.focus(); } } }, icon("edit")),
          h("button.ia", { title: "show in folder", onclick: () => void api.reveal(x.file) }, icon("folder")))),
    )));
    if (!list.length && !this.busy) this.grid.append(h("div.dim.im-none", "nothing generated yet. images the agent makes with generate_image land here too."));
  }

  private async useInChat(file: string) {
    const t = app.active;
    if (!t) return app.toast("open a session first", "warning");
    const r = await api.read(file);
    if (r.kind !== "image") return;
    act.attach(t, { data: r.data, mimeType: r.mimeType });
    app.focus(t);
  }

  private async gen() {
    const prompt = this.ta.value.trim();
    if (!prompt) return this.ta.focus();
    const refs = this.refs;
    this.refs = [];
    this.drawRefs();
    this.ta.value = "";
    autosize(this.ta, 200);
    this.busy++;
    void this.load();
    try {
      await api.generateImage(prompt, refs);
    } catch (e) {
      app.toast(String((e as Error).message ?? e).replace(/^Error invoking remote method '[^']+': (Error: )?/, ""), "error");
      this.ta.value = prompt;
    } finally {
      this.busy--;
      void this.load();
    }
  }
}

function readImg(f: File): Promise<Ref> {
  return new Promise((resolve) => {
    const r = new FileReader();
    r.onload = () => { const s = String(r.result); resolve({ mimeType: f.type || "image/png", data: s.slice(s.indexOf(",") + 1) }); };
    r.readAsDataURL(f);
  });
}
