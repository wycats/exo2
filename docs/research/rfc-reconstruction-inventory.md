# RFC Reconstruction Inventory

Generated from the canonical Markdown tree and the effective Exo RFC view on
clean `main` at `5b0e585b567ebecda8b7699abe524dc7c9777475`.

This is the final reconstruction-execution inventory, not a lifecycle
proposal. Directory placement records the current Markdown location. Metadata
stage records the RFC's retained design maturity, including withdrawn and
archived history. Effective status records what Exo presents to readers.

## Summary

- RFC Markdown files: 335
- Managed RFC records: 333
- Support files: 2 (README and template)
- Duplicate-title families: 96
- Records in duplicate-title families: 193
- Duplicate numeric anchors: 0
- Nonconvergent reviewed title families: 0

### Directory Placement

| Directory | Records |
| --- | ---: |
| `stage-0` | 65 |
| `stage-1` | 67 |
| `stage-2` | 12 |
| `stage-3` | 17 |
| `stage-4` | 25 |
| `archive` | 3 |
| `withdrawn` | 144 |
| **Total** | **333** |

### Effective Lifecycle Status

| Status | Records |
| --- | ---: |
| `active` | 145 |
| `superseded` | 41 |
| `withdrawn` | 144 |
| `archived` | 3 |
| **Total** | **333** |

### Retained Metadata Stage

| Stage | Records |
| ---: | ---: |
| 0 | 152 |
| 1 | 94 |
| 2 | 17 |
| 3 | 37 |
| 4 | 33 |
| **Total** | **333** |

## Canonical And Workspace Presence

- Canonically present: 333/333
- Present in this workspace observation: 332/333
- Different from canonical: 0
- Canonical quarantine rows: 0
- Sidecar repository: clean and synchronized

RFC 00178 is the sole record absent from the current workspace observation.
Its document declares two RFC headings and Stage 0 body metadata while living
in the Stage 1 directory, so Exo records a `metadata_conflict`. The current
155b sidecar's effective view still includes an earlier canonical SQLite row
with `workspace_presence: absent`. A fresh sidecar has no row to append because
canonical reconciliation skips the conflicted document before upsert, making
the effective record count depend on prior sidecar state. The final coherence
audit owns the document repair and clean-bootstrap verification.

## Duplicate Title Families

Every family below has a reviewed survivor or explicit historical relation.
The table preserves the approved family membership while reflecting current
paths and effective lifecycle status.

