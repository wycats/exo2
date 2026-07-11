<!-- exo:135 ulid:01kg5kp2hp95esmbdatzsp46er -->

# RFC 135: CommandSpec Unification (Narrow Conduit Architecture)

- **Status**: Withdrawn
- **Stage**: 3
- **Reason**:

# RFC 0135: CommandSpec Unification (Narrow Conduit Architecture)

> **⚠️ Superseded by [RFC 00233: ExoSpec — Unified Command Definition](../stage-1/00233-exospec-unified-command-definition-and-the-end-of-dual-source-drift.md)**
>
> This RFC accumulated multiple contradictory architecture layers over time (Hybrid with Validation → Inline Spec Definition → Transport Abstraction). RFC 00233 consolidates the vision into a single coherent document that reflects current reality and provides a concrete migration path. The core insight of this RFC — "CommandSpec is the single source of truth" — remains correct and is carried forward.

## Summary

Unify CLI and LM tool implementations through a "Narrow Conduit" architecture where **CommandSpec is the single source of truth** for both. A build-time **Schema Artifact** bridges Rust and TypeScript, eliminating manual schema duplication and ensuring perfect parity between CLI and LM tools.

## Motivation

Currently, we have:

1. **Rust CommandSpec**: Defines CLI commands, args, and routing
2. **TypeScript LM Tools**: Manually duplicate schema definitions in Zod
3. **Two Execution Paths**: Tools shell out to CLI (`runExoCommand`)

**Problems:**

- Schema drift: CLI adds arg, TS tool not updated
- Duplication: 14 tools × 2 implementations = maintenance burden
- Parity testing impossible without shared source of truth
- No machine-readable contract between Rust and TS

**Vision:** "Spec is Law" - CommandSpec generates everything.

## Detailed Design

### Architecture: The Narrow Conduit

```
┌─────────────────────────────────────────────────────────────────────┐
│                         CommandSpec (Rust)                          │
│    The Single Source of Truth for ALL command schemas               │
└──────────────────────────────┬──────────────────────────────────────┘
                               │
                         Build Time
                               │
                               ▼
           ┌───────────────────────────────────────┐
           │         Schema Artifact (JSON)        │
           │   • Command metadata                  │
           │   • Argument schemas (JSON Schema)    │
           │   • Effect annotations (pure/exec)    │
           │   • Idiom references                  │
           │   • Intent keywords                   │
           └──────────────────────┬────────────────┘
                                  │
                                  │
           ┌──────────────────────┴───────────────────┐
           │                                          │
           ▼                                          ▼
┌──────────────────────────┐            ┌─────────────────────────────┐
│    CLI Router (Rust)     │            │   LM Tool Registry (TS)     │
│  • Parses argv tokens    │            │  • Consumes schema artifact │
│  • Routes to Commands    │            │  • Generates Zod schemas    │
│  • Human/JSON output     │            │  • Registers tools at init  │
│                          │            │  • Invokes CLI or Machine   │
│                          │            │    Channel                  │
└──────────────────────────┘            └─────────────────────────────┘
```

### Phase 1: Argument Metadata Capture

**Problem:** `CommandSpec::from_registry()` doesn't populate argument metadata.

**Solution:** Extend `Arg` struct to capture:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct Arg {
    pub name: String,
    pub short: Option<char>,
    pub long: Option<String>,
    pub help: String,
    pub required: bool,
    pub takes_value: bool,
    pub default: Option<String>,
    pub value_type: ValueType,  // NEW
    pub intent_keywords: Vec<String>,  // NEW
}

#[derive(Debug, Clone, Serialize)]
pub enum ValueType {
    String,
    Number,
    Boolean,
    Path,
    PhaseId,
    TaskId,
    // Semantic types for validation
}
```

**Task:** Update `from_clap_command()` to extract arg metadata from clap's `Arg` type.

### Phase 2: Schema Artifact Generation

**Location:** Build script (`build.rs` or standalone `exo schema generate`)

**Output:** `target/schema-artifact.json`

```json
{
  "$schema": "https://exosuit.dev/schemas/commandspec-v1.json",
  "version": "1.0.0",
  "generated_at": "2025-01-15T12:00:00Z",
  "commands": {
    "status": {
      "path": ["status"],
      "effect": "pure",
      "description": "Returns current project status",
      "args": [],
      "intent_keywords": ["status", "where am I", "what's happening"],
      "lm_tool": {
        "name": "exo-status",
        "tier": "zero-arg",
        "use_when": "User asks about current status or needs orientation",
        "do_not_use_when": "User needs detailed task breakdown (use exo-phase)"
      }
    },
    "phase": {
      "path": ["phase"],
      "subcommands": {
        "status": {
          "path": ["phase", "status"],
          "effect": "pure",
          "description": "Show current phase details",
          "args": [
            {
              "name": "full",
              "type": "boolean",
              "required": false,
              "default": false,
              "description": "Include full task list"
            }
          ]
        },
        "start": {
          "path": ["phase", "start"],
          "effect": "exec",
          "description": "Start a new phase",
          "args": [
            {
              "name": "id",
              "type": "string",
              "required": true,
              "description": "Phase ID (e.g., 110)"
            }
          ]
        }
      }
    }
  }
}
```

**CI Integration:** Generate on every build, commit to repo for IDE consumption.

### Phase 3: TypeScript Schema Consumption

**Location:** `packages/exosuit-vscode/src/lmtool/schema-loader.ts`

```typescript
import schemaArtifact from "../../../target/schema-artifact.json";

