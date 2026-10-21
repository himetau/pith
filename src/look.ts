// Colours from matugen, font from the terminal, window geometry from hyprland
import { app } from "./app";

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
}
