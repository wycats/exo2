#!/usr/bin/env node
import { createHash } from "node:crypto";
import { readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const extensionRoot = join(__dirname, "..");
const bundlePath = join(extensionRoot, "out", "extension.js");
const manifestPath = join(extensionRoot, "out", "dev-host-bundle.json");
const packageJsonPath = join(extensionRoot, "package.json");

const BUILD_STAMP_RE = /\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z/g;

function extractBuildStamp(bundleSource) {
  const buildStampLineMatch = bundleSource.match(
    /Build stamp:[^0-9]{0,120}(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z)/,
  );
  if (buildStampLineMatch) {
    return buildStampLineMatch[1];
  }

  const matches = bundleSource.match(BUILD_STAMP_RE) ?? [];
  if (matches.length === 0) {
    throw new Error(
      "Could not find Vite __BUILD_STAMP__ literal in extension bundle",
    );
  }

  const counts = new Map();
  for (const stamp of matches) {
    counts.set(stamp, (counts.get(stamp) ?? 0) + 1);
  }

  const ranked = [...counts.entries()].sort((a, b) => b[1] - a[1]);
  if (ranked.length === 1 || ranked[0][1] > ranked[1][1]) {
    return ranked[0][0];
  }

  throw new Error(
    `Could not identify a unique build stamp in extension bundle: ${[
      ...counts.keys(),
    ].join(", ")}`,
  );
}

const bundleStat = statSync(bundlePath);
const bundleBytes = readFileSync(bundlePath);
const bundleSource = bundleBytes.toString("utf-8");
const packageJson = JSON.parse(readFileSync(packageJsonPath, "utf-8"));
const extensionId = `${packageJson.publisher}.${packageJson.name}`;

const manifest = {
  schema: 2,
  kind: "exosuit-vscode-extension-bundle",
  extensionId,
  packageVersion: packageJson.version,
  daemonRuntimePaths: "project-resolve",
  bundle: "out/extension.js",
  buildStamp: extractBuildStamp(bundleSource),
  bundleSha256: createHash("sha256").update(bundleBytes).digest("hex"),
  bundleSizeBytes: bundleStat.size,
  bundleMtimeMs: bundleStat.mtimeMs,
  generatedAt: new Date().toISOString(),
};

writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
