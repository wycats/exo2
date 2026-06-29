<!-- exo:10131 ulid:01kmzxey2awhg17bm9dnxqcf7b -->


# RFC 10131: Optimistic UI Guard — Transient Signals and Echo Suppression

## Summary

This RFC defines the **Transient Signal** pattern for optimistic UI updates in the reactive architecture. When a mutation is initiated, the UI renders an optimistic prediction immediately. The prediction self-destructs when the canonical state converges (confirmed by content digest comparison), or on timeout.

The key mechanism is **echo suppression**: a pending-hash filter that distinguishes "echoes of our own writes" from "true external changes," preventing flicker during the write→confirm round-trip.

**Extends**: RFC 0026 (Validation-Based Reactive Architecture) — adds the write round-trip protocol that 0026's pull-based model doesn't address.

**Substrate**: Content digests from RFC 10165's revision store (or file hashes in the file-watching interim architecture) provide the convergence detection mechanism.

## Motivation

The reactive architecture (RFC 0026) defines a pull-based validation model: $UI = f(\text{State})$. This works well for reads — the UI revalidates when revisions change. But it says nothing about what happens _during_ a write:

1. **T0**: User initiates mutation. UI should update immediately (optimistic).
2. **T1**: Mutation reaches the store (SQLite, or file write in interim architecture).
3. **T2**: Reactive system detects the change (revision bump, or file watcher).
4. **T3**: UI re-reads canonical state.

Between T0 and T3, the UI has rendered an optimistic prediction. At T3, the canonical state arrives — but it might be an _echo_ of the optimistic write, not a new external change. Naively re-rendering causes flicker.

## Design

### The Transient Signal Algebra

In the pure pull-based model (RFC 0026):

$$UI = f(\text{CanonicalState})$$

With optimistic UI, we introduce a higher-precedence, ephemeral prediction:

$$UI = f(\text{TransientState} \ ?? \ \text{CanonicalState})$$

- **TransientState**: A local, ephemeral prediction of the future state. It has a strict TTL or destruction condition (Convergence).
- **CanonicalState**: The persistent truth (SQLite row revisions, or file hashes in the interim architecture).
- **Convergence**: The moment when $\text{Digest}(\text{CanonicalState}) \equiv \text{Digest}(\text{TransientState})$.

**Invariant**: The TransientState MUST self-destruct upon Convergence or Timeout. This ensures the system always eventually settles back to $UI = f(\text{CanonicalState})$.

### Echo Suppression Protocol

The guard tracks three pieces of state:

1. **canonicalDigest**: The content digest of the last confirmed canonical state.
2. **pendingDigests**: A set of digests for writes we initiated but haven't seen confirmed yet.
3. **latestOptimisticDigest**: The digest of the most recent optimistic prediction.

When the reactive system reports a state change (revision bump, file change, etc.):

1. Compute `readDigest` from the new canonical state.
2. **If `pendingDigests.has(readDigest)`**: This is an echo of our own write. **IGNORE** (don't re-render). If `readDigest === latestOptimisticDigest`, we have converged — clear `pendingDigests`.
3. **If `readDigest === canonicalDigest`**: Stale echo (no actual change). **IGNORE**.
4. **Else**: True external change. **ACCEPT** — update `canonicalDigest`, clear `pendingDigests`, re-render.

### Digest Substrate

The echo suppression protocol needs a content digest to compare. The substrate depends on the architecture:

- **File-watching interim** (current): SHA-256 of file content (already computed by the WASM reactivity engine).
- **Reactive-sqlite** (target): Content digests from `_rev` tables (BLAKE3 hashes computed by `content_hash()` in `exosuit-storage`).

The protocol is identical in both cases — only the digest source changes.

## Open Questions

- **Where does the guard live?** In the extension host (TypeScript), in the WASM engine (Rust), or split across both?
- **Timeout policy**: How long before a TransientState self-destructs if convergence never arrives?
- **Batch mutations**: If multiple mutations are in flight, the pending set may contain multiple digests. Does convergence require all of them to resolve, or just the latest?
