<!-- exo:70 ulid:01kg5kp2ed96ek64sa0wmeakmd -->

# RFC 70: Resource Protocol and Layered Architecture


# RFC 0070: Resource Protocol and Layered Architecture

## Summary

This RFC proposes a layered architecture centered on a **Resource Protocol**.

The Resource Protocol is a domain-agnostic but tooling-opinionated core that defines:

- A single request/response envelope for all tool surfaces (CLI, VS Code, API).
- Deterministic addressing and canonical reference resolution.
- Stable, protocol-shaped errors with steering (non-ok implies steering).
- Explicit effect typing (`pure`, `write`, `exec`) and confirm requirements.
- Paging/listing as first-class operations.

On top of this protocol core, Exosuit exposes **resources** (epochs, phases, tasks, RFCs, etc.) as a unified set of addressable objects with projections and operations.

Finally, we add thin adapters:

- Rust CLI adapter(s) that compile argv → invocation → protocol calls.
- A generic TypeScript client for VS Code that calls operations and consumes projections without per-resource handcrafted code.

## Motivation

Today, `tools/exo` effectively contains:

- protocol concerns (envelopes, errors, steering),
- domain logic (plan/phase/task/rfc),
- and CLI wiring,

in a single crate and module surface.

This causes drift relative to RFC guidance (notably: spec-driven CLI/compiler design and “no shell”) and makes the VS Code extension more bespoke than we want.

We want:

- One stable protocol surface shared across CLI + VS Code.
- A small, generic VS Code client that can consume _any_ resource and _any_ operation.
- Domain logic that is reusable outside of the CLI.
- A clear dependency direction (core → resources → adapters) that keeps invariants enforceable.

## Goals

- Define the _core_ protocol nouns and invariants (“laws”).
- Define the crate boundaries and dependency direction.
- Make “list operations” (via `Position`) core, reusable operation shapes.
- Enable a generic TS client (no hand-crafted client per resource kind).

## Non-Goals

- Finalize canonical identifier strategy (ULID/slug is tracked elsewhere; this RFC assumes canonical refs exist).
- Replace all current CLI code in one shot (migration will be incremental).
- Specify UI components or VS Code UX.

## Proposed Architecture

### Crate Split (target)

1. **`exosuit-protocol` (Rust)**

Domain-agnostic protocol types + invariants:

- Envelopes: request/response, protocol versioning
- Addresses: root/namespaces/operations
- Effects: `pure | write | exec`
- Errors: stable codes + minimal stable `details` schema
- Steering: next-call suggestions
- Paging/listing primitives
- Canonical reference surface: `ResourceRef` + `ResolvedRef` (echo canonical)

2. **`exosuit-resources` (Rust)**

Domain models expressed _in terms of_ protocol core:

- Resource kinds (Epoch/Phase/Task/RFC/…)
- Resolvers (canonical refs + aliases)
- Projections (read models)
- Operations (write models) with explicit effect typing
- Standard operation “traits” (e.g. collections/list ops)

3. **`exosuit-cli-adapter` (Rust)**

Thin wrapper that:

- Presents the resource operations as CLI commands.
- Compiles argv (or tool JSON) into protocol calls.
- Enforces tool-safety invariants:
  - spawn argv, never a shell
  - `--format json` emits exactly one JSON value on stdout
  - non-ok implies steering

4. **`exosuit-client` (TypeScript)**

Generic client used by the VS Code extension:

- `help(address)` for discovery
- `list(address, kind, page)`
- `call(address, input)`
- a small reactive cache layer that maps invalidations → re-fetch

The VS Code extension should not maintain per-resource bespoke clients.

In addition, the generic client is expected to provide a **root materializer registry**:

- a small registry keyed by root ID/address
- responsible for the only permitted disk I/O for those roots (read bytes / list directories)
- updates the reactive runtime’s disk/directory cells and revisions

Watcher-driven invalidations remain I/O-free; disk reads occur only on demand/revalidation.

### Reactive Runtime Placement (assumption)

The generic TypeScript client is expected to sit on top of a **long-lived reactive runtime**.

In the current codebase, this is already realized as a **WASM module embedded in the VS Code extension** (built from the existing Rust `exosuit-reactivity` crate), with the extension host forwarding filesystem watcher events into the runtime.

This RFC assumes we will **continue** with this model:

- VS Code provides change notifications (invalidate triggers).
- The reactive runtime is authoritative for dependency tracking and revalidation.
- The client is responsible for top-down revalidation and projection fetching.

### Dependency Direction

- `exosuit-protocol` has no dependency on resources or adapters.
- `exosuit-resources` depends on `exosuit-protocol`.
- CLI/VS Code adapters depend on both.

This ensures the protocol laws remain enforceable and not “accidentally bypassed”.

## Core Protocol Laws (Normative)

1. **Single envelope**: every request yields exactly one response envelope.
2. **Non-ok implies steering**: if `status != ok`, include steering (at least one viable next call).
3. **No shell**: execution is explicit spawn spec (`argv`, `cwd`, `env`, `stdin`); no `sh -c`.
4. **Echo canonical**: when a user supplies an alias, return the resolved canonical ref in machine output.
5. **Deterministic errors**: stable `error.code`, human `error.message`, structured `error.details`.

6. **Projections are computations over reactive roots**: projection results are computed as normal (pure) functions against reactive roots. Projections do not write roots; writes happen only through explicit operations.

7. **Confirm-required is a protocol handshake**: operations with `Effect::Write` or `Effect::Exec` may require an explicit second call.

   - First call returns `status = confirm_required` with a `ticket`.
   - Steering must include a `next_call` that is the _same operation_ replayed with `confirm = true` and the `ticket`.
   - Steering should include a machine-readable flag indicating this step requires explicit user confirmation (so CLI/VS Code/agents stop and ask before replaying).

## Operation Discovery and Generic Clients

To enable a generic TS client, the protocol surface must be discoverable.

`help(address)` should return:

- available namespaces and operations
- effect classification
- stable operation identifiers
- (optionally) schema references for operation input/output

Additionally, operations that expose “collections” should advertise standardized list operations (insert/move/remove) via a small vocabulary of operation IDs.

## Relationship to Existing RFCs

- RFC 0132: spec-driven CLI/compiler approach and “no shell” execution model.
- RFC 0063: boundary conversion and protocol-shaped failure surfaces.
- RFC 0131: canonical execution artifact (implementation plan).
- RFC 0064: canonical vs projections and upgrade gates.
- RFC 0068: `Position` as a reusable list operation abstraction.

This RFC is the architectural “wiring” that makes those laws structurally enforceable.

## Migration Strategy (Incremental)

1. Introduce `exosuit-protocol` as a crate and move envelope/error/steering primitives into it.
2. Introduce `exosuit-resources` and migrate one vertical slice (customer zero).
3. Rebuild CLI wiring as a thin adapter over the resource registry.
4. Replace VS Code bespoke clients with a generic `exosuit-client`.

## Open Questions

- How do we version protocol schemas (capabilities + op IDs) across releases?
- What is the initial canonical ref type (path refs vs ULID refs) for each resource?
- How should confirmation semantics interact with `Effect::Write/Exec`?

TS client shape (scoping question): should the reactive client expose an explicit `beginObserve()/endObserve()` API that returns a `scope` used for reads, so dependency tracking is explicit and nestable?
