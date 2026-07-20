//! # Terminology: Goals vs Tasks
//!
//! This codebase distinguishes two concepts:
//! - **Goal**: A planning-level item in the SQLite-backed phase state.
//!   Goals represent what needs to be accomplished in a phase.
//! - **Task**: An execution-level item tracked alongside the active phase.
//!   Tasks are concrete implementation steps with TDD status.
//!
//! The `Goal` struct in this file represents persisted goal records.
//! Implementation tasks are managed separately in the `task` module.

pub mod rfc;
pub mod sqlite_loader;
pub mod sqlite_writer;

pub use sqlite_loader::SqliteLoader;
pub use sqlite_writer::SqliteWriter;

use crate::ExoResult;
use crate::project::{Project, StatePolicy};
use crate::ulid_util::{ExoUlid, UlidResolvable};
use anyhow::Context;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const DEPRECATED_SQL_PROJECTION_FILES: &[&str] = &["agent_events.sql"];

/// Normalize legacy status values to canonical forms.
///
/// Mappings:
/// - `"active"` → `"in-progress"` (phase/epoch status)
/// - `"complete"` → `"completed"` (phase/epoch/goal status)
/// - `"bankrupt"` → `"abandoned"` (phase/goal status)
/// - `"aborted"` → `"abandoned"` (goal status)
///
/// All other values pass through unchanged.
fn normalize_status(status: &str) -> &'static str {
    match status {
        "active" => "in-progress",
        "complete" => "completed",
        "bankrupt" => "abandoned",
        "aborted" => "abandoned",
        // Return static strings for common values to avoid allocation
        "pending" => "pending",
        "in-progress" => "in-progress",
        "completed" => "completed",
        "abandoned" => "abandoned",
        "deferred" => "deferred",
        "skipped" => "skipped",
        "red" => "red",
        "green" => "green",
        _ => {
            // For unknown values, we can't return a static str
            // This is a fallback that should rarely be hit
            Box::leak(status.to_string().into_boxed_str())
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Storage Backend
// ─────────────────────────────────────────────────────────────────────────────

/// Relative path from workspace root to the `SQLite` database.
///
/// Per RFC 10177 (Local XDG), `.cache/` is for regenerable artifacts
/// including binary databases. The DB can be regenerated from TOML
/// via `exo migrate to-sqlite`.
pub const SQLITE_DB_PATH: &str = ".cache/exo.db";

#[must_use]
pub fn db_path(root: &Path, project: Option<&Project>) -> PathBuf {
    project.map_or_else(|| root.join(SQLITE_DB_PATH), Project::db_path)
}

#[must_use]
pub fn db_path_resolving_project(root: &Path) -> PathBuf {
    let project = Project::resolve(root).ok();
    db_path(root, project.as_ref())
}

/// Storage backend for loading project state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StorageBackend {
    /// Load from `SQLite` database (.cache/exo.db)
    #[default]
    Sqlite,
}

impl StorageBackend {
    /// Parse a storage backend from a string.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "toml" | "sqlite" | "sql" | "db" => Some(Self::Sqlite),
            _ => None,
        }
    }

    /// Get the canonical name for this backend.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite",
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Schema Version Metadata
// ─────────────────────────────────────────────────────────────────────────────

/// Schema version metadata for canonical TOML files.
///
/// This struct is embedded in the `[meta]` section of all CLI-managed TOML files.
/// It enables forward-compatible parsing and version gating.
///
/// Fields use `#[serde(default)]` to allow partial `[meta]` sections in existing
/// files to parse without error, providing backward compatibility.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Meta {
    /// Semantic version of this file's schema (e.g., "1.0.0").
    #[serde(default = "Meta::default_schema_version")]
    pub schema_version: String,

    /// Version of `exo` that last wrote this file.
    #[serde(default = "Meta::default_exo_version")]
    pub exo_version: String,
}

impl Default for Meta {
    fn default() -> Self {
        Self {
            schema_version: "1.0.0".to_string(),
            exo_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

impl Meta {
    /// Create a new Meta with explicit versions.
    #[must_use]
    pub fn new(schema_version: impl Into<String>, exo_version: impl Into<String>) -> Self {
        Self {
            schema_version: schema_version.into(),
            exo_version: exo_version.into(),
        }
    }

    /// Create a new Meta with the current exo version.
    #[must_use]
    pub fn current() -> Self {
        Self::default()
    }

    /// Default schema version for serde.
    fn default_schema_version() -> String {
        "1.0.0".to_string()
    }

    /// Default exo version for serde.
    fn default_exo_version() -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Active Phase Lookup Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Result of finding the active phase in a plan, with full context.
#[derive(Debug, Clone, Serialize)]
pub struct ActivePhaseInfo<'a> {
    pub epoch: &'a Epoch,
    pub phase: &'a Phase,
    pub epoch_idx: usize,
    pub phase_idx: usize,
}

/// Owned version of active phase data for contexts that need ownership.
#[derive(Debug, Clone, Serialize)]
pub struct ActivePhaseData {
    pub id: String,
    pub title: String,
    pub epoch_id: String,
    pub epoch_title: String,
    /// Associated RFCs for this phase.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rfcs: Vec<PhaseRfc>,
    /// The kind of work this phase represents.
    #[serde(default)]
    pub kind: PhaseKind,
}

impl ActivePhaseInfo<'_> {
    /// Convert to an owned `ActivePhaseData` struct.
    pub fn to_owned_data(&self) -> ActivePhaseData {
        ActivePhaseData {
            id: self.phase.id.clone(),
            title: self.phase.title.clone(),
            epoch_id: self.epoch.id.clone(),
            epoch_title: self.epoch.title.clone(),
            rfcs: self.phase.rfcs.clone(),
            kind: self.phase.kind,
        }
    }
}

impl ExoState {
    /// Find the first globally in-progress phase in the plan.
    ///
    /// This is a whole-plan helper for diagnostics and legacy data inspection.
    /// Agent-facing "current phase" surfaces should use
    /// [`Self::find_workspace_active_phase`] so workspace focus is respected.
    pub fn find_active_phase(&self) -> Option<ActivePhaseInfo<'_>> {
        for (epoch_idx, epoch) in self.epochs.iter().enumerate() {
            for (phase_idx, phase) in epoch.phases.iter().enumerate() {
                if phase.status == "in-progress" {
                    return Some(ActivePhaseInfo {
                        epoch,
                        phase,
                        epoch_idx,
                        phase_idx,
                    });
                }
            }
        }
        None
    }

    /// Find only the ID of the currently active phase.
    ///
    /// This is a convenience method for cases where only the ID is needed.
    pub fn find_active_phase_id(&self) -> Option<String> {
        self.find_active_phase().map(|info| info.phase.id.clone())
    }

    /// Find the active epoch.
    ///
    /// Uses `derived_status()` — the canonical source of truth for epoch state.
    /// An epoch is "in-progress" when it has an in-progress phase, or has both
    /// completed and pending phases (between-phases state).
    pub fn find_active_epoch(&self) -> Option<&Epoch> {
        self.epochs
            .iter()
            .find(|e| e.derived_status() == "in-progress")
    }

    /// Find the first pending phase after a given anchor phase ID.
    ///
    /// If `anchor_id` is `None`, returns the first pending phase in plan order.
    /// Useful for suggesting the next phase to work on.
    pub fn find_next_pending_phase(&self, anchor_id: Option<&str>) -> Option<NextPhaseInfo<'_>> {
        let mut seen_anchor = anchor_id.is_none();

        for epoch in &self.epochs {
            for phase in &epoch.phases {
                if !seen_anchor {
                    if Some(phase.id.as_str()) == anchor_id {
                        seen_anchor = true;
                    }
                    continue;
                }

                if phase.status == "pending" {
                    return Some(NextPhaseInfo { epoch, phase });
                }
            }
        }

        None
    }

