// Settings, saved as you change them (~/.config/pith/settings.json)
import { MODES } from "../actions";
import { app } from "../app";
import { api, type Settings } from "../lib/api";
import { h, swap } from "../lib/dom";
import { tilde } from "../lib/fmt";
import { icon } from "../lib/icons";
import { applyLook, SCALES } from "../look";

export class SettingsPage {
  el = h("div.page.settings");
  constructor() {
    app.on("view", () => { if (app.view === "settings") this.draw(); });
  }

  draw() {
    const st = app.settings, sys = app.sys;
    const save = (patch: Partial<Settings>) => void app.saveSettings(patch).then(() => applyLook());
    swap(this.el,
      h("div.page-head", h("h1", "settings"), h("span.dim", tilde(`${sys.configDir}/settings.json`))),
      group("pi",
        field("pi binary", "what pith runs for every session", text(st.piPath, (v) => save({ piPath: v || "pi" }))),
        field("version", "", h("span.mono", sys.piVersion)),
        field("extra arguments", "added to every pi it starts, like --no-skills", text(st.extraArgs, (v) => save({ extraArgs: v }), "--flag value")),
        field("open sessions", "idle sessions past this give their process back until you open them again", num(st.maxLive, 1, 32, (v) => save({ maxLive: v }))),
      ),
      group("defaults",
        field("mode for new sessions", "", seg(MODES.map((m) => m.id), st.mode, (v) => save({ mode: v as Settings["mode"] }))),
        field("default folder", "where new sessions start", dirField(st.defaultCwd, (v) => save({ defaultCwd: v }))),
        field("design folder", "where new design sessions start", dirField(st.designDir, (v) => save({ designDir: v }))),
        field("browser home", "first page in a session's browser pane", text(st.browserHome, (v) => save({ browserHome: v }))),
        field("notifications", "when a session in the background finishes or needs an answer", toggle(st.notify, (v) => save({ notify: v }))),
      ),
      group("look",
        field("theme", sys.theme.found ? "from matugen, follows your wallpaper live" : "matugen hasn't written it yet. run your wallpaper script once", h("span.mono", tilde(sys.theme.file))),
        field("font", `empty uses your terminal's${sys.font ? ` (${sys.font.family})` : ""}`, text(st.font, (v) => save({ font: v }), sys.font?.family ?? "monospace")),
        field("reading font", "for replies. empty keeps them in the same font", text(st.proseFont, (v) => save({ proseFont: v }), "same as above")),
        field("scale", "everything in the window. ctrl+= and ctrl+- step it, ctrl+0 resets", seg(SCALES.map(pct), pct(st.scale || 1), (v) => save({ scale: parseFloat(v) / 100 }))),
        field("text size", "", num(st.fontSize, 10, 22, (v) => save({ fontSize: v }))),
        field("windows", "gaps, borders and corners come from hyprland", h("span.mono", `gap ${sys.hypr.gap} · border ${sys.hypr.border} · rounding ${sys.hypr.rounding}`)),
      ),
      group("budgets",
        h("p.dim.g-note", "pi doesn't know your plan's limits, so these are yours. the usage page measures spend against them. 0 turns one off."),
        field("5 hour window", "", money(st.budget.window5h, (v) => save({ budget: { ...st.budget, window5h: v } }))),
        field("7 days", "", money(st.budget.week, (v) => save({ budget: { ...st.budget, week: v } }))),
        field("month", "", money(st.budget.month, (v) => save({ budget: { ...st.budget, month: v } }))),
      ),
      group("images",
        field("provider", "a provider from pi's models.json or auth", text(st.imageProvider, (v) => save({ imageProvider: v }))),
        field("model", "any model that answers with images, like gemini-3.1-flash-image or gemini-3-pro-image", text(st.imageModel, (v) => save({ imageModel: v }))),
        field("price per image", "your endpoint doesn't report image prices, so usage counts this for each one. 0 leaves it out", money(st.imageCost, (v) => save({ imageCost: v }))),
        field("library", "", h("button.link", { onclick: () => void api.openPath(sys.imagesDir) }, tilde(sys.imagesDir))),
      ),
      group("keys",
        h("div.keys", [
          ["ctrl+n", "new session"], ["ctrl+k", "commands"], ["ctrl+p", "find a session"], ["ctrl+b", "sidebar"], ["ctrl+= / ctrl+-", "scale"], ["ctrl+\\", "side window"],
          ["alt+1…6", "web · preview · changes · files · agents · terminal"], ["shift+tab", "cycle mode"], ["ctrl+m", "model"], ["ctrl+e", "effort"],
          ["esc", "stop the run"], ["enter while running", "steer"], ["alt+enter", "queue for after"], ["↑", "last prompt"], ["!cmd", "run in shell, output goes to context"], ["!!cmd", "shell, kept out of context"],
        ].map(([k, v]) => h("div.key", h("kbd", k), h("span", v)))),
      ),
    );
  }
}

function group(title: string, ...kids: (HTMLElement | null)[]) {
  return h("section.s-group", h("div.s-legend", title), ...kids);
}

function field(label: string, hint: string, control: HTMLElement) {
  return h("div.s-field", h("div.s-l", h("div", label), hint ? h("div.s-h", hint) : null), h("div.s-c", control));
}

function text(v: string, on: (v: string) => void, placeholder = "") {
  const i = h("input.s-in", { value: v, placeholder, spellcheck: false }) as HTMLInputElement;
  i.addEventListener("change", () => on(i.value.trim()));
  return i;
}

function num(v: number, min: number, max: number, on: (v: number) => void) {
  const i = h("input.s-in.s-num", { type: "number", value: String(v), min: String(min), max: String(max) }) as HTMLInputElement;
  i.addEventListener("change", () => on(Math.max(min, Math.min(max, Number(i.value) || min))));
  return i;
}

function money(v: number, on: (v: number) => void) {
  const i = h("input.s-in.s-num", { type: "number", value: String(v), min: "0", step: "0.5" }) as HTMLInputElement;
  i.addEventListener("change", () => on(Math.max(0, Number(i.value) || 0)));
  return h("span.s-money", h("span.dim", "$"), i);
}

function toggle(v: boolean, on: (v: boolean) => void) {
  const b = h(`button.toggle${v ? ".on" : ""}`, { onclick: () => { v = !v; b.classList.toggle("on", v); on(v); } }, h("i"));
  return b;
}

const pct = (x: number) => `${Math.round(x * 100)}%`;

function seg(opts: readonly string[], v: string, on: (v: string) => void) {
  const box = h("div.segs");
  const draw = () => swap(box, opts.map((o) => h(`button.seg${o === v ? ".on" : ""}`, { onclick: () => { v = o; draw(); on(o); } }, o)));
  draw();
  return box;
}

function dirField(v: string, on: (v: string) => void) {
  const i = text(v, on);
  return h("span.s-dir", i, h("button.pb", { title: "browse", onclick: async () => { const d = await api.pickDir(i.value); if (d) { i.value = d; on(d); } } }, icon("folder")));
}
