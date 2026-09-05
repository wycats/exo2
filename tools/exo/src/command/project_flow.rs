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
    finalize_pr_attachment, finalize_refresh_with_preflight, prepare_pr_attachment,
    prepare_refresh, prepared_campaign_for_finalization, project_flow_precondition,
    refresh_with_provider_and_preflight, status_with_effective_rfcs,
};
use anyhow::{Result as ExoResult, bail};

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
                let target_stage = target_stage.map(u8::try_from).transpose().map_err(|_| {
                    project_flow_precondition(
                        "project_flow.invalid_target_stage",
                        "target stage must be between 0 and 4",
                    )
                })?;
                if target_stage.is_some_and(|stage| stage > 4) {
                    return Err(project_flow_precondition(
                        "project_flow.invalid_target_stage",
                        "target stage must be between 0 and 4",
                    ));
                }
                let relation = RfcRelation::parse(&relation).map_err(|error| {
                    project_flow_precondition(
                        "project_flow.invalid_rfc_relation",
                        error.to_string(),
                    )
                })?;
                CommandBox::mutable(ProjectFlowRfcAttach::new(
                    selector,
                    campaign,
                    relation,
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
            } => {
                let identity = parse_pull_request_identity(&selector)?;
                let role = parse_delivery_role(&role)?;
                CommandBox::mutable(ProjectFlowPrAttach::new(identity, campaign, role))
            }
            Self::PrDetach { selector, campaign } => {
                let identity = parse_pull_request_identity(&selector)?;
                CommandBox::mutable(ProjectFlowPrDetach::new(identity, campaign))
            }
            Self::Refresh { campaign } => CommandBox::mutable(ProjectFlowRefresh::new(campaign)),
            Self::Status { campaign } => CommandBox::pure(ProjectFlowStatus::new(campaign)),
        })
    }
}

fn parse_pull_request_identity(selector: &str) -> ExoResult<PullRequestIdentity> {
    PullRequestIdentity::parse(selector).map_err(|error| {
        project_flow_precondition(
            "project_flow.invalid_pull_request_selector",
            error.to_string(),
        )
    })
}

fn parse_delivery_role(role: &str) -> ExoResult<DeliveryRole> {
    DeliveryRole::parse(role).map_err(|error| {
        project_flow_precondition("project_flow.invalid_delivery_role", error.to_string())
    })
}

fn active_campaign(ctx: &CommandContext<'_>) -> ExoResult<String> {
    let loader = SqliteLoader::open(ctx.db_path())?;
    let agent = AgentContext::load(ctx.root.to_path_buf())?;
    let workspace_root = agent.workspace_root_key();
    let Some(details) =
        loader.load_active_phase_details_for_workspace(workspace_root.as_deref())?
    else {
        return Err(no_active_campaign_error());
    };
    Ok(details.phase_id)
}

