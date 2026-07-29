import { sveltekit } from "@sveltejs/kit/vite";
import { loadEnv } from "vite";
import { defineConfig } from "vitest/config";

export default defineConfig(({ mode }) => {
  const configuredOrigin = loadEnv(
    mode,
    process.cwd(),
    "",
  ).EXO_WORKBENCH_DEV_ORIGIN?.trim();
  const diagnosticOrigin = configuredOrigin
    ? new URL(configuredOrigin).origin
    : null;

  return {
    plugins: [sveltekit()],
    resolve: {
      conditions: ["browser"],
    },
    server: diagnosticOrigin
      ? {
          proxy: {
            "/api": {
              target: diagnosticOrigin,
              changeOrigin: true,
              configure(proxy) {
                proxy.on("proxyReq", (request) => {
                  request.setHeader("Origin", diagnosticOrigin);
                });
              },
            },
          },
        }
      : undefined,
    test: {
      environment: "happy-dom",
      include: ["src/**/*.test.ts"],
    },
  };
});