export function generateZodSchema(commandPath: string[]): z.ZodSchema {
  const command = resolveCommand(schemaArtifact.commands, commandPath);
  return buildZodFromArgs(command.args);
}

export function getToolMetadata(toolName: string): ToolMetadata {
  // Find command with matching lm_tool.name
  return findByToolName(schemaArtifact.commands, toolName);
}
```

**Build Integration:**

1. Schema artifact generated in Rust build
2. TypeScript imports as JSON module
3. Zod schemas generated at runtime or build-time

### Phase 4: Tool JSON Frontend

**Purpose:** Allow CommandSpec to be defined declaratively in TOML (future DSL).

**Format (CommandSpec TOML):**

```toml
[[commands]]
name = "status"
namespace = "root"
effect = "pure"
description = "Returns current project status"

[commands.lm_tool]
name = "exo-status"
tier = "zero-arg"
use_when = "User asks about current status"
do_not_use_when = "User needs detailed task breakdown"

[[commands]]
name = "phase"
namespace = "phase"

[[commands.subcommands]]
name = "start"
effect = "exec"
description = "Start a new phase"

[[commands.subcommands.args]]
name = "id"
type = "string"
required = true
```

**Migration Path:**

1. Current: Hand-coded `Command` trait implementations
2. Near-term: TOML declarations + generated implementations
3. Future: DSL with richer validation and idioms

### Phase 5: Execution Path Unification

**Current State:** LM tools shell out to CLI via `runExoCommand()`.

**Target State:** Shared execution path through Machine Channel.

```typescript
// Current (shell out)
const { stdout } = await runExoCommand("status --format json");
return JSON.parse(stdout);

