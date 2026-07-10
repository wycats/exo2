//! Source-build activation checks for the local dogfood MCP proxy.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};

pub const DOGFOOD_ACTIVATION_ENV: &str = "EXO_DOGFOOD_ACTIVATION";
const DOGFOOD_ACTIVATION_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub struct DogfoodActivation {
    configured: bool,
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

#[derive(Debug, Clone, Deserialize)]
struct DogfoodActivationRecord {
    version: u32,
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
    size_bytes: u64,
    modified_unix_ms: Option<u128>,
}

impl DogfoodActivation {
    pub fn from_environment() -> Self {
        let Some(path) = std::env::var_os(DOGFOOD_ACTIVATION_ENV).map(PathBuf::from) else {
            return Self {
                configured: false,
                record: Err("dogfood activation is not configured".to_string()),
            };
        };
        let record = fs::read(&path)
            .map_err(|error| format!("failed to read dogfood activation: {error}"))
            .and_then(|content| {
                serde_json::from_slice(&content)
                    .map_err(|error| format!("failed to parse dogfood activation: {error}"))
            });
        Self {
            configured: true,
            record,
        }
    }

    pub fn status(
        &self,
        proxy_path: &Path,
        worker_identity: Option<&JsonValue>,
    ) -> DogfoodActivationStatus {
        if !self.configured {
            return DogfoodActivationStatus {
                configured: false,
                ok: true,
                state: "not_configured",
                issue: None,
            };
        }
        let record = match &self.record {
            Ok(record) if record.version == DOGFOOD_ACTIVATION_VERSION => record,
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
            && file_blake3(&record.source.exo.path).ok().as_deref()
                != worker_identity
                    .pointer("/executable_identity/stable_hash")
                    .and_then(JsonValue::as_str)
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

    pub fn ensure_before_worker(&self, proxy_path: &Path) -> Result<(), String> {
        let status = self.status(proxy_path, None);
        status.ok.then_some(()).ok_or_else(|| {
            status
                .issue
                .unwrap_or_else(|| "dogfood activation is not current".to_string())
        })
    }

    pub fn ensure_worker(
        &self,
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
        (record.version == DOGFOOD_ACTIVATION_VERSION).then_some(record.source.exo.path)
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
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    let modified_unix_ms = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis());
    if metadata.len() == expected.size_bytes && modified_unix_ms == expected.modified_unix_ms {
        return true;
    }
    file_blake3(path).is_ok_and(|hash| hash == expected.blake3)
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
    use super::*;

    fn fingerprint(path: &Path) -> DogfoodActivationBinary {
        let metadata = fs::metadata(path).expect("stat fixture");
        DogfoodActivationBinary {
            path: path.to_path_buf(),
            blake3: file_blake3(path).expect("hash fixture"),
            size_bytes: metadata.len(),
            modified_unix_ms: metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis()),
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
            record: Ok(DogfoodActivationRecord {
                version: DOGFOOD_ACTIVATION_VERSION,
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
        let activation = activation(&source_exo, &source_mcp, &installed_exo, &installed_mcp);
        let worker = json!({ "executable_identity": { "stable_hash": file_blake3(&source_exo).expect("hash") } });

        let status = activation.status(&installed_mcp, Some(&worker));
        assert!(status.ok);
        assert_eq!(status.state, "current");
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
        let activation = activation(&source_exo, &source_mcp, &installed_exo, &installed_mcp);
        fs::write(&source_exo, "new source build").expect("update source build");

        let worker = json!({ "executable_identity": { "stable_hash": file_blake3(&installed_exo).expect("hash") } });
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
        let activation = activation(&source_exo, &source_mcp, &installed_exo, &installed_mcp);
        fs::write(&source_mcp, "rebuilt source proxy").expect("rebuild source proxy");
        let worker = json!({ "executable_identity": { "stable_hash": file_blake3(&source_exo).expect("hash") } });

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
}
