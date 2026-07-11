//! Source-build activation checks for the local dogfood MCP proxy.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};

use crate::mcp::ExecutableIdentity;

pub const DOGFOOD_ACTIVATION_ENV: &str = "EXO_DOGFOOD_ACTIVATION";
const DOGFOOD_ACTIVATION_VERSION: u32 = 2;

#[derive(Debug, Clone)]
pub struct DogfoodActivation {
    configured: bool,
    activation_path: Option<PathBuf>,
    record: Result<DogfoodActivationRecord, String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DogfoodActivationStatus {
    pub configured: bool,
    pub ok: bool,
    pub state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issue: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DogfoodActivationBinding {
    pub activation_path: PathBuf,
    pub pinned_mcp_config: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
struct DogfoodActivationRecord {
    version: u32,
    #[serde(default)]
    pinned_mcp_config: Option<PathBuf>,
    source: DogfoodActivationBinaries,
    installed: DogfoodActivationBinaries,
}

#[derive(Debug, Clone, Deserialize)]
struct DogfoodActivationBinaries {
    exo: DogfoodActivationBinary,
    exo_mcp: DogfoodActivationBinary,
}

#[derive(Debug, Clone, Deserialize)]
struct DogfoodActivationBinary {
    path: PathBuf,
    blake3: String,
}

impl DogfoodActivation {
    pub fn from_environment() -> Self {
        let Some(path) = std::env::var_os(DOGFOOD_ACTIVATION_ENV).map(PathBuf::from) else {
            return Self {
                configured: false,
                activation_path: None,
                record: Err("dogfood activation is not configured".to_string()),
            };
        };
        Self {
            configured: true,
            activation_path: Some(path.clone()),
            record: Self::read_record(&path),
        }
    }

    pub fn status(
        &mut self,
        proxy_path: &Path,
        worker_identity: Option<&JsonValue>,
    ) -> DogfoodActivationStatus {
        self.reload();
        if !self.configured {
            return DogfoodActivationStatus {
                configured: false,
                ok: true,
                state: "not_configured",
                issue: None,
            };
        }
        let record = match &self.record {
            Ok(record) if matches!(record.version, 1 | DOGFOOD_ACTIVATION_VERSION) => record,
            Ok(_) => {
                return self.failure(
                    "unsupported_activation",
                    "dogfood activation has an unsupported version",
                );
            }
            Err(error) => return self.failure("invalid_activation", error),
        };

        if !record.source.exo.path.is_file() || !record.source.exo_mcp.path.is_file() {
            return self.failure(
                "source_build_missing",
                "the source Exo build recorded by dogfood activation is unavailable; run `cargo dogfood-exo` from the source checkout",
            );
        }
        if !path_matches(&record.source.exo_mcp.path, proxy_path)
            && !fingerprint_matches_path_for(&record.installed.exo_mcp, proxy_path)
        {
            return self.failure(
                "proxy_binary_changed",
                "the Exo MCP proxy no longer matches its dogfood activation; run `cargo dogfood-exo` from the source checkout",
            );
        }
        if let Some(worker_identity) = worker_identity
            && !worker_matches_source(&record.source.exo, worker_identity)
        {
            return self.failure(
                "worker_source_mismatch",
                "the Exo MCP worker does not match the current source build; the proxy will reconnect through the source worker before the next tool call",
            );
        }

        DogfoodActivationStatus {
            configured: true,
            ok: true,
            state: "current",
            issue: None,
        }
    }

    pub fn ensure_before_worker(&mut self, proxy_path: &Path) -> Result<(), String> {
        let status = self.status(proxy_path, None);
        status.ok.then_some(()).ok_or_else(|| {
            status
                .issue
                .unwrap_or_else(|| "dogfood activation is not current".to_string())
        })
    }

    pub fn ensure_worker(
        &mut self,
        proxy_path: &Path,
        worker_identity: &JsonValue,
    ) -> Result<(), String> {
        let status = self.status(proxy_path, Some(worker_identity));
        status.ok.then_some(()).ok_or_else(|| {
            status
                .issue
                .unwrap_or_else(|| "dogfood activation is not current".to_string())
        })
    }

    pub fn source_worker_path_from_environment() -> Option<PathBuf> {
        let path = std::env::var_os(DOGFOOD_ACTIVATION_ENV).map(PathBuf::from)?;
        let content = fs::read(path).ok()?;
        let record = serde_json::from_slice::<DogfoodActivationRecord>(&content).ok()?;
        matches!(record.version, 1 | DOGFOOD_ACTIVATION_VERSION).then_some(record.source.exo.path)
    }

    pub fn pinned_mcp_config_from_environment() -> Result<Option<PathBuf>, String> {
        Ok(Self::binding_from_environment()?.map(|binding| binding.pinned_mcp_config))
    }

    pub fn binding_from_environment() -> Result<Option<DogfoodActivationBinding>, String> {
        let Some(path) = std::env::var_os(DOGFOOD_ACTIVATION_ENV).map(PathBuf::from) else {
            return Ok(None);
        };
        let activation_path = path.canonicalize().map_err(|error| {
            format!(
                "failed to resolve dogfood activation {}: {error}",
                path.display()
            )
        })?;
        let pinned_mcp_config = Self::pinned_mcp_config_from_path(&activation_path)?;
        Ok(Some(DogfoodActivationBinding {
            activation_path,
            pinned_mcp_config,
        }))
    }

    fn pinned_mcp_config_from_path(path: &Path) -> Result<PathBuf, String> {
        let record = Self::read_record(path)?;
        if record.version != DOGFOOD_ACTIVATION_VERSION {
            return Err(format!(
                "dogfood activation version {} does not identify the pinned Codex plugin config; run `cargo dogfood-exo` from the configured source checkout",
                record.version
            ));
        }
        let configured = record.pinned_mcp_config.ok_or_else(|| {
            "dogfood activation does not identify the pinned Codex plugin config; run `cargo dogfood-exo` from the configured source checkout"
                .to_string()
        })?;
        if !configured.is_file() {
            return Err(format!(
                "the pinned Codex plugin config is unavailable at {}; run `cargo dogfood-exo` from the configured source checkout",
                configured.display()
            ));
        }
        configured.canonicalize().map_err(|error| {
            format!(
                "failed to resolve pinned Codex plugin config {}: {error}",
                configured.display()
            )
        })
    }

    fn reload(&mut self) {
        if let Some(path) = &self.activation_path {
            self.record = Self::read_record(path);
        }
    }

    fn read_record(path: &Path) -> Result<DogfoodActivationRecord, String> {
        fs::read(path)
            .map_err(|error| format!("failed to read dogfood activation: {error}"))
            .and_then(|content| {
                serde_json::from_slice(&content)
                    .map_err(|error| format!("failed to parse dogfood activation: {error}"))
            })
    }

    fn failure(&self, state: &'static str, issue: impl Into<String>) -> DogfoodActivationStatus {
        DogfoodActivationStatus {
            configured: true,
            ok: false,
            state,
            issue: Some(issue.into()),
        }
    }
}

fn fingerprint_matches_path_for(expected: &DogfoodActivationBinary, path: &Path) -> bool {
    file_blake3(path).is_ok_and(|hash| hash == expected.blake3)
}

fn worker_matches_source(expected: &DogfoodActivationBinary, worker_identity: &JsonValue) -> bool {
    let expected_path = expected
        .path
        .canonicalize()
        .unwrap_or_else(|_| expected.path.clone());
    let worker_path = worker_identity
        .get("executable_path")
        .and_then(JsonValue::as_str)
        .map(PathBuf::from)
        .map(|path| path.canonicalize().unwrap_or(path));
    let worker_executable_identity = worker_identity
        .get("executable_identity")
        .cloned()
        .and_then(|value| serde_json::from_value::<ExecutableIdentity>(value).ok());
    #[cfg(unix)]
    let identity_matches = worker_executable_identity.as_ref().is_some_and(|identity| {
        identity
            .metadata_matches_path(&expected.path)
            .unwrap_or(false)
    });
    #[cfg(not(unix))]
    let identity_matches = worker_executable_identity.as_ref().is_some_and(|identity| {
        crate::mcp::executable_identity_matches_path(identity, &expected.path).unwrap_or(false)
    });
    worker_path.as_deref() == Some(expected_path.as_path())
        && identity_matches
}

fn path_matches(expected: &Path, actual: &Path) -> bool {
    let expected = expected
        .canonicalize()
        .unwrap_or_else(|_| expected.to_path_buf());
    let actual = actual
        .canonicalize()
        .unwrap_or_else(|_| actual.to_path_buf());
    expected == actual
}

fn file_blake3(path: &Path) -> Result<String, std::io::Error> {
    let mut file = fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

pub fn status_value(status: DogfoodActivationStatus) -> JsonValue {
    json!(status)
}

#[cfg(test)]
mod tests {
    use crate::mcp::{executable_identity_for_path, test_support};

    use super::*;

    fn fingerprint(path: &Path) -> DogfoodActivationBinary {
        DogfoodActivationBinary {
            path: path.to_path_buf(),
            blake3: file_blake3(path).expect("hash fixture"),
        }
    }

    fn activation(
        source_exo: &Path,
        source_mcp: &Path,
        installed_exo: &Path,
        installed_mcp: &Path,
    ) -> DogfoodActivation {
        DogfoodActivation {
            configured: true,
            activation_path: None,
            record: Ok(DogfoodActivationRecord {
                version: DOGFOOD_ACTIVATION_VERSION,
                pinned_mcp_config: Some(source_mcp.to_path_buf()),
                source: DogfoodActivationBinaries {
                    exo: fingerprint(source_exo),
                    exo_mcp: fingerprint(source_mcp),
                },
                installed: DogfoodActivationBinaries {
                    exo: fingerprint(installed_exo),
                    exo_mcp: fingerprint(installed_mcp),
                },
            }),
        }
    }

    #[test]
    fn unchanged_activation_is_current() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source_exo = temp.path().join("source-exo");
        let source_mcp = temp.path().join("source-mcp");
        let installed_exo = temp.path().join("installed-exo");
        let installed_mcp = temp.path().join("installed-mcp");
        for path in [&source_exo, &source_mcp, &installed_exo, &installed_mcp] {
            fs::write(path, path.as_os_str().as_encoded_bytes()).expect("write fixture");
        }
        let mut activation = activation(&source_exo, &source_mcp, &installed_exo, &installed_mcp);
        let worker = json!({
            "executable_path": source_exo,
            "executable_identity": executable_identity_for_path(&source_exo).expect("identity")
        });

        let status = activation.status(&installed_mcp, Some(&worker));
        assert!(status.ok);
        assert_eq!(status.state, "current");
    }

    #[test]
    fn repeated_activation_checks_do_not_rehash_the_source_worker() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source_exo = temp.path().join("source-exo");
        let source_mcp = temp.path().join("source-mcp");
        let installed_exo = temp.path().join("installed-exo");
        let installed_mcp = temp.path().join("installed-mcp");
        for path in [&source_exo, &source_mcp, &installed_exo, &installed_mcp] {
            fs::write(path, path.as_os_str().as_encoded_bytes()).expect("write fixture");
        }
        let mut activation = activation(&source_exo, &source_mcp, &installed_exo, &installed_mcp);
        let worker = json!({
            "executable_path": source_exo,
            "executable_identity": executable_identity_for_path(&source_exo).expect("identity")
        });
        test_support::reset_stable_file_hash_calls();

        assert!(activation.status(&installed_mcp, Some(&worker)).ok);
        assert!(activation.status(&installed_mcp, Some(&worker)).ok);
        assert_eq!(test_support::stable_file_hash_calls(), 0);
    }

    #[test]
    fn changed_source_build_is_detected_against_the_running_worker() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source_exo = temp.path().join("source-exo");
        let source_mcp = temp.path().join("source-mcp");
        let installed_exo = temp.path().join("installed-exo");
        let installed_mcp = temp.path().join("installed-mcp");
        for path in [&source_exo, &source_mcp, &installed_exo, &installed_mcp] {
            fs::write(path, path.as_os_str().as_encoded_bytes()).expect("write fixture");
        }
        let mut activation = activation(&source_exo, &source_mcp, &installed_exo, &installed_mcp);
        let stale_identity = executable_identity_for_path(&source_exo).expect("identity");
        fs::write(&source_exo, "new source build").expect("update source build");

        let worker = json!({
            "executable_path": source_exo,
            "executable_identity": stale_identity,
        });
        let status = activation.status(&installed_mcp, Some(&worker));
        assert!(!status.ok);
        assert_eq!(status.state, "worker_source_mismatch");
        assert!(status.issue.expect("issue").contains("reconnect"));
    }

