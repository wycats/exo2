import { downloadAndUnzipVSCode } from "@vscode/test-electron";
import { spawn, execSync } from "child_process";
import * as path from "path";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

async function run() {
  try {
    // 0. Build the extension and webviews
    execSync("pnpm run build:webview && pnpm run bundle", {
      stdio: "inherit",
      cwd: path.resolve(__dirname, ".."),
    });

    // 1. Download/Resolve VS Code
    const executablePath = await downloadAndUnzipVSCode("stable");

    // 2. Run Playwright with the resolved path
    // We pass it as an environment variable so fixtures.ts can pick it up
    const env = {
      ...process.env,
      VSCODE_PATH: executablePath,
    };

    // Capture the *host* session display details before any headless sanitization.
    // We'll use these to hard-fail if the test ends up rendering on the host display.
    const hostDisplay = process.env.DISPLAY;
    const hostWaylandDisplay = process.env.WAYLAND_DISPLAY;
    const hostSessionType = process.env.XDG_SESSION_TYPE;

    // Pass any arguments from the command line to Playwright
    const args = ["playwright", "test", ...process.argv.slice(2)];

    // Check for xvfb-run on Linux for headless execution
    const isLinux = process.platform === "linux";
    const useHeadless = isLinux && !args.includes("--headed");
    let hasXvfb = false;

    if (useHeadless) {
      try {
        const xvfbPath = execSync("which xvfb-run").toString().trim();
        hasXvfb = true;
      } catch (e) {}
    }

    let command = "pnpm";
    let commandArgs = args;

    if (useHeadless && hasXvfb) {
      // Signal to the test fixtures that we *require* headless isolation.
      env.EXOSUIT_EXPECT_HEADLESS = "1";
      if (hostDisplay) env.EXOSUIT_HOST_DISPLAY = hostDisplay;
      if (hostWaylandDisplay)
        env.EXOSUIT_HOST_WAYLAND_DISPLAY = hostWaylandDisplay;
      if (hostSessionType) env.EXOSUIT_HOST_XDG_SESSION_TYPE = hostSessionType;

      // Sanitize environment to prevent Wayland leakage
      // This forces Electron/VS Code to use the X11 display provided by xvfb-run
      // instead of trying to connect to the host Wayland compositor.
      if (env.WAYLAND_DISPLAY) {
        delete env.WAYLAND_DISPLAY;
      }
      if (env.XDG_SESSION_TYPE === "wayland") {
        delete env.XDG_SESSION_TYPE;
      }
      // Ensure we don't pass the host DISPLAY to xvfb-run, just in case
      if (env.DISPLAY) {
        delete env.DISPLAY;
      }

      // Wrap the command in xvfb-run
      // We construct the full command string for spawn with shell: true
      command = "xvfb-run";
      commandArgs = [
        "--auto-servernum",
        "--server-args=-screen 0 1280x1024x24 -nolisten tcp",
        "--",
        "pnpm",
        ...args,
      ];
    }

    const playwright = spawn(command, commandArgs, {
      env,
      stdio: "inherit",
      shell: false,
    });

    playwright.on("close", (code) => {
      process.exit(code ?? 0);
    });
  } catch {
    process.exit(1);
  }
}

run();
