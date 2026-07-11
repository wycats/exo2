<!-- exo:201 ulid:01kmzxbd08cc9e9bgzk56g70wz -->

# RFC 201: ExoSpec Derive Macro: Inline CommandSpec Definition

- **Status**: Withdrawn
- **Stage**: 1
- **Reason**:

# RFC 0201: ExoSpec Derive Macro: Inline CommandSpec Definition

> **⚠️ Superseded by [RFC 00233: ExoSpec — Unified Command Definition](00233-exospec-unified-command-definition-and-the-end-of-dual-source-drift.md)**
>
> The proc-macro design from this RFC is absorbed into RFC 00233, which adds: a ground-truth audit of current implementation, a concrete incremental migration strategy, and consolidation of the scattered RFC constellation. The macro design itself is largely unchanged.

## Summary

A proc-macro (`ExoSpec`) that extracts CommandSpec from Clap-annotated enums extended with custom `#[exo(...)]` attributes. This achieves true single-source-of-truth for CLI and LM tool schemas by deriving all metadata from inline declarations.

## Motivation

### The Problem: Dual Definition Drift

RFC 0135 promises "CommandSpec is the single source of truth," but the current implementation has two sources:

1. **Clap derive macros** in `main.rs` define CLI parsing
2. **`Command::args()` trait implementations** define LM tool metadata

These must be kept in sync manually, with parity tests catching drift. This is better than no validation, but still requires:

- Writing the same information twice
- Maintaining ~10 lines of `args()` implementation per command
- Running parity tests to catch mistakes
- Debugging when tests fail due to subtle differences

**Real-world example**: RFC 0200 changed `inbox add` to use `--subject` as a named flag, but the `Command::args()` implementation still expected positional `subject`, causing a panic.

### The Solution: Inline Definition

Define everything once using Clap annotations plus custom `#[exo(...)]` attributes. A proc-macro extracts the complete CommandSpec at compile time.

**Before (Hybrid):**

```rust
// In main.rs - Clap definition
#[derive(Subcommand)]
enum InboxCommands {
    Add {
        #[arg(short = 's', long)]
        subject: String,
    }
}

// In inbox.rs - Separate trait implementation
impl Command for InboxAdd {
    fn args(&self) -> Vec<ArgSpec> {
        vec![ArgSpec {
            name: "subject".into(),
            short: Some('s'),
            long: Some("subject".into()),
            value_type: ValueType::String,
            required: true,
            // ... more fields
        }]
    }
}
```

**After (Inline):**

```rust
#[derive(Subcommand, ExoSpec)]
enum InboxCommands {
    #[exo(effect = "write")]
    Add {
        #[arg(short = 's', long)]
        subject: String,
    }
}
// That's it. ExoSpec generates the CommandSpec.
```

### Benefits

1. **True single source**: One definition, zero drift
2. **Less code**: ~2 attributes vs ~10 lines per command
3. **No parity tests**: Nothing to validate when there's only one source
4. **Compile-time extraction**: Errors caught at build time, not test time
5. **Preserves Clap ergonomics**: Developers keep familiar derive syntax

## Detailed Design

### Key Insight: Interface vs. Location

When RFC 0135 states "CommandSpec is the single source of truth," it means the **interface contract** is canonical—not that a specific file location contains all definitions.

Clap annotations + `#[exo(...)]` attributes _are_ the CommandSpec definition, just written in a different syntax. The proc-macro extracts the interface; the Clap file is the source of truth.

**Analogy**: GraphQL schemas can be defined via SDL files or code-first decorators. Both produce the same schema. The _schema_ is the source of truth—the _syntax_ is implementation detail.

### Custom Attributes

#### Operation-Level Attributes

```rust
#[exo(effect = "pure|write|exec")]
```

- **Required** on every command variant
- `pure`: Read-only, no side effects
- `write`: Modifies project state (files, config)
- `exec`: External effects (git, shell, network)

```rust
#[exo(confirmation = true|false)]
```

- **Optional**, defaults to `false`
- When `true`, CLI prompts for confirmation; machine channel returns `confirm_required`

```rust
#[exo(lm_tool(
    display_name = "Human Readable Name",
    icon = "emoji-or-icon-name",
    tags = ["category", "another"]
))]
```

- **Optional** LM-tool-specific metadata
- `display_name`: Shown in tool picker UI
- `icon`: Visual identifier
- `tags`: For filtering/grouping tools

#### Argument-Level Attributes

```rust
#[exo(arg_type = "string|number|boolean|enum|file|path|json")]
```

- **Optional**, inferred from Rust type when possible
- Override when inference is insufficient (e.g., `String` that should be `file`)

### Derive Macro Usage

```rust
use exo_macros::ExoSpec;

#[derive(Subcommand, ExoSpec)]
#[exo(namespace = "inbox")]
enum InboxCommands {
    /// Add a new item to the inbox
    #[exo(effect = "write")]
    Add {
        /// Subject line for the inbox item
        #[arg(short = 's', long)]
        subject: String,

        /// Optional body content
        #[arg(long)]
        body: Option<String>,

        /// Category for triage
        #[arg(long, default_value = "guidance")]
        category: String,
    },

    /// List all inbox items
    #[exo(effect = "pure")]
    #[exo(lm_tool(display_name = "List Inbox", icon = "📥"))]
    List {
        /// Show all items including resolved
        #[arg(long)]
        all: bool,
    },
}
```

### Generated Output

The `ExoSpec` derive macro generates:

#### 1. `HasCommandSpec` Trait Implementation

