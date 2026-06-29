//! Pluggable upgrade system for Exosuit projects.
//!
//! This module implements RFC 0084: Pluggable Upgrade System with Protocol Versioning.
//!
//! # Architecture
//!
//! The upgrade system is built around three core concepts:
//!
//! 1. **`UpgradePlugin`**: A trait representing a single, idempotent migration operation
//! 2. **`UpgradeRegistry`**: An orchestrator that manages and executes plugins in order
//! 3. **Severity**: Classification of upgrade urgency (Critical, Warning, Info)
//!
//! # Usage
//!
//! ```ignore
//! let registry = UpgradeRegistry::new();
//! let check = registry.check_all(&context)?;
//!
//! if !check.critical.is_empty() {
//!     // Must run `exo update` before proceeding
//! }
//!
//! let summary = registry.apply_all(&mut context)?;
//! ```

// Allow &str in trait signatures for flexibility (implementations return 'static anyway)
#![allow(clippy::unnecessary_literal_bound)]

pub mod plugins;

use crate::ExoResult;
use crate::context::AgentContext;
use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// Core Types
// ─────────────────────────────────────────────────────────────────────────────

/// Severity level for an upgrade operation.
///
/// Determines whether the upgrade blocks other operations or is merely advisory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    /// Migration blocks all operations (upgrade gate triggers).
    /// The agent MUST run `exo update` before any other action.
    Critical,

    /// Migration should be applied but system remains usable.
    /// Logged as a warning but doesn't block operations.
    Warning,

    /// Migration is cosmetic or performance-related.
    /// Silent unless verbose mode is enabled.
    Info,
}

impl Severity {
    /// Returns true if this severity level blocks operations.
    #[must_use]
    pub const fn is_blocking(&self) -> bool {
        matches!(self, Self::Critical)
    }

    /// Human-readable label for display.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Critical => "critical",
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }
}

/// Result of checking whether an upgrade is needed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpgradeStatus {
    /// Upgrade not needed (already applied or not applicable).
    NotNeeded,

    /// Upgrade should be applied.
    Needed {
        /// How urgent is this upgrade?
        severity: Severity,
        /// Human-readable explanation of why the upgrade is needed.
        reason: String,
    },
}

impl UpgradeStatus {
    /// Create a "needed" status with the given severity and reason.
    #[must_use]
    pub fn needed(severity: Severity, reason: impl Into<String>) -> Self {
        Self::Needed {
            severity,
            reason: reason.into(),
        }
    }

    /// Create a critical upgrade status.
    #[must_use]
    pub fn critical(reason: impl Into<String>) -> Self {
        Self::needed(Severity::Critical, reason)
    }

    /// Create a warning upgrade status.
    #[must_use]
    pub fn warning(reason: impl Into<String>) -> Self {
        Self::needed(Severity::Warning, reason)
    }

    /// Create an info upgrade status.
    #[must_use]
    pub fn info(reason: impl Into<String>) -> Self {
        Self::needed(Severity::Info, reason)
    }

    /// Returns true if upgrade is needed.
    #[must_use]
    pub const fn is_needed(&self) -> bool {
        matches!(self, Self::Needed { .. })
    }

    /// Returns the severity if needed, None otherwise.
    #[must_use]
    pub const fn severity(&self) -> Option<Severity> {
        match self {
            Self::NotNeeded => None,
            Self::Needed { severity, .. } => Some(*severity),
        }
    }
}

/// Report from applying a single upgrade plugin.
#[derive(Debug, Clone, Serialize)]
pub struct UpgradeReport {
    /// Unique identifier of the plugin that was applied.
    pub plugin_id: String,

    /// Whether any changes were made.
    pub applied: bool,

    /// List of changes that were made (for logging/display).
    pub changes: Vec<String>,

    /// Non-fatal warnings encountered during application.
    pub warnings: Vec<String>,
}

