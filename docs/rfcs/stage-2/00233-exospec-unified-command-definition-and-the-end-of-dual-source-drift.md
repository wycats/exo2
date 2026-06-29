<!-- exo:233 ulid:01kmzxbcyxb0p4ytm13pgzan4m -->

# RFC 233: ExoSpec: Unified Command Definition and the End of Dual-Source Drift


# RFC 00233: ExoSpec — Unified Command Definition and the End of Dual-Source Drift

- **Supersedes**: RFC 0135 (CommandSpec Unification), RFC 0201 (ExoSpec Derive Macro)
- **Absorbs relevant content from**: RFC 0132 (CLI Patterns), RFC 0136 (LM Tool Architecture v2), RFC 0200 (CLI Argument Consistency)
- **Builds on**: RFC 0085 (Command Trait Architecture), RFC 0097 (Machine Channel v2)

## Summary

A single `#[derive(ExoSpec)]` macro replaces the triple-source command definition system (Clap derive + `Command::args()` trait + legacy `CommandSpec`). Clap is removed entirely from the dispatch path. The macro generates everything from `#[exo(...)]` attributes: the spec (for routing, help, and `command-spec.json`), the `from_invocation()` constructor (for typed command dispatch), and the `args()` bridge (for backward compatibility during migration).

The CLI entry point parses raw argv through the spec-driven `compile_argv()` router. `--help` renders human-readable text directly from the spec, with `[pure]`/`[write]`/`[exec]` effect annotations. `--format json` makes `--help` return the raw `HelpResult` as JSON.

## What Exists Today

The ExoSpec crate (`crates/exospec/`) is implemented and the first namespace (`tdd`) is migrated end-to-end. Here's the current state:

### Implemented

| Component                                         | Location                                | Status             |
| ------------------------------------------------- | --------------------------------------- | ------------------ |
| `#[derive(ExoSpec)]` proc macro                   | `crates/exospec/src/parse.rs`           | ✅ 20 tests        |
| `HasExoSpec` trait (`fn spec() -> NamespaceSpec`) | `tools/exo/src/command/command_spec.rs` | ✅                 |
| `from_invocation()` constructor generation        | `crates/exospec/src/parse.rs`           | ✅                 |
| `merge_exospec<T>()` registry wiring              | `tools/exo/src/command/command_spec.rs` | ✅                 |
| `TddCommands` migrated with `#[derive(ExoSpec)]`  | `tools/exo/src/command/tdd.rs`          | ✅ Parity verified |
| Spec-driven `compile_argv()` router               | `tools/exo/src/argv_compiler.rs`        | ✅                 |
| `help_for_address()` returning `HelpResult`       | `tools/exo/src/api/handler.rs`          | ✅                 |
| `dispatch_via_invoke_json()` CLI dispatch         | `tools/exo/src/main.rs`                 | ✅                 |
| 454 exo tests + 20 exospec tests                  |                                         | ✅ All passing     |

### Still Using Legacy Path

| Component                      | Issue                                                | Fix                                  |
| ------------------------------ | ---------------------------------------------------- | ------------------------------------ |
| `Cli::parse()` in `main()`     | Clap parses argv, result discarded for most commands | Replace with raw argv routing        |
| `--help` / `--version`         | Handled by Clap                                      | Render from spec                     |
| `--format` global flag         | Extracted from Clap struct                           | Parse from raw argv                  |
| 4 infrastructure commands      | Detected via Clap match                              | Pattern-match raw argv               |
| ~30 namespaces                 | Still use manual `Command::args()`                   | Migrate to `#[derive(ExoSpec)]`      |
| `LIFECYCLE_OPERATIONS` TS list | Manual mapping for upgrade-gated tools               | Delete after all namespaces migrated |

## Motivation

Command metadata defined in multiple places drifts independently. The current mitigations (parity tests, manual TypeScript lists) catch some drift some of the time. The only cure is single-source definition.

Clap's parse result is already discarded — `dispatch_via_invoke_json()` re-parses raw `std::env::args()` through `compile_argv()`. Both CLI and machine channel converge at `Invocation`. Clap is a redundant parser whose only remaining jobs (`--help`, `--version`, `--format`, infrastructure command detection) are trivially replaceable.

