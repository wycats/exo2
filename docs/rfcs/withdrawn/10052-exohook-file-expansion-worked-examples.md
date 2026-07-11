<!-- exo:10052 ulid:01kmzxefdbh5jnvh60r0ennbhf -->


# RFC 10052: Exohook: File Expansion Worked Examples

- **Status**: Withdrawn
- **Stage**: 1
- **Reason**:

## Summary

We want validation to be a first-class product surface, not a pile of scripts.

This RFC proposes a new standalone tool, `exohook`, plus a declarative configuration file at `.config/exo/hooks.toml` that defines checks once and derives:

- “as you go” validation (uncommitted files)
- git hook behavior (pre-commit / pre-push)
- CI workflows (e.g. GitHub Actions steps)

This successor RFC focuses specifically on the most confusion-prone part of the system: **how file lists flow through the system**, how they are substituted into commands, how chunking/reruns work, and how **autofix + restage** behaves safely in pre-commit.

## Motivation

A recurring source of confusion (and drift) is: “how does the list of files actually get injected into a tool command, and what happens when it’s long?”

If this is underspecified, a declarative validation system becomes a new kind of sharp edge:

- users can’t predict what a command actually runs
- list expansion differs per tool
- OS argument limits cause confusing partial failures
- the rerun story is unclear

A second source of confusion is pre-commit formatting and autofixes:

- formatters / `--fix` lints may rewrite files in the working tree
- pre-commit correctness is about the *index* (what will be committed)
- without a deterministic restage policy, users can “fix” a file but still commit the unfixed staged version

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

Define fixed canonical lane names:

- `dev`: run constantly against **uncommitted** files (changed + untracked + staged)
- `coherence`: run against **staged** files (pre-commit coherence gate)
- `gate`: run against **committed-not-pushed** code (pre-push CI emulator)
- `ci`: run against **repository at HEAD** (authoritative CI)

Open question: allow user-defined lanes in addition to canonical lanes.


### Human-facing metadata (labels, descriptions)

While lane and check **IDs** must be stable and tooling-friendly, `.config/exo/hooks.toml` is also a contributor-facing document.

- Lanes and checks SHOULD support optional `label` and `description` fields intended purely for display (CLI/UI) and onboarding.
- These fields MUST NOT affect execution semantics (selection, scoping, restage, parallelism).
- Tooling that edits or canonicalizes the file (migrators, config subcommands) SHOULD preserve `label`/`description` when present.

Rationale: the canonical lane IDs (`dev`, `coherence`, `gate`, `ci`) are meaningful to the system, but *not inherently meaningful* to new contributors. Labels/descriptions allow the file to explain itself without changing the underlying model.


### Filesets: an Algebra (not a menu)

Instead of hardcoding a small list, define filesets as expressions:

- primitives: `staged`, `changed`, `untracked`, `committed_not_pushed`, `all_tracked`
- combinators: `union`, `intersect`, `diff`
- filters: `glob`, `exclude`

### File List Expansion Patterns

`exohook` must standardize file injection by making it an explicit part of each check definition.

#### The Core Concept: checks declare an **input mode**

Each check declares how it consumes inputs:

- `none`: ignores filesets, always runs on the whole workspace (e.g. `tsc --noEmit`, `cargo clippy --workspace`)
- `paths`: consumes a list of file paths (e.g. `eslint {{files}}`, `prettier {{files}}`)
- `packages` (future): consumes package/crate identifiers derived from fileset analysis

A check that declares `none` MUST still be selectable in lanes, but it simply does not receive file arguments.

#### Substitution sites

When a check consumes `paths`, `exohook` must define *exactly* where and how the list appears.

Minimal teachable contract:

- Config contains a placeholder `{{files}}` at the substitution site.
- `exohook` replaces `{{files}}` with the lane’s concrete file list (after filtering + normalization).

Supported patterns:

- **Positional list**: `tool ... {{files}}`
- **Flagged list**: `tool ... --files {{files}}` (tool-specific)
- **Response file**: `tool ... @<path>` (tool-specific)
- **STDIN list**: `tool ...` and newline-delimited files on stdin (tool-specific)

#### Chunking semantics

`exohook` must define deterministic chunking behavior:

- If a `paths` check’s expanded arguments would exceed system limits, split into chunks and run multiple times.
- Chunk order is stable (sorted by normalized path).
- Failures are aggregated.

A check can declare:

- `batchable = true|false`

If `batchable = false` and the list is too long, `exohook` falls back (in order):

1. response file
2. stdin list
3. fail with a recovery-oriented error

#### Parallel execution (lefthook compatibility)

Some validation configs (including lefthook’s) run multiple independent checks and expect them to execute concurrently.

Proposed lane policy:

- A lane can declare `parallel = true|false` (default `false`).
- When `parallel = true`, `exohook` starts all selected checks concurrently and waits for all checks to finish.
- Failure semantics are “run all, then fail”: `exohook` exits non-zero if any check fails, and it reports all failures (it does not stop at the first failure).

