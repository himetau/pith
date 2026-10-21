// Usage: limits against your own budgets, totals, a year of days, last 30 days, and where it went
import { app } from "../app";
import { api, type Spend, type UsageReport } from "../lib/api";
import { h, s, swap } from "../lib/dom";
import { base, cost, dayKey, tilde, tokens } from "../lib/fmt";
import { icon } from "../lib/icons";

type Row = Spend & { sess: number };
const HOUR = 3600_000, DAY = 24 * HOUR;

const tot = (rows: Spend[]) => rows.reduce((a, r) => ({ cost: a.cost + r[6], input: a.input + r[2], output: a.output + r[3], cacheRead: a.cacheRead + r[4], cacheWrite: a.cacheWrite + r[5], n: a.n + 1 }), { cost: 0, input: 0, output: 0, cacheRead: 0, cacheWrite: 0, n: 0 });
const allTok = (t: ReturnType<typeof tot>) => t.input + t.output + t.cacheRead + t.cacheWrite;

export class UsagePage {
  el = h("div.page.usage");
  private data: UsageReport | null = null;
  private metric: "cost" | "tokens" = "cost";
  private tip = h("div.tip");

  constructor() {
    app.on("view", () => { if (app.view === "usage") void this.load(); });
  }

  async load() {
    if (!this.data) swap(this.el, h("div.page-head", h("h1", "usage")), h("div.dim", "reading sessions…"));
    this.data = await api.usage();
    this.draw();
  }

