//! Campaign RFC objectives and pull-request delivery evidence.

use super::traits::{
    Command, CommandBox, CommandContext, CommandOutput, MutableCommand, MutableCommandContext,
};
use crate::api::protocol::{Effect, Op, RequestEnvelope};
use crate::command::command_spec::CommandSpec;
use crate::command::registry::default_registry;
use crate::command::router::Invocation;
use crate::command::unified_diagnostics::IntoDiagnosticSteering;
use crate::context::{AgentContext, SqliteLoader};
use crate::daemon_outcomes::{DaemonOwnerIdentity, request_command_path};
use crate::project_flow::{
    DeliveryRole, GithubProvider, ProjectFlowStatus as ProjectFlowStatusData, PullRequestIdentity,
    RfcObjectiveMotion, RfcRelation, attach_pr_with_provider, attach_rfc, detach_pr, detach_rfc,
    finalize_pr_attachment, finalize_refresh, prepare_pr_attachment, prepare_refresh,
    refresh_with_provider, status,
};
use anyhow::{Context, Result as ExoResult, bail};

fn mutable_command_unreachable(name: &str) -> ! {
    unreachable!("{name} should be dispatched via execute_mut")
}

#[derive(Debug, exospec::ExoSpec)]
#[exo(
    namespace = "project-flow",
    description = "Campaign RFC and pull-request relationships"
)]
pub enum ProjectFlowCommands {
    #[exo(
        operation = "rfc.attach",
        effect = "write",
        description = "Attach a canonical RFC objective to a campaign"
    )]
    RfcAttach {
        #[exo(positional, description = "RFC ULID or uniquely resolving number")]
        selector: String,
        #[exo(long, description = "Campaign ID or alias")]
        campaign: String,
        #[exo(long, description = "Relationship: drives, implements, or validates")]
        relation: String,
        #[exo(long, optional, description = "Target RFC stage (0-4)")]
        target_stage: Option<i64>,
    },

    #[exo(
        operation = "rfc.detach",
        effect = "write",
        description = "Detach a canonical RFC objective from a campaign"
    )]
    RfcDetach {
        #[exo(positional, description = "RFC ULID or uniquely resolving number")]
        selector: String,
        #[exo(long, description = "Campaign ID or alias")]
        campaign: String,
    },

    #[exo(
        operation = "pr.attach",
        effect = "write",
        description = "Attach a pull request to a campaign and refresh its observation"
    )]
    PrAttach {
        #[exo(positional, description = "GitHub URL or owner/repository#number")]
        selector: String,
        #[exo(long, description = "Campaign ID or alias")]
        campaign: String,
        #[exo(long, description = "Delivery role: implements or validates")]
        role: String,
    },

    #[exo(
        operation = "pr.detach",
        effect = "write",
        description = "Detach a pull request from a campaign"
    )]
    PrDetach {
        #[exo(positional, description = "GitHub URL or owner/repository#number")]
        selector: String,
        #[exo(long, description = "Campaign ID or alias")]
        campaign: String,
    },

    #[exo(
        effect = "write",
        description = "Refresh stored pull-request observations for a campaign"
    )]
    Refresh {
        #[exo(
            long,
            optional,
            description = "Campaign ID or alias (defaults to active)"
        )]
        campaign: Option<String>,
    },

    #[exo(
        effect = "pure",
        description = "Read stored RFC and pull-request motion for a campaign"
    )]
    Status {
        #[exo(
            long,
            optional,
            description = "Campaign ID or alias (defaults to active)"
        )]
        campaign: Option<String>,
    },
}

impl ProjectFlowCommands {
    pub fn to_command_box(self, _root: &std::path::Path) -> ExoResult<CommandBox> {
        Ok(match self {
            Self::RfcAttach {
                selector,
                campaign,
                relation,
                target_stage,
            } => {
                let target_stage = target_stage
                    .map(u8::try_from)
                    .transpose()
                    .map_err(|_| anyhow::anyhow!("target stage must be between 0 and 4"))?;
                CommandBox::mutable(ProjectFlowRfcAttach::new(
                    selector,
                    campaign,
                    RfcRelation::parse(&relation)?,
                    target_stage,
                ))
            }
            Self::RfcDetach { selector, campaign } => {
                CommandBox::mutable(ProjectFlowRfcDetach::new(selector, campaign))
            }
            Self::PrAttach {
                selector,
                campaign,
                role,
            } => CommandBox::mutable(ProjectFlowPrAttach::new(
                PullRequestIdentity::parse(&selector)?,
                campaign,
                DeliveryRole::parse(&role)?,
            )),
            Self::PrDetach { selector, campaign } => CommandBox::mutable(ProjectFlowPrDetach::new(
                PullRequestIdentity::parse(&selector)?,
                campaign,
            )),
            Self::Refresh { campaign } => CommandBox::mutable(ProjectFlowRefresh::new(campaign)),
            Self::Status { campaign } => CommandBox::pure(ProjectFlowStatus::new(campaign)),
        })
    }
}

