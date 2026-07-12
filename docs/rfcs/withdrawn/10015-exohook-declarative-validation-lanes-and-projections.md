<!-- exo:10015 ulid:01kmzxefdkfn85waz4qvpdjnfa -->

# RFC 10015: Exohook: Declarative Validation Lanes and Projections

- **Status**: Withdrawn
- **Stage**: 0
- **Reason**:

# RFC 10015: Exohook: Declarative Validation Lanes and Projections

- **Superseded by**: RFC 0081


## Summary

We want validation to be a first-class product surface, not a pile of scripts.

This RFC proposes a new standalone tool, `exohook`, plus a declarative configuration file at `.config/exo/hooks.toml` that defines checks once and derives:

- “as you go” validation (uncommitted files)
- git hook behavior (pre-commit / pre-push)
- CI workflows (e.g. GitHub Actions steps)

The immediate goal is to remove the requirement that contributors install a third-party hook runner (e.g. lefthook) while preserving (and strengthening) strict quality gates, including Rust clippy “warnings as errors”, formatting, and JS/TS validation.

## Motivation

Today we maintain overlapping sources of truth:

- Git hooks configured via third-party tools (e.g. `lefthook.yml`)
- ad-hoc scripts (e.g. `scripts/check`)
- CI configuration

This creates drift:

- Hooks can be stricter than local checks (or vice versa)
- Running checks “as you go” is awkward
- CI is not obviously derived from the same definition
- Hooks sometimes hang when a tool unexpectedly becomes interactive

We want a single declarative definition that can be projected into multiple execution contexts, with consistent semantics, strong defaults, and excellent diagnostics.

This aligns with:

- Axiom 10 (Generative over Descriptive)
- Axiom 11 (Agent-First Tooling)
- The existing “Quality Gates” decision (currently implemented via lefthook): preserve the intent, replace the mechanism.

## Proposed Axiom (Follow-up)

**Validation is a Product Surface**: Validation must be declarative, derivable (hooks/CI/dev loop), deterministic, and produce recovery-oriented errors.

If accepted, follow up by adding this axiom via `exo axiom add`.

## Detailed Design

### File Location & Project Convention

Adopt “repo config lives under `.config/`”. For Exosuit projects, configuration moves under `.config/exo/`.

- New config: `.config/exo/hooks.toml` (authoritative)
- Proposed migration: move `exosuit.toml` into `.config/exo/` (exact filename TBD) and update tools to discover config there first.

### Terminology

- **check**: a runnable validation unit (lint, format, typecheck, test, etc.)
- **fileset**: a computed set of paths (e.g. staged, untracked)
- **lane**: a canonical view that binds a fileset + check selection + policy
- **projection**: a derived artifact, e.g. git hook shims or CI steps

### Canonical Lanes

Define fixed canonical lane names (internally implemented abstractly as a composable “lane” construct):

- `dev`: run constantly against **uncommitted** files (changed + untracked + staged)
- `coherence`: run against **staged** files (pre-commit coherence gate)
- `gate`: run against **committed-not-pushed** code (pre-push CI emulator)
- `ci`: run against **repository at HEAD** (authoritative CI)

Open question: allow user-defined lanes in addition to canonical lanes.

### Filesets: an Algebra (not a menu)

Instead of hardcoding a small list, define filesets as expressions:

- primitives: `staged`, `changed`, `untracked`, `committed_not_pushed`, `all_tracked`
- combinators: `union`, `intersect`, `diff`
- filters: `glob`, `exclude`

The config must document predictable “de-facto” patterns for where file lists are substituted into commands, and how long lists are chunked.

### Checks

Checks are defined once, then selected by lanes.

Each check has:

- `id`, `label`
- a runner definition (typed tool model preferred; raw command escape hatch)
- which inputs it supports (no-files / file-list / package-list)
- strictness policy per lane (e.g. warnings-as-errors in `coherence/gate/ci`)
- timeouts + non-interactive policy

**Rust strictness** (baseline):

- formatting: `cargo fmt --all` (may re-stage in `coherence`)
- clippy strict: `cargo clippy --workspace -- -D warnings` in strict lanes

**JS/TS strictness** (baseline):

- lint strict: ESLint with `--max-warnings 0` in strict lanes
- typecheck: `tsc --noEmit` (or project-specific equivalents)
- tests: workspace test commands as configured

Exact command lines remain configurable, but the default policy is: warnings block in strict lanes.

### Non-Interactive Execution, Timeouts, and Diagnostics

All executions must:

- default to non-interactive (no TTY assumptions)
- enforce per-check timeouts
- produce deterministic output (stable ordering, stable prefixes)
- detect common “hang” causes and emit actionable messages

Error messages should be written as recovery prompts (“this likely happened; try X”).

### Git Hooks: Shims Managed by `exohook`

Git hook installation writes minimal shims into `.git/hooks/` that call `exohook validate <lane>`.

No third-party tool is required.

### CI Projection

`.config/exo/hooks.toml` is general-purpose: it must contain enough information to derive CI steps.

Requirement: CI should show separate steps (readable pipeline), while still being driven from one spec.

Initial design: `exohook ci emit github-actions` (or similar) emits a step list grouped by check metadata (e.g. `group = "lint" | "test" | "format"`).

Open question: whether to materialize workflow YAML or emit a reusable, consumed artifact.

### `exo` Integration (“as you go”)

For Exosuit projects:

- `exo` uses the same validation definition
- marking tasks as done should run `dev` (uncommitted) validation (and potentially `coherence` depending on policy)

This enforces early feedback well before pre-commit.

### Plugins

`exohook` should support plugins.

A plugin can be a native executable or a **WASI module** (e.g. `exohook-foo.wasm`).

Discovery should not require executing arbitrary binaries just to identify them.

Proposed: embedded manifest payload in a dedicated section:

- WASI: custom section contains CBOR manifest
- Native: embedded section contains CBOR manifest

If feasible, investigate using WASI Components + WIT for plugin interface definition and capability boundaries; otherwise record as a follow-up investigation.

## Migration Plan

Goal: do not break existing workflows; move quickly.

1. Add `.config/exo/hooks.toml` and `exohook validate <lane>` (additive)
2. Provide `exohook hooks install` to install shims (opt-in)
3. Provide `exohook migrate lefthook` to generate config from `lefthook.yml` without changing existing hooks
4. Later: switch bootstrap defaults to `exohook`, leaving lefthook as legacy/optional

## Drawbacks

- Introduces a new tool surface to maintain (`exohook`)
- Requires careful design to avoid “yet another config language”
- Deriving CI steps reliably across platforms is non-trivial

## Alternatives

- Keep lefthook/husky as the canonical runner and improve scripts
- Use existing Rust hook tooling (e.g. cargo-husky/rusty-hook/fasthooks)
- Only enforce in CI (no local hooks)

## Unresolved Questions

- Exact schema for `.config/exo/hooks.toml` (typed tool model vs command-first)
- How much lane customization beyond canonical lanes
- WASI Components + WIT feasibility in current build/tooling
- How to best document file list substitution patterns and chunking behavior

## Future Possibilities

- Automatic migration from husky/lefthook
- Rich UI integration (show lane status in VS Code)
- Smarter “impact analysis” filesets (package/crate affected sets)