Output contract:

- `exohook` captures each check’s stdout/stderr and prints it grouped by check (no interleaving).
- Group order is stable (config order), regardless of which checks finish first.

Interaction with chunking:

- A single check that chunks its file list runs its chunks in stable order within that check.
- Chunk failures are aggregated and reported under the check’s output group.

Safety rule for autofix + restage:

- If a lane enables restaging behavior (e.g. `coherence` defaults), and any selected check is `autofix = true`, `exohook` MUST NOT run those checks in parallel.
- MVP behavior: in this situation, `exohook` forces sequential execution (and may emit a warning that `parallel = true` was ignored for safety).

#### Rerun contract (for debugging)

When chunking occurs, `exohook` must print rerun commands with stable chunk IDs.

The exact CLI flags are an implementation detail, but the behavior is not: every chunk failure must be rerunnable without recomputing the entire world.

#### Autofix + Restage contract (pre-commit safety)

Some checks are *autofixable*: they may modify files (formatters, or linters in `--fix` mode).

`exohook` must support an explicit declaration that a check is allowed to modify files:

- `autofix = true` (name not final; equivalent to “fixes”)

**Default policy**:

- In the `coherence` lane (pre-commit; staged files), `autofix = true` implies **automatic restaging**, unless explicitly disabled.
- In all other lanes (`dev`, `gate`, `ci`), restaging is **off by default**.

**What “restage” means**:

- If the autofix tool changes files on disk, `exohook` updates the Git index so the commit includes those fixes.

**Safety constraints (non-negotiable)**:

- Restage must *never* stage unrelated working-tree changes.
- Restage must be scoped to the lane’s file list (after filtering + normalization).
- Restage must be deterministic and explain what it staged.

**Suggested algorithm (conceptual)**:

1. Compute `input_paths` = the concrete file list for the check (post-filter).
2. Run the tool on `input_paths` (or via response file / stdin list).
3. Compute `changed_paths` = working-tree paths that changed *within* `input_paths`.
4. If `changed_paths` is non-empty, run `git add -- <changed_paths>`.

**Containment enforcement**:

If an autofix tool modifies files *outside* `input_paths`, `exohook` should treat that as a configuration error and fail with a recovery-oriented message. (This prevents “formatter ran on the whole repo” from silently staging unexpected changes.)

**`input_mode = none` and restage**:

For workspace-wide tools (`input_mode = none`), restaging is ambiguous.

- Default: disallow implicit restage.
- If needed, require an explicit, opt-in strategy (e.g. `restage_strategy = "all_staged"`) so the blast radius is deliberate.

### Worked Examples (Concrete)

These examples are intentionally “boringly explicit”. The goal is that a reader can understand exactly where the file list goes, and what to do when chunking happens.

#### Example 1: ESLint on changed TS/JS files (paths-based)

Conceptual config:

- lane: `dev`
- fileset: `uncommitted`
- check: `eslint`
- input_mode: `paths`
- placeholder: `{{files}}`

Illustrative snippet (schema not finalized):

```toml
[[checks]]
# id = "eslint"
# label = "ESLint"
# input_mode = "paths"
# batchable = true
# glob = "*.{js,ts,jsx,tsx}"
# run = "pnpm exec eslint --max-warnings 0 {{files}}"
```

What actually runs (example):

```text
pnpm exec eslint --max-warnings 0 packages/exosuit-vscode/src/foo.ts packages/exosuit-core/src/bar.ts
```

If the list is too long, `exohook` chunks:

```text
[eslint] 1,284 files exceeds argv limit; running 7 chunks
[eslint] chunk 3/7 failed (187 files)
Rerun: exohook validate dev --check eslint --chunk 3
```

#### Example 2: Prettier formatting (autofix + auto-restage in coherence)

Conceptual config:

```toml
[[checks]]
# id = "prettier"
# label = "Prettier"
# input_mode = "paths"
# batchable = true
# autofix = true
# run = "pnpm exec prettier --write {{files}}"
```

Behavior by lane:

- In `dev`: formatting may be allowed, but **no restage** occurs by default.
- In `coherence`: after formatting succeeds, `exohook` automatically restages *only the files from the lane list that actually changed*.

This makes the pre-commit experience “automatic” while still being safe:

- it never stages unrelated edits
- it keeps the staged snapshot consistent with the working tree fixes
- it can explain exactly what it staged

#### Example 3: ESLint autofix (autofixable lints)

Autofixable lints behave like formatters from `exohook`’s perspective.

```toml
[[checks]]
# id = "eslint-fix"
# label = "ESLint (fix)"
# input_mode = "paths"
# batchable = true
# autofix = true
# run = "pnpm exec eslint --fix --max-warnings 0 {{files}}"
```

In `coherence`, this implies the same auto-restage behavior as formatters.

