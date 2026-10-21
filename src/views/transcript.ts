// The scrollback. Items re-render only when their version moved, text blocks only when their text did
import { act } from "../actions";
import { api } from "../lib/api";
import { h } from "../lib/dom";
import { cost, dur, tokens } from "../lib/fmt";
import { icon } from "../lib/icons";
import { md } from "../lib/md";
import type { Block, Item, Tab } from "../tab";
import { toolNode } from "./tools";

const thinkingOpen = new Set<string>();

type Rendered = { v: number; el: HTMLElement; blocks?: { v: number; el: HTMLElement }[] };

export class Transcript {
  el = h("div.tx");
  private nodes = new Map<number, Rendered>();
  constructor(private tab: Tab, private onEdit: (text: string, entryId: string) => void) {
    this.el.addEventListener("click", (e) => this.click(e));
  }

  update() {
    const seen = new Set<number>();
    let prev: Element | null = null;
    for (const it of this.tab.items) {
      seen.add(it.id);
      let r = this.nodes.get(it.id);
      if (!r) {
        r = { v: -1, el: h("div") };
        this.nodes.set(it.id, r);
      }
      if (r.v !== it.v) this.render(it, r);
      const want: Element | null = prev ? prev.nextElementSibling : this.el.firstElementChild;
      if (want !== r.el) this.el.insertBefore(r.el, want);
      prev = r.el;
    }
    for (const [id, r] of this.nodes) if (!seen.has(id)) { r.el.remove(); this.nodes.delete(id); }
  }

  private render(it: Item, r: Rendered) {
    r.v = it.v;
    switch (it.k) {
      case "user": {
        const el = h("div.msg.user",
          h("span.prompt-glyph", "❯"),
          h("div.ubody",
            it.images.length ? h("div.uimgs", it.images.map((im) => { const src = `data:${im.mimeType};base64,${im.data}`; return h("img", { src, onclick: () => act.lightbox(src) }); })) : null,
            it.typed ? h("div.utext", h("span.u-cmd", it.typed)) : h("div.utext", it.text),
            it.typed ? h("details.u-exp", h("summary", "expanded prompt"), h("div.utext.dim", it.text)) : null,
          ),
          h("div.hover-acts",
            h("button.ia", { title: "copy", onclick: () => void api.copy(it.text) }, icon("copy")),
            it.entryId ? h("button.ia", { title: "edit and branch from here", onclick: () => this.onEdit(it.text, it.entryId!) }, icon("branch")) : null,
          ),
        );
        r.el.replaceWith(el);
        r.el = el;
        break;
      }
      case "assistant":
        this.renderAssistant(it, r);
        break;
      case "bash": {
        const el = h(`div.msg.bash${it.exit && it.exit !== 0 ? ".bad" : ""}`,
          h("div.bash-cmd", h("span.prompt-glyph.shell", "$"), h("span", it.command), it.running ? h("span.dim", { "data-spin": "" }) : it.exit != null ? h("span.bash-exit", it.cancelled ? "cancelled" : `exit ${it.exit}`) : null, it.excluded ? h("span.dim", " · not sent to the model") : null),
          h("pre.tool-out", it.output || (it.running ? "" : "(no output)")),
        );
        r.el.replaceWith(el);
        r.el = el;
        break;
      }
      case "compaction": {
        const el = h("details.marker.compact", h("summary", h("span.rule"), h("span", `compacted${it.tokensBefore ? ` · ${tokens(it.tokensBefore)} tokens summarised` : ""}`), h("span.rule")), h("div.prose.dim", { html: md(it.summary) }));
        r.el.replaceWith(el);
        r.el = el;
        break;
      }
      case "branch": {
        const el = h("details.marker", h("summary", h("span.rule"), h("span", "came back from another branch"), h("span.rule")), h("div.prose.dim", { html: md(it.summary) }));
        r.el.replaceWith(el);
        r.el = el;
        break;
      }
      case "custom": {
        const el = h("div.msg.custom", h("span.dim", it.type), h("div.prose", { html: md(it.text) }));
        r.el.replaceWith(el);
        r.el = el;
        break;
      }
      case "note": {
        const el = h(`div.note.${it.level}`, it.text);
        r.el.replaceWith(el);
        r.el = el;
        break;
      }
    }
  }

