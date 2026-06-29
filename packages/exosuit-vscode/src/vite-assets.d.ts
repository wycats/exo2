// Intentionally left blank.
// We previously used Vite-style `?url` imports for `.wasm` assets, but the
// extension host now uses `new URL('..._bg.wasm', import.meta.url)` which works
// with both Vite and the esbuild-based test bundler.
