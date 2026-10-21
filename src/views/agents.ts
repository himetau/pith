// Agent definitions the subagent extension reads (~/.pi/agent/agents, .pi/agents), and summoning them
import { app } from "../app";
import { api, type AgentDef } from "../lib/api";
import { h, swap } from "../lib/dom";
import { tilde } from "../lib/fmt";
import { icon } from "../lib/icons";
import { hub } from "../panes";

export class AgentsPage {
  el = h("div.page.agents-page");
  private defs: AgentDef[] = [];
  private sel: AgentDef | null = null;

  constructor() {
    app.on("view", () => { if (app.view === "agents") void this.load(); });
  }

  async load() {
    this.defs = await api.agentDefs(app.active?.cwd);
    if (this.sel) this.sel = this.defs.find((d) => d.file === this.sel!.file) ?? null;
    this.draw();
  }

  draw() {
    const running = (hub?.agents ?? []).filter((a: any) => ["spawning", "thinking", "tool", "streaming"].includes(a.status)).length;
    swap(this.el,
      h("div.page-head", h("h1", "agents"), h("span.dim", `${this.defs.length} defined · ${running} running`), h("span.grow"),
        h("button.btn.sm", { onclick: () => { this.sel = { name: "", description: "", tools: "read, grep, find, ls, bash", model: "", file: "", scope: "user", body: "" }; this.draw(); } }, icon("plus"), "new agent"),
        app.active ? h("button.btn.sm.ghost", { onclick: () => app.active && (app.active.pane = "agents", app.focus(app.active)) }, "live view") : null),
      h("p.dim.g-note", "subagents are separate pi processes the main agent hands work to. it picks them by name and description, so say plainly what each is for. this needs a subagent extension in ~/.pi/agent/extensions."),
      h("div.ag-page",
        h("div.ag-defs", this.defs.map((d) => h(`button.ag-def${this.sel?.file === d.file ? ".on" : ""}`, { onclick: () => { this.sel = d; this.draw(); } },
          h("div.ag-dn", d.name, d.scope === "project" ? h("span.tagx", "project") : null), h("div.ag-dd", d.description), d.tools ? h("div.ag-dt", d.tools) : null))),
        this.sel ? this.editor(this.sel) : h("div.pane-empty", h("div.dim", "pick an agent to edit, or summon one")),
      ),
    );
  }

  private editor(d: AgentDef) {
    const name = h("input.s-in", { value: d.name, placeholder: "name, like reviewer" }) as HTMLInputElement;
    const desc = h("input.s-in", { value: d.description, placeholder: "when the main agent should use it" }) as HTMLInputElement;
    const tools = h("input.s-in", { value: d.tools, placeholder: "read, grep, find, ls, bash, edit, write" }) as HTMLInputElement;
    const model = h("input.s-in", { value: d.model, placeholder: "inherit" }) as HTMLInputElement;
    const body = h("textarea.s-in.ag-body", { rows: 16, placeholder: "the agent's system prompt" }) as HTMLTextAreaElement;
    body.value = d.body;
    const task = h("input.s-in", { placeholder: "a task for it" }) as HTMLInputElement;
    const save = async () => {
      if (!name.value.trim()) return app.toast("it needs a name", "warning");
      const file = await api.saveAgent({ name: name.value.trim(), description: desc.value.trim(), tools: tools.value.trim(), model: model.value.trim(), body: body.value, file: d.file || undefined }, app.active?.cwd, d.scope);
      app.toast(`saved ${tilde(file)}`);
      this.sel = { ...d, file };
      await this.load();
    };
    const summon = () => {
      const t = app.active;
      if (!t) return app.toast("open a session to summon into", "warning");
      if (!task.value.trim()) return task.focus();
      void t.send(`/summon ${name.value.trim() || d.name} ${task.value.trim()}`);
      app.focus(t);
      t.pane = "agents";
      app.focus(t);
    };
    task.addEventListener("keydown", (e) => { if (e.key === "Enter") summon(); });
    return h("div.ag-edit",
      h("div.ag-row2", lab("name", name), lab("model", model)),
      lab("description", desc),
      lab("tools", tools),
      lab("prompt", body),
      h("div.ag-acts", h("button.btn.pri", { onclick: () => void save() }, "save"), d.file ? h("span.dim", tilde(d.file)) : null),
      h("div.ag-summon", h("div.s-legend", "summon into the current session"), h("div.ag-row2", task, h("button.btn", { onclick: summon }, icon("agents"), "summon"))),
    );
  }
}

function lab(l: string, el: HTMLElement) { return h("label.lab", h("span", l), el); }
