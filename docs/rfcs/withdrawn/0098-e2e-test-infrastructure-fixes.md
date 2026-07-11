<!-- exo:98 ulid:01kg5kp2fwz4hmvgq8t65d241y -->

# RFC 98: E2E Test Infrastructure Fixes

- **Status**: Withdrawn
- **Stage**: 0
- **Reason**: Withdrawn by RFC 10180 storage disposition: this proposal depends on retired file-backed phase context or direct editing/protection of legacy docs/agent-context current artifacts.

# RFC 0098: E2E Test Infrastructure Fixes

## Summary

This RFC documents the root causes of E2E test failures and provides a shovel-ready implementation plan to fix them. After fixing the Playwright `workers` configuration (commit `a88ca67`), we now have 17 tests that run to completion but 15 fail due to test infrastructure issues, not application bugs.

## Current State

**Test Results (with `workers=1`):**

- ✅ 2 passed: `activation.test.ts`, `extensions.test.ts`
- ❌ 15 failed: Infrastructure/locator issues (documented below)
- ✅ 0 EAGAIN/fork errors (parallelism fix working)

## Root Cause Analysis

### Category A: Holodeck Scaffolding Incomplete

**Severity: Critical (blocks 12+ tests)**

The `ScenarioBuilder` creates files but doesn't create the required directory structure. The extension expects certain directories to exist even if empty.

**Evidence from logs:**

```
Error: File not found: .../exosuit-holodeck-xxx/docs/agent-context/plan.toml
Error: ENOENT: no such file or directory, scandir '.../docs/rfcs/stage-0'
Error: ENOENT: no such file or directory, scandir '.../docs/agent-context/current'
```

**Current `ScenarioBuilder.withPhase()` behavior:**

- Creates `docs/agent-context/plan.toml` ✅
- Does NOT create `docs/rfcs/stage-{0..4}/` directories ❌
- Does NOT create `docs/agent-context/current/` directory ❌
- Does NOT include required `[meta]` section in plan.toml ❌

**File:** `packages/exosuit-vscode/tests/e2e/scenarios.ts`

### Category B: Dashboard Locator Mismatch

**Severity: High (blocks 3-5 tests)**

The `DashboardPage` page object uses outdated locators that don't match the current Dashboard UI.

**Current locators in `exosuit-test.ts`:**

```typescript
get currentPhase() {
  return this.root.getByText("CURRENT PHASE");  // ❌ Text doesn't exist
}

get feedbackSection() {
  return this.root.getByText("FEEDBACK", { exact: true });  // ❌ Section removed/renamed
}

get rfcSection() {
  return this.root.getByText("RFCs", { exact: true });  // ❌ Section removed/renamed
}
```

**Actual Dashboard structure (from `App.svelte`):**

```svelte
<div class="phase-card">
  <div class="phase-title">{phase.title}</div>
  <div class="phase-id">{phase.phaseId}</div>
</div>
```

**File:** `packages/exosuit-vscode/tests/e2e/lib/exosuit-test.ts`

### Category C: LM Tool Registration Warnings

**Severity: Medium (causes test noise, may trigger failure)**

During extension activation in tests, the Dashboard webview loads before all LM tools are registered. This generates 60+ warning messages:

```
Tool "exo-map" was not contributed.
Tool "exo-axiom-list" was not contributed.
... (60+ more)
```

The test framework's webview error detection treats these as failures via `checkForWebviewErrors()`.

**File:** `packages/exosuit-vscode/tests/e2e/fixtures.ts` (error detection logic)

### Category D: Debug/Reactivity Test Assumptions

**Severity: Low (2 tests)**

Tests like `reactivity-flow.test.ts` look for debug UI elements that have been removed:

```typescript
await expect(dashboard.debugRootStatus).toContainText("pending");
```

The `#debug-root-status` element was part of a development-time reactivity debugging feature that's been cleaned up.

**Files:**

- `tests/e2e/reactivity-flow.test.ts`
- `tests/e2e/debug-frames.test.ts`

## Implementation Plan

### Task 1: Fix Holodeck Directory Scaffolding

