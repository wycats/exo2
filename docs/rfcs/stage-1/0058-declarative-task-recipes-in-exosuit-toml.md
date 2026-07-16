<!-- exo:58 ulid:01kmzxey0sggjfny7jxw78y01s -->

# RFC 58: Declarative Task Recipes in exosuit.toml

- **Superseded by**: RFC 0044




# RFC 0058: Declarative Task Recipes in exosuit.toml

## Summary

Extend the `exosuit.toml` task system so a task can be defined as a **recipe**: an ordered list of **steps**, each step being a process invocation with explicit `cwd` and optional `env`.

Steps support two equivalent forms:

- `argv = ["prog", "arg", ...]` (canonical)
- `run = "prog --flag 'two words'"` (shorthand, parsed into canonical argv using a real shell-words parser)

This makes complex workflows (like extension dogfooding) expressible directly in `exosuit.toml`, without requiring wrapper shell scripts.

## Motivation

Today, `exosuit.toml` tasks are effectively:

- a name
- a shell command string (`cmd`)
- a coarse working-directory selector (`cwd`)

That is enough for simple commands, but it pushes real workflows into shell scripts (like `scripts/dev/dogfood-extension.sh`). This creates:

- **Duplication / drift**: scripts become the real “source of truth,” while `exo run <task>` is only an indirection.
- **Poor structure**: we can’t name steps, report progress, or add future affordances (dry-run, structured failure, per-step timing).
- **Safety & determinism**: shell-string execution is fragile (quoting, whitespace) and hard to constrain.

A recipe format lets us keep the “single obvious entry point” (`exo run`) while making the workflow itself declarative, inspectable, and enforceable.

## Detailed Design

This section is normative.

### Terminology

- **Task**: a named runnable workflow discovered from `exosuit.toml`.
- **Recipe**: a task defined as a sequence of steps.
- **Step**: one process invocation (argv + cwd + env) executed in order.

### Backwards compatibility

Existing task definitions remain valid:

```toml
[tasks]
check = { cmd = "./scripts/check", desc = "Run all checks", cwd = "root" }
```

This RFC introduces a new shape (“recipe tasks”), and tools should treat it as the preferred, future-facing format.

### Recipe tasks

A task MAY instead be defined as a table with `steps`:

```toml
[tasks.build-ext]
desc = "Compile and install the extension"
cwd = "root" # default cwd for steps

[[tasks.build-ext.steps]]
name = "Install JS deps (pnpm)"
run = "proto exec pnpm -- pnpm install"

[[tasks.build-ext.steps]]
name = "Build wasm bindings"
run = "bash ./scripts/build-wasm.sh"

[[tasks.build-ext.steps]]
name = "Build workspace TS deps (core)"
run = "proto exec pnpm -- pnpm -C packages/exosuit-core run build"

[[tasks.build-ext.steps]]
name = "Build workspace TS deps (rtd)"
run = "proto exec pnpm -- pnpm -C packages/exosuit-rtd run build"

[[tasks.build-ext.steps]]
name = "Build extension (dogfood)"
run = "proto exec pnpm -- pnpm -C packages/exosuit-vscode run build:dogfood"

[[tasks.build-ext.steps]]
name = "Install extension"
run = "bash ./scripts/dev/install-extension.sh"
```

#### Task fields

For a recipe task (`[tasks.<id>]` as a table):

- `desc` (optional): human-readable description.
- `cwd` (optional): default working directory for steps. Defaults to `root`.
- `steps` (required): array of step tables.

#### Step fields

- `name` (optional): human-readable label for logs.
- Exactly one of:
  - `argv` (canonical): array of program + args. Executed without a shell.
  - `run` (shorthand): a single command string parsed into argv.
- `cwd` (optional): overrides task-level cwd for this step.
- `env` (optional): a string map of environment variables to add/override.

If both `argv` and `run` are present, configuration loading MUST fail with an error.

If `run` is present, `exo` MUST parse it into argv using a shell-words parser (POSIX-like quoting rules). The parsed argv is treated as the canonical execution plan.

This shorthand is explicitly not “shell execution”: it is only for converting a string into argv.

#### `cwd` values

This RFC tightens `cwd` to a small, explicit vocabulary:

- `"root"`: workspace root.
- `"package_root"`: inferred package root for file-driven workflows.
- `"path:<relative>"`: resolve `<relative>` from workspace root.

Any other value MUST be treated as a configuration error.

For `path:<relative>`:

- `<relative>` MUST be a relative path.
- Implementations MUST reject path traversal that resolves outside the workspace root.

#### `run` parsing constraints

Because `run` is only a shorthand for argv (not an embedded shell), `run` MUST be limited to “a single process argv” and MUST NOT support shell operators.

Implementations MUST reject `run` strings that contain (after parsing) any of the following shell constructs/operators:

- pipes (`|`)
- redirects (`>`, `>>`, `<`, `2>`, etc.)
- boolean chaining (`&&`, `||`)
- command sequencing (`;`)
- command substitution (`$()`, backticks)

If a workflow genuinely requires shell features, it should continue to use `cmd` (legacy) or an explicit wrapper script until recipes grow a structured alternative.

#### `env` constraints

- `env` keys MUST be non-empty strings.
- `env` values MUST be strings.
- Implementations MUST NOT perform variable expansion on `env` values.

### Execution semantics

- `exo run <task>` executes steps **in order**.
- It prints step headers (using `name` when present).
- It stops on the first non-zero exit code.
- Each step runs via argv spawning (no `sh -c`).
- `env` and `cwd` are applied per step.
- Before execution, each step MUST be normalized to canonical argv:
  - If `argv` present: use it.
  - If `run` present: parse into argv.

### Discoverability / inspection

Future flags (not required for Stage 1) that become possible once tasks are structured:

- `exo run --list` showing whether a task is `cmd` or `recipe`.
- `exo run --dry-run <task>` printing resolved steps without executing.
- `exo run --json <task>` emitting a structured execution plan for tools/UI.

## Migration

- Convert `build-ext` from `cmd = "./scripts/dev/dogfood-extension.sh"` to a recipe (example above).
- Keep the shell script temporarily (or delete it later) once the recipe is validated.
- Over time, migrate other multi-action scripts into recipes.

## Drawbacks

- More config surface area (users must learn the recipe shape).
- Some workflows are genuinely easier in shell (conditionals, loops). Recipes must resist becoming a full programming language.

## Alternatives

- Continue using wrapper scripts and keep `cmd` as-is.
- Add a dedicated scripting language or embedded DSL (likely too heavy).
- Accept shell strings but add strict linting/sanitization (still fragile).

## Unresolved Questions

- Should we support small, explicit variable interpolation (or none at all)?
- Do we need a first-class `proto` runner abstraction, or is `argv = ["proto", ...]` enough?
- How should `cwd = "package_root"` be interpreted for `exo run` when invoked manually?
- Should recipes support explicit “artifacts”/outputs for future caching and UI?

## Future Possibilities

- Parallel step groups (explicitly declared, not inferred).
- Per-step caching keyed by declared inputs/outputs.
- Structured progress events surfaced in the VS Code extension UI.
- First-class integration with the `exosuit` LM tool (`run` port) using the same recipe schema.
