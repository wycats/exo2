import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests/e2e",
  timeout: 60000,
  retries: 0,
  // Limit to 1 worker to prevent fork/EAGAIN resource exhaustion.
  // Each VS Code E2E test spawns ~10-20 child processes (Electron, GPU,
  // extension host, etc.). Running multiple workers in parallel quickly
  // exhausts system process limits, causing "fork: Resource temporarily
  // unavailable" (EAGAIN) errors.
  workers: 1,
  use: {
    trace: "on-first-retry",
    screenshot: "only-on-failure",
  },
});
