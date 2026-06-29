<!-- exo:132 ulid:01kmzxbcxn3jzfdsy1bf14z4dm -->

# RFC 132: CLI Patterns: Command Spec, Router, and Tool-Safe DSL


# RFC 0132: CLI Patterns: Command Spec, Router, and Tool-Safe DSL

## Summary

Define a Rust-first, generic "CLI Patterns" crate that treats a CLI as a _command specification_ (the law) and treats parsing as a _compiler_ from CLI-shaped inputs into a typed `Invocation` AST plus structured diagnostics.

This enables:

- A safe execution model (`argv` + explicit `stdin/env/cwd`, never a shell)
- An LLM-friendly tool surface (presence-based JSON "CLI AST" and/or a tiny bash-like DSL frontend)
- First-class steering diagnostics (prompt-like errors + concrete suggestions), driven by the same command spec
- Multiple projections from one source of truth: CLI runner/help, VS Code LM tool schema, docs, completion

This RFC is Stage 1: it establishes the direction, core types, invariants, and an initial DSL grammar sketch.

## Motivation

Exosuit needs a tool interface that preserves the advantages of CLI cognition (verb-first, flags, subcommands) without inheriting the hazards and ambiguity of shell evaluation.

Key tensions:

- **Ergonomics vs. Safety**: Shell syntax is ergonomic but unsafe/ambiguous; pure JSON is safe but can drift away from how humans/LLMs naturally express commands.
- **One Source of Truth**: Today, CLI parsing, tool schemas, docs, and steering live in different places and can diverge.
- **Agent-First Recovery**: Agents need errors that _teach recovery_ (structured, local, actionable), not just "invalid input".

This RFC proposes a core, reusable crate that lets projects define commands once and derive everything else.

## Detailed Design

### Terminology

- **Command Spec**: A reflectable, machine-checkable description of the command tree (subcommands, flags, args, types, constraints).
- **Invocation AST**: A generic, typed representation of a specific command invocation after routing + parsing.
- **Frontend**: A surface syntax that can be compiled into an Invocation (e.g. argv tokens, tool JSON, tiny DSL).
- **Projection/Backend**: A derived output from the command spec (help text, JSON schema, completion, etc.).
- **Idiom**: A declared equivalence between common shell idioms and safe command/flag patterns (e.g. `head`, `grep`, `2>&1`). Idioms power _suggestions_, not silent rewrites.

### Core Principle: Spec is Law; Parsers are Compilers

This crate treats the CLI as a well-defined language:

- **Law**: The command spec is the authoritative definition.
- **Compiler**: Parsing produces (1) a typed `Invocation` and (2) structured diagnostics/suggestions.
- **No Shell**: The compiler never evaluates shell operators, expansions, or pipelines.

This aligns with Exosuit axioms:

- **Agent-First Tooling**: diagnostics are prompts; errors are recoverable.
- **Tooling Independence**: core logic lives in the workspace/Rust core; editors are projections.
- **Generative over Descriptive**: one mental model (spec → compile → execute/projection) replaces a list of ad-hoc rules.

### Core Data Model (Rust)

This RFC is intentionally data-first. Traits exist for execution/extensibility, but the authoritative shape is reflectable.

#### `CommandSpec`

A command spec describes a tree of commands.

- Each command has:
  - `name` (the token the user types)
  - `about` (short help)
  - `args` (flags/options/positionals)
  - `children` (subcommands)
  - constraints (mutual exclusivity, required args, defaults)
  - optional idioms (see below)

#### Argument Shapes

Three canonical forms:

- **Flag**: boolean presence (`--verbose`)
- **Option**: key/value (`--limit 20`, `--limit=20`)
- **Positional**: ordered values (`exo list tasks` where `tasks` is a positional)

Arguments are identified by stable IDs (not just names) so diagnostics and projections can remain stable across renames.

#### Value Model

Values are typed _at parse time_.

- `Value` is an enum of primitives (initial set):
  - `Bool`, `Int`, `Float`, `String`, `Path`, `Json`, `Enum(Symbol)`
- Parsing uses a pluggable `ValueParser` trait.

This keeps errors local: a bad `--limit` is a type error, not a downstream runtime error.

#### Span / Source Metadata

To support rich diagnostics across different frontends, parsing preserves a full-fidelity source model:

- Original input (string or token array) is captured as a `Source` handle
- Tokens carry:
  - token index
  - byte spans (when originating from a string frontend)
  - normalized vs original lexeme (for repairs)

