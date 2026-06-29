# Stage 3/4 RFC Current-State Classification

Initial classification of RFCs that already claim Candidate or Stable status. The purpose is to identify which records can remain historical law, which need rewrite, and which are superseded duplicates before any RFC files are moved or edited.

Classification values are deliberately conservative and descriptive rather than a closed taxonomy: `current` means the concept has obvious current code evidence; `partial-current` means real code exists but the RFC likely overstates completeness; `current-with-drift` means the core behavior exists but the RFC wording has materially drifted; `stale-*` means the RFC describes an older or absent architecture; `superseded-*` means another Stage 3/4 record appears to cover the same concept more canonically.

Post-checkpoint update: PR [#187](https://github.com/wycats/exo2/pull/187)
retired the agent-context phase archive model. RFC `0114` is no longer a
pending Stage 4 rewrite candidate; it was withdrawn with the broader
current/archive-file cleanup.

## Trajectory Overlay

This table primarily classifies implementation state: whether each Stage 3/4
RFC matches the current codebase. A second axis is needed before deciding what
to do with drift: whether the drift points toward or away from the cohesive
design direction we intend.

The [lane-centered workbench design package](../design/lane-centered-workbench/README.md)
is useful directional evidence, but it is not implementation scope for this
checkpoint. Its relevant thesis is that a lane is an observable execution
stream, not a prettier name for a branch, worktree, pull request, task list,
phase, or chat thread. That means an RFC can be current in code but still
transitional, or stale in code but aligned enough to revive as a future plan.

Use this decision frame in later rewrite passes:

| Implementation State | Design Trajectory | Likely Action |
| --- | --- | --- |
| Current | Aligned | Stabilize or canonicalize. |
| Current | Transitional | Preserve as implemented history; rewrite before promoting as future law. |
| Current | Away from target | Treat as implemented history that needs replacement. |
| Stale | Aligned | Consider reviving as a smaller Stage 0/1/2 future plan. |
| Stale | Away from target | Withdraw, archive, or supersede. |

## Table

| Stage | RFC | Title | Classification | Evidence | Notes |
| --- | ---: | --- | --- | --- | --- |
| `stage-3` | 00184 | Mode-Aware Sidebar Cockpit | `partial-current` | `packages/exosuit-vscode/src/PhaseDetailsProvider.ts`<br>`packages/exosuit-vscode/src/TreeModel.ts`<br>`packages/exosuit-cockpit/` | Sidebar/cockpit surfaces exist, but the code follows the later collapse of distinct transition state into between-phase context. |
| `stage-3` | 0021 | RFC Triage Tooling | `stale-command-absent` | `tools/exo/src/command/rfc.rs`<br>`tools/exo/src/rfc.rs`<br>`exo rfc --help` | RFC lifecycle commands exist, but the proposed `rfc triage` gardener command is not registered in the current command set. |
| `stage-3` | 0069 | Canonical ULIDs, Scoped Slugs, and RFC Corpus Control | `current` | `tools/exo/src/context.rs`<br>`crates/exosuit-ulid/`<br>`crates/exosuit-storage/migrations/V015__rfcs_table.sql` | ULID/scoped identity concepts are present in current models and RFC storage. |
| `stage-3` | 0080 | Agent-first CLI Discovery Ladder | `partial-current` | `tools/exo/src/status.rs`<br>`tools/exo/src/steering.rs`<br>`tools/exo/src/map.rs` | Status/steering/map surfaces exist, but current behavior exposed ownership/suggestion misalignment. |
| `stage-3` | 0081 | Exohook File Expansion Worked Examples | `current` | `crates/exohook/src/fileset.rs`<br>`crates/exohook/src/validate.rs`<br>`crates/exohook/tests/` | Exohook file expansion and validation tests are active. |
| `stage-3` | 0125 | Capability Tree + Machine Channel v1 | `partial-current` | `tools/exo/src/api/mod.rs`<br>`tools/exo/src/api/protocol.rs`<br>`tools/exo/src/api/handler.rs`<br>`packages/exosuit-vscode/src/machine-channel/DaemonChannelServer.ts`<br>`tools/exo/src/mcp.rs` | Protocol envelope, machine channel, handler, and MCP bridge are active; the help tree is manually curated and does not yet advertise the complete live CLI surface. |
| `stage-3` | 0128 | Exo-Shell Pattern | `current-with-drift` | `tools/exo/src/command/run.rs`<br>`tools/exo/src/run.rs`<br>`packages/exosuit-vscode/src/tasks/ExosuitTaskProvider.ts`<br>`packages/exosuit-vscode/src/lmtool/exo-run.ts` | `exo run` and VS Code task projection are active; the extension still exposes universal `exo-run`, so the RFC's no-router-tool posture is no longer literal. |
| `stage-3` | 0129 | Configurable TDD Runners | `stale-unimplemented` | `tools/exo/src/config.rs`<br>`tools/exo/src/steering.rs`<br>`tools/exo/src/command/registry.rs`<br>`docs/rfcs/stage-3/10165-reactive-sqlite-virtual-table-integration-with-revision-algebra.md` | TDD exists only as config skeleton, LM metadata/display strings, and steering suggestions. No registered `tdd` namespace or runner dispatch exists, and RFC 10165 explicitly says RFC 0129 was never implemented and should demote. |
| `stage-3` | 0130 | ULID-like Identifiers, Ordering Projections, and Human Slugs | `current` | `tools/exo/src/context.rs`<br>`crates/exosuit-storage/migrations/V007__add_sort_key.sql`<br>`crates/exosuit-storage/migrations/V010__epoch_sort_key.sql` | IDs and sort keys are implemented in current models/migrations. |
| `stage-3` | 0132 | CLI Patterns: Command Spec, Router, and Tool-Safe DSL | `current-with-drift` | `tools/exo/src/command/command_spec.rs`<br>`tools/exo/src/command/registry.rs`<br>`tools/exo/src/argv_compiler.rs`<br>`tools/exo/src/command_text.rs`<br>`crates/exospec/` | Registry-derived command specs, invocation routing, argv compilation, and shell-operator rejection are active; the full tiny DSL/projection grammar is only partial. |
| `stage-3` | 0136 | LM Tool Architecture v2 | `partial-current` | `tools/exo/src/mcp.rs`<br>`tools/exo/src/command/lm_tool_metadata.rs`<br>`tools/exo/src/command/json.rs`<br>`packages/exosuit-vscode/package.json`<br>`packages/exosuit-vscode/src/lmtool/tool-factory.ts` | LM metadata/toolset generation exists, but VS Code still registers extension-native tools plus universal `exo-run`; generated ToolSets are not the only runtime registration surface. |
| `stage-3` | 0137 | Exohook CI Workflow Projection | `current` | `crates/exohook/src/ci_emit.rs`<br>`.github/workflows/exo-ci.yml` | Generated GitHub Actions projection exists and was validated in the coverage PR. |
| `stage-3` | 0200 | CLI Argument Consistency | `partial-current` | `tools/exo/src/command/task.rs`<br>`tools/exo/src/command/goal.rs`<br>`tools/exo/src/command/idea.rs`<br>`tools/exo/src/command/inbox.rs`<br>`tools/exo/src/argv_compiler.rs` | Add/create and selector patterns are partly active, with `Next` hints present; the completed-list language overstates current code because `goal update` still takes the new label positionally and no CLI-wide lint exists. |
| `stage-3` | 10165 | Reactive SQLite Virtual Table Integration | `current` | `crates/exosuit-storage/src/vtab/`<br>`crates/exosuit-reactivity-core/`<br>`crates/exosuit-storage/migrations/` | SQLite/vtab/revision algebra code is present. |
| `stage-3` | 10170 | Mutation Boundaries in Feedback Loops | `partial-current` | `crates/exohook/src/config.rs`<br>`crates/exohook/src/validate.rs`<br>`crates/exohook/src/discover.rs`<br>`packages/exosuit-vscode/src/services/ExohookTestController.ts` | Exohook check categories, JSONL discovery category, Test Explorer filtering, and continuous-run queueing are active; the RFC-planned mutation confirmation and unified diagnostic quick-fix model are not current. |
| `stage-3` | 10175 | Surgical Strikes as Goals | `current` | `tools/exo/src/command/strike.rs`<br>`tools/exo/src/context.rs` | Strike command and goal kind are present. |
| `stage-3` | 10176 | Project State Model | `current` | `crates/exosuit-storage/migrations/V001__core_tables.sql`<br>`tools/exo/src/context.rs`<br>`tools/exo/src/plan.rs` | Epoch/phase/goal/task model is current SQLite-backed reality. |
| `stage-3` | 10179 | Binary Re-exec Workspace-Local Development Builds | `current-with-drift` | `crates/exo-reexec/src/lib.rs`<br>`tools/exo/src/main.rs`<br>`tools/exo/src/bin/exo-mcp.rs`<br>`tools/exo/src/mcp.rs`<br>`packages/exosuit-vscode/src/exoBin.ts` | Workspace-local re-exec is active for exo-family binaries, and MCP worker freshness is guarded by executable identity; extension cleanup is not literal because VS Code still has binary-dir resolution. |
| `stage-4` | 0002 | Design Axioms | `superseded-stub` | `docs/rfcs/stage-4/10153-design-axioms.md`<br>`tools/exo/src/axiom.rs` | Low-number file is only a headings stub; 10153 is the substantive axiom record, though 10153 still needs drift cleanup before being treated as fully canonical. |
| `stage-4` | 0004 | Modes of Collaboration | `superseded-duplicate` | `docs/rfcs/stage-4/10155-modes-of-collaboration.md` | Same stable concept exists as 10155. |
| `stage-4` | 0006 | Workspace Cache | `stale` | `packages/exosuit-vscode/src/WorkspaceCache.ts` | WorkspaceCache exists in VS Code, but the RFC describes old Smart Kernel/O(1) architecture language. |
| `stage-4` | 0018 | The exo CLI | `current` | `tools/exo/src/main.rs`<br>`tools/exo/src/command/` | Rust `exo` CLI is the central current surface. |
| `stage-4` | 0020 | Rich Text DOM | `superseded-duplicate` | `docs/rfcs/stage-4/10159-rich-text-dom-rtd.md`<br>`packages/exosuit-rtd/` | Same stable concept exists as 10159. |
| `stage-4` | 0022 | Unified Project State | `stale-replaced-by-10176` | `docs/rfcs/stage-3/10176-project-state-model.md`<br>`crates/exosuit-storage/`<br>`tools/exo/src/context.rs` | Describes non-existent TypeScript `ContextService` state; current reality is SQLite-backed project state, but this relationship is substantive replacement rather than recorded supersession metadata. |
| `stage-4` | 0024 | Exosuit UI Architecture | `superseded-duplicate` | `docs/rfcs/stage-4/10162-exosuit-ui-architecture.md`<br>`packages/exosuit-vscode/` | Same stable concept exists as 10162. |
| `stage-4` | 0063 | Operation-Context Errors and Boundary Conversion | `current-with-duplicate` | `tools/exo/src/failure.rs`<br>`tools/exo/src/boundary.rs`<br>`docs/rfcs/stage-2/10027-operation-context-errors-and-boundary-conversion.md` | Error boundary code exists, but duplicate Stage 2/Stage 4 records need reconciliation. |
| `stage-4` | 0085 | Command Trait Architecture | `current` | `tools/exo/src/command/traits.rs`<br>`tools/exo/src/command/mod.rs` | Trait-based command modules are current. |
| `stage-4` | 0099 | Exohook Adaptive Terminal Width | `current` | `crates/exohook/src/terminal.rs`<br>`crates/exohook/src/validate.rs`<br>`crates/exohook/tests/terminal_width.rs` | Terminal width behavior is implemented and tested. |
| `stage-4` | 0106 | Staged RFC Process | `superseded-by-0108` | `docs/rfcs/stage-4/0108-refined-staged-rfc-process.md`<br>`tools/exo/src/command/rfc.rs` | Refined process RFC exists; keep one canonical stable process doc. |
| `stage-4` | 0108 | Refined Staged RFC Process | `current` | `docs/rfcs/README.md`<br>`tools/exo/src/command/rfc.rs` | Current staged process reference, though README still needs cleanup. |
| `stage-4` | 0110 | Robust Extension Architecture | `partial-current` | `packages/exosuit-vscode/src/extension.ts`<br>`packages/exosuit-vscode/src/exoBin.ts` | Extension architecture exists, but daemon authority work shows lifecycle policy is still active/future. |
| `stage-4` | 0111 | Agent Guidance Architecture | `current-with-drift` | `AGENTS.md`<br>`src/templates/AGENTS.md`<br>`plugins/exo/skills/exo/SKILL.md` | Agent guidance exists, but instructions and Exo suggestions have drift. |
| `withdrawn` | 0114 | Advanced Phase Transition | `withdrawn-in-187` | `docs/rfcs/withdrawn/0114-advanced-phase-transition.md`<br>`tools/exo/src/phase.rs`<br>`tools/exo/src/command/phase_cmd.rs` | PR #187 withdrew the close/pivot/archive record and removed the phase-finish archive side effect. |
| `stage-4` | 0115 | Externalized Prompts | `partial-current` | `packages/exosuit-core/src/PromptService.ts`<br>`crates/exosuit-core/tests/prompts.rs` | Prompt services/tests exist, but old doc likely predates current package split. |
| `stage-4` | 0117 | Phase-Aware Dirty Working Tree Steering | `current` | `tools/exo/src/status.rs`<br>`tools/exo/src/steering.rs` | Status and steering include git dirty awareness. |
| `stage-4` | 0120 | RFC Lifecycle Management Tools | `current` | `tools/exo/src/command/rfc.rs`<br>`tools/exo/src/rfc.rs` | RFC authoring/repair/list/show machinery is active. |
| `stage-4` | 0121 | Shared Agent Runtime | `current-with-adjacent-runtime-drift` | `packages/exosuit-vscode/src/agent/AgentRuntime.ts`<br>`packages/exosuit-vscode/src/agent/ExosuitChatParticipant.ts`<br>`packages/exosuit-vscode/src/agent/TriageParticipant.ts` | The RFC's core ask, extracting `AgentRuntime` and using it from main and triage participants, is implemented; durable MCP proxy and daemon lifecycle work are adjacent runtime strata rather than proof this RFC is stale. |
| `stage-4` | 0122 | Exohook Streaming Progress Reporting | `current` | `crates/exohook/src/validate.rs`<br>`crates/exohook/src/pty_runner.rs`<br>`crates/exohook/src/pipe_runner.rs` | Streaming/progress implementation is active. |
| `stage-4` | 0123 | Grand Unification | `historical-transition-plan` | `docs/rfcs/stage-4/0123-the-grand-unification-rfc-transition.md`<br>`current RFC reconstruction phase` | Historical Epoch 12 transition plan, not current stable system law; do not treat Stage 2 RFC 10134 as a canonical superseding record. |
| `stage-4` | 0156 | CLI Command for Axioms | `current-with-duplicate` | `tools/exo/src/command/axiom.rs`<br>`tools/exo/src/axiom.rs`<br>`crates/exosuit-storage/migrations/V012__axioms_table.sql` | Axiom CLI/storage exists; duplicate with withdrawn 10152 noted in inventory. |
| `stage-4` | 10153 | Design Axioms | `current-canonical-candidate` | `tools/exo/src/axiom.rs`<br>`docs/design/axioms.design.toml` | Better canonical stable doc than 0002. |
| `stage-4` | 10154 | Context Persistence | `stale-needs-rewrite` | `docs/agent-context/`<br>`tools/exo/src/context.rs`<br>`tools/exo/src/project.rs`<br>`docs/design/agent-context-ownership.md` | The RFC says to check in `docs/agent-context`; current policy distinguishes repo projection, sidecar projection, and shadow state, so the persistence rule needs a rewrite. |
| `stage-4` | 10155 | Modes of Collaboration | `current-canonical-candidate` | `AGENTS.md`<br>`docs/rfcs/stage-4/0004-modes.md` | Better canonical stable doc than 0004, though mode language may need current alignment. |
| `stage-4` | 10159 | Rich Text DOM | `current-canonical-candidate` | `packages/exosuit-rtd/` | Better canonical stable doc than 0020. |
| `stage-4` | 10162 | Exosuit UI Architecture | `current-canonical-candidate` | `packages/exosuit-vscode/`<br>`packages/exosuit-cockpit/` | Better canonical stable doc than 0024, but it has a bad internal RFC reference and cockpit/shared perception work is ongoing. |

## Immediate Implications

- Stage 4 is not consistently canonical. Several stable docs are low-number/high-number duplicates, and the high-number versions often look like the better canonical target.
- Some Stage 4 docs are historical transition plans rather than stable current system law, especially `0123`; `0022` and `10154` are stronger stale rewrite cases. RFC `0114` has already been handled by the phase-archive withdrawal in PR #187.
- Stage 3 is closer to current reality than Stage 4 overall, especially the SQLite/project-state cluster, but the recon pass found several Stage 3 rows with real drift (`0125`, `0132`, `0136`, `10170`, `10179`) and one unimplemented placeholder (`0129`).
- The next reconstruction pass should separate canonical current law from historical decision records before rewriting future plans.
