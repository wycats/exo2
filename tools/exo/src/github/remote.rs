use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedGithubRemote {
    pub(crate) host: String,
    pub(crate) owner: String,
    pub(crate) repo: String,
    pub(crate) original: String,
    pub(crate) project_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GithubRemoteParseError {
    UnsupportedScheme,
    NotGithubRemote,
    MalformedRemote,
}

impl fmt::Display for GithubRemoteParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedScheme => f.write_str("unsupported Git remote scheme"),
            Self::NotGithubRemote => f.write_str("remote is not hosted on github.com"),
            Self::MalformedRemote => f.write_str("malformed GitHub remote"),
        }
    }
}

impl std::error::Error for GithubRemoteParseError {}

pub(crate) fn parse_github_remote(
    remote: &str,
) -> Result<ParsedGithubRemote, GithubRemoteParseError> {
    if remote.trim() != remote {
        return Err(GithubRemoteParseError::MalformedRemote);
    }

    let (host, path) = if let Some(rest) = remote.strip_prefix("https://") {
        split_url_remote(rest)?
    } else if let Some(rest) = remote.strip_prefix("ssh://git@") {
        split_url_remote(rest)?
    } else if let Some(rest) = remote.strip_prefix("git@") {
        split_scp_remote(rest)?
    } else if remote.contains("://") {
        return Err(GithubRemoteParseError::UnsupportedScheme);
    } else {
        return Err(GithubRemoteParseError::MalformedRemote);
    };

    let host = host.to_ascii_lowercase();
    if host != "github.com" {
        return Err(GithubRemoteParseError::NotGithubRemote);
    }

    let (owner, repo) = split_owner_repo(path)?;
    let project_key = format!("{host}/{owner}/{repo}");

    Ok(ParsedGithubRemote {
        host,
        owner,
        repo,
        original: remote.to_string(),
        project_key,
    })
}

fn split_url_remote(remote: &str) -> Result<(&str, &str), GithubRemoteParseError> {
    let (host, path) = remote
        .split_once('/')
        .ok_or(GithubRemoteParseError::MalformedRemote)?;
    if host.is_empty() || path.is_empty() {
        return Err(GithubRemoteParseError::MalformedRemote);
    }
    Ok((host, path))
}

fn split_scp_remote(remote: &str) -> Result<(&str, &str), GithubRemoteParseError> {
    let (host, path) = remote
        .split_once(':')
        .ok_or(GithubRemoteParseError::MalformedRemote)?;
    if host.is_empty() || path.is_empty() {
        return Err(GithubRemoteParseError::MalformedRemote);
    }
    Ok((host, path))
}

fn split_owner_repo(path: &str) -> Result<(String, String), GithubRemoteParseError> {
    let mut parts = path.split('/');
    let owner = parts
        .next()
        .filter(|value| !value.is_empty())
        .ok_or(GithubRemoteParseError::MalformedRemote)?;
    let repo = parts
        .next()
        .filter(|value| !value.is_empty())
        .ok_or(GithubRemoteParseError::MalformedRemote)?;

    if parts.next().is_some() {
        return Err(GithubRemoteParseError::MalformedRemote);
    }

    let repo = repo.strip_suffix(".git").unwrap_or(repo);
    if repo.is_empty() || repo == ".git" || owner.contains(':') || repo.contains(':') {
        return Err(GithubRemoteParseError::MalformedRemote);
    }

    Ok((owner.to_string(), repo.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_remote(remote: &str, owner: &str, repo: &str) {
        let parsed = parse_github_remote(remote).expect("remote should parse");
        assert_eq!(parsed.host, "github.com");
        assert_eq!(parsed.owner, owner);
        assert_eq!(parsed.repo, repo);
        assert_eq!(parsed.original, remote);
        assert_eq!(parsed.project_key, format!("github.com/{owner}/{repo}"));
    }

    #[test]
    fn github_profile_remote_parser_accepts_https_with_git_suffix() {
        assert_remote("https://github.com/wycats/locald.git", "wycats", "locald");
    }

    #[test]
    fn github_profile_remote_parser_accepts_https_without_git_suffix() {
        assert_remote("https://github.com/my-org/project", "my-org", "project");
    }

    #[test]
    fn github_profile_remote_parser_accepts_scp_ssh_syntax() {
        assert_remote("git@github.com:wycats/locald.git", "wycats", "locald");
    }

    #[test]
    fn github_profile_remote_parser_accepts_ssh_url_syntax() {
        assert_remote("ssh://git@github.com/wycats/locald", "wycats", "locald");
    }

    #[test]
    fn github_profile_remote_parser_accepts_owner_with_dots() {
        assert_remote(
            "https://github.com/example-inc.my/team-tool.git",
            "example-inc.my",
            "team-tool",
        );
    }

    #[test]
    fn github_profile_remote_parser_rejects_non_github_hosts() {
        assert_eq!(
            parse_github_remote("https://gitlab.com/foo/bar").unwrap_err(),
            GithubRemoteParseError::NotGithubRemote,
        );
    }

    #[test]
    fn github_profile_remote_parser_rejects_missing_repo() {
        assert_eq!(
            parse_github_remote("https://github.com/foo").unwrap_err(),
            GithubRemoteParseError::MalformedRemote,
        );
        assert_eq!(
            parse_github_remote("git@github.com:foo").unwrap_err(),
            GithubRemoteParseError::MalformedRemote,
        );
    }

    #[test]
    fn github_profile_remote_parser_rejects_unsupported_scheme() {
        assert_eq!(
            parse_github_remote("HTTPS://github.com/wycats/locald.git").unwrap_err(),
            GithubRemoteParseError::UnsupportedScheme,
        );
    }

    #[test]
    fn github_profile_remote_parser_rejects_surrounding_whitespace() {
        assert_eq!(
            parse_github_remote(" https://github.com/wycats/locald.git").unwrap_err(),
            GithubRemoteParseError::MalformedRemote,
        );
    }
}
