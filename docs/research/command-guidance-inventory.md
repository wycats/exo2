# Command Guidance Inventory

RFC: 10194
Date: 2026-06-18

This is the first executable inventory for command-surface coherence. It is
intentionally representative, not exhaustive: the goal is to establish the
validation harness and the buckets we need before deleting the larger body of
hand-authored command strings.

## Buckets

- **CommandSpec-backed Exo CLI guidance**: terminal-style strings such as
  `exo task complete <id>` that should compile after stripping the leading
  executable name.
- **CommandSpec-backed exo-run guidance**: MCP command text such as
  `task complete <id> --log $1`.
- **Human action guidance**: prose stored in `SuggestedAction.command`, such as
  asking the human to confirm an outcome. These need a better field/model, but
  they are not Exo commands.
- **External shell guidance**: `git ...` recovery commands. These are out of
  scope for CommandSpec validation until Exo grows a typed external-action
  model.
- **Legacy Exo surfaces**: Exo-authored command strings that are not currently
  represented in CommandSpec, such as the old `exo tdd new ...` steering.

## First Findings

- `task complete --message` is invalid; the supported shape is
  `task complete <id> --log <value>`. The new harness keeps this as a regression
  sample so future guidance drift is caught at the parser/spec boundary.
- Goal completion-log steering was rendering `exo goal complete <id> --log`
  without a value placeholder. CLI guidance now renders
  `exo goal complete <id> --log <summary>`, while exo-run guidance keeps using
  `$1` placeholder arguments because that surface owns substitution.
- Completion-confirmation suggestions are currently prose in a field named
  `command`. The harness classifies them as human actions for now; the typed
  command-reference slice should split command invocations from human
  validation prompts.

## Next Inventory Expansion

The typed-builder work should expand this inventory across all Exo-authored
guidance sites, including steering, structured errors, recovery text, packaged
plugin skills, generated help/docs, and cockpit actions. The target state is for
raw strings in those surfaces to be rendered from typed references or validated
at build/test time.
