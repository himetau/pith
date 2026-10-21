export const home = { dir: "" };

export function tilde(p: string) {
  return home.dir && p.startsWith(home.dir) ? "~" + p.slice(home.dir.length) : p;
}

export function base(p: string) {
  const parts = p.replace(/\/+$/, "").split("/");
  return parts[parts.length - 1] || p;
}

export function tokens(n: number) {
  if (!n) return "0";
  if (n < 1000) return String(n);
  if (n < 10_000) return `${(n / 1000).toFixed(1)}k`;
  if (n < 1_000_000) return `${Math.round(n / 1000)}k`;
  if (n < 10_000_000) return `${(n / 1_000_000).toFixed(2)}M`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}

export function cost(n: number) {
  if (!n) return "$0";
  if (n < 0.01) return `$${n.toFixed(4)}`;
  if (n < 10) return `$${n.toFixed(2)}`;
  if (n < 1000) return `$${n.toFixed(1)}`;
  return `$${Math.round(n).toLocaleString()}`;
}

export function ago(ts: number) {
  const s = Math.max(0, (Date.now() - ts) / 1000);
  if (s < 45) return "now";
  if (s < 3600) return `${Math.round(s / 60)}m`;
  if (s < 86400) return `${Math.round(s / 3600)}h`;
  if (s < 86400 * 7) return `${Math.round(s / 86400)}d`;
  const d = new Date(ts);
  return d.toLocaleDateString(undefined, { month: "short", day: "numeric", ...(d.getFullYear() !== new Date().getFullYear() ? { year: "2-digit" } : {}) });
}

export function dur(ms: number) {
  if (ms < 1000) return `${Math.round(ms)}ms`;
  const s = ms / 1000;
  if (s < 60) return `${s < 10 ? s.toFixed(1) : Math.round(s)}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m${Math.round(s % 60).toString().padStart(2, "0")}s`;
  return `${Math.floor(m / 60)}h${(m % 60).toString().padStart(2, "0")}m`;
}

export function bytes(n: number) {
  if (n < 1024) return `${n}B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)}KB`;
  return `${(n / 1024 / 1024).toFixed(1)}MB`;
}

export function plural(n: number, word: string, many = word + "s") {
  return `${n} ${n === 1 ? word : many}`;
}

export function dayKey(ts: number) {
  const d = new Date(ts);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

// a title out of the first prompt, for sessions nobody named
export function titleFrom(text: string) {
  const t = text.replace(/\s+/g, " ").trim();
  return t.length > 60 ? t.slice(0, 58).replace(/\s\S*$/, "") + "…" : t || "new session";
}

export const SPIN = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
export const spinFrame = () => SPIN[Math.floor(performance.now() / 80) % SPIN.length];