## Detailed Design

### Architecture

```
  #[derive(ExoSpec)] + #[exo(...)]
         (Single Source)
                │
         ExoSpec macro
        (compile-time)
                │
    ┌───────────┼───────────┐
    ▼           ▼           ▼
 HasExoSpec   args()    from_invocation()
 fn spec()→   (bridge,  (typed struct
 NamespaceSpec temporary) from Invocation)
    │                       │
    ├───────────┐           │
    ▼           ▼           ▼
 CLI help    cmd-spec    Command
 --help      .json       dispatch
 rendering   (TS tools)
```

Both entry points converge at `Invocation`:

```
CLI:              raw argv → compile_argv(spec) → Invocation → dispatch
Machine Channel:  JSONL    → Invocation::from_json(spec) → dispatch
```

No Clap. No double-parse. One spec drives everything.

### The ExoSpec Derive Macro

The macro is implemented in `crates/exospec/` (proc-macro crate, deps: `syn` 2, `quote` 1, `proc-macro2` 1).

#### Usage (implemented)

```rust
#[derive(Debug, exospec::ExoSpec)]
#[exo(namespace = "tdd", description = "Manage TDD cycles")]
enum TddCommands {
    /// Start a new TDD cycle for a task
    #[exo(effect = "write")]
    New {
        /// The task selector (task id or goal::task)
        #[exo(long, short = 'n')]
        name: String,
        /// The test command or file
        #[exo(long, short = 't')]
        test: String,
    },
    /// Confirm the test is failing (red phase)
    #[exo(effect = "write")]
    Red,
    /// Confirm the test is passing (green phase)
    #[exo(effect = "write")]
    Green,
}
```

#### Generated Code

The macro generates two items:

1. **`impl HasExoSpec`** — returns a `NamespaceSpec` with all operations, args, effects, descriptions:

```rust
pub trait HasExoSpec {
    fn spec() -> NamespaceSpec;
}
```

2. **`from_invocation()`** — typed constructor that extracts values from an `Invocation`:

```rust
impl TddCommands {
    pub fn from_invocation(inv: &Invocation) -> anyhow::Result<Self> {
        match inv.operation() {
            "new" => {
                let name = inv.get_string("name")
                    .ok_or_else(|| anyhow::anyhow!("Missing required argument 'name'"))?
                    .to_string();
                let test = inv.get_string("test")
                    .ok_or_else(|| anyhow::anyhow!("Missing required argument 'test'"))?
                    .to_string();
                Ok(TddCommands::New { name, test })
            }
            "red" => Ok(TddCommands::Red),
            "green" => Ok(TddCommands::Green),
            other => anyhow::bail!("Unknown operation '{}' for namespace 'tdd'", other),
        }
    }
}
```

#### Type Inference

Field types map to `ValueType` automatically:

| Rust Type                                 | ValueType    | Extraction Method                  |
| ----------------------------------------- | ------------ | ---------------------------------- |
| `String`                                  | `String`     | `get_string()`                     |
| `bool`                                    | `Bool`       | `get_bool()`                       |
| `i32`, `i64`, `u8`, `u32`, `u64`, `usize` | `Int`        | `get_int()`                        |
| `f32`, `f64`                              | `Float`      | `get_float()`                      |
| `PathBuf`                                 | `Path`       | `get_string()` + `PathBuf::from()` |
| `Option<T>`                               | (inner type) | Optional extraction                |

#### Attribute Reference

**Namespace-level** (on the enum):

| Attribute             | Required | Description                                     |
| --------------------- | -------- | ----------------------------------------------- |
| `namespace = "..."`   | Yes      | The command namespace name                      |
| `description = "..."` | No       | Namespace description (defaults to doc comment) |

**Operation-level** (on enum variants):

| Attribute                      | Required | Description                 |
| ------------------------------ | -------- | --------------------------- |
| `effect = "pure\|write\|exec"` | Yes      | Side-effect classification  |
| `upgrade_gate`                 | No       | Requires upgrade gate check |
| `description = "..."`          | No       | Override doc comment        |

