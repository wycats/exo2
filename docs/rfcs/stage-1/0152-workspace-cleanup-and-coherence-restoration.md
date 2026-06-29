<!-- exo:152 ulid:01kg5kp2jj6btm7zvhqnt612vk -->

# RFC 152: Workspace Cleanup and Coherence Restoration


# RFC 0152: Workspace Cleanup and Coherence Restoration

> **Note**: This RFC was originally RFC 0106 but was withdrawn due to an RFC numbering collision. It has been restored as RFC 0152 to continue the cleanup work.

## Summary

This RFC proposes a systematic cleanup of accreted files, dormant concepts, and orphaned artifacts in the exosuit workspace. The goal is to restore coherence between the documented workflow model and the actual file structure, making the system fit-for-purpose for shipping.

## Motivation

### The Problem

The workspace has accumulated significant cruft over 19+ epochs of development:

1. **~100 RFCs in stages 0-2** that don't serve their purpose as implementation guides
2. **Dormant concept files** that are well-defined but not wired into workflows
3. **Orphaned directories** that don't belong to any system (e.g., `/stage-1/` at repo root)
4. **Accreted files** in `docs/agent-context/` from earlier workflow iterations
5. **Broken infrastructure** (e.g., `feedback.toml` with property name mismatches)

### The Insight

From the shipping documents analysis:

> "The concepts aren't lost. They're dormant, waiting to be awakened."

But awakening them as-is would add noise. The opportunity is to **awaken the spirit while simplifying the machinery** — reconnect what powers workflows, archive what doesn't.

## Proposal

### Phase 1: RFC Triage ✅ COMPLETED

Review all RFCs systematically with the following disposition rules:

| Current Stage | If Implemented | If Partially Done     | If Aspirational | If Superseded |
| ------------- | -------------- | --------------------- | --------------- | ------------- |
| Stage 3       | → Stage 4      | → Stage 4 (note gaps) | N/A             | → Withdraw    |
| Stage 2       | → Stage 4      | → Stage 3/4 (edit)    | → Ideas backlog | → Withdraw    |
| Stage 1       | → Stage 3/4    | → Stage 2 (edit)      | → Ideas backlog | → Withdraw    |
| Stage 0       | → Stage 3/4    | → Stage 1/2           | → Ideas backlog | → Withdraw    |

**Stage 4 requirement**: Any RFC promoted to Stage 4 must have corresponding Manual documentation (or create it as part of promotion).

### Phase 2: Concept File Cleanup ✅ COMPLETED

| File                   | Purpose                 | Disposition                                    |
| ---------------------- | ----------------------- | ---------------------------------------------- |
| `axioms.legacy.toml`   | Old axioms format       | Archive (content preserved in current axioms)  |
| `axioms.system.toml`   | System axioms           | Keep, wire into Plan/Decide workflows          |
| `axioms.workflow.toml` | Workflow axioms         | Keep, wire into Plan/Decide workflows          |
| `council.toml`         | Experiment (Dream Team) | Archive (not workflow-critical)                |
| `decisions.toml`       | Decision log            | Review: merge useful into RFCs/Manual, archive |
| `feedback.toml`        | Broken intent capture   | Remove (superseded by inbox.toml)              |
| `modes.toml`           | Agent work modes        | Keep, wire into steering (mode-aware behavior) |
| `prompts.toml`         | Prompt registry         | Review: still needed?                          |
| `changelog.md`         | Historical record       | Keep (reference material)                      |

### Phase 3: Orphaned Directory Cleanup ✅ COMPLETED

| Path                                            | What It Is          | Disposition                                           |
| ----------------------------------------------- | ------------------- | ----------------------------------------------------- |
| `/stage-1/`                                     | Stray RFC (10012)   | Move to `docs/rfcs/stage-1/`, delete directory        |
| `/test.json`, `/test.toml`                      | Test artifacts      | Evaluate: needed for tests? Otherwise delete          |
| `docs/agent-context/future/`                    | Future phase plans  | Evaluate each file for ideas extraction, then archive |
| `docs/agent-context/phase-25-ideas-and-triage/` | Old phase artifacts | Archive or delete                                     |
| `docs/agent-context/specs/`                     | Orphaned specs      | Move useful content to Manual, archive rest           |
| `docs/agent-context/templates/`                 | Task templates      | Evaluate if still used                                |
| `docs/agent-context/research/`                  | Research notes      | Keep (valuable context), but audit for stale content  |

### Phase 4: Future Phase Planning Infrastructure ✅ COMPLETED

**Outcome**: Scope reduced after analysis.

