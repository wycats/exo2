<!-- exo:84 ulid:01kg5m2xpz5vh6e662z3bf0kf5 -->

# RFC 84: Pluggable Upgrade System with Protocol Versioning


# RFC 0084: Pluggable Upgrade System with Protocol Versioning

- **Stage**: 0 (Idea)
- **Created**: 2026-01-03
- **Author**: Agent (collaborative)
- **Related RFCs**: 10028, 10008, 0045, 0057, 10033, 10034

## Summary

Define a trait-based pluggable upgrade system that allows:

1. Modular, testable upgrade plugins
2. Automatic detection of breaking changes
3. Protocol versioning with semantic compatibility
4. `exo update` as the single upgrade orchestrator

## Motivation

### Current State

The current upgrade system in `exo update` (see [command/update.rs](tools/exo/src/command/update.rs)) is:

- **Procedural**: A linear waterfall of hardcoded migration steps
- **Implicit**: No schema versioning metadata to track applied migrations
- **Untestable**: Each migration is tightly coupled to the update function
- **Binary**: The upgrade gate (RFC 0064) is all-or-nothing

### Problems

1. **No Migration Registry**: Adding a new migration requires editing `run_update()` directly
2. **No Idempotency Guarantee**: Some migrations check existence, others don't
3. **No Severity Levels**: Can't distinguish "cosmetic" from "blocking" upgrades
4. **Protocol Versioning Disconnect**: API version is unrelated to schema version
5. **No Breaking Change Detection**: Nothing forces a migration for schema changes

### Prior Art

| System              | Approach                                                                 | Relevance                                           |
| ------------------- | ------------------------------------------------------------------------ | --------------------------------------------------- |
| **Flyway**          | Versioned SQL migrations, tracks applied versions in DB table            | Pattern: version tracking, sequential execution     |
| **refinery** (Rust) | `embed_migrations!` macro, checksums, supports contiguous/non-contiguous | Pattern: embedded migrations, checksum verification |
| **schemer** (Rust)  | DAG-based dependencies between migrations                                | Pattern: dependency resolution (overkill for us)    |
| **barrel** (Rust)   | Schema builder DSL                                                       | Not relevant (SQL-focused)                          |

**Key insight**: Database migration tools track "which migrations have been applied" via a metadata table. We need the equivalent for project files—a **schema version** in `plan.toml` and other canonical files.

## Design

### Core Trait: `UpgradePlugin`

```rust
/// A single upgrade operation that can be applied to a project.
pub trait UpgradePlugin: Send + Sync {
    /// Unique identifier for this upgrade (e.g., "remove-deprecated-projections-v1")
    fn id(&self) -> &str;

    /// Semantic version when this upgrade was introduced
    fn introduced_in(&self) -> semver::Version;

    /// Minimum schema version this plugin applies to (None = any)
    fn applies_to(&self) -> Option<semver::VersionReq> {
        None
    }

    /// Check if this upgrade is needed (fast, read-only)
    fn is_needed(&self, context: &AgentContext) -> ExoResult<UpgradeStatus>;

    /// Apply the upgrade (must be idempotent)
    fn apply(&self, context: &mut AgentContext) -> ExoResult<UpgradeReport>;

    /// Verify upgrade was successful (for tests)
    fn verify(&self, context: &AgentContext) -> ExoResult<()> {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum UpgradeStatus {
    /// Upgrade not needed (already applied or not applicable)
    NotNeeded,
    /// Upgrade should be applied
    Needed {
        severity: Severity,
        reason: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Migration blocks all operations (upgrade gate triggers)
    Critical,
    /// Migration should be applied but system remains usable
    Warning,
    /// Migration is cosmetic/performance-related
    Info,
}

pub struct UpgradeReport {
    pub plugin_id: String,
    pub applied: bool,
    pub changes: Vec<String>,
    pub warnings: Vec<String>,
}
```

### Schema Version Metadata

All canonical TOML files that have structured schemas require version tracking:

| File                       | Purpose                        | Example Path                                          |
| -------------------------- | ------------------------------ | ----------------------------------------------------- |
| `plan.toml`                | Project epochs/phases/tasks    | `docs/agent-context/plan.toml`                        |
| `implementation-plan.toml` | Current phase execution state  | `docs/agent-context/current/implementation-plan.toml` |
| `ideas.toml`               | Backlog/brainstorming items    | `docs/agent-context/ideas.toml`                       |
| `axioms.workflow.toml`     | Workflow axioms                | `docs/agent-context/axioms.workflow.toml`             |
| `axioms.system.toml`       | System axioms                  | `docs/agent-context/axioms.system.toml`               |
| `axioms.design.toml`       | Design axioms                  | `docs/design/axioms.design.toml`                      |
| `council.toml`             | Steering council configuration | `docs/agent-context/council.toml`                     |
| `modes.toml`               | Agent mode definitions         | `docs/agent-context/modes.toml`                       |
| `decisions.toml`           | Decision records               | `docs/agent-context/decisions.toml`                   |
| `prompts.toml`             | Prompt configuration           | `docs/agent-context/prompts.toml`                     |

**Note**: `exosuit.toml` is user-configurable and NOT a CLI-managed artifact, so it doesn't require `[meta]` versioning.

Each file gets a `[meta]` section:

```toml
[meta]
schema_version = "1.0.0"  # Semantic version of this file's schema
exo_version = "0.3.1"     # Version of `exo` that last wrote this file
```

**Version semantics:**

- **Major bump**: Breaking change (fields removed, renamed, or semantics changed)
- **Minor bump**: Additive change (new optional fields)
- **Patch bump**: Documentation/comment changes only

### Upgrade Registry

```rust
pub struct UpgradeRegistry {
    plugins: Vec<Box<dyn UpgradePlugin>>,
}

impl UpgradeRegistry {
    pub fn new() -> Self {
        Self {
            plugins: vec![
                // Listed in execution order (dependencies flow downward)
                // Phase 1: Prompt and template updates
                Box::new(UpdatePromptsPlugin),
                Box::new(RefreshAgentsMdPlugin),
                // Phase 2: Axiom migrations
                Box::new(EnsureScopedAxiomsPlugin),
                Box::new(ArchiveLegacyAxiomsPlugin),
                Box::new(EnsureCoreAxiomsPlugin),
                // Phase 3: File migrations
                Box::new(MigrateToolPresentationPlugin),
                Box::new(EnsureImplementationPlanPlugin),
                // Phase 4: Cleanup
                Box::new(RemoveDeprecatedProjectionsPlugin),
                Box::new(EnforceReadOnlyInvariantsPlugin),
                Box::new(RepairRfcPermissionsPlugin),
                // Phase 5: Schema updates
                Box::new(MigratePlanIdsPlugin),
                Box::new(AddSchemaVersionsPlugin),
            ],
        }
    }

    /// Check which upgrades are needed, in severity order
    pub fn check_all(&self, context: &AgentContext) -> ExoResult<UpgradeCheck> {
        let mut critical = Vec::new();
        let mut warning = Vec::new();
        let mut info = Vec::new();

        for plugin in &self.plugins {
            match plugin.is_needed(context)? {
                UpgradeStatus::NotNeeded => {}
                UpgradeStatus::Needed { severity, reason } => {
                    let item = UpgradeNeeded {
                        plugin_id: plugin.id().to_string(),
                        reason,
                    };
                    match severity {
                        Severity::Critical => critical.push(item),
                        Severity::Warning => warning.push(item),
                        Severity::Info => info.push(item),
                    }
                }
            }
        }

        Ok(UpgradeCheck { critical, warning, info })
    }

    /// Apply all needed upgrades
    pub fn apply_all(&self, context: &mut AgentContext) -> ExoResult<UpgradeSummary> {
        let mut summary = UpgradeSummary::new();

        for plugin in &self.plugins {
            if matches!(plugin.is_needed(context)?, UpgradeStatus::Needed { .. }) {
                let report = plugin.apply(context)?;
                plugin.verify(context)?;
                summary.reports.push(report);
            }
        }

        // Update schema version in plan.toml
        summary.new_schema_version = update_schema_version(context)?;

        Ok(summary)
    }
}
```

### Integration with Upgrade Gate

The state machine's `check_upgrade_gate()` will use the registry:

```rust
pub fn check_upgrade_gate(context: &AgentContext) -> ExoResult<()> {
    let registry = UpgradeRegistry::new();
    let check = registry.check_all(context)?;

    if !check.critical.is_empty() {
        let reasons = check.critical.iter()
            .map(|u| format!("  - {}", u.reason))
            .collect::<Vec<_>>()
            .join("\n");

        return Err(ExoFailure::new(
            ErrorCode::PreconditionFailed,
            format!("Project requires upgrade:\n{reasons}\n\nRun `exo update` to apply."),
        )
        .with_steering(Steering {
            primary_intent: "upgrade-required".to_string(),
            confidence: 1.0, // Maximum confidence - this is not optional
            next_actions: vec![
                SteeringAction {
                    tool: "run_in_terminal".to_string(),
                    args: json!({"command": "exo update", "explanation": "Apply required project upgrades"}),
                    priority: Priority::Blocking, // Agent MUST do this first
                    reason: "Project files require migration before any other operations".to_string(),
                }
            ],
            repair_actions: vec![], // No alternative path
        })
        .into());
    }

    Ok(())
}
```

