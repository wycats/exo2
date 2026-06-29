"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
const config_1 = require("vitest/config");
const vite_plugin_svelte_1 = require("@sveltejs/vite-plugin-svelte");
const path_1 = require("path");
exports.default = (0, config_1.defineConfig)({
    plugins: [(0, vite_plugin_svelte_1.svelte)()],
    test: {
        environment: "happy-dom",
        include: ["src/webview/**/*.test.ts"],
        alias: {
            $lib: (0, path_1.resolve)("./src/webview/studio/lib"),
        },
    },
});
//# sourceMappingURL=vitest.config.js.map