// Future (Machine Channel)
const result = await machineChannel.invoke({
  command: ["status"],
  format: "json",
});
return result;
```

**Benefits:**

- Faster: No process spawn overhead
- Structured: No stdout parsing
- Testable: Mock the channel, not shell execution

**Implementation:** Machine Channel v2 with WASM or FFI bridge.

### LM Tool → CLI Mappings

| LM Tool         | CLI Command                       | Format Flag     | Fallback | Status                           |
| --------------- | --------------------------------- | --------------- | -------- | -------------------------------- |
| `exo-status`    | `exo status`                      | `--format json` | Human    | ✅ Working                       |
| `exo-plan`      | `exo plan`                        | `--format json` | Human    | ✅ Working                       |
| `exo-phase`     | `exo phase status`                | `--format json` | Human    | ⚠️ Currently calls wrong command |
| `exo-steering`  | `exo map`                         | `--format json` | Human    | ⚠️ Rename from exo-map           |
| `exo-context`   | `exo ai context`                  | `--format json` | Human    | ✅ Working                       |
| `exo-inbox`     | `exo inbox`                       | None            | Human    | ⚠️ No JSON support               |
| `exo-idea`      | `exo idea add`                    | N/A             | N/A      | ✅ Working (mutation)            |
| `exo-add-task`  | `exo task add`                    | N/A             | N/A      | ✅ Working (mutation)            |
| `exo-phase-ops` | `exo phase {start,finish,status}` | Varies          | Human    | ❌ Missing tool                  |
| `exo-task-ops`  | `exo task {add,complete,list}`    | Varies          | Human    | ❌ Missing tool                  |
| `exosuit`       | Machine Channel v1                | Structured      | N/A      | ✅ Working                       |

## Implementation Plan (Stage 2)

### Milestone 1: Argument Metadata Capture (2-3 days)

- [ ] Extend `Arg` struct with `ValueType` and `intent_keywords`
- [ ] Update `from_clap_command()` to extract arg metadata
- [ ] Add unit tests for arg extraction
- [ ] Document arg metadata schema

### Milestone 2: Schema Artifact Generation (2 days)

- [ ] Create `exo schema generate` command
- [ ] Generate JSON Schema for schema artifact
- [ ] Add to build process (build.rs or CI)
- [ ] Commit artifact to repo

### Milestone 3: TypeScript Schema Consumption (3 days)

- [ ] Create schema-loader.ts module
- [ ] Generate Zod schemas from artifact
- [ ] Update tool registration to use generated schemas
- [ ] Add parity tests (generated vs manual)

### Milestone 4: Fix Critical Issues (1 day)

- [ ] Fix `exo-phase` to call `phase status` not `phase show`
- [ ] Rename `exo-map` to `exo-steering`
- [ ] Add JSON support to `exo inbox`

### Milestone 5: Implement Missing Tools (3-4 days)

- [ ] Implement `exo-phase-ops` method-dispatch tool
- [ ] Implement `exo-task-ops` method-dispatch tool
- [ ] Update tool descriptions per RFC 0095 template

### Milestone 6: Execution Path Unification (4-5 days)

- [ ] Design Machine Channel v2 protocol
- [ ] Implement WASM or FFI bridge
- [ ] Migrate tools from `runExoCommand` to channel
- [ ] Deprecate shell-out path

### Milestone 7: Migrate All Commands (5-7 days)

- [ ] Migrate all 20+ commands to CommandSpec
- [ ] Generate tool registrations from artifact
- [ ] Remove manual tool definitions
- [ ] Final parity verification

### Testing Requirements

1. **Schema Generation Tests**
   - Verify artifact structure matches JSON Schema
   - Verify all commands have required metadata
   - Verify arg types are correctly extracted

2. **Parity Tests**
   - CLI JSON output matches tool JSON output
   - Schemas generated from artifact match manual schemas
   - Tool behavior identical regardless of execution path

3. **Round-Trip Tests**
   - TOML → CommandSpec → Schema Artifact → Zod → Validation
   - Ensure no information loss through pipeline

## Success Criteria

1. **100% Schema Coverage**: All CLI commands have corresponding schema entries
2. **Zero Manual Tools**: All LM tools generated from CommandSpec
3. **Parity Guaranteed**: CI fails if CLI/tool schemas diverge
4. **Single Maintenance Point**: Change CommandSpec, both CLI and tools update
5. **Execution Parity**: CLI `--format json` output matches machine channel response for all pure-effect operations
6. **Command Trait Coverage**: All user-facing commands implement Command trait (except documented infrastructure commands)

## Alternatives Considered

### Alternative 1: Protobuf Schemas

Use .proto files as source of truth instead of CommandSpec.

**Rejected:** Adds external dependency, Rust already has good serialization.

### Alternative 2: TypeScript as Source

Define schemas in TypeScript, generate Rust code.

**Rejected:** Rust is our systems layer, TypeScript is UI. Direction should be systems → UI.

### Alternative 3: Keep Dual Maintenance

Continue maintaining CLI and tools separately with manual sync.

**Rejected:** Already causing drift, will only get worse with more tools.

## Context Updates (Stage 3)

- [ ] Create `docs/manual/architecture/commandspec.md`
- [ ] Update `docs/manual/features/lm-tools.md` with generated tools
- [ ] Document schema artifact format in manual
- [ ] Update `docs/agent-context/plan.toml` to reflect completed unification

## Open Questions

1. **WASM vs FFI for Machine Channel v2?**
   - WASM: More portable, can run in web
   - FFI: Faster, direct memory sharing
   - **Leaning:** WASM for initial implementation, FFI as optimization

2. **Should schema artifact be committed or generated?**
   - Committed: IDE can consume without build
   - Generated: No drift, always fresh
   - **Leaning:** Committed, regenerated in CI, fail on drift

3. **DSL syntax for CommandSpec?**
   - TOML: Familiar, good tooling
   - Custom DSL: More expressive, learning curve
   - **Leaning:** TOML initially, DSL when patterns stabilize

## Implementation Architecture (Finalized 2026-01-12)

Based on Phase 12 design decisions, the following implementation approach has been selected:

### Hybrid Architecture

The CommandSpec unification uses a **Hybrid with Validation** approach that preserves the developer experience of Clap derive macros while ensuring perfect parity with LM tool schemas:

```
┌─────────────────────────────────────────────────────────────┐
│                     SOURCE OF TRUTH                         │
│                                                             │
│  ┌─────────────────┐          ┌─────────────────────────┐  │
│  │   Clap Derive   │          │  Command::args() trait  │  │
│  │   (CLI parsing) │◄────────►│   (LM tool metadata)    │  │
│  └────────┬────────┘   test   └───────────┬─────────────┘  │
│           │            parity             │                 │
└───────────┼───────────────────────────────┼─────────────────┘
            │                               │
            ▼                               ▼
    ┌───────────────┐              ┌───────────────────────┐
    │   CLI Router  │              │  build.rs generates   │
    │   (runtime)   │              │  command-spec.json    │
    └───────────────┘              └───────────┬───────────┘
                                               │
                                               ▼
                                   ┌───────────────────────┐
                                   │  TypeScript types +   │
                                   │  LM tool factory      │
                                   └───────────────────────┘