| Family | Count | Current Members | Disposition State |
| --- | ---: | --- | --- |
| `agent-cwd-discipline-rooted-execution` | 2 | `withdrawn` `0093` Agent CWD Discipline (Rooted Execution) (docs/rfcs/withdrawn/0093-agent-cwd-discipline-rooted-execution.md) [withdrawn]<br>`stage-0` `10080` Agent CWD Discipline (Rooted Execution) (docs/rfcs/stage-0/10080-agent-cwd-discipline-rooted-execution.md) [superseded] | reviewed and convergent |
| `agent-ecosystem` | 2 | `stage-0` `0142` Agent Ecosystem (docs/rfcs/stage-0/0142-agent-ecosystem.md) [active]<br>`withdrawn` `0144` Agent Ecosystem (docs/rfcs/withdrawn/0144-agent-ecosystem.md) [withdrawn] | reviewed and convergent |
| `ai-activity-visualization` | 2 | `stage-1` `0008` AI Activity Visualization (docs/rfcs/stage-1/0008-ai-activity-visualization.md) [active]<br>`stage-1` `10101` AI Activity Visualization (docs/rfcs/stage-1/10101-ai-activity-visualization.md) [superseded] | reviewed and convergent |
| `ai-tool-affordances` | 2 | `stage-1` `0041` AI Tool Affordances (docs/rfcs/stage-1/0041-ai-tool-affordances.md) [active]<br>`stage-1` `10117` AI Tool Affordances (docs/rfcs/stage-1/10117-ai-tool-affordances.md) [superseded] | reviewed and convergent |
| `bug-reporting-workflow` | 2 | `stage-0` `0046` Bug Reporting Workflow (docs/rfcs/stage-0/0046-bug-reporting-workflow.md) [active]<br>`stage-0` `10063` Bug Reporting Workflow (docs/rfcs/stage-0/10063-bug-reporting-workflow.md) [superseded] | reviewed and convergent |
| `cli-ast-tool-schema` | 2 | `withdrawn` `0042` CLI AST Tool Schema (docs/rfcs/withdrawn/0042-cli-ast-tool-schema.md) [withdrawn]<br>`withdrawn` `10118` CLI AST Tool Schema (docs/rfcs/withdrawn/10118-cli-ast-tool-schema.md) [withdrawn] | reviewed and convergent |
| `cli-command-for-axioms` | 2 | `stage-4` `0156` CLI Command for Axioms (docs/rfcs/stage-4/0156-cli-command-for-axioms.md) [active]<br>`withdrawn` `10152` CLI Command for Axioms (docs/rfcs/withdrawn/10152-cli-command-for-axioms.md) [withdrawn] | reviewed and convergent |
| `code-based-mcp-runner` | 2 | `withdrawn` `0161` Code-Based MCP Runner (docs/rfcs/withdrawn/0161-code-based-mcp.md) [withdrawn]<br>`withdrawn` `10082` Code-Based MCP Runner (docs/rfcs/withdrawn/10082-code-based-mcp-runner.md) [withdrawn] | reviewed and convergent |
| `coherence-bootstrap` | 2 | `stage-0` `0051` Coherence Bootstrap (docs/rfcs/stage-0/0051-coherence-bootstrap.md) [active]<br>`stage-0` `10068` Coherence Bootstrap (docs/rfcs/stage-0/10068-coherence-bootstrap.md) [superseded] | reviewed and convergent |
| `configurable-tdd-runners` | 2 | `withdrawn` `0129` Configurable TDD Runners (docs/rfcs/withdrawn/0129-configurable-tdd-runners.md) [withdrawn]<br>`withdrawn` `10115` Configurable TDD Runners (docs/rfcs/withdrawn/10115-configurable-tdd-runners.md) [withdrawn] | reviewed and convergent |
| `consolidate-agent-workflow-into-rfc-status` | 2 | `withdrawn` `0019` Consolidate Agent Workflow into rfc-status (docs/rfcs/withdrawn/0019-rfc-status-commands.md) [withdrawn]<br>`stage-0` `10056` Consolidate Agent Workflow into rfc-status (docs/rfcs/stage-0/10056-consolidate-agent-workflow-into-rfc-status.md) [superseded] | reviewed and convergent |
| `cwd-discipline-wrong-directory-mitigation` | 2 | `withdrawn` `0163` CWD Discipline & Wrong Directory Mitigation (docs/rfcs/withdrawn/0163-cwd-discipline.md) [withdrawn]<br>`stage-0` `10084` CWD Discipline & Wrong Directory Mitigation (docs/rfcs/stage-0/10084-cwd-discipline-wrong-directory-mitigation.md) [superseded] | reviewed and convergent |
| `dashboard-v2` | 2 | `withdrawn` `0126` Dashboard V2 (docs/rfcs/withdrawn/0126-dashboard-v2.md) [withdrawn]<br>`stage-2` `10136` Dashboard V2 (docs/rfcs/stage-2/10136-dashboard-v2.md) [superseded] | reviewed and convergent |
| `declarative-task-recipes-in-exosuit-toml` | 2 | `stage-1` `0044` Declarative Task Recipes in exosuit.toml (docs/rfcs/stage-1/0044-declarative-task-recipes-in-exosuit-toml.md) [active]<br>`stage-1` `0058` Declarative Task Recipes in exosuit.toml (docs/rfcs/stage-1/0058-declarative-task-recipes-in-exosuit-toml.md) [superseded] | reviewed and convergent |
| `dedicated-format-for-agent-context-links` | 2 | `withdrawn` `0010` Dedicated Format for Agent Context Links (docs/rfcs/withdrawn/0010-agent-context-links.md) [withdrawn]<br>`stage-0` `10055` Dedicated Format for Agent Context Links (docs/rfcs/stage-0/10055-dedicated-format-for-agent-context-links.md) [active] | reviewed and convergent |
| `design-axioms` | 2 | `stage-4` `0002` Design Axioms (docs/rfcs/stage-4/0002-axioms.md) [superseded]<br>`stage-4` `10153` Design Axioms (docs/rfcs/stage-4/10153-design-axioms.md) [active] | reviewed and convergent |
| `directory-based-rfc-organization` | 2 | `stage-0` `0164` Directory-Based RFC Organization (docs/rfcs/stage-0/0164-directory-based-rfcs.md) [active]<br>`withdrawn` `10085` Directory-Based RFC Organization (docs/rfcs/withdrawn/10085-directory-based-rfc-organization.md) [withdrawn] | reviewed and convergent |
| `distinguishing-spec-vs-work-rfcs` | 2 | `withdrawn` `0075` Distinguishing Spec vs. Work RFCs (docs/rfcs/withdrawn/0075-spec-vs-work-rfcs.md) [withdrawn]<br>`stage-1` `10126` Distinguishing Spec vs. Work RFCs (docs/rfcs/stage-1/10126-distinguishing-spec-vs-work-rfcs.md) [active] | reviewed and convergent |
| `dynamic-planning` | 2 | `withdrawn` `0009` Dynamic Planning (docs/rfcs/withdrawn/0009-dynamic-planning.md) [withdrawn]<br>`stage-1` `10102` Dynamic Planning (docs/rfcs/stage-1/10102-dynamic-planning.md) [superseded] | reviewed and convergent |
| `e2e-holodeck` | 2 | `withdrawn` `0078` E2E Holodeck (docs/rfcs/withdrawn/0078-e2e-holodeck.md) [withdrawn]<br>`withdrawn` `10077` E2E Holodeck (docs/rfcs/withdrawn/10077-e2e-holodeck.md) [withdrawn] | reviewed and convergent |
| `e2e-verified-dashboard-behavior` | 2 | `stage-1` `0074` E2E Verified Dashboard Behavior (docs/rfcs/stage-1/0074-e2e-verified-dashboard-behavior.md) [active]<br>`stage-1` `10124` E2E Verified Dashboard Behavior (docs/rfcs/stage-1/10124-e2e-verified-dashboard-behavior.md) [superseded] | reviewed and convergent |
| `ears-in-literate-kernel` | 2 | `stage-0` `0031` EARS in Literate Kernel (docs/rfcs/stage-0/0031-ears-in-literate-kernel.md) [active]<br>`withdrawn` `10059` EARS in Literate Kernel (docs/rfcs/withdrawn/10059-ears-in-literate-kernel.md) [withdrawn] | reviewed and convergent |
| `ebpf-auto-instrumentation` | 2 | `withdrawn` `0050` eBPF Auto Instrumentation (docs/rfcs/withdrawn/0050-ebpf-auto-instrumentation.md) [withdrawn]<br>`withdrawn` `10067` eBPF Auto Instrumentation (docs/rfcs/withdrawn/10067-ebpf-auto-instrumentation.md) [withdrawn] | reviewed and convergent |
| `editing-tools-improvements` | 2 | `withdrawn` `0165` Editing Tools Improvements (docs/rfcs/withdrawn/0165-editing-tools-improvements.md) [withdrawn]<br>`withdrawn` `10086` Editing Tools Improvements (docs/rfcs/withdrawn/10086-editing-tools-improvements.md) [withdrawn] | reviewed and convergent |
| `enforced-ui-verification` | 2 | `withdrawn` `0166` Enforced UI Verification (docs/rfcs/withdrawn/0166-enforced-verification.md) [withdrawn]<br>`withdrawn` `10087` Enforced UI Verification (docs/rfcs/withdrawn/10087-enforced-ui-verification.md) [withdrawn] | reviewed and convergent |
| `exo-map-command` | 2 | `withdrawn` `0055` exo map Command (docs/rfcs/withdrawn/0055-exo-map-command.md) [withdrawn]<br>`withdrawn` `10072` exo map Command (docs/rfcs/withdrawn/10072-exo-map-command.md) [withdrawn] | reviewed and convergent |
| `exohook-ci-workflow-projection` | 2 | `stage-3` `0137` Exohook CI Workflow Projection (docs/rfcs/stage-3/0137-exohook-ci-workflow-projection.md) [active]<br>`withdrawn` `0140` Exohook CI Workflow Projection (docs/rfcs/withdrawn/0140-exohook-ci-workflow-projection.md) [withdrawn] | reviewed and convergent |
| `exohook-declarative-validation-lanes-and-projections` | 2 | `withdrawn` `10015` Exohook: Declarative Validation Lanes and Projections (docs/rfcs/withdrawn/10015-exohook-declarative-validation-lanes-and-projections.md) [withdrawn]<br>`withdrawn` `10073` Exohook: Declarative Validation Lanes and Projections (docs/rfcs/withdrawn/10073-exohook-declarative-validation-lanes-and-projections.md) [withdrawn] | reviewed and convergent |
| `exohook-file-expansion-worked-examples` | 2 | `stage-3` `0081` Exohook: File Expansion Worked Examples (docs/rfcs/stage-3/0081-exohook-file-expansion-worked-examples.md) [active]<br>`withdrawn` `10052` Exohook: File Expansion Worked Examples (docs/rfcs/withdrawn/10052-exohook-file-expansion-worked-examples.md) [withdrawn] | reviewed and convergent |
| `exohook-file-list-expansion-patterns` | 2 | `withdrawn` `10016` Exohook: File List Expansion Patterns (docs/rfcs/withdrawn/10016-exohook-file-list-expansion-patterns.md) [withdrawn]<br>`withdrawn` `10074` Exohook: File List Expansion Patterns (docs/rfcs/withdrawn/10074-exohook-file-list-expansion-patterns.md) [withdrawn] | reviewed and convergent |
| `exohook-streaming-progress-reporting` | 2 | `stage-4` `0122` Exohook Streaming Progress Reporting (docs/rfcs/stage-4/0122-exohook-streaming-progress-reporting.md) [active]<br>`withdrawn` `0141` Exohook Streaming Progress Reporting (docs/rfcs/withdrawn/0141-exohook-streaming-progress-reporting.md) [withdrawn] | reviewed and convergent |
| `exosuit-development-kit-edk` | 2 | `stage-0` `0035` Exosuit Development Kit (EDK) (docs/rfcs/stage-0/0035-exosuit-development-kit.md) [active]<br>`stage-0` `10061` Exosuit Development Kit (EDK) (docs/rfcs/stage-0/10061-exosuit-development-kit-edk.md) [superseded] | reviewed and convergent |
| `exosuit-ui-architecture` | 2 | `stage-4` `0024` Exosuit UI Architecture (docs/rfcs/stage-4/0024-ui-architecture.md) [superseded]<br>`stage-4` `10162` Exosuit UI Architecture (docs/rfcs/stage-4/10162-exosuit-ui-architecture.md) [active] | reviewed and convergent |
| `exposing-rfcs-as-copilot-resources` | 2 | `stage-0` `0162` Exposing RFCs as Copilot Resources (docs/rfcs/stage-0/0162-copilot-resources.md) [active]<br>`withdrawn` `10083` Exposing RFCs as Copilot Resources (docs/rfcs/withdrawn/10083-exposing-rfcs-as-copilot-resources.md) [withdrawn] | reviewed and convergent |
| `externalized-prompts` | 2 | `stage-4` `0115` Externalized Prompts (docs/rfcs/stage-4/0115-externalized-prompts.md) [active]<br>`withdrawn` `10138` Externalized Prompts (docs/rfcs/withdrawn/10138-externalized-prompts.md) [withdrawn] | reviewed and convergent |
| `formal-spec-frontmatter-upgrade` | 2 | `stage-1` `0076` Formal Spec Frontmatter Upgrade (docs/rfcs/stage-1/0076-spec-frontmatter-upgrade.md) [active]<br>`withdrawn` `10127` Formal Spec Frontmatter Upgrade (docs/rfcs/withdrawn/10127-formal-spec-frontmatter-upgrade.md) [withdrawn] | reviewed and convergent |
| `ideas-triage-system` | 2 | `stage-1` `0011` Ideas & Triage System (docs/rfcs/stage-1/0011-ideas-and-triage.md) [active]<br>`stage-1` `10103` Ideas & Triage System (docs/rfcs/stage-1/10103-ideas-triage-system.md) [superseded] | reviewed and convergent |
| `implementation-plan-as-canonical-execution-artifact` | 2 | `withdrawn` `0131` Implementation Plan as Canonical Execution Artifact (docs/rfcs/withdrawn/0131-implementation-plan-as-canonical-execution-artifact.md) [withdrawn]<br>`withdrawn` `10120` Implementation Plan as Canonical Execution Artifact (docs/rfcs/withdrawn/10120-implementation-plan-as-canonical-execution-artifact.md) [withdrawn] | reviewed and convergent |
| `implicit-walkthrough-via-task-logs` | 2 | `withdrawn` `0148` Implicit Walkthrough via Task Logs (docs/rfcs/withdrawn/0148-implicit-walkthrough-via-task-logs.md) [withdrawn]<br>`withdrawn` `10199` Implicit Walkthrough via Task Logs (docs/rfcs/withdrawn/10199-implicit-walkthrough-via-task-logs-stage1.md) [withdrawn] | reviewed and convergent |
| `interactive-verification-ui` | 2 | `withdrawn` `0067` Interactive Verification UI (docs/rfcs/withdrawn/0067-interactive-verification-ui.md) [withdrawn]<br>`withdrawn` `10031` Interactive Verification UI (docs/rfcs/withdrawn/10031-interactive-verification-ui.md) [withdrawn] | reviewed and convergent |
| `lightweight-checks-cognitive-load` | 2 | `withdrawn` `0167` Lightweight Checks / Cognitive Load (docs/rfcs/withdrawn/0167-lightweight-checks.md) [withdrawn]<br>`withdrawn` `10088` Lightweight Checks / Cognitive Load (docs/rfcs/withdrawn/10088-lightweight-checks-cognitive-load.md) [withdrawn] | reviewed and convergent |
| `lint-and-format-lane` | 2 | `withdrawn` `0168` Lint and Format Lane (docs/rfcs/withdrawn/0168-lint-and-format-lane.md) [withdrawn]<br>`withdrawn` `10089` Lint and Format Lane (docs/rfcs/withdrawn/10089-lint-and-format-lane.md) [withdrawn] | reviewed and convergent |
| `local-rag-architecture-rust-wasm` | 2 | `stage-0` `0169` Local RAG Architecture (Rust/Wasm) (docs/rfcs/stage-0/0169-local-rag-architecture.md) [active]<br>`stage-0` `10090` Local RAG Architecture (Rust/Wasm) (docs/rfcs/stage-0/10090-local-rag-architecture-rust-wasm.md) [superseded] | reviewed and convergent |
| `manual-test-rfc` | 2 | `withdrawn` `0155` Manual Test RFC (docs/rfcs/withdrawn/0155-manual-test-rfc.md) [withdrawn]<br>`withdrawn` `10078` Manual Test RFC (docs/rfcs/withdrawn/10078-manual-test-rfc.md) [withdrawn] | reviewed and convergent |
| `modes-of-collaboration` | 2 | `stage-4` `0004` Modes of Collaboration (docs/rfcs/stage-4/0004-modes.md) [superseded]<br>`stage-4` `10155` Modes of Collaboration (docs/rfcs/stage-4/10155-modes-of-collaboration.md) [active] | reviewed and convergent |
| `native-integration` | 2 | `withdrawn` `0066` Native Integration (docs/rfcs/withdrawn/0066-native-integration.md) [withdrawn]<br>`withdrawn` `10030` Native Integration (docs/rfcs/withdrawn/10030-native-integration.md) [withdrawn] | reviewed and convergent |
| `native-task-list-integration` | 2 | `withdrawn` `0171` Native Task List Integration (docs/rfcs/withdrawn/0171-native-task-list.md) [withdrawn]<br>`stage-0` `10092` Native Task List Integration (docs/rfcs/stage-0/10092-native-task-list-integration.md) [active] | reviewed and convergent |
| `north-star-user-journey` | 2 | `withdrawn` `0012` North Star User Journey (docs/rfcs/withdrawn/0012-north-star-journey.md) [withdrawn]<br>`withdrawn` `10104` North Star User Journey (docs/rfcs/withdrawn/10104-north-star-user-journey.md) [withdrawn] | reviewed and convergent |
| `operation-context-errors-and-boundary-conversion` | 2 | `stage-4` `0063` Operation-Context Errors and Boundary Conversion (docs/rfcs/stage-4/0063-operation-context-errors-and-boundary-conversion.md) [active]<br>`stage-2` `10027` Operation-Context Errors and Boundary Conversion (docs/rfcs/stage-2/10027-operation-context-errors-and-boundary-conversion.md) [superseded] | reviewed and convergent |
| `ordering-syntax` | 2 | `withdrawn` `0061` Ordering Syntax (docs/rfcs/withdrawn/0061-ordering-syntax.md) [withdrawn]<br>`withdrawn` `10123` Ordering Syntax (docs/rfcs/withdrawn/10123-ordering-syntax.md) [withdrawn] | reviewed and convergent |
| `organic-context-injection-state-aware-agents` | 2 | `withdrawn` `0172` Organic Context Injection (State-Aware Agents) (docs/rfcs/withdrawn/0172-organic-context-injection.md) [withdrawn]<br>`stage-0` `10093` Organic Context Injection (State-Aware Agents) (docs/rfcs/stage-0/10093-organic-context-injection-state-aware-agents.md) [superseded] | reviewed and convergent |
| `phase-aware-dirty-working-tree-steering` | 2 | `stage-4` `0117` Phase-Aware Dirty Working Tree Steering (docs/rfcs/stage-4/0117-phase-aware-dirty-working-tree-steering.md) [active]<br>`withdrawn` `10201` Phase-Aware Dirty Working Tree Steering (docs/rfcs/withdrawn/10201-phase-aware-dirty-working-tree-steering.md) [withdrawn] | reviewed and convergent |
| `phase-state-machine-projections` | 2 | `withdrawn` `0064` Phase State Machine & Projections (docs/rfcs/withdrawn/0064-phase-state-machine--projections.md) [withdrawn]<br>`withdrawn` `10028` Phase State Machine & Projections (docs/rfcs/withdrawn/10028-phase-state-machine--projections.md) [withdrawn] | reviewed and convergent |
| `plan-lens-architecture` | 2 | `withdrawn` `0038` Plan Lens Architecture (docs/rfcs/withdrawn/0038-plan-lens-architecture.md) [withdrawn]<br>`stage-2` `10133` Plan Lens Architecture (docs/rfcs/stage-2/10133-plan-lens-architecture.md) [active] | reviewed and convergent |
| `print-inspired-vertical-rhythm-spacing` | 2 | `stage-1` `0033` Print-Inspired Vertical Rhythm & Spacing (docs/rfcs/stage-1/0033-print-design-spacing.md) [active]<br>`stage-1` `10113` Print-Inspired Vertical Rhythm & Spacing (docs/rfcs/stage-1/10113-print-inspired-vertical-rhythm-spacing.md) [superseded] | reviewed and convergent |
| `prompt-patterns-promptspec-resourcespec-and-cross-spec-interpolation` | 2 | `stage-1` `0060` Prompt Patterns: PromptSpec, ResourceSpec, and Cross-Spec Interpolation (docs/rfcs/stage-1/0060-prompt-patterns-promptspec-resourcespec-and-cross-spec-interpolation.md) [active]<br>`stage-1` `10122` Prompt Patterns: PromptSpec, ResourceSpec, and Cross-Spec Interpolation (docs/rfcs/stage-1/10122-prompt-patterns-promptspec-resourcespec-and-cross-spec-interpolation.md) [superseded] | reviewed and convergent |
| `prompt-workflow-integration` | 2 | `stage-0` `0143` Prompt Workflow Integration (docs/rfcs/stage-0/0143-prompt-workflow-integration.md) [active]<br>`withdrawn` `0145` Prompt Workflow Integration (docs/rfcs/withdrawn/0145-prompt-workflow-integration.md) [withdrawn] | reviewed and convergent |
| `protected-file-watcher-with-revert-and-notice-system` | 2 | `withdrawn` `0091` Protected File Watcher with Revert and Notice System (docs/rfcs/withdrawn/0091-protected-file-watcher.md) [withdrawn]<br>`withdrawn` `10054` Protected File Watcher with Revert and Notice System (docs/rfcs/withdrawn/10054-protected-file-watcher-with-revert-and-notice-system.md) [withdrawn] | reviewed and convergent |
| `protocol-cli-tool-test-dsl-template-holes` | 2 | `stage-1` `0062` Protocol/CLI/Tool Test DSL (Template+Holes) (docs/rfcs/stage-1/0062-protocol-cli-tool-test-dsl-template-holes-.md) [active]<br>`withdrawn` `10026` Protocol/CLI/Tool Test DSL (Template+Holes) (docs/rfcs/withdrawn/10026-protocol-cli-tool-test-dsl-template-holes-.md) [withdrawn] | reviewed and convergent |
| `refined-staged-rfc-process` | 2 | `stage-4` `0108` Refined Staged RFC Process (docs/rfcs/stage-4/0108-refined-staged-rfc-process.md) [active]<br>`withdrawn` `10151` Refined Staged RFC Process (docs/rfcs/withdrawn/10151-refined-staged-rfc-process.md) [withdrawn] | reviewed and convergent |
| `rfc-lifecycle-management-tools-authoring` | 2 | `stage-4` `0120` RFC Lifecycle Management Tools (Authoring) (docs/rfcs/stage-4/0120-rfc-lifecycle-management-tools-authoring.md) [active]<br>`withdrawn` `10148` RFC Lifecycle Management Tools (Authoring) (docs/rfcs/withdrawn/10148-rfc-lifecycle-management-tools-authoring.md) [withdrawn] | reviewed and convergent |
| `rfc-tooling-completion` | 2 | `withdrawn` `0173` RFC Tooling Completion (docs/rfcs/withdrawn/0173-rfctooling-rfc-tooling-completion.md) [withdrawn]<br>`withdrawn` `10095` RFC Tooling Completion (docs/rfcs/withdrawn/10095-rfc-tooling-completion.md) [withdrawn] | reviewed and convergent |
| `rfc-triage-tooling-the-gardener` | 2 | `withdrawn` `0021` RFC Triage Tooling (The Gardener) (docs/rfcs/withdrawn/0021-rfc-triage-tooling.md) [withdrawn]<br>`withdrawn` `10142` RFC Triage Tooling (The Gardener) (docs/rfcs/withdrawn/10142-rfc-triage-tooling-the-gardener.md) [withdrawn] | reviewed and convergent |
| `rich-context-editors` | 2 | `withdrawn` `0005` Rich Context Editors (docs/rfcs/withdrawn/0005-rich-context-editors.md) [withdrawn]<br>`withdrawn` `10156` Rich Context Editors (docs/rfcs/withdrawn/10156-rich-context-editors.md) [withdrawn] | reviewed and convergent |
| `rich-diffs-for-context-editors` | 2 | `stage-0` `0174` Rich Diffs for Context Editors (docs/rfcs/stage-0/0174-rich-diffs.md) [active]<br>`stage-0` `10096` Rich Diffs for Context Editors (docs/rfcs/stage-0/10096-rich-diffs-for-context-editors.md) [superseded] | reviewed and convergent |
| `rich-text-dom-rtd` | 2 | `stage-4` `0020` Rich Text DOM (RTD) (docs/rfcs/stage-4/0020-rtd.md) [superseded]<br>`stage-4` `10159` Rich Text DOM (RTD) (docs/rfcs/stage-4/10159-rich-text-dom-rtd.md) [active] | reviewed and convergent |
| `rigorous-rust-infrastructure` | 2 | `stage-2` `0127` Rigorous Rust Infrastructure (docs/rfcs/stage-2/0127-rigorous-rust-infrastructure.md) [active]<br>`withdrawn` `10109` Rigorous Rust Infrastructure (docs/rfcs/withdrawn/10109-rigorous-rust-infrastructure.md) [withdrawn] | reviewed and convergent |
| `robust-extension-architecture` | 2 | `stage-4` `0110` Robust Extension Architecture (docs/rfcs/stage-4/0110-robust-extension-architecture.md) [active]<br>`withdrawn` `10145` Robust Extension Architecture (docs/rfcs/withdrawn/10145-robust-extension-architecture.md) [withdrawn] | reviewed and convergent |
| `rss-screen-proposals` | 2 | `stage-1` `0014` RSS Screen Proposals (docs/rfcs/stage-1/0014-rss-screen-proposals.md) [active]<br>`stage-1` `10106` RSS Screen Proposals (docs/rfcs/stage-1/10106-rss-screen-proposals.md) [superseded] | reviewed and convergent |
| `rtd-footnote-support` | 2 | `stage-0` `0175` RTD Footnote Support (docs/rfcs/stage-0/0175-rtd-footnotes.md) [active]<br>`stage-0` `10098` RTD Footnote Support (docs/rfcs/stage-0/10098-rtd-footnote-support.md) [superseded] | reviewed and convergent |
| `rtd-surface-mapping` | 2 | `withdrawn` `0015` RTD Surface Mapping (docs/rfcs/withdrawn/0015-rtd-surface-mapping.md) [withdrawn]<br>`withdrawn` `10107` RTD Surface Mapping (docs/rfcs/withdrawn/10107-rtd-surface-mapping.md) [withdrawn] | reviewed and convergent |
| `semantic-merge-driver-for-structured-context-files` | 3 | `withdrawn` `0072` Semantic Merge Driver for Structured Context Files (docs/rfcs/withdrawn/0072-semantic-merge-driver-for-structured-context-files.md) [withdrawn]<br>`withdrawn` `0073` Semantic Merge Driver for Structured Context Files (docs/rfcs/withdrawn/0073-semantic-merge-driver-for-structured-context-files.md) [withdrawn]<br>`withdrawn` `10036` Semantic Merge Driver for Structured Context Files (docs/rfcs/withdrawn/10036-semantic-merge-driver-for-structured-context-files.md) [withdrawn] | reviewed and convergent |
| `staged-rfc-process` | 2 | `stage-4` `0106` Staged RFC Process (docs/rfcs/stage-4/0106-staged-rfc-process.md) [superseded]<br>`withdrawn` `10140` Staged RFC Process (docs/rfcs/withdrawn/10140-staged-rfc-process.md) [withdrawn] | reviewed and convergent |
| `strategic-plan-review` | 2 | `stage-1` `0034` Strategic Plan Review (docs/rfcs/stage-1/0034-strategic-review.md) [active]<br>`stage-1` `10114` Strategic Plan Review (docs/rfcs/stage-1/10114-strategic-plan-review.md) [superseded] | reviewed and convergent |
| `structured-context-api` | 2 | `withdrawn` `0007` Structured Context API (docs/rfcs/withdrawn/0007-structured-context-api.md) [withdrawn]<br>`withdrawn` `10158` Structured Context API (docs/rfcs/withdrawn/10158-structured-context-api.md) [withdrawn] | reviewed and convergent |
| `structured-io-cli` | 2 | `withdrawn` `0077` Structured IO CLI (docs/rfcs/withdrawn/0077-structured-io-cli.md) [withdrawn]<br>`withdrawn` `10128` Structured IO CLI (docs/rfcs/withdrawn/10128-structured-io-cli.md) [withdrawn] | reviewed and convergent |
| `studio-ux-polish` | 2 | `withdrawn` `0028` Studio UX Polish (docs/rfcs/withdrawn/0028-studio-ux-polish.md) [withdrawn]<br>`withdrawn` `10057` Studio UX Polish (docs/rfcs/withdrawn/10057-studio-ux-polish.md) [withdrawn] | reviewed and convergent |
| `studio-visual-language-svl` | 2 | `withdrawn` `0032` Studio Visual Language (SVL) (docs/rfcs/withdrawn/0032-studio-visual-language.md) [withdrawn]<br>`withdrawn` `10060` Studio Visual Language (SVL) (docs/rfcs/withdrawn/10060-studio-visual-language-svl.md) [withdrawn] | reviewed and convergent |
| `surface-refinements` | 2 | `withdrawn` `0065` Surface Refinements (docs/rfcs/withdrawn/0065-surface-refinements.md) [withdrawn]<br>`withdrawn` `10029` Surface Refinements (docs/rfcs/withdrawn/10029-surface-refinements.md) [withdrawn] | reviewed and convergent |
| `surgical-context` | 2 | `withdrawn` `0047` Surgical Context (docs/rfcs/withdrawn/0047-surgical-context.md) [withdrawn]<br>`withdrawn` `10064` Surgical Context (docs/rfcs/withdrawn/10064-surgical-context.md) [withdrawn] | reviewed and convergent |
| `test-driven-development-tdd-workflow-for-agents` | 2 | `stage-0` `0092` Test-Driven Development (TDD) Workflow for Agents (docs/rfcs/stage-0/0092-test-driven-development-tdd-workflow-for-agents.md) [active]<br>`stage-0` `10100` Test-Driven Development (TDD) Workflow for Agents (docs/rfcs/stage-0/10100-test-driven-development-tdd-workflow-for-agents.md) [superseded] | reviewed and convergent |
| `the-agent-quality-loop` | 2 | `withdrawn` `0052` The Agent Quality Loop (docs/rfcs/withdrawn/0052-the-agent-quality-loop.md) [withdrawn]<br>`stage-0` `10069` The Agent Quality Loop (docs/rfcs/stage-0/10069-the-agent-quality-loop.md) [superseded] | reviewed and convergent |
| `the-ai-subcommand-pattern` | 2 | `withdrawn` `0159` The `ai` Subcommand Pattern (docs/rfcs/withdrawn/0159-ai-subcommand-pattern.md) [withdrawn]<br>`withdrawn` `10081` The `ai` Subcommand Pattern (docs/rfcs/withdrawn/10081-the-ai-subcommand-pattern.md) [withdrawn] | reviewed and convergent |
| `the-exo-cli` | 2 | `stage-4` `0018` The `exo` CLI (docs/rfcs/stage-4/0018-exo-cli.md) [active]<br>`withdrawn` `10141` The `exo` CLI (docs/rfcs/withdrawn/10141-the-exo-cli.md) [withdrawn] | reviewed and convergent |
| `the-exosuit-modal-workflows` | 2 | `withdrawn` `0053` The Exosuit Modal Workflows (docs/rfcs/withdrawn/0053-the-exosuit-modal-workflows.md) [withdrawn]<br>`stage-0` `10070` The Exosuit Modal Workflows (docs/rfcs/stage-0/10070-the-exosuit-modal-workflows.md) [superseded] | reviewed and convergent |
| `the-exosuit-release-lifecycle` | 2 | `stage-0` `0037` The Exosuit Release Lifecycle (docs/rfcs/stage-0/0037-release-lifecycle.md) [active]<br>`stage-0` `10062` The Exosuit Release Lifecycle (docs/rfcs/stage-0/10062-the-exosuit-release-lifecycle.md) [superseded] | reviewed and convergent |
| `the-grand-unification-rfc-transition` | 2 | `stage-4` `0123` The Grand Unification (RFC Transition) (docs/rfcs/stage-4/0123-the-grand-unification-rfc-transition.md) [active]<br>`stage-2` `10134` The Grand Unification (RFC Transition) (docs/rfcs/stage-2/10134-the-grand-unification-rfc-transition.md) [superseded] | reviewed and convergent |
| `the-standard-bootstrap` | 2 | `withdrawn` `0138` The Standard Bootstrap (docs/rfcs/withdrawn/0138-the-standard-bootstrap.md) [withdrawn]<br>`withdrawn` `10119` The Standard Bootstrap (docs/rfcs/withdrawn/10119-the-standard-bootstrap.md) [withdrawn] | reviewed and convergent |
| `the-welcome-experience` | 2 | `stage-1` `0025` The Welcome Experience (docs/rfcs/stage-1/0025-welcome-experience.md) [active]<br>`stage-1` `10111` The Welcome Experience (docs/rfcs/stage-1/10111-the-welcome-experience.md) [superseded] | reviewed and convergent |
| `ulid-like-identifiers-ordering-projections-and-human-slugs` | 2 | `withdrawn` `0057` ULID-like Identifiers, Ordering Projections, and Human Slugs (docs/rfcs/withdrawn/0057-ulid-identifiers.md) [withdrawn]<br>`stage-3` `0130` ULID-like Identifiers, Ordering Projections, and Human Slugs (docs/rfcs/stage-3/0130-ulid-like-identifiers-ordering-projections-and-human-slugs.md) [active] | reviewed and convergent |
| `unified-file-and-directory-rendering` | 2 | `withdrawn` `10022` Unified File and Directory Rendering (docs/rfcs/withdrawn/10022-unified-file-and-directory-rendering.md) [withdrawn]<br>`stage-0` `10076` Unified File and Directory Rendering (docs/rfcs/stage-0/10076-unified-file-and-directory-rendering.md) [active] | reviewed and convergent |
| `unified-variable-environment-and-lexical-scope` | 2 | `stage-1` `0045` Unified Variable Environment and Lexical Scope (docs/rfcs/stage-1/0045-unified-variable-environment-and-lexical-scope.md) [active]<br>`stage-1` `0059` Unified Variable Environment and Lexical Scope (docs/rfcs/stage-1/0059-unified-variable-environment-and-lexical-scope.md) [superseded] | reviewed and convergent |
| `user-facing-workflow-model-for-exosuit-vs-code` | 2 | `stage-1` `0043` User-Facing Workflow Model for Exosuit (VS Code) (docs/rfcs/stage-1/0043-user-facing-workflow-model-vscode.md) [superseded]<br>`stage-1` `0056` User-Facing Workflow Model for Exosuit (VS Code) (docs/rfcs/stage-1/0056-user-facing-workflow-model-vscode.md) [superseded] | reviewed and convergent |
| `verified-text-surgery` | 2 | `withdrawn` `10018` Verified Text Surgery (docs/rfcs/withdrawn/10018-verified-text-surgery.md) [withdrawn]<br>`stage-0` `10075` Verified Text Surgery (docs/rfcs/stage-0/10075-verified-text-surgery.md) [active] | reviewed and convergent |
| `vs-code-surface-inventory` | 2 | `withdrawn` `0079` VS Code Surface Inventory (docs/rfcs/withdrawn/0079-vs-code-surface-inventory.md) [withdrawn]<br>`withdrawn` `10050` VS Code Surface Inventory (docs/rfcs/withdrawn/10050-vs-code-surface-inventory.md) [withdrawn] | reviewed and convergent |
| `walkthrough-workflow` | 2 | `withdrawn` `0017` Walkthrough Workflow (docs/rfcs/withdrawn/0017-walkthrough-workflow.md) [withdrawn]<br>`withdrawn` `10198` Walkthrough Workflow (docs/rfcs/withdrawn/10198-walkthrough-workflow.md) [withdrawn] | reviewed and convergent |

