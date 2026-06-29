# Numeric RFC ID Collision Review

This is a recon checkpoint for duplicate numeric RFC IDs. It does not change,
move, withdraw, archive, or renumber any RFC markdown files. Its purpose is to
make the cleanup decisions explicit before later work uses `exo rfc` operations
against IDs that currently resolve ambiguously.

## Summary

- Duplicate numeric ID families: 8
- Affected RFC markdown files: 16
- Clear canonical-survivor rows: `0021`, `0056`, `0057`, `0058`, `0059`, `0148`
- Review-required rows before repair: `0060`, `10187`

## Command Surface Finding

`exo rfc show <id>` does not fail loudly for these duplicate IDs. It selects one
record, which can hide another record with the same numeric ID. In several cases
below, the selected record is the withdrawn member while an active Stage 1 RFC
with the same ID exists.

Because the cleanup commands are ID-shaped, later `supersede`, `archive`,
`withdraw`, `repair`, or `rename` work should not rely on an ambiguous numeric
ID until the target file is made unambiguous or the operation has an explicit
path-qualified/manual cleanup plan.

## Decision Table

Paths in the `Colliding Files` column are relative to `docs/rfcs/`.

| RFC ID | Colliding Files | Observed `exo rfc show` Behavior | Recommended Canonical Survivor | Other Member Disposition | Repair Risk | Review Needed Before Action |
| --- | --- | --- | --- | --- | --- | --- |
| `0021` | `stage-3/0021-rfc-triage-tooling.md`<br>`withdrawn/0021-walkthrough-workflow.md` | Selects Stage 3 `RFC Triage Tooling (The Gardener)`. | Keep `stage-3/0021-rfc-triage-tooling.md` as the `0021` survivor. | Repair or retire the withdrawn walkthrough collision so it no longer owns `0021`. | Low: selected read behavior already matches the active survivor, but the withdrawn record still blocks reliable ID-only cleanup. | No design review needed beyond confirming how to preserve the withdrawn walkthrough tombstone. |
| `0056` | `stage-1/0056-user-facing-workflow-model-vscode.md`<br>`withdrawn/0056-exohook-declarative-validation-lanes-and-projections.md` | Selects withdrawn `Exohook: Declarative Validation Lanes and Projections`. | Keep `stage-1/0056-user-facing-workflow-model-vscode.md` as the `0056` survivor. | Repair the withdrawn Exohook lanes record; its body already points at RFC `0081` as superseding context. | Medium: Exo currently resolves to the withdrawn record, so naive ID-only operations would affect the wrong member. | Confirm only the preservation mechanism for the withdrawn/superseded record. |
| `0057` | `stage-1/0057-ulid-identifiers.md`<br>`withdrawn/0057-exohook-file-list-expansion-patterns.md` | Selects withdrawn `Exohook: File List Expansion Patterns`. | Keep `stage-1/0057-ulid-identifiers.md` as the `0057` survivor. | Repair the withdrawn Exohook file-list record so it no longer owns `0057`. | Medium: Exo currently resolves to the withdrawn record and hides the active ULID RFC. | Confirm only the preservation mechanism for the withdrawn record. |
| `0058` | `stage-1/0058-declarative-task-recipes-in-exosuit-toml.md`<br>`withdrawn/0058-verified-text-surgery.md` | Selects withdrawn `Verified Text Surgery`. | Keep `stage-1/0058-declarative-task-recipes-in-exosuit-toml.md` as the `0058` survivor. | Repair the withdrawn verified-text-surgery record so it no longer owns `0058`. | Medium: Exo currently resolves to the withdrawn record and hides the active task-recipes RFC. | Confirm only the preservation mechanism for the withdrawn record. |
| `0059` | `stage-1/0059-unified-variable-environment-and-lexical-scope.md`<br>`withdrawn/0059-unified-file-and-directory-rendering.md` | Selects withdrawn `Unified File and Directory Rendering`. | Keep `stage-1/0059-unified-variable-environment-and-lexical-scope.md` as the `0059` survivor. | Repair the withdrawn file/directory rendering record so it no longer owns `0059`. | Medium: Exo currently resolves to the withdrawn record and hides the active lexical-scope RFC. | Confirm only the preservation mechanism for the withdrawn record. |
| `0060` | `stage-1/0060-prompt-patterns-promptspec-resourcespec-and-cross-spec-interpolation.md`<br>`withdrawn/0060-phase-aware-dirty-working-tree-steering.md` | Selects withdrawn `Phase-Aware Dirty Working Tree Steering`. | Review-required. The prompt-patterns file is active but currently a tiny placeholder, while the withdrawn dirty-tree steering record is substantively duplicated by Stage 4 RFC `0117`. | Do not repair by ID blindly. First decide whether the active prompt-patterns placeholder should survive as `0060` or be superseded by the fuller prompt-patterns family member. Then move the withdrawn dirty-tree record behind the `0117` survivor. | High: both the selected record and the nominal active survivor point at larger drift decisions. | Yes: choose the canonical prompt-patterns survivor before any ID repair. |
| `0148` | `stage-2/0148-implicit-walkthrough-via-task-logs.md`<br>`withdrawn/0148-implicit-walkthrough-via-task-logs-stage1.md` | Selects Stage 2 `Implicit Walkthrough via Task Logs`. | Keep `stage-2/0148-implicit-walkthrough-via-task-logs.md` as the `0148` survivor. | Treat the withdrawn same-title record as a duplicate tombstone needing ambiguity cleanup. | Low: selected read behavior matches the active survivor and the two records are essentially the same design lineage. | Confirm only whether the tombstone should be renumbered, archived as historical duplicate, or represented through sidecar relation state. |
| `10187` | `stage-0/10187-cli-shaped-exo-run-mcp-transport.md`<br>`stage-1/10187-github-profile-sidecar-discovery.md` | Selects Stage 1 `GitHub Profile Sidecar Discovery`. | Review-required. These are unrelated active RFCs, so the survivor cannot be chosen mechanically from stage or current read behavior alone. | One record must receive a different numeric ID or be folded into a reviewed canonical family. Candidate A keeps GitHub Profile Sidecar Discovery as `10187`; Candidate B keeps CLI-shaped `exo-run` MCP transport as `10187` because it is closer to current implemented tool transport concerns. | High: both files are active and unrelated; either choice has reference and trajectory implications. | Yes: choose which active design keeps `10187` before any repair. |

## Next Cleanup Guidance

1. Resolve the two review-required rows (`0060`, `10187`) before running any
   ID-shaped RFC mutation for those numbers.
2. For the low-risk rows, choose a preservation mechanism for withdrawn
   tombstones before repair: sidecar relation, new ID, archive note, or explicit
   duplicate marker.
3. Improve or avoid the read surface for ambiguous IDs before relying on
   `exo rfc show <id>` as evidence that a cleanup target is unique.
