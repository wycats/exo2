import { defineConfig } from "vite";
import { resolve } from "path";
import { builtinModules } from "module";
import glob from "glob";

const testFiles = glob
  .sync("src/test/**/*.ts", { nodir: true })
  .filter((file) => !file.endsWith(".d.ts"));
const input = Object.fromEntries(
  testFiles.map((file) => [
    file.replace(/^src\/test\//, "").replace(/\.ts$/, ""),
    resolve(__dirname, file),
  ]),
);

export default defineConfig({
  resolve: {
    alias: {
      "@exosuit/rtd": resolve(__dirname, "../exosuit-rtd/src/index.ts"),
    },
    conditions: ["node"],
  },
  build: {
    target: "node18",
    lib: {
      entry: resolve(__dirname, "src/test/runTest.ts"),
      formats: ["es"],
    },
    outDir: "out/test",
    emptyOutDir: false,
    assetsInlineLimit: 10 * 1024 * 1024,
    rollupOptions: {
      input,
      external: [
        "vscode",
        "@vscode/test-electron",
        "glob",
        ...builtinModules,
        ...builtinModules.map((m) => `node:${m}`),
      ],
      output: {
        format: "es",
        entryFileNames: "[name].js",
        chunkFileNames: "chunks/[name].js",
      },
    },
    sourcemap: true,
  },
});
