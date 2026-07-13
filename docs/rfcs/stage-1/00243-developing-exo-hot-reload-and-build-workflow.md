<!-- exo:243 ulid:01kmzxeffvdgyy4ajxjj1wy9c5 -->

# RFC 243: Developing Exo: Hot Reload and Build Workflow


# RFC 00243: Developing Exo: Hot Reload and Build Workflow {#00243}

## Summary

This RFC documents the current development workflow for the Exosuit VS Code extension and proposes improvements to reduce iteration time. The primary goals are:

1. Enable webview hot module replacement (HMR) during development
2. Explore extension host code refresh without window reload
3. Accelerate incremental builds

## Current Architecture

### CLI and Daemon Freshness

RFC 10179 defines the current development-binary contract. A workspace selects
its local Exo toolchain through `[dev].binary_dir` in `exosuit.toml`. Rust
entry points re-exec to that build when necessary, while the VS Code extension
resolves the same workspace-local binary directly.

The active machine channel is daemon-backed. Before it reuses a socket, the
extension invokes Rust `daemon ensure`, which compares the daemon's recorded
workspace and executable identity with the selected binary and probes the exact
instance. A stale daemon is replaced, and the extension reconnects its socket
lanes and invalidates cached state. CLI changes therefore become available on
the next ensured request after rebuilding the configured workspace binary;
they do not require a VS Code window reload.

### Extension Build Pipeline

The extension uses a two-stage build process:

1. **Webview build** (vite.config.mts): Vite + Svelte for webview assets → `out/webview/`
2. **Extension bundle** (vite.extension.config.mts): Vite for extension host code → `out/extension.js`

Build scripts:

- `build`: Full build (typecheck + webview + bundle)
- `bundle`: Extension host bundle only
- `build:webview`: Webview assets only
- `typecheck`: `tsc -b` + test bundle

### Full Dogfood Workflow

The `dogfood-extension.sh` script runs:

1. `pnpm install --frozen-lockfile`
2. `./scripts/build-wasm.sh` (WASM bindings)
3. `cargo run -p exo -- json artifact` (command spec)
4. `node scripts/sync-lm-tools.ts --add` (sync LM tools to package.json)
5. `pnpm -r run build` (workspace deps)
6. `pnpm -C packages/exosuit-vscode run build` (extension)
7. `./scripts/dev/install-extension.sh` (package + install VSIX)

**Current pain**: Every TypeScript/Svelte change requires VSIX reinstall + window reload.

### What Requires Window Reload (VS Code Limitation)

Changes to `package.json` contribution points intrinsically require window reload:

- Commands
- Views
- Settings
- Language model tools
- Activation events

This is a VS Code platform limitation — contribution points are read once at extension activation.

## Pain Points

1. **Webview iteration is slow**: Svelte component changes require full rebuild + VSIX reinstall + window reload
2. **Extension host iteration is slow**: TypeScript changes require rebuild + VSIX reinstall + window reload
3. **Build is slower than necessary**: `tsc -b` is used for typechecking, but Vite/esbuild could provide faster incremental builds

## Proposed Improvements

### Phase 1: Webview Dev Server Mode (Highest Impact)

Webviews are just HTML/JS/CSS loaded via `webview.html`. During development, we can point at a Vite dev server instead of bundled assets.

**Implementation**:

1. Add `vite dev` mode for webview development
2. Detect dev mode via environment variable or setting
3. In dev mode, webview HTML loads from `http://localhost:5173` instead of `out/webview/`
4. Vite HMR provides instant updates for Svelte components

**Benefits**:

- Instant hot reload for all Svelte component changes
- No rebuild, no VSIX reinstall, no window reload
- This is where most UI iteration happens

**Considerations**:

- Need to handle CSP (Content Security Policy) for localhost
- Dev server must be running before opening webviews
- Production builds remain unchanged

### Phase 2: Extension Host Code Refresh

Some extension host components could potentially be refreshed without a full window reload:

| Component       | Current State          | Refresh Potential           |
| --------------- | ---------------------- | --------------------------- |
| Tree providers  | Registered once        | Could dispose + re-register |
| Machine channel | Has restart capability | Already supports restart    |
| LM tools        | Registered once        | Handlers could be swapped   |
| Commands        | Registered once        | Handlers could be swapped   |

**Approach**:

1. Implement a "refresh extension" command that:
   - Disposes tree providers and re-registers them
   - Restarts machine channel (already supported)
   - Re-imports and re-registers command/tool handlers
2. Use dynamic imports to reload changed modules
3. Maintain state across refresh where possible

**Challenges**:

- Module caching in Node.js ESM
- State management across refreshes
- Not all components can be cleanly disposed

### Phase 3: Faster Incremental Builds

Currently using `tsc -b` for typechecking. Could improve with:

1. **Vite/esbuild for extension host**: Already using Vite for bundling; could use it for watch mode too
2. **Parallel builds**: Webview and extension host builds are independent
3. **Skip typecheck in dev**: Use editor for type errors, skip `tsc -b` during rapid iteration

## Implementation Priority

1. **Webview HMR** (Phase 1) — Biggest win, most iteration happens in Svelte
2. **Faster builds** (Phase 3) — Quick wins with existing tooling
3. **Extension host refresh** (Phase 2) — More complex, lower frequency need

## Open Questions

1. Should dev server mode be opt-in (setting) or automatic (detect running server)?
2. How to handle webview state preservation across HMR updates?
3. Is extension host refresh worth the complexity given contribution point limitations?

## References

- `packages/exosuit-vscode/src/machine-channel/DaemonChannelServer.ts` — Daemon lifecycle and reconnect behavior
- `packages/exosuit-vscode/src/exoBin.ts` — Binary resolution logic
- `packages/exosuit-vscode/vite.config.mts` — Webview build config
- `packages/exosuit-vscode/vite.extension.config.mts` — Extension bundle config
- `scripts/dev/dogfood-extension.sh` — Full build script
