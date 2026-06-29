<!-- exo:10177 ulid:01kmzxbcyv5cbpp94n3f8xc01t -->


# RFC 10177: Local XDG: Project-Scoped Directory Conventions

# RFC: Local XDG — Project-Scoped Directory Conventions

## Summary

Define a project-local directory hierarchy that mirrors XDG Base Directory semantics, enabling consistent handling of config, state, cache, data, and runtime artifacts within a workspace.

## Motivation

The XDG Base Directory Specification provides a well-understood taxonomy for user-global files:

| XDG Directory     | Purpose           | Default           |
| ----------------- | ----------------- | ----------------- |
| `XDG_CONFIG_HOME` | User preferences  | `~/.config/`      |
| `XDG_STATE_HOME`  | Persistent state  | `~/.local/state/` |
| `XDG_DATA_HOME`   | User data         | `~/.local/share/` |
| `XDG_CACHE_HOME`  | Regenerable cache | `~/.cache/`       |
| `XDG_RUNTIME_DIR` | Ephemeral/session | `/run/user/$UID/` |

However, **no equivalent standard exists for project-local directories**. Projects have evolved ad-hoc conventions:

- `.git/` — Version control (special-cased)
- `node_modules/` — Dependencies (gitignored)
- `.env` / `.envrc` — Environment (sometimes checked in)
- `.editorconfig` — Editor settings (checked in)
- `dist/`, `build/`, `target/` — Build outputs (gitignored)

This RFC proposes a **Local XDG** pattern: project-local directories that mirror XDG semantics, with clear rules for what gets checked into version control.

## Design

### Directory Taxonomy

| Local Directory | XDG Analog        | Purpose               | Git Status               |
| --------------- | ----------------- | --------------------- | ------------------------ |
| `.config/`      | `XDG_CONFIG_HOME` | Tool configs          | ✅ Checked in            |
| `.state/`       | `XDG_STATE_HOME`  | Persistent state      | ✅ Checked in (diffable) |
| `.cache/`       | `XDG_CACHE_HOME`  | Regenerable artifacts | ❌ Gitignored            |
| `.runtime/`     | `XDG_RUNTIME_DIR` | Sockets, PIDs         | ❌ Gitignored            |

**Naming convention**: Flat directories at project root (`.config/`, `.state/`, `.cache/`, `.runtime/`), not nested like XDG's `~/.local/state/`. The project root is effectively "local."

**Deferred**: `.data/` (XDG_DATA_HOME analog) — no concrete use case yet.

### Detailed Semantics

#### Config (`.config/`)

Tool configuration that supports the `.config/` convention lives in a flat directory:

```
.config/
  eslint.mjs        # ESLint config
  prettier.json     # Prettier config
  nuxt.ts           # Nuxt config
  lefthook.yml      # Git hooks config
```