The guiding rule: _store more metadata than you need_ so future UX can improve without changing the semantic core.

### Invocation AST

The parsed output is a generic representation, not Exosuit-specific.

A minimal shape:

- `Invocation { path, args, occurrences }`
  - `path`: the resolved command path (`["list", "tasks"]`)
  - `args`: map from `ArgId` → typed `Value`
  - `occurrences`: map from `ArgId` → count (for repeatable flags)

This AST can be:

- lowered into `argv` for execution
- serialized into tool JSON (presence-based CLI AST)
- used to generate structured summaries

### Diagnostics + Steering

Diagnostics are a first-class output, designed to guide an agent.

A diagnostic includes:

- `code`: stable identifier (`unknown-flag`, `unsupported-shell-feature`, `missing-required-arg`)
- `message`: human/agent facing summary
- `span`: where it occurred
- `context`: structured fields (expected flags, command path)
- `suggestions`: concrete fixes (if available)

#### Idioms as Declarative Suggestion Hooks

The command spec may declare idioms such as:

- `Idiom::Head { limit_arg: ArgId }`
- `Idiom::Grep { pattern_arg: ArgId }`
- `Idiom::StderrToStdout { capture_arg: ArgId }`

When the frontend input contains shell-like operators (`| head`, `2>&1`), the compiler:

1. Emits `unsupported-shell-feature`
2. If an idiom mapping exists _in the active command context_, emits a suggestion:
   - "Pipelines are not supported; use `--limit` instead" (only when declared)

Normative rule: idioms produce **suggestions**, not automatic rewrites, unless the mapping is explicitly defined as lossless.

### Namespace Organization Guidelines

When organizing commands in a CommandSpec tree:

1. **Top-level namespaces** represent major functional areas
   - Examples: `phase`, `plan`, `rfc`, `task`, `impl`
   - These map naturally to LM tool boundaries

2. **Intermediate namespaces** group related operations
   - Examples: `feedback.thread`, `docs.links`
   - Use sparingly; prefer 1-2 levels of nesting

3. **Leaf operations** perform atomic actions
   - Examples: `phase.start`, `rfc.create`, `task.complete`
   - Each leaf declares its effect (pure/write/exec)

#### Naming Conventions

- **Namespaces**: lowercase singular nouns (`phase`, `task`, `rfc`)
- **Operations**: lowercase verbs (`start`, `create`, `list`, `update`)
- **Multi-word identifiers**: hyphenated (`add-task`, `update-status`)
- **Argument IDs**: stable identifiers that survive renames

#### Effect Consistency

Operations within a namespace may have different effects, but should be semantically related:

- ✅ `phase` namespace with `status` (pure) and `start`/`finish` (exec)
- ✅ `rfc` namespace with `list`/`show` (pure) and `create`/`promote` (write)
- ❌ Mixing unrelated domains in one namespace

#### LM Tool Mapping Considerations

When the CommandSpec will be projected to LM tools:

1. **Keep tool count low**: OpenAI recommends fewer than 20 functions
2. **Use method-based dispatch**: Group operations by namespace into single tools
3. **Preserve type safety**: Use enums for method parameters
4. **Enable discovery**: Help ladder allows progressive disclosure

Example tool projection from CommandSpec:

```
CommandSpec:
  phase/
    start (exec)
    finish (exec)
    status (pure)

LM Tool:
  exo_phase_ops { method: "start"|"finish"|"status", id?: string }
```

### Tool Projection Strategy (RFC 0136)

When projecting CommandSpec namespaces to LM tools, follow [RFC 0136: LM Tool Architecture v2](0136-lm-tool-architecture-v2.md) for the three-tier taxonomy.

#### Namespace → Tool Category Mapping

| CommandSpec Pattern                | Tool Category         | Projection                              |
| ---------------------------------- | --------------------- | --------------------------------------- |
| Leaf with `effect=pure`, zero args | Zero-arg orientation  | Direct tool (e.g., `exo-status`)        |
| Namespace with multiple operations | Method-based dispatch | Enum-based tool (e.g., `exo-phase-ops`) |
| High-frequency `effect=write` leaf | Convenience zero-arg  | Wrapper tool (e.g., `exo-idea`)         |

#### Zero-Arg Operation Detection in CommandSpec

An operation qualifies for zero-arg orientation if:

```toml
[[commands]]
name = "status"
effect = "pure"              # Must be pure (no side effects)
about = "Project health summary"

# All arguments optional or have defaults:
[[commands.args]]
name = "format"
type = "enum"
default = "json"
optional = true
```

**Projection**: This becomes `exo_status` tool with no required parameters.

#### Method-Based Dispatch in CommandSpec

A namespace qualifies for method-based dispatch if it contains multiple related operations:

```toml
[[commands]]
name = "phase"
about = "Phase lifecycle operations"

[[commands.children]]
name = "start"
effect = "exec"

[[commands.children]]
name = "finish"
effect = "exec"

[[commands.children]]
name = "status"
effect = "pure"
```

**Projection**: This becomes `exo_phase_ops` tool with:

```json
{
  "method": { "enum": ["start", "finish", "status"] }
}
```

#### Effect Annotations Guide Tool Design

| Effect  | Requires Confirmation      | Response Pattern               |
| ------- | -------------------------- | ------------------------------ |
| `pure`  | Never                      | State + steering               |
| `write` | Always (unless overridden) | Success + undo info + steering |
| `exec`  | Always                     | Status + next steps + steering |

The tool projection layer reads these annotations from CommandSpec to auto-generate confirmation requirements.

### Routing Layer (clap-like, spec-driven)

Routing resolves tokens to a command path:

- first token chooses a child command
- subsequent tokens are consumed as:
  - subcommand names (when unambiguous)
  - flags/options
  - positionals

Routing must be deterministic:

- ambiguity is an error with a diagnostic listing the competing interpretations
- unknown subcommands/flags trigger suggestion diagnostics (edit distance / prefix match)

### Frontends

This crate supports multiple input frontends that compile into the same Invocation.

#### Frontend A: Token Array

Input is a token array (argv-like) with no quoting ambiguity.

This is the simplest and safest model for tools.

#### Frontend B: Tool JSON (Presence-Based CLI AST)

A tool JSON surface mirrors CLI subcommands:

- `{ "list": { "kind": "tasks", "limit": 20 } }`

This is a serialization of an invocation AST into JSON.

#### Frontend C: Tiny bash-like DSL (string)

A minimal shell-looking DSL is allowed only as a frontend that compiles deterministically.

- It is _not_ POSIX shell.
- It rejects shell operators and expansions.
- It supports conservative repairs for LLM-typical syntax errors.

### DSL Grammar Sketch (EBNF)

The DSL is designed to "vibe" like a shell while being far smaller.

```
input        := ws? command ws? EOF
command      := word (ws+ word)*

word         := bare | single_quoted | double_quoted | interpolation

bare         := bare_char+

single_quoted := "'" sq_char* "'"
# No escapes inside single quotes.

double_quoted := '"' dq_part* '"'
# double quotes allow a limited escape set and interpolation.

dq_part      := dq_char | escape | interpolation
escape       := "\\" ("\\" | "\"" | "n" | "t")

interpolation := "$" ident
ident        := ident_start ident_continue*
ident_start  := [A-Za-z_]
ident_continue := [A-Za-z0-9_]

ws           := (" " | "\t" | "\n" | "\r")+
```

#### DSL Semantics

- The compiler produces a token array.
- `$name` interpolation is _single-token substitution_:
  - it must be provided explicitly by the caller via `vars[name]`
  - otherwise it is a diagnostic error
- The following are **always errors** (with steering):
  - backticks, `$(`, `${...}`, `;`, `&&`, `||`, `|`, `<`, `>`, `2>&1`

#### Conservative Repairs (Normative)

The DSL frontend may apply only repairs that do not change intent:

- normalize Unicode quotes to ASCII quotes
- if input ends inside an open quote, close it at EOF
- treat newlines as whitespace outside quotes
- normalize `--flag=value` and `--flag value` equivalently

Any other malformed syntax produces a diagnostic without guessing.

### Projections / Backends

From the same command spec, we derive:

- **CLI runner**: native parsing via the router + execution via handlers
- **Tool schema**: JSON Schema for the presence-based tool input
- **Help text**: `--help` output from spec
- **Completion**: (future) completions generated from spec
- **Transitional clap backend** (optional): generate a clap program from `CommandSpec` for migration/testing

### Execution API Surface

The spec should be independent from execution.

A minimal interface:

- `compile(frontend_input) -> Result<Invocation, Diagnostics>`
- `lower(invocation) -> SpawnSpec { argv, stdin, env, cwd }`
- `execute(invocation, host) -> Result<ExitStatus, CommandError>` (host-defined)

