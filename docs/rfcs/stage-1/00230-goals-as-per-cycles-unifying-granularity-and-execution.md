<!-- exo:230 ulid:01kmzxey0np68avt38bj2q74xb -->

# RFC 230: Goals as PER Cycles: Unifying Granularity and Execution


# RFC 00230: Goals as PER Cycles: Unifying Granularity and Execution

## Summary

Goals should be calibrated to the granularity of a single PER (Prepare→Execute→Review) cycle. This provides a concrete, testable heuristic for goal sizing and naturally integrates the coordinator agent as the orchestrator of goal-level execution.

## Motivation

### The Granularity Problem

"What's the right size for a goal?" is currently a vibes question. Goals range from trivial ("fix typo") to sprawling ("implement feature X"). Without calibration, we get:

- **Too-big goals**: Become mini-projects that lose focus, accumulate scope creep, and resist completion
- **Too-small goals**: Create ceremony overhead where Prepare and Review are pure formality
- **Inconsistent goals**: Make phase planning unreliable — some phases have 3 goals, others have 30

### The PER Connection

The PER protocol already defines a natural unit of work:

1. **Prepare**: Audit the plan against reality, verify assumptions, identify blockers
2. **Execute**: Implement the work, write code and tests
3. **Review**: Verify output meets expectations, capture learnings

This is exactly what a well-sized goal should encompass. The insight: **a goal IS a PER cycle**.

### Stale Goal Problem

Goals are often created during the Review step of a previous phase — "here's what we should do next." By the time we reach that phase, context has shifted:

- Other work happened
- User priorities evolved
- The goal's framing no longer fits

Without a refresh mechanism, agents charge ahead on stale assumptions.

## Detailed Design

### The Goal = PER Equivalence

| PER Phase | Goal Lifecycle | What Happens |
|-----------|---------------|--------------|
| Prepare | Goal has tasks, not yet started | Audit tasks against codebase, verify assumptions |
| Execute | Goal in-progress | Implement tasks, write code + tests |
| Review | Ready-for-logging | Verify output, mark goal complete |

### Granularity Heuristics

**Too big** — Split the goal if:
- Prepare reveals multiple independent implementation passes needed
- The goal requires coordination across unrelated subsystems
- Execute would produce multiple reviewable artifacts that could ship independently

**Too small** — Merge into a larger goal if:
- Prepare and Review are pure ceremony (nothing to audit, nothing to verify)
- The "goal" is really a single task within a larger coherent unit
- Completion doesn't represent meaningful progress visible to the user

**Just right** — A goal is well-sized when:
- Prepare can audit it as one coherent unit
- Execute produces one reviewable artifact (PR, document, etc.)
- Review can verify the whole thing in one pass
- Completion represents a meaningful checkpoint

### Stale Goal Review

When a phase starts (or resumes) with pre-planned goals, steering should nudge a **refresh dialogue** before tasking:

```
Phase starts with pre-planned goals
         │
         ▼
  ┌──────────────┐
  │ ORIENT: Are   │  ← Steering nudge: "Review goals before tasking"
  │ goals still   │
  │ fresh?        │
  └──────┬───────┘
         │
    ┌────┴────┐
    │ Refresh  │  Human-AI conversation: reword, reorder, abandon
    │ dialogue │
    └────┬────┘
         │
         ▼
  ┌──────────────┐
  │ ACT: Add      │  ← Now fill in tasks on validated goals
  │ tasks to      │
  └──────────────┘
```

The refresh dialogue should surface **what changed since goal creation**:
- Commits since goal was added
- Completed phases
- New RFCs or ideas
- Time elapsed

This grounds the review in concrete drift, not abstract "are these still good?"

### Coordinator Agent Integration

The coordinator agent becomes the natural orchestrator of goal-level PER cycles:

1. **Pick next goal** — Select the top incomplete goal in the phase
2. **Run PER cycle** — Prepare→Execute→Review on that goal
3. **Mark complete** — `exo goal complete` with log
4. **Repeat** — Move to next goal

Goal ordering in the phase becomes the execution schedule. The coordinator works through goals top-to-bottom, each as a PER cycle.

### Research Goals

Some goals are research/recon rather than implementation. PER still applies:

- **Prepare**: What are we looking for? What questions need answers?
- **Execute**: Investigate, read code, fetch docs, explore
- **Review**: Synthesize findings into actionable output

The difference is that Execute produces a report rather than code. The structure remains.

### PR as the Review Artifact

The PER cycle maps naturally onto GitHub pull requests:

| PER Phase | Goal State | GitHub Artifact |
|-----------|------------|-----------------|
| Prepare | Has tasks, not started | Branch created |
| Execute | In-progress | Commits accumulating |
| Review | Ready-for-logging | PR opened, ready for review |
| Complete | Completed | PR merged |

**Key insight**: Opening a PR is the signal that Execute is done and Review should begin. The PR *is* the Review artifact — it captures what was done and invites verification. Merging closes the loop.

This clarifies PR timing:
- **During Execute**: Commits are WIP. A draft PR is optional (for CI visibility).
- **At ready-for-logging**: PR is opened or marked ready. This is the transition from Execute → Review.
- **Goal completion = PR merge**: The natural checkpoint. `exo goal complete` should follow merge, not precede it.

#### Phase-Level PR Metadata

Since a phase is a sequence of goal-level PER cycles, the phase itself has a PR lifecycle. Phase details should track:

- `pr_url` — The PR associated with this phase's work
- `pr_status` — draft / open / approved / merged
- `ci_status` — passing / failing / pending

This metadata could live directly on the phase in `plan.toml`, or as a derived root (since PR status is external and changes independently). The derived root approach is likely better — it avoids stale data in plan.toml and lets the system query GitHub for current status.

#### READY_TO_SHIP: The Phase-Level Review State

When all goals in a phase are complete, the phase enters **READY_TO_SHIP** mode. This is distinct from both EXECUTING (work in progress) and DISCOVERY (no active phase):

```
PLANNING → EXECUTING → READY_TO_SHIP → SHIPPED
                            │
                            ▼
                     ┌──────────────┐
                     │ Review Flow   │
                     │ • Open PR     │
                     │ • CI passes   │
                     │ • Get review  │
                     │ • Merge       │
                     └──────┬───────┘
                            │
                            ▼
                      phase finish
```

READY_TO_SHIP parallels `ready-for-logging` at the goal level:
- **Goal**: All tasks done → ready-for-logging → complete (with log)
- **Phase**: All goals done → READY_TO_SHIP → finished (PR merged)

The current `progress_heuristic` conflates "phase done, not yet finished" with "no active phase." These are different states requiring different steering:
- **READY_TO_SHIP**: "Open a PR, get review, merge, then finish the phase"
- **DISCOVERY**: "Plan what to work on next"

### Implications

1. **Goals require tasks** — A PER cycle with nothing to Execute is degenerate. Goals without tasks should be flagged by steering.

2. **Task granularity follows** — Tasks are the work items within Execute. They should be completable in a single focused session.

3. **Ready-for-logging gains meaning** — This derived state (all tasks done, no completion log) signals that Execute is done and Review should begin.

4. **Phase = sequence of PER cycles** — A phase is complete when all its goals have completed their PER cycles.

5. **PR is the phase-level review artifact** — A phase isn't truly finished until its PR is merged. READY_TO_SHIP is the mode that bridges "code done" and "phase finished."

## Implementation Plan

### Phase 1: READY_TO_SHIP Mode

Add READY_TO_SHIP to the progress heuristic:
- Detect: active phase exists, 0 pending goals, phase not yet finished
- Steering nudge: "Open PR, get review, merge, then finish phase"
- Distinct from DISCOVERY (no active phase) and EXECUTING (goals pending)

### Phase 2: Steering Nudges for Stale Goals

Add steering logic to detect goals that may be stale:
- Goal created > N days ago without tasks
- Significant commits since goal creation
- Phase was inactive for extended period

Nudge: "Review goals with user before adding tasks"

### Phase 3: PR Metadata

Add phase-level PR tracking:
- Derived root for PR status (avoids stale data in plan.toml)
- Surface PR URL, status, CI results in phase details
- Steering integration: nudge PR creation at READY_TO_SHIP

### Phase 4: Coordinator PER Integration

Update coordinator agent prompt to:
- Treat each goal as a PER cycle
- Run Prepare before adding tasks to a goal
- Run Review before marking goal complete
- Surface granularity concerns during Prepare

### Phase 5: Granularity Tooling

Add optional `exo goal split` and `exo goal merge` commands for when Prepare reveals sizing issues.

### Phase 6: Drift Surfacing

Enhance steering to show concrete drift:
- `exo goal drift <id>` — Show what changed since goal creation
- Include drift summary in steering output for stale goals

## Alternatives Considered

### Goals as Arbitrary Containers

Status quo. Goals have no defined granularity, leading to inconsistent sizing and unreliable planning.

**Rejected**: The lack of calibration makes phase planning a guessing game.

### Goals = PRs

Tie goals directly to pull requests — one goal, one PR.

**Partially accepted**: This is often true in practice, but some goals produce documents or research rather than PRs. PER is more general.

### Automatic Goal Splitting

Have the system automatically split goals that are too big.

**Rejected**: Splitting requires judgment about domain boundaries. Better to surface the issue and let human decide.

## Prior Art

- **PER Protocol** (copilot-instructions.md) — The Prepare→Execute→Review loop this RFC builds on
- **SOAR Loop** (RFC 00224) — The tactical workflow model; PER operates within the Act phase of SOAR
- **Goal Status Authority** (RFC 00229) — Defines goal lifecycle states that this RFC gives semantic meaning

## Unresolved Questions

1. **Staleness threshold** — How old is "stale"? Days? Commits? Should it be configurable?

2. **Recon goals** — Should research goals have a different lifecycle, or is PER sufficient?

3. **Nested PER** — If a goal's Execute phase is complex, can it have sub-PER cycles? Or should that trigger a split?

4. **Tooling weight** — How much tooling (split, merge, drift) is worth building vs. relying on human judgment?

5. **PR metadata storage** — Derived root (live query) vs plan.toml field (snapshot)? Derived root avoids staleness but requires GitHub API access.

6. **Multi-PR phases** — Some phases may produce multiple PRs (e.g., one per goal). Should PR metadata be on goals, phases, or both?

## Future Possibilities

- **PER metrics** — Track time spent in each PER phase per goal for workflow optimization
- **Goal templates** — Pre-defined goal structures for common patterns (feature, bugfix, refactor, research)
- **Automatic drift detection** — Proactively surface when goals may need refresh, not just at phase start
- **Chore phases** — Lightweight interstitial phases for system-detected housekeeping (open PRs, stale branches, unreviewed epochs). See RFC 00231.