- **No `epoch-backlog.toml`** - Use existing `ideas.toml` with tags for epoch-level ideas
- **No `next-phase.toml`** - Deferred as future feature (captured in ideas.toml)
- **Cleaned up `future/`** - Deleted 6 obsolete markdown files, extracted valuable ideas to backlog

### Phase 5: Walkthrough as View over Implementation Plan 🔄 IN PROGRESS

**Original concept**: Walkthrough was a separate `walkthrough.toml` file for narrative tracking.

**Problem identified**: This created redundancy with task logs in `implementation-plan.toml` and was deprecated by the upgrade gate system.

**Refined approach** (from first-principles audit):

Walkthrough should be a **view** over `implementation-plan.toml`, not a separate artifact:

1. **`exo walkthrough view`** (pure) - Render a human-readable narrative from implementation-plan task logs and verification data
2. **`exo walkthrough add`** (write-through) - Append a log entry via `exo impl add-task-log` with convenient targeting
3. **`exo phase status`** - Include walkthrough narrative section by default for review/transition

**Cleanup required**:

- Remove `walkthrough.toml` scaffolding from templates and bootstrap
- Update VS Code walkthrough UI to render from implementation-plan
- Remove deprecated walkthrough commands that target standalone file
- Update LM tool surface to use new walkthrough view

### Phase 6: RFC Renumbering

The RFC numbering system has become inconsistent:

- **0001-0060**: Early RFCs, 4-digit with leading zeros
- **10001-10112**: Later RFCs, 5-digit starting with "10" (arbitrary)
- **9998-9999**: Test/special RFCs
- Duplicates exist, gaps are random

**Proposal**: Renumber all RFCs to 4-digit sequential (0001, 0002, 0003...).

**Why this is safe**: RFCs have ULIDs as stable identifiers. References in code use ULIDs, so renumbering the human-readable ID won't break links.

**Process**:

1. Build mapping: old number → new number (sorted by creation date)
2. Rename files with new numbers
3. Update frontmatter in each RFC
4. Update any hardcoded numeric references (search for old patterns)
5. Add tooling enforcement: `exo rfc create` assigns next sequential number

**Result**: RFCs numbered 0001-00XX contiguously, easy to reference ("RFC 42").

## Implementation

### Approach: Subagent-Driven Review

Each RFC category will be reviewed by a subagent that:

1. Reads the RFC content
2. Searches codebase for implementation evidence
3. Reports findings: implemented/partial/not-implemented/superseded
4. Recommends disposition

Human makes final call on each disposition.

### Progress Tracking

| Phase                               | Status         | Notes                       |
| ----------------------------------- | -------------- | --------------------------- |
| Phase 1: RFC Triage                 | ✅ Completed   | See `rfc-triage-summary.md` |
| Phase 2: Concept File Cleanup       | ✅ Completed   | Merged PRs #57-59           |
| Phase 3: Orphaned Directory Cleanup | ✅ Completed   | `docs/design/` consolidated |
| Phase 4: Future Planning Infra      | 🔄 In Progress | Current phase               |
| Phase 5: Walkthrough Restoration    | ⏳ Pending     |                             |
| Phase 6: RFC Renumbering            | ⏳ Pending     |                             |

### Success Criteria

After cleanup:

1. **RFCs are actionable**: Every RFC in stages 0-2 has clear path to implementation or is withdrawn
2. **RFC numbers are sequential**: 4-digit, contiguous, easy to reference
3. **Manual is authoritative**: Stage 4 concepts documented in Manual
4. **No orphan files**: Every file in `docs/agent-context/` has clear purpose
5. **Future planning works**: Next phase can be sketched before current phase ends
6. **Walkthrough restored**: Human-readable narrative artifact for phase review

## Risks

1. **Information loss**: Aggressive cleanup might delete useful context
   - Mitigation: Archive rather than delete, extract ideas before removal

2. **Scope creep**: "While we're here..." syndrome
   - Mitigation: Strict phase boundaries, file cleanup ideas for later

3. **Bookkeeping overhead**: RFC triage takes time
   - Mitigation: Subagent-driven review, human only makes disposition calls

## Alternatives Considered

1. **Do nothing**: Let cruft accumulate
   - Rejected: Already impacting workflow coherence

2. **Declare bankruptcy**: Delete everything, start fresh
   - Rejected: Would lose valuable context and implemented features

3. **Incremental cleanup**: Clean as we go
   - Rejected: Has been the approach, cruft keeps accumulating

## References

- RFC 0050: Async Intent Channel (inbox system)
- RFC 0064: Phase State Machine
- RFC 0149: Axiom System Integration
- RFC 0150: Modes and Persona System Unification