    /// Find a specific phase by ID, slug, ULID, or alias.
    ///
    /// Accepts:
    /// - Canonical reference: `phase@01HZVY...`
    /// - Raw ULID: `01HZVY...`
    /// - Slug: `map-phase-3`
    /// - Legacy ID: `phase-3`
    /// - Alias
    pub fn find_phase_by_id(&self, phase_id: &str) -> Option<PhaseInfo<'_>> {
        for epoch in &self.epochs {
            for phase in &epoch.phases {
                if phase.matches_id(phase_id).is_some() {
                    return Some(PhaseInfo { epoch, phase });
                }
            }
        }
        None
    }

    /// Find a specific epoch by ID, slug, ULID, or alias.
    pub fn find_epoch_by_id(&self, epoch_id: &str) -> Option<&Epoch> {
        self.epochs
            .iter()
            .find(|&epoch| epoch.matches_id(epoch_id).is_some())
            .map(|v| v as _)
    }

    /// Find a goal by ID within the active phase.
    pub fn find_goal_in_active_phase(&self, task_id: &str) -> Option<(&Phase, &Goal)> {
        let active = self.find_active_phase()?;
        for goal in &active.phase.goals {
            if goal.matches_id(task_id).is_some() {
                return Some((active.phase, goal));
            }
        }
        None
    }

    /// Find a goal by ID across all phases in the plan.
    ///
    /// This is the canonical lookup for goal data. Goals live in `SQLite`.
    ///
    /// Accepts:
    /// - Canonical reference: `goal@01HZVY...`
    /// - Raw ULID: `01HZVY...`
    /// - Slug: `my-goal-slug`
    /// - Legacy ID: `goal-id`
    pub fn find_goal_by_id(&self, goal_id: &str) -> Option<GoalInfo<'_>> {
        for epoch in &self.epochs {
            for phase in &epoch.phases {
                for goal in &phase.goals {
                    if goal.matches_id(goal_id).is_some() {
                        return Some(GoalInfo { epoch, phase, goal });
                    }
                }
            }
        }
        None
    }

    /// Find all epochs that need review (completed but not reviewed).
    pub fn find_unreviewed_epochs(&self) -> Vec<&Epoch> {
        self.epochs.iter().filter(|e| e.needs_review()).collect()
    }
}

/// Result of finding a goal by ID.
#[derive(Debug, Clone)]
pub struct GoalInfo<'a> {
    pub epoch: &'a Epoch,
    pub phase: &'a Phase,
    pub goal: &'a Goal,
}

/// Result of finding the next pending phase.
#[derive(Debug, Clone)]
pub struct NextPhaseInfo<'a> {
    pub epoch: &'a Epoch,
    pub phase: &'a Phase,
}

/// Result of finding a phase by ID.
#[derive(Debug, Clone)]
pub struct PhaseInfo<'a> {
    pub epoch: &'a Epoch,
    pub phase: &'a Phase,
}

/// A goal within a phase.
///
/// The `id` field can be either:
/// - A legacy string ID (e.g., "scaffold", "ci-tests")
/// - A ULID with optional slug (e.g., "01HZVY3X4M5N6P7Q8R9S0TABC1")
///
/// During migration, both forms coexist. After `exo plan migrate-ids`,
/// all goals will have ULID-based IDs with slugs.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct Goal {
    /// Primary identifier. Can be a legacy string ID or ULID.
    pub id: String,
    /// Human-readable label for the goal.
    pub label: String,
    /// Status: "pending", "in-progress", "completed", "abandoned", "skipped", "red", or "green"
    pub status: String,
    /// Kind of goal: "regular" or "strike".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// RFC3339 timestamp indicating when this goal started.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "started_at",
        alias = "started-at"
    )]
    pub started_at: Option<DateTime<Utc>>,
    /// Optional description for strike goals.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Completion log for the goal.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "completion_log",
        alias = "completion-log"
    )]
    pub completion_log: Option<String>,
    /// ULID-based canonical identifier (present after migration).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ulid: Option<ExoUlid>,
    /// Human-readable slug (preserved from legacy ID or generated).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    /// Alternative IDs for this goal (for backward compat lookups).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// RFC this goal is advancing (e.g., "00238").
    /// When set, steering knows this goal is pipeline work, not generic implementation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rfc: Option<String>,
    /// Target stage for RFC promotion (e.g., 1 means "promote to Stage 1").
    /// When set with `rfc`, this is a promotion goal — steering suggests review, not TDD.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "target-stage"
    )]
    pub target_stage: Option<u8>,
}

impl<'de> Deserialize<'de> for Goal {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct GoalInput {
            id: String,
            label: String,
            status: String,
            #[serde(default)]
            kind: Option<String>,
            #[serde(default, rename = "started_at", alias = "started-at")]
            started_at: Option<DateTime<Utc>>,
            #[serde(default)]
            description: Option<String>,
            #[serde(default, rename = "completion_log", alias = "completion-log")]
            completion_log: Option<String>,
            #[serde(default)]
            ulid: Option<ExoUlid>,
            #[serde(default)]
            slug: Option<String>,
            #[serde(default)]
            aliases: Vec<String>,
            #[serde(default)]
            rfc: Option<String>,
            #[serde(default, alias = "target-stage")]
            target_stage: Option<u8>,
        }

        let input = GoalInput::deserialize(deserializer)?;
        Ok(Self {
            id: input.id,
            label: input.label,
            status: normalize_status(&input.status).to_string(),
            kind: input.kind,
            started_at: input.started_at,
            description: input.description,
            completion_log: input.completion_log,
            ulid: input.ulid,
            slug: input.slug,
            aliases: input.aliases,
            rfc: input.rfc,
            target_stage: input.target_stage,
        })
    }
}

impl Goal {
    /// Get the canonical reference for this goal.
    ///
    /// If the goal has a ULID, returns `goal@{ulid}`.
    /// Otherwise returns the legacy string ID.
    #[must_use]
    pub fn canonical_ref(&self) -> String {
        if let Some(ulid) = &self.ulid {
            ulid.canonical_ref("goal")
        } else {
            self.id.clone()
        }
    }

    /// Check if this goal has been migrated to ULID.
    #[must_use]
    pub const fn has_ulid(&self) -> bool {
        self.ulid.is_some()
    }

    /// Check if this goal is in a terminal state.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status.as_str(),
            "completed" | "red" | "green" | "aborted" | "skipped" | "abandoned"
        )
    }
}

impl UlidResolvable for Goal {
    fn get_ulid(&self) -> Option<&ExoUlid> {
        self.ulid.as_ref()
    }

    fn get_slug(&self) -> Option<&str> {
        self.slug.as_deref()
    }

    fn get_id(&self) -> &str {
        &self.id
    }

    fn get_aliases(&self) -> &[String] {
        &self.aliases
    }
}

/// An RFC reference within a phase.
///
/// Supports two formats for backward compatibility:
/// - Simple string: `"00238"` (related RFC, no target)
/// - Record: `{ id = "00238", target = 1 }` (driving RFC with target stage)
///
/// The presence of `target` determines the role:
/// - With `target`: driving RFC (`▸` prefix, this phase advances it)
/// - Without `target`: related RFC (`·` prefix, referenced but not advanced)
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PhaseRfcInput {
    String(String),
    Struct {
        id: String,
        target: Option<u8>,
        relation: Option<String>,
    },
}

impl From<PhaseRfcInput> for PhaseRfc {
    fn from(input: PhaseRfcInput) -> Self {
        match input {
            // Bare string is always "related" — no target or relation possible.
            PhaseRfcInput::String(id) => Self {
                id,
                target: None,
                relation: "related".to_string(),
            },
            // Struct: relation should be present after migration.
            // If missing, default to "related" — the upgrade gate will flag it.
            PhaseRfcInput::Struct {
                id,
                target,
                relation,
            } => Self {
                id,
                target,
                relation: relation.unwrap_or_else(|| "related".to_string()),
            },
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(from = "PhaseRfcInput")]
pub struct PhaseRfc {
    /// RFC ID (e.g., "00238")
    pub id: String,
    /// Target stage for this phase (if this phase aims to advance the RFC)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<u8>,
    /// Relation type: "driving", "related", or "blocked".
    /// Always populated — derived from `target` presence during deserialization if not explicit.
    pub relation: String,
}

impl PhaseRfc {
    /// Create a driving RFC reference (with target stage).
    #[must_use]
    pub fn driving(id: impl Into<String>, target: u8) -> Self {
        Self {
            id: id.into(),
            target: Some(target),
            relation: "driving".to_string(),
        }
    }

    /// Create a related RFC reference (no target, just referenced).
    #[must_use]
    pub fn related(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            target: None,
            relation: "related".to_string(),
        }
    }

    /// Create a blocked RFC reference.
    #[must_use]
    pub fn blocked(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            target: None,
            relation: "blocked".to_string(),
        }
    }

