import esbuild from "esbuild";
import glob from "glob";
import path from "path";
import fs from "fs";
import { fileURLToPath } from "url";

// Why esbuild instead of Vite here?
// - This step is strictly for bundling Node-side *tests* quickly into `out/test`.
// - The extension bundle uses Vite/Rollup to handle VS Code externals and assets.
// - Keeping tests on esbuild avoids extra Vite/Rollup config just to bundle many
//   entry points, and keeps `pnpm run typecheck` fast.

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// Plugin to resolve .js imports to .ts files for workspace packages
const resolveJsToTs = {
  name: "resolve-js-to-ts",
  setup(build) {
    build.onResolve({ filter: /\.js$/ }, (args) => {
      // Check if the import is relative and coming from a workspace package
      if (args.kind === "import-statement" && args.path.startsWith(".")) {
        const absPath = path.resolve(path.dirname(args.importer), args.path);
        const tsPath = absPath.replace(/\.js$/, ".ts");
        if (fs.existsSync(tsPath)) {
          return { path: tsPath };
        }
      }
      return null;
    });
  },
};

async function build() {
  const tests = glob.sync("src/test/**/*.ts");

  // Avoid stale compiled tests lingering in out/test when files are deleted/renamed.
  fs.rmSync("out/test", { recursive: true, force: true });

  try {
    await esbuild.build({
      entryPoints: tests,
      outdir: "out/test",
      bundle: true,
      external: ["vscode", "@vscode/test-electron", "glob"],
      format: "esm",
      platform: "node",
      sourcemap: true,
      plugins: [resolveJsToTs],
      logLevel: "info",
    });
  } catch (e) {
    const message = e instanceof Error ? (e.stack ?? e.message) : String(e);
    process.stderr.write(`Build failed: ${message}\n`);
    process.exit(1);
  }
}

build();
