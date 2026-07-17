<!-- exo:10140 ulid:01kmzxbczghk3fnzm97zt40gfy -->


# RFC 10140: Staged RFC Process

- **Superseded by**: RFC 0106


- **Status**: Withdrawn
- **Stage**: 3
- **Reason**:

---
title: Migration to Staged RFC Process
feature: Documentation
stage: 0
---

# Migration to Staged RFC Process

## Goal
Transition the project's documentation structure from a loose collection of design documents to a unified **Staged RFC** process. This establishes a clear distinction between "Session Laws" (historical decisions) and "The Code" (current system state).

## The Core Concept: "Session Laws" vs. "The Code"
We distinguish between two types of documentation:
*   **RFCs (`docs/rfcs/`)**: The historical record of *decisions*. These are like "Session Laws". They use persuasive language ("We propose...", "We should...").
*   **Agent Context (`docs/agent-context/`)**: The living manual of the *current system*. This is like "The Code" (US Code). It uses descriptive language ("The system does...", "The architecture is...").

## The Transition Algorithm

### 1. Backfilling History (The "Constitution")
*   Audit the existing codebase.
*   For every existing feature (e.g., Daemon, IPC, CLI), create a **Stage 3 (Recommended)** RFC.
*   These RFCs represent the system as it exists today.

### 2. Migrating Proposals
*   Take loose files from `docs/design/` and `docs/future/`.
*   Convert them into **Stage 0 (Strawman)** or **Stage 1 (Accepted)** RFCs.
*   Renumber them (starting at 0038) to ensure a clean sequence after the backfilled history.

### 3. Metadata Standardization
*   Every markdown file in `docs/rfcs/` must have YAML front matter tracking its `stage`, `feature`, and `title`.

### 4. The "Consolidation" Rule
When a feature moves from **Stage 2 (Available)** to **Stage 3 (Recommended)**, we perform a "Consolidation" step:
*   **Action**: Copy the design content from the RFC into `docs/agent-context/`.
*   **Transformation**: **Strip away the proposal language.**
    *   *RFC:* "We propose adding a `stage` field."
    *   *Context:* "The `stage` field tracks maturity."
*   **Result**: The RFC remains as a historical record, but the Agent Context becomes the source of truth for the AI.

## Implementation Plan

### Phase 1: Scaffolding & Backfill
1.  Create `docs/rfcs/` directory.
2.  Create templates for RFCs.
3.  Backfill Stage 3 RFCs for existing features (using the provided JSON list).

### Phase 2: Migration
1.  Migrate active design docs from `docs/design/` to `docs/rfcs/`.
2.  Archive or delete `docs/design/`.

### Phase 3: Tooling Update
1.  **Update Scripts**:
    *   `bootstrap.sh`: Scaffold `docs/rfcs` instead of `docs/design`. Update references to `axioms.md` and `decisions.md` to use TOML or RFCs.
    *   `scripts/agent/restore-context.sh`: Check `docs/rfcs` instead of `docs/design`.
    *   `scripts/agent/verify-phase.sh`: Verify RFC status instead of design docs.
    *   `scripts/agent/prepare-phase-transition.sh`: Ensure RFCs are consolidated before transition.
2.  **Update Documentation & Prompts**:
    *   `AGENTS.md`: Update "Design Axioms" and "Creation/Promotion" sections to describe the RFC process.
    *   `src/prompts/*.md`: Update all prompts (`bootstrap-axioms`, `fresh-eyes`, `persona-build`, `phase-start`, `phase-status`, `phase-transition`, `prepare-phase`, `rebuild-coherence`) to reference `docs/rfcs` and the distinction between Session Laws and The Code.
    *   `src/templates/AGENTS.md`: Update the template.
3.  **Update VS Code Extension**:
    *   **Sidebar (`DesignPaneProvider`)**:
        *   Point to `docs/rfcs` instead of `docs/design`.
        *   Group RFCs by Stage (0-3) using the YAML frontmatter.
    *   **Commands (`PromoteCommands`)**:
        *   Implement `exosuit.promoteToRFC`: Creates a new Stage 0 RFC from selection/input.
        *   Refactor `promoteToDecision` to append to `decisions.toml` (fixing the `.md` bug) or deprecate in favor of RFCs.
    *   **Icons (`IconUtils`)**: Update icons for RFC files if needed.
    *   **Panel (`ExosuitPanel`, `ContentProvider`)**: Ensure compatibility with new paths.
    *   **Context Dashboard**:
        *   Add an "Active RFCs" section showing Stage 0-2 RFCs.

### Phase 4: Legacy Cleanup
1.  **Decisions**:
    *   Complete the migration from `decisions.md` to `decisions.toml`.
    *   Update all scripts and prompts to reference `decisions.toml`.
    *   Archive `decisions.md`.
