<!-- exo:10172 ulid:01kmzxey21ne4memb0sbhm5aeq -->


# RFC 10172: Sidebar Visual Design: Principles, Mechanisms, and Pipeline Notation

> Supersedes: RFC 0094, RFC 10169, RFC 00239

> Consolidates RFC 0094 (Sidebar-First UI Design), RFC 10169 (FileDecoration-Based Tree Item Styling), and RFC 00239 (Pipeline Visual Language) into a single layered design document.

## Summary

This RFC defines the visual design system for Exosuit's VS Code sidebar, organized in three layers:

1. **Design Principles** — Density, native integration, and glanceability constraints
2. **Implementation Mechanism** — `FileDecorationProvider` + `resourceUri` for label coloring without icon gutters
3. **Pipeline Notation** — A terse, composable visual language for RFC pipeline state

Together these layers ensure the sidebar feels like a native VS Code extension while conveying rich pipeline information at a glance.

## Layer 1: Design Principles

_Origin: RFC 0094_

### The Sidebar is the Stage

Assume a default width of ~300px. UI must be responsive but optimized for this narrow column. Horizontal scrolling is a failure state.

### Density is Data

In a coding environment, screen real estate is precious.

- **Avoid**: Large padding (e.g., `20px`), card containers with drop shadows, `<h1>` page titles.
- **Prefer**: Compact lists, 4-8px padding, 1px borders for separation.

### Native Camouflage

The UI should look like VS Code itself, not a website embedded in it.

- Use `var(--vscode-...)` CSS variables for all colors (backgrounds, foregrounds, borders, inputs).
- Match the typography and font sizes of the editor (usually 13px).
- Use standard VS Code icons (Codicons) where icons are needed.

### Design Patterns

**Section Headers**: Uppercase text, ~11px, bold, using `--vscode-sideBarSectionHeader-background` and `--vscode-sideBarSectionHeader-foreground`.

**Lists over Cards**: Data in flat lists with 1px solid borders for separation. Hover states use `--vscode-list-hoverBackground`.

**Status Colors**: Use accessible VS Code theme colors:

- Info/Open: `--vscode-charts-blue`
- Success/Resolved: `--vscode-charts-green`
- Warning: `--vscode-charts-yellow`
- Error: `--vscode-charts-red`

## Layer 2: Tree Item Styling via FileDecoration

\*Origin: RFC 10169. **Status: Implemented.\***

### Core Mechanism

VS Code's `FileDecorationProvider` + `resourceUri` enables coloring tree item label text directly, replacing the ThemeIcon-only approach that required a 20px icon gutter.

1. **`resourceUri`**: Each TreeItem gets a URI encoding its type and status:

   ```
   exosuit-tree://task/completed/some-task-id
   exosuit-tree://goal/in-progress/some-goal-id
   ```

2. **`FileDecorationProvider`**: A single `TreeDecorationProvider` parses the URI and returns a `ThemeColor` for the label text.

3. **Zero-width icon trick**: Setting `iconPath = undefined` removes the icon gutter. Unicode symbols in the label text serve as status glyphs.

### Implementation

The `TreeDecorationProvider` class (in `TreeDecorationProvider.ts`) implements `FileDecorationProvider`:

```typescript
// URI scheme: exosuit-tree://<type>/<status>/<id>
// Status → ThemeColor mapping:
//   completed       → charts.green
//   in-progress     → charts.yellow
//   ready-for-logging → charts.green
//   phase-active    → charts.blue
//   abandoned/skipped → charts.gray
//   pending         → (no color, default foreground)
```

The `treeItemUri()` helper builds URIs, and `TreeDataService` assigns `resourceUri` to tree items while omitting `iconPath` to reclaim horizontal space.

### What This Unlocks

