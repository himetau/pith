// Image generation over an OpenAI-style chat endpoint (gemini image models answer with message.images).
// Used by the bridge's generate_image tool and by the app's images page. Plain node, no imports from pi
import { execFile } from "node:child_process";
import { mkdirSync, readFileSync, symlinkSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { basename, join, resolve } from "node:path";

export type ImageIn = { data: string; mimeType: string };
export type ImageOut = { files: string[]; images: ImageIn[]; text: string; usage?: { input: number; output: number } };

const agentDir = process.env.PI_CODING_AGENT_DIR || join(homedir(), ".pi", "agent");
const known: Record<string, string> = { openrouter: "https://openrouter.ai/api/v1", openai: "https://api.openai.com/v1" };

function baseUrl(provider: string): string {
  try {
    const m = JSON.parse(readFileSync(join(agentDir, "models.json"), "utf8"));
    const u = m.providers?.[provider]?.baseUrl;
    if (u) return String(u).replace(/\/$/, "");
  } catch { /* not a custom provider */ }
  if (known[provider]) return known[provider];
  throw new Error(`don't know where ${provider} lives, add it to models.json`);
}

// pi already knows how to resolve keys ($ENV, !command, auth.json), ask it instead of redoing that
function apiKey(pi: string, provider: string): Promise<string> {
  return new Promise((resolve, reject) => execFile(pi, ["auth", "print-api-key", "--provider", provider], { timeout: 15000 }, (e, out) => {
    if (e || !out.trim()) reject(new Error(`no api key for ${provider}`));
    else resolve(out.trim());
  }));
}

const slug = (s: string) => s.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 40) || "image";

export async function generateImage(o: { prompt: string; inputs?: ImageIn[]; model: string; provider: string; outDir: string; library?: string; pi?: string; aspect?: string }): Promise<ImageOut> {
  const key = await apiKey(o.pi || "pi", o.provider);
  const text = o.aspect ? `${o.prompt}\n\n(aspect ratio ${o.aspect})` : o.prompt;
  const content = o.inputs?.length
    ? [{ type: "text", text }, ...o.inputs.map((i) => ({ type: "image_url", image_url: { url: `data:${i.mimeType};base64,${i.data}` } }))]
    : text;
  const res = await fetch(`${baseUrl(o.provider)}/chat/completions`, {
    method: "POST",
    headers: { "content-type": "application/json", authorization: `Bearer ${key}` },
    body: JSON.stringify({ model: o.model, messages: [{ role: "user", content }], modalities: ["image", "text"] }),
  });
  if (!res.ok) throw new Error(`${o.model}: ${res.status} ${(await res.text()).slice(0, 300)}`);
  const j: any = await res.json();
  const msg = j.choices?.[0]?.message ?? {};
  const urls: string[] = [];
  for (const im of msg.images ?? []) if (im?.image_url?.url) urls.push(im.image_url.url);
  // some gateways put the image in content parts instead
  if (Array.isArray(msg.content)) for (const p of msg.content) if (p?.type === "image_url" && p.image_url?.url) urls.push(p.image_url.url);
  const images: ImageIn[] = [];
  for (const u of urls) {
    const m = u.match(/^data:([^;]+);base64,(.+)$/);
    if (m) images.push({ mimeType: m[1], data: m[2] });
    else {
      const r = await fetch(u);
      images.push({ mimeType: r.headers.get("content-type") || "image/png", data: Buffer.from(await r.arrayBuffer()).toString("base64") });
    }
  }
  if (!images.length) throw new Error(`${o.model} didn't return an image${typeof msg.content === "string" && msg.content ? `: ${msg.content.slice(0, 300)}` : ""}`);
  mkdirSync(o.outDir, { recursive: true });
  const stamp = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
  const files = images.map((im, i) => {
    const ext = im.mimeType.includes("png") ? "png" : im.mimeType.includes("webp") ? "webp" : "jpg";
    const file = join(o.outDir, `${stamp}-${slug(o.prompt)}${images.length > 1 ? `-${i + 1}` : ""}.${ext}`);
    writeFileSync(file, Buffer.from(im.data, "base64"));
    writeFileSync(file.replace(/\.\w+$/, ".txt"), `${o.prompt}\n\nmodel: ${o.model}\n`);
    // saved somewhere else (a design folder): link it into the library so it shows up there too
    if (o.library && resolve(o.library) !== resolve(o.outDir)) {
      try {
        mkdirSync(o.library, { recursive: true });
        symlinkSync(file, join(o.library, basename(file)));
        symlinkSync(file.replace(/\.\w+$/, ".txt"), join(o.library, basename(file).replace(/\.\w+$/, ".txt")));
      } catch { /* already linked */ }
    }
    return file;
  });
  return { files, images, text: typeof msg.content === "string" ? msg.content : "", usage: j.usage ? { input: j.usage.prompt_tokens || 0, output: j.usage.completion_tokens || 0 } : undefined };
}