fn active_campaign(ctx: &CommandContext<'_>) -> ExoResult<String> {
    let loader = SqliteLoader::open(ctx.db_path())?;
    let agent = AgentContext::load(ctx.root.to_path_buf())?;
    let workspace_root = agent.workspace_root_key();
    let Some(details) =
        loader.load_active_phase_details_for_workspace(workspace_root.as_deref())?
    else {
        bail!("no active campaign; pass --campaign with an exact campaign ID or alias");
    };
    Ok(details.phase_id)
}

fn active_campaign_mut(ctx: &MutableCommandContext<'_>) -> ExoResult<String> {
    let read = CommandContext {
        root: ctx.root,
        project: ctx.project,
        format: ctx.format,
        agent_id: ctx.agent_id.clone(),
        request_id: ctx.request_id.clone(),
        workflow_confirmation: ctx.workflow_confirmation.clone(),
        input_content: ctx.input_content.clone(),
        runtime_services: ctx.runtime_services,
    };
    active_campaign(&read)
}

fn request_id(ctx: &MutableCommandContext<'_>) -> String {
    ctx.request_id
        .clone()
        .unwrap_or_else(|| ulid::Ulid::new().to_string().to_lowercase())
}

fn prepared_read_is_finalizing(ctx: &MutableCommandContext<'_>) -> ExoResult<bool> {
    Ok(exosuit_storage::active_request_database(ctx.db_path())?.is_some())
}

pub(crate) fn prepare_external_read_request(
    db_path: &std::path::Path,
    workspace_root: &std::path::Path,
    request: &RequestEnvelope,
    owner: &DaemonOwnerIdentity,
) -> ExoResult<()> {
    let Some((namespace, operation)) = request_command_path(request) else {
        bail!("prepared external read requires an operation request");
    };
    if namespace != "project-flow" {
        bail!("prepared external read is not a project-flow operation");
    }
    let Op::Call(params) = &request.op else {
        bail!("prepared external read requires a call request");
    };
    let spec = CommandSpec::from_registry(&default_registry());
    let invocation = Invocation::from_json(&params.input, &namespace, &operation, &spec)
        .map_err(|diagnostic| anyhow::anyhow!(diagnostic.format_plain()))?;
    let command = ProjectFlowCommands::from_invocation(&invocation)?;
    let provider = GithubProvider::default();
    match command {
        ProjectFlowCommands::PrAttach {
            selector,
            campaign,
            role,
        } => prepare_pr_attachment(
            db_path,
            &request.id,
            &campaign,
            PullRequestIdentity::parse(&selector)?,
            DeliveryRole::parse(&role)?,
            owner,
            &provider,
        ),
        ProjectFlowCommands::Refresh { campaign } => {
            let campaign =
                campaign.map_or_else(|| active_campaign_at(db_path, workspace_root), Ok)?;
            prepare_refresh(db_path, &request.id, &campaign, owner, &provider)
        }
        _ => bail!("project-flow operation does not use prepared external reads"),
    }
}

fn active_campaign_at(
    db_path: &std::path::Path,
    workspace_root: &std::path::Path,
) -> ExoResult<String> {
    let loader = SqliteLoader::open(db_path)?;
    let agent = AgentContext::load(workspace_root.to_path_buf())?;
    loader
        .load_active_phase_details_for_workspace(agent.workspace_root_key().as_deref())?
        .map(|details| details.phase_id)
        .context("no active campaign; pass --campaign with an exact campaign ID or alias")
}

#[derive(Debug, Clone)]
pub struct ProjectFlowRfcAttach {
    selector: String,
    campaign: String,
    relation: RfcRelation,
    target_stage: Option<u8>,
}