**File:** `packages/exosuit-vscode/tests/e2e/scenarios.ts`

**Changes to `apply()` method:**

```typescript
async apply() {
  // 1. Create required directory structure BEFORE writing files
  const requiredDirs = [
    'docs/agent-context',
    'docs/agent-context/current',
    'docs/rfcs/stage-0',
    'docs/rfcs/stage-1',
    'docs/rfcs/stage-2',
    'docs/rfcs/stage-3',
    'docs/rfcs/stage-4',
    '.vscode',
  ];

  for (const dir of requiredDirs) {
    await fs.mkdir(path.join(this.rootPath, dir), { recursive: true });
  }

  // 2. Then write files as before...
}
```

**Changes to `withPhase()` method:**

```typescript
withPhase(phaseId: string, title: string, status: string = "active") {
  const plan = {
    meta: {
      schema_version: "1.0.0",
      exo_version: "0.1.0",
    },
    epochs: [
      {
        id: "epoch-1",
        title: "Epoch 1",
        status: "active",
        ulid: "01TEST00000000000000000001",
        slug: "epoch-1",
        aliases: ["epoch-1"],
        phases: [
          {
            id: phaseId,
            title: title,
            status: status,
            ulid: "01TEST00000000000000000002",
            slug: phaseId,
            aliases: [phaseId],
            tasks: [],
          },
        ],
      },
    ],
  };
  this.files.set("docs/agent-context/plan.toml", toml.stringify(plan));
  return this;
}
```

**Add new helper for minimal implementation plan:**

```typescript
withImplementationPlan() {
  const plan = {
    phase: {
      id: "phase-1",
      title: "Test Phase",
      rfcs: [],
    },
    plan: {
      changes: [],
    },
    verification: {
      automated: [],
      manual: [],
    },
  };
  this.files.set(
    "docs/agent-context/current/implementation-plan.toml",
    toml.stringify(plan)
  );
  return this;
}
```

### Task 2: Update Dashboard Page Object Locators

**File:** `packages/exosuit-vscode/tests/e2e/lib/exosuit-test.ts`

**Replace locators to match current UI:**

```typescript
export class DashboardPage extends BasePage {
  constructor(frame: Frame) {
    super(frame);
  }

  /**
   * Locates the phase card - the main indicator that Dashboard loaded successfully.
   * Falls back to empty-state message if no phase is active.
   */
  get phaseIndicator() {
    return this.root.locator(".phase-card, .empty-state, .genesis-card");
  }

  /**
   * Gets the phase title element within the phase card.
   */
  get phaseTitle() {
    return this.root.locator(".phase-title");
  }

  /**
   * Gets the epoch badge element.
   */
  get epochBadge() {
    return this.root.locator(".epoch-badge");
  }

  /**
   * The main app container - basic "did the webview load" check.
   */
  get appRoot() {
    return this.root.locator("#app");
  }

  /**
   * Task list section
   */
  get taskList() {
    return this.root.locator(".task-list, .tasks-section");
  }

  async getRfcItem(id: string) {
    // RFCs may be in a list or chips section
    return this.root.getByText(id);
  }

  /**
   * Verifies the Dashboard has loaded successfully.
   * Checks for the app container and either a phase card or empty state.
   */
  async expectLoaded() {
    // First, verify the webview app itself loaded
    await expect(this.appRoot).toBeVisible({ timeout: 10000 });

    // Then verify either a phase is shown or the empty/genesis state
    await expect(this.phaseIndicator).toBeVisible({ timeout: 20000 });
  }

  /**
   * Verifies a specific phase is displayed.
   */
  async expectPhase(title: string) {
    await expect(this.phaseTitle).toContainText(title, { timeout: 10000 });
  }
}
```

### Task 3: Handle Tool Registration Warnings

**File:** `packages/exosuit-vscode/tests/e2e/fixtures.ts`

**Option A (Recommended): Filter known warnings in error detection**

Locate the `checkForWebviewErrors()` or similar function and add filtering:

