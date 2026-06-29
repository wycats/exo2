# E2E fixture modernization audit

## Purpose

This audit inventories the legacy E2E fixture assumptions that block canonical daemon/SQLite-backed E2E testing for the `Canonical E2E Harness Modernization` phase.

## Files inspected

- `packages/exosuit-vscode/tests/e2e/README.md`
- `packages/exosuit-vscode/tests/e2e/fixtures.ts`
- `packages/exosuit-vscode/tests/e2e/scenarios.ts`
- `packages/exosuit-vscode/tests/e2e/lib/exosuit-test.ts`
- `packages/exosuit-vscode/tests/e2e/workbench-fixture.ts`
- `packages/exosuit-vscode/tests/e2e/notebook-fixture.ts`
- `packages/exosuit-vscode/tests/e2e/utils.ts`
- `packages/exosuit-vscode/tests/e2e/test-logger.ts`
- `packages/exosuit-vscode/tests/e2e/tree-views.test.ts`
- `packages/exosuit-vscode/tests/e2e/ideas.test.ts`
- `packages/exosuit-vscode/tests/e2e/lm-tools.test.ts`
- `packages/exosuit-vscode/tests/e2e/studio.test.ts`
- `packages/exosuit-vscode/tests/e2e/rfc-view.test.ts`
- `packages/exosuit-vscode/tests/e2e/notebook.test.ts`
- `packages/exosuit-vscode/tests/e2e/activation.test.ts`
- `packages/exosuit-vscode/tests/e2e/diagnostics.test.ts`
- `packages/exosuit-vscode/tests/e2e/extensions.test.ts`
- `packages/exosuit-vscode/tests/e2e/templates/migration-template.ts`
- `packages/exosuit-vscode/playwright.config.ts`
- `packages/exosuit-vscode/scripts/test-e2e.ts`
- `packages/exosuit-vscode/package.json`
- `packages/exosuit-vscode/src/PlanService.ts`
- `packages/exosuit-vscode/src/ExosuitTreeProvider.ts`
- `packages/exosuit-vscode/src/extension.ts`

## Legacy assumptions

### File projection seeding instead of canonical state

`ScenarioBuilder` writes fixture files directly into the holodeck workspace. It creates agent-context directories and writes TOML projections for phases, implementation plans, ideas, and inbox entries. It does not run canonical `exo` commands, initialize SQLite state, import a fixture database, or notify daemon-backed trace roots.

This blocks canonical sidebar and tree tests because extension state now flows through daemon/machine-channel reads and TraceCache invalidation rather than direct TOML reads.

### Project Plan tests depend on legacy plan TOML

`tree-views.test.ts` generates legacy implementation-plan TOML in test code, writes it into `docs/agent-context/current/implementation-plan.toml`, and expects Project Plan trees to render from that projection.

The current extension loads plan data through machine-channel `plan.read`, and it refreshes tree providers from TraceCache writes. Direct file writes do not establish daemon-visible state and do not exercise the canonical reactivity path.

### Reactivity tests mutate projection files directly

The Project Plan reactivity test edits legacy plan TOML and expects the tree to update. Ideas reactivity tests edit legacy ideas TOML and expect Studio output to update.

Canonical reactivity must be driven by daemon writes or machine-channel notifications. Direct projection mutation exercises obsolete behavior.

### LM tool tests run real commands against non-canonical seeds

`lm-tools.test.ts` resolves and invokes a real `exo` binary with the holodeck as the current working directory, but the fixture setup uses legacy file projections. Those tests either fail under canonical state or accidentally test migration/import fallback instead of the daemon/SQLite path.

### README and migration template encode the obsolete model

The E2E README describes the current fixture helpers as SQLite-backed even though they write TOML projections. The migration template also seeds a legacy implementation-plan TOML file. Both need to be updated after canonical seed helpers exist.

### Ideas and Studio tests conflate document rendering with canonical state

Ideas and Studio suites open and render managed TOML/markdown documents directly. That is valid for document-rendering smoke tests, but it is not canonical Ideas, phase, plan, or RFC state coverage.

### RFC tests create managed files directly

