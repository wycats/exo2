<!-- exo:239 ulid:01ky5qe7mdgbb0nwt6a2knkze3 -->

# RFC 00239: Pipeline Visual Language: Terse RFC Status Notation for Sidebar and Tooling

**Feature**: sidebar
- **Superseded by**: RFC 10172

## Summary

Define a terse, composable visual language for representing RFC pipeline state in the VS Code sidebar and CLI tooling. Like git's `M`/`A`/`D`/`R` file status indicators, this notation encodes stage, movement, and role in a compact format that can be used anywhere RFCs are referenced.

## Motivation

The sidebar currently shows RFCs as flat cross-references: "RFC 00238 • Stage 1". This tells you what exists but not what's happening. The user can't glance at the sidebar and see:

- Which RFCs are being actively advanced vs. merely referenced
- What stage transitions are expected this phase or epoch
- Whether an RFC was created during the current work or existed before
- What's blocked and what's flowing

Git solved this for files: `M src/main.ts` instantly communicates status. We need the same density for the RFC pipeline.

### Relationship to RFC 00238

RFC 00238 (Pipeline-Aware Self-Model) establishes that the RFC pipeline should be the central organizing principle and that VS Code affordances should serve as shared perception channels. This RFC provides the concrete visual notation that makes that perception possible.

## Detailed Design

### Core Vocabulary

Three orthogonal dimensions compose to describe any RFC's pipeline state:

#### Stage Badges

Always present. Encodes current position in the pipeline:

```
○   S0  Idea         (empty — not yet formed)
◐   S1  Proposal     (half — proposed, not detailed)
◑   S2  Draft        (more than half — detailed spec)
●   S3  Candidate    (full — implemented)
★   S4  Canon        (star — shipped law)
✕   W   Withdrawn    (crossed out)
```

#### Movement Arrows

When an RFC is expected to change stage, show the trajectory:

```
S0→1     One-step promotion (this phase)
S0→1→3   Full trajectory (epoch view, multi-phase)
S0→1✓→3  Milestone reached (S1 achieved, heading to S3)
S0       Static — referenced but not advancing
+S0→1    Born this phase/epoch and promoted
```