impl ProjectFlowRfcAttach {
    pub fn new(
        selector: impl Into<String>,
        campaign: impl Into<String>,
        relation: RfcRelation,
        target_stage: Option<u8>,
    ) -> Self {
        Self {
            selector: selector.into(),
            campaign: campaign.into(),
            relation,
            target_stage,
        }
    }
}

impl Command for ProjectFlowRfcAttach {
    fn namespace(&self) -> &'static str {
        "project-flow"
    }
    fn operation(&self) -> &'static str {
        "rfc.attach"
    }
    fn effect(&self) -> Effect {
        Effect::Write
    }
    fn execute(&self, _ctx: &CommandContext<'_>) -> ExoResult<CommandOutput> {
        mutable_command_unreachable("project-flow rfc attach")
    }
    fn description(&self) -> &'static str {
        "Attach a canonical RFC objective to a campaign"
    }
}

impl MutableCommand for ProjectFlowRfcAttach {
    fn execute_mut(&self, ctx: &mut MutableCommandContext<'_>) -> ExoResult<CommandOutput> {
        let objective = attach_rfc(
            &ctx.db_path(),
            &self.campaign,
            &self.selector,
            self.relation,
            self.target_stage,
        )?;
        Ok(CommandOutput::data(objective).with_message("Attached RFC objective to campaign."))
    }
}

#[derive(Debug, Clone)]
pub struct ProjectFlowRfcDetach {
    selector: String,
    campaign: String,
}

impl ProjectFlowRfcDetach {
    pub fn new(selector: impl Into<String>, campaign: impl Into<String>) -> Self {
        Self {
            selector: selector.into(),
            campaign: campaign.into(),
        }
    }
}

impl Command for ProjectFlowRfcDetach {
    fn namespace(&self) -> &'static str {
        "project-flow"
    }
    fn operation(&self) -> &'static str {
        "rfc.detach"
    }
    fn effect(&self) -> Effect {
        Effect::Write
    }
    fn execute(&self, _ctx: &CommandContext<'_>) -> ExoResult<CommandOutput> {
        mutable_command_unreachable("project-flow rfc detach")
    }
    fn description(&self) -> &'static str {
        "Detach a canonical RFC objective from a campaign"
    }
}

impl MutableCommand for ProjectFlowRfcDetach {
    fn execute_mut(&self, ctx: &mut MutableCommandContext<'_>) -> ExoResult<CommandOutput> {
        let detached = detach_rfc(&ctx.db_path(), &self.campaign, &self.selector)?;
        Ok(
            CommandOutput::data(serde_json::json!({ "detached": detached })).with_message(
                if detached {
                    "Detached RFC objective."
                } else {
                    "RFC objective was not attached."
                },
            ),
        )
    }
}

#[derive(Debug, Clone)]
pub struct ProjectFlowPrAttach {
    identity: PullRequestIdentity,
    campaign: String,
    role: DeliveryRole,
}

impl ProjectFlowPrAttach {
    pub fn new(
        identity: PullRequestIdentity,
        campaign: impl Into<String>,
        role: DeliveryRole,
    ) -> Self {
        Self {
            identity,
            campaign: campaign.into(),
            role,
        }
    }
}

impl Command for ProjectFlowPrAttach {
    fn namespace(&self) -> &'static str {
        "project-flow"
    }
    fn operation(&self) -> &'static str {
        "pr.attach"
    }
    fn effect(&self) -> Effect {
        Effect::Write
    }
    fn execute(&self, _ctx: &CommandContext<'_>) -> ExoResult<CommandOutput> {
        mutable_command_unreachable("project-flow pr attach")
    }
    fn description(&self) -> &'static str {
        "Attach a pull request to a campaign and refresh its observation"
    }
}

impl MutableCommand for ProjectFlowPrAttach {
    fn execute_mut(&self, ctx: &mut MutableCommandContext<'_>) -> ExoResult<CommandOutput> {
        let request_id = request_id(ctx);
        let status = if prepared_read_is_finalizing(ctx)? {
            finalize_pr_attachment(
                &ctx.db_path(),
                &request_id,
                &self.campaign,
                self.identity.clone(),
                self.role,
            )?
        } else {
            attach_pr_with_provider(
                &ctx.db_path(),
                &request_id,
                &self.campaign,
                self.identity.clone(),
                self.role,
                &GithubProvider::default(),
            )?
        };
        let message = render_project_flow_mutation("Attached pull request to campaign.", &status);
        Ok(CommandOutput::data(status).with_message(message))
    }
}