```

**Key Design Decisions:**

1. **Dual Source of Truth with Validation:**
   - Keep Clap derive macros for CLI parsing (compile-time safety, IDE support)
   - Add `fn args(&self) -> Vec<ArgSpec>` to Command trait for LM tool metadata
   - Validation test harness ensures parity: `assert_args_equivalent()`

2. **Validation Testing:**
   - Test harness: `tools/exo/tests/clap_commandspec_parity.rs`
   - Each command has a test verifying Clap args match trait args
   - CI enforces: new commands must pass validation
   - Implementation is staged: validate each namespace as `args()` is added

3. **Artifact Generation:**
   - Build script (`build.rs`) with `rerun-if-changed=src/command/`
   - Generates on every build, but only when command sources change
   - Minimal overhead: ~100ms when no changes, ~1-2s on first build
   - CI naturally validates (build fails if spec doesn't compile)

4. **Artifact Location:**
   - `packages/exosuit-vscode/src/lmtool/command-spec.json`
   - Close to TypeScript consumption point
   - Naturally tracked by git (not in target/)
   - Clean import path in TypeScript
   - Marked with `// @generated` header to prevent manual edits

5. **TypeScript Integration:**
   - JSON artifact → generate `.d.ts` script (Option 4a)
   - Perfect parity: types generated from runtime data
   - Two-step generation: Rust → JSON → TypeScript types
   - Script: `scripts/generate-command-types.ts`

6. **Mandatory Staged Validation:**
   - Validation harness implemented in Phase 12.0
   - Each namespace validated as `args()` is added
   - No shipping commands without validation tests
   - Prevents drift between CLI and LM tool schemas

### Spec Unification (Added 2026-01-25)

**Problem**: Two parallel spec systems exist:

- Legacy `command_spec.rs` (root level) - tree-based structure, no defaults, used by argv_compiler, help_gen, json_schema, tool_surface
- RFC-aligned `command/command_spec.rs` - namespace-based, has defaults, used by JSON parsing and router

This causes argv routing to skip default application, breaking parity between CLI and machine channel.

**Decision**: Migrate all consumers to RFC-aligned spec and delete legacy.

**Sub-decisions**:

1. **Shell Operator Handling**: Extract to standalone `shell_ops.rs` module. This is a CLI-only concern orthogonal to command spec structure.

2. **Tree-Walking Consumers**: Rewrite `json_schema.rs`, `help_gen.rs`, `tool_surface.rs` to use flat namespace iteration instead of tree traversal.

3. **Validation**: Factor spec validation as a standalone function that:
   - Runs at command registration (catches bugs early)
   - Can run retrospectively on spec files (`exo validate specs` for cleanup/debugging)

4. **Equivalence Testing**: Property-based tests verifying JSON and argv paths produce identical Invocations for the same logical input. Safety net before migration.

**Migration Order** (lowest to highest risk):

1. Extract shell operators to separate module
2. Add property-based equivalence tests
3. Migrate argv_compiler to RFC-aligned spec
4. Migrate help_gen, json_schema, tool_surface to flat model
5. Factor validation as standalone + cleanup tool
6. Delete legacy command_spec.rs

### Rationale

This hybrid approach balances:

- **Developer Experience:** Clap derive macros provide excellent ergonomics and compile-time safety
- **Parity Guarantee:** Validation tests catch any drift between CLI and LM tools
- **Flexibility:** Trait method can include LM-specific metadata not present in Clap
- **Maintainability:** ~10 lines overhead per command, but full type safety preserved

See [phase-12-decisions.md](../../agent-context/current/phase-12-decisions.md) for detailed investigation and tradeoff analysis.

---

## Implementation Note: Inline Spec Definition (2026-02-02)

The "Hybrid with Validation" approach described above has been **superseded** by **Inline Spec Definition**, which achieves true single-source-of-truth semantics.

### Clarification: "Single Source of Truth"

When this RFC states "CommandSpec is the single source of truth," it means the **interface contract** is canonical—not that a specific file location contains all definitions. The source of truth is the _declared specification_, regardless of where that declaration lives.

### Chosen Implementation: Inline Spec Definition

Rather than maintaining two parallel definitions (Clap derive + `Command::args()` trait) with validation tests to catch drift, we now define CommandSpec **inline** using Clap annotations extended with custom `#[exo(...)]` attributes.

**Approach:**

1. **Clap annotations remain primary** for CLI parsing (compile-time safety, IDE support)
2. **Custom `#[exo(...)]` attributes** extend Clap with LM-tool-specific metadata
3. **`ExoSpec` derive macro** extracts the complete CommandSpec at compile time
4. **No separate `Command::args()` implementation** required—the macro generates it

**Example:**

```rust
#[derive(Subcommand, ExoSpec)]
enum InboxCommands {
    /// Add a new inbox item
    #[exo(effect = "write")]
    Add {
        /// Subject line for the inbox item
        #[arg(short = 's', long)]
        subject: String,

        /// Category for triage
        #[arg(long, default_value = "guidance")]
        category: String,
    },
}
```

**Benefits over Hybrid Approach:**

