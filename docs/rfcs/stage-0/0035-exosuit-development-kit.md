<!-- exo:35 ulid:01kg5m2xmq7pq9e967ad8zxc8p -->

# RFC 35: Exosuit Development Kit (EDK)

- **Supersedes**: RFC 10061



# RFC 0035: Exosuit Development Kit (EDK)

- **Status**: Stage 0 (Draft)
- **Feature**: Tooling / Architecture
- **Related**: RFC 0127 (Rigorous Rust Infrastructure)

## Summary

Extract the "Exosuit Way" of building VS Code extensions—including the build system, test harness, webview RPC bridge, and UI scaffolding—into a reusable library or framework ("The Exosuit Development Kit").

## Motivation

The current Exosuit extension architecture is robust, featuring:
-   **Hybrid Build System**: Vite for Webviews + esbuild for Extension Host.
-   **Rigorous Testing**: Playwright for E2E + Vitest for Unit tests.
-   **Type-Safe RPC**: A bridge between the Extension Host and Webviews.
-   **Reactive Bindings**: Signals for VS Code APIs.

However, this setup is tightly coupled to the `exosuit-vscode` package. Reusing this architecture for other projects currently requires copying a massive amount of boilerplate, which is "daunting" and unmaintainable.

We want to commoditize this setup so that creating a new "Exosuit-style" extension is as simple as `npm init @exosuit/extension`.

## Design

### 1. The `exo-kit` Package

We will create a new workspace package: `packages/exo-kit` (or `packages/exosuit-devkit`).

This package will export:

#### A. The Build Chain
-   Shared `vite.config.ts` and `esbuild` scripts.
-   Standardized `package.json` scripts for `compile`, `watch`, `package`.

#### B. The Runtime Bridge
-   **`WebviewProvider`**: A generic abstract class that handles the boilerplate of registering a webview, handling message passing, and state restoration.
-   **`RpcBridge`**: The type-safe communication layer.
-   **Framework Agnostic Core**: The core protocol is framework-agnostic. It leans on the bindings we already want to build for the reactivity system.
-   **Svelte Premium Support**: While agnostic, we provide "Premium" support for Svelte. This means a "batteries-included" setup (components, stores, bindings) that just works out of the box.

#### C. The Test Harness
-   A wrapper around Playwright that provides a pre-configured VS Code instance.
-   Helpers for `openEditor`, `executeCommand`, `assertNotification`.

### 2. Refactoring `exosuit-vscode`

The main `exosuit-vscode` package will become a *consumer* of `exo-kit`.

**Before:**
`exosuit-vscode` contains `scripts/build.js`, `src/utils/WebviewBase.ts`, `playwright.config.ts`.

**After:**
`exosuit-vscode` imports `WebviewBase` from `@exosuit/kit`.
`exosuit-vscode`'s `package.json` scripts call `exo-kit build`.

### 3. Independence from Core Logic

The Kit must **not** depend on `exosuit-core` (the Rust/WASM logic) or the specific "Agent" business logic. It is purely for the *container* (VS Code Extension).

However, it *should* integrate well with them if present.

### 4. Updatability & Lifecycle

-   **Non-Negotiable Updatability**: The EDK is designed to be updated. We minimize generated code.
-   **Semver Strictness**: We follow a strict "Train Model" (see RFC 0037).
-   **Codemods**: We prioritize automated migration tools for any breaking changes or deprecations.

## Strategy: "Extraction by Refactoring"

We will not build a separate repo yet. We will build it *in situ* within the monorepo.

1.  **Identify**: Tag the "Glue Code" in `exosuit-vscode`.
2.  **Move**: Shift it to `packages/exo-kit`.
3.  **Consume**: Update `exosuit-vscode` to use the new package.
4.  **Verify**: Ensure `exosuit-vscode` still builds and passes tests.

## Unresolved Questions

-   **Distribution**: Do we publish this to NPM? Or just use it as a template? (Decision: Start with monorepo workspace, decide on publishing later).
-   **Reactivity**: Does the Kit include the `exosuit-reactivity` engine?
    -   **Decision**: **YES**. The Reactivity Engine is core to the "Exosuit Way". It provides the "Glitch-Free" state management that makes the EDK powerful. It is lightweight enough (WASM) to be included by default.

## Vision

See [docs/vision-edk.md](../../vision-edk.md) for the aspirational vision of the EDK.

## Future Possibilities

-   `exo new extension <name>`: A CLI command to scaffold a new project using the Kit.

