<!-- exo:85 ulid:01kg5kp2f6c0b8aqhrnr42qn7n -->

# RFC 85: Command Trait Architecture


# RFC 0085: Command Trait Architecture

## Summary

Refactor the `exo` CLI from 18+ monolithic handler functions (~2,100 lines) into a trait-based architecture where each subcommand implements a `Command` trait. This enables centralized handling of cross-cutting concerns (format dispatch, error boxing, upgrade gates) and prepares the codebase for RFC 0132's "spec is law" vision.

## Motivation

The current `main.rs` contains 18 handler functions with significant duplication:

| Pattern                         | Frequency | Issue                                             |
| ------------------------------- | --------- | ------------------------------------------------- |
| Format dispatch (JSON vs Human) | 14/18     | Repeated `if format == OutputFormat::Json` blocks |
| Error boxing with steering      | 7/18      | Repeated `fn box_<cmd>_error(...)` helpers        |
| Upgrade gate checks             | 6/18      | Scattered `check_upgrade_gate()` calls            |
| JSON envelope construction      | 14/18     | Manual `serde_json::json!({ "kind": ... })`       |

The largest handlers (`handle_phase_command`: ~560 lines, `handle_impl_command`: ~389 lines) are difficult to test, maintain, and extend.

### Relationship to RFC 0132

RFC 0132 (CLI Patterns: Command Spec, Router, and Tool-Safe DSL) envisions a "spec is law" model where commands are defined declaratively and projected to multiple backends. This RFC provides the **runtime implementation target** for that vision:

- `Command` trait implementations become the authoritative source of command behavior
- The registry enables capability tree generation (RFC 0125)
- Effect annotations flow from trait methods to capability discovery

## Detailed Design

### Core Trait: `Command`

```rust
/// A single CLI operation that can be executed.
///
/// Each subcommand (e.g., `phase start`, `task complete`) implements this trait.
/// The trait is designed to be:
/// - **Format-agnostic**: Handlers return structured data, not println output
/// - **Testable**: Commands can be constructed and executed without Clap
/// - **Discoverable**: The registry can enumerate all commands for capability tree
pub trait Command: Send + Sync {
    /// The namespace for this command (e.g., "phase", "plan", "task").
    fn namespace(&self) -> &'static str;

    /// The operation name (e.g., "start", "finish", "list").
    fn operation(&self) -> &'static str;

    /// Execute the command, returning a structured result.
    ///
    /// This is the pure execution logic. Format conversion and error boxing
    /// are handled by the dispatcher.
    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput>;

    /// The effect classification for this command (RFC 0125).
    fn effect(&self) -> Effect {
        Effect::Pure
    }

    /// Whether this command requires an upgrade gate check before execution.
    fn needs_upgrade_gate(&self) -> bool {
        false
    }

    /// Default steering suggestions for errors from this command.
    fn default_steering(&self) -> Vec<SuggestedAction> {
        vec![SuggestedAction::orient("exo map", "Use map to orient.")]
    }

    /// Human-readable description for help and capability tree.
    fn description(&self) -> &'static str {
        ""
    }
}
```

### Mutable Commands

Some commands need to mutate the `AgentContext` (e.g., `plan update`, `phase start`). These implement an extended trait:

```rust
/// A command that mutates project state.
///
/// This is a sub-trait of Command for operations that need `&mut AgentContext`.
pub trait MutableCommand: Command {
    /// Execute the command with mutable access to context.
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput>;
}

/// Context for mutable commands.
pub struct MutableCommandContext<'a> {
    pub root: &'a Path,
    pub plan: &'a mut Plan,
    pub format: OutputFormat,
    // ... other mutable fields
}
```

The dispatcher checks if a command implements `MutableCommand` and routes accordingly.

### CommandOutput

Commands return structured output that the dispatcher formats:

```rust
/// Structured output from command execution.
pub struct CommandOutput {
    /// Structured data for JSON output (always present).
    pub data: serde_json::Value,

    /// Human-readable message (if different from auto-formatting data).
    pub human_message: Option<String>,
}

impl CommandOutput {
    /// Create output with just data (auto-format for human mode).
    pub fn data(data: impl Serialize) -> Self {
        Self {
            data: serde_json::to_value(data).unwrap_or(serde_json::Value::Null),
            human_message: None,
        }
    }

    /// Create output with explicit human message.
    pub fn with_message(mut self, msg: impl Into<String>) -> Self {
        self.human_message = Some(msg.into());
        self
    }
}
```

> **Note**: Steering suggestions are provided via the `Command::default_steering()` trait method rather than being embedded in `CommandOutput`. This keeps output data-focused while allowing commands to declare contextual next-step suggestions.

```rust
// Example: default_steering() provides operation-specific guidance
fn default_steering(&self) -> Vec<SuggestedAction> {
    vec![SuggestedAction::orient("exo map", "Use map to orient.")]
}
```

### Effect Classification

Per RFC 0125, each command declares its effect:

```rust
/// Coarse effect classification for capability tree (RFC 0125).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    /// Read-only, deterministic operation.
    Pure,
    /// Writes to Exosuit-managed artifacts.
    Write,
    /// Executes a workflow (phase transitions, etc.).
    Exec,
}
```

The VS Code LM tool uses this to determine when confirmation is required.

### Effect Distribution in Practice

Analysis of the 70 implemented commands reveals the following distribution:

| Effect    | Count | Examples                                                                         |
| --------- | ----- | -------------------------------------------------------------------------------- |
| **Pure**  | 17    | `list`, `show`, `status` operations; AI context commands; JSON/TOML reads        |
| **Write** | 43    | `add`, `remove`, `update` operations; file writes; impl and phase state changes  |
| **Exec**  | 10    | TDD cycle (`new`, `red`, `green`); Strike lifecycle (`start`, `finish`, `abort`) |

**Key Insight**: The `Exec` effect is reserved for operations that transition workflow state machines. These commands often have upgrade gates and represent irreversible (or costly to reverse) operations.

### Command Registry

Commands are collected in a registry that enables dispatch and discovery:

```rust
/// Registry of all available commands.
pub struct CommandRegistry {
    commands: HashMap<(&'static str, &'static str), Box<dyn Command>>,
    order: Vec<(&'static str, &'static str)>,
}

impl CommandRegistry {
    /// Register a command.
    pub fn register(&mut self, cmd: Box<dyn Command>) {
        let key = (cmd.namespace(), cmd.operation());
        if !self.commands.contains_key(&key) {
            self.order.push(key);
        }
        self.commands.insert(key, cmd);
    }

    /// Find a command by namespace and operation.
    pub fn find(&self, namespace: &str, operation: &str) -> Option<&dyn Command> {
        self.commands.iter()
            .find(|((ns, op), _)| *ns == namespace && *op == operation)
            .map(|(_, cmd)| cmd.as_ref())
    }

    /// Iterate all commands for capability discovery.
    pub fn iter(&self) -> impl Iterator<Item = &dyn Command> {
        self.order.iter()
            .filter_map(|key| self.commands.get(key).map(|c| c.as_ref()))
    }

    /// Get all unique namespaces.
    pub fn namespaces(&self) -> Vec<&'static str>;

    /// Get all commands in a namespace.
    pub fn commands_in_namespace(&self, namespace: &str) -> Vec<&dyn Command>;

    /// Get metadata for all registered commands.
    pub fn metadata(&self) -> Vec<CommandMetadata>;
}
```

**Note**: The registry stores representative instances of commands with placeholder values (for parameterized commands). It is used for:

- **Discovery**: Generating capability trees (RFC 0125)
- **Introspection**: Deriving `CommandSpec` (RFC 0132)
- **Metadata**: Listing available namespaces/operations

For **execution**, commands are constructed from Clap args and dispatched via `CommandBox`.

### Registry vs CommandBox

These two components serve complementary purposes:

| Component         | Purpose                     | When Used                              |
| ----------------- | --------------------------- | -------------------------------------- |
| `CommandRegistry` | Discovery and introspection | Generating capability tree, docs, help |
| `CommandBox`      | Execution dispatch          | Runtime command execution in main.rs   |

The registry answers "what commands exist?" while `CommandBox` answers "how do I run this command?"

### Excluded Commands

Some commands are explicitly excluded from trait migration:

- **`json channel`**: Uses stdin/stdout streaming that doesn't fit the `CommandOutput` model. Remains in main.rs with `CommandBox::NotMigrated`.

### Command Dispatcher

The dispatcher handles cross-cutting concerns:

```rust
/// Central dispatcher that handles format conversion and error boxing.
pub struct CommandDispatcher {
    format: OutputFormat,
}

impl CommandDispatcher {
    pub fn dispatch(
        &self,
        cmd: &dyn Command,
        ctx: &CommandContext,
    ) -> Result<RunOutcome, Box<dyn std::error::Error>> {
        // 1. Upgrade gate check (if command requires it)
        if cmd.needs_upgrade_gate() {
            state_machine::check_upgrade_gate(&ctx.agent_context)
                .map_err(|e| self.box_error(e, cmd.default_steering()))?;
        }

        // 2. Execute command
        let output = cmd.execute(ctx)
            .map_err(|e| self.box_error(e, cmd.default_steering()))?;

        // 3. Format response
        self.format_output(cmd, output)
    }

    fn format_output(
        &self,
        cmd: &dyn Command,
        output: CommandOutput,
    ) -> Result<RunOutcome, Box<dyn std::error::Error>> {
        match self.format {
            OutputFormat::Json => {
                let kind = format!("{}.{}", cmd.namespace(), cmd.operation());
                let mut envelope = serde_json::json!({
                    "kind": kind,
                    "ok": true,
                });

                // Merge data into envelope
                if let serde_json::Value::Object(data) = output.data {
                    for (k, v) in data {
                        envelope[k] = v;
                    }
                } else {
                    envelope["result"] = output.data;
                }

                if let Some(steering) = output.steering {
                    envelope["steering"] = serde_json::to_value(steering)?;
                }

                Ok(RunOutcome::Json(envelope))
            }
            OutputFormat::Human => {
                if let Some(msg) = output.human_message {
                    println!("{msg}");
                }
                Ok(RunOutcome::Human)
            }
        }
    }

    fn box_error(
        &self,
        e: anyhow::Error,
        steering: Vec<SuggestedAction>,
    ) -> Box<dyn std::error::Error> {
        boundary::box_anyhow_internal_with_actions(e, steering)
    }
}
```

### Clap Integration Pattern

The migration uses a `CommandBox` enum to bridge Clap variants to trait dispatch:

```rust
/// A boxed command that can be either pure (read-only) or mutable.
///
/// This enum provides a unified dispatch mechanism for commands parsed
/// from Clap enums, handling both `Command` and `MutableCommand` traits.
pub enum CommandBox {
    /// A pure (read-only) command.
    Pure(Box<dyn Command>),
    /// A mutable command that modifies project state.
    Mutable(Box<dyn MutableCommand>),
    /// A command that has not been migrated to the trait architecture.
    ///
    /// This variant is used for commands like `json channel` that have
    /// special execution requirements and remain in main.rs.
    NotMigrated,
}

impl CommandBox {
    /// Dispatch the command, executing it with the appropriate context.
    pub fn dispatch(&self, ctx: &CommandContext) -> Option<ExoResult<CommandOutput>> {
        match self {
            CommandBox::Pure(cmd) => Some(cmd.execute(ctx)),
            CommandBox::Mutable(cmd) => {
                let mut mutable_ctx = MutableCommandContext {
                    root: ctx.root,
                    format: ctx.format,
                };
                Some(cmd.execute_mut(&mut mutable_ctx))
            }
            CommandBox::NotMigrated => None,
        }
    }
}
```

Mirror enums in `clap_bridge.rs` provide `From` implementations:

```rust
/// Mirror of the EpochCommands enum from main.rs.
pub enum EpochCommands {
    List,
    Review { id: String },
}

impl From<EpochCommands> for CommandBox {
    fn from(cmd: EpochCommands) -> Self {
        match cmd {
            EpochCommands::List => CommandBox::Pure(Box::new(EpochList)),
            EpochCommands::Review { id } => CommandBox::Mutable(Box::new(EpochReview::new(id))),
        }
    }
}
```

The original pattern below shows a direct `Box<dyn Command>` conversion, but `CommandBox` provides better ergonomics by:

1. **Distinguishing Pure vs Mutable** at the type level
2. **Supporting NotMigrated** for gradual migration
3. **Centralizing dispatch logic** in one place

#### Original Pattern (for reference)

The original recommended pattern uses `From` conversion to bridge Clap enums to boxed commands:

```rust
// Clap derives as normal
#[derive(clap::Subcommand)]
pub enum PhaseSubCommand {
    Start(PhaseStartArgs),
    Finish(PhaseFinishArgs),
    Status(PhaseStatusArgs),
}

#[derive(clap::Args)]
pub struct PhaseStartArgs {
    /// The phase ID to start
    pub id: String,
}

// Each subcommand is a separate Command implementer
pub struct PhaseStart {
    id: String,
}

impl Command for PhaseStart {
    fn namespace(&self) -> &'static str { "phase" }
    fn operation(&self) -> &'static str { "start" }
    fn effect(&self) -> Effect { Effect::Exec }
    fn needs_upgrade_gate(&self) -> bool { true }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        // Validation logic...
        plan::update_status(&ctx.root, &self.id, "active")?;
        phase::init_phase(&ctx.root, &ctx.plan, &self.id)?;

        Ok(CommandOutput::data(json!({ "phase_id": &self.id }))
            .with_message(format!("Phase '{}' started successfully.", self.id)))
    }
}

// Since PhaseStart mutates plan, it also implements MutableCommand
impl MutableCommand for PhaseStart {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        plan::update_status(&ctx.root, &self.id, "active")?;
        phase::init_phase(&ctx.root, ctx.plan, &self.id)?;

        Ok(CommandOutput::data(json!({ "phase_id": &self.id }))
            .with_message(format!("Phase '{}' started successfully.", self.id)))
    }
}

// Conversion from Clap enum to boxed command
impl From<PhaseSubCommand> for Box<dyn Command> {
    fn from(cmd: PhaseSubCommand) -> Self {
        match cmd {
            PhaseSubCommand::Start(args) => Box::new(PhaseStart { id: args.id }),
            PhaseSubCommand::Finish(args) => Box::new(PhaseFinish { message: args.message }),
            PhaseSubCommand::Status(args) => Box::new(PhaseStatus { full: args.full }),
        }
    }
}
```

### Migration Strategy

The migration follows a wave-based pattern, organized by complexity:

#### Wave 1: Foundation (9 ops) ✅

Pure/simple commands to validate the pattern:

- `epoch`: list, review
- `ai`: context, prompt
- `json`: read, write, schema (channel excluded)
- `toml`: read, write

#### Wave 2: Simple Namespaces (17 ops) ✅

Straightforward namespaces with clear effect patterns:

- `axiom`: add, list, remove
- `idea`: add, list
- `tdd`: new, red, green
- `context`: restore, paths
- `inbox`: list, add, ack, resolve
- `strike`: start (with upgrade gate), finish, abort

#### Wave 3: Task & Criteria (10 ops) ✅

Namespaces with validation logic and state interactions:

- `task`: init (deprecated), clear (deprecated), add, list, complete
- `criteria`: add, satisfy, unsatisfy, remove, list

Note: Original plan included `task block/unblock` but these operations were never implemented in the codebase.

#### Wave 4: RFC Namespace (8 ops)

File operations with clear boundaries:

- `rfc`: list, show, create, edit, promote, supersede, link, lint

**Risk**: 🟢 LOW - Isolated file operations, no deep state coupling.

#### Wave 5: Plan Namespace (13 ops)

