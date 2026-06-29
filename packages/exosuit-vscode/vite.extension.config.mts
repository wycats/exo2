import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { resolve } from "path";
import { builtinModules } from "module";

export default defineConfig({
  define: {
    __BUILD_STAMP__: JSON.stringify(new Date().toISOString()),
  },
  // __BUILD_STAMP__ is injected via `define` above
  plugins: [svelte()],
  resolve: {
    alias: {
      "@exosuit/rtd": resolve(__dirname, "../exosuit-rtd/src/index.ts"),
    },
    conditions: ["node"],
  },
  build: {
    target: "node18",
    assetsInlineLimit: 0,
    lib: {
      entry: resolve(__dirname, "src/extension.ts"),
      fileName: () => "extension.js",
      formats: ["es"],
    },
    outDir: "out",
    emptyOutDir: false,
    rollupOptions: {
      external: [
        "vscode",
        ...builtinModules,
        ...builtinModules.map((m) => `node:${m}`),
      ],
      output: {
        entryFileNames: "extension.js",
      },
    },
    sourcemap: true,
    minify: process.env.MINIFY === "true",
  },
});
