<!-- exo:10142 ulid:01kmzxefdrm66e6hmdq3x95kcj -->


# RFC 10142: RFC Triage Tooling (The Gardener)

## Summary

This RFC proposes a set of CLI tools (`exo rfc triage`) and metadata standards to manage the lifecycle of Stage 0 (Strawman) RFCs. The goal is to prevent "Idea Rot" by enforcing a regular "Up or Out" decision process, ensuring that the `stage-0` directory remains a focused list of active proposals rather than a graveyard of abandoned thoughts.

## Motivation

- **Accumulation**: It is easy to create Stage 0 RFCs (`exo rfc new`), but hard to remember to close them.
- **Cognitive Load**: A cluttered `stage-0` folder makes it difficult to see what is actually being worked on.
- **Lack of Signal**: It is unclear which Strawmen are "Active/Incubating" and which are "Stale/Abandoned".

## Detailed Design

### 1. Metadata Extensions

We will add optional fields to the RFC frontmatter for Stage 0:

```yaml
last-reviewed: 2025-12-01
status: incubating # active | incubating | stale | deferred
owner: agent # or user
```

### 2. The `exo rfc triage` Command

This command acts as a "Gardener" for the RFC repository.

**Analysis Mode (`exo rfc triage --check`)**:
Scans `docs/rfcs/stage-0/` and reports:

- **Stale**: Modified > 14 days ago.
- **Orphaned**: Not linked to any `plan.toml` or other RFCs.
- **Rotting**: Marked `deferred` > 30 days ago.

**Interactive Mode (`exo rfc triage`)**:
Iterates through "Needs Attention" RFCs and prompts the user/agent for an action:

1.  **Promote**: Move to Stage 1 (starts the formal process).
2.  **Refresh**: Update `last-reviewed` (keep it in Stage 0).
3.  **Consolidate**: Merge into another RFC (and delete this one).
4.  **Withdraw**: Move to `docs/rfcs/withdrawn/`.
5.  **Defer**: Move to `docs/rfcs/deferred/` (or keep in Stage 0 with `status: deferred`).

### 3. The Chat-First Workflow (Collaborative Triage)

While the CLI provides the mechanism, the **Agent** provides the interface. We want to avoid forcing the user to drop into a terminal for high-level decision making.

**The Workflow:**

1.  **Initiation**: User says "Let's triage the RFCs" or "What's the status of our ideas?".
2.  **Analysis**: The Agent runs `exo rfc triage --check --json`.
3.  **Presentation**: The Agent presents a summarized "Triage Report" in the chat.
    > "I found 3 stale RFCs and 2 orphans.
    >
    > 1. `cwd-discipline` (Stale): Seems superseded by `exo-shell`. Consolidate?
    > 2. `random-idea` (Orphan): No updates in 30 days. Withdraw?"
4.  **Negotiation**: The User replies in natural language.
    > "Yes, consolidate cwd into exo-shell. Keep random-idea for now."
5.  **Execution**: The Agent calls the underlying CLI tools (`exo rfc consolidate`, `exo rfc update`) to execute the decisions.

This pattern treats the CLI as a **Headless Backend** for the Agent's **Conversational UI**.

### 4. The "Consolidation" Workflow

Often, multiple Strawmen (e.g., "Fix CWD", "Fix Shell Tools") should be merged into one Proposal (e.g., "The Exo-Shell Pattern").

The tool should support this:
`exo rfc consolidate --into target-rfc.md source-rfc-1.md source-rfc-2.md`

- Appends the content of sources to the target as "Prior Art" or "Merged Ideas".
- Deletes the source files.
- Updates the target's `relations` to preserve history.

### 5. Phase Alignment (Planning Triage)

Beyond cleaning up Strawmen, we need to ensure that active RFCs are properly scheduled.

**The Problem**:

- RFCs are promoted to Stage 1 but never added to `plan.toml`.
- Phases are defined in `plan.toml` but lack the RFCs that define them.

**The Solution (`exo rfc triage --planning`)**:
This mode analyzes the relationship between `docs/rfcs/` and `docs/agent-context/plan.toml`.

- **Unplanned RFCs**: Lists Stage 1+ RFCs that are not referenced in any Phase.
  - _Action_: "Add to Backlog" or "Schedule for Phase X".
- **Empty Phases**: Lists Phases that have no associated RFCs.
  - _Action_: "Create RFC" or "Link existing RFC".
- **Drift Detection**: Lists RFCs that are `Stage 3` (Candidate) but whose Phase is still `Planned` (not `Active` or `Completed`).

**Visualization**:
The output should be a structured JSON object that the Agent (or Sidebar) can render as a "Planning Board":

```json
{
  "unplanned": [{ "id": "0035", "title": "Process Refinement", "stage": 1 }],
  "phases": [{ "id": "phase-28", "rfcs": ["0029"], "missing_rfcs": [] }]
}
```

### 6. Integration with Phase Lifecycle

We can enforce hygiene by adding a check to `exo phase start`:

- "Warning: You have 5 stale Stage 0 RFCs. Please run `exo rfc triage` before starting a new major phase."

## Drawbacks

- **Bureaucracy**: Triage takes time.
- **Automation Bias**: We might auto-close good ideas just because they are old. (Mitigation: "Withdrawn" is not "Deleted").

## Alternatives

- **Auto-Archive**: A bot that just moves old files to `archive/` automatically. (Too aggressive).
- **Ignore It**: Let the folder grow. (Current status quo, leads to mess).