The `→` is the "chugging through" signal. No arrow = context, not work.
The `✓` marks a completed milestone in a multi-step trajectory.
The `+` prefix means "created during this phase/epoch" (analogous to git's `A`).

#### Role Prefix

Why this RFC appears in context:

```
▸  Driving   — this phase/epoch is advancing this RFC
·  Related   — referenced, informs the work
◇  Blocked   — needs something before it can advance
```

### Composed Notation

A single RFC line in any context:

```
▸ 00238 Pipeline-Aware Self-Model          S0→1
·  00224 SOAR Loop                          S1
▸ 00236 Resource Projections               +S0→1
◇ 00188 Derived Roots                      S3→4  ⧗ needs manual update
```

### Phase Touch Indicators (Epoch View)

In the epoch pipeline view, show which phases interact with each RFC:

```
P1 ✓  (done advancing in this phase)
P2 ○  (this phase advances the RFC)
P3 ·  (this phase references but doesn't advance)
```

### Sidebar Application

#### Phase View — "What's this phase doing to the pipeline?"

```
▼ Phase: Vision Capture + RFC Audit           ✓ 1/4 • 4/10
│
├── RFCs
│   ▸ 00238 Pipeline-Aware Self-Model         S0→1
│   ▸ 00239 Pipeline Visual Language          +S0→1
│   ·  00224 SOAR Loop                         S1
│   ·  00231 Chore Phases                      S0  subsumed
│   ·  00236 Resource Projections              S0  Phase 3
│
├── ▼ Goal: Audit existing RFCs               [2/3]
│   ├── ✓ Generate RFC inventory
│   ├── ✓ Classify RFCs
│   └── ○ Identify dependencies
│
├── ▼ Goal: Define work types                 [0/2]
│   ...
```

The RFCs section is compact — a few lines of terse notation answering "what pipeline work am I in the middle of?" The `▸` on driving RFCs and movement arrows give at-a-glance pipeline awareness.

Compare to a later phase in the same epoch:

```
▼ Phase: Pipeline-Aware Steering              ○ 0/5 • 0/5
│
├── RFCs
│   ▸ 00238 Pipeline-Aware Self-Model         S1→3
│   ·  00236 Resource Projections              S0
│
├── ▼ Goal: Steering knows phase→RFC link     [0/2]
│   ...
```

Same RFC, different phase — now `S1→3`. You can feel it moving.

#### Epoch View — "How's the pipeline chugging?"

```
▼ 📦 Pipeline Awareness                       [1/4 phases]
│
├── Pipeline
│   ▸ 00238 Pipeline-Aware Self-Model         S0→1✓→3
│   │  P1 ✓  P2 ○  P3 ·  P4 ○
│   │
│   ▸ 00239 Pipeline Visual Language          +S0→1→3
│   │  P1 ✓  P2 ·  P3 ○
│   │
│   ▸ 00236 Resource Projections              S0→1
│   │  P1 ·  P2 ·  P3 ○
│   │
│   └── Chore Lane (empty)
│
├── Phases
│   ├── ✓ P1: Vision Capture + RFC Audit
│   ├── ○ P2: Pipeline-Aware Steering          ← next
│   ├── ○ P3: Shared Perception Channel
│   └── ○ P4: Chore Lane + Review Flow
```

After Phase 2 completes:

```
├── Pipeline
│   ▸ 00238 Pipeline-Aware Self-Model         S0→1✓→3
│   │  P1 ✓  P2 ✓  P3 ·  P4 ○
```

The `→1✓` shows: it reached Stage 1, that milestone is done. Still heading to 3.

#### Between Phases — "What should I do next?"

Expands to support decision-making:

```
  📦 Pipeline Awareness                       [1/4 phases done]

  Pipeline
  ▸ 00238 Pipeline-Aware Self-Model           S0→1✓→3
  │  Next: Phase 2 adds steering awareness
  │  Needs: goal metadata schema, steering engine changes
  │
  ▸ 00239 Pipeline Visual Language            +S0→1✓→3
  │  Next: Phase 3 implements sidebar rendering
  │
  ▸ 00236 Resource Projections                S0
  │  Next: Phase 3 spike
  │
  ─── Chore Lane ───
  (no pending chores)

  ─────────────────────────────────────────
  ✓ Just Finished
  │  Phase 1: Vision Capture + RFC Audit
  │  4 goals completed • 00238 promoted to S1
  │
  ○ Up Next: Phase 2 — Pipeline-Aware Steering
  │  5 goals planned • advancing 00238 S1→3
  │  [Start Phase]  [Preview Goals]
  │
  ○ Or...
  │  [Insert Chore Phase]  [Reorder Phases]  [View Backlog]
```

The pipeline section expands with "Next" and "Needs" context. The "Or..." section surfaces alternatives: inserting a chore phase, reordering upcoming phases, or reviewing the backlog.

### Chore Lane

Chore phases have distinct visual weight — lighter ceremony, no pipeline advancement:

```
▼ Phase: Merge PR #72 + cleanup               🔧 Chore
│
├── ○ Merge PR #72
├── ○ Delete stale branches
├── ○ Clean up .context/ temp files
│
│  (no RFCs — chore phases don't advance the pipeline)
```

The `🔧 Chore` badge replaces goal/task counts. No RFC section. The visual weight signals "interstitial, not pipeline work."

In the epoch view, chores appear in their own lane:

```
│   └── Chore Lane
│       🔧 Merge PR #72 + cleanup             ✓ done
│       🔧 Rebase feature branches             ← active
```

### Mid-Phase RFC Creation

When an RFC is created during a phase (not planned at phase start):

```
▸ 00239 Pipeline Visual Language              +S0→1
```

The `+` prefix signals "born this phase." In the epoch view, `+` persists because the RFC was born during this epoch — it's part of the story of what the epoch produced.

This handles the common pattern: working on a phase surfaces a design question, an RFC gets created to capture the decision, it may even get promoted during the same phase. The visual language represents this naturally without requiring the plan to have predicted it.

### CLI Application

The same notation works in CLI output:

```
$ exo pipeline
Pipeline Awareness [1/4 phases]

  ▸ 00238 Pipeline-Aware Self-Model         S0→1✓→3
  ▸ 00239 Pipeline Visual Language          +S0→1→3
  ▸ 00236 Resource Projections              S0→1
  ─── Chore Lane ───
  (empty)
```

```
$ exo phase status
Phase: Vision Capture + RFC Audit  ✓ 1/4 • 4/10

RFCs:
  ▸ 00238 S0→1  ▸ 00239 +S0→1  · 00224 S1
```

## Alternatives Considered

### Use color alone (no symbols)

Rejected: color doesn't work in all contexts (CLI piped to file, accessibility, agent tool output). The notation must be readable in plain text.

### Show all 210 RFCs in the sidebar

Rejected: the pipeline view should show only RFCs relevant to the current epoch/phase. The full inventory is a separate concern (RFC audit tooling, file explorer).

### Use numeric progress bars instead of stage badges

Rejected: progress bars imply linear completion. RFC stages are qualitative transitions (idea → proposal → draft → candidate → canon), not percentages. The stage badges (`○ ◐ ◑ ● ★`) better represent discrete pipeline positions.

## Drawbacks

- **Learning curve**: The notation is terse — new users need to learn what `S0→1✓→3` means. Mitigated by tooltips in the sidebar and a legend in documentation.
- **Movement arrows require planning data**: To show `S0→1→3`, the system needs to know the target stage. This requires epoch/phase metadata to encode RFC trajectory, which doesn't exist today.
- **Blocked status is manual**: The `◇` blocked indicator requires someone to mark an RFC as blocked. Automatic dependency detection is future work.

## Future Possibilities

- **Velocity indicators**: Show how fast RFCs are moving through the pipeline (e.g., "RFC 00238 promoted 2 stages in 3 phases")
- **Dependency graph**: Expand `◇` blocked indicator into a navigable dependency view
- **Notification integration**: Surface pipeline state changes as VS Code notifications ("RFC 00238 reached Stage 3!")
- **Agent perception**: Expose the visual language through LM tools so agents can perceive pipeline state the same way humans see it in the sidebar

## Visual Language Reference

```
Stage Badges    ○ S0   ◐ S1   ◑ S2   ● S3   ★ S4   ✕ W
Movement        S0→1  (one step)
                S0→1→3 (full trajectory, epoch view)
                S0→1✓→3 (milestone reached)
                +S0→1 (born this phase/epoch)
Role            ▸ driving   · related   ◇ blocked
Phase Touch     P1 ✓ (done)  P2 ○ (advances)  P3 · (references)
Chore           🔧 badge, no RFC section, lighter weight
```