### Steering Philosophy

The upgrade system is designed to be **aggressive** about steering agents toward running upgrades:

1. **Upgrades are a narrow conduit**: By automating migrations reliably, we avoid the complexity of maintaining backward compatibility throughout the codebase. The effort goes into the migration plugins instead.

2. **Blocking priority**: When `Severity::Critical` upgrades are detected, the steering returns `Priority::Blocking`, meaning the agent cannot proceed with any other action until the upgrade completes.

3. **No fallback path**: The `repair_actions` array is empty because there is no alternative. The agent must run the upgrade.

4. **High confidence**: The steering returns `confidence: 1.0` because upgrade detection is deterministic—if the registry says an upgrade is needed, it is needed.

### Integration with `exo-map`

The `exo-map` tool checks for pending upgrades as its first priority:

```rust
pub fn build_steering(context: &AgentContext) -> SteeringResult {
    let registry = UpgradeRegistry::new();
    let check = registry.check_all(context)?;

    // Upgrades take absolute priority
    if !check.critical.is_empty() {
        return SteeringResult {
            primary_intent: "upgrade-required",
            confidence: 1.0,
            next_actions: vec![
                SteeringAction {
                    tool: "run_in_terminal",
                    args: json!({"command": "exo update"}),
                    priority: Priority::Blocking,
                    reason: format!(
                        "Project requires {} critical upgrade(s) before any work can proceed",
                        check.critical.len()
                    ),
                }
            ],
            context_note: Some(
                "The project's schema has changed. All operations are blocked until \
                 `exo update` is run. This is expected after updating `exo` or \
                 checking out a branch with schema changes.".to_string()
            ),
        };
    }

    // Warning-level upgrades are surfaced but don't block
    if !check.warning.is_empty() {
        // Add as secondary action, don't override primary intent
    }

    // Normal steering logic continues...
}
```

### Protocol Versioning

Extend `protocol.rs` with semantic versioning:

```rust
use semver::Version;

pub const PROTOCOL_VERSION: Version = Version::new(1, 0, 0);

impl Version {
    /// Check if client version is compatible with server
    pub fn is_compatible(&self, client: &Version) -> bool {
        // Same major version required
        self.major == client.major
        // Client minor must be <= server minor
        && client.minor <= self.minor
    }
}
```

### Breaking Change Detection (CI)

Add a workflow that detects schema changes:

```yaml
# .github/workflows/breaking-changes.yml
name: Breaking Change Detection

on: [pull_request]

jobs:
  check-schemas:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 2

      - name: Check for schema changes
        run: |
          SCHEMA_FILES=(
            "tools/exo/src/context.rs"
            "tools/exo/src/plan.rs"
            "tools/exo/src/api/protocol.rs"
          )

          for file in "${SCHEMA_FILES[@]}"; do
            if git diff HEAD^ HEAD -- "$file" | grep -q '^[+-].*struct\|^[+-].*enum'; then
              echo "::error::Schema change detected in $file"
              echo "Please ensure a migration plugin exists for this change"
              exit 1
            fi
          done
```

### Testing Strategy

#### Unit Tests (Per-Plugin)

```rust
#[test]
fn test_remove_deprecated_projections_idempotent() {
    let temp = TempDir::new().unwrap();
    setup_project_with_deprecated_files(&temp);
    let mut context = AgentContext::load(temp.path()).unwrap();

    let plugin = RemoveDeprecatedProjections;

    // First run: should remove
    assert!(matches!(plugin.is_needed(&context), Ok(UpgradeStatus::Needed { .. })));
    let report = plugin.apply(&mut context).unwrap();
    assert!(report.applied);

    // Second run: should be no-op
    assert!(matches!(plugin.is_needed(&context), Ok(UpgradeStatus::NotNeeded)));
}
```

#### Integration Tests

