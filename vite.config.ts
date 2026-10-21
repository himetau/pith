import { defineConfig } from "vite";

export default defineConfig({
  root: "src",
  base: "./",
  build: { outDir: "../dist/ui", emptyOutDir: true, target: "chrome140" },
});
