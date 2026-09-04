//! Canonical campaign RFC objectives and pull-request delivery evidence.

use crate::api::protocol::ErrorCode;
use crate::context::sqlite_loader::RfcRecord;
use crate::daemon_outcomes::{DaemonOwnerIdentity, DaemonOwnerState, classify_daemon_owner};
use crate::failure::ExoFailure;
use crate::process_spawn::CommandSpawnExt as _;
use anyhow::{Context, Result, bail};
use chrono::Utc;
use exosuit_storage::{
    Connection, OptionalExtension, RequestTransaction, open_request_database, params,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use std::process::{Command as ProcessCommand, Output};

pub(crate) fn project_flow_precondition(
    kind: &'static str,
    message: impl Into<String>,
) -> anyhow::Error {
    project_flow_precondition_with_details(kind, message, serde_json::json!({}))
}

pub(crate) fn project_flow_precondition_with_details(
    kind: &'static str,
    message: impl Into<String>,
    details: Value,
) -> anyhow::Error {
    let mut details = details.as_object().cloned().unwrap_or_default();
    details.insert("kind".to_string(), Value::String(kind.to_string()));
    ExoFailure::new(
        ErrorCode::PreconditionFailed,
        message,
        ExoFailure::orienting_steering(Vec::new()),
    )
    .with_details(Value::Object(details))
    .into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RfcRelation {
    Drives,
    Implements,
    Validates,
}

impl RfcRelation {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "drives" => Ok(Self::Drives),
            "implements" => Ok(Self::Implements),
            "validates" => Ok(Self::Validates),
            _ => bail!("relationship must be drives, implements, or validates"),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Drives => "drives",
            Self::Implements => "implements",
            Self::Validates => "validates",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryRole {
    Implements,
    Validates,
}

impl DeliveryRole {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "implements" => Ok(Self::Implements),
            "validates" => Ok(Self::Validates),
            _ => bail!("delivery role must be implements or validates"),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Implements => "implements",
            Self::Validates => "validates",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequestIdentity {
    pub provider: String,
    pub repository: String,
    pub number: i64,
    pub url: String,
}

impl PullRequestIdentity {
    pub fn parse(selector: &str) -> Result<Self> {
        if selector.is_empty()
            || selector
                .chars()
                .any(|character| character.is_whitespace() || character.is_control())
        {
            bail!("GitHub pull-request selectors cannot contain whitespace or controls");
        }
        let (repository, number) = if let Some(path) = selector.strip_prefix("https://github.com/")
        {
            if path.contains(['?', '#']) {
                bail!("GitHub pull-request URLs cannot contain a query or fragment");
            }
            let parts = path.split('/').collect::<Vec<_>>();
            if parts.len() != 4 || parts[2] != "pull" {
                bail!("expected https://github.com/owner/repository/pull/number");
            }
            (format!("{}/{}", parts[0], parts[1]), parts[3])
        } else {
            let Some((repository, number)) = selector.rsplit_once('#') else {
                bail!("expected owner/repository#number or a GitHub pull-request URL");
            };
            if repository.matches('/').count() != 1 {
                bail!("expected owner/repository#number");
            }
            (repository.to_string(), number)
        };

        let (owner, name) = repository
            .split_once('/')
            .context("expected owner/repository#number")?;
        validate_github_owner(owner)?;
        validate_github_repository(name)?;
        let repository = format!(
            "{}/{}",
            owner.to_ascii_lowercase(),
            name.to_ascii_lowercase()
        );
        let number = number
            .parse::<i64>()
            .context("pull-request number must be a positive integer")?;
        if number <= 0 {
            bail!("pull-request number must be a positive integer");
        }
        Ok(Self {
            provider: "github".to_string(),
            url: format!("https://github.com/{repository}/pull/{number}"),
            repository,
            number,
        })
    }
}

fn validate_github_owner(owner: &str) -> Result<()> {
    if owner.is_empty() || owner.len() > 39 {
        bail!("GitHub owner must contain between 1 and 39 characters");
    }
    if owner.starts_with('-') || owner.ends_with('-') {
        bail!("GitHub owner has an invalid hyphen placement");
    }
    if !owner
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        bail!("GitHub owner contains unsupported characters");
    }
    Ok(())
}

fn validate_github_repository(repository: &str) -> Result<()> {
    if repository.is_empty() || repository.len() > 100 || matches!(repository, "." | "..") {
        bail!("GitHub repository name must contain between 1 and 100 characters");
    }
    if !repository
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        bail!("GitHub repository name contains unsupported characters");
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderObservation {
    pub identity: PullRequestIdentity,
    pub title: String,
    pub lifecycle: String,
    pub head_oid: Option<String>,
    pub review_state: String,
    pub checks_state: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderFailure {
    pub class: &'static str,
    pub message: String,
}

pub trait PullRequestProvider {
    fn observe(
        &self,
        identity: &PullRequestIdentity,
    ) -> Result<ProviderObservation, ProviderFailure>;
}

pub trait GhProcess {
    fn output(&self, args: &[&str]) -> std::io::Result<Output>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct StdGhProcess;

impl GhProcess for StdGhProcess {
    fn output(&self, args: &[&str]) -> std::io::Result<Output> {
        ProcessCommand::new("gh")
            .args(args)
            .output_guarded_timeout(std::time::Duration::from_secs(30))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct GithubProvider<P = StdGhProcess> {
    process: P,
}

impl<P> GithubProvider<P> {
    pub const fn new(process: P) -> Self {
        Self { process }
    }
}

impl Default for GithubProvider<StdGhProcess> {
    fn default() -> Self {
        Self::new(StdGhProcess)
    }
}

impl<P: GhProcess> PullRequestProvider for GithubProvider<P> {
    fn observe(
        &self,
        identity: &PullRequestIdentity,
    ) -> Result<ProviderObservation, ProviderFailure> {
        let number = identity.number.to_string();
        let output = self
            .process
            .output(&[
                "pr",
                "view",
                &number,
                "--repo",
                &identity.repository,
                "--json",
                "title,state,mergedAt,url,headRefOid,reviewDecision,statusCheckRollup",
            ])
            .map_err(|error| ProviderFailure {
                class: if error.kind() == std::io::ErrorKind::TimedOut {
                    "timeout"
                } else {
                    "process_unavailable"
                },
                message: sanitize_provider_error(&error.to_string()),
            })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ProviderFailure {
                class: classify_github_failure(&stderr),
                message: sanitize_provider_error(&stderr),
            });
        }
        let value: Value =
            serde_json::from_slice(&output.stdout).map_err(|error| ProviderFailure {
                class: "invalid_response",
                message: sanitize_provider_error(&error.to_string()),
            })?;
        parse_github_observation(&value)
    }
}

fn parse_github_observation(value: &Value) -> Result<ProviderObservation, ProviderFailure> {
    let identity = value
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| ProviderFailure {
            class: "invalid_response",
            message: "GitHub response omitted the canonical pull-request URL".to_string(),
        })
        .and_then(|url| {
            PullRequestIdentity::parse(url).map_err(|error| ProviderFailure {
                class: "invalid_response",
                message: format!("GitHub returned an invalid pull-request URL: {error}"),
            })
        })?;
    let title = value
        .get("title")
        .and_then(Value::as_str)
        .ok_or_else(|| ProviderFailure {
            class: "invalid_response",
            message: "GitHub response omitted the pull-request title".to_string(),
        })?;
    let lifecycle = match value.get("state").and_then(Value::as_str) {
        Some("OPEN") => "open",
        Some("CLOSED") if value.get("mergedAt").is_some_and(|value| !value.is_null()) => "merged",
        Some("CLOSED" | "MERGED") => {
            if value.get("state").and_then(Value::as_str) == Some("MERGED") {
                "merged"
            } else {
                "closed"
            }
        }
        state => {
            return Err(ProviderFailure {
                class: "invalid_response",
                message: format!(
                    "GitHub response contained an unknown pull-request state: {}",
                    state.unwrap_or("<missing>")
                ),
            });
        }
    };
    let review_state = match value.get("reviewDecision").and_then(Value::as_str) {
        None | Some("") => "none",
        Some("REVIEW_REQUIRED") => "pending",
        Some("APPROVED") => "approved",
        Some("CHANGES_REQUESTED") => "changes_requested",
        Some(_) => "unknown",
    };
    let checks_state = match value.get("statusCheckRollup") {
        Some(Value::Array(checks)) if checks.is_empty() => "none",
        Some(Value::Array(checks)) => {
            let check_states = checks.iter().map(github_check_state).collect::<Vec<_>>();
            if check_states.contains(&"failing") {
                "failing"
            } else if check_states.contains(&"pending") {
                "pending"
            } else if check_states.contains(&"unknown") {
                "unknown"
            } else if check_states.iter().all(|state| *state == "passing") {
                "passing"
            } else {
                "unknown"
            }
        }
        Some(Value::Null) | None | Some(_) => "unknown",
    };
    Ok(ProviderObservation {
        identity,
        title: title.to_string(),
        lifecycle: lifecycle.to_string(),
        head_oid: value
            .get("headRefOid")
            .and_then(Value::as_str)
            .map(str::to_string),
        review_state: review_state.to_string(),
        checks_state: checks_state.to_string(),
    })
}

fn github_check_state(check: &Value) -> &'static str {
    let kind = check.get("__typename").and_then(Value::as_str);
    if kind == Some("StatusContext") || (kind.is_none() && check.get("state").is_some()) {
        return match check.get("state").and_then(Value::as_str) {
            Some("SUCCESS") => "passing",
            Some("FAILURE" | "ERROR") => "failing",
            Some("PENDING" | "EXPECTED") => "pending",
            _ => "unknown",
        };
    }
    if kind == Some("CheckRun")
        || (kind.is_none() && (check.get("status").is_some() || check.get("conclusion").is_some()))
    {
        return match check.get("status").and_then(Value::as_str) {
            Some("QUEUED" | "IN_PROGRESS" | "WAITING" | "PENDING" | "REQUESTED") => "pending",
            Some("COMPLETED") => match check.get("conclusion").and_then(Value::as_str) {
                Some("SUCCESS" | "NEUTRAL" | "SKIPPED") => "passing",
                Some(
                    "FAILURE" | "CANCELLED" | "TIMED_OUT" | "ACTION_REQUIRED" | "STARTUP_FAILURE"
                    | "STALE",
                ) => "failing",
                _ => "unknown",
            },
            _ => "unknown",
        };
    }
    "unknown"
}

fn classify_github_failure(stderr: &str) -> &'static str {
    let lower = stderr.to_ascii_lowercase();
    if lower.contains("permission")
        || lower.contains("forbidden")
        || lower.contains("resource not accessible")
        || lower.contains("insufficient scope")
        || lower.contains("required scopes")
    {
        "permission"
    } else if lower.contains("auth")
        || lower.contains("login")
        || lower.contains("not logged")
        || lower.contains("bad credentials")
    {
        "authentication"
    } else if lower.contains("not found")
        || lower.contains("404")
        || lower.contains("could not resolve")
        || lower.contains("no pull requests found")
    {
        "not_found"
    } else {
        "provider_unavailable"
    }
}

fn sanitize_provider_error(message: &str) -> String {
    let single_line = message
        .lines()
        .next()
        .unwrap_or("provider request failed")
        .trim();
    single_line.chars().take(240).collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RfcObjectiveView {
    pub rfc_ulid: String,
    pub rfc_number: i64,
    pub title: String,
    pub observed_stage: Option<u8>,
    pub current_stage: Option<u8>,
    pub lifecycle: Option<String>,
    pub superseded_by: Option<String>,
    pub target_stage: Option<u8>,
    pub relation: String,
    pub source: String,
    pub diagnostic: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RfcObjectiveMotion {
    Advancing,
    TargetReached,
    Associated,
    Terminal,
    IdentityMissing,
}

impl RfcObjectiveView {
    pub fn motion(&self) -> RfcObjectiveMotion {
        let Some(current_stage) = self.current_stage else {
            return RfcObjectiveMotion::IdentityMissing;
        };
        if matches!(
            self.lifecycle.as_deref(),
            Some("withdrawn" | "archived" | "superseded")
        ) {
            return RfcObjectiveMotion::Terminal;
        }
        match self.target_stage {
            Some(target_stage) if current_stage >= target_stage => {
                RfcObjectiveMotion::TargetReached
            }
            Some(_) if current_stage < 4 && self.lifecycle.as_deref() == Some("active") => {
                RfcObjectiveMotion::Advancing
            }
            Some(_) | None => RfcObjectiveMotion::Associated,
        }
    }

    pub fn can_advance(&self) -> bool {
        self.motion() == RfcObjectiveMotion::Advancing
    }
}

pub(crate) fn effective_rfc_lifecycle(rfc: &RfcRecord) -> (String, Option<String>) {
    let superseded_by = rfc.superseded_by.clone();
    let lifecycle = if matches!(rfc.status.as_str(), "withdrawn" | "archived") {
        rfc.status.as_str()
    } else if superseded_by.is_some() || rfc.status == "superseded" {
        "superseded"
    } else {
        "active"
    };
    (lifecycle.to_string(), superseded_by)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequestView {
    pub identity: PullRequestIdentity,
    pub role: String,
    pub title: Option<String>,
    pub lifecycle: Option<String>,
    pub head_oid: Option<String>,
    pub review_state: Option<String>,
    pub checks_state: Option<String>,
    pub last_success_at: Option<String>,
    pub last_attempt_at: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectFlowStatus {
    pub campaign_id: String,
    pub rfc_objectives: Vec<RfcObjectiveView>,
    pub pull_requests: Vec<PullRequestView>,
    pub diagnostics: Vec<String>,
}

pub fn resolve_campaign(conn: &Connection, selector: &str) -> Result<String> {
    let mut candidates = conn
        .prepare(
            "SELECT DISTINCT phase.text_id
             FROM phases_data phase
             LEFT JOIN entity_aliases alias
               ON alias.entity_type = 'phase' AND alias.entity_id = phase.id
             WHERE phase.text_id = ?1 OR alias.alias = ?1
             ORDER BY phase.text_id",
        )?
        .query_map([selector], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    candidates.dedup();
    match candidates.as_slice() {
        [campaign] => Ok(campaign.clone()),
        [] => Err(project_flow_precondition(
            "project_flow.campaign_not_found",
            format!("campaign '{selector}' not found"),
        )),
        _ => Err(project_flow_precondition(
            "project_flow.campaign_ambiguous",
            format!(
                "campaign selector '{selector}' is ambiguous: {}",
                candidates.join(", ")
            ),
        )),
    }
}

fn resolve_rfc(conn: &Connection, selector: &str) -> Result<RfcRecord> {
    let by_number = selector.bytes().all(|byte| byte.is_ascii_digit());
    let (sql, parameter): (&str, Box<dyn exosuit_storage::rusqlite::ToSql>) = if by_number {
        let number = selector.parse::<i64>().context("invalid RFC number")?;
        (
            "SELECT text_id, rfc_number, title, stage, status, feature, slug, file_path, superseded_by, supersedes, withdrawal_reason, archived_reason, consolidated_into FROM rfcs_data WHERE rfc_number = ?1 ORDER BY text_id",
            Box::new(number),
        )
    } else {
        (
            "SELECT text_id, rfc_number, title, stage, status, feature, slug, file_path, superseded_by, supersedes, withdrawal_reason, archived_reason, consolidated_into FROM rfcs_data WHERE text_id = ?1",
            Box::new(selector.to_string()),
        )
    };
    let mut stmt = conn.prepare(sql)?;
    let records = stmt
        .query_map([parameter.as_ref()], |row| {
            Ok(RfcRecord {
                text_id: row.get(0)?,
                rfc_number: row.get(1)?,
                title: row.get(2)?,
                stage: row.get(3)?,
                status: row.get(4)?,
                feature: row.get(5)?,
                slug: row.get(6)?,
                file_path: row.get(7)?,
                superseded_by: row.get(8)?,
                supersedes: row.get(9)?,
                withdrawal_reason: row.get(10)?,
                archived_reason: row.get(11)?,
                consolidated_into: row.get(12)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    match records.as_slice() {
        [record] => Ok(record.clone()),
        [] => Err(project_flow_precondition(
            "project_flow.rfc_not_found",
            format!("RFC '{selector}' not found"),
        )),
        _ => {
            let candidates = records
                .iter()
                .map(|record| {
                    serde_json::json!({
                        "rfc_ulid": record.text_id,
                        "title": record.title,
                        "stage": record.stage,
                        "status": record.status,
                        "path": record.file_path,
                    })
                })
                .collect::<Vec<_>>();
            Err(project_flow_precondition_with_details(
                "project_flow.rfc_ambiguous",
                format!(
                    "RFC number {selector} resolves to {}",
                    records
                        .iter()
                        .map(|record| format!(
                            "{} ({}, Stage {}, {}, {})",
                            record.text_id,
                            record.title,
                            record.stage,
                            record.status,
                            record.file_path
                        ))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                serde_json::json!({ "selector": selector, "candidates": candidates }),
            ))
        }
    }
}

pub fn attach_rfc(
    db_path: &Path,
    campaign: &str,
    selector: &str,
    relation: RfcRelation,
    target_stage: Option<u8>,
) -> Result<RfcObjectiveView> {
    let db = open_request_database(db_path)?;
    let conn = db.connection();
    let campaign = resolve_campaign(conn, campaign)?;
    let rfc = resolve_rfc(conn, selector)?;
    if let Some(target) = target_stage {
        if target > 4 {
            return Err(project_flow_precondition(
                "project_flow.invalid_target_stage",
                "target stage must be between 0 and 4",
            ));
        }
    }
    let phase_id: i64 = conn.query_row(
        "SELECT id FROM phases_data WHERE text_id = ?1",
        [&campaign],
        |row| row.get(0),
    )?;
    let now = Utc::now().to_rfc3339();
    let existing = conn
        .query_row(
            "SELECT id, observed_stage, target_stage, relation FROM campaign_rfc_objectives_data
             WHERE phase_id = ?1 AND rfc_ulid = ?2",
            params![phase_id, rfc.text_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<u8>>(1)?,
                    row.get::<_, Option<u8>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?;
    let observed_stage = existing
        .as_ref()
        .and_then(|(_, observed_stage, _, _)| *observed_stage)
        .unwrap_or(rfc.stage);
    if let Some((id, _, existing_target, existing_relation)) = existing {
        if existing_target == target_stage && existing_relation == relation.as_str() {
            let (lifecycle, superseded_by) = effective_rfc_lifecycle(&rfc);
            return Ok(RfcObjectiveView {
                rfc_ulid: rfc.text_id,
                rfc_number: rfc.rfc_number,
                title: rfc.title,
                observed_stage: Some(observed_stage),
                current_stage: Some(rfc.stage),
                lifecycle: Some(lifecycle),
                superseded_by,
                target_stage,
                relation: relation.as_str().to_string(),
                source: "typed".to_string(),
                diagnostic: None,
            });
        }
        if existing_target != target_stage
            && let Some(target) = target_stage
            && target <= rfc.stage
        {
            return Err(project_flow_precondition(
                "project_flow.target_stage_not_future",
                format!(
                    "target Stage {target} must be greater than current Stage {}",
                    rfc.stage
                ),
            ));
        }
        conn.execute(
            "UPDATE campaign_rfc_objectives
             SET target_stage = ?2, relation = ?3, updated_at = ?4
             WHERE id = ?1",
            params![id, target_stage, relation.as_str(), now],
        )?;
    } else {
        if let Some(target) = target_stage
            && target <= rfc.stage
        {
            return Err(project_flow_precondition(
                "project_flow.target_stage_not_future",
                format!(
                    "target Stage {target} must be greater than current Stage {}",
                    rfc.stage
                ),
            ));
        }
        let text_id = ulid::Ulid::new().to_string().to_lowercase();
        conn.execute(
            "INSERT INTO campaign_rfc_objectives (
                 text_id, phase_id, rfc_ulid, rfc_number_snapshot, rfc_title_snapshot,
                 observed_stage, target_stage, relation, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
            params![
                text_id,
                phase_id,
                rfc.text_id,
                rfc.rfc_number,
                rfc.title,
                rfc.stage,
                target_stage,
                relation.as_str(),
                now
            ],
        )?;
    }
    let (lifecycle, superseded_by) = effective_rfc_lifecycle(&rfc);
    Ok(RfcObjectiveView {
        rfc_ulid: rfc.text_id,
        rfc_number: rfc.rfc_number,
        title: rfc.title,
        observed_stage: Some(observed_stage),
        current_stage: Some(rfc.stage),
        lifecycle: Some(lifecycle),
        superseded_by,
        target_stage,
        relation: relation.as_str().to_string(),
        source: "typed".to_string(),
        diagnostic: None,
    })
}

pub fn detach_rfc(db_path: &Path, campaign: &str, selector: &str) -> Result<bool> {
    let db = open_request_database(db_path)?;
    let conn = db.connection();
    let campaign = resolve_campaign(conn, campaign)?;
    let rfc_ulid = if selector.bytes().all(|byte| byte.is_ascii_digit()) {
        resolve_rfc(conn, selector)?.text_id
    } else {
        selector.to_string()
    };
    Ok(conn.execute(
        "DELETE FROM campaign_rfc_objectives
         WHERE phase_id = (SELECT id FROM phases_data WHERE text_id = ?1)
           AND rfc_ulid = ?2",
        params![campaign, rfc_ulid],
    )? > 0)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PreparedTarget {
    identity: PullRequestIdentity,
    role: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PreparedObservation {
    identity: PullRequestIdentity,
    observation: Option<ProviderObservation>,
    failure_class: Option<String>,
    failure_message: Option<String>,
    attempted_at: String,
}

impl PreparedObservation {
    fn from_provider(
        identity: PullRequestIdentity,
        attempted_at: String,
        result: Result<ProviderObservation, ProviderFailure>,
    ) -> Self {
        match result {
            Ok(observation) => Self {
                identity,
                observation: Some(observation),
                failure_class: None,
                failure_message: None,
                attempted_at,
            },
            Err(failure) => Self {
                identity,
                observation: None,
                failure_class: Some(failure.class.to_string()),
                failure_message: Some(failure.message),
                attempted_at,
            },
        }
    }

    fn result(&self) -> Result<ProviderObservation, ProviderFailure> {
        self.observation.clone().ok_or_else(|| ProviderFailure {
            class: prepared_failure_class(self.failure_class.as_deref()),
            message: self
                .failure_message
                .clone()
                .unwrap_or_else(|| "provider read failed".to_string()),
        })
    }
}

fn prepared_failure_class(class: Option<&str>) -> &'static str {
    match class {
        Some("authentication") => "authentication",
        Some("permission") => "permission",
        Some("not_found") => "not_found",
        Some("invalid_response") => "invalid_response",
        Some("timeout") => "timeout",
        Some("process_unavailable") => "process_unavailable",
        _ => "provider_unavailable",
    }
}

enum PreparedRead {
    Completed(ProjectFlowStatus),
    Prepared {
        targets: Vec<PreparedTarget>,
        owner: DaemonOwnerIdentity,
    },
    Ready {
        targets: Vec<PreparedTarget>,
        observations: Vec<PreparedObservation>,
        owner: DaemonOwnerIdentity,
    },
    Terminalizing {
        owner: DaemonOwnerIdentity,
    },
}

fn read_prepared(
    conn: &Connection,
    request_id: &str,
    hash: &str,
    payload: &str,
    campaign: &str,
) -> Result<Option<PreparedRead>> {
    let row = conn
        .query_row(
            "SELECT request_hash, normalized_payload, phase_text_id, targets_json,
                provider_results_json, owner_instance_id, owner_pid,
                owner_process_start_id, state, result_json
         FROM project_flow_prepared_reads WHERE request_id = ?1",
            [request_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, u32>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, Option<String>>(9)?,
                ))
            },
        )
        .optional()?;
    let Some((
        stored_hash,
        stored_payload,
        stored_campaign,
        targets,
        provider_results,
        owner_instance_id,
        owner_pid,
        owner_process_start_id,
        state,
        result,
    )) = row
    else {
        return Ok(None);
    };
    if stored_hash != hash || stored_payload != payload || stored_campaign != campaign {
        return Err(project_flow_precondition(
            "project_flow.request_id_conflict",
            "project-flow request ID was reused with a different normalized payload",
        ));
    }
    if state == "completed" {
        let result = result.context("completed prepared read omitted its result")?;
        return Ok(Some(PreparedRead::Completed(serde_json::from_str(
            &result,
        )?)));
    }
    let targets = serde_json::from_str(&targets)?;
    if state == "ready" {
        let provider_results =
            provider_results.context("ready prepared read omitted provider results")?;
        return Ok(Some(PreparedRead::Ready {
            targets,
            observations: serde_json::from_str(&provider_results)?,
            owner: DaemonOwnerIdentity {
                instance_id: owner_instance_id,
                pid: owner_pid,
                process_start_id: owner_process_start_id,
            },
        }));
    }
    if state == "terminalizing" {
        result.context("terminalizing prepared read omitted its terminal response")?;
        return Ok(Some(PreparedRead::Terminalizing {
            owner: DaemonOwnerIdentity {
                instance_id: owner_instance_id,
                pid: owner_pid,
                process_start_id: owner_process_start_id,
            },
        }));
    }
    if state != "prepared" {
        bail!("project-flow prepared read has unsupported state '{state}'");
    }
    Ok(Some(PreparedRead::Prepared {
        targets,
        owner: DaemonOwnerIdentity {
            instance_id: owner_instance_id,
            pid: owner_pid,
            process_start_id: owner_process_start_id,
        },
    }))
}

fn prepare_external_read<F>(
    db_path: &Path,
    request_id: &str,
    payload: &str,
    campaign: &str,
    owner: &DaemonOwnerIdentity,
    build_targets: F,
    provider: &dyn PullRequestProvider,
) -> Result<PreparedRead>
where
    F: FnOnce(&Connection) -> Result<Vec<PreparedTarget>>,
{
    prepare_external_read_with_classifier(
        db_path,
        request_id,
        payload,
        campaign,
        owner,
        build_targets,
        provider,
        classify_daemon_owner,
    )
}

#[allow(clippy::too_many_arguments)]
fn prepare_external_read_with_classifier<F, C>(
    db_path: &Path,
    request_id: &str,
    payload: &str,
    campaign: &str,
    owner: &DaemonOwnerIdentity,
    build_targets: F,
    provider: &dyn PullRequestProvider,
    classify_owner: C,
) -> Result<PreparedRead>
where
    F: FnOnce(&Connection) -> Result<Vec<PreparedTarget>>,
    C: Fn(&DaemonOwnerIdentity) -> DaemonOwnerState,
{
    let hash = blake3::hash(payload.as_bytes()).to_hex().to_string();
    let prepared = {
        let db = open_request_database(db_path)?;
        read_prepared(db.connection(), request_id, &hash, payload, campaign)?
    };
    if let Some(prepared) = prepared {
        return resume_prepared_external_read(
            db_path,
            request_id,
            &hash,
            owner,
            prepared,
            provider,
            &classify_owner,
        );
    }
    let transaction = RequestTransaction::begin(db_path)?;
    let conn = transaction.database().connection();
    let targets = build_targets(conn)?;
    let now = Utc::now().to_rfc3339();
    let targets_json = serde_json::to_string(&targets)?;
    let changed = conn.execute(
        "INSERT INTO project_flow_prepared_reads (
             request_id, request_hash, normalized_payload, phase_text_id, targets_json,
             owner_instance_id, owner_pid, owner_process_start_id, recovery_class, state, prepared_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'prepared_external_read', 'prepared', ?9)
         ON CONFLICT(request_id) DO NOTHING",
        params![request_id, hash, payload, campaign, targets_json, owner.instance_id,
            owner.pid, owner.process_start_id, now],
    )?;
    if changed != 1 {
        transaction.rollback()?;
        let db = open_request_database(db_path)?;
        let prepared = read_prepared(db.connection(), request_id, &hash, payload, campaign)?
            .ok_or_else(|| {
                project_flow_precondition(
                    "project_flow.prepared_insert_lost",
                    "project-flow prepared read disappeared after a concurrent insertion",
                )
            })?;
        drop(db);
        return resume_prepared_external_read(
            db_path,
            request_id,
            &hash,
            owner,
            prepared,
            provider,
            &classify_owner,
        );
    }
    transaction.commit()?;
    observe_and_ready(db_path, request_id, &hash, owner, targets, provider)
}

fn resume_prepared_external_read<C>(
    db_path: &Path,
    request_id: &str,
    hash: &str,
    owner: &DaemonOwnerIdentity,
    prepared: PreparedRead,
    provider: &dyn PullRequestProvider,
    classify_owner: &C,
) -> Result<PreparedRead>
where
    C: Fn(&DaemonOwnerIdentity) -> DaemonOwnerState,
{
    let (targets, observations, stored_owner, state) = match prepared {
        PreparedRead::Completed(result) => return Ok(PreparedRead::Completed(result)),
        PreparedRead::Prepared {
            targets,
            owner: stored_owner,
        } => (targets, None, stored_owner, "prepared"),
        PreparedRead::Ready {
            targets,
            observations,
            owner: stored_owner,
        } => (targets, Some(observations), stored_owner, "ready"),
        PreparedRead::Terminalizing {
            owner: stored_owner,
        } => (Vec::new(), None, stored_owner, "terminalizing"),
    };
    if stored_owner == *owner {
        if state == "terminalizing" {
            return Ok(PreparedRead::Terminalizing {
                owner: owner.clone(),
            });
        }
        if let Some(observations) = observations {
            return Ok(PreparedRead::Ready {
                targets,
                observations,
                owner: owner.clone(),
            });
        }
        return observe_and_ready(db_path, request_id, hash, owner, targets, provider);
    }
    match classify_owner(&stored_owner) {
        DaemonOwnerState::Dead | DaemonOwnerState::PidReused => {}
        DaemonOwnerState::Current => {
            return Err(project_flow_precondition(
                "project_flow.prepared_owner_live",
                format!(
                    "project-flow request is owned by live daemon instance {}",
                    stored_owner.instance_id
                ),
            ));
        }
        DaemonOwnerState::Unknown => {
            return Err(project_flow_precondition(
                "project_flow.prepared_owner_unknown",
                format!(
                    "project-flow request owner {} could not be verified",
                    stored_owner.instance_id
                ),
            ));
        }
    }
    let transaction = RequestTransaction::begin(db_path)?;
    let conn = transaction.database().connection();
    let changed = conn.execute(
        "UPDATE project_flow_prepared_reads
         SET owner_instance_id = ?2, owner_pid = ?3, owner_process_start_id = ?4
         WHERE request_id = ?1 AND request_hash = ?5 AND state = ?9
           AND owner_instance_id = ?6 AND owner_pid = ?7
           AND owner_process_start_id = ?8",
        params![
            request_id,
            owner.instance_id,
            owner.pid,
            owner.process_start_id,
            hash,
            stored_owner.instance_id,
            stored_owner.pid,
            stored_owner.process_start_id,
            state,
        ],
    )?;
    if changed != 1 {
        transaction.rollback()?;
        return Err(project_flow_precondition(
            "project_flow.prepared_owner_changed",
            "project-flow prepared read ownership changed during recovery",
        ));
    }
    transaction.commit()?;
    if state == "terminalizing" {
        Ok(PreparedRead::Terminalizing {
            owner: owner.clone(),
        })
    } else if let Some(observations) = observations {
        Ok(PreparedRead::Ready {
            targets,
            observations,
            owner: owner.clone(),
        })
    } else {
        observe_and_ready(db_path, request_id, hash, owner, targets, provider)
    }
}

fn observe_and_ready(
    db_path: &Path,
    request_id: &str,
    hash: &str,
    owner: &DaemonOwnerIdentity,
    targets: Vec<PreparedTarget>,
    provider: &dyn PullRequestProvider,
) -> Result<PreparedRead> {
    let observations = targets
        .iter()
        .map(|target| {
            let attempted_at = Utc::now().to_rfc3339();
            PreparedObservation::from_provider(
                target.identity.clone(),
                attempted_at,
                provider.observe(&target.identity),
            )
        })
        .collect::<Vec<_>>();
    let transaction = RequestTransaction::begin(db_path)?;
    let changed = transaction.database().connection().execute(
        "UPDATE project_flow_prepared_reads
         SET provider_results_json = ?5, state = 'ready'
         WHERE request_id = ?1 AND request_hash = ?2 AND state = 'prepared'
           AND owner_instance_id = ?3 AND owner_pid = ?4
           AND owner_process_start_id = ?6",
        params![
            request_id,
            hash,
            owner.instance_id,
            owner.pid,
            serde_json::to_string(&observations)?,
            owner.process_start_id
        ],
    )?;
    if changed != 1 {
        return Err(project_flow_precondition(
            "project_flow.prepared_owner_changed",
            "project-flow prepared read ownership changed before provider results were stored",
        ));
    }
    transaction.commit()?;
    Ok(PreparedRead::Ready {
        targets,
        observations,
        owner: owner.clone(),
    })
}

fn complete_prepared_read(
    conn: &Connection,
    request_id: &str,
    hash: &str,
    completed_at: &str,
    status: &ProjectFlowStatus,
) -> Result<()> {
    let changed = conn.execute(
        "UPDATE project_flow_prepared_reads
         SET state = 'completed', completed_at = ?3, result_json = ?4
         WHERE request_id = ?1 AND request_hash = ?2 AND state = 'ready'",
        params![
            request_id,
            hash,
            completed_at,
            serde_json::to_string(status)?
        ],
    )?;
    if changed != 1 {
        return Err(project_flow_precondition(
            "project_flow.prepared_completion_conflict",
            "project-flow prepared read could not be completed exactly once",
        ));
    }
    Ok(())
}

fn record_observation(
    conn: &Connection,
    artifact_id: i64,
    observation: &Result<ProviderObservation, ProviderFailure>,
    attempted_at: &str,
) -> Result<()> {
    match observation {
        Ok(observation) => {
            if let Some(id) = observation_id(conn, artifact_id)? {
                conn.execute(
                    "UPDATE project_flow_pull_request_observations
                     SET title = ?2, lifecycle = ?3, head_oid = ?4, review_state = ?5,
                         checks_state = ?6, last_success_at = ?7, last_attempt_at = ?7,
                         last_error = NULL
                     WHERE id = ?1 AND (last_attempt_at IS NULL OR last_attempt_at <= ?7)",
                    params![
                        id,
                        observation.title,
                        observation.lifecycle,
                        observation.head_oid,
                        observation.review_state,
                        observation.checks_state,
                        attempted_at
                    ],
                )?;
            } else {
                conn.execute(
                    "INSERT INTO project_flow_pull_request_observations (
                         artifact_id, title, lifecycle, head_oid, review_state, checks_state,
                         last_success_at, last_attempt_at, last_error
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, NULL)",
                    params![
                        artifact_id,
                        observation.title,
                        observation.lifecycle,
                        observation.head_oid,
                        observation.review_state,
                        observation.checks_state,
                        attempted_at
                    ],
                )?;
            }
        }
        Err(error) => {
            let message = format!("{}: {}", error.class, error.message);
            if let Some(id) = observation_id(conn, artifact_id)? {
                conn.execute(
                    "UPDATE project_flow_pull_request_observations
                     SET last_attempt_at = ?2, last_error = ?3
                     WHERE id = ?1 AND (last_attempt_at IS NULL OR last_attempt_at <= ?2)",
                    params![id, attempted_at, message],
                )?;
            } else {
                conn.execute(
                    "INSERT INTO project_flow_pull_request_observations (
                         artifact_id, last_attempt_at, last_error
                     ) VALUES (?1, ?2, ?3)",
                    params![artifact_id, attempted_at, message],
                )?;
            }
        }
    }
    Ok(())
}

fn observation_id(conn: &Connection, artifact_id: i64) -> Result<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT id FROM project_flow_pull_request_observations_data WHERE artifact_id = ?1",
            [artifact_id],
            |row| row.get(0),
        )
        .optional()?)
}

fn ensure_artifact(conn: &Connection, identity: &PullRequestIdentity) -> Result<i64> {
    let existing = conn
        .query_row(
            "SELECT id FROM project_flow_pull_requests_data
             WHERE provider = ?1 AND repository = ?2 AND number = ?3",
            params![identity.provider, identity.repository, identity.number],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    if let Some(id) = existing {
        conn.execute(
            "UPDATE project_flow_pull_requests SET url = ?2 WHERE id = ?1",
            params![id, identity.url],
        )?;
        return Ok(id);
    }
    let text_id = ulid::Ulid::new().to_string().to_lowercase();
    conn.execute(
        "INSERT INTO project_flow_pull_requests(text_id, provider, repository, number, url)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            text_id,
            identity.provider,
            identity.repository,
            identity.number,
            identity.url
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

fn campaign_artifact_id(
    conn: &Connection,
    campaign: &str,
    identity: &PullRequestIdentity,
) -> Result<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT artifact.id FROM project_flow_pull_requests_data artifact
             JOIN phase_pull_request_relations_data relation ON relation.artifact_id = artifact.id
             JOIN phases_data phase ON phase.id = relation.phase_id
             WHERE phase.text_id = ?1 AND artifact.provider = ?2
               AND artifact.repository = ?3 AND artifact.number = ?4",
            params![
                campaign,
                identity.provider,
                identity.repository,
                identity.number
            ],
            |row| row.get(0),
        )
        .optional()?)
}

fn upsert_campaign_delivery_relation(
    conn: &Connection,
    campaign: &str,
    previous_artifact_id: Option<i64>,
    canonical_identity: &PullRequestIdentity,
    role: &str,
) -> Result<i64> {
    let canonical_artifact_id = ensure_artifact(conn, canonical_identity)?;
    let canonical_relation_id = conn
        .query_row(
            "SELECT relation.id
             FROM phase_pull_request_relations_data relation
             JOIN phases_data phase ON phase.id = relation.phase_id
             WHERE phase.text_id = ?1 AND relation.artifact_id = ?2",
            params![campaign, canonical_artifact_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    if let Some(relation_id) = canonical_relation_id {
        conn.execute(
            "UPDATE phase_pull_request_relations
             SET role = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?1",
            params![relation_id, role],
        )?;
    } else {
        conn.execute(
            "INSERT INTO phase_pull_request_relations(phase_id, artifact_id, role)
             VALUES((SELECT id FROM phases_data WHERE text_id = ?1), ?2, ?3)",
            params![campaign, canonical_artifact_id, role],
        )?;
    }
    if let Some(previous_artifact_id) =
        previous_artifact_id.filter(|artifact_id| *artifact_id != canonical_artifact_id)
    {
        conn.execute(
            "DELETE FROM phase_pull_request_relations
             WHERE phase_id = (SELECT id FROM phases_data WHERE text_id = ?1)
               AND artifact_id = ?2",
            params![campaign, previous_artifact_id],
        )?;
        let remaining: i64 = conn.query_row(
            "SELECT COUNT(*) FROM phase_pull_request_relations_data WHERE artifact_id = ?1",
            [previous_artifact_id],
            |row| row.get(0),
        )?;
        if remaining == 0 {
            conn.execute(
                "DELETE FROM project_flow_pull_requests WHERE id = ?1",
                [previous_artifact_id],
            )?;
        }
    }
    Ok(canonical_artifact_id)
}

pub fn attach_pr_with_provider(
    db_path: &Path,
    request_id: &str,
    campaign: &str,
    identity: PullRequestIdentity,
    role: DeliveryRole,
    provider: &dyn PullRequestProvider,
) -> Result<ProjectFlowStatus> {
    let owner = direct_process_owner()?;
    prepare_pr_attachment(
        db_path,
        request_id,
        campaign,
        identity.clone(),
        role,
        &owner,
        provider,
    )?;
    let transaction = RequestTransaction::begin(db_path)?;
    let status = finalize_pr_attachment(db_path, request_id, campaign, identity, role)?;
    transaction.commit()?;
    Ok(status)
}

fn direct_process_owner() -> Result<DaemonOwnerIdentity> {
    crate::daemon_outcomes::direct_prepared_read_owner()
}

pub(crate) fn prepare_pr_attachment(
    db_path: &Path,
    request_id: &str,
    campaign: &str,
    identity: PullRequestIdentity,
    role: DeliveryRole,
    owner: &DaemonOwnerIdentity,
    provider: &dyn PullRequestProvider,
) -> Result<()> {
    let db = open_request_database(db_path)?;
    let campaign = resolve_campaign(db.connection(), campaign)?;
    drop(db);
    let target = PreparedTarget {
        identity,
        role: role.as_str().to_string(),
    };
    let payload = serde_json::to_string(&("pr.attach", &campaign, &target))?;
    prepare_external_read(
        db_path,
        request_id,
        &payload,
        &campaign,
        owner,
        |_| Ok(vec![target]),
        provider,
    )?;
    Ok(())
}

pub(crate) fn finalize_pr_attachment(
    db_path: &Path,
    request_id: &str,
    campaign: &str,
    identity: PullRequestIdentity,
    role: DeliveryRole,
) -> Result<ProjectFlowStatus> {
    let db = open_request_database(db_path)?;
    let campaign = resolve_campaign(db.connection(), campaign)?;
    let target = PreparedTarget {
        identity: identity.clone(),
        role: role.as_str().to_string(),
    };
    let payload = serde_json::to_string(&("pr.attach", &campaign, &target))?;
    let hash = blake3::hash(payload.as_bytes()).to_hex().to_string();
    let prepared = read_prepared(db.connection(), request_id, &hash, &payload, &campaign)?
        .ok_or_else(|| {
            project_flow_precondition(
                "project_flow.prepared_read_missing",
                "project-flow provider read was not prepared",
            )
        })?;
    let (targets, observations) = match prepared {
        PreparedRead::Completed(result) => return Ok(result),
        PreparedRead::Ready {
            targets,
            observations,
            ..
        } => (targets, observations),
        PreparedRead::Prepared { .. } => {
            return Err(project_flow_precondition(
                "project_flow.prepared_read_in_progress",
                "project-flow provider read is still in progress",
            ));
        }
        PreparedRead::Terminalizing { .. } => {
            return Err(project_flow_precondition(
                "project_flow.prepared_terminal_outcome_pending",
                "project-flow terminal outcome persistence is still in progress",
            ));
        }
    };
    let target = targets.into_iter().next().ok_or_else(|| {
        project_flow_precondition(
            "project_flow.prepared_target_missing",
            "prepared PR attachment omitted its target",
        )
    })?;
    if target.role != role.as_str() || target.identity != identity {
        return Err(project_flow_precondition(
            "project_flow.prepared_input_changed",
            "prepared PR attachment does not match the requested relationship",
        ));
    }
    let observation = observations.into_iter().next().ok_or_else(|| {
        project_flow_precondition(
            "project_flow.prepared_observation_missing",
            "prepared PR attachment omitted its provider result",
        )
    })?;
    if observation.identity != identity {
        return Err(project_flow_precondition(
            "project_flow.prepared_input_changed",
            "prepared PR observation does not match the requested pull request",
        ));
    }
    let conn = db.connection();
    resolve_campaign(conn, &campaign)?;
    let canonical_identity = observation
        .observation
        .as_ref()
        .map(|observation| &observation.identity)
        .unwrap_or(&identity);
    let requested_artifact_id = campaign_artifact_id(conn, &campaign, &identity)?;
    let artifact_id = upsert_campaign_delivery_relation(
        conn,
        &campaign,
        requested_artifact_id,
        canonical_identity,
        role.as_str(),
    )?;
    record_observation(
        conn,
        artifact_id,
        &observation.result(),
        &observation.attempted_at,
    )?;
    let status = status_with_connection(conn, &campaign)?;
    complete_prepared_read(conn, request_id, &hash, &observation.attempted_at, &status)?;
    Ok(status)
}

pub fn detach_pr(db_path: &Path, campaign: &str, identity: &PullRequestIdentity) -> Result<bool> {
    let db = open_request_database(db_path)?;
    let conn = db.connection();
    let campaign = resolve_campaign(conn, campaign)?;
    let ids = conn
        .query_row(
            "SELECT relation.id, artifact.id
         FROM phase_pull_request_relations_data relation
         JOIN phases_data phase ON phase.id = relation.phase_id
         JOIN project_flow_pull_requests_data artifact ON artifact.id = relation.artifact_id
         WHERE phase.text_id = ?1 AND artifact.provider = ?2
           AND artifact.repository = ?3 AND artifact.number = ?4",
            params![
                campaign,
                identity.provider,
                identity.repository,
                identity.number
            ],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()?;
    let Some((relation_id, artifact_id)) = ids else {
        return Ok(false);
    };
    conn.execute(
        "DELETE FROM phase_pull_request_relations WHERE id = ?1",
        [relation_id],
    )?;
    let remaining: i64 = conn.query_row(
        "SELECT COUNT(*) FROM phase_pull_request_relations_data WHERE artifact_id = ?1",
        [artifact_id],
        |row| row.get(0),
    )?;
    if remaining == 0 {
        conn.execute(
            "DELETE FROM project_flow_pull_requests WHERE id = ?1",
            [artifact_id],
        )?;
    }
    Ok(true)
}

pub fn refresh_with_provider(
    db_path: &Path,
    request_id: &str,
    campaign: &str,
    provider: &dyn PullRequestProvider,
) -> Result<ProjectFlowStatus> {
    let owner = direct_process_owner()?;
    prepare_refresh(db_path, request_id, campaign, &owner, provider)?;
    let transaction = RequestTransaction::begin(db_path)?;
    let status = finalize_refresh(db_path, request_id, campaign)?;
    transaction.commit()?;
    Ok(status)
}

pub(crate) fn prepare_refresh(
    db_path: &Path,
    request_id: &str,
    campaign: &str,
    owner: &DaemonOwnerIdentity,
    provider: &dyn PullRequestProvider,
) -> Result<()> {
    let db = open_request_database(db_path)?;
    let campaign = resolve_campaign(db.connection(), campaign)?;
    drop(db);
    let payload = serde_json::to_string(&("refresh", &campaign))?;
    prepare_external_read(
        db_path,
        request_id,
        &payload,
        &campaign,
        owner,
        |conn| {
            let mut stmt = conn.prepare(
                "SELECT artifact.provider, artifact.repository, artifact.number, artifact.url,
                        relation.role
                 FROM phase_pull_request_relations_data relation
                 JOIN phases_data phase ON phase.id = relation.phase_id
                 JOIN project_flow_pull_requests_data artifact ON artifact.id = relation.artifact_id
                 WHERE phase.text_id = ?1
                 ORDER BY artifact.provider, artifact.repository, artifact.number",
            )?;
            Ok(stmt
                .query_map([&campaign], |row| {
                    Ok(PreparedTarget {
                        identity: PullRequestIdentity {
                            provider: row.get(0)?,
                            repository: row.get(1)?,
                            number: row.get(2)?,
                            url: row.get(3)?,
                        },
                        role: row.get(4)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?)
        },
        provider,
    )?;
    Ok(())
}

pub(crate) fn finalize_refresh(
    db_path: &Path,
    request_id: &str,
    campaign: &str,
) -> Result<ProjectFlowStatus> {
    let db = open_request_database(db_path)?;
    let campaign = resolve_campaign(db.connection(), campaign)?;
    let payload = serde_json::to_string(&("refresh", &campaign))?;
    let hash = blake3::hash(payload.as_bytes()).to_hex().to_string();
    let prepared = read_prepared(db.connection(), request_id, &hash, &payload, &campaign)?
        .ok_or_else(|| {
            project_flow_precondition(
                "project_flow.prepared_read_missing",
                "project-flow provider read was not prepared",
            )
        })?;
    let (targets, observations) = match prepared {
        PreparedRead::Completed(result) => return Ok(result),
        PreparedRead::Ready {
            targets,
            observations,
            ..
        } => (targets, observations),
        PreparedRead::Prepared { .. } => {
            return Err(project_flow_precondition(
                "project_flow.prepared_read_in_progress",
                "project-flow provider read is still in progress",
            ));
        }
        PreparedRead::Terminalizing { .. } => {
            return Err(project_flow_precondition(
                "project_flow.prepared_terminal_outcome_pending",
                "project-flow terminal outcome persistence is still in progress",
            ));
        }
    };
    if targets.len() != observations.len() {
        return Err(project_flow_precondition(
            "project_flow.prepared_input_changed",
            "prepared project-flow targets and provider results do not align",
        ));
    }
    let conn = db.connection();
    resolve_campaign(conn, &campaign)?;
    for (target, observation) in targets.iter().zip(&observations) {
        if observation.identity != target.identity {
            return Err(project_flow_precondition(
                "project_flow.prepared_input_changed",
                "prepared project-flow provider result changed identity",
            ));
        }
        let (artifact_id, current_role): (i64, String) = conn
            .query_row(
                "SELECT artifact.id, relation.role FROM project_flow_pull_requests_data artifact
             JOIN phase_pull_request_relations_data relation ON relation.artifact_id = artifact.id
             JOIN phases_data phase ON phase.id = relation.phase_id
             WHERE phase.text_id = ?1 AND artifact.provider = ?2
               AND artifact.repository = ?3 AND artifact.number = ?4",
                params![
                    campaign,
                    target.identity.provider,
                    target.identity.repository,
                    target.identity.number
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| {
                project_flow_precondition(
                    "project_flow.prepared_input_changed",
                    "prepared project-flow relationship no longer exists",
                )
            })?;
        if current_role != target.role {
            return Err(project_flow_precondition(
                "project_flow.prepared_input_changed",
                "prepared project-flow relationship changed delivery role",
            ));
        }
        let canonical_identity = observation
            .observation
            .as_ref()
            .map(|observation| &observation.identity)
            .unwrap_or(&target.identity);
        let canonical_artifact_id = upsert_campaign_delivery_relation(
            conn,
            &campaign,
            Some(artifact_id),
            canonical_identity,
            &target.role,
        )?;
        record_observation(
            conn,
            canonical_artifact_id,
            &observation.result(),
            &observation.attempted_at,
        )?;
    }
    let status = status_with_connection(conn, &campaign)?;
    let completed_at = Utc::now().to_rfc3339();
    complete_prepared_read(conn, request_id, &hash, &completed_at, &status)?;
    Ok(status)
}

pub fn status(db_path: &Path, campaign: &str) -> Result<ProjectFlowStatus> {
    let db = open_request_database(db_path)?;
    let campaign = resolve_campaign(db.connection(), campaign)?;
    status_with_connection(db.connection(), &campaign)
}

fn status_with_connection(conn: &Connection, campaign: &str) -> Result<ProjectFlowStatus> {
    let (objectives, mut diagnostics) = campaign_rfc_objectives_with_connection(conn, campaign)?;
    let phase_id: i64 = conn.query_row(
        "SELECT id FROM phases WHERE text_id = ?1",
        [campaign],
        |row| row.get(0),
    )?;

    let mut stmt = conn.prepare(
        "SELECT artifact.provider, artifact.repository, artifact.number, artifact.url, relation.role,
                observation.title, observation.lifecycle, observation.head_oid,
                observation.review_state, observation.checks_state,
                observation.last_success_at, observation.last_attempt_at, observation.last_error
         FROM phase_pull_request_relations relation
         JOIN project_flow_pull_requests artifact ON artifact.id = relation.artifact_id
         LEFT JOIN project_flow_pull_request_observations observation ON observation.artifact_id = artifact.id
         WHERE relation.phase_id = ?1
         ORDER BY relation.role, artifact.provider, artifact.repository, artifact.number",
    )?;
    let pull_requests = stmt
        .query_map([phase_id], |row| {
            Ok(PullRequestView {
                identity: PullRequestIdentity {
                    provider: row.get(0)?,
                    repository: row.get(1)?,
                    number: row.get(2)?,
                    url: row.get(3)?,
                },
                role: row.get(4)?,
                title: row.get(5)?,
                lifecycle: row.get(6)?,
                head_oid: row.get(7)?,
                review_state: row.get(8)?,
                checks_state: row.get(9)?,
                last_success_at: row.get(10)?,
                last_attempt_at: row.get(11)?,
                last_error: row.get(12)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    diagnostics.sort();
    diagnostics.dedup();
    Ok(ProjectFlowStatus {
        campaign_id: campaign.to_string(),
        rfc_objectives: objectives,
        pull_requests,
        diagnostics,
    })
}

pub(crate) fn campaign_rfc_objectives(
    db_path: &Path,
    campaign: &str,
) -> Result<(Vec<RfcObjectiveView>, Vec<String>)> {
    let db = open_request_database(db_path)?;
    let campaign = resolve_campaign(db.connection(), campaign)?;
    campaign_rfc_objectives_with_connection(db.connection(), &campaign)
}

pub(crate) fn campaign_rfc_objectives_with_effective_rfcs(
    db_path: &Path,
    campaign: &str,
    effective_rfcs: &[crate::rfc::EffectiveRfcRecord],
) -> Result<(Vec<RfcObjectiveView>, Vec<String>)> {
    let (mut objectives, mut diagnostics) = campaign_rfc_objectives(db_path, campaign)?;
    let effective = effective_rfcs
        .iter()
        .map(|effective| (effective.record.text_id.as_str(), &effective.record))
        .collect::<std::collections::HashMap<_, _>>();
    for objective in &mut objectives {
        let Some(record) = effective.get(objective.rfc_ulid.as_str()) else {
            continue;
        };
        let (lifecycle, superseded_by) = effective_rfc_lifecycle(record);
        objective.rfc_number = record.rfc_number;
        objective.title.clone_from(&record.title);
        objective.current_stage = Some(record.stage);
        objective.lifecycle = Some(lifecycle);
        objective.superseded_by = superseded_by;
        objective.diagnostic = None;
    }
    diagnostics.retain(|diagnostic| {
        !objectives.iter().any(|objective| {
            objective.current_stage.is_some()
                && diagnostic
                    == &format!("project_flow.rfc_identity_missing: {}", objective.rfc_ulid)
        })
    });
    objectives.sort_by(|left, right| {
        objective_relation_priority(&left.relation)
            .cmp(&objective_relation_priority(&right.relation))
            .then(left.rfc_number.cmp(&right.rfc_number))
            .then(left.rfc_ulid.cmp(&right.rfc_ulid))
    });
    Ok((objectives, diagnostics))
}

fn campaign_rfc_objectives_with_connection(
    conn: &Connection,
    campaign: &str,
) -> Result<(Vec<RfcObjectiveView>, Vec<String>)> {
    let mut diagnostics = Vec::new();
    let phase_id: i64 = conn.query_row(
        "SELECT id FROM phases WHERE text_id = ?1",
        [campaign],
        |row| row.get(0),
    )?;
    let current_rfcs = load_rfc_map(conn)?;
    let mut stmt = conn.prepare(
        "SELECT objective.rfc_ulid, objective.rfc_number_snapshot, objective.rfc_title_snapshot,
                objective.observed_stage, objective.target_stage, objective.relation
         FROM campaign_rfc_objectives objective
         WHERE objective.phase_id = ?1 ORDER BY objective.rfc_number_snapshot, objective.rfc_ulid",
    )?;
    let typed_rows = stmt
        .query_map([phase_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<u8>>(3)?,
                row.get::<_, Option<u8>>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut typed_ulids = std::collections::HashSet::new();
    let mut typed_numbers = std::collections::HashSet::new();
    let mut objectives = Vec::new();
    for (ulid, number_snapshot, title_snapshot, observed_stage, target_stage, relation) in
        typed_rows
    {
        typed_ulids.insert(ulid.clone());
        typed_numbers.insert(number_snapshot);
        if let Some(current) = current_rfcs.get(&ulid) {
            typed_numbers.insert(current.rfc_number);
            let (lifecycle, superseded_by) = effective_rfc_lifecycle(current);
            objectives.push(RfcObjectiveView {
                rfc_ulid: ulid,
                rfc_number: current.rfc_number,
                title: current.title.clone(),
                observed_stage,
                current_stage: Some(current.stage),
                lifecycle: Some(lifecycle),
                superseded_by,
                target_stage,
                relation,
                source: "typed".to_string(),
                diagnostic: None,
            });
        } else {
            let diagnostic = format!("project_flow.rfc_identity_missing: {ulid}");
            objectives.push(RfcObjectiveView {
                rfc_ulid: ulid,
                rfc_number: number_snapshot,
                title: title_snapshot,
                observed_stage,
                current_stage: None,
                lifecycle: None,
                superseded_by: None,
                target_stage,
                relation,
                source: "typed".to_string(),
                diagnostic: Some(diagnostic.clone()),
            });
            diagnostics.push(diagnostic);
        }
    }
    append_legacy_objectives(
        conn,
        phase_id,
        &current_rfcs,
        &typed_ulids,
        &typed_numbers,
        &mut objectives,
        &mut diagnostics,
    )?;
    objectives.sort_by(|left, right| {
        objective_relation_priority(&left.relation)
            .cmp(&objective_relation_priority(&right.relation))
            .then(left.rfc_number.cmp(&right.rfc_number))
            .then(left.rfc_ulid.cmp(&right.rfc_ulid))
    });
    diagnostics.sort();
    diagnostics.dedup();
    Ok((objectives, diagnostics))
}

fn load_rfc_map(conn: &Connection) -> Result<std::collections::HashMap<String, RfcRecord>> {
    let mut stmt = conn.prepare(
        "SELECT text_id, rfc_number, title, stage, status, feature, slug, file_path,
                superseded_by, supersedes, withdrawal_reason, archived_reason, consolidated_into
         FROM rfcs",
    )?;
    Ok(stmt
        .query_map([], |row| {
            Ok(RfcRecord {
                text_id: row.get(0)?,
                rfc_number: row.get(1)?,
                title: row.get(2)?,
                stage: row.get(3)?,
                status: row.get(4)?,
                feature: row.get(5)?,
                slug: row.get(6)?,
                file_path: row.get(7)?,
                superseded_by: row.get(8)?,
                supersedes: row.get(9)?,
                withdrawal_reason: row.get(10)?,
                archived_reason: row.get(11)?,
                consolidated_into: row.get(12)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|rfc| (rfc.text_id.clone(), rfc))
        .collect())
}

fn append_legacy_objectives(
    conn: &Connection,
    phase_id: i64,
    current: &std::collections::HashMap<String, RfcRecord>,
    typed_ulids: &std::collections::HashSet<String>,
    typed_numbers: &std::collections::HashSet<i64>,
    objectives: &mut Vec<RfcObjectiveView>,
    diagnostics: &mut Vec<String>,
) -> Result<()> {
    let mut candidates = Vec::<(String, Option<u8>, String, String)>::new();
    let mut phase_stmt =
        conn.prepare("SELECT rfc_id, target, relation FROM phase_rfcs WHERE phase_id = ?1")?;
    candidates.extend(
        phase_stmt
            .query_map([phase_id], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    "legacy_phase".to_string(),
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?,
    );
    let mut goal_stmt = conn.prepare(
        "SELECT rfc, target_stage FROM goals
         WHERE phase_id = ?1 AND rfc IS NOT NULL
         ORDER BY text_id",
    )?;
    candidates.extend(
        goal_stmt
            .query_map([phase_id], |row| {
                let target = row.get::<_, Option<u8>>(1)?;
                Ok((
                    row.get(0)?,
                    target,
                    if target.is_some() {
                        "driving"
                    } else {
                        "related"
                    }
                    .to_string(),
                    "legacy_goal".to_string(),
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?,
    );
    let mut legacy_targets = std::collections::HashMap::<i64, Option<u8>>::new();
    for (number, target, relation, source) in candidates {
        let Ok(number_value) = number.parse::<i64>() else {
            diagnostics.push(format!("project_flow.legacy_rfc_invalid: {number}"));
            continue;
        };
        if let Some(previous) = legacy_targets.insert(number_value, target)
            && previous != target
        {
            diagnostics.push(format!(
                "project_flow.legacy_rfc_target_conflict: {number_value} ({previous:?} vs {target:?})"
            ));
        }
        if typed_numbers.contains(&number_value) {
            continue;
        }
        let matches = current
            .values()
            .filter(|rfc| rfc.rfc_number == number_value)
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [rfc]
                if !typed_ulids.contains(&rfc.text_id)
                    && !objectives
                        .iter()
                        .any(|objective| objective.rfc_ulid == rfc.text_id) =>
            {
                let (lifecycle, superseded_by) = effective_rfc_lifecycle(rfc);
                objectives.push(RfcObjectiveView {
                    rfc_ulid: rfc.text_id.clone(),
                    rfc_number: rfc.rfc_number,
                    title: rfc.title.clone(),
                    observed_stage: None,
                    current_stage: Some(rfc.stage),
                    lifecycle: Some(lifecycle),
                    superseded_by,
                    target_stage: target,
                    relation,
                    source,
                    diagnostic: None,
                });
            }
            [] => diagnostics.push(format!("project_flow.legacy_rfc_missing: {number}")),
            [_] => {}
            _ => diagnostics.push(format!("project_flow.rfc_ambiguous: {number}")),
        }
    }
    Ok(())
}

fn objective_relation_priority(relation: &str) -> u8 {
    match relation {
        "drives" => 0,
        "implements" => 1,
        "validates" => 2,
        _ => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::SqliteWriter;
    use crate::rfc::{EffectiveRfcRecord, RfcViewProvenance};
    use std::cell::Cell;

    #[derive(Debug)]
    struct TimedOutGh;

    impl GhProcess for TimedOutGh {
        fn output(&self, _args: &[&str]) -> std::io::Result<Output> {
            Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "bounded timeout",
            ))
        }
    }

    fn fixture() -> (tempfile::TempDir, std::path::PathBuf, String) {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("exo.db");
        let writer = SqliteWriter::open(&path).unwrap();
        let epoch = writer.add_epoch("Epoch", Some("epoch"), &[]).unwrap();
        let campaign = writer
            .add_phase(
                &epoch,
                "Campaign",
                "regular",
                Some("campaign"),
                &["campaign-alias".to_string()],
            )
            .unwrap();
        writer.database().connection().execute(
            "INSERT INTO rfcs(text_id, rfc_number, title, stage, status, slug, file_path)
             VALUES('01rfc000000000000000000001', 10207, 'Project flow', 2, 'active', 'project-flow', 'docs/rfcs/stage-2/10207.md')",
            [],
        ).unwrap();
        (temp, path, campaign)
    }

    #[derive(Debug)]
    struct FakeProvider {
        calls: Cell<usize>,
        result: Result<ProviderObservation, ProviderFailure>,
    }

    impl PullRequestProvider for FakeProvider {
        fn observe(
            &self,
            _identity: &PullRequestIdentity,
        ) -> Result<ProviderObservation, ProviderFailure> {
            self.calls.set(self.calls.get() + 1);
            self.result.clone()
        }
    }

    fn observation(title: &str) -> ProviderObservation {
        ProviderObservation {
            identity: PullRequestIdentity::parse("wycats/exo2#75").unwrap(),
            title: title.to_string(),
            lifecycle: "open".to_string(),
            head_oid: Some("abc".to_string()),
            review_state: "approved".to_string(),
            checks_state: "passing".to_string(),
        }
    }

    #[test]
    fn github_pull_request_identity_accepts_canonical_names_and_normalizes_case() {
        for (selector, expected) in [
            ("Wycats/Exo2#75", "wycats/exo2"),
            ("owner-name/repo.name_2#7", "owner-name/repo.name_2"),
            ("https://github.com/Wycats/Exo2/pull/75", "wycats/exo2"),
        ] {
            let identity = PullRequestIdentity::parse(selector).unwrap();
            assert_eq!(identity.repository, expected, "{selector}");
            assert_eq!(
                identity.url,
                format!("https://github.com/{expected}/pull/{}", identity.number)
            );
        }
    }

    #[test]
    fn github_pull_request_identity_rejects_noncanonical_input() {
        for selector in [
            "wycats/exo2?tab=readme#75",
            "wycats/exo2#75#fragment",
            "wycats/exo2 #75",
            "wycats/exo2\n#75",
            "wycats/exo two#75",
            "wycats/exo2/extra#75",
            "wycats_/exo2#75",
            "-wycats/exo2#75",
            "wycats-/exo2#75",
            "https://github.com/wycats/exo2/pull/75?tab=checks",
            "https://github.com/wycats/exo2/pull/75#discussion",
            "https://github.com/wycats/exo2/pulls/75",
            "https://github.com/wycats/exo2/pull/75/extra",
        ] {
            assert!(
                PullRequestIdentity::parse(selector).is_err(),
                "{selector} must be rejected"
            );
        }
    }

    #[test]
    fn github_observation_uses_the_provider_canonical_identity() {
        let observed = parse_github_observation(&serde_json::json!({
            "url": "https://github.com/New-Owner/Renamed-Repo/pull/75",
            "title": "Moved PR",
            "state": "OPEN",
            "mergedAt": null,
            "reviewDecision": "APPROVED",
            "statusCheckRollup": [],
        }))
        .unwrap();

        assert_eq!(observed.identity.repository, "new-owner/renamed-repo");
        assert_eq!(
            observed.identity.url,
            "https://github.com/new-owner/renamed-repo/pull/75"
        );
    }

    #[test]
    fn attachment_persists_the_provider_canonical_identity() {
        let (_temp, path, campaign) = fixture();
        let requested = PullRequestIdentity::parse("old-owner/old-repo#75").unwrap();
        let mut canonical = observation("Moved PR");
        canonical.identity = PullRequestIdentity::parse("new-owner/new-repo#75").unwrap();
        let provider = FakeProvider {
            calls: Cell::new(0),
            result: Ok(canonical),
        };

        let status = attach_pr_with_provider(
            &path,
            "canonical-provider-identity",
            &campaign,
            requested,
            DeliveryRole::Implements,
            &provider,
        )
        .unwrap();

        assert_eq!(status.pull_requests.len(), 1);
        assert_eq!(
            status.pull_requests[0].identity.repository,
            "new-owner/new-repo"
        );
        let old_count: i64 = SqliteWriter::open(&path)
            .unwrap()
            .database()
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM project_flow_pull_requests_data
                 WHERE repository = 'old-owner/old-repo'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(old_count, 0);
    }

    #[test]
    fn successful_retry_rehomes_a_failed_attachment_to_the_canonical_identity() {
        let (_temp, path, campaign) = fixture();
        let requested = PullRequestIdentity::parse("old-owner/old-repo#75").unwrap();
        let failed = attach_pr_with_provider(
            &path,
            "failed-provider-identity",
            &campaign,
            requested.clone(),
            DeliveryRole::Implements,
            &FakeProvider {
                calls: Cell::new(0),
                result: Err(ProviderFailure {
                    class: "not_found",
                    message: "repository moved".to_string(),
                }),
            },
        )
        .unwrap();
        assert_eq!(failed.pull_requests.len(), 1);
        assert_eq!(failed.pull_requests[0].identity, requested);

        let mut canonical = observation("Moved PR");
        canonical.identity = PullRequestIdentity::parse("new-owner/new-repo#75").unwrap();
        let recovered = attach_pr_with_provider(
            &path,
            "successful-provider-identity",
            &campaign,
            requested,
            DeliveryRole::Implements,
            &FakeProvider {
                calls: Cell::new(0),
                result: Ok(canonical),
            },
        )
        .unwrap();

        assert_eq!(recovered.pull_requests.len(), 1);
        assert_eq!(
            recovered.pull_requests[0].identity.repository,
            "new-owner/new-repo"
        );
        let artifact_count: i64 = SqliteWriter::open(&path)
            .unwrap()
            .database()
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM project_flow_pull_requests_data",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(artifact_count, 1);
    }

    #[test]
    fn refresh_rehomes_a_relation_to_the_provider_canonical_identity() {
        let (_temp, path, campaign) = fixture();
        let requested = PullRequestIdentity::parse("old-owner/old-repo#75").unwrap();
        let mut original = observation("Original PR");
        original.identity = requested.clone();
        attach_pr_with_provider(
            &path,
            "attach-original-identity",
            &campaign,
            requested,
            DeliveryRole::Implements,
            &FakeProvider {
                calls: Cell::new(0),
                result: Ok(original),
            },
        )
        .unwrap();
        let mut moved = observation("Moved PR");
        moved.identity = PullRequestIdentity::parse("new-owner/new-repo#75").unwrap();

        let status = refresh_with_provider(
            &path,
            "refresh-canonical-identity",
            &campaign,
            &FakeProvider {
                calls: Cell::new(0),
                result: Ok(moved),
            },
        )
        .unwrap();

        assert_eq!(status.pull_requests.len(), 1);
        assert_eq!(
            status.pull_requests[0].identity.repository,
            "new-owner/new-repo"
        );
    }

    #[test]
    fn typed_objectives_project_effective_terminal_lifecycle() {
        let (_temp, path, campaign) = fixture();
        attach_rfc(&path, &campaign, "10207", RfcRelation::Drives, Some(3)).unwrap();
        let writer = SqliteWriter::open(&path).unwrap();

        for (status_value, superseded_by, expected) in [
            ("withdrawn", None, "withdrawn"),
            ("archived", Some("10208"), "archived"),
            ("active", Some("10208"), "superseded"),
        ] {
            writer
                .database()
                .connection()
                .execute(
                    "UPDATE rfcs SET status = ?1, superseded_by = ?2 WHERE rfc_number = 10207",
                    params![status_value, superseded_by],
                )
                .unwrap();
            let status = status(&path, &campaign).unwrap();
            let objective = &status.rfc_objectives[0];
            assert_eq!(objective.lifecycle.as_deref(), Some(expected));
            assert_eq!(objective.superseded_by.as_deref(), superseded_by);
            assert!(!objective.can_advance());
        }
    }

    #[test]
    fn objective_motion_requires_live_identity_and_a_strictly_future_target() {
        let objective = |current_stage, lifecycle: Option<&str>, target_stage| RfcObjectiveView {
            rfc_ulid: "01rfc000000000000000000001".to_string(),
            rfc_number: 10207,
            title: "Project flow".to_string(),
            observed_stage: current_stage,
            current_stage,
            lifecycle: lifecycle.map(str::to_string),
            superseded_by: None,
            target_stage,
            relation: "drives".to_string(),
            source: "typed".to_string(),
            diagnostic: None,
        };

        assert_eq!(
            objective(Some(2), Some("active"), Some(3)).motion(),
            RfcObjectiveMotion::Advancing
        );
        assert_eq!(
            objective(Some(3), Some("active"), Some(3)).motion(),
            RfcObjectiveMotion::TargetReached
        );
        assert_eq!(
            objective(Some(4), Some("active"), None).motion(),
            RfcObjectiveMotion::Associated
        );
        assert_eq!(
            objective(Some(2), Some("active"), None).motion(),
            RfcObjectiveMotion::Associated
        );
        assert_eq!(
            objective(Some(2), None, Some(3)).motion(),
            RfcObjectiveMotion::Associated
        );
        assert_eq!(
            objective(Some(2), Some("withdrawn"), Some(3)).motion(),
            RfcObjectiveMotion::Terminal
        );
        assert_eq!(
            objective(None, None, Some(3)).motion(),
            RfcObjectiveMotion::IdentityMissing
        );
    }

    #[test]
    fn typed_objective_is_idempotent_and_legacy_update_does_not_remove_it() {
        let (_temp, path, campaign) = fixture();
        attach_rfc(
            &path,
            "campaign-alias",
            "10207",
            RfcRelation::Drives,
            Some(3),
        )
        .unwrap();
        let writer = SqliteWriter::open(&path).unwrap();
        let before: (String, String, u8, String) = writer
            .database()
            .connection()
            .query_row(
                "SELECT rfc_title_snapshot, created_at, observed_stage, updated_at
                 FROM campaign_rfc_objectives_data",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        attach_rfc(&path, &campaign, "10207", RfcRelation::Drives, Some(3)).unwrap();
        let unchanged: (String, String, u8, String) = writer
            .database()
            .connection()
            .query_row(
                "SELECT rfc_title_snapshot, created_at, observed_stage, updated_at
                 FROM campaign_rfc_objectives_data",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(unchanged, before, "exact reattachment must be a no-op");
        writer
            .database()
            .connection()
            .execute(
                "UPDATE rfcs SET title = 'Changed title', stage = 3 WHERE rfc_number = 10207",
                [],
            )
            .unwrap();
        let advanced_noop = attach_rfc(&path, &campaign, "10207", RfcRelation::Drives, Some(3))
            .expect("exact attachment remains idempotent after reaching its target");
        assert_eq!(advanced_noop.observed_stage, Some(2));
        assert_eq!(advanced_noop.current_stage, Some(3));
        let after_advance: (String, String, u8, String) = writer
            .database()
            .connection()
            .query_row(
                "SELECT rfc_title_snapshot, created_at, observed_stage, updated_at
                 FROM campaign_rfc_objectives_data",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            after_advance, before,
            "advanced exact reattachment is a no-op"
        );
        let relation_only_change =
            attach_rfc(&path, &campaign, "10207", RfcRelation::Validates, Some(3))
                .expect("an unchanged reached target permits a relationship correction");
        assert_eq!(relation_only_change.relation, "validates");
        assert_eq!(relation_only_change.target_stage, Some(3));
        attach_rfc(&path, &campaign, "10207", RfcRelation::Validates, Some(4)).unwrap();
        let after: (String, String, u8) = writer
            .database()
            .connection()
            .query_row(
                "SELECT rfc_title_snapshot, created_at, observed_stage
             FROM campaign_rfc_objectives_data",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(after, (before.0.clone(), before.1.clone(), before.2));
        writer
            .replace_phase_rfcs(&campaign, &["10207".to_string()])
            .unwrap();
        let count: i64 = writer
            .database()
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM campaign_rfc_objectives_data",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn rfc_targets_at_or_below_the_current_stage_are_typed_preconditions() {
        let (_temp, path, campaign) = fixture();
        let error = attach_rfc(&path, &campaign, "10207", RfcRelation::Drives, Some(2))
            .expect_err("new objective must target a future stage");
        let failure = error
            .downcast_ref::<ExoFailure>()
            .expect("typed project-flow precondition");
        assert_eq!(failure.error.code, ErrorCode::PreconditionFailed);
        assert_eq!(
            failure.error.details.as_ref().unwrap()["kind"],
            "project_flow.target_stage_not_future"
        );

        attach_rfc(&path, &campaign, "10207", RfcRelation::Drives, Some(3)).unwrap();
        let error = attach_rfc(&path, &campaign, "10207", RfcRelation::Drives, Some(2))
            .expect_err("updated objective must target a future stage");
        let failure = error
            .downcast_ref::<ExoFailure>()
            .expect("typed project-flow precondition");
        assert_eq!(failure.error.code, ErrorCode::PreconditionFailed);
        assert_eq!(
            failure.error.details.as_ref().unwrap()["kind"],
            "project_flow.target_stage_not_future"
        );
    }

    #[test]
    fn effective_workspace_rfc_view_overlays_campaign_objectives() {
        let (_temp, path, campaign) = fixture();
        attach_rfc(&path, &campaign, "10207", RfcRelation::Drives, Some(3)).unwrap();
        let effective = EffectiveRfcRecord {
            record: RfcRecord {
                text_id: "01rfc000000000000000000001".to_string(),
                rfc_number: 10207,
                title: "Workspace project flow".to_string(),
                stage: 3,
                status: "active".to_string(),
                feature: None,
                slug: "project-flow".to_string(),
                file_path: "docs/rfcs/stage-3/10207.md".to_string(),
                superseded_by: None,
                supersedes: None,
                withdrawal_reason: None,
                archived_reason: None,
                consolidated_into: None,
            },
            provenance: RfcViewProvenance {
                document_source: "workspace".to_string(),
                workspace_presence: "present".to_string(),
                canonical_presence: "present".to_string(),
                workspace_branch: Some("wycats/project-flow".to_string()),
                workspace_head: Some("abc123".to_string()),
                canonical_ref: Some("refs/heads/main".to_string()),
                canonical_head: Some("def456".to_string()),
                differs_from_canonical: true,
            },
        };

        let (objectives, diagnostics) =
            campaign_rfc_objectives_with_effective_rfcs(&path, &campaign, &[effective]).unwrap();
        assert_eq!(objectives[0].title, "Workspace project flow");
        assert_eq!(objectives[0].current_stage, Some(3));
        assert_eq!(objectives[0].motion(), RfcObjectiveMotion::TargetReached);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn github_unknown_or_missing_lifecycle_is_not_reported_as_closed() {
        for state in [
            serde_json::json!({"title": "PR"}),
            serde_json::json!({"title": "PR", "state": "UNKNOWN"}),
        ] {
            let error = parse_github_observation(&state).expect_err("unknown state must fail");
            assert_eq!(error.class, "invalid_response");
        }
    }

    #[test]
    fn github_check_rollup_distinguishes_check_runs_and_status_contexts() {
        let observation_with = |checks: Value| {
            parse_github_observation(&serde_json::json!({
                "url": "https://github.com/wycats/exo2/pull/75",
                "title": "PR",
                "state": "OPEN",
                "mergedAt": null,
                "reviewDecision": "APPROVED",
                "statusCheckRollup": checks,
            }))
            .unwrap()
        };

        for check in [
            serde_json::json!({
                "__typename": "CheckRun",
                "status": "COMPLETED",
                "conclusion": "SUCCESS"
            }),
            serde_json::json!({"__typename": "StatusContext", "state": "SUCCESS"}),
        ] {
            assert_eq!(
                observation_with(serde_json::json!([check])).checks_state,
                "passing"
            );
        }
        for check in [
            serde_json::json!({
                "__typename": "CheckRun",
                "status": "COMPLETED",
                "conclusion": "FAILURE"
            }),
            serde_json::json!({"__typename": "StatusContext", "state": "FAILURE"}),
        ] {
            assert_eq!(
                observation_with(serde_json::json!([check])).checks_state,
                "failing"
            );
        }
        for check in [
            serde_json::json!({
                "__typename": "CheckRun",
                "status": "IN_PROGRESS",
                "conclusion": null
            }),
            serde_json::json!({"__typename": "StatusContext", "state": "PENDING"}),
        ] {
            assert_eq!(
                observation_with(serde_json::json!([check])).checks_state,
                "pending"
            );
        }
        assert_eq!(
            observation_with(serde_json::json!([{
                "__typename": "FutureCheckType",
                "state": "SUCCESS"
            }]))
            .checks_state,
            "unknown"
        );

        for conclusion in [
            "FAILURE",
            "CANCELLED",
            "TIMED_OUT",
            "ACTION_REQUIRED",
            "STARTUP_FAILURE",
            "STALE",
        ] {
            assert_eq!(
                observation_with(serde_json::json!([{
                    "__typename": "CheckRun",
                    "status": "COMPLETED",
                    "conclusion": conclusion,
                }]))
                .checks_state,
                "failing",
                "{conclusion} is a terminal failure-like conclusion"
            );
        }
    }

    #[test]
    fn github_check_rollup_distinguishes_valid_empty_from_missing_or_malformed() {
        let observation = |rollup: Option<Value>| {
            let mut value = serde_json::json!({
                "url": "https://github.com/wycats/exo2/pull/75",
                "title": "PR",
                "state": "OPEN",
                "mergedAt": null,
                "reviewDecision": "APPROVED",
            });
            if let Some(rollup) = rollup {
                value["statusCheckRollup"] = rollup;
            }
            parse_github_observation(&value).unwrap()
        };

        assert_eq!(
            observation(Some(serde_json::json!([]))).checks_state,
            "none"
        );
        for rollup in [None, Some(Value::Null), Some(serde_json::json!({}))] {
            assert_eq!(observation(rollup).checks_state, "unknown");
        }
    }

    #[test]
    fn github_provider_failures_distinguish_auth_permission_and_not_found() {
        for (message, expected) in [
            (
                "authentication required: run gh auth login",
                "authentication",
            ),
            (
                "HTTP 403: Resource not accessible by integration",
                "permission",
            ),
            ("token has insufficient scope", "permission"),
            ("GraphQL: Could not resolve to a PullRequest", "not_found"),
            ("pull request not found (HTTP 404)", "not_found"),
            ("upstream connection reset", "provider_unavailable"),
        ] {
            assert_eq!(classify_github_failure(message), expected, "{message}");
        }
    }

    #[test]
    fn github_timeout_is_a_recoverable_provider_failure() {
        let provider = GithubProvider::new(TimedOutGh);
        let identity = PullRequestIdentity::parse("wycats/exo2#75").unwrap();
        let failure = provider
            .observe(&identity)
            .expect_err("provider should time out");
        assert_eq!(failure.class, "timeout");
    }

    #[test]
    fn duplicate_rfc_number_requires_exact_identity() {
        let (_temp, path, campaign) = fixture();
        let writer = SqliteWriter::open(&path).unwrap();
        writer.database().connection().execute(
            "INSERT INTO rfcs(text_id, rfc_number, title, stage, status, slug, file_path)
             VALUES('01rfc000000000000000000002', 10207, 'Other', 1, 'withdrawn', 'other', 'docs/rfcs/stage-1/10207-other.md')",
            [],
        ).unwrap();
        let error =
            attach_rfc(&path, &campaign, "10207", RfcRelation::Drives, Some(3)).unwrap_err();
        let failure = error.downcast_ref::<ExoFailure>().expect("typed ambiguity");
        assert_eq!(failure.error.code, ErrorCode::PreconditionFailed);
        assert_eq!(
            failure
                .error
                .details
                .as_ref()
                .and_then(|details| details["kind"].as_str()),
            Some("project_flow.rfc_ambiguous")
        );
        let details = failure.error.details.as_ref().unwrap();
        assert_eq!(details["candidates"].as_array().unwrap().len(), 2);
        assert_eq!(details["candidates"][1]["status"], "withdrawn");
        assert!(failure.error.message.contains("withdrawn"));
        attach_rfc(
            &path,
            &campaign,
            "01rfc000000000000000000001",
            RfcRelation::Drives,
            Some(3),
        )
        .unwrap();
    }

    #[test]
    fn prepared_takeover_probes_outside_lock_and_rejects_changed_owner() {
        let (_temp, path, campaign) = fixture();
        let identity = PullRequestIdentity::parse("wycats/exo2#10207").unwrap();
        let old_owner = DaemonOwnerIdentity {
            instance_id: "old".to_string(),
            pid: 101,
            process_start_id: "old-start".to_string(),
        };
        let replacement = DaemonOwnerIdentity {
            instance_id: "replacement".to_string(),
            pid: 202,
            process_start_id: "replacement-start".to_string(),
        };
        let provider = FakeProvider {
            calls: Cell::new(0),
            result: Ok(observation("Prepared")),
        };
        let payload = serde_json::to_string(&(
            "pr.attach",
            &campaign,
            PreparedTarget {
                identity,
                role: DeliveryRole::Implements.as_str().to_string(),
            },
        ))
        .unwrap();
        let request_id = "prepared-owner-cas";
        let hash = blake3::hash(payload.as_bytes()).to_hex().to_string();
        let transaction = RequestTransaction::begin(&path).unwrap();
        transaction
            .database()
            .connection()
            .execute(
                "INSERT INTO project_flow_prepared_reads (
                 request_id, request_hash, normalized_payload, phase_text_id, targets_json,
                 owner_instance_id, owner_pid, owner_process_start_id, recovery_class, state,
                 prepared_at
             ) VALUES (?1, ?2, ?3, ?4, '[]', ?5, ?6, ?7,
                       'prepared_external_read', 'prepared', ?8)",
                params![
                    request_id,
                    hash,
                    payload,
                    campaign,
                    old_owner.instance_id,
                    old_owner.pid,
                    old_owner.process_start_id,
                    Utc::now().to_rfc3339()
                ],
            )
            .unwrap();
        transaction.commit().unwrap();

        let error = match prepare_external_read_with_classifier(
            &path,
            request_id,
            &payload,
            &campaign,
            &replacement,
            |_| panic!("existing prepared bytes must be retained"),
            &provider,
            |_| {
                let writer = SqliteWriter::open(&path).expect("probe must run outside writer lock");
                writer
                    .database()
                    .connection()
                    .execute(
                        "UPDATE project_flow_prepared_reads
                     SET owner_instance_id = 'changed', owner_pid = 303,
                         owner_process_start_id = 'changed-start'
                     WHERE request_id = ?1",
                        [request_id],
                    )
                    .unwrap();
                DaemonOwnerState::Dead
            },
        ) {
            Ok(_) => panic!("changed owner must defeat takeover CAS"),
            Err(error) => error,
        };
        let failure = error
            .downcast_ref::<ExoFailure>()
            .expect("typed owner change");
        assert_eq!(
            failure.error.details.as_ref().unwrap()["kind"],
            "project_flow.prepared_owner_changed"
        );
        let owner: (String, u32, String) = SqliteWriter::open(&path)
            .unwrap()
            .database()
            .connection()
            .query_row(
                "SELECT owner_instance_id, owner_pid, owner_process_start_id
                 FROM project_flow_prepared_reads WHERE request_id = ?1",
                [request_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            owner,
            ("changed".to_string(), 303, "changed-start".to_string())
        );
        assert_eq!(provider.calls.get(), 0);
    }

    #[test]
    fn legacy_objectives_preserve_relation_and_report_target_conflicts() {
        let (_temp, path, campaign) = fixture();
        let writer = SqliteWriter::open(&path).unwrap();
        let conn = writer.database().connection();
        let phase_id: i64 = conn
            .query_row(
                "SELECT id FROM phases_data WHERE text_id = ?1",
                [&campaign],
                |row| row.get(0),
            )
            .unwrap();
        conn.execute(
            "INSERT INTO phase_rfcs_data(phase_id, rfc_id, target, relation)
             VALUES(?1, '10207', NULL, 'blocked')",
            [phase_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO goals_data(text_id, label, status, phase_id, kind, rfc, target_stage)
             VALUES('legacy-goal', 'Advance RFC', 'pending', ?1, 'regular', '010207', 3)",
            [phase_id],
        )
        .unwrap();

        let (objectives, diagnostics) = campaign_rfc_objectives(&path, &campaign).unwrap();
        assert_eq!(objectives.len(), 1);
        assert_eq!(objectives[0].relation, "blocked");
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.contains("project_flow.legacy_rfc_target_conflict"))
        );
    }

    #[test]
    fn legacy_goal_conflicts_choose_a_stable_representative() {
        let (_temp, path, campaign) = fixture();
        let writer = SqliteWriter::open(&path).unwrap();
        let conn = writer.database().connection();
        let phase_id: i64 = conn
            .query_row(
                "SELECT id FROM phases_data WHERE text_id = ?1",
                [&campaign],
                |row| row.get(0),
            )
            .unwrap();
        for (text_id, target_stage) in [("z-later", 4), ("a-first", 3)] {
            conn.execute(
                "INSERT INTO goals_data(text_id, label, status, phase_id, kind, rfc, target_stage)
                 VALUES(?1, ?1, 'pending', ?2, 'regular', '10207', ?3)",
                params![text_id, phase_id, target_stage],
            )
            .unwrap();
        }

        let (objectives, diagnostics) = campaign_rfc_objectives(&path, &campaign).unwrap();
        assert_eq!(objectives.len(), 1);
        assert_eq!(objectives[0].target_stage, Some(3));
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.contains("project_flow.legacy_rfc_target_conflict"))
        );
    }

    #[test]
    fn status_reads_project_motion_through_reactive_tables() {
        let (_temp, path, campaign) = fixture();
        attach_rfc(&path, &campaign, "10207", RfcRelation::Drives, Some(3)).unwrap();
        let provider = FakeProvider {
            calls: Cell::new(0),
            result: Ok(observation("Reactive delivery")),
        };
        attach_pr_with_provider(
            &path,
            "reactive-status",
            &campaign,
            PullRequestIdentity::parse("wycats/exo2#75").unwrap(),
            DeliveryRole::Implements,
            &provider,
        )
        .unwrap();

        let (result, trace) = exosuit_storage::TraceScope::run(|| status(&path, &campaign));
        result.unwrap();
        let sources = trace
            .dependencies
            .iter()
            .map(|dependency| dependency.cell_id.source_id.as_str())
            .collect::<std::collections::HashSet<_>>();
        for expected in [
            "campaign_rfc_objectives_data",
            "phase_pull_request_relations_data",
            "project_flow_pull_requests_data",
            "project_flow_pull_request_observations_data",
        ] {
            assert!(
                sources.contains(expected),
                "missing reactive dependency {expected}"
            );
        }
    }

    #[test]
    fn project_motion_orders_typed_relations_before_legacy_objectives() {
        let (_temp, path, campaign) = fixture();
        let writer = SqliteWriter::open(&path).unwrap();
        let conn = writer.database().connection();
        for (ulid, number, title) in [
            ("01rfc000000000000000000002", 10208, "Validation"),
            ("01rfc000000000000000000003", 10209, "Implementation"),
            ("01rfc000000000000000000004", 10206, "Legacy"),
        ] {
            conn.execute(
                "INSERT INTO rfcs(text_id, rfc_number, title, stage, status, slug, file_path)
                 VALUES(?1, ?2, ?3, 1, 'active', ?3, ?3)",
                params![ulid, number, title],
            )
            .unwrap();
        }
        attach_rfc(&path, &campaign, "10208", RfcRelation::Validates, Some(2)).unwrap();
        attach_rfc(&path, &campaign, "10209", RfcRelation::Implements, Some(2)).unwrap();
        let phase_id: i64 = conn
            .query_row(
                "SELECT id FROM phases_data WHERE text_id = ?1",
                [&campaign],
                |row| row.get(0),
            )
            .unwrap();
        conn.execute(
            "INSERT INTO phase_rfcs_data(phase_id, rfc_id, relation)
             VALUES(?1, '10206', 'related')",
            [phase_id],
        )
        .unwrap();

        let (objectives, _) = campaign_rfc_objectives(&path, &campaign).unwrap();
        assert_eq!(
            objectives
                .iter()
                .map(|objective| objective.relation.as_str())
                .collect::<Vec<_>>(),
            vec!["implements", "validates", "related"]
        );
    }

    #[test]
    fn older_provider_attempt_cannot_overwrite_a_newer_observation() {
        let (_temp, path, campaign) = fixture();
        let identity = PullRequestIdentity::parse("wycats/exo2#75").unwrap();
        let writer = SqliteWriter::open(&path).unwrap();
        let conn = writer.database().connection();
        let artifact_id = ensure_artifact(conn, &identity).unwrap();
        let mut newer = observation("Newer title");
        newer.review_state = "approved".to_string();
        record_observation(conn, artifact_id, &Ok(newer), "2026-09-03T12:05:00+00:00").unwrap();
        record_observation(
            conn,
            artifact_id,
            &Err(ProviderFailure {
                class: "provider_unavailable",
                message: "older failure".to_string(),
            }),
            "2026-09-03T12:00:00+00:00",
        )
        .unwrap();

        let stored: (String, String, Option<String>) = conn
            .query_row(
                "SELECT title, last_attempt_at, last_error
                 FROM project_flow_pull_request_observations_data WHERE artifact_id = ?1",
                [artifact_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(stored.0, "Newer title");
        assert_eq!(stored.1, "2026-09-03T12:05:00+00:00");
        assert_eq!(stored.2, None);
        drop(campaign);
    }

    #[test]
    fn refresh_rejects_a_delivery_role_change_after_provider_io() {
        let (_temp, path, campaign) = fixture();
        let identity = PullRequestIdentity::parse("wycats/exo2#75").unwrap();
        let provider = FakeProvider {
            calls: Cell::new(0),
            result: Ok(observation("Prepared refresh")),
        };
        attach_pr_with_provider(
            &path,
            "attach-before-role-change",
            &campaign,
            identity,
            DeliveryRole::Implements,
            &provider,
        )
        .unwrap();
        let owner = DaemonOwnerIdentity {
            instance_id: "role-change-owner".to_string(),
            pid: 303,
            process_start_id: "role-change-start".to_string(),
        };
        prepare_refresh(
            &path,
            "refresh-before-role-change",
            &campaign,
            &owner,
            &provider,
        )
        .unwrap();
        SqliteWriter::open(&path)
            .unwrap()
            .database()
            .connection()
            .execute(
                "UPDATE phase_pull_request_relations SET role = 'validates'",
                [],
            )
            .unwrap();

        let error = finalize_refresh(&path, "refresh-before-role-change", &campaign)
            .expect_err("changed delivery role must invalidate prepared provider evidence");
        let failure = error
            .downcast_ref::<ExoFailure>()
            .expect("typed prepared-input failure");
        assert_eq!(
            failure.error.details.as_ref().unwrap()["kind"],
            "project_flow.prepared_input_changed"
        );
    }

    #[test]
    fn status_retains_disconnected_typed_objective_from_establishment_snapshot() {
        let (_temp, path, campaign) = fixture();
        attach_rfc(
            &path,
            &campaign,
            "01rfc000000000000000000001",
            RfcRelation::Drives,
            Some(3),
        )
        .unwrap();
        let writer = SqliteWriter::open(&path).unwrap();
        let conn = writer.database().connection();
        conn.execute(
            "DELETE FROM rfcs WHERE text_id = '01rfc000000000000000000001'",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO rfcs(text_id, rfc_number, title, stage, status, slug, file_path)
             VALUES('01rfc000000000000000000002', 10207, 'Replacement title', 4, 'active',
                    'replacement', 'docs/rfcs/stage-4/10207-replacement.md')",
            [],
        )
        .unwrap();

        let status = status(&path, &campaign).unwrap();
        assert_eq!(status.rfc_objectives.len(), 1);
        let objective = &status.rfc_objectives[0];
        assert_eq!(objective.rfc_ulid, "01rfc000000000000000000001");
        assert_eq!(objective.rfc_number, 10207);
        assert_eq!(objective.title, "Project flow");
        assert_eq!(objective.current_stage, None);
        assert_eq!(objective.target_stage, Some(3));
        assert!(status.diagnostics.iter().any(|diagnostic| diagnostic
            == "project_flow.rfc_identity_missing: 01rfc000000000000000000001"));
    }

    #[test]
    fn failed_refresh_preserves_last_successful_observation_and_status_is_provider_free() {
        let (_temp, path, campaign) = fixture();
        let identity = PullRequestIdentity::parse("wycats/exo2#75").unwrap();
        let success = FakeProvider {
            calls: Cell::new(0),
            result: Ok(observation("First title")),
        };
        attach_pr_with_provider(
            &path,
            "request-attach",
            &campaign,
            identity.clone(),
            DeliveryRole::Implements,
            &success,
        )
        .unwrap();
        let failed = FakeProvider {
            calls: Cell::new(0),
            result: Err(ProviderFailure {
                class: "provider_unavailable",
                message: "offline".to_string(),
            }),
        };
        let result = refresh_with_provider(&path, "request-refresh", &campaign, &failed).unwrap();
        assert_eq!(
            result.pull_requests[0].title.as_deref(),
            Some("First title")
        );
        assert!(
            result.pull_requests[0]
                .last_error
                .as_deref()
                .unwrap()
                .contains("offline")
        );
        assert_eq!(failed.calls.get(), 1);
        let stored = status(&path, &campaign).unwrap();
        assert_eq!(
            stored.pull_requests[0].title.as_deref(),
            Some("First title")
        );
        assert_eq!(
            failed.calls.get(),
            1,
            "status must not contact the provider"
        );
    }

    #[test]
    fn reaching_an_rfc_target_preserves_objective_and_delivery_history() {
        let (_temp, path, campaign) = fixture();
        attach_rfc(
            &path,
            &campaign,
            "01rfc000000000000000000001",
            RfcRelation::Drives,
            Some(3),
        )
        .unwrap();
        let identity = PullRequestIdentity::parse("wycats/exo2#75").unwrap();
        let provider = FakeProvider {
            calls: Cell::new(0),
            result: Ok(observation("Deliver project flow")),
        };
        attach_pr_with_provider(
            &path,
            "request-attach",
            &campaign,
            identity,
            DeliveryRole::Implements,
            &provider,
        )
        .unwrap();
        SqliteWriter::open(&path)
            .unwrap()
            .database()
            .connection()
            .execute("UPDATE rfcs SET stage = 3 WHERE rfc_number = 10207", [])
            .unwrap();

        let project_flow = status(&path, &campaign).unwrap();
        assert_eq!(project_flow.rfc_objectives.len(), 1);
        assert_eq!(
            project_flow.rfc_objectives[0].motion(),
            RfcObjectiveMotion::TargetReached
        );
        assert_eq!(project_flow.pull_requests.len(), 1);
        assert_eq!(
            project_flow.pull_requests[0].title.as_deref(),
            Some("Deliver project flow")
        );
    }

    #[test]
    fn detach_garbage_collects_only_an_unreferenced_artifact() {
        let (_temp, path, campaign) = fixture();
        let writer = SqliteWriter::open(&path).unwrap();
        let epoch = writer.add_epoch("Other", None, &[]).unwrap();
        let other = writer
            .add_phase(&epoch, "Other campaign", "regular", None, &[])
            .unwrap();
        let identity = PullRequestIdentity::parse("wycats/exo2#75").unwrap();
        let provider = FakeProvider {
            calls: Cell::new(0),
            result: Ok(observation("PR")),
        };
        attach_pr_with_provider(
            &path,
            "attach-a",
            &campaign,
            identity.clone(),
            DeliveryRole::Implements,
            &provider,
        )
        .unwrap();
        attach_pr_with_provider(
            &path,
            "attach-b",
            &other,
            identity.clone(),
            DeliveryRole::Validates,
            &provider,
        )
        .unwrap();
        assert!(detach_pr(&path, &campaign, &identity).unwrap());
        let db = open_request_database(&path).unwrap();
        let count: i64 = db
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM project_flow_pull_requests_data",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        drop(db);
        assert!(detach_pr(&path, &other, &identity).unwrap());
        let db = open_request_database(&path).unwrap();
        let count: i64 = db
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM project_flow_pull_requests_data",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn detach_pr_reuses_the_outer_request_transaction() {
        let (_temp, path, campaign) = fixture();
        let identity = PullRequestIdentity::parse("wycats/exo2#75").unwrap();
        let provider = FakeProvider {
            calls: Cell::new(0),
            result: Ok(observation("PR")),
        };
        attach_pr_with_provider(
            &path,
            "attach-before-atomic-detach",
            &campaign,
            identity.clone(),
            DeliveryRole::Implements,
            &provider,
        )
        .unwrap();

        let transaction = RequestTransaction::begin(&path).unwrap();
        assert!(detach_pr(&path, &campaign, &identity).unwrap());
        let remaining: i64 = transaction
            .database()
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM phase_pull_request_relations_data",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(remaining, 0);
        transaction.rollback().unwrap();

        assert_eq!(status(&path, &campaign).unwrap().pull_requests.len(), 1);
    }
}