```rust
#[test]
fn test_upgrade_from_schema_v1_to_v2() {
    let temp = load_fixture("fixtures/project-schema-v1");
    let mut context = AgentContext::load(temp.path()).unwrap();

    let registry = UpgradeRegistry::new();
    let summary = registry.apply_all(&mut context).unwrap();

    // Verify schema version updated
    let plan = reload_plan(temp.path());
    assert_eq!(plan.meta.schema_version, semver::Version::new(2, 0, 0));
}
```

## Migration from Current System

### Module Structure

```
tools/exo/src/upgrade/
  mod.rs              # UpgradePlugin trait, UpgradeStatus, Severity, UpgradeRegistry
  plugins/
    mod.rs            # Plugin re-exports
    update_prompts.rs
    refresh_agents_md.rs
    ensure_scoped_axioms.rs
    archive_legacy_axioms.rs
    ensure_core_axioms.rs
    migrate_tool_presentation.rs
    ensure_implementation_plan.rs
    remove_deprecated_projections.rs
    enforce_read_only_invariants.rs
    repair_rfc_permissions.rs
    migrate_plan_ids.rs
    add_schema_versions.rs
```

### Severity Assignments

Based on current `run_update()` behavior and blocking requirements:

| Plugin                              | Severity     | Rationale                                            |
| ----------------------------------- | ------------ | ---------------------------------------------------- |
| `RemoveDeprecatedProjectionsPlugin` | **Critical** | Blocks all mutations (current upgrade gate behavior) |
| `EnsureScopedAxiomsPlugin`          | **Warning**  | Data migration, should complete but not blocking     |
| `MigratePlanIdsPlugin`              | **Warning**  | Nice to have but doesn't break functionality         |
| `EnsureImplementationPlanPlugin`    | **Warning**  | Auto-created if missing                              |
| `AddSchemaVersionsPlugin`           | **Warning**  | Needed for future upgrades                           |
| `UpdatePromptsPlugin`               | **Info**     | Template sync, cosmetic                              |
| `RefreshAgentsMdPlugin`             | **Info**     | Template sync, cosmetic                              |
| `ArchiveLegacyAxiomsPlugin`         | **Info**     | Cleanup after migration                              |
| `MigrateToolPresentationPlugin`     | **Info**     | Config migration                                     |
| `EnforceReadOnlyInvariantsPlugin`   | **Info**     | Protection fix                                       |
| `RepairRfcPermissionsPlugin`        | **Info**     | Cosmetic fix                                         |
| `EnsureCoreAxiomsPlugin`            | **Info**     | Creates missing files                                |

### Implementation Phases

#### Phase 4A: Foundation (Tasks 1-2)

1. Create `UpgradePlugin` trait in `tools/exo/src/upgrade/mod.rs`
2. Define `UpgradeStatus`, `Severity`, `UpgradeReport` types
3. Implement `UpgradeRegistry` with `check_all()` and `apply_all()`
4. No dependencies on existing code—pure new module

#### Phase 4B: Plugin Extraction (Tasks 3-4)

1. Create `tools/exo/src/upgrade/plugins/` directory
2. Convert each step from `run_update()` to a plugin (12 plugins total)
3. Register all plugins in `UpgradeRegistry::new()`
4. Update `run_update()` to call `registry.apply_all()`
5. Verify identical behavior with integration tests

#### Phase 4C: Schema Versioning (Task 5)

1. Update schema types in `context.rs` to include optional `Meta` field
2. Create `AddSchemaVersionsPlugin` to add `[meta]` to existing files
3. Update `run_update()` to bump `exo_version` after successful run

#### Phase 4D: Enhanced Gate (Tasks 6-7)

1. Modify `check_upgrade_gate()` to use `UpgradeRegistry::check_all()`
2. Only block on `Severity::Critical`
3. Update `exo-map` to return `Priority::Blocking` for critical upgrades

## Related RFCs

### Must Reference

- **RFC 0064**: Phase State Machine & Projections - Defines upgrade gate concept
- **RFC 0138**: The Standard Bootstrap - Defines `exo update` command
- **RFC 0057/10033**: ULID Identifiers - First major migration using this system

### Potentially Superseded

- **RFC 0064 §5.2**: The ad-hoc `detect_deprecated_projections()` function would be replaced by a proper plugin

### Should Inform

- **RFC 0026**: Release Lifecycle - Higher-level deprecation policy that plugins implement

## Open Questions

1. **Should plugins declare dependencies?** schemer uses DAG, but our migrations are simpler. _Leaning no—sequential ordering is sufficient._
2. **How to handle failed migrations?** Rollback vs. leave in partial state. _Leaning toward atomic per-plugin with clear error reporting._
3. **Should we support `exo upgrade --only <plugin-id>`?** For debugging/testing. _Yes, useful for development._

