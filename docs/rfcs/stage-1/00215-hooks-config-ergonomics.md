<!-- exo:215 ulid:01kmzxey1gqcqyjrm5f7me603d -->

# RFC 215: Hooks Config Ergonomics


# RFC 00215: Hooks Config Ergonomics

## Summary

The current `.config/exo/hooks.toml` configuration format (v2) exposes implementation details, requires users to understand workflow/lane abstractions, and forces repetitive bash boilerplate. This RFC proposes a v3 format that:

1. **Puts hooks first**: `[hooks]` section directly maps git hooks to checks
2. **Infers context**: Pre-commit means staged files + restaging; pre-push means verify-only
3. **Simplifies fixes**: `fix = true` on a check means "fix when appropriate"
4. **Hides plumbing**: File filtering, skip-if-empty, and tool invocation are declarative

The result: a minimal config that gives you pre-commit, pre-push, and CI generation.

## Motivation

### The Problem

Users configuring hooks encounter several friction points:

1. **Leaky Abstractions**: Implementation details bleed into user-facing config
   - JSON-RPC payloads constructed inline in shell scripts
   - Runner tuning knobs (`chunk_target_bytes`, `chunk_id`) at the top level
   - Internal concepts like "projections" exposed in vocabulary

2. **Missing Domain Concepts**: Common patterns require manual implementation
   - File filtering via `mapfile + grep` bash incantations
   - Skip-if-no-matching-files logic repeated in every check
   - Autofix + restage workflow split across check and lane definitions

3. **Vocabulary Mismatch**: Current terminology doesn't match user mental models
   - "Lanes" is jargon; "Workflows" is clearer
   - "Projections" is abstract; "Triggers" describes the actual function
   - No explicit "Targets" concept despite being central to the model

### Evidence from Current Config

The `verify-exo-links` check demonstrates the problem:

```toml
[check."verify-exo-links"]
run = """
mapfile -t files < <(printf '%s\\n' {{files}} | grep -E '\\.md$' || true)
if [ ${#files[@]} -eq 0 ]; then
  echo "info: verify-exo-links skipped (no .md files)"
  exit 0
fi
node -e 'const files=process.argv.slice(1);const input={protocol_version:1,id:\"exohook-docs-links-check\",op:{kind:\"call\",params:{address:{kind:\"operation\",path:[\"docs\",\"links\",\"check\"]},input:{targets:{paths:files}}}}};process.stdout.write(JSON.stringify(input));' -- \"${files[@]}\" | cargo exo json server > /dev/null
"""
```

This single check contains:

- Bash boilerplate for file filtering (5 lines)
- Skip-if-empty logic (duplicated in 4+ checks)
- Raw JSON-RPC protocol construction
- Knowledge of internal operation addressing

### Desired Outcome

A configuration format where:

- Common patterns are one-liners
- Implementation protocols are hidden
- Vocabulary matches user intent
- Migration from v2 is straightforward

### The User's Mental Model

When developers set up validation, they think:

> "I want to run **these checks** on **my code** before it gets **committed/pushed/merged**."

They're NOT thinking about:

- Workflow definitions and trigger mappings
- Scope algebra and file set computation
- Restaging policies and containment modes

#### Context-Aware Behavior

The tool should infer behavior from context:

| Hook                   | Scope                | Fix Behavior    | Rationale                |
| ---------------------- | -------------------- | --------------- | ------------------------ |
| `pre_commit`           | Staged files         | Fix + restage   | User wants clean commits |
| `pre_push`             | Committed-not-pushed | Verify only     | Too late to fix          |
| `ci`                   | All files            | Verify only     | Can't modify repo        |
| Manual (`exohook run`) | Uncommitted          | Fix, no restage | User is iterating        |

A check with `fix = true` means "I CAN fix issues." The context determines whether fixing actually happens.

## Detailed Design

### Terminology

| Current (v2)                               | Proposed (v3)     | Rationale                       |
| ------------------------------------------ | ----------------- | ------------------------------- |
| `[projections.git_hooks]`                  | `[hooks]`         | Direct and obvious              |
| `[lane.*]`                                 | (removed)         | Hooks reference checks directly |
| `scope = { op = "base", base = "staged" }` | (inferred)        | Context determines scope        |
| `autofix = true`                           | `fix = true`      | Clearer intent                  |
| `[lane.*.overrides.*.restage]`             | (inferred)        | Context determines restage      |
| `input_mode = "paths"`                     | `filters = [...]` | Declarative file matching       |
| `run = "..."`                              | `command = "..."` | Standard vocabulary             |
| (none)                                     | `tool = "..."`    | First-class tool invocation     |

