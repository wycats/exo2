import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { resolve } from "path";

export default defineConfig({
  base: "./",
  plugins: [svelte()],
  resolve: {
    alias: {
      "@exosuit/core/Logger.ts": resolve(
        __dirname,
        "../exosuit-core/src/Logger.ts",
      ),
    },
  },
  build: {
    outDir: "out/webview",
    target: "es2020",
    rollupOptions: {
      input: {
        dashboard: resolve("src/webview/dashboard/index.html"),
      },
      output: {
        manualChunks(id) {
          // The VSIX file-count warning is dominated by Shiki language/theme chunks.
          // Collapse them into a single shared chunk to drastically reduce packaged files.
          if (
            id.includes("node_modules/shiki/") ||
            id.includes("node_modules/@shikijs/") ||
            id.includes("node_modules/vscode-textmate/") ||
            id.includes("node_modules/oniguruma")
          ) {
            return "shiki";
          }

          if (id.includes("node_modules/")) {
            return "vendor";
          }

          return undefined;
        },
        entryFileNames: `[name].js`,
        chunkFileNames: `[name].js`,
        assetFileNames: `[name].[ext]`,
      },
    },
  },
});
