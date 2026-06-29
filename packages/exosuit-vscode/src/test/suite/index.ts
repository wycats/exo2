import * as path from "path";
import glob from "glob";
import { fileURLToPath } from "url";
import * as fs from "node:fs";
import * as os from "node:os";
import { installHarnessGlobals, runHarness } from "./harness.js";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const activationTracePath = path.join(
  os.tmpdir(),
  "exosuit-vscode-test-activation.log",
);
const trace = (msg: string) => {
  if (process.env.EXOSUIT_TEST_WRAPPER !== "true") {
    return;
  }
  try {
    fs.appendFileSync(
      activationTracePath,
      `${new Date().toISOString()} [TestHarness] ${msg}\n`,
    );
  } catch {
    // ignore
  }
};

const writeStderr = (msg: string) => {
  process.stderr.write(`${msg}\n`);
};

export function run(): Promise<void> {
  trace(
    `run() entered; EXOSUIT_TEST_GLOB=${
      process.env.EXOSUIT_TEST_GLOB ?? "<unset>"
    }`,
  );
  const preimportModule = process.env.EXOSUIT_TEST_PREIMPORT_MODULE;
  const testsRoot = path.resolve(__dirname, "..");

  // Scientific-debugging hook: allow narrowing which tests are loaded.
  // - EXOSUIT_TEST_GLOB=__none__ loads no tests (activation-only).
  // - EXOSUIT_TEST_GLOB=**/WorkspaceCache.test.js loads only matching tests.
  const requestedGlob = process.env.EXOSUIT_TEST_GLOB;
  const testGlob =
    requestedGlob && requestedGlob.trim().length > 0
      ? requestedGlob.trim()
      : "**/**.test.js";

  return (async () => {
    installHarnessGlobals();

    // Scientific-debugging hook: optionally import a module before any test loading
    // (useful for isolating hangs during module resolution/evaluation).
    if (preimportModule && preimportModule.trim().length > 0) {
      const spec = preimportModule.trim();
      trace(`EXOSUIT_TEST_PREIMPORT_MODULE set; preimport begin: ${spec}`);
      await import(spec);
      trace(`preimport end: ${spec}`);
    }

    if (testGlob === "__none__") {
      trace("testGlob=__none__; no tests loaded");
    } else {
      const files = await new Promise<string[]>((resolve, reject) => {
        glob(testGlob, { cwd: testsRoot }, (err, found) => {
          if (err) {
            reject(err);
          } else {
            resolve(found);
          }
        });
      });

      trace(`glob(${testGlob}) returned ${files.length} files`);
      trace(`files=[${files.join(", ")}]`);

      for (const file of files) {
        const resolved = path.resolve(testsRoot, file);
        trace(`import begin: ${resolved}`);
        try {
          await import(resolved);
        } catch (err) {
          writeStderr(
            err instanceof Error ? (err.stack ?? err.message) : String(err),
          );
          throw err;
        }
        trace(`import end: ${resolved}`);
      }
    }

    const summary = await runHarness();
    trace(
      `runHarness complete; passed=${summary.passed} failed=${summary.failed} skipped=${summary.skipped}`,
    );
    if (summary.failed > 0) {
      throw new Error(`${summary.failed} tests failed.`);
    }
  })();
}