fn no_active_campaign_error() -> anyhow::Error {
    project_flow_precondition(
        "project_flow.active_campaign_required",
        "no active campaign; pass --campaign with an exact campaign ID or alias",
    )
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
            parse_pull_request_identity(&selector)?,
            parse_delivery_role(&role)?,
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
        .ok_or_else(no_active_campaign_error)
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
            let campaign = prepared_campaign_for_finalization(&ctx.db_path(), &request_id)?;
            finalize_pr_attachment(
                &ctx.db_path(),
                &request_id,
                &campaign,
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
        let request_id = request_id(ctx);
        let finalizing = prepared_read_is_finalizing(ctx)?;
        let campaign = if finalizing {
            prepared_campaign_for_finalization(&ctx.db_path(), &request_id)?
        } else {
            self.campaign
                .clone()
                .map_or_else(|| active_campaign_mut(ctx), Ok)?
        };
        let preflight = || {
            crate::post_write::preflight_sidecar_post_write(
                ctx.project,
                "project-flow",
                "pr.attach",
                Effect::Write,
            )
        };
        let status = if finalizing {
            finalize_refresh_with_preflight(&ctx.db_path(), &request_id, &campaign, preflight)?
        } else {
            refresh_with_provider_and_preflight(
                &ctx.db_path(),
                &request_id,
                &campaign,
                &GithubProvider::default(),
                preflight,
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
        let effective_rfcs = crate::rfc::load_effective_rfcs(ctx.root, ctx.project)?;
        let status = status_with_effective_rfcs(&ctx.db_path(), &campaign, &effective_rfcs)?;
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
    use crate::api::protocol::{Address, CallParams, ErrorCode};
    use crate::failure::ExoFailure;
    use crate::project_flow::{PullRequestView, RfcObjectiveView};

    #[test]
    fn prepared_commands_keep_their_campaign_when_focus_or_alias_changes() {
        struct UnavailableProvider;
        impl crate::project_flow::PullRequestProvider for UnavailableProvider {
            fn observe(
                &self,
                _: &PullRequestIdentity,
            ) -> Result<
                crate::project_flow::ProviderObservation,
                crate::project_flow::ProviderFailure,
            > {
                Err(crate::project_flow::ProviderFailure {
                    class: "provider_unavailable",
                    message: "fixture unavailable".to_string(),
                })
            }
        }
        for scenario in [
            "refresh-switch",
            "refresh-clear",
            "refresh-alias",
            "attach-alias",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path();
            let path = crate::context::db_path(root, None);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let writer = crate::context::SqliteWriter::open(&path).unwrap();
            let epoch = writer.add_epoch("Epoch", Some("epoch"), &[]).unwrap();
            let first = writer
                .add_phase(
                    &epoch,
                    "First",
                    "regular",
                    Some("first-campaign"),
                    &["first-campaign".to_string()],
                )
                .unwrap();
            let second = writer
                .add_phase(&epoch, "Second", "regular", Some("second-campaign"), &[])
                .unwrap();
            writer.update_phase_status(&first, "in-progress").unwrap();
            let identity = PullRequestIdentity::parse("wycats/exo2#76").unwrap();
            let owner = crate::daemon_outcomes::direct_prepared_read_owner().unwrap();
            let attaching = scenario == "attach-alias";
            if attaching {
                prepare_pr_attachment(
                    &path,
                    "pending",
                    "first-campaign",
                    identity.clone(),
                    DeliveryRole::Implements,
                    &owner,
                    &UnavailableProvider,
                )
                .unwrap();
                for (campaign, changed_identity, role) in [
                    (second.as_str(), identity.clone(), DeliveryRole::Implements),
                    (
                        first.as_str(),
                        PullRequestIdentity::parse("wycats/exo2#77").unwrap(),
                        DeliveryRole::Implements,
                    ),
                    (first.as_str(), identity.clone(), DeliveryRole::Validates),
                ] {
                    let error = prepare_pr_attachment(
                        &path,
                        "pending",
                        campaign,
                        changed_identity,
                        role,
                        &owner,
                        &UnavailableProvider,
                    )
                    .unwrap_err();
                    assert_eq!(
                        error
                            .downcast_ref::<ExoFailure>()
                            .unwrap()
                            .error
                            .details
                            .as_ref()
                            .unwrap()["kind"],
                        "project_flow.request_id_conflict"
                    );
                }
            } else {
                attach_pr_with_provider(
                    &path,
                    "initial",
                    &first,
                    identity.clone(),
                    DeliveryRole::Implements,
                    &UnavailableProvider,
                )
                .unwrap();
                prepare_refresh(
                    &path,
                    "pending",
                    "first-campaign",
                    &owner,
                    &UnavailableProvider,
                )
                .unwrap();
                let error =
                    prepare_refresh(&path, "pending", &second, &owner, &UnavailableProvider)
                        .unwrap_err();
                assert_eq!(
                    error
                        .downcast_ref::<ExoFailure>()
                        .unwrap()
                        .error
                        .details
                        .as_ref()
                        .unwrap()["kind"],
                    "project_flow.request_id_conflict"
                );
            }
            if scenario.ends_with("alias") {
                writer.database().connection().execute(
                    "UPDATE entity_aliases SET entity_id = (SELECT id FROM phases_data WHERE text_id = ?1)
                     WHERE entity_type = 'phase' AND alias = 'first-campaign'", [&second],
                ).unwrap();
                assert_eq!(
                    crate::project_flow::resolve_campaign(
                        writer.database().connection(),
                        "first-campaign"
                    )
                    .unwrap(),
                    second
                );
            } else {
                writer.update_phase_status(&first, "pending").unwrap();
                if scenario == "refresh-switch" {
                    writer.update_phase_status(&second, "in-progress").unwrap();
                }
            }
            let transaction = exosuit_storage::RequestTransaction::begin(&path).unwrap();
            let mut ctx = MutableCommandContext {
                root,
                project: None,
                format: crate::command::traits::OutputFormat::Json,
                agent_id: None,
                request_id: Some("pending".to_string()),
                workflow_confirmation: None,
                input_content: None,
                runtime_services: None,
            };
            let output = if attaching {
                ProjectFlowPrAttach::new(
                    identity,
                    "first-campaign".to_string(),
                    DeliveryRole::Implements,
                )
                .execute_mut(&mut ctx)
            } else {
                ProjectFlowRefresh::new(
                    scenario
                        .ends_with("alias")
                        .then(|| "first-campaign".to_string()),
                )
                .execute_mut(&mut ctx)
            }
            .unwrap();
            transaction.commit().unwrap();
            assert_eq!(output.data["campaign_id"], first, "{scenario}");
            assert_eq!(
                crate::project_flow::status(&path, &first)
                    .unwrap()
                    .pull_requests
                    .len(),
                1
            );
            assert!(
                crate::project_flow::status(&path, &second)
                    .unwrap()
                    .pull_requests
                    .is_empty()
            );
        }
    }

    #[test]
    fn status_command_observes_the_workspace_rfc_document() {
        use crate::process_spawn::CommandSpawnExt as _;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(root)
                .args(args)
                .output_guarded()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout).unwrap().trim().to_string()
        };
        git(&["init", "--initial-branch=main"]);
        git(&["config", "user.name", "Exo Test"]);
        git(&["config", "user.email", "exo-test@example.invalid"]);
        git(&["config", "commit.gpgsign", "false"]);
        let original = root.join("docs/rfcs/stage-2/10207-project-flow.md");
        std::fs::create_dir_all(original.parent().unwrap()).unwrap();
        std::fs::write(&original,
            "<!-- exo:10207 ulid:01rfc000000000000000000001 -->\n\n# RFC 10207: Canonical project flow\n").unwrap();
        git(&["add", "docs/rfcs"]);
        git(&["commit", "-m", "Canonical RFC"]);
        let head = git(&["rev-parse", "HEAD"]);
        git(&["update-ref", "refs/remotes/origin/main", &head]);
        git(&[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ]);

        let path = crate::context::db_path(root, None);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let writer = crate::context::SqliteWriter::open(&path).unwrap();
        let epoch = writer.add_epoch("Epoch", Some("epoch"), &[]).unwrap();
        let campaign = writer
            .add_phase(&epoch, "Campaign", "regular", Some("campaign"), &[])
            .unwrap();
        crate::rfc::observe_effective_rfcs(root, None).unwrap();
        crate::project_flow::attach_rfc(
            &path,
            &campaign,
            "10207",
            crate::project_flow::RfcRelation::Drives,
            Some(3),
        )
        .unwrap();

        let promoted = root.join("docs/rfcs/stage-3/10207-project-flow.md");
        std::fs::create_dir_all(promoted.parent().unwrap()).unwrap();
        std::fs::rename(original, &promoted).unwrap();
        std::fs::write(promoted,
            "<!-- exo:10207 ulid:01rfc000000000000000000001 -->\n\n# RFC 10207: Workspace project flow\n").unwrap();
        let ctx = CommandContext {
            root,
            project: None,
            format: crate::command::traits::OutputFormat::Json,
            agent_id: None,
            request_id: None,
            workflow_confirmation: None,
            input_content: None,
            runtime_services: None,
        };
        let output = ProjectFlowStatus::new(Some(campaign))
            .execute(&ctx)
            .unwrap();
        assert_eq!(output.data["rfc_objectives"][0]["current_stage"], 3);
        assert_eq!(
            output.data["rfc_objectives"][0]["title"],
            "Workspace project flow"
        );
        assert_eq!(
            writer
                .database()
                .connection()
                .query_row(
                    "SELECT stage FROM rfcs_data WHERE rfc_number = 10207",
                    [],
                    |row| row.get::<_, u8>(0),
                )
                .unwrap(),
            2,
            "the status view must not promote shared canonical state"
        );
    }

    #[test]
    fn project_flow_status_human_output_renders_motion_and_degraded_truth() {
        let rendered = render_project_flow_status(&ProjectFlowStatusData {
            portable_state_changed: false,
            campaign_id: "campaign-one".to_string(),
            rfc_objectives: vec![RfcObjectiveView {
                rfc_ulid: "01rfc".to_string(),
                rfc_number: 10207,
                title: "Project flow".to_string(),
                observed_stage: Some(2),
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
            observed_stage: Some(current_stage),
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
                portable_state_changed: false,
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
                portable_state_changed: false,
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

    #[test]
    fn missing_active_campaign_is_a_typed_precondition() {
        let error = no_active_campaign_error();
        let failure = error
            .downcast_ref::<ExoFailure>()
            .expect("typed project-flow precondition");
        assert_eq!(failure.error.code, ErrorCode::PreconditionFailed);
        assert_eq!(
            failure.error.details.as_ref().unwrap()["kind"],
            "project_flow.active_campaign_required"
        );
    }

    #[test]
    fn invalid_rfc_relation_is_a_typed_precondition() {
        let error = ProjectFlowCommands::RfcAttach {
            selector: "10207".to_string(),
            campaign: "campaign-one".to_string(),
            relation: "related".to_string(),
            target_stage: Some(3),
        }
        .to_command_box(std::path::Path::new("."))
        .expect_err("legacy relation must not enter the typed command path");
        let failure = error
            .downcast_ref::<ExoFailure>()
            .expect("typed project-flow precondition");
        assert_eq!(failure.error.code, ErrorCode::PreconditionFailed);
        assert_eq!(
            failure.error.details.as_ref().unwrap()["kind"],
            "project_flow.invalid_rfc_relation"
        );
    }

    #[test]
    fn invalid_pr_detach_selector_is_a_typed_precondition() {
        let error = ProjectFlowCommands::PrDetach {
            selector: "not-a-pull-request".to_string(),
            campaign: "campaign-one".to_string(),
        }
        .to_command_box(std::path::Path::new("."))
        .expect_err("malformed pull-request selector must fail during command construction");
        let failure = error
            .downcast_ref::<ExoFailure>()
            .expect("typed project-flow precondition");
        assert_eq!(failure.error.code, ErrorCode::PreconditionFailed);
        assert_eq!(
            failure.error.details.as_ref().unwrap()["kind"],
            "project_flow.invalid_pull_request_selector"
        );
    }

    #[test]
    fn prepared_pr_attachment_validation_uses_typed_preconditions() {
        let request = |selector: &str, role: &str| RequestEnvelope {
            protocol_version: 1,
            id: "prepared-attach-validation".to_string(),
            op: Op::Call(CallParams {
                address: Address::Operation {
                    path: vec![
                        "project-flow".to_string(),
                        "pr".to_string(),
                        "attach".to_string(),
                    ],
                },
                input: serde_json::json!({
                    "selector": selector,
                    "campaign": "campaign-one",
                    "role": role,
                }),
            }),
            workspace_root: None,
            auth: None,
            workflow_confirmation: None,
            agent_id: None,
        };
        let owner = DaemonOwnerIdentity {
            instance_id: "instance-a".to_string(),
            pid: 101,
            process_start_id: "start-a".to_string(),
        };

        for (selector, role, expected_kind) in [
            (
                "not-a-pull-request",
                "implements",
                "project_flow.invalid_pull_request_selector",
            ),
            (
                "wycats/exo2#76",
                "related",
                "project_flow.invalid_delivery_role",
            ),
        ] {
            let error = prepare_external_read_request(
                std::path::Path::new("unused.db"),
                std::path::Path::new("."),
                &request(selector, role),
                &owner,
            )
            .expect_err("invalid prepared attachment must fail before provider I/O");
            let failure = error
                .downcast_ref::<ExoFailure>()
                .expect("typed project-flow precondition");
            assert_eq!(failure.error.code, ErrorCode::PreconditionFailed);
            assert_eq!(
                failure.error.details.as_ref().unwrap()["kind"],
                expected_kind
            );
        }
    }
}