#[derive(Debug, Clone)]
pub struct ProjectFlowPrDetach {
    identity: PullRequestIdentity,
    campaign: String,
}

impl ProjectFlowPrDetach {
    pub fn new(identity: PullRequestIdentity, campaign: impl Into<String>) -> Self {
        Self {
            identity,
            campaign: campaign.into(),
        }
    }
}

impl Command for ProjectFlowPrDetach {
    fn namespace(&self) -> &'static str {
        "project-flow"
    }
    fn operation(&self) -> &'static str {
        "pr.detach"
    }
    fn effect(&self) -> Effect {
        Effect::Write
    }
    fn execute(&self, _ctx: &CommandContext<'_>) -> ExoResult<CommandOutput> {
        mutable_command_unreachable("project-flow pr detach")
    }
    fn description(&self) -> &'static str {
        "Detach a pull request from a campaign"
    }
}

impl MutableCommand for ProjectFlowPrDetach {
    fn execute_mut(&self, ctx: &mut MutableCommandContext<'_>) -> ExoResult<CommandOutput> {
        let detached = detach_pr(&ctx.db_path(), &self.campaign, &self.identity)?;
        Ok(
            CommandOutput::data(serde_json::json!({ "detached": detached })).with_message(
                if detached {
                    "Detached pull request."
                } else {
                    "Pull request was not attached."
                },
            ),
        )
    }
}

#[derive(Debug, Clone)]
pub struct ProjectFlowRefresh {
    campaign: Option<String>,
}

impl ProjectFlowRefresh {
    pub const fn new(campaign: Option<String>) -> Self {
        Self { campaign }
    }
}

impl Command for ProjectFlowRefresh {
    fn namespace(&self) -> &'static str {
        "project-flow"
    }
    fn operation(&self) -> &'static str {
        "refresh"
    }
    fn effect(&self) -> Effect {
        Effect::Write
    }
    fn execute(&self, _ctx: &CommandContext<'_>) -> ExoResult<CommandOutput> {
        mutable_command_unreachable("project-flow refresh")
    }
    fn description(&self) -> &'static str {
        "Refresh stored pull-request observations for a campaign"
    }
}

impl MutableCommand for ProjectFlowRefresh {
    fn execute_mut(&self, ctx: &mut MutableCommandContext<'_>) -> ExoResult<CommandOutput> {
        let campaign = self
            .campaign
            .clone()
            .map_or_else(|| active_campaign_mut(ctx), Ok)?;
        let request_id = request_id(ctx);
        let status = if prepared_read_is_finalizing(ctx)? {
            finalize_refresh(&ctx.db_path(), &request_id, &campaign)?
        } else {
            refresh_with_provider(
                &ctx.db_path(),
                &request_id,
                &campaign,
                &GithubProvider::default(),
            )?
        };
        let message = render_project_flow_mutation("Refreshed project-flow observations.", &status);
        Ok(CommandOutput::data(status).with_message(message))
    }
}

#[derive(Debug, Clone)]
pub struct ProjectFlowStatus {
    campaign: Option<String>,
}

impl ProjectFlowStatus {
    pub const fn new(campaign: Option<String>) -> Self {
        Self { campaign }
    }
}

impl Command for ProjectFlowStatus {
    fn namespace(&self) -> &'static str {
        "project-flow"
    }
    fn operation(&self) -> &'static str {
        "status"
    }
    fn effect(&self) -> Effect {
        Effect::Pure
    }
    fn description(&self) -> &'static str {
        "Read stored RFC and pull-request motion for a campaign"
    }

    fn execute(&self, ctx: &CommandContext<'_>) -> ExoResult<CommandOutput> {
        let campaign = self
            .campaign
            .clone()
            .map_or_else(|| active_campaign(ctx), Ok)?;
        let status = status(&ctx.db_path(), &campaign)?;
        let message = render_project_flow_status(&status);
        Ok(CommandOutput::data(status).with_message(message))
    }
}