**Argument-level** (on struct fields):

| Attribute             | Required | Description                                         |
| --------------------- | -------- | --------------------------------------------------- |
| `long`                | No       | Expose as `--field-name` (default for named fields) |
| `short = 'x'`         | No       | Short flag alias                                    |
| `positional`          | No       | Positional argument (no `--` prefix)                |
| `flag`                | No       | Boolean flag (no value)                             |
| `default = "..."`     | No       | Default value if not provided                       |
| `optional`            | No       | Mark as optional (also inferred from `Option<T>`)   |
| `description = "..."` | No       | Override doc comment                                |

### CLI Help Rendering

`--help` renders human-readable text from the spec with effect annotations. The effect tag (`[pure]`/`[write]`/`[exec]`) is always shown — it tells the caller at a glance whether a command is safe to run.

**Namespace level** (`exo tdd --help`):

```
tdd — Manage TDD cycles

Commands:
  new     Start a new TDD cycle for a task          [write]
  red     Confirm the test is failing (red phase)   [write]
  green   Confirm the test is passing (green phase)  [write]
```

**Operation level** (`exo tdd new --help`):

```
tdd new [write] — Start a new TDD cycle for a task

Arguments:
  --name, -n <string>    The task selector (required)
  --test, -t <string>    The test command or file (required)
```

**Root level** (`exo --help`):

```
exo — Exosuit CLI

Namespaces:
  epoch      Manage epochs
  phase      Manage the current phase
  tdd        Manage TDD cycles
  ...

Commands:
  status     Show project status                     [pure]
  map        Show the project map                    [pure]
```

Design principles:

- No banners, no "USAGE:" ceremony, no "[OPTIONS]" noise
- Effect annotations always visible — `[pure]` means safe for orientation, `[write]` means state mutation, `[exec]` means external process
- `--format json` makes `--help` return the raw `HelpResult` as JSON (machine channel)
- Required/optional status shown inline with args

### Registry Wiring

`CommandSpec::from_registry()` calls `merge_exospec::<T>()` for each migrated namespace. This replaces the namespace entry with the macro-generated `NamespaceSpec`, preserving LM tool metadata from existing entries. During migration, both paths coexist — unmigrated namespaces use `Command::args()`, migrated ones use `HasExoSpec::spec()`.

### Infrastructure Commands

Four commands bypass spec-driven routing because they have special requirements:

| Command        | Reason                                                    |
| -------------- | --------------------------------------------------------- |
| `json server`  | Persistent subprocess mode, special stdin/stdout handling |
| `init`         | Runs without valid agent context                          |
| `merge-driver` | Runs without valid agent context, special exit codes      |
| `validate`     | Delegates to `exohook` subprocess                         |

These are detected by pattern-matching raw argv before `compile_argv()` runs.

## Migration Strategy

### Principle: Keep the Wheels On

Every step leaves the system working. The macro and manual `args()` coexist during migration. All 454+ tests pass after every change.

### Goal 1: ExoSpec Crate + First Namespace ✅

**Completed** (commit `695498d7`). Scaffolded `crates/exospec/` and migrated `tdd` namespace end-to-end:

- [x] `#[derive(ExoSpec)]` proc macro with full attribute grammar
- [x] `HasExoSpec` trait (`fn spec() -> NamespaceSpec`)
- [x] `from_invocation()` constructor generation
- [x] `merge_exospec<T>()` registry wiring
- [x] `TddCommands` migrated with parity tests verifying macro output matches hand-written specs
- [x] 20 exospec tests + 454 exo tests pass, zero clippy warnings

### Goal 2: Remove Clap (in progress)

**Goal**: Clap removed from dispatch path. CLI parses raw argv through `compile_argv()`.

