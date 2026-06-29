# Exosuit

Exosuit is a workspace-centered collaboration system for coordinating human and
agent work. The repository contains the `exo` CLI, storage/runtime crates,
VS Code integration work, documentation, RFCs, and supporting packages.

The current source tree is active dogfood infrastructure. Public distribution
and packaged plugin installation are planned work; for now, development starts
from a source checkout.

Read [Vision: Workspace-Centered Collaboration](docs/vision.md) for the
project's product thesis, current architectural foundation, and lane-centered
direction.

## Repository Layout

- `tools/exo/` - the `exo` CLI and MCP server.
- `crates/` - Rust libraries for storage, reactivity, process hosting, file
  references, and related runtime primitives.
- `packages/` - TypeScript/Svelte packages, including the VS Code integration,
  cockpit UI, docs site, and rich-text document work.
- `docs/rfcs/` - decision records for design and implementation direction.
- `docs/specs/`, `docs/design/`, and `docs/research/` - durable specifications,
  design notes, and research checkpoints.

## Local Development

Install JavaScript dependencies:

```sh
pnpm install
```

Build or check the main Rust CLI:

```sh
cargo check -p exo
```

Run `exo` from the source checkout:

```sh
cargo run -p exo -- status
cargo run -p exo -- task list
```

Run package-level JavaScript checks where package scripts are available:

```sh
pnpm --filter exosuit-cockpit run check
```

The repository uses Exo-managed project state during development. Start with:

```sh
cargo run -p exo -- status
cargo run -p exo -- task list
```

## Project State

Operational state lives in SQLite and is accessed through `exo`. Generated or
local runtime state is intentionally kept out of the public source tree. Durable
human-authored material belongs under `docs/`, especially RFCs, specs, design
notes, and research checkpoints.

## License

Exosuit is dual-licensed under either the MIT License or the Apache License,
Version 2.0, at your option. See `LICENSE-MIT` and `LICENSE-APACHE`.
