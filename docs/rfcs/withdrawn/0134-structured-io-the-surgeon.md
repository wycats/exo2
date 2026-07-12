<!-- exo:134 ulid:01kg5kp2hnp4qs8zy448ewwkn7 -->

# RFC 134: Structured IO (The Surgeon)

- **Status**: Withdrawn
- **Stage**: 3
- **Reason**:

# RFC 0134: Structured IO (The Surgeon)

## Meta

- **Status**: Stage 2 (Draft) (Proposal)
- **Created**: 2025-12-05
- **Tags**: tooling, agent-experience, reliability

## The Problem: "Text Surgery" is Risky

LLMs are probabilistic text generators. When asked to edit a structured file (JSON, TOML, YAML), they often fail at the "Text Surgery" level:

1.  **Syntax Errors**: Missing commas, unclosed braces, invalid indentation (YAML/Python).
2.  **Hallucination**: Inventing keys that don't exist or nesting structures incorrectly.
3.  **Context Window Waste**: Reading the entire file just to change one value, then rewriting the entire file (risking truncation).
4.  **Formatting Wars**: The agent's output often conflicts with the project's Prettier/EditorConfig settings, leading to "lint fix" loops.

## The Solution: Structured IO (The Surgeon)

Instead of asking the agent to "edit the text of `package.json`", we provide a suite of CLI tools that operate at the **Semantic Level**.

The agent doesn't edit the file; it issues a command to _mutate the structure_.

### The Metaphor

- **The Patient**: The structured file (JSON, TOML, YAML).
- **The Surgeon**: The `exo` CLI tool that performs precise, sterile interventions.
- **The Agent**: The hospital administrator who orders the surgery but doesn't hold the scalpel.

### Proposed Commands

#### 1. JSON Operations

```bash
# Set a value (creates keys if missing)
exo json set package.json "dependencies.react" "^18.0.0"

# Delete a key
exo json delete tsconfig.json "compilerOptions.noImplicitAny"

# Append to an array
exo json push .vscode/settings.json "files.exclude" "**/node_modules"
```

#### 2. TOML Operations

```bash
# Update a dependency version
exo toml set Cargo.toml "dependencies.serde.version" "1.0"

# Add a feature flag
exo toml push Cargo.toml "dependencies.serde.features" "derive"
```

#### 3. YAML Operations

```bash
# Update a workflow step
exo yaml set .github/workflows/ci.yml "jobs.build.steps[0].run" "cargo test"
```

## Benefits

1.  **Guaranteed Validity**: The tool parses, modifies, and serializes. It is impossible to generate invalid JSON/TOML.
2.  **Idempotency**: Running `exo json set key value` twice results in the same state.
3.  **Token Efficiency**: The agent only outputs a short command, not the full file content.
4.  **Format Preservation**: The tool can respect (or re-apply) the project's formatting rules automatically.

## The Conceptual Model: The Agent OS Stack

We are not just building a CLI; we are building an **Operating System for the Agent**. This RFC defines the "System Call" layer.

### Level 0: The Kernel (Infrastructure)

- **Rust Runtime**: Fast, memory-safe execution (RFC 0029).
- **Tree-Sitter / Ast-Grep**: The "Physics Engine" that understands the structure of code.
- **The File System**: The raw storage medium.

### Level 1: The System Calls (The Surgeon - RFC 0023)

- **Stateless & Atomic**: These commands do exactly one thing, safely.
- **Guaranteed Validity**: They cannot leave the system in a broken syntax state.
- **Primitives**:
  - `exo json set`: Atomic configuration mutation.
  - `exo code patch`: Atomic code refactoring.
  - `exo fs move`: Atomic file relocation (updating imports).

### Level 2: The User Space (The Workflows - RFC 0008/0032)

- **Stateful & Process-Aware**: These tools chain system calls to achieve a business goal.
- **The Bureaucrat (`exo rfc edit`)**: Uses `exo toml` to enforce metadata laws.
- **The Gardener (`exo rfc triage`)**: Uses `exo fs` to organize the workspace.

### Level 3: The Agent (The User)

- **The Driver**: The LLM that decides _which_ system calls to make based on high-level intent.
- **The Benefit**: By exposing a robust Level 1, we prevent the Agent from having to operate at Level 0 (raw bytes), reducing hallucination and error.

## Implementation Strategy

- Build as a subcommand of `exo` (e.g., `exo io` or top-level `exo json`).
- Use Rust's robust ecosystem (`serde_json`, `toml_edit`, `serde_yaml`) to handle the parsing.
- **Crucial**: Use format-preserving parsers (like `toml_edit`) where possible to avoid destroying comments and whitespace.

## Future Scope