| Capability           | Before (ThemeIcon) | After (FileDecoration)      |
| -------------------- | ------------------ | --------------------------- |
| Colored label text   | No                 | Yes                         |
| Badge suffix         | No                 | Yes (1-2 chars)             |
| Icon gutter          | Always 20px        | Optional (zero-width trick) |
| Status color channel | Icon glyph only    | Entire label text           |

### Design Decision: When to Use Which

| Item Type        | Approach                                       | Rationale                                                |
| ---------------- | ---------------------------------------------- | -------------------------------------------------------- |
| Tasks, Goals     | FileDecoration color + Unicode symbol in label | Status color on text is more informative than icon color |
| Log items, notes | Zero-width icon + no decoration                | Color doesn't matter, save space                         |
| Section headers  | Zero-width icon + no decoration                | Structural, not status-bearing                           |

## Layer 3: Pipeline Visual Language

_Origin: RFC 00239. **Status: Partially implemented** — stage dots, lifecycle glyphs, role prefixes, and short IDs are implemented. Movement arrows, milestone markers, and chore lane are forward plan._

### Core Vocabulary

Three orthogonal dimensions compose to describe any RFC's pipeline state:

#### Stage Dots (Positional System)

A 4-dot sequence encodes the RFC's current stage position:

```
○○○○   S0  Idea         (no stages completed)
●○○○   S1  Proposal     (stage 1 complete)
●●○○   S2  Draft        (stages 1-2 complete)
●●●○   S3  Candidate    (stages 1-3 complete)
●●●●   S4  Canon        (all stages complete)
```

Glyphs: `●` (U+25CF) completed, `○` (U+25CB) future.

#### Lifecycle Status (Active Stage Transition)

When a stage transition is in progress, the target position shows lifecycle status:

```
◌   Idle         (target identified, work not started)
◔   In-progress  (actively being worked on)
◕   Validating   (implementation complete, verifying)
●   Done         (stage transition complete)
```

Combined example (Stage 1 RFC targeting Stage 2):

```
●◌○○   S1→2 idle
●◔○○   S1→2 in-progress
●◕○○   S1→2 validating
●●○○   S2   (promotion complete)
```

**Key rule**: Lifecycle fill variants (`◌ ◔ ◕`) only appear when an RFC is "in motion" — linked to the active phase with a target stage. Static RFCs show only `●` and `○`.

#### Multi-Stage Advancement

When a phase advances an RFC through multiple stages, each target position shows its own lifecycle status:

```
●◌◌○   S1→3 idle        (both S2 and S3 targets queued)
●◔◌○   S1→2 in-progress (working toward S2, S3 still queued)
●●◌○   S2 reached       (S2 done, S3 now the active target)
●●◔○   S2→3 in-progress
●●●○   S3 reached
```

#### RFC ID Format

Short format with `#` prefix: `#225` (not `00225`). Full IDs in tooltips and file paths.

#### Role Prefix

```
▸  Driving   — this phase/epoch is advancing this RFC
·  Related   — referenced, informs the work
◇  Blocked   — needs something before it can advance
```

### Implementation: Shared Component

All RFC rendering flows through a shared `rfcDisplay.ts` module (Layer 1: pure TypeScript, no UI dependencies):

```typescript
// Types: RfcDisplayState, DotData, LifecycleStatus, DotStatus
// Functions:
//   computeStageDots(state) → DotData[]  (for Svelte components)
//   renderStageDots(state) → string      (for TreeView/CLI)
//   formatRfcId(id, format) → string     (#225 or 00225)
```

The `RfcPipelineProvider` consumes these functions to build the pipeline TreeView with three sections: In-Flight (driving), Blocked, and Related.

### Relation Field

Each phase-RFC association carries a `relation` field (`"driving"` | `"related"` | `"blocked"`) stored in `implementation-plan.toml`. A migration plugin (`normalize_rfc_relation.rs`, Critical severity) ensures all `[[epochs.phases.rfcs]]` entries have a normalized relation on load. The relation determines:

- Which pipeline section the RFC appears in
- Which role prefix glyph is shown
- Whether lifecycle glyphs are displayed (only for driving RFCs)