Execution uses `spawn(argv)`; never a shell.

### Implementation Target: Command Trait (RFC 0085)

The `CommandSpec` vision is implemented through the `Command` trait architecture defined in [RFC 0085](../stage-3/0085-command-trait-architecture.md) (Stage 3: Candidate).

The trait provides the runtime execution target:

```rust
pub trait Command: Send + Sync {
    fn namespace(&self) -> &'static str;
    fn operation(&self) -> &'static str;
    fn effect(&self) -> Effect;
    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput>;
}
```

**Relationship to CommandSpec**:

1. **CommandSpec is derived from trait implementations**: The `CommandRegistry` can be introspected to generate `CommandSpec` automatically, ensuring spec and implementation cannot diverge.

2. **Effect annotations flow from trait to spec**: Each `Command::effect()` implementation provides the effect metadata required by RFC 0125's capability tree.

3. **Namespace/operation structure matches addressing**: The trait's `namespace()` + `operation()` directly maps to the capability tree's addressing model (`["phase", "start"]`).

4. **Diagnostics and steering are unified**: The `Command::default_steering()` method provides the suggestions referenced in the diagnostics model.

### Customer Zero: Exosuit Adoption

- **Phase 0** (RFC 0085, complete): `Command` trait architecture implemented, existing handlers migrated to trait-based commands.
- **Phase 1**: Generate `CommandSpec` from `CommandRegistry` introspection.
- **Phase 2**: Route/parse in the CLI using the new router (with clap backend optional).
- **Phase 3**: Unify steering diagnostics across CLI + VS Code tool by sharing diagnostic codes and suggestion rules.

## Drawbacks

- Replacing clap is non-trivial: help UX, parsing edge cases, and completion require careful work.
- A DSL frontend increases scope; keeping it minimal and deterministic is essential.
- Declared idioms must be curated to avoid "suggestion spam" or misleading hints.

## Alternatives

- **Stay on clap + custom tool schema**: simpler short term, but divergence persists.
- **Pure JSON only**: safest, but loses CLI-shaped cognition and increases translation errors.
- **Raw cmdline string**: ambiguous; reintroduces quoting and temptation toward shell semantics.
- **Adopt nushell syntax**: richer types but imports an entire language surface and expectations (pipelines).

## Unresolved Questions

- Exact stability/versioning guarantees for diagnostic codes (needed for crates.io consumers).
- The minimal initial set of `Value` primitives and how JSON/path parsing should behave.
- How aggressively to suggest corrections (edit distance thresholds).
- Whether the clap backend is required for initial migration or purely optional.

## Future Possibilities

- Proc-macro DSL for ergonomically declaring `CommandSpec`.
- Pipelines as data (safe, spec-driven transforms) rather than shell pipelines.
- Richer value types (bytes, duration, glob, regex) with explicit parsers.
- Auto-generated docs/manual sections derived from spec (codification).

---

## Implementation Note: Inline Spec Definition (2026-02-02, updated 2026-02-05)

The "CommandSpec is derived from trait implementations" approach described in the "Implementation Target" section is being **superseded** by **Inline Spec Definition** via the `ExoSpec` derive macro.

### What Changed

The original plan was:

1. Define commands via `Command` trait implementations
2. Generate `CommandSpec` from `CommandRegistry` introspection at runtime

The new approach instead:

1. Defines CommandSpec **inline** using Clap annotations + `#[exo(...)]` custom attributes
2. Extracts CommandSpec at **compile time** via the `ExoSpec` proc-macro
3. Eliminates the need for separate `Command::args()` trait implementations

### Impact on This RFC

- **Section "Implementation Target: Command Trait"**: The relationship described remains accurate for _execution_, but CommandSpec metadata will come from inline attributes rather than trait introspection.
- **Section "Customer Zero: Exosuit Adoption"**: Phase 1 ("Generate CommandSpec from CommandRegistry introspection") is replaced by proc-macro extraction.
- **Section "Future Possibilities"**: "Proc-macro DSL for ergonomically declaring CommandSpec" is the ExoSpec macro.

**See Also**: [RFC 00233: ExoSpec — Unified Command Definition](../stage-1/00233-exospec-unified-command-definition-and-the-end-of-dual-source-drift.md) for the consolidated design and migration plan. (Supersedes the earlier RFC 0201 which covered only the macro design.)