impl UpgradeReport {
    /// Create a new report indicating no changes were needed.
    #[must_use]
    pub fn no_changes(plugin_id: impl Into<String>) -> Self {
        Self {
            plugin_id: plugin_id.into(),
            applied: false,
            changes: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// Create a new report indicating changes were applied.
    #[must_use]
    pub fn with_changes(plugin_id: impl Into<String>, changes: Vec<String>) -> Self {
        Self {
            plugin_id: plugin_id.into(),
            applied: true,
            changes,
            warnings: Vec::new(),
        }
    }

    /// Add a warning to the report.
    #[must_use]
    pub fn with_warning(mut self, warning: impl Into<String>) -> Self {
        self.warnings.push(warning.into());
        self
    }
}

/// Information about a single needed upgrade.
#[derive(Debug, Clone, Serialize)]
pub struct UpgradeNeeded {
    /// Plugin identifier.
    pub plugin_id: String,

    /// Why this upgrade is needed.
    pub reason: String,
}

/// Result of checking all upgrades.
#[derive(Debug, Clone, Default, Serialize)]
pub struct UpgradeCheck {
    /// Critical upgrades that block operations.
    pub critical: Vec<UpgradeNeeded>,

    /// Warning-level upgrades that should be applied.
    pub warning: Vec<UpgradeNeeded>,

    /// Info-level upgrades (cosmetic).
    pub info: Vec<UpgradeNeeded>,
}

impl UpgradeCheck {
    /// Returns true if any upgrades are needed at any severity level.
    #[must_use]
    pub const fn any_needed(&self) -> bool {
        !self.critical.is_empty() || !self.warning.is_empty() || !self.info.is_empty()
    }

    /// Returns true if any blocking (critical) upgrades are needed.
    #[must_use]
    pub const fn has_blocking(&self) -> bool {
        !self.critical.is_empty()
    }

    /// Total count of needed upgrades across all severities.
    #[must_use]
    pub const fn total_count(&self) -> usize {
        self.critical.len() + self.warning.len() + self.info.len()
    }
}

/// Summary of all upgrades applied.
#[derive(Debug, Clone, Default, Serialize)]
pub struct UpgradeSummary {
    /// Reports from each plugin that was executed.
    pub reports: Vec<UpgradeReport>,

    /// Total number of plugins that made changes.
    pub applied_count: usize,

    /// Total number of plugins that were skipped (already up-to-date).
    pub skipped_count: usize,
}

impl UpgradeSummary {
    /// Create a new empty summary.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a report to the summary.
    pub fn add_report(&mut self, report: UpgradeReport) {
        if report.applied {
            self.applied_count += 1;
        } else {
            self.skipped_count += 1;
        }
        self.reports.push(report);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// UpgradePlugin Trait
// ─────────────────────────────────────────────────────────────────────────────

/// A single upgrade operation that can be applied to a project.
///
/// Plugins must be:
/// - **Idempotent**: Running twice should have the same effect as running once
/// - **Self-contained**: No dependencies on other plugins' runtime state
/// - **Deterministic**: `is_needed()` must accurately reflect whether `apply()` will change anything
///
/// # Example Implementation
///
/// ```ignore
/// pub struct RemoveDeprecatedProjectionsPlugin;
///
/// impl UpgradePlugin for RemoveDeprecatedProjectionsPlugin {
///     fn id(&self) -> &str {
///         "remove-deprecated-projections-v1"
///     }
///
///     fn is_needed(&self, context: &AgentContext) -> ExoResult<UpgradeStatus> {
///         let deprecated_projection = context.root.join(".exo/legacy/task-list.toml");
///         if deprecated_projection.exists() {
///             Ok(UpgradeStatus::critical("Deprecated projection found"))
///         } else {
///             Ok(UpgradeStatus::NotNeeded)
///         }
///     }
///
///     fn apply(&self, context: &mut AgentContext) -> ExoResult<UpgradeReport> {
///         // ... implementation
///     }
/// }
/// ```
pub trait UpgradePlugin: Send + Sync {
    /// Unique identifier for this upgrade.
    ///
    /// Convention: `{operation}-v{version}` (e.g., "remove-deprecated-projections-v1")
    fn id(&self) -> &str;

    /// Human-readable description of what this plugin does.
    fn description(&self) -> &str {
        "No description provided"
    }

    /// Check if this upgrade is needed (fast, read-only).
    ///
    /// This method should be cheap to call and must not modify any state.
    /// It's called during `exo-steering` steering and upgrade gate checks.
    fn is_needed(&self, context: &AgentContext) -> ExoResult<UpgradeStatus>;

    /// Apply the upgrade (must be idempotent).
    ///
    /// This method may modify files in the project. It should:
    /// - Use `edit_cli_managed_file()` for TOML modifications
    /// - Handle missing files gracefully
    /// - Return detailed change information in the report
    ///
    /// The `context` is passed as `&mut` to signal permission to modify project state,
    /// though most mutations are file I/O rather than in-memory changes.
    fn apply(&self, context: &mut AgentContext) -> ExoResult<UpgradeReport>;

    /// Verify upgrade was successful (optional, for tests).
    ///
    /// Called after `apply()` to validate post-conditions.
    /// Default implementation does nothing.
    fn verify(&self, _context: &AgentContext) -> ExoResult<()> {
        Ok(())
    }

    /// The severity level of this upgrade.
    ///
    /// Override this to change the default severity.
    /// Most plugins should return `Severity::Info` or `Severity::Warning`.
    fn severity(&self) -> Severity {
        Severity::Info
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// UpgradeRegistry
// ─────────────────────────────────────────────────────────────────────────────

/// Registry that manages and orchestrates upgrade plugins.
///
/// Plugins are registered in a specific order that respects dependencies.
/// The registry provides methods to check which upgrades are needed and
/// apply them in sequence.
pub struct UpgradeRegistry {
    plugins: Vec<Box<dyn UpgradePlugin>>,
}

impl std::fmt::Debug for UpgradeRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UpgradeRegistry")
            .field("plugin_count", &self.plugins.len())
            .field("plugin_ids", &self.plugin_ids())
            .finish()
    }
}

impl Default for UpgradeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl UpgradeRegistry {
    /// Create a new registry with all known upgrade plugins.
    ///
    /// Plugins are listed in execution order. Dependencies flow downward:
    /// earlier plugins may create state that later plugins depend on.
    #[must_use]
    pub fn new() -> Self {
        Self {
            plugins: vec![
                // Phase 1: Prompt and template updates
                Box::new(plugins::UpdatePromptsPlugin),
                Box::new(plugins::RefreshAgentsMdPlugin),
                Box::new(plugins::InstallGlobalPromptsPlugin),
                //
                // Phase 2: Axiom migrations
                Box::new(plugins::EnsureScopedAxiomsPlugin),
                Box::new(plugins::EnsureCoreAxiomsPlugin),
                //
                // Phase 3: File migrations
                Box::new(plugins::MigrateToolPresentationPlugin),
                Box::new(plugins::RepairRfcPermissionsPlugin),
                Box::new(plugins::MigrateRfcMetadataPlugin),
                Box::new(plugins::MigrateLegacyPlanPlugin),
                //
                // Phase 8: Bootstrap scaffolding (RFC 0138)
                Box::new(plugins::EnsureGitattributesPlugin),
                Box::new(plugins::EnsureGitignorePlugin),
            ],
        }
    }

    /// Check which upgrades are needed, grouped by severity.
    ///
    /// This is a read-only operation suitable for use in upgrade gates
    /// and `exo-steering` steering.
    pub fn check_all(&self, context: &AgentContext) -> ExoResult<UpgradeCheck> {
        let mut check = UpgradeCheck::default();

        for plugin in &self.plugins {
            match plugin.is_needed(context)? {
                UpgradeStatus::NotNeeded => {}
                UpgradeStatus::Needed { severity, reason } => {
                    let item = UpgradeNeeded {
                        plugin_id: plugin.id().to_string(),
                        reason,
                    };
                    match severity {
                        Severity::Critical => check.critical.push(item),
                        Severity::Warning => check.warning.push(item),
                        Severity::Info => check.info.push(item),
                    }
                }
            }
        }

        Ok(check)
    }

    /// Apply all needed upgrades in order.
    ///
    /// Stops on the first error. Each plugin is verified after application.
    pub fn apply_all(&self, context: &mut AgentContext) -> ExoResult<UpgradeSummary> {
        let mut summary = UpgradeSummary::new();

        for plugin in &self.plugins {
            let status = plugin.is_needed(context)?;

            if status.is_needed() {
                let report = plugin.apply(context)?;
                plugin.verify(context)?;
                summary.add_report(report);
            } else {
                // Track skipped plugins for completeness
                summary.add_report(UpgradeReport::no_changes(plugin.id()));
            }
        }

        Ok(summary)
    }

    /// Get a list of all registered plugin IDs (for debugging/testing).
    #[must_use]
    pub fn plugin_ids(&self) -> Vec<&str> {
        self.plugins.iter().map(|p| p.id()).collect()
    }

    /// Number of registered plugins.
    #[must_use]
    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::SQLITE_DB_PATH;
    use crate::context::SqliteWriter;
    use std::fs;

    fn setup_upgrade_test_root(root: &std::path::Path) {
        fs::create_dir_all(root.join("docs/agent-context")).unwrap();
        fs::create_dir_all(root.join("docs/rfcs")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();

        let db_path = root.join(SQLITE_DB_PATH);
        SqliteWriter::open(&db_path).unwrap();
    }

    #[allow(dead_code)] // Scaffolding for future tests
    struct TestPlugin {
        id: String,
        needed: bool,
        severity: Severity,
    }

    #[allow(dead_code)]
    impl UpgradePlugin for TestPlugin {
        fn id(&self) -> &str {
            &self.id
        }

        fn is_needed(&self, _context: &AgentContext) -> ExoResult<UpgradeStatus> {
            if self.needed {
                Ok(UpgradeStatus::needed(self.severity, "test reason"))
            } else {
                Ok(UpgradeStatus::NotNeeded)
            }
        }

        fn apply(&self, _context: &mut AgentContext) -> ExoResult<UpgradeReport> {
            Ok(UpgradeReport::with_changes(
                self.id.clone(),
                vec!["test change".to_string()],
            ))
        }

        fn severity(&self) -> Severity {
            self.severity
        }
    }

    #[test]
    fn test_upgrade_status_constructors() {
        let critical = UpgradeStatus::critical("test");
        assert!(critical.is_needed());
        assert_eq!(critical.severity(), Some(Severity::Critical));

        let warning = UpgradeStatus::warning("test");
        assert!(warning.is_needed());
        assert_eq!(warning.severity(), Some(Severity::Warning));

        let info = UpgradeStatus::info("test");
        assert!(info.is_needed());
        assert_eq!(info.severity(), Some(Severity::Info));

        let not_needed = UpgradeStatus::NotNeeded;
        assert!(!not_needed.is_needed());
        assert_eq!(not_needed.severity(), None);
    }

    #[test]
    fn test_severity_is_blocking() {
        assert!(Severity::Critical.is_blocking());
        assert!(!Severity::Warning.is_blocking());
        assert!(!Severity::Info.is_blocking());
    }

    #[test]
    fn test_upgrade_check_aggregation() {
        let mut check = UpgradeCheck::default();
        assert!(!check.any_needed());
        assert!(!check.has_blocking());
        assert_eq!(check.total_count(), 0);

        check.warning.push(UpgradeNeeded {
            plugin_id: "test".to_string(),
            reason: "test".to_string(),
        });
        assert!(check.any_needed());
        assert!(!check.has_blocking());
        assert_eq!(check.total_count(), 1);

        check.critical.push(UpgradeNeeded {
            plugin_id: "test2".to_string(),
            reason: "test2".to_string(),
        });
        assert!(check.has_blocking());
        assert_eq!(check.total_count(), 2);
    }

    #[test]
    fn test_upgrade_summary_tracking() {
        let mut summary = UpgradeSummary::new();
        assert_eq!(summary.applied_count, 0);
        assert_eq!(summary.skipped_count, 0);

        summary.add_report(UpgradeReport::with_changes(
            "p1",
            vec!["change".to_string()],
        ));
        assert_eq!(summary.applied_count, 1);
        assert_eq!(summary.skipped_count, 0);

        summary.add_report(UpgradeReport::no_changes("p2"));
        assert_eq!(summary.applied_count, 1);
        assert_eq!(summary.skipped_count, 1);
    }

    #[test]
    fn test_upgrade_report_builder() {
        let report = UpgradeReport::with_changes("test", vec!["c1".to_string()]).with_warning("w1");

        assert!(report.applied);
        assert_eq!(report.changes.len(), 1);
        assert_eq!(report.warnings.len(), 1);
    }

    #[test]
    fn test_registry_has_expected_plugins() {
        let registry = UpgradeRegistry::new();

        // Verify specific plugin IDs are present
        let expected_ids = [
            "update-prompts-v1",
            "refresh-agents-md-v1",
            "install-global-prompts-v1",
            "ensure-scoped-axioms-v1",
            "ensure-core-axioms-v1",
            "migrate-tool-presentation-v1",
            "repair-rfc-permissions-v1",
            "migrate-rfc-metadata-v1",
            "migrate-legacy-plan-v1",
            "ensure-gitattributes-v1",
            "ensure-gitignore-v1",
        ];
        let ids = registry.plugin_ids();
        assert_eq!(registry.plugin_count(), expected_ids.len());
        for expected_id in expected_ids {
            assert!(ids.contains(&expected_id));
        }
    }

    #[test]
    fn test_registry_check_all_empty_context() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path().to_path_buf();
        setup_upgrade_test_root(&root);
        let context = AgentContext::new_for_testing(root);

        let registry = UpgradeRegistry::new();
        let check = registry.check_all(&context).unwrap();

        // In an empty context, some plugins will report as needed
        // (e.g., EnsureScopedAxiomsPlugin for missing axiom files)
        // but none should fail
        let _ = check.total_count(); // Just verify it doesn't panic
    }

    #[test]
    fn test_registry_apply_all_creates_minimal_project_structure() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path().to_path_buf();

        setup_upgrade_test_root(&root);

        fs::write(root.join("docs/agent-context/axioms.sql"), "-- dump\n").unwrap();

        let mut context = AgentContext::new_for_testing(root.clone());
        let registry = UpgradeRegistry::new();

        // Apply all upgrades - should not fail
        let summary = registry.apply_all(&mut context).unwrap();

        // Should have processed all plugins
        assert_eq!(summary.reports.len(), registry.plugin_count());

        // Verify current axiom migration artifacts remain available
        assert!(root.join("docs/agent-context/axioms.sql").exists());
        assert!(root.join(SQLITE_DB_PATH).exists());
    }

    #[test]
    fn test_registry_apply_all_is_idempotent() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path().to_path_buf();

        setup_upgrade_test_root(&root);
        fs::write(root.join("docs/agent-context/axioms.sql"), "-- dump\n").unwrap();

        let mut context = AgentContext::new_for_testing(root);
        let registry = UpgradeRegistry::new();

        // Apply once
        let summary1 = registry.apply_all(&mut context).unwrap();

        // Apply again - should be mostly no-ops
        let summary2 = registry.apply_all(&mut context).unwrap();

        // Second run should have fewer or equal applied plugins
        assert!(summary2.applied_count <= summary1.applied_count);
    }

    #[test]
    fn test_registry_detects_critical_upgrades() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path().to_path_buf();

        // Create structure with missing required bootstrap files (triggers upgrades)
        setup_upgrade_test_root(&root);

        let context = AgentContext::new_for_testing(root);
        let registry = UpgradeRegistry::new();
        let check = registry.check_all(&context).unwrap();

        // Should detect needed upgrades in an uninitialized project
        assert!(check.any_needed());
        assert!(!check.info.is_empty() || !check.warning.is_empty());

        let needed_ids: Vec<_> = check
            .info
            .iter()
            .chain(check.warning.iter())
            .chain(check.critical.iter())
            .map(|u| u.plugin_id.as_str())
            .collect();
        assert!(needed_ids.contains(&"ensure-gitignore-v1"));
    }
}