- **Smart Merging**: `exo json merge config.default.json config.local.json`
- **Schema Validation**: `exo json validate --schema schema.json config.json`
- **XML/HTML Support**: (Maybe? Much harder to do generically).

### Extension: Code Surgery (The Pattern Engine)

While structured data (JSON/TOML) covers configuration, the ultimate goal is to apply "The Surgeon" metaphor to source code itself.

- **The Challenge**: Code has scope, control flow, and complex grammar. It is not just key-value pairs.
- **The Infrastructure**: `tree-sitter` provides a robust, error-tolerant parser for almost any language.
- **The Engine**: We will leverage **ast-grep** (written in Rust), which provides pattern matching and rewriting logic on top of `tree-sitter`.

#### The Layered Approach

To make this accessible to LLMs (which struggle with raw AST complexity), we propose a three-tier hierarchy of commands:

##### Level 1: Named Intents (The Easy Path)

High-level, semantic commands for the most common refactoring tasks. These are "foolproof" and require no knowledge of the underlying AST.

```bash
# Rename a symbol safely
exo code rename src/main.rs --symbol "OldName" --to "NewName"

# Insert code after a specific function
exo code insert src/lib.rs --after-function "init" --content "fn cleanup() { ... }"

# Manage imports
exo code import add src/app.ts --module "react" --named "useState"
```

##### Level 2: Structural Patch (The Power User)

A generic "Search and Replace" that understands code structure. This uses a "Code Template" syntax (similar to Comby or ast-grep) where `$VAR` represents a metavariable.

```bash
# Generic Structural Replace
# Matches: console.log("...")
# Rewrites to: logger.info("...")
exo code patch src/ --lang ts \
  --search 'console.log($MSG)' \
  --rewrite 'logger.info($MSG)'
```

**Why this works for Agents:**

1.  **Few Tokens**: The pattern is short and expressive.
2.  **High Precision**: It respects nesting and scope (unlike regex).
3.  **Language Agnostic**: The same logic applies to Rust, TypeScript, Python, etc.

##### Level 3: Raw Query (The Nuclear Option)

Direct access to `tree-sitter` S-expression queries (`.scm` files) or complex `ast-grep` configurations. This is reserved for complex, multi-step refactorings and is likely hidden from the standard agent workflow, or used only by specialized "Refactoring Agents".

## Safety & Concurrency: The "Do No Harm" Principle

Agents operate in a world where the file system is mutable. A user might edit a file while the agent is "thinking". To prevent "Stale Read" disasters (where the agent clobbers user changes), we introduce **Optimistic Verification**.

### 1. The "Verify-then-Act" Pattern

Every mutation command should accept an optional verification flag. Since LLMs are excellent at "quoting" context, we leverage this strength.

```bash
# "I want to rename 'foo' to 'bar', but ONLY if 'foo' looks like this..."
exo code rename src/main.rs \
  --symbol "foo" \
  --to "bar" \
  --verify-anchor "fn foo() -> Result<(), Error> {"
```

**The "Fuzzy Match" Guarantee**:
To address the LLM's tendency to hallucinate whitespace or miss trailing commas, the `verify-anchor` logic must be **Whitespace Insensitive**.

- It normalizes all whitespace (tabs, spaces, newlines) to a single space.
- It strips comments if the `--ignore-comments` flag is passed.
- It ensures the anchor is **Unique**. If the anchor matches multiple locations, the tool aborts to prevent ambiguous edits.

If the anchor is not found (or has changed significantly), the operation aborts with a **Stale Context Error**.

### 2. Atomic Feedback Loops

When an operation fails, the tool must not just say "Error". It must provide **Actionable Intelligence** to help the agent self-correct without a new read cycle.

**Scenario**: Agent tries to delete a key that doesn't exist.
**Bad Output**: `Error: Key not found.`
**Good Output**:

```text
Error: Key 'dependencies.react' not found in package.json.
Did you mean:
- dependencies.react-dom
- devDependencies.react
Current keys in 'dependencies': [react-dom, vue, svelte]
```

This allows the agent to say, "Ah, I meant `react-dom`," and retry immediately.

### 3. Dry Run & Diffing

Agents should be able to "preview" a surgery before cutting.

```bash
exo code patch ... --dry-run
```

**Output**: A standard unified diff (or a rich JSON diff) showing exactly what _would_ happen. This allows the agent to "think" about the change ("Does this look right?") before committing.

## Implementation & Feasibility

- **Engine**: Embed the `ast-grep` crate directly into the `exo` CLI.
- **Language Support**: `ast-grep` supports 20+ languages out of the box, aligning with our polyglot goals.
- **Safety**: All operations should be dry-run capable (`--check`) to allow the agent to verify the impact before applying changes.

