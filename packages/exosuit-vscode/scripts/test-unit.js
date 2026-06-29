import { spawn, execSync } from "child_process";
import * as fs from "node:fs";
import path from "path";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

function resolveXvfbRun() {
  const envOverride =
    process.env.EXOSUIT_XVFB_RUN ?? process.env.XVFB_RUN ?? "";
  if (envOverride.trim().length > 0 && fs.existsSync(envOverride)) {
    return envOverride.trim();
  }

  try {
    const resolved = execSync("command -v xvfb-run", {
      stdio: ["ignore", "pipe", "ignore"],
    })
      .toString()
      .trim();
    if (resolved.length > 0) {
      return resolved;
    }
  } catch {
    // ignore
  }

  const fallback = "/usr/bin/xvfb-run";
  if (fs.existsSync(fallback)) {
    return fallback;
  }

  return null;
}

async function run() {
  try {
    const args = process.argv.slice(2);

    // Check for xvfb-run on Linux for headless execution
    const isLinux = process.platform === "linux";
    const isHeaded = args.includes("--headed");

    let command = "node";
    let scriptPath = path.resolve(__dirname, "../out/test/runTest.js");
    let commandArgs = [scriptPath, ...args];

    const env = { ...process.env, EXOSUIT_TEST_WRAPPER: "true" };

    if (isLinux && !isHeaded) {
      const xvfbRun = resolveXvfbRun();
      if (xvfbRun) {
        // Prevent Wayland leakage; force X11/Xvfb
        delete env.WAYLAND_DISPLAY;
        delete env.DISPLAY;
        if (env.XDG_SESSION_TYPE === "wayland") {
          delete env.XDG_SESSION_TYPE;
        }

        command = xvfbRun;
        commandArgs = [
          "--auto-servernum",
          "--server-args=-screen 0 1280x1024x24 -nolisten tcp",
          "--",
          "node",
          scriptPath,
          ...args,
        ];
      }
    }

    const testProcess = spawn(command, commandArgs, {
      stdio: "inherit",
      shell: false,
      env,
    });

    testProcess.on("close", (code) => {
      process.exit(code ?? 0);
    });
  } catch {
    process.exit(1);
  }
}

run();