```rust
pub trait HasCommandSpec {
    fn command_spec() -> NamespaceSpec;
}

// Generated:
impl HasCommandSpec for InboxCommands {
    fn command_spec() -> NamespaceSpec {
        NamespaceSpec {
            name: "inbox".into(),
            operations: vec![
                OperationSpec {
                    name: "add".into(),
                    effect: Effect::Write,
                    confirmation: false,
                    description: "Add a new item to the inbox".into(),
                    args: vec![
                        ArgSpec {
                            name: "subject".into(),
                            short: Some('s'),
                            long: Some("subject".into()),
                            description: "Subject line for the inbox item".into(),
                            value_type: ValueType::String,
                            required: true,
                            default: None,
                        },
                        // ... more args
                    ],
                    lm_tool: None,
                },
                // ... more operations
            ],
        }
    }
}
```

#### 2. Build-Time Artifact Generation

The build script collects all `HasCommandSpec` implementations and generates `command-spec.json`:

```json
{
  "namespaces": {
    "inbox": {
      "operations": {
        "add": {
          "effect": "write",
          "description": "Add a new item to the inbox",
          "args": [
            {
              "name": "subject",
              "short": "s",
              "long": "subject",
              "type": "string",
              "required": true
            }
          ]
        }
      }
    }
  }
}
```

### Type Inference Rules

The macro infers `ValueType` from Rust types:

| Rust Type                  | Inferred ValueType              | Override With                         |
| -------------------------- | ------------------------------- | ------------------------------------- |
| `String`                   | `String`                        | `#[exo(arg_type = "file")]` for paths |
| `bool`                     | `Boolean`                       | —                                     |
| `i32`, `i64`, `u32`, `u64` | `Number`                        | —                                     |
| `PathBuf`                  | `Path`                          | —                                     |
| `Option<T>`                | Same as `T`, `required = false` | —                                     |
| `Vec<T>`                   | `Array` of `T`'s type           | —                                     |
| Enum types                 | `Enum`                          | —                                     |

### Error Handling

Compile-time errors for:

- Missing `#[exo(effect = "...")]` on command variants
- Invalid effect values
- Conflicting attributes

```
error: missing required attribute `#[exo(effect = "...")]`
  --> src/main.rs:42:5
   |
42 |     Add { subject: String },
   |     ^^^
   |
   = help: add `#[exo(effect = "pure")]`, `#[exo(effect = "write")]`, or `#[exo(effect = "exec")]`
```

## Implementation Plan

### Phase 0: Attribute Scaffolding (1 day)

- [ ] Define `#[exo(...)]` attribute syntax (no-op initially)
- [ ] Add attributes to existing Clap enums in `main.rs`
- [ ] Verify Clap still works with extra attributes

### Phase 1: Proc-Macro Core (3-4 days)

- [ ] Create `exo-macros` crate with `ExoSpec` derive macro
- [ ] Parse Clap attributes (`#[arg(...)]`, `#[command(...)]`)
- [ ] Parse custom `#[exo(...)]` attributes
- [ ] Generate `HasCommandSpec` implementation
- [ ] Add comprehensive tests

### Phase 2: Build Integration (2 days)

- [ ] Update `build.rs` to collect `HasCommandSpec` implementations
- [ ] Generate `command-spec.json` from collected specs
- [ ] Verify generated JSON matches current format
- [ ] Add CI validation

### Phase 3: Legacy Removal (2 days)

- [ ] Remove manual `Command::args()` implementations
- [ ] Remove `clap_bridge.rs` mirror enums
- [ ] Remove parity tests (no longer needed)
- [ ] Update documentation

### Phase 4: Polish (1 day)

- [ ] Improve error messages
- [ ] Add IDE support hints
- [ ] Document migration guide

## Relationship to Other RFCs

| RFC                                     | Relationship                                                                                     |
| --------------------------------------- | ------------------------------------------------------------------------------------------------ |
| **RFC 0135** (CommandSpec Unification)  | **Implements**. This RFC provides the mechanism for RFC 0135's "single source of truth" promise. |
| **RFC 0132** (CLI Patterns)             | **Complements**. Uses the `CommandSpec` data model defined in RFC 0132.                          |
| **RFC 0200** (CLI Argument Consistency) | **Supports**. Inline definition makes it easier to audit and enforce argument conventions.       |

## Drawbacks

1. **Proc-macro complexity**: Adds a proc-macro crate to the build
2. **Attribute syntax learning curve**: Developers must learn `#[exo(...)]` attributes
3. **Clap version coupling**: Macro must understand Clap's attribute syntax

## Alternatives Considered

### Alternative 1: Keep Hybrid with Better Tooling

Continue with Clap + `Command::args()` but add code generation to reduce boilerplate.

**Rejected**: Still two sources, just with less typing. Drift is still possible.

### Alternative 2: External DSL

Define commands in TOML/YAML, generate both Clap and CommandSpec.

**Rejected**: Loses Clap's compile-time safety and IDE support. Adds indirection.

### Alternative 3: Clap's Built-in Reflection

Wait for Clap to add reflection capabilities.

**Rejected**: Clap has no plans for this. We need LM-specific metadata anyway.

## Unresolved Questions

1. **Attribute namespace**: Should we use `#[exo(...)]` or `#[exospec(...)]` to avoid conflicts?
2. **Nested subcommands**: How deep should the macro support? (Current: 2 levels)
3. **Custom validators**: Should the macro support custom validation attributes?

## Future Possibilities

1. **IDE integration**: LSP support for `#[exo(...)]` attribute completion
2. **Documentation generation**: Generate command docs from attributes
3. **Shell completion**: Generate completion scripts from CommandSpec
4. **OpenAPI generation**: Generate API specs for HTTP exposure