## Design Discussion (from review)

### Why aggressive steering?

The upgrade system is intentionally aggressive about blocking operations until upgrades complete. This is a deliberate tradeoff:

- **Cost**: Agent cannot do anything until upgrade runs
- **Benefit**: We avoid maintaining backward compatibility throughout the codebase

The philosophy: **put the effort into migration plugins instead of scattered compat code**. This creates a "narrow conduit" where all schema evolution flows through a well-tested, well-documented upgrade path.

### Files requiring `[meta]` versioning

All canonical TOML files with structured schemas need version tracking (see Schema Version Metadata section). This includes `plan.toml`, `implementation-plan.toml`, `ideas.toml`, `axioms.*.toml`, `council.toml`, and `modes.toml`.

## Acceptance Criteria

- [ ] `UpgradePlugin` trait defined with `is_needed`, `apply`, `verify`
- [ ] `UpgradeRegistry` orchestrates all plugins
- [ ] Schema version tracked in `[meta]` section for all canonical TOML files (10 files)
- [ ] `check_upgrade_gate()` uses registry with severity levels
- [ ] `exo-map` returns `Priority::Blocking` steering when critical upgrades pending
- [ ] All existing migrations converted to plugins (12 plugins from current `run_update()`)
- [ ] Unit tests verify idempotency for each plugin
- [ ] Integration test for schema upgrade path

## Appendix: Example Plugins

### RemoveDeprecatedProjections

```rust
struct RemoveDeprecatedProjections;

impl UpgradePlugin for RemoveDeprecatedProjections {
    fn id(&self) -> &str { "remove-deprecated-projections-v1" }
    fn introduced_in(&self) -> semver::Version { semver::Version::new(0, 2, 0) }

    fn is_needed(&self, context: &AgentContext) -> ExoResult<UpgradeStatus> {
        let deprecated = [
            "docs/agent-context/current/task-list.toml",
            "docs/agent-context/current/walkthrough.toml",
        ];

        let found: Vec<_> = deprecated.iter()
            .filter(|p| context.root.join(p).exists())
            .map(|p| *p)
            .collect();

        if found.is_empty() {
            Ok(UpgradeStatus::NotNeeded)
        } else {
            Ok(UpgradeStatus::Needed {
                severity: Severity::Critical,
                reason: format!("Deprecated projection files found: {}", found.join(", ")),
            })
        }
    }

    fn apply(&self, context: &mut AgentContext) -> ExoResult<UpgradeReport> {
        let mut report = UpgradeReport::new(self.id());

        for rel in ["task-list.toml", "walkthrough.toml"] {
            let path = context.root.join("docs/agent-context/current").join(rel);
            if path.exists() {
                std::fs::remove_file(&path)?;
                report.changes.push(format!("Removed {rel}"));
            }
        }

        report.applied = !report.changes.is_empty();
        Ok(report)
    }
}
```

### MigratePlanIds

```rust
struct MigratePlanIds;

impl UpgradePlugin for MigratePlanIds {
    fn id(&self) -> &str { "migrate-plan-ids-v1" }
    fn introduced_in(&self) -> semver::Version { semver::Version::new(0, 3, 0) }

    fn is_needed(&self, context: &AgentContext) -> ExoResult<UpgradeStatus> {
        // Check if any entity lacks a ULID
        let has_unmigrated = context.plan.epochs.iter().any(|e| {
            e.ulid.is_none() || e.phases.iter().any(|p| {
                p.ulid.is_none() || p.tasks.iter().any(|t| t.ulid.is_none())
            })
        });

        if has_unmigrated {
            Ok(UpgradeStatus::Needed {
                severity: Severity::Info,
                reason: "Some plan entities lack ULID identifiers".to_string(),
            })
        } else {
            Ok(UpgradeStatus::NotNeeded)
        }
    }

    fn apply(&self, context: &mut AgentContext) -> ExoResult<UpgradeReport> {
        let json = crate::plan::build_migrate_ids_json(&context.root, true)?;
        let migration: crate::plan::MigrationReport = serde_json::from_value(json)?;

        Ok(UpgradeReport {
            plugin_id: self.id().to_string(),
            applied: migration.applied,
            changes: vec![
                format!("{} epochs migrated", migration.epochs.len()),
                format!("{} phases migrated", migration.phases.len()),
                format!("{} tasks migrated", migration.tasks.len()),
            ],
            warnings: Vec::new(),
        })
    }
}
```