RFC view tests and Studio RFC rendering tests create staged RFC markdown directly. That is acceptable for pure renderer smoke tests. It is not acceptable for canonical RFC pipeline tests because it bypasses RFC CLI creation, IDs, metadata, SQLite relationships, and phase linkage.

## Blocker classification

### Hard blockers

1. `ScenarioBuilder` has no canonical seed path.
2. Project Plan tree tests seed and mutate legacy plan TOML.
3. LM tool tests run canonical CLI commands against legacy TOML seeds.
4. Reactivity tests mutate files directly instead of invoking canonical writes.
5. The migration template still teaches the obsolete fixture pattern.
6. The README mislabels TOML-backed helpers as SQLite-backed.

### Partial blockers

1. Ideas tests are document-rendering tests, not canonical Ideas tests.
2. Studio tests remain useful renderer tests, but they should not be counted as canonical phase/plan/axiom state tests.
3. Direct RFC file fixtures are fine for rendering and wrong for RFC pipeline coverage.

### Non-blockers

1. Activation, diagnostics, extensions, notebook, workbench, webview monitor, Playwright config, and logging helpers are harness/platform concerns.
2. VS Code isolation in the E2E fixture layer is compatible with canonical daemon-backed testing.

## Canonical fixture requirements implied by the audit

The next task should define a seed layer that can:

1. Initialize an Exosuit project in a holodeck workspace.
2. Create daemon/SQLite-visible epochs, phases, goals, tasks, ideas, inbox items, and RFC relationships.
3. Use deterministic IDs and labels for test assertions.
4. Trigger the same invalidation path used in production, including TraceCache refreshes.
5. Keep document-rendering fixtures separate from canonical state fixtures.
6. Preserve VS Code/Electron isolation and existing diagnostic dump behavior.

## Recommended migration order

1. Add canonical holodeck seed helpers beside the existing `ScenarioBuilder`.
2. Migrate Project Plan tree tests first.
3. Migrate LM tool tests second.
4. Split Ideas tests into document-rendering and canonical-state suites.
5. Add a Run sidebar happy-path test with seeded active phase, goals, and tasks.
6. Keep pure RFC/Studio renderer tests separate, then add RFC pipeline tests using canonical creation paths.
7. Update the README and migration template after canonical helpers land.

## Canonical seed requirements

### Seed layer shape

Add a canonical seed layer beside `ScenarioBuilder` instead of replacing it outright. `ScenarioBuilder` should remain available for document-rendering fixtures; canonical state tests should opt into a separate helper so tests cannot accidentally count direct file fixtures as daemon-backed coverage.

The helper should hang off `ExosuitTest`, for example `exo.seed`, and provide an explicit setup flow:

1. `initProject()` creates an isolated git-backed Exosuit workspace in the holodeck.
2. Entity builders enqueue deterministic epochs, phases, goals, tasks, ideas, inbox items, and RFCs.
3. `apply()` performs canonical writes through the same CLI or machine-channel paths used by real users.
4. `refresh()` triggers the production invalidation path after writes, either by relying on daemon write notifications or by executing a harmless canonical read after the writes complete.

The helper must not write `docs/agent-context/*.toml` or `docs/agent-context/*.sql` directly.

### Project initialization

Canonical E2E setup needs a non-interactive initializer. The current CLI supports `exo init --defaults`, but it only works safely when the holodeck is git-backed and already contains only allowed files such as `exosuit.toml`.

Required initialization behavior:

1. Run `git init` in each holodeck before canonical seeding.
2. Write a minimal `exosuit.toml` before initialization:

```toml
[storage]
backend = "sqlite"
```

3. Run `exo init --defaults` through the same `exo` binary used by E2E tests.
4. Treat generated repo files as expected holodeck artifacts: `AGENTS.md`, `.config/exo/hooks.toml`, `.github/prompts/**`, `.github/instructions/**`, `.gitattributes`, `.gitignore`, `.exo/cache/exo.db`, and generated SQL projections.
5. Avoid interactive `exo init`; it blocks in non-TTY probes and is not suitable for Playwright fixtures.

### Command surface for canonical writes

Use canonical commands for entity creation:

| Entity       | Canonical write path                                                                                                  | Determinism requirement                                                                          |
| ------------ | --------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| Epoch        | `exo epoch add --title <title>`                                                                                       | Capture generated ID from JSON output or derive it through a follow-up read.                     |
| Active epoch | `exo epoch start <id>`                                                                                                | Start by ID after creation.                                                                      |
| Phase        | `exo phase add --title <title> --epoch <epoch-id> --kind <kind>`                                                      | Capture generated ID from JSON output or derive it through a follow-up read.                     |
| Active phase | `exo phase start <id>`                                                                                                | Start by ID after creation.                                                                      |
| Goal         | `exo goal add <label> --id <id> --phase <phase-id>`                                                                   | Prefer explicit IDs.                                                                             |
| Task         | `exo task add <label> --id <id> --goal <goal-id>`                                                                     | Prefer explicit IDs.                                                                             |
| Idea         | `exo idea add <title> --description <text> --tags <tags>`                                                             | If explicit IDs are needed, add CLI support before canonical Ideas E2E assertions depend on IDs. |
| Inbox item   | `exo inbox add <subject> --entity-type <type> --entity-id <id> --intent <intent> --priority <priority> --body <body>` | Target seeded entities by explicit ID.                                                           |
| RFC          | `exo rfc create <title> --id <id> --stage <stage> --body <body>`                                                      | Always use `exo rfc create`; never create RFC markdown directly for pipeline tests.              |

The seed helper should call commands with `--format json` when available and parse IDs from structured output. If a write command lacks enough JSON output to recover generated IDs reliably, the helper should either require explicit IDs or add the missing JSON result in the CLI before migrating the dependent E2E test.

### Daemon and TraceCache requirements

Canonical UI tests need the extension to observe daemon-visible state, not just a populated database on disk.

Requirements:

1. Seed writes must target the same holodeck workspace root that the extension opens.
2. Seed writes must happen after the holodeck path exists and before assertions that read sidebar state.
3. When writes happen before extension activation, the first TraceCache root fetch should see the seeded SQLite state.
4. When writes happen after extension activation, writes must go through daemon/machine-channel or CLI commands that produce daemon write notifications.
5. Project Plan assertions depend on `plan.read`.
6. Active phase goal/task assertions depend on `phase.read-goals` and `phase.read-tasks`.
7. Run sidebar assertions depend on TraceCache roots:
   - `context.snapshot`
   - `phase.read-details`
   - `rfc.pipeline`
8. Reactivity tests should mutate state through canonical writes and wait for the UI to update via TraceCache invalidation, not via file watchers.

### Test-facing assertions

The seed layer should expose the created IDs and labels to tests so assertions do not rediscover state through DOM text only.

Minimum seed result shape:

```ts
type CanonicalSeedResult = {
  workspaceRoot: string;
  epochs: Record<string, { id: string; title: string }>;
  phases: Record<string, { id: string; title: string; epochId: string }>;
  goals: Record<string, { id: string; label: string; phaseId: string }>;
  tasks: Record<string, { id: string; label: string; goalId?: string }>;
  ideas: Record<string, { id?: string; title: string }>;
  inbox: Record<string, { id?: string; subject: string }>;
  rfcs: Record<string, { id: string; title: string; path?: string }>;
};
```

Tests should assert against user-visible labels while using IDs for follow-up writes and focus commands.

### Migration constraints

1. Keep legacy document helpers named and documented as legacy/file-projection helpers.
2. Do not edit generated `docs/agent-context/**` files by hand inside fixtures.
3. Do not create RFC files directly except in pure markdown-rendering tests.
4. Do not require network access or GitHub authentication.
5. Keep sidecar remote creation out of the canonical seed layer; sidecar tests can add that later.
6. Keep each holodeck isolated from the developer’s real `.exo` state by using a git-backed temp workspace and repo-local `.exo` state.
7. Avoid interactive CLI flows.

### First implementation target

The first seed helper should support the Project Plan tree tests and Run sidebar happy-path test only:

1. Initialize canonical repo-policy state.
2. Create two epochs.
3. Create multiple phases with `completed`, `in-progress`, and `pending` states.
4. Create at least one goal and two tasks under the active phase.
5. Start the desired active epoch and phase.
6. Verify `exo status --format json`, `exo plan review --format json`, `exo phase status --format json`, `exo goal list --format json`, and `exo task list --format json` all succeed before UI assertions run.

