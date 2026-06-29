# RFC 0021 Triage Tooling Withdrawal

## Summary

RFC `0021` has been withdrawn because its Stage 3 claim describes
`exo rfc triage`, and that command is not implemented in the current `exo rfc`
command surface.

The useful remaining idea is an agent-facing RFC corpus review report: a
structured review of stale, orphaned, duplicated, or unplanned RFCs, grounded in
the current Exo/RFC state.

## Command Surface Evidence

| Surface | Observation |
| --- | --- |
| `exo rfc --help` | Lists the current RFC lifecycle commands; no `triage` subcommand is registered. |
| `tools/exo/src/command/rfc.rs` | Registers create/edit/list/show/repair/rename/promote/supersede/withdraw/archive/status/pipeline flows, but no gardener/triage command. |
| `tools/exo/src/rfc.rs` | Implements RFC file discovery, lifecycle movement, repair, and synchronization helpers, but not the stale/orphan triage workflow described by RFC `0021`. |

## Withdrawal Rationale

RFC `0021` is marked Stage 3, but its central behavior is absent. Keeping it as
current law makes the RFC corpus overstate implemented reality.

Withdrawing it records the current state cleanly while preserving the design
pressure that produced it: the RFC corpus needs periodic review artifacts that
help humans and agents decide what should be promoted, withdrawn, merged, or
reframed.

## Relation To RFC 10142

RFC `10142` is already withdrawn and contains the same triage-tooling concept
with older planning-state wording. It remains useful as historical duplicate
context, but it does not change the disposition for `0021`.

## Preserved Idea

The follow-up idea is not a terminal-first `exo rfc triage` command as specified
in RFC `0021`. The preserved shape is a review report generated from current
state:

| Review Input | Report Output |
| --- | --- |
| RFC stage, status, title, duplicate family, and file path | Candidate disposition rows with evidence. |
| Exo phase/goal/RFC associations | Planning alignment notes. |
| Current command and code surface | Implemented-vs-stale classification. |
| Human review | Final disposition: keep, withdraw, supersede, rewrite, or extract future work. |
