<!-- exo:45 ulid:01kg5kp2d5tf7npqfdp902vtnp -->

# RFC 45: Unified Variable Environment and Lexical Scope

- **Supersedes**: RFC 0059




# RFC 0045: Unified Variable Environment and Lexical Scope

# RFC 0026: Unified Variable Environment and Lexical Scope

## Summary

Define a unified, formal **Variable Environment** (VE) and **lexical scope model** shared by:

- `exosuit.toml` (Exosuit tasks / recipes)
- Exohook configuration (`hooks.toml` lanes/checks)

…and specify the exact **variable expansion semantics** (syntax, timing, and errors) at every point where expansion is supported.

This RFC also defines a precise mapping from **lexical scope in configuration files** to an internal **runtime scope stack** used for expansion.

## Motivation

We want a crisp, testable, and debuggable specification for “what variables exist” and “how they expand” across tools.

Today:

- Similar concepts exist in multiple places (e.g., file-relative values, package root heuristics, placeholders) but are inconsistently implemented.
- Some surfaces are shell-based (`run` executed by a shell), while others aim to be argv-native.
- The absence of a formal scope model makes it hard to reason about shadowing, defaults, and availability.

This RFC introduces:

- A shared **Variable Environment** vocabulary.
- A formal **lexical scope** definition per config format.
- A deterministic **runtime scope stack** evaluated at each expansion site.

This supports:

- predictable behavior
- high-quality diagnostics (“unknown var X; available: …”)
- future UX (structured dry-run plans, VS Code inspection)
- future manual documentation (copy/paste normative sections)

## Non-goals

- Executing shell syntax as part of expansion.
- Implicit reading of process environment variables (unless explicitly modeled as a scope frame in a future RFC).
- Introducing a full template language (conditionals/loops).

## Definitions

- **Variable Environment (VE)**: a mapping `name -> typed value`.
- **Expansion**: rewriting a template string or argv tokens by substituting variable references with serialized values.
- **Lexical scope**: source-level containment in a config file.
- **Runtime scope stack**: an ordered list of frames providing bindings for expansion.

### Typed values

VE values are typed:

- `String`
- `Path`
- `List<String>`
- `List<Path>`

Serialization rules are defined in this RFC.

## Configuration surfaces

### Exosuit (`exosuit.toml`)

Expansion is supported in the task system (including RFC 0044 recipe tasks):

- argv-native: `argv = ["prog", "arg", ...]`
- shorthand: `run = "..."` (a string that is parsed into argv)

### Exohook (`hooks.toml`)

Expansion is supported in check definitions:

- argv-native: `argv = ["prog", "arg", ...]`
- legacy shell string: `run = "..."` (currently executed by a shell)

This RFC defines unified variable availability and expansion semantics for argv surfaces, and precisely specifies the behavior and boundaries for shell-string surfaces.

## Lexical scope (source-level)

Lexical scope defines a containment tree for each configuration format.

### `exosuit.toml` lexical scope

- Document scope: the whole TOML document
- Task scope: a single task definition (`[tasks.<task_id>]`)
- Step scope: a single recipe step (`[[tasks.<task_id>.steps]]`)

### Exohook (`hooks.toml`) lexical scope

- Document scope: the whole TOML document
- Lane scope: `[lane.<lane_id>]`
- Check scope: `[check.<check_id>]`
- Override scope: lane overrides that specialize a check for that lane (when applicable)

## Runtime scope stack (evaluation model)

At any expansion point, expansion is evaluated against a **Scope Stack**: an ordered list of frames from lowest precedence to highest precedence.

### Frame order (lowest -> highest precedence)

1) **Builtins Frame**
   - Always present.
   - Provides foundational bindings such as `root`.

2) **Document Frame**
   - Bindings defined at the document level (if/when a config format supports them).
   - If not supported, this frame is empty.

3) **Container Frame**
   - Exosuit: task-level defaults (e.g., default `cwd`, default `env`).
   - Exohook: lane/check bindings that apply broadly.

4) **Node Frame**
   - Exosuit: step-level overrides.
   - Exohook: the specific check invocation instance (including override bindings).

5) **Invocation Frame**
   - Computed at runtime from the invocation context (selected file(s), inferred `package_root`, etc.).
   - Includes only bindings that are well-defined for the current invocation.

### Lookup (shadowing)