    /// Returns the relation type: "driving", "related", or "blocked".
    #[must_use]
    pub fn relation(&self) -> &str {
        &self.relation
    }

    /// Returns true if this is a driving RFC.
    #[must_use]
    pub fn is_driving(&self) -> bool {
        self.relation == "driving"
    }
}

/// The kind of work a phase represents.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PhaseKind {
    /// Regular product work, typically driven by an RFC
    #[default]
    Regular,
    /// Maintenance or chore work, not needing an RFC
    Chore,
}

impl PhaseKind {
    /// Returns the string representation of this phase kind.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Regular => "regular",
            Self::Chore => "chore",
        }
    }
}

impl std::str::FromStr for PhaseKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "regular" => Ok(Self::Regular),
            "chore" => Ok(Self::Chore),
            _ => Err(format!("Invalid phase kind: {s}")),
        }
    }
}

/// A phase within an epoch.
///
/// Phases represent major work units in the project plan.
/// After migration, phases have ULID-based identifiers.
#[derive(Debug, Serialize, Clone)]
pub struct Phase {
    /// Primary identifier. Can be a legacy string ID or ULID.
    pub id: String,
    /// Human-readable title for the phase.
    pub title: String,
    /// Status: "pending", "in-progress", "completed", "abandoned", or "deferred"
    pub status: String,
    /// Goals within this phase.
    #[serde(alias = "tasks")]
    pub goals: Vec<Goal>,
    /// Associated RFCs with optional target stages.
    ///
    /// - Driving RFCs have `target` set (this phase aims to advance them)
    /// - Related RFCs have no `target` (referenced but not advanced)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rfcs: Vec<PhaseRfc>,
    /// The kind of work this phase represents.
    #[serde(default)]
    pub kind: PhaseKind,
    /// ULID-based canonical identifier (present after migration).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ulid: Option<ExoUlid>,
    /// Human-readable slug (preserved from legacy ID or generated).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    /// Alternative IDs for this phase (for backward compat lookups).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
}

impl Phase {
    /// Get the canonical reference for this phase.
    ///
    /// If the phase has a ULID, returns `phase@{ulid}`.
    /// Otherwise returns the legacy string ID.
    #[must_use]
    pub fn canonical_ref(&self) -> String {
        if let Some(ulid) = &self.ulid {
            ulid.canonical_ref("phase")
        } else {
            self.id.clone()
        }
    }

    /// Check if this phase has been migrated to ULID.
    #[must_use]
    pub const fn has_ulid(&self) -> bool {
        self.ulid.is_some()
    }
}

impl UlidResolvable for Phase {
    fn get_ulid(&self) -> Option<&ExoUlid> {
        self.ulid.as_ref()
    }

    fn get_slug(&self) -> Option<&str> {
        self.slug.as_deref()
    }

    fn get_id(&self) -> &str {
        &self.id
    }

    fn get_aliases(&self) -> &[String] {
        &self.aliases
    }
}

impl<'de> Deserialize<'de> for Phase {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum GoalsInput {
            Structured(Vec<Goal>),
            Labels(Vec<String>),
        }

        /// Accepts either a simple string or a full record.
        /// - `"00238"` → related RFC (no target)
        /// - `{ id = "00238", target = 1 }` → driving RFC
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum RfcInput {
            Record(PhaseRfc),
            Simple(String),
        }

        #[derive(Deserialize)]
        struct PhaseInput {
            id: String,
            title: String,
            status: String,
            /// Legacy field name for phase goals
            #[serde(default)]
            tasks: Option<GoalsInput>,
            /// New field name for phase goals (alias for tasks)
            /// Per RFC 00177: "goals" is the preferred terminology
            #[serde(default)]
            goals: Option<GoalsInput>,
            #[serde(default)]
            rfcs: Option<Vec<RfcInput>>,
            #[serde(default)]
            kind: PhaseKind,
            #[serde(default)]
            ulid: Option<ExoUlid>,
            #[serde(default)]
            slug: Option<String>,
            #[serde(default)]
            aliases: Vec<String>,
        }

        let input = PhaseInput::deserialize(deserializer)?;

        let normalized_status = normalize_status(&input.status);
        let default_task_status = if normalized_status == "completed" {
            "completed"
        } else {
            "pending"
        };

        // Accept either 'goals' (new, preferred) or 'tasks' (legacy)
        // If both are present, 'goals' takes precedence
        let goals_input = input.goals.or(input.tasks);
        let goals = match goals_input {
            None => Vec::new(),
            Some(GoalsInput::Structured(goals)) => goals,
            Some(GoalsInput::Labels(labels)) => upgrade_goal_labels(labels, default_task_status),
        };

        // Convert RfcInput to PhaseRfc, upgrading simple strings to related RFCs
        let rfcs = input
            .rfcs
            .unwrap_or_default()
            .into_iter()
            .map(|rfc_input| match rfc_input {
                RfcInput::Record(rfc) => rfc,
                RfcInput::Simple(id) => PhaseRfc::related(id),
            })
            .collect();

        Ok(Self {
            id: input.id,
            title: input.title,
            status: normalized_status.to_string(),
            goals,
            rfcs,
            kind: input.kind,
            ulid: input.ulid,
            slug: input.slug,
            aliases: input.aliases,
        })
    }
}

fn upgrade_goal_labels(labels: Vec<String>, default_status: &str) -> Vec<Goal> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    labels
        .into_iter()
        .map(|label| {
            let base = slugify_task_id(&label);
            let base = if base.is_empty() {
                "goal".to_string()
            } else {
                base
            };

            let next = seen
                .entry(base.clone())
                .and_modify(|c| *c += 1)
                .or_insert(1);
            let id = if *next == 1 {
                base
            } else {
                format!("{base}-{next}")
            };

            Goal {
                id,
                label,
                // Legacy shorthand doesn't provide per-goal status.
                status: default_status.to_string(),
                kind: None,
                started_at: None,
                description: None,
                completion_log: None,
                // No ULID for legacy goals - migration will add these
                ulid: None,
                slug: None,
                aliases: Vec::new(),
                rfc: None,
                target_stage: None,
            }
        })
        .collect()
}

fn slugify_task_id(label: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;

    for ch in label.chars() {
        let ch = ch.to_ascii_lowercase();
        let is_alnum = ch.is_ascii_alphanumeric();
        if is_alnum {
            out.push(ch);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }

    out.trim_matches('-').to_string()
}

/// An epoch in the project plan.
///
/// Epochs are high-level groupings of phases.
/// After migration, epochs have ULID-based identifiers.
///
/// **Note**: The `status` field is derived from phase statuses and is not stored.
/// It is computed on serialization for backward compatibility with consumers.
#[derive(Debug, Clone)]
pub struct Epoch {
    /// Primary identifier. Can be a legacy string ID or ULID.
    pub id: String,
    /// Human-readable title for the epoch.
    pub title: String,
    /// Phases within this epoch.
    pub phases: Vec<Phase>,
    /// ULID-based canonical identifier (present after migration).
    pub ulid: Option<ExoUlid>,
    /// Human-readable slug (preserved from legacy ID or generated).
    pub slug: Option<String>,
    /// Alternative IDs for this epoch (for backward compat lookups).
    pub aliases: Vec<String>,
    /// Whether this epoch has been reviewed after completion.
    /// Advisory: warns if false when epoch is completed, but doesn't block.
    pub reviewed: bool,
}

/// Helper struct for deserializing Epoch from TOML.
/// Accepts (and ignores) the legacy `status` field.
#[derive(Deserialize)]
struct EpochInput {
    id: String,
    title: String,
    #[serde(default)]
    #[allow(dead_code)]
    status: Option<String>, // Ignored - status is now derived
    #[serde(default)]
    phases: Vec<Phase>,
    #[serde(default)]
    ulid: Option<ExoUlid>,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    reviewed: bool,
}

impl<'de> Deserialize<'de> for Epoch {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let input = EpochInput::deserialize(deserializer)?;
        Ok(Self {
            id: input.id,
            title: input.title,
            phases: input.phases,
            ulid: input.ulid,
            slug: input.slug,
            aliases: input.aliases,
            reviewed: input.reviewed,
        })
    }
}

