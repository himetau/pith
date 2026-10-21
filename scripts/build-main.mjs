// esbuild bundle for electron main and the two preloads (app, preview pane)
import { build } from "esbuild";

const common = { bundle: true, platform: "node", target: "node24", format: "cjs", sourcemap: true, logLevel: "warning", external: ["electron"] };

await Promise.all([
  build({ ...common, entryPoints: ["electron/main.ts"], outfile: "dist/electron/main.cjs" }),
  build({ ...common, entryPoints: ["electron/preload.ts"], outfile: "dist/electron/preload.cjs" }),
  build({ ...common, entryPoints: ["electron/pick-preload.ts"], outfile: "dist/electron/pick-preload.cjs" }),
]);
