// New session: pick a folder (or a design), type, go
import { app } from "../app";
import { runBuiltin } from "../commands";
import { api } from "../lib/api";
import { h, swap } from "../lib/dom";
import { ago, base, tilde, titleFrom } from "../lib/fmt";
import { icon } from "../lib/icons";
import { chatFor } from "./chat";
import { Composer } from "./composer";

export class Home {
  el: HTMLElement;
  composer: Composer;
  private folders = h("div.h-folders");
  private recent = h("div.h-recent");
  private kindEl = h("div.h-kind");
  private cwdEl = h("button.h-cwd");

  constructor() {
    this.composer = new Composer({
      tab: () => null,
      cwd: () => this.cwd(),
      home: true,
      placeholder: "what are we doing  ·  / commands  ·  @ files",
      send: (text, images) => void this.go(text, images),
    });
    this.el = h("div.home",
      h("div.h-inner",
        h("div.h-top", h("div.h-title", h("span.accent", "❯"), " new ", h("span.h-k")), this.kindEl),
        h("div.h-where", h("span.dim", "in "), this.cwdEl, h("button.btn.sm.ghost", { onclick: () => void this.browse() }, icon("folder"), "browse")),
        this.folders,
        this.composer.el,
        h("div.h-cols",
          h("section", h("div.h-sec", "recent"), this.recent),
          h("section", h("div.h-sec", "start from"), h("div.h-starts",
            start("/review", "review what changed for bugs", () => this.fill("/review")),
            start("/design", "make something visual, live preview", () => { this.setKind("design"); this.fill("/design "); }),
            start("/image", "generate or edit an image", () => app.show("images")),
            start("/explain", "how does a part of this work", () => this.fill("/explain ")),
            start("/init", "write an AGENTS.md for this repo", () => this.fill("/init")),
            start("/security-review", "look for holes in the pending changes", () => this.fill("/security-review")),
          )),
        ),
      ),
    );
    this.cwdEl.onclick = () => void this.browse();
    app.on("sessions", () => this.draw());
    app.on("settings", () => this.draw());
    this.draw();
  }

  private cwd() { return app.homeCwd || app.settings.defaultCwd || app.sys.home; }
  private fill(t: string) { this.composer.setText(t); this.composer.focus(); }

  private setKind(k: "chat" | "design") {
    app.homeKind = k;
    if (k === "design" && (!app.homeCwd || app.homeCwd === app.settings.defaultCwd)) app.homeCwd = app.settings.designDir;
    this.draw();
  }

  private async browse() {
    const d = await api.pickDir(this.cwd());
    if (d) { app.homeCwd = d; this.draw(); this.composer.focus(); }
  }

  draw() {
    const k = app.homeKind;
    swap(this.el.querySelector(".h-k")!, k === "design" ? "design" : "session");
    swap(this.kindEl, (["chat", "design"] as const).map((x) => h(`button.seg${k === x ? ".on" : ""}`, { onclick: () => this.setKind(x) }, x === "chat" ? "chat" : "design")));
    swap(this.cwdEl, tilde(this.cwd()));
    const seen = new Set<string>();
    const recentDirs: string[] = [];
    for (const s of app.sessions) if (s.cwd && !seen.has(s.cwd)) { seen.add(s.cwd); recentDirs.push(s.cwd); }
    swap(this.folders, recentDirs.slice(0, 8).map((d) => h(`button.fchip${d === this.cwd() ? ".on" : ""}`, { title: d, onclick: () => { app.homeCwd = d; this.draw(); this.composer.focus(); } }, base(d) || "/")));
    swap(this.recent, app.sessions.slice(0, 9).map((s) => h("button.h-row", { onclick: () => app.open(s) }, h("span.h-rt", s.name || titleFrom(s.firstPrompt)), h("span.h-rc", base(s.cwd)), h("span.h-ra", ago(s.updated)))));
    if (!app.sessions.length) swap(this.recent, h("div.dim", "nothing yet"));
  }

  private async go(text: string, images: { data: string; mimeType: string }[]) {
    if (text.startsWith("/") && /^\/(usage|settings)\b/.test(text)) { await runBuiltin(null, text); return; }
    const t = app.fresh(this.cwd(), app.homeKind, this.composer.pendingMode);
    if (app.homeKind === "design") t.pane = "preview";
    // templates only expand once pi is up and has told us its commands
    if (text.startsWith("/")) await t.ensure();
    const chat = chatFor(t);
    await chat.send(text, images, "auto");
  }
}

function start(cmd: string, desc: string, run: () => void) {
  return h("button.h-start", { onclick: run }, h("span.accent", cmd), h("span.dim", desc));
}