To resolve a variable `name`, search frames from top (highest precedence) downward; the first match wins.

If a variable is not found, the expansion MUST fail with an error that includes:

- the missing name
- the expansion site (argv/run/env)
- the available variable names in scope

### Push/pop points

#### Exosuit recipe execution

- Enter task: push Task (Container) frame.
- For each step:
  - push Step (Node) frame.
  - compute effective `cwd` and populate Invocation bindings that depend on it (e.g., `relative_file`).
  - perform expansion at supported sites.
  - execute.
  - pop Step frame.

#### Exohook execution

- Enter lane: push Lane (Container) frame.
- For each check:
  - push Check frame (+ Override frame if applicable).
  - push Invocation frame (e.g., `files` for paths mode).
  - perform expansion at supported sites.
  - execute.
  - pop frames.

## Variable vocabulary (unified standard set)

Variables exist only when their prerequisites are satisfied by the invocation context.

### Workspace / invocation

- `root: Path` — workspace root
- `cwd: Path` — effective working directory for the current step/check

### File-context (only when a single file is in context)

- `file: Path` — context file path
- `dir: Path` — directory containing `file`
- `stem: String` — filename stem
- `relative_file: Path` — `file` relative to `cwd`
- `package_root: Path` — nearest ancestor of `file` (or of `cwd` if no file) containing `package.json` or `Cargo.toml`, bounded by `root`

### Multi-file context (when a file list is in context)

- `files: List<Path>` — list of paths

This RFC reserves additional derived representations (e.g. `files_json`, `files_nul`) for a future RFC; they are not part of the Stage 1 contract.

## Expansion syntax

Two substitution forms are standardized:

### Scalar substitution: `{name}`

- Expands to a single string.
- Only valid when `name` resolves to a scalar type: `String` or `Path`.

### Injection substitution: `{{name}}`

- Expands by splicing **zero or more argv tokens**.
- Only valid when `name` resolves to a list type: `List<String>` or `List<Path>`.

This formalizes Exohook’s existing `{{files}}` placeholder as a list injection mechanism.

## Expansion sites and timing

Every expansion site MUST specify:

- whether it allows scalar substitution (`{name}`)
- whether it allows injection substitution (`{{name}}`)

### argv-native surfaces (preferred)

For `argv = [ ... ]`:

- Scalar substitution `{name}` is applied **per token**.
- Injection substitution `{{name}}` splices tokens into argv.
- There is no recursive expansion.

### `run` shorthand surfaces (string parsed to argv)

When a surface supports `run = "..."` as shorthand for argv:

1) Apply scalar substitution `{name}` to the `run` string.
2) Parse the resulting string into argv using a real shell-words parser.
3) Validate the argv for unsupported shell operators as required by the caller (e.g., RFC 0044).

`{{name}}` injection in `run` is not supported in Stage 1 unless explicitly specified by the consumer; if present, it MUST be rejected with a clear diagnostic.

### `env` surfaces

If a surface supports expansion in `env` values, it MUST be scalar-only (`{name}`) and MUST NOT perform `$VAR` interpolation.

(Stage 1 recommendation: keep `env` unexpanded unless explicitly adopted by the consumer RFC.)

## Serialization (typed -> string)

- `Path` serializes to a platform path string.
  - If the path is inside `root`, it SHOULD serialize as workspace-relative.
  - Otherwise it serializes as absolute.
- `List<Path>` / `List<String>` do not serialize to a single string for scalar substitution.
  - They are only valid via injection substitution `{{name}}`.

## Errors (normative)

- Unknown `{name}` / `{{name}}`: error.
- File-context variable used without a file in context: error.
- Scalar substitution `{name}` used for a list value: error; suggest `{{name}}`.
- Injection substitution `{{name}}` used for a scalar value: error.

## Determinism and safety

Expansion MUST NOT:

- execute commands
- interpret shell operators
- implicitly read environment variables

All derived paths (including `package_root`) MUST be bounded by `root`.

## Compatibility and migration notes (non-normative)

- Some existing implementations are partial or ad-hoc (e.g., simplistic command splitting).
- This RFC defines the target semantics; implementations should converge on a shared VE builder and expander.

## Future work

- First-class declaration of document-scoped vars (Document Frame) in both formats.
- Additional multi-file representations (`files_json`, response files, NUL-delimited).
- Migrating shell-executed `run` surfaces toward argv-native execution where feasible.