### Forward Plan (Not Yet Implemented)

The following elements from the original visual language design are not yet implemented:

#### ~~Movement Arrows~~ (Superseded)

The original RFC 00239 proposed text-based movement arrows (`S0→1`, `S0→1→3`). These have been **superseded by the lifecycle glyph system**:

- Movement is now encoded by lifecycle glyphs (`◌ ◔ ◕`) at target positions
- Multi-stage advancement shows multiple lifecycle glyphs: `●◌◌○` (S1→3 queued)
- The circle system is denser (4 chars vs 7+) and encodes lifecycle status that arrows couldn't

No implementation needed — the circle system already provides this functionality.

#### Chore Lane (Future)

Chore phases with `🔧 Chore` badge, no RFC section, lighter visual weight. See RFC 00238 for the chore lane design.

#### Phase Touch Indicators (Future, Epoch View)

```
P1 ✓  (done advancing)   P2 ○  (advances)   P3 ·  (references)
```

### Surface Migration Status

The shared `rfcDisplay.ts` module provides the canonical RFC rendering logic. **Migration complete as of Feb 2026.**

#### Current State

| Surface               | File                                                                         | Status      | Notes                                                              |
| --------------------- | ---------------------------------------------------------------------------- | ----------- | ------------------------------------------------------------------ |
| `rfcDisplay.ts`       | `packages/exosuit-vscode/src/services/rfcDisplay.ts`                         | ✅ Source   | Exports `computeStageDots()`, `renderStageDots()`, `formatRfcId()` |
| `RfcPipelineProvider` | `packages/exosuit-vscode/src/RfcPipelineProvider.ts`                         | ✅ Migrated | Uses all shared functions                                          |
| `TreeDataService`     | `packages/exosuit-vscode/src/TreeDataService.ts`                             | ✅ Migrated | Uses `formatRfcBadgesWithDots()` for phase header RFC badges       |
| Dashboard             | `packages/exosuit-vscode/src/webview/dashboard/App.svelte`                   | ✅ Migrated | Extension sends `stageDots` and `formattedId`                      |
| RfcView               | `packages/exosuit-vscode/src/webview/studio/RfcView.svelte`                  | ✅ Migrated | Local `renderStageDots()` matching shared glyphs                   |
| TextField chips       | `packages/exosuit-vscode/src/webview/studio/lib/components/TextField.svelte` | ✅ Migrated | Badge from Rust `render_stage_dots()`                              |
| file-refs (Rust)      | `crates/exosuit-file-refs/src/present.rs`                                    | ✅ Migrated | `render_stage_dots()` matching TypeScript                          |
| CLI rfc commands      | `tools/exo/src/command/rfc.rs`                                               | ✅ Migrated | `render_stage_dots()` in list/show/status output                   |
| CLI status/phase      | `tools/exo/src/status.rs`, `tools/exo/src/command/phase_cmd.rs`              | N/A         | No RFC rendering currently                                         |

#### Migration 1: TreeDataService (TypeScript)

**Goal**: Wire stage dots into phase header RFC badges and between-phases RFC preview.

**Current behavior** (L347-359, L273-278):

- Phase header shows RFC count badge: `"RFCs: 3"`
- Between-phases preview shows RFC IDs: `"RFCs: #225, #238"`

**Target behavior**:

- Phase header: `"RFCs: ●●○○ #225, ●○○○ #238"` (stage dots + short ID)
- Between-phases: Same format with dots

**Data requirement**: Need `RfcDisplayState` for each RFC:

- `currentStage`: From RFC file path (already parsed as `RfcMetadata.stage`)
- `targetStages`: From phase RFC linkage (if driving)
- `lifecycleStatus`: From phase progress (if in-motion)
- `role`: From `PhaseRfc.relation` field

**Implementation steps**:

1. Add `getRfcDisplayState(rfcId: string, phaseRfcs: PhaseRfc[]): RfcDisplayState` helper
2. Update `buildPhaseHeader()` to render dots via `renderStageDots()`
3. Update between-phases RFC preview to include dots

**Complexity**: Moderate — data is available, just needs wiring.

#### Migration 2: Dashboard Svelte Component

**Goal**: Replace raw `rfc.number` with formatted ID and stage dots.

**Current behavior** (App.svelte L143-146):

```svelte
{#each rfcs as rfc}
  <div class="rfc-item">{rfc.number}: {rfc.title}</div>
{/each}
```

**Target behavior**:

```svelte
{#each rfcs as rfc}
  <div class="rfc-item">{renderStageDots(state)} {formatRfcId(rfc.id)} {rfc.title}</div>
{/each}
```

**Data requirement**: Dashboard receives RFC list from extension. Need to include stage in the data or compute from RFC path.

**Implementation steps**:

1. Export `rfcDisplay` functions for Svelte consumption (may need build config)
2. Extend RFC data passed to dashboard to include `stage`
3. Update template to use shared rendering

**Complexity**: Moderate — need to verify Svelte can import from `services/`.

#### Migration 3: RfcView Svelte Component

**Goal**: Replace local 5-position CSS dots with shared 4-position glyph system.

**Current behavior** (RfcView.svelte L262-268):

```svelte
<div class="stage-dots">
  {#each [0,1,2,3,4] as i}
    <span class="dot" class:filled={i <= stage}></span>
  {/each}
</div>
```

**Issue**: Uses 5 positions (0-4) vs `rfcDisplay`'s 4 positions (stages 1-4 map to positions 0-3). Stage 0 shows as `○○○○` (no filled dots).

**Target behavior**:

```svelte
<div class="stage-dots">
  {#each computeStageDots(state) as dot}
    <span class="dot {dot.status}">{dot.glyph}</span>
  {/each}
</div>
```

**Implementation steps**:

1. Import `computeStageDots` and types
2. Replace CSS-based dots with glyph-based rendering
3. Update CSS to style glyphs instead of filled/unfilled circles
4. Remove local `getDisplayStatus()` and `getStageNumber()`

**Complexity**: Moderate — CSS refactor needed.

#### Migration 4: TextField File-Ref Chips

**Goal**: Align chip badges with shared glyph system.

**Current behavior**: Badge comes from Rust `present_file_ref()` which has its own `stage_glyph()`.

**Dependency**: Requires Rust migration first (Migration 5). Once Rust emits consistent glyphs, chips will automatically align.

**Complexity**: Trivial once Rust is migrated.

#### Migration 5: Rust file-refs Crate

**Goal**: Port glyph constants to Rust, ensure consistency with TypeScript.

**Current behavior** (`present.rs` L4-21):

```rust
fn stage_glyph(stage: u8) -> &'static str {
    match stage {
        0 => "○",
        1 => "◐",
        2 => "◑",
        3 => "●",
        4 => "★",
        _ => "?",
    }
}
```

**Issue**: Different glyphs than `rfcDisplay.ts`. Uses half-circles and star.

**Target behavior**:

```rust
// Match rfcDisplay.ts GLYPHS
const GLYPH_COMPLETED: &str = "●";  // U+25CF
const GLYPH_FUTURE: &str = "○";     // U+25CB

fn render_stage_dots(current_stage: u8) -> String {
    (1..=4).map(|pos| {
        if pos <= current_stage { GLYPH_COMPLETED } else { GLYPH_FUTURE }
    }).collect()
}
```

**Implementation steps**:

1. Add `rfc_display.rs` module with glyph constants matching TypeScript
2. Port `renderStageDots()` logic
3. Update `present_file_ref()` to use new module
4. Add tests ensuring parity with TypeScript output

**Complexity**: Moderate — straightforward port, but need to maintain parity.

#### Migration 6: CLI RFC Commands

**Goal**: Add stage dots to `rfc list`, `rfc show`, `rfc status` output.

