// The bar under the windows, waybar-ish: what the session is doing on the left, what it costs on the right
import { act, MODES } from "../actions";
import { app } from "../app";
import { api } from "../lib/api";
import { h, swap } from "../lib/dom";
import { cost, tilde, tokens } from "../lib/fmt";
import { onTab } from "../tab";

export class StatusLine {
  el = h("footer.status");
  private branch = new Map<string, string>();

  constructor() {
    app.on("view", () => this.draw());
    app.on("tabs", () => this.draw());
    onTab((t, w) => {
      if (t !== app.active) { if (w === "status") this.draw(); return; }
      if (w === "status" || w === "meta" || w === "stats" || w === "settled") this.draw();
      if (w === "settled") void this.loadBranch(t.cwd);
    });
    queueMicrotask(() => this.draw());
  }

  private async loadBranch(cwd: string) {
    const b = await api.gitBranch(cwd);
    this.branch.set(cwd, b);
    this.draw();
  }

  draw() {
    const t = app.view === "chat" ? app.active : null;
    const live = [...app.tabs.values()].filter((x) => x.status === "idle" || x.status === "running").length;
    const busy = [...app.tabs.values()].filter((x) => x.running).length;
    if (!t) {
      swap(this.el,
        h("span.st-block", app.view),
        h("span.st-seg.dim", `${live} pi running`, busy ? ` · ${busy} working` : ""),
        h("span.grow"),
        h("span.st-seg.dim", `pi ${app.sys.piVersion.replace(/^pi\s*/, "")}`),
      );
      return;
    }
    if (!this.branch.has(t.cwd)) { this.branch.set(t.cwd, ""); void this.loadBranch(t.cwd); }
    const word = t.status === "running" ? (t.compacting ? "compact" : "work") : t.status === "starting" ? "start" : t.status === "dead" ? "dead" : t.status === "cold" ? "cold" : t.asks.length ? "wait" : "idle";
    const st = t.stats;
    const cu = st?.contextUsage;
    const pct = cu?.percent ?? 0;
    const mode = MODES.find((m) => m.id === t.mode)!;
    const br = this.branch.get(t.cwd);
    swap(this.el,
      h(`span.st-block.s-${word}`, t.running ? h("span", { "data-spin": "" }) : null, word),
      h("span.st-seg", { title: t.cwd }, tilde(t.cwd)),
      br ? h("span.st-seg.dim", `⎇ ${br}`) : null,
      [...t.statuses.values()].map((v) => h("span.st-seg.ext", v)),
      h("span.grow"),
      h(`button.st-seg.st-btn.m-${t.mode}`, { title: mode.hint + " (shift+tab)", onclick: () => act.pickMode(t) }, mode.label),
      h("button.st-seg.st-btn", { title: "model (ctrl+m)", onclick: () => act.pickModel(t) }, t.model?.id ?? "model"),
      t.model?.reasoning ? h("button.st-seg.st-btn", { title: "effort (ctrl+e)", onclick: () => act.pickEffort(t) }, t.state.thinkingLevel ?? "effort") : null,
      cu?.contextWindow ? h("span.st-seg.st-ctx", { title: `context ${cu.tokens == null ? "?" : tokens(cu.tokens)} / ${tokens(cu.contextWindow)}` },
        h("span.st-bar", h("i", { style: { width: `${Math.min(100, pct)}%` }, class: pct > 80 ? "hot" : "" })), `${cu.tokens == null ? "—" : Math.round(pct)}%`) : null,
      st ? h("span.st-seg.dim", `${tokens(st.tokens.input + st.tokens.cacheRead)}↑ ${tokens(st.tokens.output)}↓`) : null,
      st ? h("button.st-seg.st-btn.st-cost", { title: "usage", onclick: () => app.show("usage") }, cost(st.cost)) : null,
      h("span.st-seg.dim", `${live} live`),
    );
  }
}