| Aspect       | Hybrid (Previous)     | Inline (Current)          |
| ------------ | --------------------- | ------------------------- |
| Sources      | 2 (Clap + trait)      | 1 (Clap + attributes)     |
| Drift risk   | Mitigated by tests    | Eliminated by design      |
| Maintenance  | ~10 lines per command | ~2 attributes per command |
| Parity tests | Required              | Unnecessary               |

**See Also:** [RFC 0201](../stage-1/0201-exospec-derive-macro-inline-commandspec-definition.md) (ExoSpec Derive Macro) for detailed proc-macro design.

### Migration Path

1. **Phase 0**: Add `#[exo(...)]` attributes to existing Clap enums (parallel to legacy `Command::args()`)
2. **Phase 1**: Implement `ExoSpec` proc-macro
3. **Phase 2**: Generate `command-spec.json` from macro output
4. **Phase 3**: Remove legacy `Command::args()` implementations
5. **Phase 4**: Remove `clap_bridge.rs` mirror enums (no longer needed for parity)

---

## Transport Abstraction Architecture (2026-01-25)

The original RFC focused on schema unification but did not fully specify how CLI and machine channel share the **execution path**. This section completes the architecture.

### Core Insight

Both CLI and LM tools should dispatch through `Command::invoke_json()`, with transport-specific behavior abstracted via a `TransportContext` trait. The Command trait is the single source of truth for execution, not just schema.

### Architecture Diagram

```
         CLI Transport                    Machine Channel Transport
              │                                    │
              ▼                                    ▼
         argv parsing                         JSON input
         (Clap derive)                    (RequestEnvelope)
              │                                    │
              ▼                                    ▼
         argv → JSON                          already JSON
              │                                    │
              └──────────────┬─────────────────────┘
                             │
                             ▼
                    ┌─────────────────┐
                    │  Command Trait  │  ← Single execution path
                    │                 │
                    │  invoke_json()  │  ← Validates input, executes
                    │  effect()       │  ← Used for confirmation
                    │  args()         │  ← Schema for validation
                    └────────┬────────┘
                             │
                             ▼
                    ┌─────────────────┐
                    │ TransportContext│  ← Transport-specific behavior
                    │                 │
                    │ confirm_exec()  │  ← CLI: prompt, Machine: ticket
                    │ format_output() │  ← CLI: human/json, Machine: envelope
                    │ format_error()  │  ← CLI: stderr, Machine: ErrorBody
                    │ render_steering()│ ← CLI: help text, Machine: JSON
                    └────────┬────────┘
                             │
              ┌──────────────┴─────────────────────┐
              │                                    │
              ▼                                    ▼
     ┌─────────────────┐              ┌──────────────────────┐
     │  CLITransport   │              │ MachineChannelTransport│
     │                 │              │                      │
     │ human/json out  │              │ ResponseEnvelope     │
     │ stdin prompt    │              │ ticket flow          │
     └─────────────────┘              └──────────────────────┘
```

### Command::invoke_json() Contract

```rust
/// Execute a command from JSON input without argv parsing.
///
/// This is the unified execution path for both CLI and machine channel.
/// The transport calls this method after converting its input to JSON.
fn invoke_json(
    &self,
    input: &JsonValue,
    transport: &dyn TransportContext,
) -> Result<CommandOutput, CommandError>;
```

**Responsibilities:**

1. Validate `input` against `self.args()` schema
2. If `self.effect() == Effect::Exec`, call `transport.confirm_exec()`
3. Execute the command logic
4. Return structured `CommandOutput`

**Key property:** No argv parsing. JSON is validated directly against `ArgSpec`.

### TransportContext Trait

```rust
/// Abstraction for transport-specific behavior.
///
/// CLI and machine channel implement this differently, but Command
/// dispatch is identical for both.
pub trait TransportContext {
    /// Request confirmation for exec effects.
    /// CLI: prompt user on stdin
    /// Machine: return ConfirmRequired status with ticket
    fn confirm_exec(&self, action: &str) -> ConfirmResult;

    /// Format successful output for this transport.
    /// CLI: render human-readable or JSON based on --format flag
    /// Machine: wrap in ResponseEnvelope
    fn format_output(&self, output: CommandOutput) -> TransportOutput;

    /// Format error for this transport.
    /// CLI: print to stderr with exit code
    /// Machine: return ErrorBody in ResponseEnvelope
    fn format_error(&self, error: CommandError) -> TransportOutput;

    /// Render steering suggestions for this transport.
    /// CLI: "Try: exo phase start <id>"
    /// Machine: { "tool": "exo-phase-start", "tool_args": {...} }
    fn render_steering(&self, suggestions: Vec<SuggestedAction>) -> SteeringOutput;
}
```

### CLI Migration Path

Currently, CLI dispatch happens in `main.rs` via a large match statement and Clap parsing. After migration:

1. **Clap parses argv** to structured types (unchanged)
2. **Convert to JSON** using `serde_json::to_value()` on the parsed struct
3. **Call `command.invoke_json(json, &cli_transport)`**
4. **CLITransport renders output** to stdout/stderr

This means CLI and machine channel share the `invoke_json()` path, with only input conversion and output rendering differing.

### Confirmation Flow

| Transport | Effect::Pure | Effect::Write | Effect::Exec                       |
| --------- | ------------ | ------------- | ---------------------------------- |
| CLI       | Execute      | Execute       | Prompt "Continue? [y/N]"           |
| Machine   | Execute      | Execute       | Return `confirm_required` + ticket |

The Command trait calls `transport.confirm_exec()` for Exec effects. The transport decides how to confirm:

- CLI blocks on stdin
- Machine channel returns early with ticket, expects retry with auth

### Why This Matters

Without this abstraction:

- Machine channel routes JSON → argv → compile (current state)
- CLI and machine channel have different execution paths
- Transport-specific logic leaks into Command implementations

With this abstraction:

- Both transports call `invoke_json()` directly
- Commands are transport-agnostic
- Confirmation, output, steering are defined once, rendered twice

### Invocation Type Unification

**Problem:** There are two `Invocation` types in the codebase:

1. **`tools/exo/src/invocation.rs`** - Used by machine channel handler
   - Path: `Vec<String>`
   - Args: `BTreeMap<ArgId, Value>`
   - Uses `Value` enum (Bool, Int, Float, String, Path, Json, Enum)

2. **`tools/exo/src/command/router.rs`** - Used by spec-driven router
   - Path: `CommandPath { namespace, operation }`
   - Args: `BTreeMap<String, TypedValue>`
   - Uses `TypedValue` enum (Bool, Int, Float, String, Path, Json, Enum, Array)
   - Has `FromInvocation` trait for command construction

**Decision:** Unify on the `command/router.rs` `Invocation` type because:

1. It has `FromInvocation` trait infrastructure
2. `TypedValue` is richer (supports Array)
3. `CommandPath` is more structured than `Vec<String>`
4. The existing `FactoryRegistry` works with it

**Migration Path:**

1. Add `from_json(input: &JsonValue, spec: &OperationSpec) -> Result<Invocation>` to router.rs
2. Update machine channel handler to use `command/router.rs::Invocation`
3. Delete `tools/exo/src/invocation.rs` after migration
4. Update `FromInvocation` implementations if needed

**Timing Decision (2026-01-25):** Unify Invocation types _early_ in the implementation sequence, immediately after `Invocation::from_json()` is implemented. This avoids temporary bridging code between two Invocation types.

### Root Command Address Format

**Decision (2026-01-25):** Root commands use space-separated paths that match the CLI prefix exactly.

| CLI Command       | Address Path         | Tool Name         |
| ----------------- | -------------------- | ----------------- |
| `exo status`      | `["status"]`         | `exo-status`      |
| `exo map`         | `["map"]`            | `exo-steering`    |
| `exo phase start` | `["phase", "start"]` | `exo-phase-start` |

**Rationale: Correct by Construction**

The address path is the CLI command tokens after `exo`. This makes the mapping trivial:

```rust
// CLI to address
let address = argv[1..].to_vec();  // Skip "exo"

// Address to CLI
let cli_command = format!("exo {}", address.join(" "));
```

**Schema Implications:**

Root commands are simply operations with a single-element path. No special "root namespace" needed:

```json
{
  "namespaces": {
    "status": {
      "operations": {
        "": { ... }  // Empty string = the namespace IS the command
      }
    },
    "phase": {
      "operations": {
        "start": { ... },
        "finish": { ... }
      }
    }
  }
}
```

Or alternatively, model as top-level operations outside namespaces:

```json
{
  "root_operations": {
    "status": { ... },
    "map": { ... }
  },
  "namespaces": { ... }
}
```

**Chosen approach:** The `root_operations` model is cleaner—it explicitly separates root commands from namespaced ones, and the handler can check `root_operations` first before namespace lookup.

### JSON to Invocation Bridge

**Missing Infrastructure:** There's no JSON → `Invocation` parser. The current flow is:

```
JSON input → call_params_to_argv() → compile_argv() → Invocation
```

We need:

```
JSON input → Invocation::from_json(input, &operation_spec) → Invocation
```

This eliminates the argv intermediate step, which is the core of unification.

**Implementation:**

```rust
impl Invocation {
    /// Parse JSON input directly to Invocation using the operation spec.
    ///
    /// This is the unified entry point for both machine channel and CLI.
    /// CLI: argv → JSON → from_json()
    /// Machine: JSON → from_json()
    pub fn from_json(
        input: &JsonValue,
        namespace: &str,
        operation: &str,
        spec: &OperationSpec,
    ) -> Result<Self, RoutingDiagnostic> {
        let mut args = BTreeMap::new();

        for arg_spec in &spec.args {
            if let Some(value) = input.get(&arg_spec.name) {
                let typed = TypedValue::from_json(value, &arg_spec.value_type)?;
                args.insert(arg_spec.name.clone(), typed);
            } else if arg_spec.required {
                return Err(RoutingDiagnostic::missing_arg(&arg_spec.name));
            }
        }

        Ok(Invocation {
            path: CommandPath::new(namespace, operation),
            args,
            source: Some(InvocationSource {
                frontend: Frontend::MachineChannel,
                tokens: None,
                input: Some(input.to_string()),
            }),
        })
    }
}
```

