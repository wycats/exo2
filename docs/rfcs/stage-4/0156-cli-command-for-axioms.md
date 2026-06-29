<!-- exo:156 ulid:01kg5kp2jrqebs4hty9xct7pjq -->

# RFC 156: CLI Command for Axioms


# RFC 0156: CLI Command for Axioms

## Summary
The `exo` CLI enforces a "READ-ONLY" policy on CLI-managed TOML files, instructing users/agents to use the CLI to modify them. Axioms are scoped across:

- `docs/agent-context/axioms.workflow.toml`
- `docs/agent-context/axioms.system.toml`
- `docs/design/axioms.design.toml`

Legacy single-file locations are deprecated and should be migrated via `exo update` (which archives legacy axioms into `*.legacy.toml`).

## Motivation
Agents are blocked from updating project axioms because:
1. The file header says "READ-ONLY: Use 'exo' CLI to modify this file."
2. `exo --help` lists no `axiom` or `design` subcommand.
3. `exo ai context` does not provide editing capabilities.

## Proposal
Add a new command `exo axiom` (or `exo design axiom`) with subcommands:
- `add`: Add a new axiom.
- `list`: List existing axioms.
- `remove`: Remove an axiom.

## Workaround
Currently, agents must edit the file manually, violating the "READ-ONLY" directive.