Core state mutations, heavily used by other commands:

- `plan`: add-epoch, remove-epoch, add-phase, update-phase, remove-phase, add-task, remove-task, update-status, review, bankrupt, linearize, migrate-ids, help

**Risk**: 🟡 MEDIUM - Core state layer, linearize/migrate-ids have complex algorithms.

#### Wave 6: Impl Namespace (10 ops)

Execution tracking with nested task model:

- `impl`: start, satisfy, add-feedback, add-step, add-task-log, remove-step, remove-task, update-step, nested task operations

**Risk**: 🟡 MEDIUM - TDD validation logic, nested task complexity.

#### Wave 7: Phase Namespace (3 ops)

**IMPLEMENT LAST** - Contains the bootstrap command:

- `phase`: start (with upgrade gate), finish (with upgrade gate), status

**Risk**: 🔴 HIGH - `phase status` is the canonical bootstrap command used by `exo status`. Breaking it would block all project work. Requires extensive manual QA.

#### Infrastructure Tasks

- Wire all migrated commands through `CommandBox` dispatch in main.rs
- Remove legacy handler code as replacements pass tests
- Generate capability tree from registry (RFC 0125 integration)

## Drawbacks

- **Migration effort**: 18 handlers with ~2,100 lines requires careful incremental migration
- **Two execution paths during transition**: Old handlers and new commands coexist temporarily
- **Trait object overhead**: Boxed commands have minor runtime cost (negligible for CLI)

## Lessons Learned (Implementation Notes)

These notes capture lessons from the actual implementation, added during Phase 6 migration.

### json.channel Exclusion

The `json channel` command uses stdin/stdout streaming that doesn't fit the `CommandOutput` model. Rather than force a poor abstraction, it remains in main.rs with `CommandBox::NotMigrated`. This pattern can be used for any future streaming commands.

### Upgrade Gate Integration

Commands that require upgrade gate checks declare `needs_upgrade_gate() -> true`. The first command to use this pattern was `StrikeStart`. The gate check uses `AgentContext::load()` to validate state before execution.

### MutableCommandContext Design

The `MutableCommandContext` struct provides mutable access to the workspace root. Commands needing to modify plan state use this context via `execute_mut()`. Key insight: keep the context minimal—commands should use the underlying modules (e.g., `plan::`, `phase::`) for actual mutations.

### Effect Classification Accuracy

Effect classification (Pure/Write/Exec) must match actual behavior:

- **Pure**: Read-only operations that don't touch disk
- **Write**: Modifications to Exosuit-managed artifacts (TOML files, etc.)
- **Exec**: Workflow transitions that may have side effects (commits, etc.)

### Registry vs Execution Separation

The `CommandRegistry` stores representative instances (with placeholder values) for discovery and introspection. For execution, commands are constructed fresh from Clap args and dispatched via `CommandBox`. This separation keeps the registry lightweight while supporting parameterized commands.

### Operation Count Accuracy

Initial estimates often differ from actual operation counts:

- Wave 3 estimated ~11 ops, actual was 10 (task block/unblock never existed)
- Wave 4-7 estimated ~50 ops total, actual is 34 (8+13+10+3)

Always audit handler files before migration to get accurate counts.

### Upgrade Gate Distribution

Of the 70 commands, 15 require upgrade gate validation:

| Wave       | Commands                                                                                                                                             | Gate Purpose                            |
| ---------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------- |
| **Wave 2** | `strike start` (1)                                                                                                                                   | Validates no active strike exists       |
| **Wave 4** | RFC mutations (5): `create`, `edit`, `rename`, `promote`, `supersede`                                                                                | Validates RFC state machine transitions |
| **Wave 5** | Plan mutations (9): `add-epoch`, `remove-epoch`, `add-phase`, `update-phase`, `remove-phase`, `add-task`, `remove-task`, `update-status`, `bankrupt` | Validates plan structure integrity      |
| **Wave 7** | `phase start`, `phase finish` (2)                                                                                                                    | Validates phase transition rules        |