### Proposed Structure

### Minimal Configuration

```toml
version = 3

[hooks]
pre_commit = ["fmt", "lint"]
pre_push = ["test"]

[check.fmt]
command = "cargo fmt --"
filters = ["**/*.rs"]
fix = true

[check.lint]
command = "cargo clippy -- -D warnings"

[check.test]
command = "cargo test"
```

This gives you:

- **Pre-commit**: Runs `fmt` and `lint` on staged `.rs` files, auto-fixes and restages
- **Pre-push**: Runs `test` on all committed-not-pushed changes
- **CI generation**: Same checks, verify-only mode (future)

### Full Configuration (exo2 equivalent)

```toml
version = 3

[hooks]
pre_commit = ["check", "typecheck", "lint", "fmt", "clippy", "verify-toml", "verify-links"]
pre_push = ["test"]

[check.check]
label = "Check"
command = "pnpm -r run check"

[check.typecheck]
label = "VS Code Typecheck"
command = "pnpm --filter exosuit-context run typecheck"

[check.lint]
label = "Lint"
command = "pnpm --filter exosuit-vscode exec eslint --max-warnings 0"
filters = ["packages/exosuit-vscode/**/*.ts", "packages/exosuit-vscode/**/*.tsx", "packages/exosuit-vscode/**/*.js"]

[check.fmt]
label = "Rust Fmt"
command = "cargo fmt --all --"
filters = ["**/*.rs"]
fix = true

[check.clippy]
label = "Rust Clippy"
command = "cargo clippy --workspace -- -D warnings"
fix = true
fix_command = "cargo clippy --workspace --fix --allow-dirty --allow-staged -- -D warnings"

[check.verify-toml]
label = "Verify Toml"
command = "node scripts/verify-toml.ts"
filters = ["**/*.toml"]

[check.verify-links]
label = "Verify Exo Links"
tool = "exo.docs.links.check"
filters = ["**/*.md"]

[check.test]
label = "Test"
command = "pnpm -r run test:unit"
```

### Inline Check Definitions

For quick setups, checks can be defined inline:

```toml
[hooks]
pre_commit = [
   { command = "cargo fmt --", filters = ["**/*.rs"], fix = true },
   { command = "cargo clippy -- -D warnings" },
   "test"  # Reference to [check.test]
]

[check.test]
command = "cargo test"
```

Inline and referenced checks can be mixed. Use `exohook extract <index> --name <name>` to promote an inline check to a reusable definition.

### Key Changes

1. **`[hooks]` as primary interface**: No workflow indirection for common cases
2. **`fix = true` with context-aware behavior**: Checks declare capability; context determines action
3. **`skip_if_empty` defaults to `true`**: When `filters` is present, empty matches skip silently
4. **Inline check definitions**: Quick setup without separate `[check.*]` sections
5. **`tool` field**: First-class exo tool invocation without JSON-RPC plumbing
6. **CI generation goal**: Single config produces git hooks AND GitHub Actions (future RFC)

### Power User: Custom Workflows

For scenarios beyond git hooks, define custom workflows:

```toml
[hooks]
pre_commit = ["fmt", "lint"]

[workflow.full]
label = "Full Validation"
checks = ["fmt", "lint", "test", "coverage"]
parallel = true

[workflow.quick]
label = "Quick Check"
checks = ["fmt", "lint"]
scope = "uncommitted"  # Override default scope
```

Run with `exohook run full` or `exohook run quick`.

Workflows support:

- `scope`: Override the inferred scope (`staged`, `uncommitted`, `all`)
- `parallel`: Run checks in parallel (default: `true`)
- `fix_policy`: Override context-aware fix behavior (rarely needed)

## Migration Concerns

### v2 → v3 Compatibility

- **Automatic migration**: Tool should auto-upgrade v2 configs on first run
- **Deprecation period**: v2 format supported with warnings for N releases
- **Escape hatch**: `run` field still available for complex shell scripts that can't be expressed declaratively

### Breaking Changes