---

## Implementation Status (2026-01-25)

### Current State Analysis

The RFC vision is partially implemented but **machine channel dispatch is incomplete**:

| Component                     | Status                             | Gap                                      |
| ----------------------------- | ---------------------------------- | ---------------------------------------- |
| CommandSpec source of truth   | ✅ Implemented                     | -                                        |
| Schema artifact generation    | ✅ `command-spec.json` exists      | -                                        |
| TypeScript consumption        | ✅ `tool-factory.ts` uses artifact | -                                        |
| Parity testing                | ✅ `clap_commandspec_parity.rs`    | -                                        |
| Transport abstraction         | ❌ Missing                         | No `TransportContext` trait              |
| `invoke_json()` method        | ❌ Missing                         | Commands don't accept JSON directly      |
| LM tool → Machine Channel     | ⚠️ Partial                         | Only 11 of 90 ops handled                |
| Machine channel full coverage | ❌ Missing                         | 79 operations fail with `UnknownAddress` |
| Root commands in spec         | ❌ Missing                         | `status`, `steering` still shell out     |
| CLI uses same dispatch path   | ❌ Missing                         | CLI has separate dispatch in main.rs     |

### The Missing Piece: Transport Abstraction

**Problem:** Machine channel converts JSON → argv → compile. This couples machine channel to CLI parsing and prevents true unification.

**Solution:** Implement the Transport Abstraction Architecture (see section above).

### The Missing Piece: Machine Channel Full Coverage

**Problem:** LM tools route through machine channel (`exo json server`), but the machine channel only handles 11 operations:

```
Machine Channel Handlers (tool_surface.rs + handler_registry.rs):
├── context.paths
├── docs.links.check
├── docs.links.fix
├── feedback.threads
├── feedback.thread.create
├── feedback.thread.reply
├── feedback.thread.status
├── phase.execution.tasks
├── rfc.show
├── run.task
└── run.tasks
```

**Missing Operations (74):**

All other operations in `command-spec.json` (20 namespaces × ~4 ops each) fail with:

```json
{ "status": "error", "error": { "code": "unknown_address" } }
```

**Current Workaround:** `machineChannel.ts` has a fallback to `spawnPerRequest()` which shells out to CLI.

### Completion Plan

#### Phase A: Generic Command Dispatch (M effort)

**Goal:** Route all CommandSpec operations through Command trait, eliminating per-op handlers.

**Design:**

```
Machine Channel Request
         │
         ▼
┌────────────────────────┐
│  api::handler::handle  │
│  (existing entry point)│
└────────────┬───────────┘
             │
             ▼
┌────────────────────────────────────────────┐
│  Route by address.path to Command registry │
│                                            │
│  1. Look up Command in default_registry()  │
│  2. Build Invocation from request.input    │
│  3. Call command.run(context, invocation)  │
│  4. Convert CommandResult to ResponseEnvelope │
└────────────────────────────────────────────┘
```

**Implementation Steps:**

1. **Add `run_from_machine_channel()` method to Command trait** (or use existing dispatch)
   - File: `tools/exo/src/command/traits.rs`
   - Takes: `&AgentContext`, `&JsonValue` (input)
   - Returns: `Result<JsonValue, CommandError>`

2. **Create generic dispatch in handler.rs**
   - File: `tools/exo/src/api/handler.rs`
   - After checking handler_registry, fall through to Command registry
   - Map `address.path` → `["exo", namespace, operation]` → Command lookup

3. **Convert input JSON to Invocation**
   - Use CommandSpec to validate and convert JSON input to typed args
   - Reuse `tool_surface::call_params_to_argv()` pattern but generalize

4. **Convert Command output to ResponseEnvelope**
   - Commands already support `--format json`, capture that output
   - Or add `run_json()` method that returns `JsonValue` directly

**Files to modify:**

- `tools/exo/src/api/handler.rs` - add generic dispatch
- `tools/exo/src/command/traits.rs` - add JSON execution method
- `tools/exo/src/api/tool_surface.rs` - delete (absorbed into generic dispatch)

#### Phase B: Root Commands in Spec (S effort)

**Goal:** Add `status` and `steering` to CommandSpec so LM tools don't shell out.

**Current:** These are "root commands" not under a namespace, handled specially in `main.rs`.

**Options:**

1. **Add pseudo-namespace** - Model as `root.status`, `root.steering`
2. **Extend CommandSpec** - Allow top-level operations without namespace
3. **Special-case in handler** - Check for root paths before namespace dispatch

