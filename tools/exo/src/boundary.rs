use crate::api::protocol::ErrorCode;
use crate::failure::ExoFailure;
use crate::steering::SuggestedAction;

/// Wrapper type for converting `anyhow::Error` into `Box<dyn std::error::Error>`
/// while preserving embedded `ExoFailure` for `render_fatal_error`.
#[derive(Debug)]
pub struct AnyhowBox(pub anyhow::Error);

impl std::fmt::Display for AnyhowBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for AnyhowBox {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // `anyhow::Error` does not participate in `std::error::Error` chaining in this
        // codebase once erased to `Box<dyn Error>`, so we explicitly surface embedded
        // `ExoFailure`.
        self.0
            .downcast_ref::<ExoFailure>()
            .map(|f| f as &(dyn std::error::Error + 'static))
            .or_else(|| {
                self.0
                    .chain()
                    .nth(1)
                    .map(|e| e as &(dyn std::error::Error + 'static))
            })
    }
}

/// Extension trait for ergonomic conversion of `anyhow::Result<T>` into
/// `Result<T, Box<dyn std::error::Error>>`.
pub trait BoundaryResultExt<T> {
    fn boundary(self) -> Result<T, Box<dyn std::error::Error>>;
}

impl<T> BoundaryResultExt<T> for anyhow::Result<T> {
    fn boundary(self) -> Result<T, Box<dyn std::error::Error>> {
        self.map_err(|e| Box::new(AnyhowBox(e)) as Box<dyn std::error::Error>)
    }
}

pub fn box_anyhow_error<F>(e: anyhow::Error, fallback: F) -> Box<dyn std::error::Error>
where
    F: FnOnce(anyhow::Error) -> ExoFailure,
{
    if let Some(failure) = crate::storage_compatibility::writer_compatibility_failure_from_error(&e)
    {
        return Box::new(failure);
    }
    match e.downcast::<ExoFailure>() {
        Ok(f) => Box::new(f),
        Err(e) => Box::new(fallback(e)),
    }
}

pub fn box_anyhow_internal_with_actions(
    e: anyhow::Error,
    next_actions: Vec<SuggestedAction>,
) -> Box<dyn std::error::Error> {
    box_anyhow_error(e, |e| {
        ExoFailure::new(
            ErrorCode::Internal,
            e.to_string(),
            ExoFailure::orienting_steering(next_actions),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{AnyhowBox, box_anyhow_error};
    use crate::api::protocol::ErrorCode;
    use crate::failure::ExoFailure;
    use crate::steering::SteeringBlock;

    fn causes_include_exo_failure(e: &(dyn std::error::Error + 'static)) -> bool {
        let mut cur: Option<&(dyn std::error::Error + 'static)> = Some(e);
        while let Some(err) = cur {
            if err.downcast_ref::<ExoFailure>().is_some() {
                return true;
            }
            cur = err.source();
        }
        false
    }

    #[test]
    fn anyhow_box_preserves_context_chain_to_exo_failure() {
        let failure = ExoFailure::new(
            ErrorCode::NotFound,
            "boom",
            SteeringBlock {
                primary_intent: crate::steering::WorkIntent::Orient,
                progress_mode: crate::steering::ProgressMode::BetweenPhases,
                situation: "Test situation.".to_string(),
                next_actions: vec![],
                repair_actions: vec![],
                perception_summaries: vec![],
                completion_digests: vec![],
                rfc_context: vec![],
                session_boundary: None,
                entity_context: None,
            },
        );

        let e = anyhow::Error::new(failure).context("outer context");
        let boxed = AnyhowBox(e);

        assert!(causes_include_exo_failure(&boxed));
    }

    #[test]
    fn boxed_cli_boundary_preserves_wrapped_storage_compatibility_failure() {
        let error = crate::storage_compatibility::map_writer_compatibility_error(
            exosuit_storage::WriterCompatibilityError::Busy {
                lock_path: std::path::PathBuf::from("/tmp/exo.writer-compat.lock"),
            },
        )
        .context("CLI command construction");

        let boxed = box_anyhow_error(error, |_| {
            panic!("wrapped storage compatibility must not use the internal fallback")
        });
        let failure = boxed
            .downcast_ref::<ExoFailure>()
            .expect("typed storage failure");
        assert_eq!(
            failure.error.details.as_ref().unwrap()["kind"],
            "storage.compatibility_busy"
        );
        assert_eq!(
            failure.error.details.as_ref().unwrap()["retry_with_same_request_id"],
            true
        );
    }
}
