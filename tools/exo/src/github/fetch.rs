use crate::github::remote::ParsedGithubRemote;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Output};

pub(crate) const SIDECAR_REGISTRY_PROFILE_PATH: &str = ".exosuit/sidecars.toml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SidecarRegistryIdentity {
    source: SidecarRegistryIdentitySource,
    login: Option<String>,
}

impl SidecarRegistryIdentity {
    const fn authenticated_user(login: Option<String>) -> Self {
        Self {
            source: SidecarRegistryIdentitySource::AuthenticatedUser,
            login,
        }
    }

    const fn remote_owner(source: SidecarRegistryIdentitySource, login: String) -> Self {
        Self {
            source,
            login: Some(login),
        }
    }

    pub(crate) const fn source(&self) -> SidecarRegistryIdentitySource {
        self.source
    }

    pub(crate) fn login(&self) -> Option<&str> {
        self.login.as_deref()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SidecarRegistryIdentitySource {
    AuthenticatedUser,
    RemoteOwnerUser,
    RemoteOwnerOrganization,
    RemoteOwnerUnknown,
}

impl SidecarRegistryIdentitySource {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::AuthenticatedUser => "authenticated-user",
            Self::RemoteOwnerUser => "remote-owner-user",
            Self::RemoteOwnerOrganization => "remote-owner-organization",
            Self::RemoteOwnerUnknown => "remote-owner-unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SidecarRegistryFetchAttempt {
    source: SidecarRegistryFetchSource,
    label: String,
    profile_repo: Option<String>,
    path: Option<String>,
    local_path: Option<PathBuf>,
    identity: SidecarRegistryIdentity,
}

impl SidecarRegistryFetchAttempt {
    fn local_file(path: &Path, repository: &ParsedGithubRemote) -> Self {
        let display_path = path.to_string_lossy().to_string();
        Self {
            source: SidecarRegistryFetchSource::LocalFile,
            label: format!("local-file:{display_path}"),
            profile_repo: None,
            path: Some(display_path),
            local_path: Some(path.to_path_buf()),
            identity: SidecarRegistryIdentity::remote_owner(
                SidecarRegistryIdentitySource::RemoteOwnerUnknown,
                repository.owner.clone(),
            ),
        }
    }

    fn authenticated_user_profile() -> Self {
        Self {
            source: SidecarRegistryFetchSource::GithubProfile,
            label: format!("github-profile:{SIDECAR_REGISTRY_PROFILE_PATH}"),
            profile_repo: None,
            path: Some(SIDECAR_REGISTRY_PROFILE_PATH.to_string()),
            local_path: None,
            identity: SidecarRegistryIdentity::authenticated_user(None),
        }
    }

    fn remote_owner_user_profile(
        repository: &ParsedGithubRemote,
        identity_source: SidecarRegistryIdentitySource,
    ) -> Self {
        Self::github_profile(
            SidecarRegistryFetchSource::GithubProfile,
            &repository.host,
            &repository.owner,
            &repository.owner,
            SidecarRegistryIdentity::remote_owner(identity_source, repository.owner.clone()),
        )
    }

    fn remote_owner_organization_profile(
        repository: &ParsedGithubRemote,
        identity_source: SidecarRegistryIdentitySource,
    ) -> Self {
        Self::github_profile(
            SidecarRegistryFetchSource::GithubOrganizationProfile,
            &repository.host,
            &repository.owner,
            ".github",
            SidecarRegistryIdentity::remote_owner(identity_source, repository.owner.clone()),
        )
    }

    fn github_profile(
        source: SidecarRegistryFetchSource,
        host: &str,
        owner: &str,
        repo: &str,
        identity: SidecarRegistryIdentity,
    ) -> Self {
        Self {
            source,
            label: format!("{}:{SIDECAR_REGISTRY_PROFILE_PATH}", source.as_str()),
            profile_repo: Some(format!("{host}/{owner}/{repo}")),
            path: Some(SIDECAR_REGISTRY_PROFILE_PATH.to_string()),
            local_path: None,
            identity,
        }
    }

    fn with_authenticated_login(&self, host: &str, login: &str) -> Self {
        Self::github_profile(
            SidecarRegistryFetchSource::GithubProfile,
            host,
            login,
            login,
            SidecarRegistryIdentity::authenticated_user(Some(login.to_string())),
        )
    }

    fn with_identity_source(&self, source: SidecarRegistryIdentitySource) -> Self {
        let mut attempt = self.clone();
        attempt.identity.source = source;
        attempt
    }

    pub(crate) const fn source(&self) -> SidecarRegistryFetchSource {
        self.source
    }

    pub(crate) fn label(&self) -> &str {
        &self.label
    }

    pub(crate) const fn identity(&self) -> &SidecarRegistryIdentity {
        &self.identity
    }

    pub(crate) fn profile_repo(&self) -> Option<&str> {
        self.profile_repo.as_deref()
    }

    pub(crate) fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    pub(crate) fn source_locator(&self) -> Option<String> {
        match self.source {
            SidecarRegistryFetchSource::LocalFile => self.path.clone(),
            SidecarRegistryFetchSource::GithubProfile
            | SidecarRegistryFetchSource::GithubOrganizationProfile => {
                let profile_repo = self.profile_repo.as_deref()?;
                let path = self.path.as_deref()?;
                Some(format!("{profile_repo}:{path}"))
            }
        }
    }

    fn local_path(&self) -> Option<&Path> {
        self.local_path.as_deref()
    }

    fn profile_owner_repo(&self) -> Option<(&str, &str)> {
        let profile_repo = self.profile_repo.as_deref()?;
        let mut parts = profile_repo.split('/');
        let first = parts.next()?;
        let (owner, repo) = if first == "github.com" {
            (parts.next()?, parts.next()?)
        } else {
            (first, parts.next()?)
        };
        parts.next().is_none().then_some((owner, repo))
    }

    fn is_authenticated_user_attempt(&self) -> bool {
        self.identity.source == SidecarRegistryIdentitySource::AuthenticatedUser
    }

    const fn is_remote_owner_attempt(&self) -> bool {
        matches!(
            self.identity.source,
            SidecarRegistryIdentitySource::RemoteOwnerUser
                | SidecarRegistryIdentitySource::RemoteOwnerOrganization
                | SidecarRegistryIdentitySource::RemoteOwnerUnknown
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SidecarRegistryFetchSource {
    LocalFile,
    GithubProfile,
    GithubOrganizationProfile,
}

impl SidecarRegistryFetchSource {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::LocalFile => "local-file",
            Self::GithubProfile => "github-profile",
            Self::GithubOrganizationProfile => "github-organization-profile",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SidecarRegistryFetchRequest {
    repository: ParsedGithubRemote,
    attempts: Vec<SidecarRegistryFetchAttempt>,
}

impl SidecarRegistryFetchRequest {
    pub(crate) fn for_discovery(
        repository: &ParsedGithubRemote,
        registry_file: Option<&Path>,
    ) -> Self {
        let attempts = registry_file.map_or_else(
            || {
                vec![
                    SidecarRegistryFetchAttempt::authenticated_user_profile(),
                    SidecarRegistryFetchAttempt::remote_owner_organization_profile(
                        repository,
                        SidecarRegistryIdentitySource::RemoteOwnerUnknown,
                    ),
                    SidecarRegistryFetchAttempt::remote_owner_user_profile(
                        repository,
                        SidecarRegistryIdentitySource::RemoteOwnerUnknown,
                    ),
                ]
            },
            |path| vec![SidecarRegistryFetchAttempt::local_file(path, repository)],
        );
        Self {
            repository: repository.clone(),
            attempts,
        }
    }

    pub(crate) const fn repository(&self) -> &ParsedGithubRemote {
        &self.repository
    }

    pub(crate) fn attempts(&self) -> &[SidecarRegistryFetchAttempt] {
        &self.attempts
    }
}

pub(crate) trait SidecarRegistryFetcher {
    fn fetch(&self, request: &SidecarRegistryFetchRequest) -> SidecarRegistryFetchReport;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SidecarRegistryFetchReport {
    pub(crate) checked: Vec<SidecarRegistryCheckedAttempt>,
    pub(crate) fetched: Vec<SidecarRegistryFetchedRegistry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SidecarRegistryCheckedAttempt {
    pub(crate) attempt_index: usize,
    pub(crate) attempt: SidecarRegistryFetchAttempt,
    pub(crate) status: SidecarRegistryFetchStatus,
    pub(crate) message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SidecarRegistryFetchedRegistry {
    pub(crate) attempt_index: usize,
    pub(crate) attempt: SidecarRegistryFetchAttempt,
    pub(crate) content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SidecarRegistryFetchStatus {
    Fetched,
    Skipped,
    NotFound,
    LoadedMatch,
    LoadedNoMatch,
    ParseError,
    UnsafeValue,
    FetchError,
}

impl SidecarRegistryFetchStatus {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Fetched => "fetched",
            Self::Skipped => "skipped",
            Self::NotFound => "not-found",
            Self::LoadedMatch => "loaded-match",
            Self::LoadedNoMatch => "loaded-no-match",
            Self::ParseError => "parse-error",
            Self::UnsafeValue => "unsafe-value",
            Self::FetchError => "fetch-error",
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct CurrentSidecarRegistryFetcher;

impl SidecarRegistryFetcher for CurrentSidecarRegistryFetcher {
    fn fetch(&self, request: &SidecarRegistryFetchRequest) -> SidecarRegistryFetchReport {
        SidecarRegistryFetchEngine::new(GhCliGithubProfileClient::default()).fetch(request)
    }
}

#[derive(Debug, Clone)]
struct SidecarRegistryFetchEngine<C> {
    github: C,
}

impl<C> SidecarRegistryFetchEngine<C> {
    const fn new(github: C) -> Self {
        Self { github }
    }
}

impl<C> SidecarRegistryFetcher for SidecarRegistryFetchEngine<C>
where
    C: GithubProfileClient,
{
    fn fetch(&self, request: &SidecarRegistryFetchRequest) -> SidecarRegistryFetchReport {
        let mut checked = Vec::new();
        let mut fetched = Vec::new();
        let mut remote_owner_kind: Option<Option<GithubAccountKind>> = None;

        for (attempt_index, attempt) in request.attempts().iter().enumerate() {
            match attempt.source() {
                SidecarRegistryFetchSource::LocalFile => {
                    fetch_local_file(attempt_index, attempt, &mut checked, &mut fetched);
                }
                SidecarRegistryFetchSource::GithubProfile
                | SidecarRegistryFetchSource::GithubOrganizationProfile => {
                    if attempt.is_authenticated_user_attempt() {
                        self.fetch_authenticated_user_profile(
                            request,
                            attempt_index,
                            attempt,
                            &mut checked,
                            &mut fetched,
                        );
                    } else if attempt.is_remote_owner_attempt() {
                        let kind = *remote_owner_kind.get_or_insert_with(|| {
                            self.github
                                .remote_owner_kind(&request.repository().owner)
                                .ok()
                                .flatten()
                        });
                        self.fetch_remote_owner_profile(
                            kind,
                            attempt_index,
                            attempt,
                            &mut checked,
                            &mut fetched,
                        );
                    }
                }
            }
        }

        SidecarRegistryFetchReport { checked, fetched }
    }
}

impl<C> SidecarRegistryFetchEngine<C>
where
    C: GithubProfileClient,
{
    fn fetch_authenticated_user_profile(
        &self,
        request: &SidecarRegistryFetchRequest,
        attempt_index: usize,
        attempt: &SidecarRegistryFetchAttempt,
        checked: &mut Vec<SidecarRegistryCheckedAttempt>,
        fetched: &mut Vec<SidecarRegistryFetchedRegistry>,
    ) {
        match self.github.authenticated_login() {
            Ok(Some(login)) => {
                let resolved = attempt.with_authenticated_login(&request.repository().host, &login);
                self.fetch_github_profile(attempt_index, &resolved, checked, fetched);
            }
            Ok(None) => checked.push(SidecarRegistryCheckedAttempt {
                attempt_index,
                attempt: attempt.clone(),
                status: SidecarRegistryFetchStatus::Skipped,
                message: Some("No authenticated GitHub user available".to_string()),
            }),
            Err(error) => checked.push(SidecarRegistryCheckedAttempt {
                attempt_index,
                attempt: attempt.clone(),
                status: SidecarRegistryFetchStatus::FetchError,
                message: Some(error),
            }),
        }
    }

    fn fetch_remote_owner_profile(
        &self,
        owner_kind: Option<GithubAccountKind>,
        attempt_index: usize,
        attempt: &SidecarRegistryFetchAttempt,
        checked: &mut Vec<SidecarRegistryCheckedAttempt>,
        fetched: &mut Vec<SidecarRegistryFetchedRegistry>,
    ) {
        if attempt.source() == SidecarRegistryFetchSource::GithubOrganizationProfile
            && owner_kind == Some(GithubAccountKind::User)
        {
            checked.push(SidecarRegistryCheckedAttempt {
                attempt_index,
                attempt: attempt
                    .with_identity_source(SidecarRegistryIdentitySource::RemoteOwnerUser),
                status: SidecarRegistryFetchStatus::Skipped,
                message: Some(
                    "Remote owner is a GitHub user; organization profile is not applicable"
                        .to_string(),
                ),
            });
            return;
        }

        if attempt.source() == SidecarRegistryFetchSource::GithubProfile
            && owner_kind == Some(GithubAccountKind::Organization)
        {
            checked.push(SidecarRegistryCheckedAttempt {
                attempt_index,
                attempt: attempt
                    .with_identity_source(SidecarRegistryIdentitySource::RemoteOwnerOrganization),
                status: SidecarRegistryFetchStatus::Skipped,
                message: Some(
                    "Remote owner is a GitHub organization; user profile is not applicable"
                        .to_string(),
                ),
            });
            return;
        }

        let identity_source = match owner_kind {
            Some(GithubAccountKind::User) => SidecarRegistryIdentitySource::RemoteOwnerUser,
            Some(GithubAccountKind::Organization) => {
                SidecarRegistryIdentitySource::RemoteOwnerOrganization
            }
            None => SidecarRegistryIdentitySource::RemoteOwnerUnknown,
        };
        let resolved = attempt.with_identity_source(identity_source);
        self.fetch_github_profile(attempt_index, &resolved, checked, fetched);
    }

    fn fetch_github_profile(
        &self,
        attempt_index: usize,
        attempt: &SidecarRegistryFetchAttempt,
        checked: &mut Vec<SidecarRegistryCheckedAttempt>,
        fetched: &mut Vec<SidecarRegistryFetchedRegistry>,
    ) {
        let Some((owner, repo)) = attempt.profile_owner_repo() else {
            checked.push(SidecarRegistryCheckedAttempt {
                attempt_index,
                attempt: attempt.clone(),
                status: SidecarRegistryFetchStatus::Skipped,
                message: Some(
                    "GitHub profile attempt did not include a profile repository".to_string(),
                ),
            });
            return;
        };
        let Some(path) = attempt.path() else {
            checked.push(SidecarRegistryCheckedAttempt {
                attempt_index,
                attempt: attempt.clone(),
                status: SidecarRegistryFetchStatus::FetchError,
                message: Some("GitHub profile attempt did not include a registry path".to_string()),
            });
            return;
        };

        match self.github.read_registry_file(owner, repo, path) {
            Ok(Some(content)) => {
                checked.push(SidecarRegistryCheckedAttempt {
                    attempt_index,
                    attempt: attempt.clone(),
                    status: SidecarRegistryFetchStatus::Fetched,
                    message: None,
                });
                fetched.push(SidecarRegistryFetchedRegistry {
                    attempt_index,
                    attempt: attempt.clone(),
                    content,
                });
            }
            Ok(None) => checked.push(SidecarRegistryCheckedAttempt {
                attempt_index,
                attempt: attempt.clone(),
                status: SidecarRegistryFetchStatus::NotFound,
                message: None,
            }),
            Err(error) => checked.push(SidecarRegistryCheckedAttempt {
                attempt_index,
                attempt: attempt.clone(),
                status: SidecarRegistryFetchStatus::FetchError,
                message: Some(error),
            }),
        }
    }
}

fn fetch_local_file(
    attempt_index: usize,
    attempt: &SidecarRegistryFetchAttempt,
    checked: &mut Vec<SidecarRegistryCheckedAttempt>,
    fetched: &mut Vec<SidecarRegistryFetchedRegistry>,
) {
    let Some(path) = attempt.local_path() else {
        checked.push(SidecarRegistryCheckedAttempt {
            attempt_index,
            attempt: attempt.clone(),
            status: SidecarRegistryFetchStatus::FetchError,
            message: Some("Local sidecar registry attempt did not include a file path".to_string()),
        });
        return;
    };

    match std::fs::read_to_string(path) {
        Ok(content) => {
            checked.push(SidecarRegistryCheckedAttempt {
                attempt_index,
                attempt: attempt.clone(),
                status: SidecarRegistryFetchStatus::Fetched,
                message: None,
            });
            fetched.push(SidecarRegistryFetchedRegistry {
                attempt_index,
                attempt: attempt.clone(),
                content,
            });
        }
        Err(error) => checked.push(SidecarRegistryCheckedAttempt {
            attempt_index,
            attempt: attempt.clone(),
            status: SidecarRegistryFetchStatus::FetchError,
            message: Some(format!("Failed to read sidecar registry file: {error}")),
        }),
    }
}

trait GithubProfileClient {
    fn authenticated_login(&self) -> Result<Option<String>, String>;
    fn remote_owner_kind(&self, owner: &str) -> Result<Option<GithubAccountKind>, String>;
    fn read_registry_file(
        &self,
        owner: &str,
        repo: &str,
        path: &str,
    ) -> Result<Option<String>, String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GithubAccountKind {
    User,
    Organization,
}

#[derive(Debug, Clone, Copy)]
struct GhCliGithubProfileClient<P = StdGhProcess> {
    process: P,
}

impl<P> GhCliGithubProfileClient<P> {
    const fn new(process: P) -> Self {
        Self { process }
    }
}

impl Default for GhCliGithubProfileClient<StdGhProcess> {
    fn default() -> Self {
        Self::new(StdGhProcess)
    }
}

impl<P> GithubProfileClient for GhCliGithubProfileClient<P>
where
    P: GhProcess,
{
    fn authenticated_login(&self) -> Result<Option<String>, String> {
        let output = self
            .process
            .output(&["api", "user", "--jq", ".login"])
            .map_err(|error| format!("Failed to run gh for authenticated GitHub user: {error}"))?;
        if output.status.success() {
            let login = output_stdout_trimmed(&output);
            return Ok((!login.is_empty()).then_some(login));
        }

        if is_auth_unavailable(&output) {
            return Ok(None);
        }

        Err(output_error(
            "Failed to resolve authenticated GitHub user",
            &output,
        ))
    }

    fn remote_owner_kind(&self, owner: &str) -> Result<Option<GithubAccountKind>, String> {
        let endpoint = format!("users/{owner}");
        let output = self
            .process
            .output(&["api", &endpoint, "--jq", ".type"])
            .map_err(|error| format!("Failed to run gh for GitHub owner lookup: {error}"))?;
        if !output.status.success() {
            return Ok(None);
        }

        match output_stdout_trimmed(&output).as_str() {
            "User" => Ok(Some(GithubAccountKind::User)),
            "Organization" => Ok(Some(GithubAccountKind::Organization)),
            _ => Ok(None),
        }
    }

    fn read_registry_file(
        &self,
        owner: &str,
        repo: &str,
        path: &str,
    ) -> Result<Option<String>, String> {
        let endpoint = format!("repos/{owner}/{repo}/contents/{path}");
        let output = self
            .process
            .output(&["api", "-H", "Accept: application/vnd.github.raw", &endpoint])
            .map_err(|error| format!("Failed to run gh for GitHub profile registry: {error}"))?;
        if output.status.success() {
            return Ok(Some(String::from_utf8_lossy(&output.stdout).to_string()));
        }

        if is_not_found(&output) {
            return Ok(None);
        }

        Err(output_error(
            "Failed to fetch GitHub profile sidecar registry",
            &output,
        ))
    }
}

trait GhProcess {
    fn output(&self, args: &[&str]) -> std::io::Result<Output>;
}

#[derive(Debug, Clone, Copy, Default)]
struct StdGhProcess;

impl GhProcess for StdGhProcess {
    fn output(&self, args: &[&str]) -> std::io::Result<Output> {
        ProcessCommand::new("gh").args(args).output()
    }
}

fn output_stdout_trimmed(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn output_stderr_trimmed(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_string()
}

fn output_error(context: &str, output: &Output) -> String {
    let stderr = output_stderr_trimmed(output);
    if stderr.is_empty() {
        format!("{context}: gh exited with {}", output.status)
    } else {
        format!("{context}: {stderr}")
    }
}

fn is_auth_unavailable(output: &Output) -> bool {
    let stderr = output_stderr_trimmed(output).to_ascii_lowercase();
    stderr.contains("not logged in")
        || stderr.contains("authentication required")
        || stderr.contains("requires authentication")
        || stderr.contains("http 401")
}

fn is_not_found(output: &Output) -> bool {
    let stderr = output_stderr_trimmed(output).to_ascii_lowercase();
    stderr.contains("http 404") || stderr.contains("not found")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::remote::parse_github_remote;
    use std::collections::BTreeMap;

    fn repository() -> ParsedGithubRemote {
        parse_github_remote("git@github.com:wycats/locald.git").expect("remote parses")
    }

    #[derive(Debug, Clone)]
    struct FakeGithubClient {
        authenticated_login: Result<Option<String>, String>,
        owner_kind: Result<Option<GithubAccountKind>, String>,
        files: BTreeMap<(String, String, String), Result<Option<String>, String>>,
    }

    impl Default for FakeGithubClient {
        fn default() -> Self {
            Self {
                authenticated_login: Ok(None),
                owner_kind: Ok(None),
                files: BTreeMap::new(),
            }
        }
    }

    impl FakeGithubClient {
        fn authenticated(login: &str) -> Self {
            Self {
                authenticated_login: Ok(Some(login.to_string())),
                ..Self::default()
            }
        }

        fn owner_kind(mut self, kind: GithubAccountKind) -> Self {
            self.owner_kind = Ok(Some(kind));
            self
        }

        fn with_file(mut self, owner: &str, repo: &str, content: &str) -> Self {
            self.files.insert(
                (
                    owner.to_string(),
                    repo.to_string(),
                    SIDECAR_REGISTRY_PROFILE_PATH.to_string(),
                ),
                Ok(Some(content.to_string())),
            );
            self
        }

        fn with_fetch_error(mut self, owner: &str, repo: &str, message: &str) -> Self {
            self.files.insert(
                (
                    owner.to_string(),
                    repo.to_string(),
                    SIDECAR_REGISTRY_PROFILE_PATH.to_string(),
                ),
                Err(message.to_string()),
            );
            self
        }
    }

    impl GithubProfileClient for FakeGithubClient {
        fn authenticated_login(&self) -> Result<Option<String>, String> {
            self.authenticated_login.clone()
        }

        fn remote_owner_kind(&self, _owner: &str) -> Result<Option<GithubAccountKind>, String> {
            self.owner_kind.clone()
        }

        fn read_registry_file(
            &self,
            owner: &str,
            repo: &str,
            path: &str,
        ) -> Result<Option<String>, String> {
            self.files
                .get(&(owner.to_string(), repo.to_string(), path.to_string()))
                .cloned()
                .unwrap_or(Ok(None))
        }
    }

    fn fetch_with(
        client: FakeGithubClient,
        request: &SidecarRegistryFetchRequest,
    ) -> SidecarRegistryFetchReport {
        SidecarRegistryFetchEngine::new(client).fetch(request)
    }

    #[test]
    fn github_profile_fetcher_local_file_success_returns_content_and_checked_attempt() {
        let temp = tempfile::tempdir().expect("tempdir");
        let registry = temp.path().join("sidecars.toml");
        std::fs::write(&registry, "version = 1\n").expect("write registry");
        let request = SidecarRegistryFetchRequest::for_discovery(&repository(), Some(&registry));

        let report = CurrentSidecarRegistryFetcher.fetch(&request);

        assert_eq!(report.checked.len(), 1);
        assert_eq!(report.checked[0].attempt_index, 0);
        assert_eq!(
            report.checked[0].attempt.source(),
            SidecarRegistryFetchSource::LocalFile
        );
        assert_eq!(
            report.checked[0].attempt.identity().source(),
            SidecarRegistryIdentitySource::RemoteOwnerUnknown
        );
        assert_eq!(report.checked[0].attempt.identity().login(), Some("wycats"));
        assert_eq!(
            report.checked[0].status,
            SidecarRegistryFetchStatus::Fetched
        );
        assert_eq!(report.checked[0].message, None);
        assert_eq!(report.fetched.len(), 1);
        assert_eq!(report.fetched[0].attempt_index, 0);
        assert_eq!(report.fetched[0].content, "version = 1\n");
    }

    #[test]
    fn github_profile_fetcher_local_file_missing_reports_fetch_error() {
        let temp = tempfile::tempdir().expect("tempdir");
        let registry = temp.path().join("missing.toml");
        let request = SidecarRegistryFetchRequest::for_discovery(&repository(), Some(&registry));

        let report = CurrentSidecarRegistryFetcher.fetch(&request);

        assert_eq!(report.checked.len(), 1);
        assert_eq!(report.checked[0].attempt_index, 0);
        assert_eq!(
            report.checked[0].attempt.source(),
            SidecarRegistryFetchSource::LocalFile
        );
        assert_eq!(
            report.checked[0].status,
            SidecarRegistryFetchStatus::FetchError
        );
        assert!(
            report.checked[0]
                .message
                .as_deref()
                .is_some_and(|message| message.contains("Failed to read sidecar registry file"))
        );
        assert!(report.fetched.is_empty());
    }

    #[test]
    fn github_profile_fetch_request_local_file_override_has_single_attempt() {
        let temp = tempfile::tempdir().expect("tempdir");
        let registry = temp.path().join("sidecars.toml");

        let request = SidecarRegistryFetchRequest::for_discovery(&repository(), Some(&registry));

        assert_eq!(request.attempts().len(), 1);
        assert_eq!(
            request.attempts()[0].source(),
            SidecarRegistryFetchSource::LocalFile
        );
        assert_eq!(request.attempts()[0].local_path(), Some(registry.as_path()));
    }

    #[test]
    fn github_profile_fetcher_request_without_registry_file_checks_profile_sources_in_order() {
        let request = SidecarRegistryFetchRequest::for_discovery(&repository(), None);

        assert_eq!(request.attempts().len(), 3);
        assert_eq!(
            request.attempts()[0].identity().source(),
            SidecarRegistryIdentitySource::AuthenticatedUser
        );
        assert_eq!(request.attempts()[0].profile_repo(), None);
        assert_eq!(
            request.attempts()[1].source(),
            SidecarRegistryFetchSource::GithubOrganizationProfile
        );
        assert_eq!(
            request.attempts()[1].identity().source(),
            SidecarRegistryIdentitySource::RemoteOwnerUnknown
        );
        assert_eq!(
            request.attempts()[1].profile_repo(),
            Some("github.com/wycats/.github")
        );
        assert_eq!(
            request.attempts()[2].source(),
            SidecarRegistryFetchSource::GithubProfile
        );
        assert_eq!(
            request.attempts()[2].profile_repo(),
            Some("github.com/wycats/wycats")
        );
    }

    #[test]
    fn github_profile_fetcher_authenticated_user_profile_records_fetched_registry() {
        let request = SidecarRegistryFetchRequest::for_discovery(&repository(), None);
        let report = fetch_with(
            FakeGithubClient::authenticated("alice").with_file("alice", "alice", "version = 1\n"),
            &request,
        );

        assert_eq!(report.checked[0].attempt_index, 0);
        assert_eq!(
            report.checked[0].attempt.identity().source(),
            SidecarRegistryIdentitySource::AuthenticatedUser
        );
        assert_eq!(report.checked[0].attempt.identity().login(), Some("alice"));
        assert_eq!(
            report.checked[0].attempt.profile_repo(),
            Some("github.com/alice/alice")
        );
        assert_eq!(
            report.checked[0].status,
            SidecarRegistryFetchStatus::Fetched
        );
        assert_eq!(report.fetched[0].attempt_index, 0);
        assert_eq!(report.fetched[0].content, "version = 1\n");
    }

    #[test]
    fn github_profile_fetcher_missing_auth_falls_back_to_unknown_owner_org_then_user() {
        let request = SidecarRegistryFetchRequest::for_discovery(&repository(), None);
        let report = fetch_with(
            FakeGithubClient::default().with_file("wycats", "wycats", "version = 1\n"),
            &request,
        );

        assert_eq!(report.checked.len(), 3);
        assert_eq!(
            report.checked[0].status,
            SidecarRegistryFetchStatus::Skipped
        );
        assert_eq!(
            report.checked[1].status,
            SidecarRegistryFetchStatus::NotFound
        );
        assert_eq!(
            report.checked[1].attempt.profile_repo(),
            Some("github.com/wycats/.github")
        );
        assert_eq!(
            report.checked[2].status,
            SidecarRegistryFetchStatus::Fetched
        );
        assert_eq!(
            report.checked[2].attempt.identity().source(),
            SidecarRegistryIdentitySource::RemoteOwnerUnknown
        );
        assert_eq!(
            report.checked[2].attempt.profile_repo(),
            Some("github.com/wycats/wycats")
        );
        assert_eq!(report.fetched[0].attempt_index, 2);
    }

    #[test]
    fn github_profile_fetcher_known_organization_uses_org_identity_and_skips_user_profile() {
        let request = SidecarRegistryFetchRequest::for_discovery(&repository(), None);
        let report = fetch_with(
            FakeGithubClient::default()
                .owner_kind(GithubAccountKind::Organization)
                .with_file("wycats", ".github", "version = 1\n"),
            &request,
        );

        assert_eq!(
            report.checked[1].status,
            SidecarRegistryFetchStatus::Fetched
        );
        assert_eq!(
            report.checked[1].attempt.identity().source(),
            SidecarRegistryIdentitySource::RemoteOwnerOrganization
        );
        assert_eq!(
            report.checked[2].status,
            SidecarRegistryFetchStatus::Skipped
        );
        assert_eq!(
            report.checked[2].attempt.identity().source(),
            SidecarRegistryIdentitySource::RemoteOwnerOrganization
        );
        assert_eq!(report.fetched[0].attempt_index, 1);
    }

    #[test]
    fn github_profile_fetcher_known_user_skips_org_profile_and_fetches_user_profile() {
        let request = SidecarRegistryFetchRequest::for_discovery(&repository(), None);
        let report = fetch_with(
            FakeGithubClient::default()
                .owner_kind(GithubAccountKind::User)
                .with_file("wycats", "wycats", "version = 1\n"),
            &request,
        );

        assert_eq!(
            report.checked[1].status,
            SidecarRegistryFetchStatus::Skipped
        );
        assert_eq!(
            report.checked[1].attempt.identity().source(),
            SidecarRegistryIdentitySource::RemoteOwnerUser
        );
        assert_eq!(
            report.checked[2].status,
            SidecarRegistryFetchStatus::Fetched
        );
        assert_eq!(
            report.checked[2].attempt.identity().source(),
            SidecarRegistryIdentitySource::RemoteOwnerUser
        );
        assert_eq!(report.fetched[0].attempt_index, 2);
    }

    #[test]
    fn github_profile_fetcher_reports_not_found_and_fetch_error_without_live_github() {
        let request = SidecarRegistryFetchRequest::for_discovery(&repository(), None);
        let report = fetch_with(
            FakeGithubClient::default().with_fetch_error("wycats", ".github", "permission denied"),
            &request,
        );

        assert_eq!(
            report.checked[1].status,
            SidecarRegistryFetchStatus::FetchError
        );
        assert_eq!(
            report.checked[1].message.as_deref(),
            Some("permission denied")
        );
        assert_eq!(
            report.checked[2].status,
            SidecarRegistryFetchStatus::NotFound
        );
        assert!(report.fetched.is_empty());
    }
}
