# E2E Testing Guide

This directory contains Playwright end-to-end tests for the Exosuit VS Code extension.

## Test architecture

### Holodeck isolation

Each test runs in a temporary workspace called a holodeck. The holodeck gives every test a fresh workspace root, VS Code user data directory, and extension directory. Tests must not depend on state from previous tests or from the developer's real workspace.

### Two fixture modes

The E2E harness now has two distinct fixture modes:

1. **Canonical state fixtures** through `exo.seed`.
2. **Document fixtures** through `exo.holodeck` / `ScenarioBuilder`.

Use the correct mode for the surface under test. Do not use direct files as a substitute for canonical Exosuit state.

## Canonical state fixtures: `exo.seed`

Use `exo.seed` for tests that exercise daemon/SQLite-backed Exosuit state, including:

- Project Plan
- Phase Details
- Epoch Context
- Run sidebar happy paths
- LM tool/CLI behavior that reads Exosuit state
- RFC pipeline tests that need RFC metadata relationships

`exo.seed` initializes a git-backed holodeck, writes a minimal `exosuit.toml`, runs `exo init --defaults`, installs a workspace-local `target/debug/exo`, and creates entities through canonical `exo` commands.

Example:

```typescript
const seeded = await exo.seed
  .epoch({ key: "run", title: "Run Sidebar Epoch" })
  .phase({
    key: "active",
    epoch: "run",
    title: "Run Sidebar Active Phase",
    status: "in-progress",
  })
  .goal({
    key: "goal",
    id: "run-sidebar-goal",
    phase: "active",
    label: "Run Sidebar Seeded Goal",
  })
  .task({
    key: "task",
    id: "run-sidebar-task",
    goal: "goal",
    label: "Run Sidebar Seeded Task",
    status: "in-progress",
  })
  .apply();
```

The returned `CanonicalSeedResult` maps stable test keys to concrete generated Exosuit IDs:

```typescript
seeded.epochs.run.id;
seeded.phases.active.id;
seeded.goals.goal.id;
seeded.tasks.task.id;
```

Use those IDs for follow-up canonical writes.

### Additive canonical seeding: `exo.seedMore`

Use `exo.seedMore` only when a test needs to add canonical state to an already initialized holodeck. It does not rerun `exo init --defaults` when `.exo/cache/exo.db` already exists.

### Canonical follow-up writes

Use `runCanonicalSeedCommand()` for direct canonical mutations inside a test:

```typescript
await runCanonicalSeedCommand(holodeckPath, [
  "phase",
  "update",
  seeded.phases.active.id,
  "--title",
  "Updated Phase",
]);
```

The helper appends `--format json`, parses the machine-channel-shaped response, and retries transient SQLite open/lock races that can happen while the extension host is starting.

After an external canonical write, refresh the UI through the same path the extension uses for daemon-backed data. Current tree tests use the machine-channel restart command before reading views that depend on daemon state.

## Document fixtures: `exo.holodeck` / `ScenarioBuilder`

Use `ScenarioBuilder` only for tests that intentionally exercise direct document rendering or missing-file recovery. It writes files directly into the holodeck.

Appropriate uses:

- Opening markdown or TOML documents in Studio.
- Pure RFC markdown renderer smoke tests.
- Missing-file or recovery-state tests.
- Notebook/editor behavior that does not depend on Exosuit daemon state.

Do not use `ScenarioBuilder.withPhase()`, `withIdeas()`, `withInbox()`, or raw `docs/agent-context/**` files for Project Plan, Run sidebar, Phase Details, Epoch Context, or LM tool state tests. Those helpers are legacy file-projection fixtures.

## Managed directory rules

`docs/rfcs/` and `docs/agent-context/` are managed data stores.

- Canonical RFC pipeline tests must use `exo rfc create`, not raw markdown file creation.
- Canonical state tests must never write `docs/agent-context/*.toml` or `docs/agent-context/*.sql` directly.
- Generated SQL projections are expected artifacts of `exo init` and canonical writes; tests should not edit them by hand.

Direct RFC files are acceptable only for pure renderer tests that explicitly do not assert RFC metadata, pipeline relationships, or phase linkage.

## Required canonical initialization behavior

Canonical fixtures must be self-contained:

1. Run in a git-backed temporary workspace.
2. Use repo-local SQLite state under `.exo/cache/exo.db`.
3. Provide a minimal `exosuit.toml` with:

```toml
[storage]
backend = "sqlite"

[dev]
binary_dir = "target/debug"
```

4. Install a workspace-local `target/debug/exo` so the extension daemon and seed helper use the same binary.
5. Avoid interactive `exo` flows.
6. Avoid network access and GitHub authentication.

## Standard directories

`ScenarioBuilder.apply()` still creates these directories for document fixtures:

- `docs/rfcs/stage-0` through `docs/rfcs/stage-4`
- `docs/agent-context/current`

Canonical fixtures create the project structure through `exo init --defaults`.

## Assertions

Prefer asserting visible labels while using seed result IDs for follow-up operations.

Good:

```typescript
await expect(
  page.getByRole("treeitem", { name: "Run Sidebar Seeded Goal" }),
).toBeVisible();
await runCanonicalSeedCommand(holodeckPath, [
  "phase",
  "update",
  seeded.phases.active.id,
  "--title",
  "Updated Phase",
]);
```

Bad:

```typescript
await fs.writeFile("docs/agent-context/plan.toml", generatedPlan);
```

## Debugging tips

1. **No project plan found**: the test used document fixtures where canonical state was required, or the extension daemon is reading a different `exo` binary/state root.
2. **SQLite open/lock races**: use canonical seed helpers; they retry transient startup races.
3. **Stale tree data**: use the machine-channel restart path before opening daemon-backed views.
4. **Webview console errors**: treat as failures and fix the root cause.
5. **Legacy TOML helpers**: keep them only for document-rendering and recovery tests.
