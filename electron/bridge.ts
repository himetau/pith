// Loopback http server for ext/pith-bridge.ts, which runs inside each pi process. Random port,
// random token, both handed to pi through the environment
import { createServer } from "node:http";
import { randomBytes } from "node:crypto";

export type BridgeHandler = (tab: string, op: string, args: any) => Promise<unknown>;

export function startBridge(handle: BridgeHandler): Promise<{ url: string; token: string }> {
  const token = randomBytes(24).toString("hex");
  const server = createServer((req, res) => {
    const fail = (code: number, msg: string) => { res.writeHead(code, { "content-type": "application/json" }); res.end(JSON.stringify({ error: msg })); };
    if (req.method !== "POST" || req.url !== "/call") return fail(404, "nope");
    if (req.headers["x-pith-token"] !== token) return fail(403, "bad token");
    const chunks: Buffer[] = [];
    req.on("data", (c) => chunks.push(c));
    req.on("end", async () => {
      let body: any;
      try { body = JSON.parse(Buffer.concat(chunks).toString("utf8")); } catch { return fail(400, "bad json"); }
      try {
        const out = await handle(String(body.tab ?? ""), String(body.op ?? ""), body.args ?? {});
        res.writeHead(200, { "content-type": "application/json" });
        res.end(JSON.stringify({ ok: true, result: out ?? null }));
      } catch (e) {
        fail(500, e instanceof Error ? e.message : String(e));
      }
    });
  });
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => {
    const addr = server.address();
    resolve({ url: `http://127.0.0.1:${typeof addr === "object" && addr ? addr.port : 0}`, token });
  }));
}