  private draw() {
    const d = this.data!;
    const rows: Row[] = d.spend.flatMap((x) => x.rows.map((r) => Object.assign([...r] as Spend, { sess: x.s }) as Row));
    const now = Date.now();
    const sinceMidnight = new Date(); sinceMidnight.setHours(0, 0, 0, 0);
    const monthStart = new Date(); monthStart.setHours(0, 0, 0, 0); monthStart.setDate(1);
    const within = (ms: number) => rows.filter((r) => r[0] >= now - ms);
    const b = app.settings.budget;

    swap(this.el,
      h("div.page-head", h("h1", "usage"), h("span.dim", `${d.sessions.length} sessions · ${rows.length.toLocaleString()} model calls on record`), h("span.grow"),
        h("button.btn.sm.ghost", { onclick: () => void this.load() }, icon("refresh"), "reload")),
      h("section.u-limits",
        limit("5 hour window", within(5 * HOUR), b.window5h, () => {
          const w = within(5 * HOUR).sort((a, c) => a[0] - c[0]);
          return w.length ? `oldest call ages out in ${Math.max(1, Math.round((w[0][0] + 5 * HOUR - now) / 60000))}m` : "nothing in the window";
        }),
        limit("last 7 days", within(7 * DAY), b.week, () => "rolling"),
        limit("this month", rows.filter((r) => r[0] >= monthStart.getTime()), b.month, () => {
          const next = new Date(monthStart); next.setMonth(next.getMonth() + 1);
          return `resets in ${Math.ceil((next.getTime() - now) / DAY)}d`;
        }),
      ),
      h("section.u-tiles",
        tile("today", tot(rows.filter((r) => r[0] >= sinceMidnight.getTime()))),
        tile("7 days", tot(within(7 * DAY))),
        tile("30 days", tot(within(30 * DAY))),
        tile("all time", tot(rows)),
      ),
      this.current(),
      h("section.u-block", h("div.u-sec", h("span", "every day, last 26 weeks"), h("span.dim", "darker is more spend")), this.heat(rows)),
      h("section.u-block",
        h("div.u-sec", h("span", "last 30 days"), h("span.grow"), h("div.segs", (["cost", "tokens"] as const).map((m) => h(`button.seg${this.metric === m ? ".on" : ""}`, { onclick: () => { this.metric = m; this.draw(); } }, m)))),
        this.bars(rows)),
      h("div.u-cols",
        h("section.u-block", h("div.u-sec", "by model"), this.table(group(rows, (r) => r[1]), (k) => k.replace(/^[^/]+\//, ""), (k) => k.split("/")[0])),
        h("section.u-block", h("div.u-sec", "by folder"), this.table(group(rows, (r) => d.sessions[r.sess]?.cwd ?? "?"), (k) => base(k) || "/", (k) => tilde(k))),
      ),
      h("section.u-block", h("div.u-sec", "most expensive sessions"), this.topSessions()),
      this.tip,
    );
  }

  private current() {
    const t = app.active;
    const st = t?.stats;
    if (!t || !st) return null;
    const cu = st.contextUsage;
    const pct = cu?.percent ?? 0;
    return h("section.u-block.u-now",
      h("div.u-sec", h("span", "this session"), h("span.dim", t.title)),
      h("div.u-now-grid",
        h("div", h("div.u-k", "context"), h("div.u-ctx", h("div.u-ctx-fill", { style: { width: `${Math.min(100, pct)}%` } })), h("div.dim", cu?.tokens != null ? `${tokens(cu.tokens)} of ${tokens(cu.contextWindow)} · ${Math.round(pct)}%` : "known after the next reply")),
        kv("input", tokens(st.tokens.input)), kv("output", tokens(st.tokens.output)), kv("cache read", tokens(st.tokens.cacheRead)), kv("cache write", tokens(st.tokens.cacheWrite)), kv("cost", cost(st.cost)), kv("tool calls", String(st.toolCalls)),
      ),
    );
  }

  private heat(rows: Row[]) {
    const byDay = new Map<string, Spend[]>();
    for (const r of rows) { const k = dayKey(r[0]); if (!byDay.has(k)) byDay.set(k, []); byDay.get(k)!.push(r); }
    const end = new Date(); end.setHours(0, 0, 0, 0);
    const start = new Date(end.getTime() - (26 * 7 - 1) * DAY);
    start.setDate(start.getDate() - ((start.getDay() + 6) % 7)); // weeks start monday
    const days: { k: string; d: Date; t: ReturnType<typeof tot> }[] = [];
    for (let d = new Date(start); d <= end; d = new Date(d.getTime() + DAY + 2 * HOUR)) { d.setHours(0, 0, 0, 0); days.push({ k: dayKey(d.getTime()), d: new Date(d), t: tot(byDay.get(dayKey(d.getTime())) ?? []) }); }
    const vals = days.map((x) => x.t.cost).filter((v) => v > 0).sort((a, b) => a - b);
    const q = (p: number) => vals[Math.min(vals.length - 1, Math.floor(p * vals.length))] ?? 0;
    const steps = [q(0.25), q(0.5), q(0.75), q(0.9)];
    const level = (v: number) => (v <= 0 ? 0 : 1 + steps.filter((x) => v > x).length);
    const cell = 13, gap = 3, weeks = Math.ceil(days.length / 7);
    const vw = weeks * (cell + gap) + 30, vh = 7 * (cell + gap) + 18;
    // drawn at 1:1 so the labels stay text sized
    const svg = s("svg", { class: "heat", width: vw, height: vh, viewBox: `0 0 ${vw} ${vh}`, role: "img", "aria-label": "daily spend for the last 26 weeks" });
    let lastMonth = -1;
    days.forEach((x, i) => {
      const w = Math.floor(i / 7), dow = i % 7;
      const r = s("rect", { x: 30 + w * (cell + gap), y: 18 + dow * (cell + gap), width: cell, height: cell, rx: 3, class: `hc l${level(x.t.cost)}` });
      r.addEventListener("mousemove", (e) => this.showTip(e, `${x.d.toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" })}`, x.t.n ? `${cost(x.t.cost)} · ${tokens(allTok(x.t))} tokens · ${x.t.n} calls` : "nothing"));
      r.addEventListener("mouseleave", () => this.hideTip());
      svg.append(r);
      if (dow === 0 && x.d.getMonth() !== lastMonth) {
        lastMonth = x.d.getMonth();
        svg.append(s("text", { x: 30 + w * (cell + gap), y: 11, class: "hl" }, x.d.toLocaleDateString(undefined, { month: "short" })));
      }
    });
    ["mon", "", "wed", "", "fri", "", ""].forEach((l, i) => l && svg.append(s("text", { x: 0, y: 18 + i * (cell + gap) + cell - 3, class: "hl" }, l)));
    const legend = h("div.heat-key", h("span.dim", "less"), [0, 1, 2, 3, 4, 5].map((l) => h(`i.hc.l${l}`)), h("span.dim", "more"));
    return h("div", svg, legend);
  }

  private bars(rows: Row[]) {
    const end = new Date(); end.setHours(0, 0, 0, 0);
    const days = Array.from({ length: 30 }, (_, i) => new Date(end.getTime() - (29 - i) * DAY + 2 * HOUR));
    const byDay = new Map<string, Spend[]>();
    for (const r of rows) { const k = dayKey(r[0]); if (!byDay.has(k)) byDay.set(k, []); byDay.get(k)!.push(r); }
    const vals = days.map((d) => { const t = tot(byDay.get(dayKey(d.getTime())) ?? []); return { d, t, v: this.metric === "cost" ? t.cost : allTok(t) }; });
    const max = Math.max(...vals.map((x) => x.v), this.metric === "cost" ? 0.01 : 1);
    const W = 1000, H = 180, pad = 46, top = 12, bw = (W - pad) / 30;
    const svg = s("svg", { class: "bars", viewBox: `0 ${-top} ${W} ${H + 24 + top}`, role: "img", "aria-label": `${this.metric} per day, last 30 days` });
    for (const f of [0.5, 1]) {
      const y = H - f * H;
      svg.append(s("line", { x1: pad, x2: W, y1: y, y2: y, class: "grid" }), s("text", { x: 0, y: y + 4, class: "hl" }, this.metric === "cost" ? cost(max * f) : tokens(max * f)));
    }
    svg.append(s("line", { x1: pad, x2: W, y1: H, y2: H, class: "base" }));
    vals.forEach((x, i) => {
      const bh = x.v > 0 ? Math.max(3, (x.v / max) * (H - 6)) : 0;
      const bx = pad + i * bw + 1, w = bw - 2;
      if (bh) svg.append(s("path", { class: "bar", d: roundTop(bx, H - bh, w, bh, Math.min(4, w / 2)) }));
      const hit = s("rect", { x: pad + i * bw, y: 0, width: bw, height: H, class: "hit" });
      hit.addEventListener("mousemove", (e) => this.showTip(e, x.d.toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" }), x.t.n ? `${cost(x.t.cost)} · ${tokens(x.t.input)} in · ${tokens(x.t.output)} out · ${tokens(x.t.cacheRead)} cached` : "nothing"));
      hit.addEventListener("mouseleave", () => this.hideTip());
      svg.append(hit);
      if (i % 5 === 4 || i === 0) svg.append(s("text", { x: i === 29 ? W : pad + i * bw + bw / 2, y: H + 16, class: `hl ${i === 29 ? "end" : "mid"}` }, i === 29 ? "today" : x.d.toLocaleDateString(undefined, { month: "short", day: "numeric" })));
    });
    return svg;
  }

  private table(g: Map<string, Spend[]>, name: (k: string) => string, sub: (k: string) => string) {
    const list = [...g.entries()].map(([k, r]) => ({ k, t: tot(r) })).sort((a, b) => b.t.cost - a.t.cost || allTok(b.t) - allTok(a.t)).slice(0, 10);
    const max = Math.max(...list.map((x) => x.t.cost), 0.0001);
    if (!list.length) return h("div.dim", "nothing yet");
    return h("table.u-table",
      h("thead", h("tr", h("th", ""), h("th.n", "in"), h("th.n", "out"), h("th.n", "cached"), h("th.n", "cost"))),
      h("tbody", list.map((x) => h("tr",
        h("td", h("div.u-name", name(x.k)), h("div.u-sub", sub(x.k)), h("div.u-mag", h("i", { style: { width: `${(x.t.cost / max) * 100}%` } }))),
        h("td.n", tokens(x.t.input)), h("td.n", tokens(x.t.output)), h("td.n", tokens(x.t.cacheRead)), h("td.n.b", cost(x.t.cost))))));
  }

  private topSessions() {
    const list = [...this.data!.sessions].sort((a, b) => b.usage.cost - a.usage.cost).slice(0, 8);
    if (!list.length) return h("div.dim", "nothing yet");
    return h("div.u-top", list.map((x) => h("button.u-trow", { onclick: () => { const s = app.sessions.find((y) => y.file === x.file); if (s) app.open(s); } },
      h("span.u-tt", x.title), h("span.dim", base(x.cwd)), h("span.n", tokens(x.usage.input + x.usage.output + x.usage.cacheRead)), h("span.n.b", cost(x.usage.cost)))));
  }

  private showTip(e: MouseEvent, title: string, body: string) {
    swap(this.tip, h("b", title), h("div", body));
    this.tip.classList.add("on");
    const r = this.el.getBoundingClientRect();
    this.tip.style.left = `${Math.min(e.clientX - r.left + 14, r.width - 260)}px`;
    this.tip.style.top = `${e.clientY - r.top + this.el.scrollTop + 14}px`;
  }
  private hideTip() { this.tip.classList.remove("on"); }
}

function roundTop(x: number, y: number, w: number, hgt: number, r: number) {
  r = Math.min(r, hgt);
  return `M${x},${y + hgt}V${y + r}Q${x},${y} ${x + r},${y}H${x + w - r}Q${x + w},${y} ${x + w},${y + r}V${y + hgt}Z`;
}

function group(rows: Row[], key: (r: Row) => string) {
  const m = new Map<string, Spend[]>();
  for (const r of rows) { const k = key(r); if (!m.has(k)) m.set(k, []); m.get(k)!.push(r); }
  return m;
}

function tile(label: string, t: ReturnType<typeof tot>) {
  return h("div.u-tile", h("div.u-k", label), h("div.u-big", cost(t.cost)), h("div.u-sub2", `${tokens(allTok(t))} tokens · ${t.n.toLocaleString()} calls`));
}

function kv(k: string, v: string) { return h("div", h("div.u-k", k), h("div.u-v", v)); }

// budgets are the user's own, pi has no limits to report. state gets a word, never just a colour
function limit(label: string, rows: Spend[], budget: number, note: () => string) {
  const spent = rows.reduce((a, r) => a + r[6], 0);
  const pct = budget > 0 ? (spent / budget) * 100 : 0;
  const state = budget <= 0 ? "" : pct >= 100 ? "over" : pct >= 80 ? "close" : "ok";
  const cells = 24, on = budget > 0 ? Math.min(cells, Math.round((pct / 100) * cells)) : 0;
  return h(`div.u-limit.${state || "none"}`,
    h("div.ul-top", h("span.u-k", label), state ? h(`span.ul-state.${state}`, state === "over" ? "over budget" : state === "close" ? "near budget" : "within budget") : null),
    h("div.ul-amt", h("span.u-big", cost(spent)), budget > 0 ? h("span.dim", ` of ${cost(budget)}`) : h("button.link", { onclick: () => app.show("settings") }, "set a budget")),
    h("div.ul-cells", Array.from({ length: cells }, (_, i) => h(`i${i < on ? ".on" : ""}`))),
    h("div.dim.ul-note", budget > 0 ? `${Math.round(pct)}% · ${note()}` : note()),
  );
}