pub(crate) fn render_project_flow_status(status: &ProjectFlowStatusData) -> String {
    let mut lines = vec![format!(
        "Project motion for campaign {}",
        status.campaign_id
    )];

    lines.push(String::new());
    lines.push("RFC objectives:".to_string());
    if status.rfc_objectives.is_empty() {
        lines.push("  none".to_string());
    } else {
        for objective in &status.rfc_objectives {
            let current = objective.current_stage.map_or_else(
                || "unavailable".to_string(),
                |stage| format!("Stage {stage}"),
            );
            let motion = match objective.motion() {
                RfcObjectiveMotion::Advancing => objective
                    .target_stage
                    .map_or_else(String::new, |stage| format!(" -> Stage {stage}")),
                RfcObjectiveMotion::TargetReached => objective.target_stage.map_or_else(
                    || "; target reached".to_string(),
                    |stage| format!("; target Stage {stage} reached"),
                ),
                RfcObjectiveMotion::Associated => "; associated".to_string(),
                RfcObjectiveMotion::Terminal => "; terminal".to_string(),
                RfcObjectiveMotion::IdentityMissing => "; identity missing".to_string(),
            };
            let lifecycle = objective
                .lifecycle
                .as_deref()
                .map_or_else(String::new, |value| format!("; {value}"));
            let supersession = objective
                .superseded_by
                .as_deref()
                .map_or_else(String::new, |value| format!("; superseded by {value}"));
            lines.push(format!(
                "  RFC {:05} {} [{}]: {}{}{}{}",
                objective.rfc_number,
                objective.title,
                objective.relation,
                current,
                motion,
                lifecycle,
                supersession
            ));
        }
    }

    lines.push(String::new());
    lines.push("Pull-request delivery:".to_string());
    if status.pull_requests.is_empty() {
        lines.push("  none".to_string());
    } else {
        for pull_request in &status.pull_requests {
            let title = pull_request.title.as_deref().unwrap_or("title unavailable");
            let lifecycle = pull_request.lifecycle.as_deref().unwrap_or("unobserved");
            let review = pull_request.review_state.as_deref().unwrap_or("unknown");
            let checks = pull_request.checks_state.as_deref().unwrap_or("unknown");
            lines.push(format!(
                "  {}#{} {} [{}]: {}; review {}; checks {}",
                pull_request.identity.repository,
                pull_request.identity.number,
                title,
                pull_request.role,
                lifecycle,
                review,
                checks
            ));
            lines.push(format!(
                "    {}",
                render_observation_state(
                    pull_request.last_success_at.as_deref(),
                    pull_request.last_attempt_at.as_deref(),
                    pull_request.last_error.as_deref(),
                )
            ));
        }
    }

    if !status.diagnostics.is_empty() {
        lines.push(String::new());
        lines.push("Diagnostics:".to_string());
        lines.extend(
            status
                .diagnostics
                .iter()
                .map(|diagnostic| format!("  {diagnostic}")),
        );
    }

    lines.join("\n")
}

fn render_project_flow_mutation(prefix: &str, status: &ProjectFlowStatusData) -> String {
    format!("{prefix}\n\n{}", render_project_flow_status(status))
}

fn render_observation_state(
    last_success_at: Option<&str>,
    last_attempt_at: Option<&str>,
    last_error: Option<&str>,
) -> String {
    if let Some(error) = last_error {
        let attempt = last_attempt_at.map_or_else(
            || "at an unknown time".to_string(),
            |timestamp| format!("{} ago", observation_age(timestamp)),
        );
        let prior = last_success_at.map_or_else(
            || "no successful observation is stored".to_string(),
            |timestamp| format!("last success {} ago", observation_age(timestamp)),
        );
        return format!("refresh failed {attempt}: {error}; {prior}");
    }
    if let Some(timestamp) = last_success_at {
        return format!("observed {} ago", observation_age(timestamp));
    }
    if last_attempt_at.is_some() {
        return "observation unavailable".to_string();
    }
    "never observed".to_string()
}

