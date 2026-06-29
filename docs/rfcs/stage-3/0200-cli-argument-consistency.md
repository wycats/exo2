<!-- exo:200 ulid:01kmzxbcxntq4dyexmgenmnsdk -->

# RFC 200: CLI Argument Consistency: Natural Positional, Named Flags for the Rest


# RFC 0200: CLI Argument Consistency

## Summary

Establish consistent conventions for CLI argument handling across all `exo` commands:

1. **Add/Create commands**: The primary text field (label, title, subject) is **positional**. IDs are **optional named flags** (`--id`), auto-generated from slugified text when omitted.
2. **Selector commands** (show, complete, start, remove): The entity **ID is positional**.
3. **Update commands**: All fields are **named flags** (`--label`, `--title`, etc.).
4. **All other fields**: Named flags (`--flag`).

## Motivation

### The Problem: Agents Guess Wrong

The `exo` CLI is primarily used by AI agents via the `exo-run` LM tool (RFC 10163). Every command is a CLI string the agent must construct. When argument conventions are inconsistent, agents guess wrong — burning tokens, context, and user patience on retries.

An agent-intent audit (FRICTION.md) identified 10 friction patterns. The top pattern (P1: "Positional Confusion") showed that agents naturally try the most intuitive form first:

```
# What agents try (natural):
exo task add "Implement feature X"

# What the old CLI required:
exo task add my-task-id --label "Implement feature X"
```

The old design forced agents to invent IDs before they could create anything. But IDs are mechanical — they should be derived from the human-readable text, not invented separately.

### The Machine Channel Amplifier

Since all agent interaction flows through `exo-run`, argument inconsistency is a **primary cost driver**:

- An agent that learns `task add "My Task"` should be able to predict `idea add "My Idea"` and `inbox add "My Subject"`. Each mismatch costs a failed tool call, error parsing, and a retry.
- The agent has no muscle memory. Every session starts fresh. Consistency is the only thing that transfers.
- Success messages with forward-steering (`→ Next:`) teach the agent what to do next, but only if the suggested commands use the same conventions.

### Expected Outcome

After this RFC:

1. **Add/Create commands** accept the primary text as a positional argument — the most natural form
2. **IDs are auto-generated** from slugified text, eliminating the "invent an ID" friction
3. **Update commands** use named flags — no ambiguity about which field is being changed
4. **Selector commands** use positional IDs — the entity already exists, the ID is known
5. Agents can predict any command's syntax from knowing one command's syntax

## Detailed Design

### The Three Command Patterns

#### Pattern 1: Add/Create Commands — Text Positional, ID Optional

> **The primary text field is positional. The ID is an optional named flag, auto-generated from slugified text when omitted.**

```
exo task add "Implement feature X"              # ID auto-generated: implement-feature-x
exo task add "Implement feature X" --id my-id   # Explicit ID override
exo goal add "Ship v2.0"                        # ID auto-generated: ship-v2-0
exo idea add "Better error messages"            # ID auto-generated
exo inbox add "RFC promotion bug"               # ID auto-generated
exo rfc create "CLI Argument Consistency"        # ID auto-generated
```

**Rationale**: When creating something new, the human-readable text is the primary input. The ID is a mechanical artifact that should be derived, not invented. Agents naturally try the text-first form.

**Auto-ID generation**: The `slugify()` function converts text to a kebab-case ID. If slugification produces an empty string (e.g., all-emoji input), the fallback ID is `"untitled"`. Explicit `--id` always takes precedence.

**File-based input**: All add/create commands also accept `--label-file` / `--title-file` / `--subject-file` for reading the primary text from a file or stdin (`-`).

#### Pattern 2: Selector Commands — ID Positional

> **Commands that operate on an existing entity take the ID as a positional argument.**

```
exo task complete my-task
exo task start my-task
exo goal complete ship-v2-0
exo rfc show 0200
exo rfc promote 0200
exo inbox ack inbox-123
```

**Rationale**: The entity already exists. The ID is known. This is the most natural form.

#### Pattern 3: Update Commands — All Named Flags

> **Commands that modify fields on an existing entity use named flags for all fields.**

