import {
  _electron as electron,
  Page,
  ElectronApplication,
  Frame,
} from "playwright";
import { test as base } from "@playwright/test";
import * as path from "path";
import * as fs from "fs";
import * as os from "os";
import * as crypto from "crypto";
import { fileURLToPath } from "url";
import { NotebookFixture } from "./notebook-fixture";
import { WorkbenchFixture } from "./workbench-fixture";
import { ExosuitTest } from "./lib/exosuit-test";
import { clearTestLogs, dumpTestLogs, testLogger } from "./test-logger";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

/**
 * Sanitizes the environment variables to ensure the test instance is isolated
 * from the host VS Code instance (e.g. when running tests from the Integrated Terminal).
 */
function sanitizeEnv(originalEnv: NodeJS.ProcessEnv): NodeJS.ProcessEnv {
  const env = { ...originalEnv };

  // Remove ALL VS Code environment variables to ensure complete isolation
  // This includes VSCODE_IPC_HOOK_CLI, VSCODE_PID, VSCODE_CWD, VSCODE_GIT_*, etc.
  for (const key in env) {
    if (key.startsWith("VSCODE_")) {
      delete env[key];
    }
  }

  // Also remove TERM_PROGRAM to prevent tools from detecting they are in VS Code
  if (env.TERM_PROGRAM === "vscode") {
    delete env.TERM_PROGRAM;
    delete env.TERM_PROGRAM_VERSION;
  }

  return env;
}

function parseDisplayNumber(display: string | undefined): string | null {
  if (!display) return null;
  const match = display.match(/^:(\d+)/);
  return match ? match[1] : null;
}

async function getX11SocketInodes(displayNum: string): Promise<number[]> {
  const paths = [
    `/tmp/.X11-unix/X${displayNum}`,
    `@/tmp/.X11-unix/X${displayNum}`,
  ];
  try {
    const raw = await fs.promises.readFile("/proc/net/unix", "utf8");
    const inodes: number[] = [];
    for (const line of raw.split("\n")) {
      for (const socketPath of paths) {
        if (!line.includes(socketPath)) continue;
        const parts = line.trim().split(/\s+/);
        const pathIndex = parts.findIndex((p) => p === socketPath);
        if (pathIndex > 0) {
          const inode = Number.parseInt(parts[pathIndex - 1] ?? "", 10);
          if (!Number.isNaN(inode)) inodes.push(inode);
        }
      }
    }
    return Array.from(new Set(inodes));
  } catch {
    return [];
  }
}

async function getChildPids(pid: number): Promise<number[]> {
  try {
    const raw = await fs.promises.readFile(
      `/proc/${pid}/task/${pid}/children`,
      "utf8",
    );
    return raw
      .trim()
      .split(/\s+/)
      .filter(Boolean)
      .map((p) => Number.parseInt(p, 10))
      .filter((n) => Number.isFinite(n));
  } catch {
    return [];
  }
}

async function safeReadCmdline(pid: number): Promise<string | null> {
  try {
    const raw = await fs.promises.readFile(`/proc/${pid}/cmdline`, "utf8");
    const parts = raw.split("\u0000").filter(Boolean);
    return parts.length ? parts.join(" ") : null;
  } catch {
    return null;
  }
}

async function safeReadDisplayEnv(pid: number): Promise<string[]> {
  try {
    const raw = await fs.promises.readFile(`/proc/${pid}/environ`, "utf8");
    return raw
      .split("\u0000")
      .filter(Boolean)
      .filter(
        (l) =>
          l.startsWith("DISPLAY=") ||
          l.startsWith("WAYLAND_DISPLAY=") ||
          l.startsWith("XDG_SESSION_TYPE="),
      );
  } catch {
    return [];
  }
}