fn observation_age(timestamp: &str) -> String {
    let Ok(observed) = chrono::DateTime::parse_from_rfc3339(timestamp) else {
        return "at an unknown age".to_string();
    };
    let seconds = chrono::Utc::now()
        .signed_duration_since(observed.with_timezone(&chrono::Utc))
        .num_seconds()
        .max(0);
    match seconds {
        0..=59 => "less than a minute".to_string(),
        60..=3_599 => format!("{}m", seconds / 60),
        3_600..=86_399 => format!("{}h", seconds / 3_600),
        _ => format!("{}d", seconds / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_flow::{PullRequestView, RfcObjectiveView};

    #[test]
    fn project_flow_status_human_output_renders_motion_and_degraded_truth() {
        let rendered = render_project_flow_status(&ProjectFlowStatusData {
            campaign_id: "campaign-one".to_string(),
            rfc_objectives: vec![RfcObjectiveView {
                rfc_ulid: "01rfc".to_string(),
                rfc_number: 10207,
                title: "Project flow".to_string(),
                current_stage: Some(2),
                lifecycle: Some("active".to_string()),
                superseded_by: None,
                target_stage: Some(3),
                relation: "drives".to_string(),
                source: "typed".to_string(),
                diagnostic: None,
            }],
            pull_requests: vec![PullRequestView {
                identity: PullRequestIdentity::parse("wycats/exo2#75").unwrap(),
                role: "implements".to_string(),
                title: Some("Deliver project flow".to_string()),
                lifecycle: Some("open".to_string()),
                head_oid: Some("abc123".to_string()),
                review_state: Some("approved".to_string()),
                checks_state: Some("failing".to_string()),
                last_success_at: Some("2026-09-03T12:00:00Z".to_string()),
                last_attempt_at: Some("2026-09-03T12:05:00Z".to_string()),
                last_error: Some("provider unavailable".to_string()),
            }],
            diagnostics: vec!["project_flow.example_diagnostic".to_string()],
        });

        assert!(rendered.contains("Project motion for campaign campaign-one"));
        assert!(rendered.contains("RFC 10207 Project flow [drives]: Stage 2 -> Stage 3"));
        assert!(rendered.contains("wycats/exo2#75 Deliver project flow [implements]"));
        assert!(rendered.contains("review approved; checks failing"));
        assert!(rendered.contains("refresh failed"));
        assert!(rendered.contains("project_flow.example_diagnostic"));
    }

    #[test]
    fn project_flow_status_distinguishes_reached_stable_and_advancing_objectives() {
        let objective = |current_stage, target_stage| RfcObjectiveView {
            rfc_ulid: format!("01rfc{current_stage}{target_stage:?}"),
            rfc_number: 10207,
            title: "Project flow".to_string(),
            current_stage: Some(current_stage),
            lifecycle: Some("active".to_string()),
            superseded_by: None,
            target_stage,
            relation: "drives".to_string(),
            source: "typed".to_string(),
            diagnostic: None,
        };
        let render = |objective| {
            render_project_flow_status(&ProjectFlowStatusData {
                campaign_id: "campaign-one".to_string(),
                rfc_objectives: vec![objective],
                pull_requests: Vec::new(),
                diagnostics: Vec::new(),
            })
        };

        let reached = render(objective(3, Some(3)));
        assert!(reached.contains("Stage 3; target Stage 3 reached"));
        assert!(!reached.contains("Stage 3 -> Stage 3"));

        let stable = render(objective(4, None));
        assert!(stable.contains("Stage 4; associated"));
        assert!(!stable.contains("Stage 4 ->"));

        let advancing = render(objective(2, Some(3)));
        assert!(advancing.contains("Stage 2 -> Stage 3"));
    }

    #[test]
    fn project_flow_mutations_render_all_degraded_provider_classes() {
        for diagnostic in [
            "authentication: run gh auth login",
            "permission: resource not accessible",
            "not_found: could not resolve pull request",
            "invalid_response: malformed provider response",
            "provider_unavailable: upstream unavailable",
            "transport: child process failed",
        ] {
            let status = ProjectFlowStatusData {
                campaign_id: "campaign-one".to_string(),
                rfc_objectives: Vec::new(),
                pull_requests: vec![PullRequestView {
                    identity: PullRequestIdentity::parse("wycats/exo2#75").unwrap(),
                    role: "implements".to_string(),
                    title: None,
                    lifecycle: None,
                    head_oid: None,
                    review_state: None,
                    checks_state: None,
                    last_success_at: None,
                    last_attempt_at: Some("2026-09-03T12:05:00Z".to_string()),
                    last_error: Some(diagnostic.to_string()),
                }],
                diagnostics: Vec::new(),
            };
            for prefix in [
                "Attached pull request to campaign.",
                "Refreshed project-flow observations.",
            ] {
                let rendered = render_project_flow_mutation(prefix, &status);
                assert!(rendered.starts_with(prefix));
                assert!(rendered.contains(diagnostic), "{diagnostic}");
            }
        }
    }
}
