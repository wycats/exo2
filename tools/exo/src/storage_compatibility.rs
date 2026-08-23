use crate::api::protocol::ErrorCode;
use crate::failure::ExoFailure;
use exosuit_storage::{DatabaseError, WriterCompatibilityError};

pub fn map_database_error(error: DatabaseError) -> anyhow::Error {
    match error {
        DatabaseError::WriterCompatibility(error) => map_writer_compatibility_error(error),
        other => anyhow::Error::new(other),
    }
}

pub fn map_writer_compatibility_error(error: WriterCompatibilityError) -> anyhow::Error {
    match writer_compatibility_failure(&error) {
        Some(failure) => anyhow::Error::new(failure),
        None => anyhow::Error::new(error),
    }
}

pub fn writer_compatibility_failure_from_error(error: &anyhow::Error) -> Option<ExoFailure> {
    for cause in error.chain() {
        if let Some(failure) = cause.downcast_ref::<ExoFailure>()
            && failure
                .error
                .details
                .as_ref()
                .is_some_and(is_writer_compatibility_details)
        {
            return Some(failure.clone());
        }
        if let Some(error) = cause.downcast_ref::<WriterCompatibilityError>()
            && let Some(failure) = writer_compatibility_failure(error)
        {
            return Some(failure);
        }
        if let Some(DatabaseError::WriterCompatibility(error)) =
            cause.downcast_ref::<DatabaseError>()
            && let Some(failure) = writer_compatibility_failure(error)
        {
            return Some(failure);
        }
    }
    None
}

pub fn storage_failure_from_error(error: &anyhow::Error) -> Option<ExoFailure> {
    writer_compatibility_failure_from_error(error).or_else(|| {
        error.chain().find_map(|cause| {
            let failure = cause.downcast_ref::<ExoFailure>()?;
            failure
                .error
                .details
                .as_ref()
                .and_then(|details| details.get("kind"))
                .and_then(serde_json::Value::as_str)
                .is_some_and(|kind| kind.starts_with("storage."))
                .then(|| failure.clone())
        })
    })
}

pub fn is_writer_compatibility_details(details: &serde_json::Value) -> bool {
    details
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|kind| {
            kind.starts_with("storage.writer_") || kind == "storage.compatibility_busy"
        })
}

fn writer_compatibility_failure(error: &WriterCompatibilityError) -> Option<ExoFailure> {
    let kind = error.kind();
    let details = match &error {
        WriterCompatibilityError::Incompatible {
            required_generation,
            supported_generation,
            surface,
        } => serde_json::json!({
            "kind": kind,
            "required_generation": required_generation,
            "supported_generation": supported_generation,
            "state_surface": surface.as_str(),
            "upgrade_action": "Run this command with an Exo version that supports the stored writer generation.",
            "request_outcome_checked": false,
            "retry_with_same_request_id": true,
            "retryable": false,
        }),
        WriterCompatibilityError::MetadataInvalid { surface, reason } => serde_json::json!({
            "kind": kind,
            "state_surface": surface.as_str(),
            "reason": reason,
            "request_outcome_checked": false,
            "retry_with_same_request_id": true,
            "retryable": false,
        }),
        WriterCompatibilityError::Busy { lock_path } => serde_json::json!({
            "kind": kind,
            "lock_path": lock_path,
            "request_outcome_checked": false,
            "retry_with_same_request_id": true,
            "retryable": true,
        }),
        WriterCompatibilityError::Io { .. } => return None,
    };

    Some(
        ExoFailure::new(
            ErrorCode::PreconditionFailed,
            error.to_string(),
            ExoFailure::orienting_steering(vec![]),
        )
        .with_details(details),
    )
}

pub fn projection_unsettled_error(state: &str, has_conflicts: bool) -> anyhow::Error {
    anyhow::Error::new(
        ExoFailure::new(
            ErrorCode::PreconditionFailed,
            "repo-policy SQL projection is quarantined while Git integration is unsettled",
            ExoFailure::orienting_steering(vec![]),
        )
        .with_details(serde_json::json!({
            "kind": "storage.projection_unsettled",
            "repository_state": state,
            "has_conflicts": has_conflicts,
            "request_outcome_checked": false,
            "retry_with_same_request_id": true,
            "retryable": true,
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use exosuit_storage::StateSurface;

    #[test]
    fn incompatibility_maps_to_stable_precondition_failure() {
        let error = map_writer_compatibility_error(WriterCompatibilityError::Incompatible {
            required_generation: 1,
            supported_generation: 0,
            surface: StateSurface::Database,
        });
        let failure = error.downcast_ref::<ExoFailure>().expect("ExoFailure");
        assert_eq!(failure.error.code, ErrorCode::PreconditionFailed);
        assert_eq!(
            failure.error.details.as_ref().unwrap()["kind"],
            "storage.writer_incompatible"
        );
        assert_eq!(
            failure.error.details.as_ref().unwrap()["state_surface"],
            "database"
        );
    }

    #[test]
    fn all_typed_compatibility_failures_preserve_retry_contract() {
        for (error, expected_kind, retryable) in [
            (
                WriterCompatibilityError::Incompatible {
                    required_generation: 1,
                    supported_generation: 0,
                    surface: StateSurface::Database,
                },
                "storage.writer_incompatible",
                false,
            ),
            (
                WriterCompatibilityError::MetadataInvalid {
                    surface: StateSurface::Projection,
                    reason: "bad header".to_string(),
                },
                "storage.writer_metadata_invalid",
                false,
            ),
            (
                WriterCompatibilityError::Busy {
                    lock_path: std::path::PathBuf::from("/tmp/exo.writer-compat.lock"),
                },
                "storage.compatibility_busy",
                true,
            ),
        ] {
            let mapped = map_writer_compatibility_error(error)
                .context("canonical storage consumer")
                .context("request context preload");
            let failure = writer_compatibility_failure_from_error(&mapped)
                .expect("wrapped compatibility failure");
            let details = failure.error.details.as_ref().unwrap();
            assert_eq!(details["kind"], expected_kind);
            assert_eq!(details["request_outcome_checked"], false);
            assert_eq!(details["retry_with_same_request_id"], true);
            assert_eq!(details["retryable"], retryable);
        }
    }

    #[test]
    fn unrelated_wrapped_database_error_is_not_misclassified() {
        let error = anyhow::Error::new(DatabaseError::Sqlite(
            exosuit_storage::rusqlite::Error::InvalidQuery,
        ))
        .context("wrapped database failure");
        assert!(writer_compatibility_failure_from_error(&error).is_none());
    }

    #[test]
    fn wrapped_projection_quarantine_is_a_storage_failure() {
        let error = projection_unsettled_error("MERGE_HEAD", true)
            .context("Failed to preflight canonical projection")
            .context("workspace preload");
        let failure = storage_failure_from_error(&error).expect("wrapped storage failure");
        assert_eq!(failure.error.code, ErrorCode::PreconditionFailed);
        let details = failure.error.details.expect("storage details");
        assert_eq!(details["kind"], "storage.projection_unsettled");
        assert_eq!(details["repository_state"], "MERGE_HEAD");
        assert_eq!(details["request_outcome_checked"], false);
        assert_eq!(details["retry_with_same_request_id"], true);
    }
}
