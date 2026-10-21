// Colours from matugen, font from the terminal, window geometry from hyprland
import { app } from "./app";
import { api } from "./lib/api";

let style: HTMLStyleElement | null = null;

export function applyTheme(css: string) {
  if (!style) { style = document.createElement("style"); style.id = "matugen"; document.head.append(style); }
  style.textContent = css;
}

const q = (f: string) => (/[\s,]/.test(f) && !/^["']/.test(f) ? `"${f}"` : f);

export function applyLook() {
  const s = app.settings, sys = app.sys, r = document.documentElement.style;
  const term = sys.font?.family;
  const mono = [s.font, term].filter(Boolean).map((f) => q(f!)).join(", ");
  r.setProperty("--mono", `${mono ? mono + ", " : ""}"JetBrains Mono", "IBM Plex Mono", ui-monospace, monospace`);
  r.setProperty("--prose", s.proseFont ? `${q(s.proseFont)}, var(--mono)` : "var(--mono)");
  r.setProperty("--weight", String(s.font ? 400 : sys.font?.weight ?? 400));
  r.setProperty("--fs", `${s.fontSize}px`);
  r.setProperty("--gap", `${sys.hypr.gap * 2}px`);
  r.setProperty("--bw", `${Math.max(1, sys.hypr.border)}px`);
  r.setProperty("--round", `${sys.hypr.rounding}px`);
  r.setProperty("--side-w", `${s.paneWidth}px`);
  if ((s.scale || 1) !== zoomed) { zoomed = s.scale || 1; void api.zoom(zoomed); }
}

let zoomed = 1;
export const SCALES = [0.9, 1, 1.1, 1.25, 1.4, 1.5, 1.75, 2];
// ctrl+= and ctrl+- step through the same stops the settings page offers
// presses quicker than the save step from where the last one was headed
let heading = 0;
export function stepScale(dir: 1 | -1 | 0) {
  const cur = heading || app.settings.scale || 1;
  const next = dir === 0 ? 1 : dir > 0 ? SCALES.find((x) => x > cur + 0.001) ?? cur : [...SCALES].reverse().find((x) => x < cur - 0.001) ?? cur;
  if (next === cur) return;
  heading = next;
  void app.saveSettings({ scale: next }).then(() => { if (heading === next) heading = 0; applyLook(); });
}
