import { defineConfig } from "vitest/config";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { resolve } from "path";

export default defineConfig({
  plugins: [svelte()],
  resolve: {
    alias: {
      // Provide a stub for vscode module in unit tests
      // The actual mock is provided via vi.mock() in test files
      vscode: resolve("./src/__test__/vscode-stub.ts"),
      // Direct Logger import to avoid bundling Node.js APIs
      "@exosuit/core/Logger.ts": resolve(
        __dirname,
        "../exosuit-core/src/Logger.ts",
      ),
      // Stub WASM module to prevent Vite from trying to resolve the binary
      "../wasm/exosuit_reactivity.js": resolve("./src/__test__/wasm-stub.ts"),
    },
  },
  test: {
    environment: "happy-dom",
    include: [
      "src/webview/**/*.test.ts",
      "eslint-rules/**/*.test.ts",
      "src/tasks/**/*.test.ts",
      "src/services/**/*.test.ts",
      "src/machine-channel/**/*.test.ts",
      "src/lmtool/**/*.test.ts",
      "../../scripts/dev/**/*.test.ts",
    ],
    exclude: [
      // ReactivityService test requires complex WASM mocking
      // TODO: Fix WASM loading mock for this test
      "src/services/ReactivityService.test.ts",
    ],
    environmentMatchGlobs: [
      ["eslint-rules/**/*.test.ts", "node"],
      ["src/tasks/**/*.test.ts", "node"],
      ["src/services/**/*.test.ts", "node"],
      ["src/machine-channel/**/*.test.ts", "node"],
      ["src/lmtool/**/*.test.ts", "node"],
      ["../../scripts/dev/**/*.test.ts", "node"],
    ],
    alias: {
      $lib: resolve("./src/webview/studio/lib"),
    },
  },
});
