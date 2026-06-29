import fs from "node:fs";
import path from "node:path";

function ensureDir(dir) {
  fs.mkdirSync(dir, { recursive: true });
}

function copyIfExists(src, dst) {
  if (!fs.existsSync(src)) return false;
  fs.copyFileSync(src, dst);
  return true;
}

function stageOneModule({ name, srcDir, outDir }) {
  // Copy WASM + d.ts artifacts if present.
  copyIfExists(
    path.join(srcDir, `${name}_bg.wasm`),
    path.join(outDir, `${name}_bg.wasm`)
  );
  copyIfExists(
    path.join(srcDir, `${name}_bg.wasm.d.ts`),
    path.join(outDir, `${name}_bg.wasm.d.ts`)
  );
  copyIfExists(
    path.join(srcDir, `${name}.d.ts`),
    path.join(outDir, `${name}.d.ts`)
  );

  // Prefer the explicitly-generated CJS wrapper. Otherwise, fall back to the JS wrapper.
  const cjsOut = path.join(outDir, `${name}.cjs`);
  if (!copyIfExists(path.join(srcDir, `${name}.cjs`), cjsOut)) {
    // wasm-bindgen often emits `${name}.js`. We ship it as CJS for Node require().
    copyIfExists(path.join(srcDir, `${name}.js`), cjsOut);
  }
}

function main() {
  const root = process.cwd();
  const srcDir = path.join(root, "src", "wasm");
  const outDir = path.join(root, "out", "wasm");

  ensureDir(outDir);

  // Keep the package.json around for debugging/inspection.
  copyIfExists(
    path.join(srcDir, "package.json"),
    path.join(outDir, "package.json")
  );

  stageOneModule({ name: "exosuit_reactivity", srcDir, outDir });
  stageOneModule({ name: "exosuit_file_refs", srcDir, outDir });
}

main();