#### Example 4: Vitest “related” mode (paths-derived, but tool consumes differently)

Some test runners want “changed files” but do not accept them as simple positional file lists.

Two patterns `exohook` should support:

1) **Tool-native related mode** (preferred):

```toml
[[checks]]
# id = "vitest-related"
# label = "Vitest (related)"
# input_mode = "paths"
# batchable = true
# run = "pnpm exec vitest related --run {{files}}"
```

2) **Fallback to suite** (when related mode isn’t stable): set `input_mode = "none"` and run a broader command in `gate/ci`.

The key is that the config makes the consumption mode explicit, rather than leaving it to ad-hoc conventions.

#### Example 5: TypeScript typecheck (workspace-based: none)

Typecheck often can’t be meaningfully run per-file unless the tool explicitly supports it.

So:

```toml
[[checks]]
# id = "typecheck"
# label = "TypeScript typecheck"
# input_mode = "none"
# run = "pnpm -r run typecheck"
```

Even if the lane selects `uncommitted`, a `none` check runs once at workspace scope (and does not receive `{{files}}`).

#### Example 6: Rust clippy strict (workspace-based: none)

```toml
[[checks]]
# id = "clippy"
# label = "Clippy"
# input_mode = "none"
# run_strict = "cargo clippy --workspace -- -D warnings"
# run_proto  = "cargo clippy --workspace"  # optional
```

This matches the policy that strict lanes block on warnings, while still leaving room for `prototype` mode.

### Lefthook Parity Target (Exosuit Repo)

This RFC is intentionally abstract, but we need a concrete near-term target: parity with the repository’s existing `lefthook.yml` behavior.

**Current lefthook behavior to match**:

- `pre-commit`: `parallel: true`
  - runs: `check`, `lint`, `rust-fmt` (`stage_fixed: true`), `rust-clippy`, `verify-toml`
- `pre-push`: `parallel: true`
  - runs: `test`, `rust-coverage`

**Required exohook runtime semantics**:

- **Parallel lanes**: when a lane declares `parallel = true`, `exohook` runs all selected checks concurrently and uses “run all, then fail” exit semantics.
- **Output grouping**: output is grouped per check (no interleaving) and printed in stable config order, regardless of completion order.
- **Autofix safety**: if a lane enables restaging and any selected check has `autofix = true`, `exohook` must not run those checks concurrently.
  - MVP: force sequential execution for the entire lane and warn that `parallel = true` was ignored.
  - Follow-up (optional): run non-autofix checks in parallel, then run autofix+restage checks sequentially.
- **Stage-fixed parity**: checks equivalent to lefthook’s `stage_fixed: true` must restage changes into the Git index deterministically and with containment enforcement (as specified above).
- **Git hook shims**: `exohook hooks install` must install `.git/hooks/pre-commit` and `.git/hooks/pre-push` shims that invoke the projected lanes (`coherence` and `gate` in this repo), without requiring lefthook.



**Output format parity (visual)**:

The current grouped output is deterministic, but it’s a visual regression from lefthook’s compact UI. To preserve scanability without changing semantics:

- `exohook validate <lane>` MUST support two output formats:
  - **`--format=compact` (default)**: prints a concise status line per check (with duration), prints check stdout/stderr only when a check fails (or when `--verbose` is set), and prints a lane summary line at the end.
  - **`--format=grouped`**: prints “grouped output” per check (header + full captured stdout/stderr) in stable config order. This is the deterministic debug/audit mode.
- Color MUST be supported as `--color=auto|always|never` (default `auto`), with ANSI colors only when enabled.
- The hook shims installed by `exohook hooks install` SHOULD use the default format (so the contributor experience matches lefthook).

**Acceptance criteria (repo-level)**:

- Running `exohook validate coherence` on a repo state where multiple checks fail reports all failures (not fail-fast) and exits non-zero.
- Running `exohook validate gate` behaves the same for pre-push.
- When `rust-fmt` (autofix) changes a staged file, the staged snapshot is updated to include the fixes; when it changes files outside the lane’s scoped inputs, `exohook` fails with a clear containment error.

## Migration Plan

Goal: do not break existing workflows; move quickly.

1. Add `.config/exo/hooks.toml` and `exohook validate <lane>` (additive)
2. Provide `exohook hooks install` to install `.git/hooks/*` shims (opt-in)
3. Provide `exohook migrate lefthook` to generate config from `lefthook.yml` without changing existing hooks
4. Later: switch bootstrap defaults to `exohook`, leaving lefthook as legacy/optional

## Unresolved Questions

- Exact schema for `.config/exo/hooks.toml` (typed tool model vs command-first)
- How much lane customization beyond canonical lanes
- WASI Components + WIT feasibility in current build/tooling
- Exact placeholder syntax and injection/quoting rules
- How strict containment enforcement should be for autofix tools (warn vs fail)