**Naming convention**: `.config/<toolname>.<ext>` — flat, no extra nesting. This follows the [pi0/config-dir](https://github.com/pi0/config-dir) proposal (729+ stars).

**Tools with native `.config/` support** (as of 2026):

- c12, cosmiconfig, nuxt, nitro, unbuild (unjs ecosystem)
- rubocop, cargo-nextest, mise, goreleaser, lefthook, hk, prisma

**Tools without native support**: Can still use `.config/` via explicit config path flags (e.g., `eslint --config .config/eslint.mjs`) or IDE settings.

**Root-level configs**: Traditional root configs (`tsconfig.json`, `Cargo.toml`, `.editorconfig`) remain valid. Projects can adopt `.config/` incrementally for tools that support it.

#### State (`.state/`)

Persistent, app-managed state that should survive restarts and be shared across machines:

```
.state/
  plans.ndjson      # Diffable table export
  tasks.ndjson      # Diffable table export
  rfcs.ndjson       # Diffable table export
```

**Key properties**:

- ✅ Checked into git
- ✅ Human-readable (NDJSON, not binary)
- ✅ Diffable (line-per-record format)
- ⚠️ App-managed (not hand-edited)

**Anti-pattern**: Binary files in `.state/` defeat the purpose. SQLite databases go in `.cache/`.

#### Data (`.data/`)

User-created content that's part of the project but not configuration or state:

```
.data/
  templates/        # User-defined templates
  snippets/         # Code snippets
  assets/           # Large binary assets
```

**Key properties**:

- ⚠️ Git status depends on content (text vs binary, size)
- User-created, not app-managed
- May use Git LFS for large files

**Open question**: Is `.data/` needed, or do existing conventions (`assets/`, `templates/`) suffice?

#### Cache (`.cache/`)

Regenerable artifacts that speed up operations but can be deleted without data loss:

```
.cache/
  exo.db            # SQLite database (binary)
  build/            # Incremental build artifacts
  .tsbuildinfo      # TypeScript build cache
```

**Key properties**:

- ❌ Always gitignored
- Can be deleted and regenerated
- May contain binary files
- Machine-specific (not portable)

#### Runtime (`.runtime/`)

Ephemeral artifacts that exist only while processes are running:

```
.runtime/
  daemon.sock       # Unix domain socket
  daemon.pid        # PID file
  watcher.pid       # File watcher PID
```

**Key properties**:

- ❌ Always gitignored
- Session-scoped (cleaned up on process exit)
- Machine-local (not portable)

**Why project-local, not XDG global?**

1. **Worktrees**: Each worktree gets its own `.runtime/` — no collision
2. **Nested workspaces**: `monorepo/.runtime/` and `monorepo/packages/foo/.runtime/` operate independently
3. **Simplicity**: No hash calculation needed; `exo daemon` just uses `.runtime/daemon.sock`
4. **Discoverability**: Tools find the socket by walking up to `.runtime/` (like finding `.git/`)
5. **Cleanup**: `rm -rf .runtime/` cleans everything; no hunting through `~/.local/state/`

**Tradeoff**: You can't easily list "all running exo daemons" from a single location. If needed, a registry layer (`~/.local/state/exo/workspaces.json`) could track active workspaces — but that's an optimization, not the primary architecture.

### Git Configuration

Recommended `.gitignore` entries:

```gitignore
# Local XDG: Cache (always ignored)
.cache/

# Local XDG: Runtime (always ignored)
.runtime/
```

Note: `.state/` is checked in (diffable NDJSON exports). `.config/` is checked in (tool configs).

## Design Decisions

### 1. `.data/` deferred

The XDG distinction between "data" and "state" doesn't translate well to project scope. Most project "data" is already in the repo (source code, assets). **Deferred** until a concrete use case emerges.

### 2. Flat naming (`.state/` not `.local/state/`)

The project root is effectively "local," so the extra nesting adds no value. Flat names (`.config/`, `.state/`, `.cache/`, `.runtime/`) are simpler and parallel.

### 3. Project-local `.runtime/`

Runtime artifacts (sockets, PIDs) live in `.runtime/` within the project, not in XDG global directories. This solves worktree and nested workspace scenarios cleanly. See the Runtime section for rationale.

### 4. `.config/` for compliant tools

Tools that support the [pi0/config-dir](https://github.com/pi0/config-dir) convention should use `.config/<toolname>.<ext>`. Tools without native support can still use `.config/` via explicit flags. Root-level configs remain valid for tools that don't support `.config/`.

## Open Questions

### 1. Relationship to existing conventions

How does Local XDG interact with:

- `node_modules/` — Already gitignored, effectively `.cache/`
- `target/`, `dist/` — Build outputs, effectively `.cache/`
- `.env` — Config, but often gitignored for secrets

**Proposal**: Local XDG is additive. Existing conventions remain valid. Projects can adopt Local XDG incrementally.

## Prior Art

### XDG Base Directory Specification

The foundational spec for user-global directories. Well-understood, widely adopted.

### direnv

Project-local environment via `.envrc`. Demonstrates the value of project-scoped configuration.

### EditorConfig

Project-local editor settings via `.editorconfig`. Demonstrates cross-tool conventions.

### Git

The `.git/` directory is the original project-local state store. Special-cased by all tools.

### pi0/config-dir

A [community proposal](https://github.com/pi0/config-dir) (729+ stars) for `.config/` directory conventions. Actively maintained with a [support tracker](https://github.com/pi0/config-dir/discussions/6). This RFC aligns with their conventions for the config directory.

## Implementation Notes

### For Exosuit

1. **Config**: `exosuit.toml` at root (established convention)
2. **State**: `.state/*.ndjson` for diffable table exports
3. **Cache**: `.cache/exo.db` for SQLite binary (gitignored)
4. **Runtime**: `.runtime/daemon.sock` and `.runtime/daemon.pid` (gitignored)

### For Other Tools

This RFC proposes conventions, not mandates. Tools can adopt Local XDG incrementally:

1. Move regenerable artifacts to `.cache/`
2. Move persistent state to `.state/` (with diffable format)
3. Use XDG global for runtime artifacts

## References

- [XDG Base Directory Specification](https://specifications.freedesktop.org/basedir-spec/basedir-spec-latest.html)
- [Arch Wiki: XDG Base Directory](https://wiki.archlinux.org/title/XDG_Base_Directory)
- [pi0/config-dir](https://github.com/pi0/config-dir) — Community proposal for `.config/` conventions
- [pi0/config-dir Support Tracker](https://github.com/pi0/config-dir/discussions/6) — Tools that support `.config/`

## Related RFCs

- [RFC 0097: Machine Channel Unified Server Architecture](../stage-1/0097-machine-channel-unified-server-architecture.md) — Uses `.runtime/` for daemon socket
- [RFC 0125: Machine Channel Protocol](../stage-3/0125-exosuit-capability-tree-machine-channel-v1.md) — Protocol envelope format