**Recommended:** Option 2 - extend CommandSpec to have `root_commands` array.

**Implementation:**

- Update `tools/exo/src/command_spec.rs` to support root commands
- Update `exo schema generate` to emit root commands
- Update machine channel handler to recognize root paths

#### Phase C: Remove Fallback Path (S effort)

**Goal:** Delete `spawnPerRequest()` fallback, machine channel handles everything.

**Implementation:**

1. Remove fallback in `packages/exosuit-vscode/src/agent/lmtool/machineChannel.ts`
2. Update `MachineChannelServer.shouldUseServerMode()` to always return true
3. Error if machine channel fails (no silent fallback to CLI)

#### Phase D: Delete Obsolete Code (S effort)

**Goal:** Clean up now-redundant code.

**Files to delete/simplify:**

- `tools/exo/src/api/tool_surface.rs` - absorbed into generic dispatch
- `tools/exo/src/api/handler_registry.rs` - replaced by Command registry
- Hand-coded handlers in `tools/exo/src/api/handlers.rs` - use Command trait

#### Phase E: Command Trait Migration Cleanup (S effort)

**Goal:** Migrate remaining legacy commands to the Command trait architecture (RFC 0085).

**Commands to Migrate:**

| Command  | Complexity | Notes                         |
| -------- | ---------- | ----------------------------- |
| `write`  | Low        | Simple file-writing operation |
| `update` | Low        | Update/upgrade operation      |

**Commands NOT Migrating (with rationale):**

| Command        | Rationale                                                                                               |
| -------------- | ------------------------------------------------------------------------------------------------------- |
| `json server`  | Protocol entry point for persistent subprocess mode (RFC 0097); manages server lifecycle, not a command |
| `init`         | Bootstrap command that runs without `AgentContext`; must work before project is initialized             |
| `merge-driver` | Git plumbing command with separate lifecycle; invoked by git, not user or LM tools                      |

**Commands to Deprecate:**

| Command        | Rationale                                                                                  |
| -------------- | ------------------------------------------------------------------------------------------ |
| `json channel` | Single-request mode; superseded by `json server` (RFC 0097). Only used in tests and hooks. |

**Rationale for Non-Migration:**

These commands are **infrastructure entry points** rather than user operations:

1. **Protocol commands** (`json server`) are transport mechanisms that _invoke_ Command trait commands, not commands themselves
2. **Bootstrap commands** (`init`) cannot assume project co

3. **CLI/Machine Channel Parity Tests (Step 18d)**
   - Create tests verifying CLI `--format json` output matches machine channel response
   - Focus on the `result` field comparison (ignore transport envelope differences)
   - Cover all pure-effect operations (read-only commands)
   - Test structure:

     ```rust
     #[test]
     fn cli_machine_parity_status() {
         let cli_output = run_cli(&["status", "--format", "json"]);
         let machine_output = send_machine_request("status", json!({}));

         // Compare the `result` field, not the full envelope
         assert_eq!(cli_output["result"], machine_output["result"]);
     }
     ```

   - Purpose: Ensures both execution paths produce identical business logic output
   - Scope: All operations where `effect == "pure"` in `command-spec.json`ntext exists

4. **Git plumbing** (`merge-driver`) has a distinct invocation pattern and lifecycle managed by git

**Implementation:**

- Migrate `write` and `update` to Command trait
- Migrate tests/hooks from `json channel` to `json server`
- Document non-migrating commands in `docs/manual/architecture/command-trait.md`
- Add comments in source explaining why these commands don't implement Command trait

### Effort Summary

| Phase     | Effort | Description                             |
| --------- | ------ | --------------------------------------- |
| A         | M      | Generic command dispatch (2-3 days)     |
| B         | S      | Root commands in spec (1 day)           |
| C         | S      | Remove fallback path (0.5 days)         |
| D         | S      | Delete obsolete code (0.5 days)         |
| E         | S      | Command trait migration cleanup (1 day) |
| **Total** | **M**  | **5-6 days**                            |

### Success Criteria

1. **All 85 operations work through machine channel** - no `UnknownAddress` errors
2. **Root commands (status, steering) in CommandSpec** - LM tools don't shell out
3. **No fallback to CLI spawn** - machine channel is the only execution path
4. **Single handler dispatch** - Command registry, not per-op handlers

### Testing Strategy

1. **Add machine channel coverage test**
   - For each operation in `command-spec.json`, send a request through machine channel
   - Assert: status is "ok" or "confirm_required", never "error" with "unknown_address"

2. **Integration test: LM tool → machine channel → Command**
   - Mock VS Code tool invocation
   - Verify request flows through machine channel
   - Verify response matches CLI `--format json` output

3. **Regression test: remove fallback**
   - Assert `spawnPerRequest()` is never called in production
   - Fail loudly if machine channel returns unknown_address