- [ ] Extract `--format` from raw argv (already have `filtered_argv()` + `strip_flag_with_value()`)
- [ ] Handle `--version` (detect in argv, print `env!("CARGO_PKG_VERSION")`, exit)
- [ ] Detect 4 infrastructure commands by pattern-matching raw argv
- [ ] Handle `--help` via `help_for_address()` with text rendering (effect annotations)
- [ ] Replace `Cli::parse()` with raw argv → `compile_argv()` → `Invocation`
- [ ] Remove `clap` dependency from `tools/exo/Cargo.toml`
- [ ] Delete `Cli` struct, `Commands` enum, and all Clap subcommand enums (~1000 lines)
- [ ] Verify: all existing tests pass, CLI behavior unchanged

### Goal 3: Migrate Simple Namespaces

Migrate namespace-by-namespace. For each: add `#[derive(ExoSpec)]`, verify parity, wire into registry.

- [ ] `epoch` (8 operations)
- [ ] `criteria` (5 operations)
- [ ] `strike` (3 operations)
- [ ] `task` (8 operations)

### Goal 4: Migrate Complex Namespaces

- [ ] `phase` (8 operations, ordering args)
- [ ] `goal` (7 operations)
- [ ] Remaining: `rfc`, `plan`, `inbox`, `idea`, `axiom`, `commit`, `context`, `gc`, `run`, `feedback`, `docs`, `json`, `toml`, `ai`, `verify`, `write`
- [ ] Root commands (`status`, `map`) — needs `#[exo(root)]` support

### Goal 5: Delete Legacy

After all namespaces migrated:

- [ ] Delete `Command::args()` trait method (delegate to `HasExoSpec`)
- [ ] Delete legacy `CommandSpec` from `load_command_spec()`
- [ ] Delete `clap_commandspec_parity.rs`
- [ ] Delete `lm_tool_metadata.rs` (metadata moves to `#[exo(...)]` attributes)
- [ ] Remove `needs_upgrade_gate()` trait method
- [ ] Switch artifact generation to `build.rs`
- [ ] Delete `LIFECYCLE_OPERATIONS`, `TDD_OPERATIONS`, `IMPL_OPERATIONS` from TypeScript
- [ ] Update `docs/manual/architecture/command-trait-architecture.md`
- [ ] Promote this RFC to Stage 2

## Scope Boundaries

Orthogonal concerns addressed by other RFCs:

- **Machine Channel protocol** → RFC 0097
- **LM Tool grouping / ToolSets** → RFC 0136
- **CLI argument conventions** → RFC 0200 (ExoSpec makes enforcement compile-time)
- **Command Trait execution model** → RFC 0085 (ExoSpec changes metadata, not execution)

## Success Criteria

1. **Zero manual mapping lists** in TypeScript — all tool metadata from `command-spec.json`
2. **One definition per command** — no `args()` + Clap duplication
3. **Short flags work everywhere** — router uses the same spec as the artifact
4. **Adding a command** = `#[derive(ExoSpec)]` enum variant + `#[exo(...)]` + `execute_mut()`
5. **`--help` shows effect annotations** — `[pure]`/`[write]`/`[exec]` on every operation
6. **No Clap dependency** — removed from `tools/exo/Cargo.toml`

## Alternatives Considered

1. **More parity tests / manual list validation** — Rejected: band-aids don't compound. Each new check is a new maintenance surface.
2. **Runtime reflection via Clap introspection** — Rejected: Clap's API doesn't expose custom attributes (effect, upgrade_gate, LM metadata).
3. **External DSL (TOML/YAML)** — Rejected: loses compile-time safety, adds indirection.
4. **Keep dual source, improve tooling** — Rejected: the problem is having two sources. No tooling eliminates drift.
5. **Keep Clap alongside ExoSpec** — Rejected: Clap's parse result is already discarded. Keeping it means parsing undocumented Clap attribute syntax, running two parsers per invocation, and getting Clap error messages for failures the spec-driven router handles differently.

## Unresolved Questions

1. **Root commands**: `status` and `map` aren't in a namespace enum. Likely: `#[exo(root)]` attribute on individual structs.
2. **Build.rs vs. separate binary**: Leaning toward `build.rs` with `rerun-if-changed` for artifact generation.
3. **LM tool description source**: Leaning toward `#[exo(...)]` attributes with an override mechanism for non-Rust contributors.