impl Serialize for Epoch {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;

        // Count non-empty optional fields
        let mut field_count = 4; // id, title, status, phases (always present)
        if self.ulid.is_some() {
            field_count += 1;
        }
        if self.slug.is_some() {
            field_count += 1;
        }
        if !self.aliases.is_empty() {
            field_count += 1;
        }
        if self.reviewed {
            field_count += 1;
        }

        let mut state = serializer.serialize_struct("Epoch", field_count)?;
        state.serialize_field("id", &self.id)?;
        state.serialize_field("title", &self.title)?;
        state.serialize_field("status", self.derived_status())?;
        state.serialize_field("phases", &self.phases)?;
        if let Some(ref ulid) = self.ulid {
            state.serialize_field("ulid", ulid)?;
        }
        if let Some(ref slug) = self.slug {
            state.serialize_field("slug", slug)?;
        }
        if !self.aliases.is_empty() {
            state.serialize_field("aliases", &self.aliases)?;
        }
        if self.reviewed {
            state.serialize_field("reviewed", &self.reviewed)?;
        }
        state.end()
    }
}

impl Epoch {
    /// Get the canonical reference for this epoch.
    ///
    /// If the epoch has a ULID, returns `epoch@{ulid}`.
    /// Otherwise returns the legacy string ID.
    #[must_use]
    pub fn canonical_ref(&self) -> String {
        if let Some(ulid) = &self.ulid {
            ulid.canonical_ref("epoch")
        } else {
            self.id.clone()
        }
    }

    /// Check if this epoch has been migrated to ULID.
    #[must_use]
    pub const fn has_ulid(&self) -> bool {
        self.ulid.is_some()
    }

    /// Derive the epoch status from its phases.
    ///
    /// This is the canonical way to determine epoch status - the stored `status`
    /// field is deprecated and should not be read directly.
    ///
    /// Status is derived as follows:
    /// - If any phase is "in-progress" → "in-progress"
    /// - If any phase is "abandoned" → "abandoned"
    /// - If all phases are "completed" → "completed"
    /// - If all non-completed phases are "deferred" → "deferred"
    /// - Otherwise → "pending"
    #[must_use]
    pub fn derived_status(&self) -> &'static str {
        // Check for active phases first (normalize-on-read means only "in-progress")
        if self.phases.iter().any(|p| p.status == "in-progress") {
            return "in-progress";
        }

        // Check for abandoned phases
        if self.phases.iter().any(|p| p.status == "abandoned") {
            return "abandoned";
        }

        // If no phases, consider it pending
        if self.phases.is_empty() {
            return "pending";
        }

        // Check if all phases are completed
        if self.phases.iter().all(|p| p.status == "completed") {
            return "completed";
        }

        // Check if all non-completed phases are deferred
        // (epoch is deferred if remaining work is all deferred)
        if self
            .phases
            .iter()
            .filter(|p| p.status != "completed")
            .all(|p| p.status == "deferred")
        {
            return "deferred";
        }

        // Between-phases: some phases completed, some still pending.
        // This means work has started but the next phase hasn't been
        // activated yet. The epoch is still "in-progress".
        if self.phases.iter().any(|p| p.status == "completed")
            && self.phases.iter().any(|p| p.status == "pending")
        {
            return "in-progress";
        }

        // Otherwise pending (no work started yet)
        "pending"
    }

    /// Check if this epoch needs review.
    ///
    /// An epoch needs review if:
    /// - All phases are completed (`derived_status` == "completed")
    /// - The `reviewed` flag is false
    #[must_use]
    pub fn needs_review(&self) -> bool {
        self.derived_status() == "completed" && !self.reviewed
    }
}

impl UlidResolvable for Epoch {
    fn get_ulid(&self) -> Option<&ExoUlid> {
        self.ulid.as_ref()
    }

    fn get_slug(&self) -> Option<&str> {
        self.slug.as_deref()
    }

    fn get_id(&self) -> &str {
        &self.id
    }