export const test = base.extend<{
  holodeckPath: string;
  app: ElectronApplication;
  page: Page;
  dashboard: Frame;
  notebook: NotebookFixture;
  workbench: WorkbenchFixture;
  exo: ExosuitTest;
}>({
  holodeckPath: async ({}, use) => {
    clearTestLogs();
    const dir = await fs.promises.mkdtemp(
      path.join(os.tmpdir(), "exosuit-holodeck-"),
    );
    testLogger.debug(`Created Holodeck at: ${dir}`);
    await use(dir);
    try {
      await fs.promises.rm(dir, { recursive: true, force: true });
    } catch (e) {
      testLogger.warn(`Failed to cleanup Holodeck: ${(e as Error).message}`);
    }
  },
  notebook: async ({ page }, use) => {
    await use(new NotebookFixture(page));
  },
  workbench: async ({ page }, use) => {
    await use(new WorkbenchFixture(page));
  },
  exo: async ({ page, workbench, holodeckPath }, use, testInfo) => {
    const exo = new ExosuitTest(page, workbench, holodeckPath);
    await use(exo);

    // Treat unexpected webview console/page errors as test failures.
    // If it's important enough to log as an error, it should fail the test.
    try {
      if (testInfo.status === "passed") {
        exo.monitor.assertNoUnexpectedErrors();
      }
    } catch (e) {
      testLogger.error(
        `[Holodeck] Unexpected webview errors in '${testInfo.title}': ${(e as Error).message}`,
      );
      exo.dumpLogs();
      await exo.dumpFrames();
      dumpTestLogs(`Test '${testInfo.title}' failure diagnostics`);
      throw e;
    }

    if (testInfo.status !== "passed" && testInfo.status !== "skipped") {
      testLogger.error(
        `[Holodeck] Test '${testInfo.title}' failed. Dumping diagnostics...`,
      );
      exo.dumpLogs();
      await exo.dumpFrames();
      dumpTestLogs(`Test '${testInfo.title}' failure diagnostics`);
    }
  },
  app: async ({ holodeckPath }, use) => {
    testLogger.debug("Initializing app fixture...");
    const extensionPath = path.resolve(__dirname, "../../");

    // Use VSCODE_PATH env var or default to 'code' (which might not work if not in PATH or if it's a wrapper)
    // Better to require VSCODE_PATH for now.
    const executablePath = process.env.VSCODE_PATH;

    if (!executablePath) {
      testLogger.warn(
        'VSCODE_PATH not set. Attempting to use default "code" executable. This may fail if "code" is a shell script wrapper.',
      );
    } else {
      testLogger.debug(`Using VSCODE_PATH: ${executablePath}`);
    }

    // Create a unique user data directory for this test worker
    const userDataDir = await fs.promises.mkdtemp(
      path.join(os.tmpdir(), "vscode-test-user-data-"),
    );
    const extensionsDir = await fs.promises.mkdtemp(
      path.join(os.tmpdir(), "vscode-test-extensions-"),
    );
    testLogger.debug(`Using user data dir: ${userDataDir}`);
    testLogger.debug(`Using extensions dir: ${extensionsDir}`);

    // Create a unique ID for this test run to cryptographically bind the window
    const testRunId = crypto.randomUUID();
    testLogger.debug(`[Launch] Assigning Test Run ID: ${testRunId}`);

    // Sanitize environment to prevent leaking into the host VS Code instance
    const env = sanitizeEnv(process.env);

    // Hard-fail guard: if we expected headless isolation (xvfb-run), refuse to run
    // if anything indicates we're rendering on the host desktop display/session.
    const expectHeadless = env.EXOSUIT_EXPECT_HEADLESS === "1";
    if (expectHeadless) {
      const hostDisplay = env.EXOSUIT_HOST_DISPLAY;
      const hostWaylandDisplay = env.EXOSUIT_HOST_WAYLAND_DISPLAY;
      const hostSessionType = env.EXOSUIT_HOST_XDG_SESSION_TYPE;

      if (hostDisplay && env.DISPLAY === hostDisplay) {
        throw new Error(
          `[FATAL] Headless expected but DISPLAY matches host display (${hostDisplay}). Refusing to run.`,
        );
      }

      // Defensive default: if we expect headless, :0 / :1 almost certainly means the host desktop.
      // xvfb-run --auto-servernum does not use these by default.
      if (env.DISPLAY === ":0" || env.DISPLAY === ":1") {
        throw new Error(
          `[FATAL] Headless expected but DISPLAY=${env.DISPLAY}. Refusing to run.`,
        );
      }

      if (env.WAYLAND_DISPLAY) {
        throw new Error(
          `[FATAL] Headless expected but WAYLAND_DISPLAY is set (${env.WAYLAND_DISPLAY}). Refusing to run.`,
        );
      }
      if (env.XDG_SESSION_TYPE === "wayland") {
        throw new Error(
          `[FATAL] Headless expected but XDG_SESSION_TYPE=wayland. Refusing to run.`,
        );
      }
      // If the runner captured host Wayland details, treat any appearance as escape.
      if (hostWaylandDisplay && env.WAYLAND_DISPLAY === hostWaylandDisplay) {
        throw new Error(
          `[FATAL] Headless expected but WAYLAND_DISPLAY matches host (${hostWaylandDisplay}). Refusing to run.`,
        );
      }
      if (hostSessionType === "wayland" && env.XDG_SESSION_TYPE === "wayland") {
        throw new Error(
          `[FATAL] Headless expected but XDG_SESSION_TYPE matches host (wayland). Refusing to run.`,
        );
      }
    }

    testLogger.debug(`[Launch] DISPLAY environment variable: ${env.DISPLAY}`);
    testLogger.debug(
      `[Launch] WAYLAND_DISPLAY environment variable: ${env.WAYLAND_DISPLAY}`,
    );

    let app: ElectronApplication;
    try {
      app = await electron.launch({
        executablePath: executablePath,
        args: [
          `--user-data-dir=${userDataDir}`,
          `--extensions-dir=${extensionsDir}`,
          ...(process.platform === "linux" && expectHeadless
            ? ["--ozone-platform=x11"]
            : []),
          "--disable-workspace-trust",
          "--extensionDevelopmentPath=" + extensionPath,
          holodeckPath, // Open the Holodeck
          "--new-window", // Ensure new window
          "--skip-welcome", // Skip welcome page
          "--skip-release-notes",
          "--no-sandbox", // Required for some environments
          "--disable-gpu", // Often helps
        ],
        env: {
          ...env,
          NODE_ENV: "development",
          EXOSUIT_TEST_ID: testRunId, // The Golden Ticket
          EXO_BIN: path.resolve(__dirname, "../../../../target/debug/exo"),
        },
      });
    } catch (e) {
      testLogger.error(
        `[Launch] Failed to launch VS Code: ${(e as Error).message}`,
      );
      dumpTestLogs("Launch failure");
      throw e;
    }

    // Scientific debugging: attribute the *actual* spawned Electron process
    // to a concrete PID and capture the critical environment variables.
    const electronProcess = app.process();
    const electronPid = electronProcess?.pid;
    if (electronPid) {
      testLogger.debug(`[Launch] Electron PID: ${electronPid}`);
      try {
        const raw = await fs.promises.readFile(
          `/proc/${electronPid}/environ`,
          "utf8",
        );
        const lines = raw
          .split("\u0000")
          .filter(Boolean)
          .filter(
            (l) =>
              l.startsWith("DISPLAY=") ||
              l.startsWith("WAYLAND_DISPLAY=") ||
              l.startsWith("XDG_SESSION_TYPE="),
          );
        testLogger.debug(`[Launch] Electron environ:\n${lines.join("\n")}`);
      } catch (e) {
        testLogger.warn(
          `[Launch] Failed to read /proc/${electronPid}/environ: ${(e as Error).message}`,
        );
      }

      // Extra attribution: prove whether the Electron PID is actually connected
      // to the X11 socket for the DISPLAY it claims.
      const displayNum = parseDisplayNumber(env.DISPLAY);
      if (displayNum) {
        try {
          const x11SockPath = `/tmp/.X11-unix/X${displayNum}`;
          const x11SockExists = await fs.promises
            .stat(x11SockPath)
            .then(() => true)
            .catch(() => false);
          const inodes = await getX11SocketInodes(displayNum);
          testLogger.debug(
            `[Launch] X11 probe for DISPLAY=:${displayNum}: socketExists=${x11SockExists} listenerInodes=[${inodes.join(
              ", ",
            )}]`,
          );
        } catch (e) {
          testLogger.warn(`[Launch] X11 probe failed: ${(e as Error).message}`);
        }
      } else {
        testLogger.warn(
          `[Launch] Could not parse DISPLAY for X11 probe: ${env.DISPLAY}`,
        );
      }

      // Helpful for attribution if something looks off.
      try {
        const cmdline = await fs.promises.readFile(
          `/proc/${electronPid}/cmdline`,
          "utf8",
        );
        testLogger.debug(
          `[Launch] Electron cmdline:\n${cmdline
            .split("\u0000")
            .filter(Boolean)
            .join(" ")}`,
        );
      } catch {
        // ignore
      }

      // Process tree attribution: if a *different* process is creating visible windows
      // (e.g. a child spawned with DISPLAY=:0), this will catch it.
      try {
        const seen = new Set<number>();
        const queue: number[] = [electronPid];
        const rows: Array<{ pid: number; cmd: string | null; env: string[] }> =
          [];

        while (queue.length && rows.length < 50) {
          const pid = queue.shift()!;
          if (seen.has(pid)) continue;
          seen.add(pid);

          const [cmd, envLines, children] = await Promise.all([
            safeReadCmdline(pid),
            safeReadDisplayEnv(pid),
            getChildPids(pid),
          ]);
          rows.push({ pid, cmd, env: envLines });
          for (const child of children) queue.push(child);
        }

        testLogger.debug("[Launch] Process tree DISPLAY snapshot:");
        for (const row of rows) {
          const envSummary = row.env.length
            ? row.env.join(" ")
            : "(no DISPLAY/WAYLAND/XDG_SESSION_TYPE found)";
          testLogger.debug(
            `  pid=${row.pid} env={${envSummary}} cmd=${
              row.cmd ?? "(unavailable)"
            }`,
          );
        }

        if (expectHeadless) {
          const hostDisplay = env.EXOSUIT_HOST_DISPLAY;
          const escaped = rows.filter((row) => {
            if (!row.env.length) return false;
            if (hostDisplay && row.env.includes(`DISPLAY=${hostDisplay}`))
              return true;
            if (row.env.some((l) => l.startsWith("WAYLAND_DISPLAY=")))
              return true;
            if (row.env.includes("XDG_SESSION_TYPE=wayland")) return true;
            return false;
          });

          if (escaped.length) {
            try {
              await app.close();
            } catch {
              // ignore
            }
            const details = escaped
              .map(
                (row) =>
                  `pid=${row.pid} env={${row.env.join(" ")}} cmd=${
                    row.cmd ?? "(unavailable)"
                  }`,
              )
              .join("\n");
            throw new Error(
              `[FATAL] Headless expected but a spawned process appears to be using the host desktop display/session.\n${details}`,
            );
          }
        }
      } catch (e) {
        testLogger.warn(
          `[Launch] Failed to snapshot process tree: ${(e as Error).message}`,
        );
      }
    } else {
      testLogger.warn(
        "[Launch] Could not determine Electron PID from Playwright",
      );
    }

    // SENTINEL: Verify we are strictly isolated
    // If we somehow connected to the user's main instance, the userDataDir will be different.
    const actualUserDataDir = await app.evaluate(async ({ app }) => {
      return app.getPath("userData");
    });

    if (actualUserDataDir !== userDataDir) {
      testLogger.error(
        `[FATAL] VS Code launched with wrong user data dir! Isolation Failed.\nExpected: ${userDataDir}\nGot: ${actualUserDataDir}`,
      );
      await app.close();
      dumpTestLogs("Isolation failure");
      throw new Error(
        "VS Code Isolation Failed: Connected to wrong instance (likely user's main window).",
      );
    }
    testLogger.debug(
      `[Isolation Verified] User Data Dir: ${actualUserDataDir}`,
    );

    await use(app);
    try {
      await app.close();
    } catch (e) {
      testLogger.warn(
        `Error closing app, attempting to kill process: ${(e as Error).message}`,
      );
      try {
        app.process().kill();
      } catch (killError) {
        testLogger.warn(
          `Failed to kill process: ${(killError as Error).message}`,
        );
      }
    }

    // Cleanup dirs
    try {
      await fs.promises.rm(userDataDir, { recursive: true, force: true });
      await fs.promises.rm(extensionsDir, { recursive: true, force: true });
    } catch (e) {
      testLogger.warn(`Failed to cleanup dirs: ${(e as Error).message}`);
    }
  },
  page: async ({ app }, use, _testInfo) => {
    // Helper to check if a page is the main workbench window
    const isWorkbench = async (p: Page) => {
      try {
        const url = p.url();
        // 1. Check URL (Must be workbench)
        if (!url.includes("workbench.html")) {
          return false;
        }

        testLogger.debug(`[Window Check] Found Workbench URL: ${url}`);

        // 2. Wait for workbench DOM (Increased timeout)
        try {
          await p.waitForSelector(".monaco-workbench", { timeout: 15000 });
        } catch (e) {
          testLogger.warn(
            `[Window Check] Timed out waiting for .monaco-workbench`,
          );
          return false;
        }

        // 3. The Golden Ticket Check (Environment Variable)
        // Note: 'process' is often not defined in the renderer (sandbox mode).
        // We check the window title for the Holodeck ID as a proxy.
        const title = await p.title();
        if (title.includes("exosuit-holodeck-")) {
          testLogger.debug(`[Window Check] Found Holodeck Title: ${title}`);
          return true;
        }

        const windowEnvId = await p
          .evaluate(() =>
            typeof process !== "undefined" ? process.env.EXOSUIT_TEST_ID : null,
          )
          .catch(() => null);

        if (windowEnvId) {
          testLogger.debug(
            `[Window Check] Found Golden Ticket: ${windowEnvId}`,
          );
          return true;
        }

        // Fallback: If we have the right URL and DOM, and we verified the UserDataDir at launch,
        // we can be confident this is the right window.
        // BUT, we should be suspicious if the title doesn't match.
        testLogger.warn(
          `[Window Check] Accepted based on URL/DOM but missing Holodeck title. Title="${title}"`,
        );
        testLogger.debug(`[Window Check] ACCEPTED based on URL and DOM.`);
        return true;
      } catch (e) {
        testLogger.warn(
          `[Window Check] Error checking window: ${(e as Error).message}`,
        );
        return false;
      }
    };

    // Check existing windows first
    let page: Page | undefined;
    const windows = app.windows();
    for (const win of windows) {
      if (await isWorkbench(win)) {
        page = win;
        break;
      }
    }

    // If not found, wait for new windows
    if (!page) {
      try {
        const first = await app.firstWindow();
        if (await isWorkbench(first)) {
          page = first;
        } else {
          // Wait for a second window if the first one wasn't it
          testLogger.debug(
            "First window was not workbench, waiting for next window...",
          );
          page = await app.waitForEvent("window");
        }
      } catch (e) {
        testLogger.warn(
          `Error getting first window, waiting for window event...: ${(e as Error).message}`,
        );
        page = await app.waitForEvent("window");
      }
    }

    // Final check and setup
    if (page) {
      await page.waitForLoadState("domcontentloaded");
      // Ensure we really have the workbench
      await page.waitForSelector(".monaco-workbench", { timeout: 30000 });
      await page.setViewportSize({ width: 1280, height: 800 });

      await use(page);

      // Post-Mortem Analysis (Guarded by Construction)
      // If the test failed, we automatically dump the state of all frames to help diagnosis.
      // This implements Axiom 19's "Observability First" principle automatically.
      // Note: This is now handled by the 'exo' fixture which has access to the helper methods.
    } else {
      throw new Error("Could not find VS Code workbench window");
    }
  },
  dashboard: async ({ page }, use) => {
    // 1. Ensure Dashboard is Open
    await page.keyboard.press("F1");
    await page.waitForSelector(".quick-input-widget");
    await page.keyboard.type("View: Focus Dashboard (V2)");
    await page.waitForTimeout(500);
    await page.keyboard.press("Enter");

    // 2. Find the Dashboard Frame
    // Retry logic to find the frame by title AND content to ensure it's the right one
    let dashboardFrame: Frame | null = null;
    const maxRetries = 30;

    for (let i = 0; i < maxRetries; i++) {
      for (const frame of page.frames()) {
        try {
          const title = await frame.title();
          // Strict check on title
          if (title === "Exosuit Dashboard") {
            // DOUBLE CHECK: Ensure this is actually our dashboard by checking content
            // This prevents attaching to a zombie frame or a different webview with the same title
            const hasApp = await frame.$("#app");
            if (hasApp) {
              dashboardFrame = frame;
              break;
            }
          }
        } catch (e) {
          // Frame might be detached or inaccessible
        }
      }
      if (dashboardFrame) break;
      await page.waitForTimeout(500);
    }

    if (!dashboardFrame) {
      throw new Error(
        "Could not find Dashboard Webview Frame (Title: 'Exosuit Dashboard' + Selector: '#app') after " +
          maxRetries +
          " retries",
      );
    }

    await use(dashboardFrame);
  },
});