```
exo task update my-task --title "New title"
exo goal update ship-v2-0 --label "Ship v2.1"
```

**Rationale**: Update commands modify specific fields. Named flags make it unambiguous which field is being changed, and allow updating any subset of fields.

### The Exception: File Paths

File paths in "read" commands (e.g., `json read`, `toml read`) remain positional as this is a common CLI convention and the intent is unambiguous.

### Naming Consistency: Domain-Appropriate Names

Each entity type uses the field name natural to its domain:

| Entity | Primary text field | Long-form field |
| ------ | ------------------ | --------------- |
| Task   | `label`            | —               |
| Goal   | `label`            | —               |
| Idea   | `title`            | `--description` |
| Inbox  | `subject`          | `--body`        |
| RFC    | `title`            | `--body`        |

The key constraint is not that every command uses the same flag name, but that an agent learning one add command's pattern can predict the shape of another's: `exo <entity> add "<text>"`.

### Forward-Steering in Responses

All success messages include `→ Next:` hints that teach the agent what to do next, using the same conventions established here:

```
→ Next: exo task start my-task | exo task list
```

Error messages include recovery hints:

```
Task 'nonexistent' not found in active phase.
→ Try: exo task list
```

## Implementation Status

### Completed (This Phase)

**Add/Create commands — text positional, ID auto-generated:**

- [x] `TaskCommands::Add` — `label` positional, `--id` optional, auto-slugified
- [x] `GoalCommands::Add` — `label` positional, `--id` optional, auto-slugified
- [x] `IdeaCommands::Add` — `title` positional
- [x] `InboxCommands::Add` — `subject` positional
- [x] `RfcCommand::Create` — `title` positional

**Update commands — named flags:**

- [x] `TaskCommands::Update` — `--title` flag
- [x] `GoalCommands::Update` — `--label` flag (was positional, fixed)

**Defaults for optional fields:**

- [x] `TaskCommands::Complete` — `--log` defaults to "Completed"
- [x] `GoalCommands::Complete` — `--log` defaults to "Completed"

**Hidden backward-compat flags:**

- [x] `RfcCommand::Promote` — `--stage` accepted and ignored (hidden)

**Forward-steering in responses:**

- [x] All add/create success messages include `→ Next:` hints
- [x] All "not found" errors include `→ Try:` recovery hints
- [x] All "no active phase" errors include `→ Try: exo phase start` hint

### Remaining (Future Phases)

- [ ] Extend positional-text pattern to remaining add commands (`impl add-step`, `plan add-phase`, etc.)
- [ ] Normalize flag names across update commands
- [ ] Add compile-time lint to enforce the three patterns

## Drawbacks

1. **Positional text can be ambiguous**: `exo task add "text"` — is "text" the label or the ID? Resolved by convention: it's always the label. IDs are always `--id`.
2. **Slugification edge cases**: Non-alphanumeric input slugifies to empty string. Mitigated by fallback to `"untitled"`.
3. **Domain-specific field names**: `--label` vs `--title` vs `--subject` means agents must know which entity uses which name. Mitigated by the positional pattern — agents don't need to know the flag name for the primary text.

## Alternatives Considered

### Alternative A: Only IDs Positional (Original Design)

The original version of this RFC proposed making only IDs positional and all text fields named flags. Rejected after agent-intent audit showed:

- Agents naturally try `task add "My Task"` — forcing `task add my-id --label "My Task"` creates friction
- Requiring agents to invent IDs before creating entities adds cognitive overhead
- Auto-ID generation from slugified text eliminates the need for explicit IDs in most cases

### Alternative B: All Positional

Make all required arguments positional in a fixed order. Rejected because:

- Order is hard to remember for commands with many args
- Help output is less clear
- Doesn't match user expectations for well-known field names

## Unresolved Questions

1. Should we add a compile-time lint to enforce the three patterns?
2. Should `--label-file` / `--title-file` be unified to a single `--file` flag?

## Future Possibilities

1. **Compile-time lint**: Enforce the three patterns at build time
2. **Shell completions**: Better completions when conventions are predictable
3. **Op::Preview**: Structured error recovery that catches wrong flags before execution
