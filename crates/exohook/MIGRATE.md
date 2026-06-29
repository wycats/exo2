# exohook migration: lefthook → hooks.toml

`exohook migrate lefthook` is intentionally conservative.

It produces a deterministic, semantics-preserving first draft of `.config/exo/hooks.toml`, then writes a human-readable report so the next step can be an explicit review/robustification pass.

## Mental model

Exohook config is organized around:

- **checks**: runnable validation units (lint/format/typecheck/test)
- **filesets**: computed path sets (staged, uncommitted, committed-not-pushed, head)
- **lanes**: canonical views binding a fileset + check selection + lane policy
- **projections**: derived artifacts like git hook shims (`pre-commit`, `pre-push`)

Lefthook is a hook runner; it answers “what commands run at a git hook”. Exohook answers “what are our checks and policies, and how do hooks/CI/project workflows project from that?”

## The migration algorithm

### Pass 1 (deterministic translation)

The migrator aims for _repeatable output_ with minimal interpretation:

1. Parse `lefthook.yml` (preserving command order).
2. Map hooks to canonical lanes:
   - `pre-commit` → `coherence` (staged) and `dev` (uncommitted mirror)
   - `pre-push` → `gate` (committed_not_pushed) and `ci` (head mirror)
   - Set projections: `pre_commit = "coherence"`, `pre_push = "gate"`
3. Convert each lefthook command into an exohook check:
   - `id` is the lefthook command key
   - `label` is derived from the id (title-cased)
   - `run` preserves lefthook's `run:` string (executed via `bash -lc` internally)
   - `input_mode = "none"` by default (no guessing)
4. Convert lefthook `stage_fixed: true` into exohook mutate category:
   - `check.category = "mutate"`
   - In `coherence` only, add an override to enable safe auto-restaging:
     - `restage = "auto"`
     - `restage_containment = "fail"`

### Pass 2 (review and robustification)

After migration, improve the config intentionally:

- Replace shell `run = "..."` with structured argv arrays when obvious.
- Promote path-aware tools to `input_mode = "paths"` (once implemented) so exohook can do chunking/reruns deterministically.
- Tune lane policy (timeouts, containment, restaging rules) explicitly.

## Migration report

Every run writes a report file (default: `.config/exo/migrate-lefthook.report.txt`).

The report lists:

- What was migrated and where outputs were written
- The lane mapping and assumptions
- Any warnings (e.g. lefthook fields not represented yet)
- Suggested next steps for review