    #[test]
    fn rebuilt_source_proxy_remains_a_valid_activation_launcher() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source_exo = temp.path().join("source-exo");
        let source_mcp = temp.path().join("source-mcp");
        let installed_exo = temp.path().join("installed-exo");
        let installed_mcp = temp.path().join("installed-mcp");
        for path in [&source_exo, &source_mcp, &installed_exo, &installed_mcp] {
            fs::write(path, path.as_os_str().as_encoded_bytes()).expect("write fixture");
        }
        let mut activation = activation(&source_exo, &source_mcp, &installed_exo, &installed_mcp);
        fs::write(&source_mcp, "rebuilt source proxy").expect("rebuild source proxy");
        let worker = json!({
            "executable_path": source_exo,
            "executable_identity": executable_identity_for_path(&source_exo).expect("identity")
        });

        let status = activation.status(&source_mcp, Some(&worker));
        assert!(status.ok);
        assert_eq!(status.state, "current");
    }

    #[test]
    fn file_blake3_matches_the_in_memory_digest_for_large_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("large-fixture");
        let bytes = (0..(64 * 1024 * 3 + 37))
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>();
        fs::write(&path, &bytes).expect("write fixture");

        assert_eq!(
            file_blake3(&path).expect("stream fixture"),
            blake3::hash(&bytes).to_hex().to_string()
        );
    }

    #[cfg(unix)]
    #[test]
    fn fingerprint_rejects_same_size_same_mtime_content_replacement() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("proxy");
        let reference = temp.path().join("reference");
        fs::write(&path, "original").expect("write proxy");
        fs::copy(&path, &reference).expect("copy timestamp reference");
        let expected = fingerprint(&path);
        fs::write(&path, "replaced").expect("replace proxy with same size");
        let status = std::process::Command::new("touch")
            .args([
                "-r",
                reference.to_str().expect("reference path"),
                path.to_str().expect("proxy path"),
            ])
            .status()
            .expect("restore mtime");
        assert!(status.success());

        assert!(!fingerprint_matches_path_for(&expected, &path));
    }

    #[test]
    fn pinned_mcp_config_resolves_the_recorded_config() {
        let temp = tempfile::tempdir().expect("tempdir");
        let activation_path = temp.path().join("activation.json");
        let pinned_config = temp.path().join("cache/exo/.mcp.json");
        fs::create_dir_all(pinned_config.parent().expect("config parent"))
            .expect("create plugin cache");
        fs::write(&pinned_config, "{}").expect("write pinned config");
        fs::write(
            &activation_path,
            serde_json::to_vec(&json!({
                "version": DOGFOOD_ACTIVATION_VERSION,
                "pinned_mcp_config": pinned_config,
                "source": {
                    "exo": { "path": "source-exo", "blake3": "source-exo" },
                    "exo_mcp": { "path": "source-mcp", "blake3": "source-mcp" }
                },
                "installed": {
                    "exo": { "path": "installed-exo", "blake3": "installed-exo" },
                    "exo_mcp": { "path": "installed-mcp", "blake3": "installed-mcp" }
                }
            }))
            .expect("serialize activation"),
        )
        .expect("write activation");

        assert_eq!(
            DogfoodActivation::pinned_mcp_config_from_path(&activation_path)
                .expect("resolve pinned config"),
            pinned_config
                .canonicalize()
                .expect("canonical pinned config")
        );
    }

    #[test]
    fn pinned_mcp_config_reports_missing_recorded_config() {
        let temp = tempfile::tempdir().expect("tempdir");
        let activation_path = temp.path().join("activation.json");
        let missing = temp.path().join("missing/.mcp.json");
        fs::write(
            &activation_path,
            serde_json::to_vec(&json!({
                "version": DOGFOOD_ACTIVATION_VERSION,
                "pinned_mcp_config": missing,
                "source": {
                    "exo": { "path": "source-exo", "blake3": "source-exo" },
                    "exo_mcp": { "path": "source-mcp", "blake3": "source-mcp" }
                },
                "installed": {
                    "exo": { "path": "installed-exo", "blake3": "installed-exo" },
                    "exo_mcp": { "path": "installed-mcp", "blake3": "installed-mcp" }
                }
            }))
            .expect("serialize activation"),
        )
        .expect("write activation");

        let error = DogfoodActivation::pinned_mcp_config_from_path(&activation_path)
            .expect_err("missing pinned config must fail");
        assert!(error.contains("pinned Codex plugin config is unavailable"));
        assert!(error.contains("cargo dogfood-exo"));
    }
}
