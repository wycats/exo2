/**
 * Stub for ../wasm/exosuit_reactivity.js in unit tests.
 *
 * The actual WASM module is loaded at runtime via dynamic import in
 * ReactivityService.ts. For unit tests that don't exercise WASM
 * functionality, we provide this stub that exports empty functions.
 */

export default function init(_wasmUrl?: URL | string): Promise<void> {
  return Promise.resolve();
}

export function create_engine(): object {
  throw new Error("WASM stub: create_engine not available in tests");
}
