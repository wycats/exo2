#!/bin/bash
# Open VS Code with the extension loaded from source.
#
# Usage:
#   ./scripts/dev/open-dev-host.sh [workspace-path]
#
# This starts VS Code with --extensionDevelopmentPath pointing at the
# extension package. The extension loads from out/ (the Vite bundle output)
# instead of from the installed VSIX.
#
# Workflow:
#   1. Run this script to open the dev window
#   2. Make TS changes
#   3. Run "Build Extension (dev)" task (or `exo run build-ext-dev`)
#   4. Cmd+Shift+P → "Developer: Restart Extension Host"
#   5. Extension reloads with new code — no VSIX, no window reload
#
# Runtime mode: dev host. This never packages or installs a VSIX. Use
# cargo dogfood-exo for installed-VSIX dogfood mode.

set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
EXTENSION_DIR="$ROOT_DIR/packages/exosuit-vscode"
WORKSPACE="${1:-$ROOT_DIR}"

if [[ ! -f "$EXTENSION_DIR/out/extension.js" ]]; then
  echo "Error: dev-host bundle missing: $EXTENSION_DIR/out/extension.js" >&2
  echo "Run: pnpm -C packages/exosuit-vscode run build:dev-host" >&2
  exit 1
fi

echo "=== Dev-host mode ==="
echo "Opening VS Code with --extensionDevelopmentPath=$EXTENSION_DIR"
echo "No VSIX will be installed. After rebuilding, run 'Developer: Restart Extension Host'."

MANIFEST="$EXTENSION_DIR/out/dev-host-bundle.json"
if [[ ! -f "$MANIFEST" ]]; then
  echo "Error: dev-host bundle manifest missing: $MANIFEST" >&2
  echo "Run: pnpm -C packages/exosuit-vscode run build:dev-host" >&2
  exit 1
fi

node -e '
  const fs = require("node:fs");
  const manifestPath = process.argv[1];
  const bundlePath = process.argv[2];
  const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  if (manifest.kind !== "exosuit-vscode-extension-bundle") {
    throw new Error(`unexpected manifest kind: ${manifest.kind}`);
  }
  if (manifest.daemonRuntimePaths !== "project-resolve") {
    throw new Error(`unexpected daemon runtime path mode: ${manifest.daemonRuntimePaths}`);
  }
  const bundleStat = fs.statSync(bundlePath);
  if (manifest.bundleMtimeMs !== bundleStat.mtimeMs) {
    throw new Error("bundle manifest is stale; bundle mtime does not match");
  }
' "$MANIFEST" "$EXTENSION_DIR/out/extension.js" || {
  echo "Error: dev-host bundle manifest is stale or invalid." >&2
  echo "Run: pnpm -C packages/exosuit-vscode run build:dev-host" >&2
  exit 1
}

# Detect VS Code binary
if command -v code >/dev/null 2>&1; then
  VSCODE_BIN="code"
elif command -v code-insiders >/dev/null 2>&1; then
  VSCODE_BIN="code-insiders"
else
  echo "Error: neither 'code' nor 'code-insiders' found on PATH" >&2
  exit 1
fi

exec "$VSCODE_BIN" --extensionDevelopmentPath="$EXTENSION_DIR" "$WORKSPACE"