    fn get_aliases(&self) -> &[String] {
        &self.aliases
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ExoState {
    /// Schema version metadata (added in exo 0.3.x).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,

    pub epochs: Vec<Epoch>,
}

#[derive(Debug)]
pub struct AgentContext {
    pub root: PathBuf,
    pub project: Option<Project>,
    pub plan: ExoState,
}

impl AgentContext {
    /// Create a minimal AgentContext for testing purposes.
    ///
    /// This creates an empty plan with no epochs, suitable for testing
    /// upgrade plugins that only need filesystem access.
    #[cfg(test)]
    #[must_use]
    pub fn new_for_testing(root: PathBuf) -> Self {
        Self {
            root,
            project: None,
            plan: ExoState::default(),
        }
    }

    /// Load context using the default storage backend.
    ///
    /// Currently defaults to TOML. Will switch to `SQLite` once the migration is complete
    /// and storage backend is threaded through the command dispatch system.
    #[allow(clippy::missing_errors_doc)]
    pub fn load(root: PathBuf) -> ExoResult<Self> {
        Self::load_with_backend(root, StorageBackend::default())
    }

    /// Load context using the specified storage backend.
    #[allow(clippy::missing_errors_doc)]
    pub fn load_with_backend(root: PathBuf, _backend: StorageBackend) -> ExoResult<Self> {
        let project = Project::resolve(&root).ok();
        Self::load_with_project(root, project)
    }

    /// Load context using a project that the transport already resolved.
    ///
    /// Daemon and CLI dispatch resolve project identity before command
    /// execution. Reusing that value keeps command reads from spawning a new
    /// Git discovery process for every storage helper they call.
    #[allow(clippy::missing_errors_doc)]
    pub fn load_with_project(root: PathBuf, project: Option<Project>) -> ExoResult<Self> {
        let plan = Self::load_from_sqlite(&root, project.as_ref())?;
        Ok(Self {
            root,
            project,
            plan,
        })
    }

    /// Hydrate portable SQL state and load context without reconciling RFCs.
    ///
    /// RFC read commands use this before entering their writer-lane
    /// reconciliation so a fresh clone imports its existing projection without
    /// publishing RFC metadata outside the command boundary.
    #[allow(clippy::missing_errors_doc)]
    pub fn load_hydrated_with_project(root: PathBuf, project: Option<Project>) -> ExoResult<Self> {
        let loader = Self::open_sqlite_loader(&root, project.as_ref())?;
        let plan = loader
            .load_state()
            .with_context(|| "Failed to load state from SQLite database")?;
        Ok(Self {
            root,
            project,
            plan,
        })
    }

    /// Hydrate and reconcile the project database before a request-scoped
    /// transaction begins. This keeps projection import and RFC cache
    /// publication outside any command transaction that may roll back.
    pub(crate) fn prepare_request_transaction(
        root: &Path,
        project: Option<&Project>,
    ) -> ExoResult<()> {
        Self::initialize_sqlite(root, project)?;
        crate::rfc::reconcile_rfcs_once_with_project(root, project)
            .with_context(|| "Failed to reconcile RFC metadata from disk into SQLite")?;
        Ok(())
    }

    /// Load context and the coherent RFC view observed by the same request.
    #[allow(clippy::missing_errors_doc)]
    pub fn load_with_project_and_rfc_view(
        root: PathBuf,
        project: Option<Project>,
    ) -> ExoResult<(Self, crate::rfc::EffectiveRfcView)> {
        let (plan, rfc_view) = Self::load_from_sqlite_with_rfc_view(&root, project.as_ref())?;
        Ok((
            Self {
                root,
                project,
                plan,
            },
            rfc_view,
        ))
    }

    fn load_from_sqlite_with_rfc_view(
        root: &Path,
        project: Option<&Project>,
    ) -> ExoResult<(ExoState, crate::rfc::EffectiveRfcView)> {
        Self::initialize_sqlite(root, project)?;
        let (_, rfc_view) = crate::rfc::observe_effective_rfc_view_with_project(root, project)
            .with_context(|| "Failed to reconcile RFC metadata from disk into SQLite")?;
        let loader = Self::open_sqlite_loader(root, project)?;
        let plan = loader
            .load_state()
            .with_context(|| "Failed to load state from SQLite database")?;
        Ok((plan, rfc_view))
    }

    fn load_from_sqlite(root: &Path, project: Option<&Project>) -> ExoResult<ExoState> {
        Self::initialize_sqlite(root, project)?;
        let db_path = db_path(root, project);
        if exosuit_storage::active_request_database(&db_path)?.is_none() {
            crate::rfc::reconcile_rfcs_once_with_project(root, project)
                .with_context(|| "Failed to reconcile RFC metadata from disk into SQLite")?;
        }
        let loader = Self::open_sqlite_loader(root, project)?;
        loader
            .load_state()
            .with_context(|| "Failed to load state from SQLite database")
    }

    /// Ensure the SQLite file exists and its schema is current without retaining
    /// a connection across RFC reconcile-lock acquisition.
    fn initialize_sqlite(root: &Path, project: Option<&Project>) -> ExoResult<()> {
        drop(Self::open_sqlite_loader(root, project)?);
        Ok(())
    }

    fn open_sqlite_loader(root: &Path, project: Option<&Project>) -> ExoResult<SqliteLoader> {
        let db_path = db_path(root, project);

        if !db_path.exists() {
            let sql_dir = sql_projection_dir(root, project);
            let has_sql_files = sql_dir
                .as_ref()
                .is_some_and(|sql_dir| sql_dir.join("epochs.sql").exists());

            let exosuit_toml = root.join("exosuit.toml");
            if !exosuit_toml.exists() && !has_sql_files {
                anyhow::bail!(
                    "Failed to load agent context: no exosuit.toml found at {}\n\n\
                     Run 'exo init' to initialize a new workspace.",
                    root.display()
                );
            }

            if let Some(sql_dir) = sql_dir.filter(|_| has_sql_files) {
                // Fresh clone: .sql files exist but no DB — import them
                import_sql_dumps(&sql_dir, &db_path)?;
            } else {
                // Neither .sql nor TOML — create empty DB
                if let Some(parent) = db_path.parent() {
                    fs::create_dir_all::<&Path>(parent).with_context(|| {
                        format!("Failed to create .cache directory at {}", parent.display())
                    })?;
                }
            }
        }

        SqliteLoader::open(&db_path)
            .with_context(|| format!("Failed to open SQLite database at {}", db_path.display()))
    }

    #[allow(clippy::missing_errors_doc)]
    pub fn get_current_phase(&self) -> ExoResult<String> {
        self.find_workspace_active_phase_id()?
            .ok_or_else(|| anyhow::anyhow!("No active phase found"))
    }

    /// Return the canonical workspace root key used for workspace-scoped state.
    #[must_use]
    pub fn workspace_root_key(&self) -> Option<String> {
        self.project
            .as_ref()
            .and_then(|project| project.workspace_root.as_ref())
            .map(|root| root.to_string_lossy().into_owned())
    }

    /// Load the phase pin for this workspace, if the project has a workspace root.
    #[allow(clippy::missing_errors_doc)]
    pub fn workspace_active_phase_pin(&self) -> ExoResult<Option<String>> {
        let Some(workspace_root) = self.workspace_root_key() else {
            return Ok(None);
        };

        let loader = SqliteLoader::open(db_path(&self.root, self.project.as_ref()))?;
        loader.load_workspace_active_phase(&workspace_root)
    }

    /// Find the workspace-scoped active phase.
    ///
    /// If this workspace has a pin, only the pinned phase can be active, and
    /// only while that phase is still in progress. Without a pin or workspace
    /// root, exactly one global in-progress phase is accepted as a legacy
    /// fallback; multiple global in-progress phases means no active phase.
    #[allow(clippy::missing_errors_doc)]
    pub fn find_workspace_active_phase(&self) -> ExoResult<Option<ActivePhaseInfo<'_>>> {
        if let Some(pinned) = self.workspace_active_phase_pin()? {
            return Ok(self
                .find_phase_info_by_text_id(&pinned)
                .filter(|info| info.phase.status == "in-progress"));
        }

        Ok(self.find_single_global_active_phase())
    }

    /// Find only the ID of the workspace-scoped active phase.
    #[allow(clippy::missing_errors_doc)]
    pub fn find_workspace_active_phase_id(&self) -> ExoResult<Option<String>> {
        Ok(self
            .find_workspace_active_phase()?
            .map(|info| info.phase.id.clone()))
    }

    /// Find the workspace-scoped active epoch.
    ///
    /// With a workspace pin, the pinned phase anchors the epoch even after the
    /// phase completes, until the epoch itself is complete. Without a pin, this
    /// follows the single-global-active fallback.
    #[allow(clippy::missing_errors_doc)]
    pub fn find_workspace_active_epoch(&self) -> ExoResult<Option<&Epoch>> {
        if let Some(pinned) = self.workspace_active_phase_pin()? {
            return Ok(self
                .find_phase_info_by_text_id(&pinned)
                .map(|info| info.epoch)
                .filter(|epoch| epoch.derived_status() != "completed"));
        }

        Ok(self
            .find_single_global_active_phase()
            .map(|info| info.epoch))
    }

    /// Return the phase ID that anchors this workspace, even if the pinned
    /// phase is now completed.
    #[allow(clippy::missing_errors_doc)]
    pub fn workspace_anchor_phase_id(&self) -> ExoResult<Option<String>> {
        if let Some(pinned) = self.workspace_active_phase_pin()?
            && self.find_phase_info_by_text_id(&pinned).is_some()
        {
            return Ok(Some(pinned));
        }

        self.find_workspace_active_phase_id()
    }

    fn find_phase_info_by_text_id(&self, text_id: &str) -> Option<ActivePhaseInfo<'_>> {
        for (epoch_idx, epoch) in self.plan.epochs.iter().enumerate() {
            for (phase_idx, phase) in epoch.phases.iter().enumerate() {
                if phase.id == text_id {
                    return Some(ActivePhaseInfo {
                        epoch,
                        phase,
                        epoch_idx,
                        phase_idx,
                    });
                }
            }
        }

        None
    }

    fn find_single_global_active_phase(&self) -> Option<ActivePhaseInfo<'_>> {
        let mut active = None;
        for (epoch_idx, epoch) in self.plan.epochs.iter().enumerate() {
            for (phase_idx, phase) in epoch.phases.iter().enumerate() {
                if phase.status != "in-progress" {
                    continue;
                }
                if active.is_some() {
                    return None;
                }
                active = Some(ActivePhaseInfo {
                    epoch,
                    phase,
                    epoch_idx,
                    phase_idx,
                });
            }
        }

        active
    }
}

/// Write SQL dump files from `SQLite` state to the active policy projection.
///
/// Each file contains sorted INSERT statements with foreign keys resolved to
/// `text_ids`. This is the git-friendly persistence format defined in RFC 10178.
///
/// Repo policy writes workspace `docs/agent-context/*.sql`; sidecar policy
/// writes the private sidecar projection; shadow policy writes no projection by
/// default.
///
/// A projection failure is logged as a warning (it leaves durable state
/// non-portable, which RFC 10189 says Exo must make diagnosable) but does not
/// block the originating mutation.
pub fn write_sql_dump(root: &std::path::Path) {
    let project = Project::resolve(root).ok();
    write_sql_dump_with_project(root, project.as_ref());
}