## Numeric Identity

All 333 managed documents declare a unique numeric `exo:` anchor. The eight
collision families in the initial inventory, including the final RFC 0060
collision, have been resolved. ID-addressed Exo operations are no longer
blocked by numeric ambiguity.

## Final Audit Candidates

The inventory identifies evidence for the next audit task without deciding the
disposition:

- RFC 00178 has a current-workspace metadata conflict.
- RFCs 0030 and 0040 point to RFC 0080 without reciprocal target entries.
- RFC 0082 points to RFC 0122, while RFC 0122 names only RFC 0141.
- RFC 0103 points to RFC 00225 without reciprocal target metadata.
- RFC 10116 points to missing RFC 10014.
- RFCs 0124 and 10174 point to missing RFCs 0016 and 10071.
- RFC 10172 points to missing RFC 00239.
- RFC 10175 points to missing RFC 0048.
- Older linked-worktree observations retain an RFC 0111 metadata diagnostic;
  the cross-worktree audit must distinguish stale observations from canonical
  document debt.

## Managed Record Index

| Directory | RFC | Metadata Stage | Effective Status | Title | Path |
| --- | ---: | ---: | --- | --- | --- |
| `stage-0` | 0029 | 0 | `active` | Studio RFC View | `docs/rfcs/stage-0/0029-studio-rfc-view.md` |
| `stage-0` | 0031 | 0 | `active` | EARS in Literate Kernel | `docs/rfcs/stage-0/0031-ears-in-literate-kernel.md` |
| `stage-0` | 0035 | 0 | `active` | Exosuit Development Kit (EDK) | `docs/rfcs/stage-0/0035-exosuit-development-kit.md` |
| `stage-0` | 0037 | 0 | `active` | The Exosuit Release Lifecycle | `docs/rfcs/stage-0/0037-release-lifecycle.md` |
| `stage-0` | 0046 | 0 | `active` | Bug Reporting Workflow | `docs/rfcs/stage-0/0046-bug-reporting-workflow.md` |
| `stage-0` | 0051 | 0 | `active` | Coherence Bootstrap | `docs/rfcs/stage-0/0051-coherence-bootstrap.md` |
| `stage-0` | 0070 | 0 | `active` | Resource Protocol and Layered Architecture | `docs/rfcs/stage-0/0070-resource-protocol-and-layered-architecture.md` |
| `stage-0` | 0086 | 0 | `active` | Studio UI Polish and Visual Language | `docs/rfcs/stage-0/0086-studio-ui-polish-and-visual-language.md` |
| `stage-0` | 0087 | 0 | `active` | Agent Quality and Workflow Discipline | `docs/rfcs/stage-0/0087-agent-quality-and-workflow-discipline.md` |
| `stage-0` | 0088 | 0 | `active` | Agent Discipline: Context Injection and Command Control | `docs/rfcs/stage-0/0088-agent-discipline-context-injection-and-command-control.md` |
| `stage-0` | 0092 | 0 | `active` | Test-Driven Development (TDD) Workflow for Agents | `docs/rfcs/stage-0/0092-test-driven-development-tdd-workflow-for-agents.md` |
| `stage-0` | 0095 | 0 | `active` | Intent Mapping & Tool Ergonomics | `docs/rfcs/stage-0/0095-intent-mapping-tool-ergonomics.md` |
| `stage-0` | 0096 | 0 | `active` | VS Code Extension Command Cleanup | `docs/rfcs/stage-0/0096-vscode-command-cleanup.md` |
| `stage-0` | 0102 | 0 | `active` | Webview Manifest for Resource Roots | `docs/rfcs/stage-0/0102-webview-manifest-resource-roots.md` |
| `stage-0` | 0103 | 0 | `superseded` | IDE Diagnostics Integration for Instant Verification | `docs/rfcs/stage-0/0103-ide-diagnostics-integration.md` |
| `stage-0` | 0104 | 0 | `active` | TSConfig Consolidation | `docs/rfcs/stage-0/0104-tsconfig-consolidation.md` |
| `stage-0` | 0112 | 0 | `active` | `pty-responder` - Query-Aware PTY Wrapper | `docs/rfcs/stage-0/0112-pty-responder-query-aware-pty-wrapper.md` |
| `stage-0` | 0142 | 0 | `active` | Agent Ecosystem | `docs/rfcs/stage-0/0142-agent-ecosystem.md` |
| `stage-0` | 0143 | 0 | `active` | Prompt Workflow Integration | `docs/rfcs/stage-0/0143-prompt-workflow-integration.md` |
| `stage-0` | 0146 | 0 | `active` | Vercel AI Gateway: Stream Feature Mapping & Integration Fixes | `docs/rfcs/stage-0/0146-vercel-ai-gateway-stream-feature-mapping.md` |
| `stage-0` | 0147 | 0 | `active` | LM Tool: Read Output Channel Logs | `docs/rfcs/stage-0/0147-lm-tool-read-output-channel-logs.md` |
| `stage-0` | 0149 | 0 | `active` | Axiom System Integration | `docs/rfcs/stage-0/0149-axiom-system-integration.md` |
| `stage-0` | 0153 | 0 | `active` | CLI-Based Test Fixture Setup | `docs/rfcs/stage-0/0153-cli-based-test-fixture-setup.md` |
| `stage-0` | 0154 | 0 | `active` | Steering Confidence Model | `docs/rfcs/stage-0/0154-steering-confidence-model.md` |
| `stage-0` | 0162 | 0 | `active` | Exposing RFCs as Copilot Resources | `docs/rfcs/stage-0/0162-copilot-resources.md` |
| `stage-0` | 0164 | 0 | `active` | Directory-Based RFC Organization | `docs/rfcs/stage-0/0164-directory-based-rfcs.md` |
| `stage-0` | 0169 | 0 | `active` | Local RAG Architecture (Rust/Wasm) | `docs/rfcs/stage-0/0169-local-rag-architecture.md` |
| `stage-0` | 0174 | 0 | `active` | Rich Diffs for Context Editors | `docs/rfcs/stage-0/0174-rich-diffs.md` |
| `stage-0` | 0175 | 0 | `active` | RTD Footnote Support | `docs/rfcs/stage-0/0175-rtd-footnotes.md` |
| `stage-0` | 0181 | 0 | `active` | Multipart Tool Responses for LM Steering | `docs/rfcs/stage-0/00181-multipart-tool-responses-for-lm-steering.md` |
| `stage-0` | 0182 | 0 | `active` | Zero-Arg Tool Completeness | `docs/rfcs/stage-0/00182-zero-arg-tool-completeness.md` |
| `stage-0` | 0186 | 0 | `active` | Studio Log Linking | `docs/rfcs/stage-0/00186-studio-log-linking.md` |
| `stage-0` | 0214 | 0 | `active` | Autofix Check Ordering | `docs/rfcs/stage-0/00214-autofix-check-ordering.md` |
| `stage-0` | 0221 | 0 | `active` | Strategic Overview E2E Behavior | `docs/rfcs/stage-0/00221-strategic-overview-e2e-behavior.md` |
| `stage-0` | 0231 | 0 | `active` | Chore Phases: Automated Interstitial Work | `docs/rfcs/stage-0/00231-chore-phases-automated-interstitial-work.md` |
| `stage-0` | 0235 | 0 | `active` | Whiteboard Priorities: Symbol Registration, Pre-Ship Polish, and Workflow Gaps | `docs/rfcs/stage-0/00235-whiteboard-priorities-symbol-registration-pre-ship-polish-and-workflow-gaps.md` |
| `stage-0` | 0236 | 0 | `active` | Resource Projections: Exposing Reactive State as Agent-Discoverable Symbols | `docs/rfcs/stage-0/00236-computed-projections-exposing-reactive-state-as-agent-discoverable-symbols.md` |
| `stage-0` | 10032 | 0 | `active` | Position Protocol for Ordered Lists | `docs/rfcs/stage-0/10032-position-protocol-for-ordered-lists.md` |
| `stage-0` | 10055 | 0 | `active` | Dedicated Format for Agent Context Links | `docs/rfcs/stage-0/10055-dedicated-format-for-agent-context-links.md` |
| `stage-0` | 10056 | 0 | `superseded` | Consolidate Agent Workflow into rfc-status | `docs/rfcs/stage-0/10056-consolidate-agent-workflow-into-rfc-status.md` |
| `stage-0` | 10061 | 0 | `superseded` | Exosuit Development Kit (EDK) | `docs/rfcs/stage-0/10061-exosuit-development-kit-edk.md` |
| `stage-0` | 10062 | 0 | `superseded` | The Exosuit Release Lifecycle | `docs/rfcs/stage-0/10062-the-exosuit-release-lifecycle.md` |
| `stage-0` | 10063 | 0 | `superseded` | Bug Reporting Workflow | `docs/rfcs/stage-0/10063-bug-reporting-workflow.md` |
| `stage-0` | 10066 | 0 | `active` | Property Testing Strategy for Reactivity | `docs/rfcs/stage-0/10066-property-testing-strategy-for-reactivity.md` |
| `stage-0` | 10068 | 0 | `superseded` | Coherence Bootstrap | `docs/rfcs/stage-0/10068-coherence-bootstrap.md` |
| `stage-0` | 10069 | 0 | `superseded` | The Agent Quality Loop | `docs/rfcs/stage-0/10069-the-agent-quality-loop.md` |
| `stage-0` | 10070 | 0 | `superseded` | The Exosuit Modal Workflows | `docs/rfcs/stage-0/10070-the-exosuit-modal-workflows.md` |
| `stage-0` | 10075 | 0 | `active` | Verified Text Surgery | `docs/rfcs/stage-0/10075-verified-text-surgery.md` |
| `stage-0` | 10076 | 0 | `active` | Unified File and Directory Rendering | `docs/rfcs/stage-0/10076-unified-file-and-directory-rendering.md` |
| `stage-0` | 10080 | 0 | `superseded` | Agent CWD Discipline (Rooted Execution) | `docs/rfcs/stage-0/10080-agent-cwd-discipline-rooted-execution.md` |
| `stage-0` | 10084 | 0 | `superseded` | CWD Discipline & Wrong Directory Mitigation | `docs/rfcs/stage-0/10084-cwd-discipline-wrong-directory-mitigation.md` |
| `stage-0` | 10090 | 0 | `superseded` | Local RAG Architecture (Rust/Wasm) | `docs/rfcs/stage-0/10090-local-rag-architecture-rust-wasm.md` |
| `stage-0` | 10092 | 0 | `active` | Native Task List Integration | `docs/rfcs/stage-0/10092-native-task-list-integration.md` |
| `stage-0` | 10093 | 0 | `superseded` | Organic Context Injection (State-Aware Agents) | `docs/rfcs/stage-0/10093-organic-context-injection-state-aware-agents.md` |
| `stage-0` | 10096 | 0 | `superseded` | Rich Diffs for Context Editors | `docs/rfcs/stage-0/10096-rich-diffs-for-context-editors.md` |
| `stage-0` | 10098 | 0 | `superseded` | RTD Footnote Support | `docs/rfcs/stage-0/10098-rtd-footnote-support.md` |
| `stage-0` | 10100 | 0 | `superseded` | Test-Driven Development (TDD) Workflow for Agents | `docs/rfcs/stage-0/10100-test-driven-development-tdd-workflow-for-agents.md` |
| `stage-0` | 10164 | 0 | `active` | Principled Invalidation: Pure Trace Matching and Resource Pattern | `docs/rfcs/stage-0/10164-principled-invalidation-pure-trace-matching-and-resource-pattern.md` |
| `stage-0` | 10167 | 0 | `active` | Unified Derivation Layer | `docs/rfcs/stage-0/10167-unified-derivation-layer.md` |
| `stage-0` | 10169 | 0 | `superseded` | FileDecoration-Based Tree Item Styling | `docs/rfcs/stage-0/10169-filedecoration-based-tree-item-styling.md` |
| `stage-0` | 10189 | 0 | `active` | Sidecar Sync Contract and Machine Portability Policy | `docs/rfcs/stage-0/10189-sidecar-sync-contract-and-machine-portability-policy.md` |
| `stage-0` | 10191 | 0 | `active` | Sidecar Write Ownership and Stale Writer Fencing | `docs/rfcs/stage-0/10191-sidecar-write-ownership-and-stale-writer-fencing.md` |
| `stage-0` | 10192 | 0 | `active` | Epoch-Owned Sidecar Collaboration | `docs/rfcs/stage-0/10192-epoch-owned-sidecar-collaboration.md` |
| `stage-0` | 10193 | 0 | `active` | Codex Integration and Cockpit Adapter | `docs/rfcs/stage-0/10193-codex-integration-and-cockpit-adapter.md` |
| `stage-0` | 10197 | 0 | `active` | Cockpit Project Catalog and CommandSpec API Boundary | `docs/rfcs/stage-0/10197-cockpit-project-catalog-and-commandspec-api-boundary.md` |
| `stage-1` | 0008 | 1 | `active` | AI Activity Visualization | `docs/rfcs/stage-1/0008-ai-activity-visualization.md` |
| `stage-1` | 0011 | 1 | `active` | Ideas & Triage System | `docs/rfcs/stage-1/0011-ideas-and-triage.md` |
| `stage-1` | 0014 | 1 | `active` | RSS Screen Proposals | `docs/rfcs/stage-1/0014-rss-screen-proposals.md` |
| `stage-1` | 0025 | 1 | `active` | The Welcome Experience | `docs/rfcs/stage-1/0025-welcome-experience.md` |
| `stage-1` | 0033 | 1 | `active` | Print-Inspired Vertical Rhythm & Spacing | `docs/rfcs/stage-1/0033-print-design-spacing.md` |
| `stage-1` | 0034 | 1 | `active` | Strategic Plan Review | `docs/rfcs/stage-1/0034-strategic-review.md` |
| `stage-1` | 0041 | 1 | `active` | AI Tool Affordances | `docs/rfcs/stage-1/0041-ai-tool-affordances.md` |
| `stage-1` | 0043 | 1 | `superseded` | User-Facing Workflow Model for Exosuit (VS Code) | `docs/rfcs/stage-1/0043-user-facing-workflow-model-vscode.md` |
| `stage-1` | 0044 | 1 | `active` | Declarative Task Recipes in exosuit.toml | `docs/rfcs/stage-1/0044-declarative-task-recipes-in-exosuit-toml.md` |
| `stage-1` | 0045 | 1 | `active` | Unified Variable Environment and Lexical Scope | `docs/rfcs/stage-1/0045-unified-variable-environment-and-lexical-scope.md` |
| `stage-1` | 0056 | 1 | `superseded` | User-Facing Workflow Model for Exosuit (VS Code) | `docs/rfcs/stage-1/0056-user-facing-workflow-model-vscode.md` |
| `stage-1` | 0058 | 1 | `superseded` | Declarative Task Recipes in exosuit.toml | `docs/rfcs/stage-1/0058-declarative-task-recipes-in-exosuit-toml.md` |
| `stage-1` | 0059 | 1 | `superseded` | Unified Variable Environment and Lexical Scope | `docs/rfcs/stage-1/0059-unified-variable-environment-and-lexical-scope.md` |
| `stage-1` | 0060 | 1 | `active` | Prompt Patterns: PromptSpec, ResourceSpec, and Cross-Spec Interpolation | `docs/rfcs/stage-1/0060-prompt-patterns-promptspec-resourcespec-and-cross-spec-interpolation.md` |
| `stage-1` | 0062 | 1 | `active` | Protocol/CLI/Tool Test DSL (Template+Holes) | `docs/rfcs/stage-1/0062-protocol-cli-tool-test-dsl-template-holes-.md` |
| `stage-1` | 0074 | 1 | `active` | E2E Verified Dashboard Behavior | `docs/rfcs/stage-1/0074-e2e-verified-dashboard-behavior.md` |
| `stage-1` | 0076 | 1 | `active` | Formal Spec Frontmatter Upgrade | `docs/rfcs/stage-1/0076-spec-frontmatter-upgrade.md` |
| `stage-1` | 0089 | 1 | `active` | The Exosuit Application Architecture | `docs/rfcs/stage-1/0089-the-exosuit-application-architecture.md` |
| `stage-1` | 0094 | 1 | `superseded` | Sidebar-First UI Design | `docs/rfcs/stage-1/0094-sidebar-first-ui-design.md` |
| `stage-1` | 0097 | 1 | `active` | Machine Channel: Unified Server Architecture | `docs/rfcs/stage-1/0097-machine-channel-unified-server-architecture.md` |
| `stage-1` | 0113 | 1 | `active` | Exohook Machine Channel Protocol | `docs/rfcs/stage-1/0113-exohook-machine-channel-protocol.md` |
| `stage-1` | 0152 | 1 | `active` | Workspace Cleanup and Coherence Restoration | `docs/rfcs/stage-1/0152-workspace-cleanup-and-coherence-restoration.md` |
| `stage-1` | 0178 | 1 | `active` | Instruction Localization Convention | `docs/rfcs/stage-1/00178-instruction-localization-convention.md` |
| `stage-1` | 0187 | 1 | `active` | Collapse Transitioning Mode into Context-Aware BetweenPhases | `docs/rfcs/stage-1/00187-collapse-transitioning-mode-into-context-aware-betweenphases.md` |
| `stage-1` | 0215 | 1 | `active` | Hooks Config Ergonomics | `docs/rfcs/stage-1/00215-hooks-config-ergonomics.md` |
| `stage-1` | 0223 | 1 | `active` | CLI Namespace Consolidation | `docs/rfcs/stage-1/00223-cli-namespace-consolidation.md` |
| `stage-1` | 0224 | 1 | `active` | The SOAR Loop: A Workflow Model for Human-AI Collaboration | `docs/rfcs/stage-1/00224-the-soar-loop-a-workflow-model-for-human-ai-collaboration.md` |
| `stage-1` | 0225 | 1 | `active` | Problems Pane Integration with SOAR Loop | `docs/rfcs/stage-1/00225-problems-pane-integration-with-soar-loop.md` |
| `stage-1` | 0226 | 1 | `active` | Lightweight RFC Path: Minimal Ceremony for Simple Decisions | `docs/rfcs/stage-1/00226-lightweight-rfc-path-minimal-ceremony-for-simple-decisions.md` |
| `stage-1` | 0228 | 1 | `active` | Terminology Normalization: Goal/Task Hierarchy | `docs/rfcs/stage-1/00228-terminology-normalization-goal-task-hierarchy.md` |
| `stage-1` | 0230 | 1 | `active` | Goals as PER Cycles: Unifying Granularity and Execution | `docs/rfcs/stage-1/00230-goals-as-per-cycles-unifying-granularity-and-execution.md` |
| `stage-1` | 0232 | 1 | `superseded` | Situational Awareness: Progressive Context Visualization | `docs/rfcs/stage-1/00232-situational-awareness-progressive-context-visualization.md` |
| `stage-1` | 0234 | 1 | `active` | Command Output Boundary: Eliminating println! Pollution | `docs/rfcs/stage-1/00234-command-output-boundary-eliminating-println-pollution.md` |
| `stage-1` | 0237 | 1 | `active` | Dynamic Derived Roots (Reactive Families) | `docs/rfcs/stage-1/00237-dynamic-derived-roots.md` |
| `stage-1` | 0238 | 1 | `active` | Pipeline-Aware Self-Model: Making the System Perceive and Steer Itself | `docs/rfcs/stage-1/00238-pipeline-aware-self-model.md` |
| `stage-1` | 0240 | 1 | `active` | Fractal SOAR & The Goal Loop | `docs/rfcs/stage-1/00240-fractal-soar-the-goal-loop.md` |
| `stage-1` | 0241 | 1 | `active` | Reactive State Roots: Mutable Application State in the Reactive Graph | `docs/rfcs/stage-1/00241-reactive-state-roots-mutable-application-state-in-the-reactive-graph.md` |
| `stage-1` | 0242 | 1 | `active` | Progress Tool: Task Logs as Lightweight Steering | `docs/rfcs/stage-1/00242-progress-tool-lightweight-steering-during-goal-execution.md` |
| `stage-1` | 0243 | 1 | `active` | Developing Exo: Hot Reload and Build Workflow | `docs/rfcs/stage-1/00243-developing-exo-hot-reload-and-build-workflow.md` |
| `stage-1` | 10101 | 1 | `superseded` | AI Activity Visualization | `docs/rfcs/stage-1/10101-ai-activity-visualization.md` |
| `stage-1` | 10102 | 1 | `superseded` | Dynamic Planning | `docs/rfcs/stage-1/10102-dynamic-planning.md` |
| `stage-1` | 10103 | 1 | `superseded` | Ideas & Triage System | `docs/rfcs/stage-1/10103-ideas-triage-system.md` |
| `stage-1` | 10106 | 1 | `superseded` | RSS Screen Proposals | `docs/rfcs/stage-1/10106-rss-screen-proposals.md` |
| `stage-1` | 10111 | 1 | `superseded` | The Welcome Experience | `docs/rfcs/stage-1/10111-the-welcome-experience.md` |
| `stage-1` | 10112 | 1 | `active` | Chat History & Session Recovery | `docs/rfcs/stage-1/10112-context-memory-search.md` |
| `stage-1` | 10113 | 1 | `superseded` | Print-Inspired Vertical Rhythm & Spacing | `docs/rfcs/stage-1/10113-print-inspired-vertical-rhythm-spacing.md` |
| `stage-1` | 10114 | 1 | `superseded` | Strategic Plan Review | `docs/rfcs/stage-1/10114-strategic-plan-review.md` |
| `stage-1` | 10116 | 1 | `superseded` | Untitled | `docs/rfcs/stage-1/10116-untitled.md` |
| `stage-1` | 10117 | 1 | `superseded` | AI Tool Affordances | `docs/rfcs/stage-1/10117-ai-tool-affordances.md` |
| `stage-1` | 10122 | 1 | `superseded` | Prompt Patterns: PromptSpec, ResourceSpec, and Cross-Spec Interpolation | `docs/rfcs/stage-1/10122-prompt-patterns-promptspec-resourcespec-and-cross-spec-interpolation.md` |
| `stage-1` | 10124 | 1 | `superseded` | E2E Verified Dashboard Behavior | `docs/rfcs/stage-1/10124-e2e-verified-dashboard-behavior.md` |
| `stage-1` | 10126 | 1 | `active` | Distinguishing Spec vs. Work RFCs | `docs/rfcs/stage-1/10126-distinguishing-spec-vs-work-rfcs.md` |
| `stage-1` | 10131 | 1 | `active` | Optimistic UI Guard — Transient Signals and Echo Suppression | `docs/rfcs/stage-1/10131-optimistic-ui-guard-canonicalization.md` |
| `stage-1` | 10166 | 1 | `active` | Architect Agent Mode (Two-Agent Workflow) | `docs/rfcs/stage-1/10166-architect-agent-mode-two-agent-workflow.md` |
| `stage-1` | 10172 | 1 | `active` | Sidebar Visual Design: Principles, Mechanisms, and Pipeline Notation | `docs/rfcs/stage-1/10172-sidebar-visual-design-principles-mechanisms-and-pipeline-notation.md` |
| `stage-1` | 10173 | 1 | `active` | Phase Details: Unified Derived Root and Progressive Context | `docs/rfcs/stage-1/10173-phase-details-unified-derived-root-and-progressive-context.md` |
| `stage-1` | 10174 | 1 | `active` | Inbox System (Hierarchical Intent Queue) | `docs/rfcs/stage-1/10174-hierarchical-intent-queue-scope-aware-scheduling-and-disposition.md` |
| `stage-1` | 10178 | 1 | `active` | Git-Friendly Serialization: Sorted SQL Text Dumps | `docs/rfcs/stage-1/10178-git-friendly-serialization-sorted-sql-text-dumps.md` |
| `stage-1` | 10180 | 1 | `active` | Storage Disposition: Canonical State, Configuration, and Documents | `docs/rfcs/stage-1/10180-storage-disposition-canonical-state-configuration-and-documents.md` |
| `stage-1` | 10184 | 1 | `active` | Project / Workspace / Worktree: unbundling the conflated root | `docs/rfcs/stage-1/10184-project-workspace-worktree-unbundling-the-conflated-root.md` |
| `stage-1` | 10187 | 1 | `active` | GitHub Profile Sidecar Discovery | `docs/rfcs/stage-1/10187-github-profile-sidecar-discovery.md` |
| `stage-1` | 10188 | 1 | `active` | Sidecar Onboarding and Setup Flows | `docs/rfcs/stage-1/10188-sidecar-onboarding-and-setup-flows.md` |
| `stage-1` | 10190 | 1 | `active` | Durable MCP Proxy and Hot-Swappable Exo Worker | `docs/rfcs/stage-1/10190-durable-mcp-proxy-and-hot-swappable-worker.md` |
| `stage-1` | 10194 | 1 | `active` | Command Surface Coherence and the Shared Exo Command Language | `docs/rfcs/stage-1/10194-command-text-frontend-and-parser-library-evaluation.md` |
| `stage-1` | 10195 | 1 | `active` | Daemon Lifecycle Authority and Shared Perception Surfaces | `docs/rfcs/stage-1/10195-daemon-lifecycle-authority-and-shared-perception-surfaces.md` |
| `stage-1` | 10200 | 1 | `active` | CLI-Shaped exo-run MCP Transport | `docs/rfcs/stage-1/10200-cli-shaped-exo-run-mcp-transport.md` |
| `stage-1` | 10202 | 1 | `active` | Lane-Centered Workbench Adoption | `docs/rfcs/stage-1/10202-lane-centered-workbench-adoption.md` |
| `stage-2` | 0071 | 2 | `active` | Reactive Collections: Directory Listing and Writable Projections | `docs/rfcs/stage-2/0071-reactive-collections-directory-listing-and-writable-projections.md` |
| `stage-2` | 0127 | 2 | `active` | Rigorous Rust Infrastructure | `docs/rfcs/stage-2/0127-rigorous-rust-infrastructure.md` |
| `stage-2` | 0233 | 2 | `active` | ExoSpec: Unified Command Definition and the End of Dual-Source Drift | `docs/rfcs/stage-2/00233-exospec-unified-command-definition-and-the-end-of-dual-source-drift.md` |
| `stage-2` | 10027 | 2 | `superseded` | Operation-Context Errors and Boundary Conversion | `docs/rfcs/stage-2/10027-operation-context-errors-and-boundary-conversion.md` |
| `stage-2` | 10133 | 2 | `active` | Plan Lens Architecture | `docs/rfcs/stage-2/10133-plan-lens-architecture.md` |
| `stage-2` | 10134 | 2 | `superseded` | The Grand Unification (RFC Transition) | `docs/rfcs/stage-2/10134-the-grand-unification-rfc-transition.md` |
| `stage-2` | 10136 | 2 | `superseded` | Dashboard V2 | `docs/rfcs/stage-2/10136-dashboard-v2.md` |
| `stage-2` | 10163 | 2 | `active` | LM Tool Surface Reduction via CLI Delegation | `docs/rfcs/stage-2/10163-lm-tool-surface-reduction-via-cli-delegation.md` |
| `stage-2` | 10177 | 2 | `active` | Local XDG: Project-Scoped Directory Conventions | `docs/rfcs/stage-2/10177-local-xdg-project-scoped-directory-conventions.md` |
| `stage-2` | 10181 | 2 | `active` | Shared Perception: Inbox as a Steering Channel | `docs/rfcs/stage-2/10181-shared-perception-inbox-as-a-steering-channel.md` |
| `stage-2` | 10182 | 2 | `active` | Contextual Steering: Every Command is a Perception Touchpoint | `docs/rfcs/stage-2/10182-contextual-steering-every-command-is-a-perception-touchpoint.md` |
| `stage-2` | 10183 | 2 | `active` | Agent Activity Model: Event Sourcing for Steering Context | `docs/rfcs/stage-2/10183-agent-activity-model-event-sourcing-for-steering-context.md` |
| `stage-3` | 0069 | 3 | `active` | Canonical ULIDs, Scoped Slugs, and RFC Corpus Control | `docs/rfcs/stage-3/0069-canonical-ulids-scoped-slugs-and-rfc-corpus-control.md` |
| `stage-3` | 0080 | 3 | `active` | Agent-first CLI Discovery Ladder | `docs/rfcs/stage-3/0080-agent-first-cli-discovery-ladder.md` |
| `stage-3` | 0081 | 3 | `active` | Exohook: File Expansion Worked Examples | `docs/rfcs/stage-3/0081-exohook-file-expansion-worked-examples.md` |
| `stage-3` | 0125 | 3 | `active` | Exosuit Capability Tree + Machine Channel v1 | `docs/rfcs/stage-3/0125-exosuit-capability-tree-machine-channel-v1.md` |
| `stage-3` | 0128 | 3 | `active` | The Exo-Shell Pattern (Unified Command Interface) | `docs/rfcs/stage-3/0128-the-exo-shell-pattern-unified-command-interface.md` |
| `stage-3` | 0130 | 3 | `active` | ULID-like Identifiers, Ordering Projections, and Human Slugs | `docs/rfcs/stage-3/0130-ulid-like-identifiers-ordering-projections-and-human-slugs.md` |
| `stage-3` | 0132 | 3 | `active` | CLI Patterns: Command Spec, Router, and Tool-Safe DSL | `docs/rfcs/stage-3/0132-cli-patterns-command-spec-router-and-tool-safe-dsl.md` |
| `stage-3` | 0136 | 3 | `active` | LM Tool Architecture v2 | `docs/rfcs/stage-3/0136-lm-tool-architecture-v2.md` |
| `stage-3` | 0137 | 3 | `active` | Exohook CI Workflow Projection | `docs/rfcs/stage-3/0137-exohook-ci-workflow-projection.md` |
| `stage-3` | 0184 | 3 | `active` | Mode-Aware Sidebar Cockpit | `docs/rfcs/stage-3/00184-mode-aware-sidebar-cockpit.md` |
| `stage-3` | 0200 | 3 | `active` | CLI Argument Consistency: Natural Positional, Named Flags for the Rest | `docs/rfcs/stage-3/0200-cli-argument-consistency.md` |
| `stage-3` | 10165 | 3 | `active` | Reactive SQLite: Virtual Table Integration with Revision Algebra | `docs/rfcs/stage-3/10165-reactive-sqlite-virtual-table-integration-with-revision-algebra.md` |
| `stage-3` | 10170 | 3 | `active` | Mutation Boundaries in Feedback Loops | `docs/rfcs/stage-3/10170-mutation-boundaries-in-feedback-loops.md` |
| `stage-3` | 10175 | 3 | `active` | Surgical Strikes as Goals | `docs/rfcs/stage-3/10175-surgical-strikes-as-goals.md` |
| `stage-3` | 10176 | 3 | `active` | Project State Model | `docs/rfcs/stage-3/10176-project-state-model.md` |
| `stage-3` | 10179 | 3 | `active` | Binary Re-exec: Workspace-Local Development Builds | `docs/rfcs/stage-3/10179-binary-re-exec-workspace-local-development-builds.md` |
| `stage-3` | 10196 | 3 | `active` | Worktree-Aware Sidecar State and Branch-Local Document Overlays | `docs/rfcs/stage-3/10196-worktree-aware-sidecar-state-and-branch-local-document-overlays.md` |
| `stage-4` | 0002 | 4 | `superseded` | Design Axioms | `docs/rfcs/stage-4/0002-axioms.md` |
| `stage-4` | 0004 | 4 | `superseded` | Modes of Collaboration | `docs/rfcs/stage-4/0004-modes.md` |
| `stage-4` | 0006 | 4 | `active` | Workspace Cache | `docs/rfcs/stage-4/0006-workspace-cache.md` |
| `stage-4` | 0018 | 4 | `active` | The `exo` CLI | `docs/rfcs/stage-4/0018-exo-cli.md` |
| `stage-4` | 0020 | 4 | `superseded` | Rich Text DOM (RTD) | `docs/rfcs/stage-4/0020-rtd.md` |
| `stage-4` | 0024 | 4 | `superseded` | Exosuit UI Architecture | `docs/rfcs/stage-4/0024-ui-architecture.md` |
| `stage-4` | 0063 | 4 | `active` | Operation-Context Errors and Boundary Conversion | `docs/rfcs/stage-4/0063-operation-context-errors-and-boundary-conversion.md` |
| `stage-4` | 0085 | 4 | `active` | Command Trait Architecture | `docs/rfcs/stage-4/0085-command-trait-architecture.md` |
| `stage-4` | 0099 | 4 | `active` | Exohook Adaptive Terminal Width | `docs/rfcs/stage-4/0099-exohook-adaptive-terminal-width.md` |
| `stage-4` | 0106 | 4 | `superseded` | Staged RFC Process | `docs/rfcs/stage-4/0106-staged-rfc-process.md` |
| `stage-4` | 0108 | 4 | `active` | Refined Staged RFC Process | `docs/rfcs/stage-4/0108-refined-staged-rfc-process.md` |
| `stage-4` | 0110 | 4 | `active` | Robust Extension Architecture | `docs/rfcs/stage-4/0110-robust-extension-architecture.md` |
| `stage-4` | 0111 | 4 | `active` | Agent Guidance Architecture | `docs/rfcs/stage-4/0111-agent-guidance-architecture.md` |
| `stage-4` | 0115 | 4 | `active` | Externalized Prompts | `docs/rfcs/stage-4/0115-externalized-prompts.md` |
| `stage-4` | 0117 | 4 | `active` | Phase-Aware Dirty Working Tree Steering | `docs/rfcs/stage-4/0117-phase-aware-dirty-working-tree-steering.md` |
| `stage-4` | 0120 | 4 | `active` | RFC Lifecycle Management Tools (Authoring) | `docs/rfcs/stage-4/0120-rfc-lifecycle-management-tools-authoring.md` |
| `stage-4` | 0121 | 4 | `active` | Shared Agent Runtime | `docs/rfcs/stage-4/0121-shared-agent-runtime.md` |
| `stage-4` | 0122 | 4 | `active` | Exohook Streaming Progress Reporting | `docs/rfcs/stage-4/0122-exohook-streaming-progress-reporting.md` |
| `stage-4` | 0123 | 4 | `active` | The Grand Unification (RFC Transition) | `docs/rfcs/stage-4/0123-the-grand-unification-rfc-transition.md` |
| `stage-4` | 0156 | 4 | `active` | CLI Command for Axioms | `docs/rfcs/stage-4/0156-cli-command-for-axioms.md` |
| `stage-4` | 10153 | 4 | `active` | Design Axioms | `docs/rfcs/stage-4/10153-design-axioms.md` |
| `stage-4` | 10154 | 4 | `active` | Context Persistence | `docs/rfcs/stage-4/10154-context-persistence.md` |
| `stage-4` | 10155 | 4 | `active` | Modes of Collaboration | `docs/rfcs/stage-4/10155-modes-of-collaboration.md` |
| `stage-4` | 10159 | 4 | `active` | Rich Text DOM (RTD) | `docs/rfcs/stage-4/10159-rich-text-dom-rtd.md` |
| `stage-4` | 10162 | 4 | `active` | Exosuit UI Architecture | `docs/rfcs/stage-4/10162-exosuit-ui-architecture.md` |
| `archive` | 0022 | 4 | `archived` | Unified Project State | `docs/rfcs/archive/0022-unified-project-state.md` |
| `archive` | 0116 | 4 | `archived` | Feedback System | `docs/rfcs/archive/0116-feedback-system.md` |
| `archive` | 0124 | 3 | `archived` | Inbox System: Async Intent Channel | `docs/rfcs/archive/0124-async-intent-channel.md` |
| `withdrawn` | 0001 | 0 | `withdrawn` | Agent-Centric CLI Design (The Conversationalist) | `docs/rfcs/withdrawn/0001-agent-centric-cli.md` |
| `withdrawn` | 0005 | 4 | `withdrawn` | Rich Context Editors | `docs/rfcs/withdrawn/0005-rich-context-editors.md` |
| `withdrawn` | 0007 | 4 | `withdrawn` | Structured Context API | `docs/rfcs/withdrawn/0007-structured-context-api.md` |
| `withdrawn` | 0009 | 0 | `withdrawn` | Dynamic Planning | `docs/rfcs/withdrawn/0009-dynamic-planning.md` |
| `withdrawn` | 0010 | 0 | `withdrawn` | Dedicated Format for Agent Context Links | `docs/rfcs/withdrawn/0010-agent-context-links.md` |
| `withdrawn` | 0012 | 0 | `withdrawn` | North Star User Journey | `docs/rfcs/withdrawn/0012-north-star-journey.md` |
| `withdrawn` | 0015 | 1 | `withdrawn` | RTD Surface Mapping | `docs/rfcs/withdrawn/0015-rtd-surface-mapping.md` |
| `withdrawn` | 0017 | 0 | `withdrawn` | Walkthrough Workflow | `docs/rfcs/withdrawn/0017-walkthrough-workflow.md` |
| `withdrawn` | 0019 | 0 | `withdrawn` | Consolidate Agent Workflow into rfc-status | `docs/rfcs/withdrawn/0019-rfc-status-commands.md` |
| `withdrawn` | 0021 | 3 | `withdrawn` | RFC Triage Tooling (The Gardener) | `docs/rfcs/withdrawn/0021-rfc-triage-tooling.md` |
| `withdrawn` | 0028 | 0 | `withdrawn` | Studio UX Polish | `docs/rfcs/withdrawn/0028-studio-ux-polish.md` |
| `withdrawn` | 0030 | 0 | `withdrawn` | CLI Overhaul & Alignment | `docs/rfcs/withdrawn/0030-cli-overhaul.md` |
| `withdrawn` | 0032 | 0 | `withdrawn` | Studio Visual Language (SVL) | `docs/rfcs/withdrawn/0032-studio-visual-language.md` |
| `withdrawn` | 0038 | 0 | `withdrawn` | Plan Lens Architecture | `docs/rfcs/withdrawn/0038-plan-lens-architecture.md` |
| `withdrawn` | 0039 | 0 | `withdrawn` | Plan Lens Architecture (Withdrawn) | `docs/rfcs/withdrawn/0039-plan-lens-architecture.md` |
| `withdrawn` | 0040 | 0 | `withdrawn` | RFC 40 | `docs/rfcs/withdrawn/0040-vscode-cli-discovery.md` |
| `withdrawn` | 0042 | 0 | `withdrawn` | CLI AST Tool Schema | `docs/rfcs/withdrawn/0042-cli-ast-tool-schema.md` |
| `withdrawn` | 0047 | 0 | `withdrawn` | Surgical Context | `docs/rfcs/withdrawn/0047-surgical-context.md` |
| `withdrawn` | 0050 | 0 | `withdrawn` | eBPF Auto Instrumentation | `docs/rfcs/withdrawn/0050-ebpf-auto-instrumentation.md` |
| `withdrawn` | 0052 | 0 | `withdrawn` | The Agent Quality Loop | `docs/rfcs/withdrawn/0052-the-agent-quality-loop.md` |
| `withdrawn` | 0053 | 0 | `withdrawn` | The Exosuit Modal Workflows | `docs/rfcs/withdrawn/0053-the-exosuit-modal-workflows.md` |
| `withdrawn` | 0054 | 0 | `withdrawn` | The Context Inbox (Pull-Based Attention) | `docs/rfcs/withdrawn/0054-context-inbox.md` |
| `withdrawn` | 0055 | 0 | `withdrawn` | exo map Command | `docs/rfcs/withdrawn/0055-exo-map-command.md` |
| `withdrawn` | 0057 | 1 | `withdrawn` | ULID-like Identifiers, Ordering Projections, and Human Slugs | `docs/rfcs/withdrawn/0057-ulid-identifiers.md` |
| `withdrawn` | 0061 | 0 | `withdrawn` | Ordering Syntax | `docs/rfcs/withdrawn/0061-ordering-syntax.md` |
| `withdrawn` | 0064 | 2 | `withdrawn` | Phase State Machine & Projections | `docs/rfcs/withdrawn/0064-phase-state-machine--projections.md` |
| `withdrawn` | 0065 | 0 | `withdrawn` | Surface Refinements | `docs/rfcs/withdrawn/0065-surface-refinements.md` |
| `withdrawn` | 0066 | 0 | `withdrawn` | Native Integration | `docs/rfcs/withdrawn/0066-native-integration.md` |
| `withdrawn` | 0067 | 0 | `withdrawn` | Interactive Verification UI | `docs/rfcs/withdrawn/0067-interactive-verification-ui.md` |
| `withdrawn` | 0072 | 3 | `withdrawn` | Semantic Merge Driver for Structured Context Files | `docs/rfcs/withdrawn/0072-semantic-merge-driver-for-structured-context-files.md` |
| `withdrawn` | 0073 | 0 | `withdrawn` | Semantic Merge Driver for Structured Context Files | `docs/rfcs/withdrawn/0073-semantic-merge-driver-for-structured-context-files.md` |
| `withdrawn` | 0075 | 0 | `withdrawn` | Distinguishing Spec vs. Work RFCs | `docs/rfcs/withdrawn/0075-spec-vs-work-rfcs.md` |
| `withdrawn` | 0077 | 0 | `withdrawn` | Structured IO CLI | `docs/rfcs/withdrawn/0077-structured-io-cli.md` |
| `withdrawn` | 0078 | 0 | `withdrawn` | E2E Holodeck | `docs/rfcs/withdrawn/0078-e2e-holodeck.md` |
| `withdrawn` | 0079 | 0 | `withdrawn` | VS Code Surface Inventory | `docs/rfcs/withdrawn/0079-vs-code-surface-inventory.md` |
| `withdrawn` | 0082 | 0 | `withdrawn` | Exosuit VS Code Tool: Single Entry Point + Signed Capability Tickets + Steering-First Errors | `docs/rfcs/withdrawn/0082-exosuit-vs-code-tool-single-entry-point-signed-capability-tickets-steering-first-errors.md` |
| `withdrawn` | 0083 | 3 | `withdrawn` | Hybrid Tool Architecture for LM Tools | `docs/rfcs/withdrawn/0083-hybrid-tool-architecture-for-lm-tools.md` |
| `withdrawn` | 0084 | 0 | `withdrawn` | Pluggable Upgrade System with Protocol Versioning | `docs/rfcs/withdrawn/0084-pluggable-upgrade-system.md` |
| `withdrawn` | 0090 | 0 | `withdrawn` | Validation-Based Reactive Architecture | `docs/rfcs/withdrawn/0090-validation-based-reactive-architecture.md` |
| `withdrawn` | 0091 | 0 | `withdrawn` | Protected File Watcher with Revert and Notice System | `docs/rfcs/withdrawn/0091-protected-file-watcher.md` |
| `withdrawn` | 0093 | 0 | `withdrawn` | Agent CWD Discipline (Rooted Execution) | `docs/rfcs/withdrawn/0093-agent-cwd-discipline-rooted-execution.md` |
| `withdrawn` | 0098 | 0 | `withdrawn` | E2E Test Infrastructure Fixes | `docs/rfcs/withdrawn/0098-e2e-test-infrastructure-fixes.md` |
| `withdrawn` | 0100 | 0 | `withdrawn` | Comprehensive Testing Strategy & Audit | `docs/rfcs/withdrawn/0100-comprehensive-testing-strategy-audit.md` |
| `withdrawn` | 0101 | 0 | `withdrawn` | Tool JSON Contract for Task Operations | `docs/rfcs/withdrawn/0101-tool-json-contracts.md` |
| `withdrawn` | 0105 | 1 | `withdrawn` | RFC-Centric Workflow Model | `docs/rfcs/withdrawn/0105-rfc-centric-workflow-model.md` |
| `withdrawn` | 0107 | 1 | `withdrawn` | Coherent Workflow Model | `docs/rfcs/withdrawn/0107-coherent-workflow-model.md` |
| `withdrawn` | 0114 | 4 | `withdrawn` | Advanced Phase Transition | `docs/rfcs/withdrawn/0114-advanced-phase-transition.md` |
| `withdrawn` | 0126 | 3 | `withdrawn` | Dashboard V2 | `docs/rfcs/withdrawn/0126-dashboard-v2.md` |
| `withdrawn` | 0129 | 3 | `withdrawn` | Configurable TDD Runners | `docs/rfcs/withdrawn/0129-configurable-tdd-runners.md` |
| `withdrawn` | 0131 | 3 | `withdrawn` | Implementation Plan as Canonical Execution Artifact | `docs/rfcs/withdrawn/0131-implementation-plan-as-canonical-execution-artifact.md` |
| `withdrawn` | 0134 | 3 | `withdrawn` | Structured IO (The Surgeon) | `docs/rfcs/withdrawn/0134-structured-io-the-surgeon.md` |
| `withdrawn` | 0135 | 3 | `withdrawn` | CommandSpec Unification (Narrow Conduit Architecture) | `docs/rfcs/withdrawn/0135-commandspec-unification-narrow-conduit-architecture.md` |
| `withdrawn` | 0138 | 2 | `withdrawn` | The Standard Bootstrap | `docs/rfcs/withdrawn/0138-the-standard-bootstrap.md` |
| `withdrawn` | 0139 | 2 | `withdrawn` | High-Level Workflow Refinement & Automation | `docs/rfcs/withdrawn/0139-high-level-workflow-refinement-automation.md` |
| `withdrawn` | 0140 | 1 | `withdrawn` | Exohook CI Workflow Projection | `docs/rfcs/withdrawn/0140-exohook-ci-workflow-projection.md` |
| `withdrawn` | 0141 | 3 | `withdrawn` | Exohook Streaming Progress Reporting | `docs/rfcs/withdrawn/0141-exohook-streaming-progress-reporting.md` |
| `withdrawn` | 0144 | 0 | `withdrawn` | Agent Ecosystem | `docs/rfcs/withdrawn/0144-agent-ecosystem.md` |
| `withdrawn` | 0145 | 0 | `withdrawn` | Prompt Workflow Integration | `docs/rfcs/withdrawn/0145-prompt-workflow-integration.md` |
| `withdrawn` | 0148 | 2 | `withdrawn` | Implicit Walkthrough via Task Logs | `docs/rfcs/withdrawn/0148-implicit-walkthrough-via-task-logs.md` |
| `withdrawn` | 0150 | 0 | `withdrawn` | Modes and Persona System Unification | `docs/rfcs/withdrawn/0150-modes-and-persona-system-unification.md` |
| `withdrawn` | 0151 | 0 | `withdrawn` | Test Fixture Strategy | `docs/rfcs/withdrawn/0151-test-fixture-strategy.md` |
| `withdrawn` | 0155 | 0 | `withdrawn` | Manual Test RFC | `docs/rfcs/withdrawn/0155-manual-test-rfc.md` |
| `withdrawn` | 0158 | 0 | `withdrawn` | Structured Agent Command Discipline | `docs/rfcs/withdrawn/0158-agentcmd-command-discipline.md` |
| `withdrawn` | 0159 | 0 | `withdrawn` | The `ai` Subcommand Pattern | `docs/rfcs/withdrawn/0159-ai-subcommand-pattern.md` |
| `withdrawn` | 0160 | 0 | `withdrawn` | CLI Steering & VS Code Integration | `docs/rfcs/withdrawn/0160-clisteering-cli-steering.md` |
| `withdrawn` | 0161 | 0 | `withdrawn` | Code-Based MCP Runner | `docs/rfcs/withdrawn/0161-code-based-mcp.md` |
| `withdrawn` | 0163 | 0 | `withdrawn` | CWD Discipline & Wrong Directory Mitigation | `docs/rfcs/withdrawn/0163-cwd-discipline.md` |
| `withdrawn` | 0165 | 0 | `withdrawn` | Editing Tools Improvements | `docs/rfcs/withdrawn/0165-editing-tools-improvements.md` |
| `withdrawn` | 0166 | 0 | `withdrawn` | Enforced UI Verification | `docs/rfcs/withdrawn/0166-enforced-verification.md` |
| `withdrawn` | 0167 | 0 | `withdrawn` | Lightweight Checks / Cognitive Load | `docs/rfcs/withdrawn/0167-lightweight-checks.md` |
| `withdrawn` | 0168 | 0 | `withdrawn` | Lint and Format Lane | `docs/rfcs/withdrawn/0168-lint-and-format-lane.md` |
| `withdrawn` | 0171 | 0 | `withdrawn` | Native Task List Integration | `docs/rfcs/withdrawn/0171-native-task-list.md` |
| `withdrawn` | 0172 | 0 | `withdrawn` | Organic Context Injection (State-Aware Agents) | `docs/rfcs/withdrawn/0172-organic-context-injection.md` |
| `withdrawn` | 0173 | 0 | `withdrawn` | RFC Tooling Completion | `docs/rfcs/withdrawn/0173-rfctooling-rfc-tooling-completion.md` |
| `withdrawn` | 0176 | 0 | `withdrawn` | Sidebar Polish | `docs/rfcs/withdrawn/00176-sidebar-polish.md` |
| `withdrawn` | 0177 | 1 | `withdrawn` | Goals and Tasks: Unified Work Item Model | `docs/rfcs/withdrawn/00177-goals-and-tasks-unified-work-item-model.md` |
| `withdrawn` | 0183 | 0 | `withdrawn` | Machine Channel Reactivity Integration | `docs/rfcs/withdrawn/00183-machine-channel-reactivity-integration.md` |
| `withdrawn` | 0185 | 1 | `withdrawn` | Inbox-Driven Sidebar Actions | `docs/rfcs/withdrawn/00185-inbox-driven-sidebar-actions.md` |
| `withdrawn` | 0201 | 1 | `withdrawn` | ExoSpec Derive Macro: Inline CommandSpec Definition | `docs/rfcs/withdrawn/0201-exospec-derive-macro-inline-commandspec-definition.md` |
| `withdrawn` | 0213 | 0 | `withdrawn` | Test Explorer Integration for Exohook | `docs/rfcs/withdrawn/00213-test-explorer-integration-for-exohook.md` |
| `withdrawn` | 0227 | 2 | `withdrawn` | Computed Phase Details: Unified Derived Root | `docs/rfcs/withdrawn/00227-computed-phase-details-unified-derived-root.md` |
| `withdrawn` | 0229 | 1 | `withdrawn` | Goal Status Authority: plan.toml as Single Source with Derived Signals | `docs/rfcs/withdrawn/00229-goal-status-authority-plan-toml-as-single-source-with-derived-signals.md` |
| `withdrawn` | 10015 | 0 | `withdrawn` | Exohook: Declarative Validation Lanes and Projections | `docs/rfcs/withdrawn/10015-exohook-declarative-validation-lanes-and-projections.md` |
| `withdrawn` | 10016 | 0 | `withdrawn` | Exohook: File List Expansion Patterns | `docs/rfcs/withdrawn/10016-exohook-file-list-expansion-patterns.md` |
| `withdrawn` | 10018 | 0 | `withdrawn` | Verified Text Surgery | `docs/rfcs/withdrawn/10018-verified-text-surgery.md` |
| `withdrawn` | 10022 | 0 | `withdrawn` | Unified File and Directory Rendering | `docs/rfcs/withdrawn/10022-unified-file-and-directory-rendering.md` |
| `withdrawn` | 10026 | 1 | `withdrawn` | Protocol/CLI/Tool Test DSL (Template+Holes) | `docs/rfcs/withdrawn/10026-protocol-cli-tool-test-dsl-template-holes-.md` |
| `withdrawn` | 10028 | 1 | `withdrawn` | Phase State Machine & Projections | `docs/rfcs/withdrawn/10028-phase-state-machine--projections.md` |
| `withdrawn` | 10029 | 0 | `withdrawn` | Surface Refinements | `docs/rfcs/withdrawn/10029-surface-refinements.md` |
| `withdrawn` | 10030 | 0 | `withdrawn` | Native Integration | `docs/rfcs/withdrawn/10030-native-integration.md` |
| `withdrawn` | 10031 | 0 | `withdrawn` | Interactive Verification UI | `docs/rfcs/withdrawn/10031-interactive-verification-ui.md` |
| `withdrawn` | 10036 | 1 | `withdrawn` | Semantic Merge Driver for Structured Context Files | `docs/rfcs/withdrawn/10036-semantic-merge-driver-for-structured-context-files.md` |
| `withdrawn` | 10050 | 0 | `withdrawn` | VS Code Surface Inventory | `docs/rfcs/withdrawn/10050-vs-code-surface-inventory.md` |
| `withdrawn` | 10052 | 1 | `withdrawn` | Exohook: File Expansion Worked Examples | `docs/rfcs/withdrawn/10052-exohook-file-expansion-worked-examples.md` |
| `withdrawn` | 10054 | 0 | `withdrawn` | Protected File Watcher with Revert and Notice System | `docs/rfcs/withdrawn/10054-protected-file-watcher-with-revert-and-notice-system.md` |
| `withdrawn` | 10057 | 0 | `withdrawn` | Studio UX Polish | `docs/rfcs/withdrawn/10057-studio-ux-polish.md` |
| `withdrawn` | 10059 | 0 | `withdrawn` | EARS in Literate Kernel | `docs/rfcs/withdrawn/10059-ears-in-literate-kernel.md` |
| `withdrawn` | 10060 | 0 | `withdrawn` | Studio Visual Language (SVL) | `docs/rfcs/withdrawn/10060-studio-visual-language-svl.md` |
| `withdrawn` | 10064 | 0 | `withdrawn` | Surgical Context | `docs/rfcs/withdrawn/10064-surgical-context.md` |
| `withdrawn` | 10067 | 0 | `withdrawn` | eBPF Auto Instrumentation | `docs/rfcs/withdrawn/10067-ebpf-auto-instrumentation.md` |
| `withdrawn` | 10072 | 0 | `withdrawn` | exo map Command | `docs/rfcs/withdrawn/10072-exo-map-command.md` |
| `withdrawn` | 10073 | 0 | `withdrawn` | Exohook: Declarative Validation Lanes and Projections | `docs/rfcs/withdrawn/10073-exohook-declarative-validation-lanes-and-projections.md` |
| `withdrawn` | 10074 | 0 | `withdrawn` | Exohook: File List Expansion Patterns | `docs/rfcs/withdrawn/10074-exohook-file-list-expansion-patterns.md` |
| `withdrawn` | 10077 | 0 | `withdrawn` | E2E Holodeck | `docs/rfcs/withdrawn/10077-e2e-holodeck.md` |
| `withdrawn` | 10078 | 0 | `withdrawn` | Manual Test RFC | `docs/rfcs/withdrawn/10078-manual-test-rfc.md` |
| `withdrawn` | 10081 | 0 | `withdrawn` | The `ai` Subcommand Pattern | `docs/rfcs/withdrawn/10081-the-ai-subcommand-pattern.md` |
| `withdrawn` | 10082 | 0 | `withdrawn` | Code-Based MCP Runner | `docs/rfcs/withdrawn/10082-code-based-mcp-runner.md` |
| `withdrawn` | 10083 | 0 | `withdrawn` | Exposing RFCs as Copilot Resources | `docs/rfcs/withdrawn/10083-exposing-rfcs-as-copilot-resources.md` |
| `withdrawn` | 10085 | 0 | `withdrawn` | Directory-Based RFC Organization | `docs/rfcs/withdrawn/10085-directory-based-rfc-organization.md` |
| `withdrawn` | 10086 | 0 | `withdrawn` | Editing Tools Improvements | `docs/rfcs/withdrawn/10086-editing-tools-improvements.md` |
| `withdrawn` | 10087 | 0 | `withdrawn` | Enforced UI Verification | `docs/rfcs/withdrawn/10087-enforced-ui-verification.md` |
| `withdrawn` | 10088 | 0 | `withdrawn` | Lightweight Checks / Cognitive Load | `docs/rfcs/withdrawn/10088-lightweight-checks-cognitive-load.md` |
| `withdrawn` | 10089 | 0 | `withdrawn` | Lint and Format Lane | `docs/rfcs/withdrawn/10089-lint-and-format-lane.md` |
| `withdrawn` | 10095 | 0 | `withdrawn` | RFC Tooling Completion | `docs/rfcs/withdrawn/10095-rfc-tooling-completion.md` |
| `withdrawn` | 10104 | 1 | `withdrawn` | North Star User Journey | `docs/rfcs/withdrawn/10104-north-star-user-journey.md` |
| `withdrawn` | 10105 | 1 | `withdrawn` | Phase Lifecycle Vision | `docs/rfcs/withdrawn/10105-phase-lifecycle-vision.md` |
| `withdrawn` | 10107 | 1 | `withdrawn` | RTD Surface Mapping | `docs/rfcs/withdrawn/10107-rtd-surface-mapping.md` |
| `withdrawn` | 10108 | 1 | `withdrawn` | Sidebar Navigation | `docs/rfcs/withdrawn/10108-sidebar-navigation.md` |
| `withdrawn` | 10109 | 1 | `withdrawn` | Rigorous Rust Infrastructure | `docs/rfcs/withdrawn/10109-rigorous-rust-infrastructure.md` |
| `withdrawn` | 10115 | 1 | `withdrawn` | Configurable TDD Runners | `docs/rfcs/withdrawn/10115-configurable-tdd-runners.md` |
| `withdrawn` | 10118 | 1 | `withdrawn` | CLI AST Tool Schema | `docs/rfcs/withdrawn/10118-cli-ast-tool-schema.md` |
| `withdrawn` | 10119 | 1 | `withdrawn` | The Standard Bootstrap | `docs/rfcs/withdrawn/10119-the-standard-bootstrap.md` |
| `withdrawn` | 10120 | 1 | `withdrawn` | Implementation Plan as Canonical Execution Artifact | `docs/rfcs/withdrawn/10120-implementation-plan-as-canonical-execution-artifact.md` |
| `withdrawn` | 10123 | 1 | `withdrawn` | Ordering Syntax | `docs/rfcs/withdrawn/10123-ordering-syntax.md` |
| `withdrawn` | 10125 | 1 | `withdrawn` | Reactive Architecture for VS Code Extensions | `docs/rfcs/withdrawn/10125-reactive-architecture-for-vs-code-extensions.md` |
| `withdrawn` | 10127 | 1 | `withdrawn` | Formal Spec Frontmatter Upgrade | `docs/rfcs/withdrawn/10127-formal-spec-frontmatter-upgrade.md` |
| `withdrawn` | 10128 | 1 | `withdrawn` | Structured IO CLI | `docs/rfcs/withdrawn/10128-structured-io-cli.md` |
| `withdrawn` | 10138 | 3 | `withdrawn` | Externalized Prompts | `docs/rfcs/withdrawn/10138-externalized-prompts.md` |
| `withdrawn` | 10140 | 3 | `withdrawn` | Staged RFC Process | `docs/rfcs/withdrawn/10140-staged-rfc-process.md` |
| `withdrawn` | 10141 | 3 | `withdrawn` | The `exo` CLI | `docs/rfcs/withdrawn/10141-the-exo-cli.md` |
| `withdrawn` | 10142 | 3 | `withdrawn` | RFC Triage Tooling (The Gardener) | `docs/rfcs/withdrawn/10142-rfc-triage-tooling-the-gardener.md` |
| `withdrawn` | 10144 | 3 | `withdrawn` | Reactive Bridge Protocol | `docs/rfcs/withdrawn/10144-reactive-bridge-protocol.md` |
| `withdrawn` | 10145 | 3 | `withdrawn` | Robust Extension Architecture | `docs/rfcs/withdrawn/10145-robust-extension-architecture.md` |
| `withdrawn` | 10148 | 3 | `withdrawn` | RFC Lifecycle Management Tools (Authoring) | `docs/rfcs/withdrawn/10148-rfc-lifecycle-management-tools-authoring.md` |
| `withdrawn` | 10150 | 3 | `withdrawn` | Structured Context Criteria (TOML vs Markdown) | `docs/rfcs/withdrawn/10150-structured-context-criteria-toml-vs-markdown.md` |
| `withdrawn` | 10151 | 3 | `withdrawn` | Refined Staged RFC Process | `docs/rfcs/withdrawn/10151-refined-staged-rfc-process.md` |
| `withdrawn` | 10152 | 3 | `withdrawn` | CLI Command for Axioms | `docs/rfcs/withdrawn/10152-cli-command-for-axioms.md` |
| `withdrawn` | 10156 | 4 | `withdrawn` | Rich Context Editors | `docs/rfcs/withdrawn/10156-rich-context-editors.md` |
| `withdrawn` | 10158 | 4 | `withdrawn` | Structured Context API | `docs/rfcs/withdrawn/10158-structured-context-api.md` |
| `withdrawn` | 10161 | 4 | `withdrawn` | The Plan Object Model | `docs/rfcs/withdrawn/10161-the-plan-object-model.md` |
| `withdrawn` | 10168 | 1 | `withdrawn` | Tree Item Visual Design: FileDecoration-Driven Colored Labels | `docs/rfcs/withdrawn/10168-tree-item-visual-design-filedecoration-driven-colored-labels.md` |
| `withdrawn` | 10198 | 0 | `withdrawn` | Walkthrough Workflow | `docs/rfcs/withdrawn/10198-walkthrough-workflow.md` |
| `withdrawn` | 10199 | 0 | `withdrawn` | Implicit Walkthrough via Task Logs | `docs/rfcs/withdrawn/10199-implicit-walkthrough-via-task-logs-stage1.md` |
| `withdrawn` | 10201 | 0 | `withdrawn` | Phase-Aware Dirty Working Tree Steering | `docs/rfcs/withdrawn/10201-phase-aware-dirty-working-tree-steering.md` |
