//! Upgrade plugins for Exosuit project migrations.
//!
//! Each plugin is a self-contained, idempotent migration operation.
//! Plugins are registered in `UpgradeRegistry::new()` in execution order.

// These plugins are zero-sized types (ZSTs) that implement UpgradePlugin.
// Clippy complains about unused &self but the trait requires it.
#![allow(clippy::unused_self)]
#![allow(clippy::trivially_copy_pass_by_ref)]

mod ensure_core_axioms;
mod ensure_gitattributes;
mod ensure_gitignore;
mod ensure_scoped_axioms;
mod install_global_prompts;
mod migrate_legacy_plan;
mod migrate_rfc_metadata;
mod migrate_tool_presentation;
mod refresh_agents_md;
mod repair_rfc_permissions;
mod update_prompts;

pub use ensure_core_axioms::EnsureCoreAxiomsPlugin;
pub use ensure_gitattributes::EnsureGitattributesPlugin;
pub use ensure_gitignore::EnsureGitignorePlugin;
pub use ensure_scoped_axioms::EnsureScopedAxiomsPlugin;
pub use install_global_prompts::InstallGlobalPromptsPlugin;
pub use migrate_legacy_plan::MigrateLegacyPlanPlugin;
pub use migrate_rfc_metadata::MigrateRfcMetadataPlugin;
pub use migrate_tool_presentation::MigrateToolPresentationPlugin;
pub use refresh_agents_md::RefreshAgentsMdPlugin;
pub use repair_rfc_permissions::RepairRfcPermissionsPlugin;
pub use update_prompts::UpdatePromptsPlugin;