pub fn write_sql_dump_with_project(root: &std::path::Path, project: Option<&Project>) {
    if let Err(error) = write_sql_dump_with_project_result(root, project) {
        // A failed projection means durable state silently stopped becoming
        // portable. Surface it instead of swallowing — a stale projection is
        // exactly the kind of state inconsistency RFC 10189 says Exo must make
        // diagnosable. The same projection path serves every policy (repo,
        // sidecar), so name the policy rather than assuming sidecar.
        let policy = project.map_or("unknown", |project| project.policy.as_str());
        eprintln!(
            "warning: failed to write SQL projection (policy={policy}); \
             durable state may not be portable until this is resolved: {error:#}"
        );
    }
}

pub fn write_sql_dump_with_project_result(
    root: &std::path::Path,
    project: Option<&Project>,
) -> ExoResult<()> {
    let Some(dump_dir) = sql_projection_dir(root, project) else {
        return Ok(());
    };

    std::fs::create_dir_all(&dump_dir).with_context(|| {
        format!(
            "Failed to create SQL projection directory {}",
            dump_dir.display()
        )
    })?;
    remove_deprecated_sql_projection_files(&dump_dir)?;

    let db_path = db_path(root, project);
    let loader = SqliteLoader::open(&db_path)
        .with_context(|| format!("Failed to open SQLite database at {}", db_path.display()))?;

    let dumps = exosuit_storage::dump_tables(loader.database().connection())
        .context("Failed to dump SQLite tables")?;

    for (file_stem, table_name) in exosuit_storage::TABLE_ORDER {
        let Some((_, sql_content)) = dumps
            .iter()
            .find(|(dump_table, _)| dump_table == table_name)
        else {
            continue;
        };
        let file_name = format!("{file_stem}.sql");
        let path = dump_dir.join(&file_name);
        // Prepend auto-generated header so agents treat these as infrastructure.
        let header = "-- Auto-generated by exo. Regenerate: exo status\n";
        let content = format!("{header}{sql_content}");
        // Use std::fs::write directly — these are pure overwrites, not edits.
        std::fs::write(&path, content.as_bytes())
            .with_context(|| format!("Failed to write SQL projection {}", path.display()))?;
    }

    Ok(())
}

fn remove_deprecated_sql_projection_files(dump_dir: &std::path::Path) -> ExoResult<()> {
    for file_name in DEPRECATED_SQL_PROJECTION_FILES {
        let path = dump_dir.join(file_name);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("Failed to remove stale SQL projection {}", path.display())
                });
            }
        }
    }
    Ok(())
}

/// Import `.sql` dump files from `docs/agent-context/` into a fresh `SQLite` database.
///
/// Called on fresh clone: `.sql` files exist in git but `.cache/exo.db` doesn't.
/// Creates the database, runs migrations, reads each `.sql` file in dependency
/// order, and imports via `import_tables`.
pub(crate) fn sql_projection_dir(
    root: &std::path::Path,
    project: Option<&Project>,
) -> Option<PathBuf> {
    match project.map(|project| (project.policy, project.sidecar_projection_dir())) {
        Some((StatePolicy::Shadow, _)) => None,
        Some((StatePolicy::Sidecar, sidecar_dir)) => sidecar_dir,
        Some((StatePolicy::Repo, _)) | None => Some(root.join("docs/agent-context")),
    }
}