Ideas, inbox, and RFC pipeline seeding should follow after Project Plan tree coverage is migrated.

### Builder interface decision

The initial builder interface lives in `packages/exosuit-vscode/tests/e2e/canonical-seed.ts` and is exposed as `exo.seed` from the E2E harness. It intentionally defines the test-facing API before wiring command execution:

```ts
await exo.seed
  .epoch({ key: "main", title: "Main Epoch", status: "in-progress" })
  .phase({
    key: "active",
    epoch: "main",
    title: "Active Phase",
    status: "in-progress",
  })
  .goal({ key: "goal", id: "seed-goal", phase: "active", label: "Seeded Goal" })
  .task({ key: "task", id: "seed-task", goal: "goal", label: "Seeded Task" })
  .apply();
```

The API returns `CanonicalSeedResult` with stable test keys mapped to concrete Exosuit IDs. The next task, `canonical-e2e-design-seed-api::implement-daemon-seed-helpers`, should replace the placeholder command runner with real `exo` CLI execution and then harden the command output parsing against actual JSON result shapes.

## Harness modernization coverage review

### Modernized coverage

The following coverage now exercises canonical daemon/SQLite-backed state rather than direct file projections:

1. Project Plan tree rendering:

- Displays epochs and phases correctly.
- Shows phase status indicators.
- Renders multiple epochs with multiple phases.

2. Project Plan refresh behavior:

- Updates when canonical phase state changes through `exo phase update`.
- Verifies canonical `exo plan read` observes the change before asserting UI refresh.

3. Run sidebar happy path:

- Seeds an active epoch and phase.
- Seeds a goal and an in-progress task.
- Asserts Phase Details renders the phase header/progress, seeded goal, and seeded task.
- Asserts Epoch Context renders the active phase.
- Rejects recovery/empty states in the happy-path assertion.

4. Seed harness behavior:

- `CanonicalSeedBuilder` is unit-covered for command ordering and stable test-key mappings.
- `DaemonChannelServer` restart/write invalidation has focused unit coverage.

### Canonical fixture infrastructure now available

The E2E harness now exposes:

- `exo.seed` for fresh canonical state fixtures.
- `exo.seedMore` for additive canonical state in an already initialized holodeck.
- `runCanonicalSeedCommand()` for direct canonical writes during a test.
- Self-contained holodeck initialization through git, `exosuit.toml`, `exo init --defaults`, and a workspace-local `target/debug/exo`.
- Extension-host `EXO_BIN` wiring so the daemon and seed helper use the same binary.
- Transient SQLite retry handling for startup races.

### Remaining legacy coverage

The following suites still use file-projection or direct document fixtures:

1. `lm-tools.test.ts`

- Uses `withPhase()`, `withImplementationPlan()`, `withIdeas()`, and `withInbox()` before invoking real `exo` commands.
- This is the highest-priority remaining canonical migration because it currently claims CLI confidence while relying on non-canonical seeds.

2. `ideas.test.ts`

- Uses `docs/agent-context/ideas.toml` and `withPhase()`.
- Split into document-rendering coverage and separate canonical Ideas state coverage.

3. `studio.test.ts`

- Uses direct RFC, axiom, feedback, and implementation-plan documents.
- Keep renderer coverage, but do not count it as canonical phase/plan/RFC-pipeline state coverage.

4. `rfc-view.test.ts`

- Creates raw RFC markdown directly.
- Acceptable for renderer smoke only; canonical RFC pipeline tests must use `exo rfc create`.

5. `templates/migration-template.ts`

- Still demonstrates `docs/agent-context/plan.toml` seeding and should be replaced before new migrations copy it.

6. `tree-views.test.ts` empty-state case

- Still uses a direct empty `plan.toml` as recovery/document-fixture coverage.
- This is acceptable only as a recovery test and should be renamed when the suite is cleaned up.

### Coverage conclusion

The current phase has delivered the intended first canonical slice: Project Plan rendering, Project Plan update behavior, and Run sidebar happy path now run through canonical daemon/SQLite-backed seeds. The harness is ready for follow-up migrations, with LM tool tests as the next high-value target.
