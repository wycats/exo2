import * as path from "path";
import { fileURLToPath } from "url";
import { env } from "node:process";

import { runTests } from "@vscode/test-electron";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

function writeStderr(message: string) {
  process.stderr.write(`${message}\n`);
}

async function main() {
  // Safety Check: Ensure we are running via the wrapper script
  if (env.EXOSUIT_TEST_WRAPPER !== "true") {
    writeStderr("ERROR: Do not run this script directly.");
    writeStderr(
      "Please use 'pnpm test' or 'node scripts/test-unit.js' to ensure the correct environment (xvfb, etc.).",
    );
    process.exit(1);
  }

  try {
    // The folder containing the Extension Manifest package.json
    // Passed to `--extensionDevelopmentPath`
    const extensionDevelopmentPath = path.resolve(__dirname, "../../");

    // The path to test runner
    // Passed to --extensionTestsPath
    const extensionTestsPath = path.resolve(__dirname, "./suite/index.js");

    // The workspace to open
    const launchArgs = [path.resolve(__dirname, "../../")];

    // On Fedora Atomic desktops (Bazzite, Silverblue), the Chromium sandbox
    // requires unprivileged user namespaces which may be restricted.
    // Disable the sandbox for test runs to avoid library loading failures.
    if (process.platform === "linux") {
      launchArgs.push("--no-sandbox", "--disable-gpu-sandbox");
    }

    // Headless execution is handled by xvfb-run in the wrapper script.
    // Do not pass --headless here; it conflicts with Xvfb isolation.

    // Download VS Code, unzip it and run the integration test
    await runTests({
      extensionDevelopmentPath,
      extensionTestsPath,
      launchArgs,
    });
  } catch (err) {
    writeStderr("Failed to run tests");
    writeStderr(
      err instanceof Error ? (err.stack ?? err.message) : String(err),
    );
    process.exit(1);
  }
}

main();