- `[projections.*]` → removed (use `[hooks]` directly)
- `[lane.*]` → removed (use `[hooks]` + `[check.*]`)
- `scope = { op = "base", base = "..." }` → inferred from hook type
- `autofix = true` → `fix = true`
- `[lane.*.overrides.*.restage]` → inferred from context
- `input_mode` → removed (use `filters`)
- `run = "..."` → `command = "..."` (or keep `run` for complex shell scripts)

## Comparison with Alternatives

### vs Lefthook

**Lefthook:**

```yaml
pre-commit:
  parallel: true
  commands:
    fmt:
      glob: "**/*.rs"
      run: cargo fmt
      stage_fixed: true
    lint:
      run: cargo clippy -- -D warnings
```

**Exohook v3:**

```toml
[hooks]
pre_commit = ["fmt", "lint"]

[check.fmt]
command = "cargo fmt --"
filters = ["**/*.rs"]
fix = true

[check.lint]
command = "cargo clippy -- -D warnings"
```

**Key differences:**

- Exohook: Checks are reusable across hooks (define once, use in pre_commit AND pre_push)
- Exohook: Context-aware fix behavior (no `stage_fixed` needed)
- Exohook: `tool` field for first-class exo integration
- Exohook: Single config generates git hooks AND CI workflows

## Drawbacks

- **Migration burden**: Existing configs need updating
- **Reduced flexibility**: Declarative filters may not cover all edge cases
- **Learning curve**: Users familiar with v2 must learn new vocabulary

## Alternatives

1. **Keep v2, add sugar**: Add `filters` etc. without renaming concepts
   - Pro: No migration
   - Con: Vocabulary debt persists

2. **YAML instead of TOML**: More expressive for complex structures
   - Pro: Better nested config support
   - Con: Ecosystem churn, TOML is established

3. **Programmatic config (TypeScript/Lua)**: Full flexibility
   - Pro: Unlimited expressiveness
   - Con: Complexity explosion, harder to validate

## Stage 1 Decisions

### Resolved Questions

1. **Filter syntax**: Glob patterns via `globset` crate. Supports `**/*.rs`, `!**/generated/**`.

2. **Tool invocation**: `tool = "exo.docs.links.check"` → internal `exo call`. User never sees JSON-RPC.

3. **Context-aware fix behavior**:
   - `fix = true` means "this check CAN fix"
   - Pre-commit context → run `fix_command` (if present) or `command`, then restage
   - Pre-push/CI context → run `command` only (verify mode)
   - Optional `fix_command` field for tools that need different commands (e.g., `cargo clippy --fix` vs `cargo clippy`)

4. **`skip_if_empty` default**: `true` when `filters` is present. Explicit `skip_if_empty = false` to fail on no matches.

5. **Inline checks**: Allowed in `[hooks]` arrays. Mixed with references. Extractable via CLI.

6. **Workflows**: Optional power-user feature. Not needed for git hook configuration.

7. **CI generation**: Goal is single config → multiple outputs. Specifics deferred to future RFC.

### Implementation Phases

**Phase 1: Parser & Schema**

- Add v3 schema with `[hooks]` as primary interface
- Support `[workflow.*]` as optional
- Emit deprecation warning for v2

**Phase 2: Context Engine**

- Infer scope from hook type
- Infer fix behavior from context
- Implement `fix = true` → verify-only transformation for non-fixing contexts

**Phase 3: Filter Engine**

- Implement `filters` field with globset matching
- Default `skip_if_empty = true` when filters present

**Phase 4: Tool Invocation**

- Implement `tool` field resolution
- Route to `exo call` internally

**Phase 5: Migration Tooling**

- `exohook migrate` command to convert v2 → v3
- `exohook extract` to promote inline checks

## Unresolved Questions

1. **Auto-check flag**: For tools like `cargo fmt`, should the runner auto-append `--check` in verify mode? Or require explicit `fix_command`?
   - Tool-specific knowledge in runner

2. **CI generation specifics**: Output format, workflow structure, matrix builds (deferred to future RFC)

3. **Workflow inheritance**: Can workflows extend other workflows? (Deferred)

## Future Possibilities

- **Workflow composition**: `workflow.full = { extends = ["coherence", "gate"] }`
- **Remote checks**: `tool = "remote:ci.lint.eslint"` for cloud-based checks
- **Check dependencies**: `depends_on = ["typecheck"]` for ordering
- **Conditional checks**: `when = { branch = "main" }` for context-aware execution
- **IDE integration**: VS Code showing workflow status in status bar