2.  **Axioms**:
    *   Complete the migration from `axioms.md` to scoped axioms TOML files (`docs/agent-context/axioms.workflow.toml`, `docs/agent-context/axioms.system.toml`, `docs/design/axioms.design.toml`).
    *   Update all scripts and prompts to reference the scoped axioms files (or `axioms.*.toml`).
    *   Archive `axioms.md`.
3.  **Plan Outline**:
    *   Clarify the relationship between `plan-outline.md` and `plan.toml`. If `plan.toml` is the source of truth, ensure `plan-outline.md` is generated or deprecated.
4.  **Remove Vestiges**:
    *   **Delete `docs/design/`**: Once all active docs are migrated to RFCs and historical docs are archived or backfilled, delete the `docs/design/` directory entirely.
    *   **Delete `docs/future/`**: Migrate any remaining ideas to Stage 0 RFCs or `ideas.md`, then delete the directory.
    *   **Code Audit**: Run a final grep search for `docs/design`, `docs/future`, `decisions.md`, and `axioms.md` to ensure no code or prompts reference the old paths.

### Phase 5: Verification
1.  Verify that the agent can correctly identify the current state of the project.
2.  Verify that new phases follow the RFC process.

## Appendix: Proposed Backfill List (Exosuit)
We need to generate Stage 3 RFCs for these existing features.

```json
[
  { "number": "0001", "title": "Project Name: Exosuit", "stage": 3, "feature": "Architecture" },
  { "number": "0002", "title": "Monorepo Structure: pnpm workspaces", "stage": 3, "feature": "Architecture" },
  { "number": "0003", "title": "Agent Context: Single Source of Truth", "stage": 3, "feature": "Architecture" },
  { "number": "0004", "title": "Phase Lifecycle: Epochs & Phases", "stage": 3, "feature": "Workflow" },
  { "number": "0005", "title": "Chat Participant: @exosuit", "stage": 3, "feature": "Agent" },
  { "number": "0006", "title": "Sidebar: Tree View Navigation", "stage": 3, "feature": "UI" },
  { "number": "0007", "title": "Rich Context Editors: Custom Editors", "stage": 3, "feature": "UI" },
  { "number": "0008", "title": "Core Library: exosuit-core", "stage": 3, "feature": "Architecture" },
  { "number": "0009", "title": "RTD: Rich Text Document Protocol", "stage": 3, "feature": "RTD" },
  { "number": "0010", "title": "Streaming Parser: Tail Buffering", "stage": 3, "feature": "RTD" },
  { "number": "0011", "title": "Context Persistence: TOML & Markdown", "stage": 3, "feature": "Architecture" },
  { "number": "0012", "title": "Design Axioms: Living Documentation", "stage": 3, "feature": "Documentation" },
  { "number": "0013", "title": "Personas & Modes", "stage": 3, "feature": "Workflow" },
  { "number": "0014", "title": "Coherence Checkpoints", "stage": 3, "feature": "Workflow" },
  { "number": "0015", "title": "Provenance Labeling", "stage": 3, "feature": "Workflow" },
  { "number": "0016", "title": "Dynamic Planning: Plan Object Model", "stage": 3, "feature": "Workflow" },
  { "number": "0017", "title": "Literate Kernel: XML Tags", "stage": 3, "feature": "Agent" },
  { "number": "0018", "title": "Context Persistence: Hidden Comments", "stage": 3, "feature": "Agent" },
  { "number": "0019", "title": "Agent Loop: Recursive State Machine", "stage": 3, "feature": "Agent" },
  { "number": "0020", "title": "Tool Registry", "stage": 3, "feature": "Agent" },
  { "number": "0021", "title": "Smart Task Verification", "stage": 3, "feature": "Agent" },
  { "number": "0022", "title": "Context Dashboard", "stage": 3, "feature": "UI" },
  { "number": "0023", "title": "Walkthrough Editor", "stage": 3, "feature": "UI" },
  { "number": "0024", "title": "AI Activity Visualization", "stage": 3, "feature": "UI" },
  { "number": "0025", "title": "Rigorous CommonMark + LLM - HTML Philosophy", "stage": 3, "feature": "RTD" },
  { "number": "0026", "title": "State Machine Parser", "stage": 3, "feature": "RTD" },
  { "number": "0027", "title": "Fail-Safe LLM Artifact Handling", "stage": 3, "feature": "RTD" },
  { "number": "0028", "title": "Strict Link Scheme Whitelist", "stage": 3, "feature": "RTD" },
  { "number": "0029", "title": "Unified Project State Schema", "stage": 3, "feature": "Architecture" },
  { "number": "0030", "title": "Feedback System", "stage": 1, "feature": "Workflow" },
  { "number": "0031", "title": "Pane Organization Strategy", "stage": 0, "feature": "UI" },
  { "number": "0032", "title": "Native Ecosystem Integration", "stage": 0, "feature": "Integration" },
  { "number": "0033", "title": "Ideas & Triage System", "stage": 0, "feature": "Workflow" },
  { "number": "0034", "title": "Documentation Site & Playground", "stage": 0, "feature": "Documentation" }
]
```