  private renderAssistant(it: Extract<Item, { k: "assistant" }>, r: Rendered) {
    if (!r.blocks || !r.el.classList.contains("assistant")) {
      const el = h("div.msg.assistant", h("div.blocks"));
      r.el.replaceWith(el);
      r.el = el;
      r.blocks = [];
    }
    const box = r.el.querySelector(".blocks")!;
    const blocks = it.blocks.filter((b): b is Block => !!b && !(b.t === "text" && !b.text.trim() && !it.live));
    blocks.forEach((b, i) => {
      let rb = r.blocks![i];
      if (rb && rb.v === b.v && rb.el.dataset.kind === b.t) return;
      const el = this.block(b, it.live && i === blocks.length - 1);
      el.dataset.kind = b.t;
      if (rb) { rb.el.replaceWith(el); rb.el = el; rb.v = b.v; }
      else { box.append(el); r.blocks!.push({ v: b.v, el }); }
    });
    while (r.blocks!.length > blocks.length) r.blocks!.pop()!.el.remove();

    r.el.querySelector(":scope > .aerr")?.remove();
    r.el.querySelector(":scope > .turn-foot")?.remove();
    if (it.error && it.stop !== "aborted") r.el.append(h("div.aerr", it.error));
    // live runs get their duration, ones loaded from disk only what the message itself recorded
    if (it.runEnd || (!it.live && it.stop === "stop" && this.isLastOfTurn(it))) {
      const u = it.usage;
      const text = blocks.filter((b) => b.t === "text").map((b) => (b as { text: string }).text).join("\n\n");
      r.el.append(h("div.turn-foot",
        h("span.rule"),
        h("span.tf-meta", [it.model, it.runEnd ? dur(it.runEnd.ms) : null, u ? `${tokens(u.input + u.cacheRead)}↑ ${tokens(u.output)}↓` : null, u?.cost?.total ? cost(u.cost.total) : null].filter(Boolean).join("  ·  ")),
        text ? h("button.ia", { title: "copy reply", onclick: () => void api.copy(text) }, icon("copy")) : null,
        h("button.ia", { title: "retry: branch from the prompt before this and send it again", onclick: () => void this.retry(it) }, icon("refresh")),
        h("span.rule"),
      ));
    }
  }

  // same prompt, fresh answer: branch from the user message that started this turn and resend it
  private async retry(it: Item) {
    if (this.tab.running) return;
    const i = this.tab.items.indexOf(it);
    let u: Extract<Item, { k: "user" }> | undefined;
    for (let j = i; j >= 0; j--) { const x = this.tab.items[j]; if (x.k === "user") { u = x; break; } }
    if (!u?.entryId) return;
    const text = u.typed ?? u.text, images = u.images;
    if ((await this.tab.fork(u.entryId)) == null) return;
    await this.tab.send(text, images);
  }

  private isLastOfTurn(it: Item) {
    const i = this.tab.items.indexOf(it);
    for (let j = i + 1; j < this.tab.items.length; j++) {
      const n = this.tab.items[j];
      if (n.k === "user") return true;
      if (n.k === "assistant") return false;
    }
    return !this.tab.running;
  }

  private block(b: Block, last: boolean): HTMLElement {
    switch (b.t) {
      case "text":
        return h("div.prose", { html: md(b.text) + (last ? '<span class="caret"></span>' : "") });
      case "thinking": {
        const key = `${b.started ?? ""}${b.text.slice(0, 40)}`;
        const live = last && !b.ms;
        const open = thinkingOpen.has(key);
        const label = live ? "thinking" : b.redacted && !b.text ? "thought (hidden by the provider)" : b.ms ? `thought for ${dur(b.ms)}` : `thought · ${tokens(Math.round(b.text.length / 4))} tokens`;
        const tail = live ? b.text.split("\n").filter(Boolean).slice(-3).join("\n") : "";
        return h(`div.think${open ? ".open" : ""}${live ? ".live" : ""}`,
          h("div.think-head", { onclick: (e: Event) => { const el = (e.currentTarget as HTMLElement).parentElement!; if (open) thinkingOpen.delete(key); else thinkingOpen.add(key); el.classList.toggle("open"); el.querySelector(".think-body")?.toggleAttribute("hidden"); } },
            live ? h("span.tg.run", { "data-spin": "" }) : h("span.think-g", "∴"), h("span", label)),
          live && !open ? h("div.think-tail", tail) : null,
          h("div.think-body.prose", { hidden: !open, html: md(b.text) }),
        );
      }
      case "tool":
        return toolNode(b, this.tab);
    }
  }

  private click(e: Event) {
    const t = e.target as HTMLElement;
    const btn = t.closest<HTMLElement>("[data-act]");
    if (btn) {
      const code = btn.closest(".code")?.querySelector("code")?.textContent ?? "";
      if (btn.dataset.act === "copy") { void api.copy(code); btn.textContent = "copied"; setTimeout(() => (btn.textContent = "copy"), 1200); }
      if (btn.dataset.act === "preview") act.previewHtml(this.tab, code);
      return;
    }
    const p = t.closest<HTMLElement>("code.path");
    if (p?.dataset.path) {
      const m = p.dataset.path.match(/^(.*?)(?::(\d+))?(?:-\d+)?$/)!;
      act.openFile(this.tab, m[1], m[2] ? Number(m[2]) : undefined);
    }
  }
}