pub(crate) fn import_sql_dumps(
    sql_dir: &std::path::Path,
    db_path: &std::path::Path,
) -> ExoResult<()> {
    // Ensure .cache/ directory exists
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("Failed to create .cache directory at {}", parent.display())
        })?;
    }

    // Create DB with schema (open_database auto-creates + runs migrations)
    let db = exosuit_storage::open_database(db_path)
        .map_err(|e| anyhow::anyhow!("Failed to create database: {e}"))?;

    let has_any_dump = exosuit_storage::TABLE_ORDER.iter().any(|(file_stem, _)| {
        sql_dir
            .join(format!("{file_stem}.sql"))
            .metadata()
            .is_ok_and(|metadata| metadata.len() > 0)
    });

    if !has_any_dump {
        return Ok(());
    }

    let conn = db.connection();
    conn.set_db_config(
        exosuit_storage::rusqlite::config::DbConfig::SQLITE_DBCONFIG_DEFENSIVE,
        false,
    )
    .map_err(|error| anyhow::anyhow!("Failed to clear existing database rows: {error}"))?;
    let clear_result = conn.execute_batch(
        "DELETE FROM rfc_relations;
         DELETE FROM idea_task_refs;
         DELETE FROM idea_tags;
         DELETE FROM entity_aliases;
         DELETE FROM phase_rfcs_data;
         DELETE FROM axiom_tags;
         DELETE FROM axiom_implications;
         DELETE FROM axioms;
         DELETE FROM task_verifications;
         DELETE FROM task_logs;
         DELETE FROM rfcs_data;
         DELETE FROM inbox_data;
         DELETE FROM ideas_data;
         DELETE FROM tasks_data;
         DELETE FROM goals_data;
         DELETE FROM phases_data;
         DELETE FROM epochs_data;",
    );
    let restore_defensive = conn
        .set_db_config(
            exosuit_storage::rusqlite::config::DbConfig::SQLITE_DBCONFIG_DEFENSIVE,
            true,
        )
        .map_err(|error| anyhow::anyhow!("Failed to restore database defensive mode: {error}"));
    clear_result
        .map_err(|error| anyhow::anyhow!("Failed to clear existing database rows: {error}"))?;
    restore_defensive?;

    // Read .sql files in dependency order
    let mut dumps: Vec<exosuit_storage::dump::TableDump> = Vec::new();

    for (file_stem, table_name) in exosuit_storage::TABLE_ORDER {
        let path = sql_dir.join(format!("{file_stem}.sql"));
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => {
                return Err(e).with_context(|| format!("Failed to read {}", path.display()));
            }
        };
        dumps.push((table_name.to_string(), content));
    }

    // Import
    exosuit_storage::import_tables(db.connection(), &dumps)
        .map_err(|e| anyhow::anyhow!("Failed to import SQL dumps: {e}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fs2::FileExt;
    use std::fs::OpenOptions;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    const LOCK_ORDER_HELPER_MODE_ENV: &str = "EXO_TEST_CONTEXT_LOCK_ORDER_MODE";
    const LOCK_ORDER_HELPER_ROOT_ENV: &str = "EXO_TEST_CONTEXT_LOCK_ORDER_ROOT";
    const LOCK_ORDER_WAIT_MARKER_ENV: &str = "EXO_TEST_RFC_RECONCILE_LOCK_WAIT_MARKER";

    struct LockOrderChild(Option<Child>);

    impl LockOrderChild {
        fn spawn(root: &Path, mode: &str, marker: &Path) -> Self {
            let child = Command::new(std::env::current_exe().expect("current test executable"))
                .args([
                    "--exact",
                    "context::tests::lock_order_subprocess_helper",
                    "--nocapture",
                ])
                .env(LOCK_ORDER_HELPER_MODE_ENV, mode)
                .env(LOCK_ORDER_HELPER_ROOT_ENV, root)
                .env(LOCK_ORDER_WAIT_MARKER_ENV, marker)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("spawn lock-order helper process");
            Self(Some(child))
        }

        fn wait_for_marker(&mut self, marker: &Path) {
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                if marker.exists() {
                    return;
                }
                if self
                    .0
                    .as_mut()
                    .expect("child available")
                    .try_wait()
                    .expect("poll helper process")
                    .is_some()
                {
                    let output = self
                        .0
                        .take()
                        .expect("child available")
                        .wait_with_output()
                        .expect("collect lock-order helper output after premature exit");
                    panic!(
                        "lock-order helper exited before waiting on the RFC lock:\nstdout:\n{}\nstderr:\n{}",
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("lock-order helper did not reach the RFC lock within 5 seconds");
        }

        fn wait_for_success(mut self) {
            let mut child = self.0.take().expect("child available");
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                if let Some(status) = child.try_wait().expect("poll helper process") {
                    let output = child
                        .wait_with_output()
                        .expect("collect lock-order helper output");
                    assert!(
                        status.success(),
                        "lock-order helper failed:\nstdout:\n{}\nstderr:\n{}",
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    );
                    return;
                }
                assert!(
                    Instant::now() < deadline,
                    "lock-order helper did not finish within 30 seconds"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    impl Drop for LockOrderChild {
        fn drop(&mut self) {
            if let Some(child) = &mut self.0 {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    fn initialize_lock_order_workspace(root: &Path) -> PathBuf {
        std::fs::create_dir_all(root.join("docs/rfcs")).expect("create RFC directory");
        std::fs::create_dir_all(root.join(".cache")).expect("create cache directory");
        std::fs::write(
            root.join("exosuit.toml"),
            "[storage]\nbackend = \"sqlite\"\n",
        )
        .expect("write exosuit config");
        let database_path = db_path(root, None);
        drop(SqliteWriter::open(&database_path).expect("initialize SQLite database"));
        database_path
    }

    fn assert_context_path_does_not_hold_database_while_waiting(mode: &str) {
        let temp = tempfile::tempdir().expect("create tempdir");
        let root = temp.path();
        let database_path = initialize_lock_order_workspace(root);
        let lock_path = database_path.with_extension("rfc-reconcile.lock");
        let lock_file = OpenOptions::new()
            .create(true)
            .read(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .expect("open RFC reconcile lock");
        lock_file.lock_exclusive().expect("hold RFC reconcile lock");

        let marker = temp.path().join(format!("{mode}.waiting"));
        let mut child = LockOrderChild::spawn(root, mode, &marker);
        child.wait_for_marker(&marker);

        let connection = exosuit_storage::Connection::open(&database_path)
            .expect("open independent SQLite connection");
        connection
            .busy_timeout(Duration::from_millis(100))
            .expect("set independent busy timeout");
        let journal_mode: String = connection
            .query_row("PRAGMA journal_mode=DELETE", [], |row| row.get(0))
            .expect("switch journal mode while helper waits on RFC lock");
        assert_eq!(journal_mode, "delete");
        drop(connection);

        lock_file.unlock().expect("release RFC reconcile lock");
        child.wait_for_success();
    }

    #[test]
    fn lock_order_subprocess_helper() {
        let Some(mode) = std::env::var_os(LOCK_ORDER_HELPER_MODE_ENV) else {
            return;
        };
        let root = PathBuf::from(
            std::env::var_os(LOCK_ORDER_HELPER_ROOT_ENV).expect("lock-order helper root"),
        );
        match mode.to_string_lossy().as_ref() {
            "load" => {
                AgentContext::load_with_project(root, None).expect("load agent context");
            }
            "rfc-view" => {
                AgentContext::load_with_project_and_rfc_view(root, None)
                    .expect("load agent context with RFC view");
            }
            "prepare" => {
                AgentContext::prepare_request_transaction(&root, None)
                    .expect("prepare request transaction");
            }
            other => panic!("unknown lock-order helper mode: {other}"),
        }
    }

    #[test]
    fn load_from_sqlite_does_not_hold_database_while_waiting_for_rfc_lock() {
        assert_context_path_does_not_hold_database_while_waiting("load");
    }

    #[test]
    fn load_with_rfc_view_does_not_hold_database_while_waiting_for_rfc_lock() {
        assert_context_path_does_not_hold_database_while_waiting("rfc-view");
    }

    #[test]
    fn request_preparation_does_not_hold_database_while_waiting_for_rfc_lock() {
        assert_context_path_does_not_hold_database_while_waiting("prepare");
    }

    #[test]
    fn test_parse_plan() {
        let toml = r#"
[[epochs]]
id = "epoch-1"
title = "Genesis"
status = "active"

[[epochs.phases]]
id = "phase-1"
title = "Inception"
status = "completed"

[[epochs.phases.goals]]
id = "task-1"
label = "Dream"
status = "completed"
"#;
        let parsed = toml::from_str::<ExoState>(toml);
        assert!(parsed.is_ok(), "failed to parse ExoState TOML");
        let Ok(state) = parsed else {
            return;
        };
        assert_eq!(state.epochs.len(), 1);
        assert_eq!(state.epochs[0].title, "Genesis");
        assert_eq!(state.epochs[0].phases.len(), 1);
        assert_eq!(state.epochs[0].phases[0].goals.len(), 1);
    }

    #[test]
    fn test_parse_plan_with_legacy_string_tasks() {
        let toml = r#"
[[epochs]]
id = "epoch-1"
title = "Genesis"
status = "active"

[[epochs.phases]]
id = "phase-1"
title = "Setup"
status = "completed"
tasks = ["Scaffold", "Strict tooling", "CI + tests"]
"#;

        let parsed = toml::from_str::<ExoState>(toml);
        assert!(parsed.is_ok(), "failed to parse legacy string tasks");
        let state = parsed.unwrap();
        let phase = &state.epochs[0].phases[0];

        assert_eq!(phase.goals.len(), 3);
        assert_eq!(phase.goals[0].label, "Scaffold");
        assert_eq!(phase.goals[0].id, "scaffold");
        assert_eq!(phase.goals[0].status, "completed");

        assert_eq!(phase.goals[1].label, "Strict tooling");
        assert_eq!(phase.goals[1].id, "strict-tooling");
        assert_eq!(phase.goals[2].id, "ci-tests");
    }

    #[test]
    fn test_legacy_string_tasks_default_to_pending_when_phase_not_completed() {
        let toml = r#"
[[epochs]]
id = "epoch-1"
title = "Genesis"
status = "active"

[[epochs.phases]]
id = "phase-1"
title = "Setup"
status = "active"
tasks = ["One"]
"#;

        let state = toml::from_str::<ExoState>(toml).unwrap();
        let phase = &state.epochs[0].phases[0];
        assert_eq!(phase.goals[0].status, "pending");
    }

    #[test]
    fn test_legacy_string_tasks_default_to_completed_when_phase_is_complete() {
        let toml = r#"
[[epochs]]
id = "epoch-1"
title = "Genesis"
status = "active"

[[epochs.phases]]
id = "phase-1"
title = "Setup"
status = "complete"
tasks = ["One"]
"#;

        let state = toml::from_str::<ExoState>(toml).unwrap();
        let phase = &state.epochs[0].phases[0];
        assert_eq!(phase.status, "completed");
        assert_eq!(phase.goals[0].status, "completed");
    }

    #[test]
    fn test_derived_status_active_when_phase_is_active() {
        let toml = r#"
[[epochs]]
id = "epoch-1"
title = "Test"
status = "active"

[[epochs.phases]]
id = "phase-1"
title = "Phase 1"
status = "active"
"#;
        let state = toml::from_str::<ExoState>(toml).unwrap();
        assert_eq!(state.epochs[0].derived_status(), "in-progress");
    }

    #[test]
    fn test_derived_status_completed_when_all_phases_completed() {
        let toml = r#"
[[epochs]]
id = "epoch-1"
title = "Test"
status = "active"

[[epochs.phases]]
id = "phase-1"
title = "Phase 1"
status = "completed"

[[epochs.phases]]
id = "phase-2"
title = "Phase 2"
status = "completed"
"#;
        let state = toml::from_str::<ExoState>(toml).unwrap();
        assert_eq!(state.epochs[0].derived_status(), "completed");
    }

    #[test]
    fn test_derived_status_active_between_phases() {
        // When some phases are completed and some are pending (no active phase),
        // the epoch is still "active" — it's in the between-phases state.
        let toml = r#"
[[epochs]]
id = "epoch-1"
title = "Test"
status = "active"

[[epochs.phases]]
id = "phase-1"
title = "Phase 1"
status = "completed"

[[epochs.phases]]
id = "phase-2"
title = "Phase 2"
status = "pending"
"#;
        let state = toml::from_str::<ExoState>(toml).unwrap();
        assert_eq!(state.epochs[0].derived_status(), "in-progress");
    }

    #[test]
    fn test_derived_status_pending_when_no_work_started() {
        let toml = r#"
[[epochs]]
id = "epoch-1"
title = "Test"
status = "pending"

[[epochs.phases]]
id = "phase-1"
title = "Phase 1"
status = "pending"

[[epochs.phases]]
id = "phase-2"
title = "Phase 2"
status = "pending"
"#;
        let state = toml::from_str::<ExoState>(toml).unwrap();
        assert_eq!(state.epochs[0].derived_status(), "pending");
    }

    #[test]
    fn test_storage_backend_parse() {
        // "toml" now maps to Sqlite (legacy compat for existing configs)
        assert_eq!(StorageBackend::parse("toml"), Some(StorageBackend::Sqlite));
        assert_eq!(StorageBackend::parse("TOML"), Some(StorageBackend::Sqlite));
        assert_eq!(
            StorageBackend::parse("sqlite"),
            Some(StorageBackend::Sqlite)
        );
        assert_eq!(
            StorageBackend::parse("SQLITE"),
            Some(StorageBackend::Sqlite)
        );
        assert_eq!(StorageBackend::parse("sql"), Some(StorageBackend::Sqlite));
        assert_eq!(StorageBackend::parse("db"), Some(StorageBackend::Sqlite));
        assert_eq!(StorageBackend::parse("invalid"), None);
    }

    #[test]
    fn test_storage_backend_default() {
        assert_eq!(StorageBackend::default(), StorageBackend::Sqlite);
    }

    #[test]
    fn test_storage_backend_as_str() {
        assert_eq!(StorageBackend::Sqlite.as_str(), "sqlite");
    }

    #[test]
    fn write_sql_dump_removes_deprecated_agent_events_projection() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let root = temp.path();
        let projection_dir = root.join("docs/agent-context");
        std::fs::create_dir_all(&projection_dir).expect("create projection dir");
        std::fs::create_dir_all(root.join(".cache")).expect("create cache dir");
        let stale = projection_dir.join("agent_events.sql");
        std::fs::write(&stale, "-- stale telemetry projection\n").expect("write stale projection");

        let db =
            exosuit_storage::open_database(&db_path(root, None)).expect("open sqlite database");
        db.connection()
            .execute(
                "INSERT INTO agent_events(text_id, timestamp, event_type, summary)
                 VALUES('event-1', '2026-06-11T00:00:00Z', 'command', 'status')",
                [],
            )
            .expect("insert agent event");

        write_sql_dump_with_project_result(root, None).expect("write SQL dump");

        assert!(
            !stale.exists(),
            "deprecated agent_events projection should be removed"
        );
        assert!(
            projection_dir.join("epochs.sql").exists(),
            "current projection files should still be written"
        );
    }

    #[test]
    fn hydrated_context_imports_projection_without_reconciling_rfcs() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let root = temp.path();
        std::fs::create_dir_all(root.join(".cache")).expect("create cache dir");
        std::fs::create_dir_all(root.join("docs/rfcs/stage-1")).expect("create RFC dir");

        let db_path = db_path(root, None);
        let writer = SqliteWriter::open(&db_path).expect("open sqlite writer");
        writer
            .add_epoch("Hydrated Epoch", Some("hydrated-epoch"), &[])
            .expect("add projected epoch");
        drop(writer);
        write_sql_dump_with_project_result(root, None).expect("write SQL projection");
        std::fs::remove_file(&db_path).expect("remove initialized database");
        let _ = std::fs::remove_file(format!("{}-wal", db_path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", db_path.display()));

        std::fs::write(
            root.join("docs/rfcs/stage-1/00001-malformed.md"),
            "# RFC 1: Missing Anchor\n",
        )
        .expect("write malformed RFC");

        let context = AgentContext::load_hydrated_with_project(root.to_path_buf(), None)
            .expect("hydrate context without RFC reconciliation");
        assert_eq!(context.plan.epochs.len(), 1);
        assert_eq!(context.plan.epochs[0].title, "Hydrated Epoch");
    }

    #[test]
    fn repo_policy_uses_workspace_sql_projection() {
        let root = PathBuf::from("/workspace");
        let project = Project {
            id: crate::project::ProjectId::from_git_common_dir(&root.join(".git")),
            git_common_dir: root.join(".git"),
            workspace_root: Some(root.clone()),
            policy: StatePolicy::Repo,
            projects_config_path: None,
            state_root: root.join(".exo"),
            sidecar_key: None,
            sidecar_root: None,
            sidecar_auto_commit: false,
            sidecar_auto_push: crate::project::SidecarAutoPushPolicy::Never,
        };

        assert_eq!(
            sql_projection_dir(&root, Some(&project)),
            Some(root.join("docs/agent-context"))
        );
    }

    #[test]
    fn shadow_policy_has_no_workspace_sql_projection() {
        let root = PathBuf::from("/workspace");
        let project = Project {
            id: crate::project::ProjectId::from_git_common_dir(&root.join(".git")),
            git_common_dir: root.join(".git"),
            workspace_root: Some(root.clone()),
            policy: StatePolicy::Shadow,
            projects_config_path: None,
            state_root: PathBuf::from("/home/user/.exo/projects/project-id"),
            sidecar_key: None,
            sidecar_root: None,
            sidecar_auto_commit: false,
            sidecar_auto_push: crate::project::SidecarAutoPushPolicy::Never,
        };

        assert_eq!(sql_projection_dir(&root, Some(&project)), None);
    }

    #[test]
    fn sidecar_policy_uses_portable_sql_projection() {
        let root = PathBuf::from("/workspace");
        let sidecar_root = PathBuf::from("/sidecars");
        let project = Project {
            id: crate::project::ProjectId::from_git_common_dir(&root.join(".git")),
            git_common_dir: root.join(".git"),
            workspace_root: Some(root.clone()),
            policy: StatePolicy::Sidecar,
            projects_config_path: None,
            state_root: sidecar_root.join("projects/client-api"),
            sidecar_key: Some("client-api".to_string()),
            sidecar_root: Some(sidecar_root.clone()),
            sidecar_auto_commit: true,
            sidecar_auto_push: crate::project::SidecarAutoPushPolicy::IfRemote,
        };

        assert_eq!(
            sql_projection_dir(&root, Some(&project)),
            Some(sidecar_root.join("projects/client-api/agent-context"))
        );
    }

    #[test]
    fn sidecar_projection_excludes_agent_events_and_removes_stale_projection() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let root = temp.path().join("workspace");
        let sidecar_root = temp.path().join("sidecars");
        let project = Project {
            id: crate::project::ProjectId::from_git_common_dir(&root.join(".git")),
            git_common_dir: root.join(".git"),
            workspace_root: Some(root.clone()),
            policy: StatePolicy::Sidecar,
            projects_config_path: None,
            state_root: sidecar_root.join("projects/client-api"),
            sidecar_key: Some("client-api".to_string()),
            sidecar_root: Some(sidecar_root.clone()),
            sidecar_auto_commit: true,
            sidecar_auto_push: crate::project::SidecarAutoPushPolicy::IfRemote,
        };
        let projection_dir = project
            .sidecar_projection_dir()
            .expect("sidecar projection dir");
        std::fs::create_dir_all(&projection_dir).expect("create projection dir");
        let stale = projection_dir.join("agent_events.sql");
        std::fs::write(&stale, "-- stale telemetry projection\n").expect("write stale projection");

        let db_path = project.db_path();
        std::fs::create_dir_all(db_path.parent().expect("db parent")).expect("create db parent");
        let writer = SqliteWriter::open(&db_path).expect("open sqlite writer");
        writer
            .add_epoch("Sidecar Projection Epoch", None, &[])
            .expect("add epoch");
        writer
            .database()
            .connection()
            .execute(
                "INSERT INTO agent_events(text_id, timestamp, event_type, summary)
                 VALUES('event-1', '2026-06-11T00:00:00Z', 'command', 'status')",
                [],
            )
            .expect("insert agent event");

        write_sql_dump_with_project_result(&root, Some(&project)).expect("write SQL dump");

        assert!(
            !stale.exists(),
            "deprecated agent_events projection should be removed"
        );
        assert!(
            !projection_dir.join("agent_events.sql").exists(),
            "sidecar projection must not write agent_events.sql"
        );
        assert!(
            projection_dir.join("epochs.sql").exists(),
            "sidecar projection should still write durable tables"
        );
    }
}