All gate checks use `AgentContext::load()` to validate state before execution.

### Registry Test Coverage

The `registry.rs` module includes comprehensive tests validating all 70 commands:

- **Completeness**: Every registered command has namespace, operation, and effect
- **Uniqueness**: No duplicate (namespace, operation) pairs
- **Introspection**: Registry iteration returns commands in registration order
- **Discovery**: `find()` correctly locates commands by namespace and operation

## Alternatives

### Keep Monolithic Handlers

- **Pros**: No migration effort
- **Cons**: Continued duplication, difficult testing, no capability tree

### Macro-Based Approach

- **Pros**: Compile-time dispatch, zero runtime overhead
- **Cons**: More complex, harder to debug, less flexible

### External Command Spec (TOML/YAML)

- **Pros**: Non-Rust definition, easier tooling
- **Cons**: Runtime parsing, type safety loss, divergence risk

## Unresolved Questions

1. **Human output formatting**: Should `CommandOutput` include a structured human representation, or is `human_message: Option<String>` sufficient?

2. **Async commands**: Should the trait support async execution for future network operations?

3. **Command composition**: Should commands be able to delegate to other commands?

## Future Possibilities

- **CommandSpec generation**: Generate RFC 0132 `CommandSpec` from registry introspection
- **Capability tree**: Automatically generate RFC 0125 capability tree from registry
- **Test harness**: Property-based testing of command invariants
- **Documentation**: Auto-generate CLI help from trait implementations

## Stage 3 Achievement

RFC 0085 achieved Stage 3 (Candidate) status in January 2026 with full implementation:

| Metric                | Value                    |
| --------------------- | ------------------------ |
| **Waves Implemented** | 7 of 7                   |
| **Total Operations**  | 70                       |
| **Test Coverage**     | 331 passing tests        |
| **Production Status** | Deployed and operational |
| **Blockers**          | Zero                     |

The command registry architecture is now the foundation for all CLI operations, enabling the capability tree (RFC 0125), upgrade plugins, and machine channel integration.

## References

- [RFC 0132: CLI Patterns: Command Spec, Router, and Tool-Safe DSL](../stage-3/0132-cli-patterns-command-spec-router-and-tool-safe-dsl.md)
- [RFC 0125: Machine Channel v1](../stage-3/0125-exosuit-capability-tree-machine-channel-v1.md)
- [RFC 0063: Operation-Context Errors](../stage-2/0063-operation-context-errors.md)
- [UpgradePlugin trait](../../tools/exo/src/upgrade/mod.rs) - Precedent for trait-based architecture

---

## Implementation Note: CommandSpec Extraction (2026-02-02)

The `Command::args()` method described in this RFC for providing CommandSpec metadata is being **replaced** by the `ExoSpec` proc-macro approach.

### What Changed

**Original approach** (this RFC):

- Each `Command` implementation provides an `args()` method returning `Vec<ArgSpec>`
- CommandSpec is derived from trait implementations at runtime

**New approach** (RFC 0201):

- CommandSpec is defined inline via Clap annotations + `#[exo(...)]` custom attributes
- The `ExoSpec` derive macro extracts CommandSpec at compile time
- No separate `Command::args()` implementation required

### Impact on This RFC

- **Core `Command` trait**: Remains the authoritative source for _execution behavior_ (namespace, operation, effect, execute)
- **`Command::args()` method**: Being phased out; metadata now comes from inline attributes
- **Mirror enums in `clap_bridge.rs`**: Will be removed per RFC 0201 Phase 4
- **CommandRegistry introspection**: Still valid for capability discovery, but CommandSpec generation moves to compile-time

### Migration Path

1. Add `#[exo(...)]` attributes to existing Clap enums
2. Implement `ExoSpec` proc-macro (RFC 0201)
3. Remove `Command::args()` implementations
4. Remove `clap_bridge.rs` mirror enums

**See Also**: [RFC 0201](../stage-1/0201-exospec-derive-macro-inline-commandspec-definition.md) (ExoSpec Derive Macro) for detailed proc-macro design.