**Current behavior** (`rfc.rs`):

- `rfc list`: Table with stage number column
- `rfc show`: Prints `Stage: 2`
- `rfc status`: Groups by stage name

**Target behavior**:

- `rfc list`: `●●○○  #225  Sidebar Visual Design`
- `rfc show`: `Stage: ●●○○ (Draft)`
- `rfc status`: Same grouping, dots in each row

**Dependency**: Requires Migration 5 (Rust glyph module).

**Implementation steps**:

1. Import from shared Rust module
2. Update table formatting to include dots
3. Update show/status output

**Complexity**: Moderate — formatting changes.

#### Migration Order

1. **TreeDataService** — Already TypeScript, data available, immediate value
2. **Dashboard/RfcView Svelte** — Share TS module, unify visual language in UI
3. **Rust file-refs** — Port glyphs, establish Rust source of truth
4. **TextField chips** — Automatic once Rust migrated
5. **CLI commands** — Use Rust module, complete the circle

#### Verification

After migration, all surfaces should render the same RFC identically:

```
●●○○ #225  (Stage 2 Draft, static)
●◔○○ #238  (Stage 1→2 in-progress, in-motion)
```

Test case: Create an RFC at Stage 2, verify dots match across:

- RfcPipelineProvider tree
- TreeDataService phase header
- Dashboard list
- RfcView detail
- File-ref chip
- CLI `rfc list` output

### Layered Architecture

```
┌─────────────────────────────────────────────────────────────┐
│  Layer 2: Svelte Components (Studio only)                   │
│  <RfcStageDots dots={$rfcDots} />                           │
│  Reactive binding to DotData[], CSS styling                 │
├─────────────────────────────────────────────────────────────┤
│  Layer 1: Pure Data Model + Rendering Logic                 │
│  rfcDisplay.ts (no UI dependencies)                         │
│  - computeStageDots(state) → DotData[]                      │
│  - renderStageDots(state) → string                          │
│  - formatRfcId(id, format) → string                         │
└─────────────────────────────────────────────────────────────┘
```

## Visual Language Reference

```
Stage Dots      ○○○○ S0   ●○○○ S1   ●●○○ S2   ●●●○ S3   ●●●● S4
                Position encodes stage (1-4), fill encodes completion

Lifecycle       ◌ idle   ◔ in-progress   ◕ validating   ● done
                Replaces target position dot during active transition
                ONLY appears when RFC is in-motion (active phase linkage)

Multi-Stage     ●◌◌○ S1→3 idle (both targets queued)
                ●◔◌○ S1→2 in-progress, S3 queued
                ●●◌○ S2 done, S3 now active target
                ●●◔○ S2→3 in-progress
                ●●●○ S3 reached

Static vs       ●●○○ Static (not in-motion, no lifecycle glyphs)
In-Motion       ●◔○○ In-motion (lifecycle glyph at target)

ID Format       #225 (short, default)
                00225 (full, tooltips/paths)

Role            ▸ driving   · related   ◇ blocked
```

## Alternatives Considered

### Color alone (no symbols)

Rejected: color doesn't work in all contexts (CLI piped to file, accessibility, agent tool output). The notation must be readable in plain text.

### Numeric progress bars

Rejected: progress bars imply linear completion. RFC stages are qualitative transitions, not percentages. The positional dot system better represents discrete pipeline positions.

### Unique glyphs per stage (○ ◐ ◑ ● ★)

Rejected: conflicted with using fill variants for lifecycle status. Half-circle glyphs render at inconsistent sizes across fonts.

## Drawbacks

- **Learning curve**: The notation is terse — new users need to learn what stage dots mean. Mitigated by tooltips.
- **Movement arrows require planning data**: Showing trajectories requires epoch/phase metadata encoding RFC targets.
- **Blocked status is manual**: The `◇` blocked indicator requires explicit marking. Automatic dependency detection is future work.