```typescript
const EXPECTED_WARNINGS = [
  /Tool "exo-[\w-]+" was not contributed/,
  /Tool "exo-[\w-]+" already has an implementation/,
];

function isExpectedWarning(message: string): boolean {
  return EXPECTED_WARNINGS.some((pattern) => pattern.test(message));
}

// In the error collection logic:
const errors = collectedErrors.filter((e) => !isExpectedWarning(e.message));
if (errors.length > 0) {
  throw new Error("Unexpected Webview errors detected.");
}
```

**Option B: Suppress all webview console errors in tests**

Add to the test fixture setup:

```typescript
page.on("console", (msg) => {
  if (msg.type() === "error" && msg.text().includes("was not contributed")) {
    return; // Suppress tool registration warnings
  }
  // ... existing handling
});
```

### Task 4: Remove or Update Debug Test Dependencies

**Files to update:**

1. **`tests/e2e/reactivity-flow.test.ts`**
   - Remove references to `debugRootStatus` and `debugRootValue`
   - Update to test reactivity via actual UI elements (phase card, task list)

2. **`tests/e2e/debug-frames.test.ts`**
   - Either delete (if purely diagnostic) or update assertions

3. **`tests/e2e/lib/exosuit-test.ts`**
   - Remove dead `debugRootStatus` and `debugRootValue` getters from `DashboardPage`

### Task 5: Update Test Assertions

**File:** `tests/e2e/dashboard-behavior.test.ts`

Update the `beforeEach` to use improved scaffolding:

```typescript
test.beforeEach(async ({ exo }) => {
  await exo.holodeck
    .withAgentsMd()
    .withPhase("phase-1", "Initial Phase")
    .withImplementationPlan() // NEW: Add implementation plan
    .withRfc("stage-0", "0001", "Test RFC")
    .apply();
});
```

Update assertions to use new locators:

```typescript
test("Initial Load: Verifies Dashboard renders", async ({ exo }) => {
  const dashboard = await exo.openDashboard();
  await dashboard.expectLoaded();
  await dashboard.expectPhase("Initial Phase");
});
```

### Task 6: Fix Feature-Specific Tests

After Tasks 1-5 are complete, these tests should be re-evaluated:

| Test File                | Expected Outcome                                |
| ------------------------ | ----------------------------------------------- |
| `notebook.test.ts`       | Should work with proper scaffolding             |
| `rfc-view.test.ts`       | Update locators for Studio view                 |
| `studio.test.ts`         | Update locators for Studio view                 |
| `walkthrough-*.test.ts`  | Add `walkthrough.toml` to scaffolding           |
| `welcome-wizard.test.ts` | Should work (uses empty Holodeck intentionally) |

## Acceptance Criteria

1. All 17 E2E tests pass with `pnpm test:e2e --workers=1`
2. No EAGAIN/fork errors
3. No "Unexpected Webview errors" failures from tool registration warnings
4. Dashboard tests verify actual UI elements, not removed debug elements
5. `ScenarioBuilder` creates complete directory structure

## Test Commands

```bash
# Run single test for quick iteration
cd packages/exosuit-vscode
pnpm test:e2e -- --grep "Initial Load" --workers=1

# Run all Dashboard tests
pnpm test:e2e -- --grep "Dashboard" --workers=1

# Run full suite
pnpm test:e2e --workers=1
```

## Files to Modify

| File                                   | Changes                                                                   |
| -------------------------------------- | ------------------------------------------------------------------------- |
| `tests/e2e/scenarios.ts`               | Add directory creation, fix `withPhase()`, add `withImplementationPlan()` |
| `tests/e2e/lib/exosuit-test.ts`        | Update `DashboardPage` locators, remove debug getters                     |
| `tests/e2e/fixtures.ts`                | Filter tool registration warnings                                         |
| `tests/e2e/dashboard-behavior.test.ts` | Update test scaffolding and assertions                                    |
| `tests/e2e/reactivity-flow.test.ts`    | Remove debug element dependencies                                         |
| `tests/e2e/debug-frames.test.ts`       | Update or delete                                                          |

## Non-Goals

- Adding new test coverage (Phase 3 of E2E epoch)
- CI integration (Phase 4 of E2E epoch)
- Parallelism improvements beyond `workers=1`

