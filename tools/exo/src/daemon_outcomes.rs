//! Durable daemon request outcomes for transparent mutation recovery.
//!
//! Mutating requests are reserved before command dispatch and their complete
//! response is persisted before the daemon writes to the client socket. A
//! reconnecting client can therefore resend the same request envelope without
//! executing the command a second time. `workbench.launch` is the deliberate
//! exception: its bearer-bearing response stays in daemon memory while SQLite
//! stores only a typed, secret-free completion marker.

use crate::api::protocol::{
    Address, Effect, ErrorBody, ErrorCode, Op, PROTOCOL_VERSION, RecoveryClass, RequestEnvelope,
    ResponseEnvelope, Status,
};
use crate::command::command_spec::CommandSpec;
use crate::command::registry::{build_command_from_invocation, default_registry};
use crate::command::router::Invocation;
use anyhow::{Context, Result, anyhow};
use exosuit_storage::rusqlite::{OpenFlags, TransactionBehavior};
use exosuit_storage::{Connection, OptionalExtension, RequestTransaction, params};
use std::collections::HashSet;
use std::fs::OpenOptions;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const DAEMON_OUTCOME_DB_NAME: &str = "daemon-outcomes.sqlite3";
const COMPLETED_OUTCOME_RETENTION_SECS: i64 = 7 * 24 * 60 * 60;
const WORKBENCH_LAUNCH_COMPLETION_KIND: &str = "workbench.launch.completed";
const WORKBENCH_LAUNCH_COMPLETION_SCHEMA_VERSION: u8 = 1;

#[derive(Debug, Clone)]
pub struct RequestOutcomeLedger {
    path: PathBuf,
    notifications: Arc<OutcomeNotifications>,
}

#[derive(Debug, Default)]
struct OutcomeNotifications {
    generation: Mutex<u64>,
    changed: Condvar,
    #[cfg(test)]
    waiters: AtomicUsize,
}

#[cfg(test)]
struct OutcomeWaiterGuard<'a>(&'a AtomicUsize);

#[cfg(test)]
impl Drop for OutcomeWaiterGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Debug)]
enum PersistedCompletion {
    Response(Box<ResponseEnvelope>),
    WorkbenchLaunchMarker,
}

type LaunchReplayCallback<'a> = dyn Fn(&str) -> Option<ResponseEnvelope> + 'a;

#[derive(Debug)]
enum Reservation {
    Execute,
    Replay(PersistedCompletion),
    InFlight {
        instance_id: String,
        recovery_class: Option<RecoveryClass>,
    },
    Conflict,
}

#[derive(Debug)]
enum WaitForResponse {
    Completed(PersistedCompletion),
    TimedOut,
    ReservationReleased,
}

#[derive(Debug, PartialEq, Eq)]
enum RuntimeOutcomeState {
    Missing,
    InFlight {
        instance_id: String,
        recovery_class: Option<RecoveryClass>,
    },
    Terminal,
}

#[derive(Debug)]
pub struct OutcomeExecution {
    pub response: ResponseEnvelope,
    pub replayed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedRequestRecovery {
    pub effect: Effect,
    pub recovery_class: RecoveryClass,
}

#[derive(Debug)]
struct AtomicCoreExecution {
    response: ResponseEnvelope,
    committed: bool,
    replayed: bool,
    request_id_conflict: bool,
}

#[derive(Debug)]
struct RequestHashes {
    current: String,
    legacy: String,
}

impl RequestHashes {
    fn matches(&self, stored: &str) -> bool {
        stored == self.current || stored == self.legacy
    }
}

impl RequestOutcomeLedger {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let ledger = Self {
            path: path.into(),
            notifications: Arc::new(OutcomeNotifications::default()),
        };
        if let Some(parent) = ledger.path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!(
                    "create daemon outcome ledger directory {}",
                    parent.display()
                )
            })?;
        }
        create_owner_only_file_if_missing(&ledger.path)?;
        let connection = ledger.connection()?;
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS daemon_request_outcomes (
                 request_id TEXT PRIMARY KEY,
                 request_hash TEXT NOT NULL,
                 effect TEXT NOT NULL,
                 instance_id TEXT NOT NULL,
                 recovery_class TEXT,
                 response_json TEXT,
                 started_at INTEGER NOT NULL,
                 completed_at INTEGER
             );
             CREATE INDEX IF NOT EXISTS daemon_request_outcomes_completed_at
                 ON daemon_request_outcomes(completed_at);",
        )?;
        let has_recovery_class: bool = connection.query_row(
            "SELECT EXISTS (
                 SELECT 1 FROM pragma_table_info('daemon_request_outcomes')
                 WHERE name = 'recovery_class'
             )",
            [],
            |row| row.get(0),
        )?;
        if !has_recovery_class {
            connection.execute(
                "ALTER TABLE daemon_request_outcomes ADD COLUMN recovery_class TEXT",
                [],
            )?;
        }
        ledger.sanitize_legacy_workbench_launch_responses(&connection)?;
        ledger.prune_completed(&connection)?;
        ledger.harden_owner_only_files()?;
        Ok(ledger)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Return a completed runtime response or request-ID conflict before
    /// request preparation. Canonical atomic outcomes still pass through the
    /// atomic recovery path so finalization can repopulate this runtime ledger.
    #[cfg(test)]
    pub(crate) fn terminal_outcome_before_preparation(
        &self,
        request: &RequestEnvelope,
    ) -> Result<Option<OutcomeExecution>> {
        self.terminal_outcome_before_preparation_with_launch_replay(request, None)
    }

    pub(crate) fn terminal_outcome_before_preparation_with_launch_replay(
        &self,
        request: &RequestEnvelope,
        replay_launch: Option<&LaunchReplayCallback<'_>>,
    ) -> Result<Option<OutcomeExecution>> {
        let request_hashes = request_hashes(request)?;
        self.harden_owner_only_files()?;
        let runtime_outcome =
            Connection::open_with_flags(&self.path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                .with_context(|| format!("open daemon outcome ledger {}", self.path.display()))
                .and_then(|connection| {
                    connection.pragma_update(None, "busy_timeout", 5_000)?;
                    connection
                        .query_row(
                            "SELECT request_hash, effect, recovery_class, response_json
             FROM daemon_request_outcomes
             WHERE request_id = ?1",
                            [&request.id],
                            |row| {
                                Ok((
                                    row.get::<_, String>(0)?,
                                    row.get::<_, String>(1)?,
                                    row.get::<_, Option<String>>(2)?,
                                    row.get::<_, Option<String>>(3)?,
                                ))
                            },
                        )
                        .optional()
                        .map_err(Into::into)
                });

        if let Ok(Some((stored_hash, effect, recovery_class, response_json))) = &runtime_outcome {
            let effect = effect_from_name(&effect)?;
            if !request_hashes.matches(stored_hash) {
                let response = request_id_conflict_response(request.id.clone(), effect);
                return Ok(Some(OutcomeExecution {
                    response: if matches!(
                        recovery_class.as_deref().and_then(recovery_class_from_name),
                        None | Some(RecoveryClass::AtomicProjectState)
                    ) {
                        without_committed_effect(response)
                    } else {
                        response
                    },
                    replayed: false,
                }));
            }
            if let Some(response_json) = response_json {
                let legacy_launch = is_workbench_launch_request(request)
                    && raw_response_is_workbench_launch(response_json);
                if legacy_launch {
                    self.replace_legacy_workbench_launch_response(
                        &request.id,
                        stored_hash,
                        response_json,
                    )?;
                }
                let completion = if legacy_launch {
                    PersistedCompletion::WorkbenchLaunchMarker
                } else {
                    persisted_completion_from_json(response_json)?
                };
                if is_workbench_launch_request(request) {
                    return Ok(Some(OutcomeExecution {
                        response: replay_workbench_launch_completion(
                            &request.id,
                            effect,
                            completion,
                            replay_launch,
                        ),
                        replayed: true,
                    }));
                }
                let PersistedCompletion::Response(response) = completion else {
                    return Err(anyhow!(
                        "workbench launch completion marker was recorded for a non-launch request"
                    ));
                };
                let response = *response;
                if request.auth.as_ref().is_some_and(|auth| auth.confirm)
                    && is_transient_execution_confirmation(&response)
                {
                    self.connection()?.execute(
                        "DELETE FROM daemon_request_outcomes
                         WHERE request_id = ?1
                           AND request_hash = ?2
                           AND response_json = ?3",
                        params![request.id, stored_hash, response_json],
                    )?;
                    return Ok(None);
                }
                return Ok(Some(OutcomeExecution {
                    response,
                    replayed: true,
                }));
            }
        }

        runtime_outcome?;
        Ok(None)
    }

    /// Return the recorded recovery authority for a matching in-flight request.
    /// This preserves at-most-once handling when current command construction
    /// depends on a workspace path or argument file that is no longer present.
    pub(crate) fn reserved_request_recovery_before_preparation(
        &self,
        request: &RequestEnvelope,
    ) -> Result<Option<ResolvedRequestRecovery>> {
        let request_hashes = request_hashes(request)?;
        let connection = Connection::open_with_flags(&self.path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .with_context(|| format!("open daemon outcome ledger {}", self.path.display()))?;
        connection.pragma_update(None, "busy_timeout", 5_000)?;
        let reserved = connection
            .query_row(
                "SELECT request_hash, effect, recovery_class, response_json
                 FROM daemon_request_outcomes
                 WHERE request_id = ?1",
                [&request.id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()?;

        let Some((stored_hash, effect, recovery_class, None)) = reserved else {
            return Ok(None);
        };
        if !request_hashes.matches(&stored_hash) {
            return Ok(None);
        }

        Ok(Some(ResolvedRequestRecovery {
            effect: effect_from_name(&effect)?,
            recovery_class: recovery_class
                .as_deref()
                .and_then(recovery_class_from_name)
                .unwrap_or(RecoveryClass::ExternalAtMostOnce),
        }))
    }

    /// Return whether an atomic request may execute and therefore needs current
    /// project preparation. Completed, conflicting, and same-instance in-flight
    /// requests are resolved by the outcome ledger before mutable preparation.
    #[cfg(test)]
    pub(crate) fn atomic_request_needs_preparation(
        &self,
        request: &RequestEnvelope,
        project_db_path: &Path,
        instance_id: &str,
    ) -> Result<bool> {
        self.atomic_request_needs_preparation_after_compatibility_preflight(
            request,
            project_db_path,
            instance_id,
            || Ok(()),
        )
    }

    pub(crate) fn atomic_request_needs_preparation_after_compatibility_preflight(
        &self,
        request: &RequestEnvelope,
        project_db_path: &Path,
        instance_id: &str,
        preflight: impl FnOnce() -> Result<()>,
    ) -> Result<bool> {
        let request_hashes = request_hashes(request)?;
        let runtime_outcome = self.runtime_outcome_state(&request.id, &request_hashes);
        if matches!(runtime_outcome, Ok(RuntimeOutcomeState::Terminal))
            || matches!(
                &runtime_outcome,
                Ok(RuntimeOutcomeState::InFlight {
                    recovery_class: Some(recovery_class),
                    ..
                }) if *recovery_class != RecoveryClass::AtomicProjectState
            )
            || matches!(
                &runtime_outcome,
                Ok(RuntimeOutcomeState::InFlight {
                    recovery_class: None,
                    ..
                })
            )
        {
            return Ok(false);
        }
        preflight()?;
        if matches!(
            &runtime_outcome,
            Ok(RuntimeOutcomeState::InFlight {
                instance_id: owner,
                ..
            }) if owner == instance_id
        ) {
            return Ok(false);
        }
        let canonical_outcome = canonical_atomic_outcome_exists(project_db_path, &request.id);
        match (runtime_outcome, canonical_outcome) {
            (_, Ok(true)) => Ok(false),
            (
                Ok(RuntimeOutcomeState::Missing | RuntimeOutcomeState::InFlight { .. }),
                Ok(false),
            ) => Ok(true),
            (Err(_), Ok(false)) => Ok(true),
            (_, Err(error)) => Err(error),
            (Ok(RuntimeOutcomeState::Terminal), _) => Ok(false),
        }
    }

    pub fn execute<F>(
        &self,
        request: RequestEnvelope,
        effect: Effect,
        instance_id: &str,
        in_flight_wait: Duration,
        execute: F,
    ) -> OutcomeExecution
    where
        F: FnOnce(RequestEnvelope) -> ResponseEnvelope,
    {
        let request_id = request.id.clone();
        let request_hashes = match request_hashes(&request) {
            Ok(hashes) => hashes,
            Err(error) => {
                return OutcomeExecution {
                    response: ledger_error_response(
                        request_id,
                        effect,
                        "daemon.request_outcome_fingerprint_failed",
                        error,
                        false,
                    ),
                    replayed: false,
                };
            }
        };
        let request_hash = request_hashes.current.clone();

        let mut request = Some(request);
        let mut execute = Some(execute);
        loop {
            match self.reserve_compatible(
                &request_id,
                &request_hashes,
                effect,
                RecoveryClass::ExternalAtMostOnce,
                instance_id,
            ) {
                Ok(Reservation::Replay(completion)) => {
                    return OutcomeExecution {
                        response: generic_replay_response(&request_id, effect, completion),
                        replayed: true,
                    };
                }
                Ok(Reservation::Conflict) => {
                    return OutcomeExecution {
                        response: request_id_conflict_response(request_id, effect),
                        replayed: false,
                    };
                }
                Ok(Reservation::InFlight {
                    instance_id: owner, ..
                }) if owner == instance_id => {
                    match self.wait_for_response(&request_id, &request_hash, in_flight_wait) {
                        Ok(WaitForResponse::Completed(completion)) => {
                            return OutcomeExecution {
                                response: generic_replay_response(&request_id, effect, completion),
                                replayed: true,
                            };
                        }
                        Ok(WaitForResponse::TimedOut) => {
                            return OutcomeExecution {
                                response: in_flight_response(request_id, effect, &owner, false),
                                replayed: false,
                            };
                        }
                        Ok(WaitForResponse::ReservationReleased) => {}
                        Err(error) => {
                            return OutcomeExecution {
                                response: ledger_error_response(
                                    request_id,
                                    effect,
                                    "daemon.request_outcome_lookup_failed",
                                    error,
                                    false,
                                ),
                                replayed: false,
                            };
                        }
                    }
                }
                Ok(Reservation::InFlight {
                    instance_id: owner, ..
                }) => {
                    return OutcomeExecution {
                        response: in_flight_response(request_id, effect, &owner, true),
                        replayed: false,
                    };
                }
                Ok(Reservation::Execute) => {
                    let (Some(execute), Some(request)) = (execute.take(), request.take()) else {
                        return OutcomeExecution {
                            response: ledger_error_response(
                                request_id,
                                effect,
                                "daemon.request_outcome_execution_state_invalid",
                                anyhow!("external request execution state was already consumed"),
                                false,
                            ),
                            replayed: false,
                        };
                    };
                    let response = execute(request);
                    if is_retryable_daemon_busy_response(&response) {
                        return match self.abandon(&request_id, &request_hash, instance_id) {
                            Ok(()) => OutcomeExecution {
                                response: normalize_retryable_daemon_busy_response(response),
                                replayed: false,
                            },
                            Err(error) => OutcomeExecution {
                                response: without_committed_effect(ledger_error_response(
                                    request_id,
                                    effect,
                                    "daemon.request_outcome_abandon_failed",
                                    error,
                                    false,
                                )),
                                replayed: false,
                            },
                        };
                    }
                    if response.status == Status::ConfirmRequired
                        || is_rejected_execution_confirmation(&response)
                    {
                        // Authorization changes on the approved replay, so the
                        // pre-execution response cannot become a terminal outcome.
                        return match self.abandon(&request_id, &request_hash, instance_id) {
                            Ok(()) => OutcomeExecution {
                                response,
                                replayed: false,
                            },
                            Err(error) => OutcomeExecution {
                                response: without_committed_effect(ledger_error_response(
                                    request_id,
                                    effect,
                                    "daemon.request_outcome_abandon_failed",
                                    error,
                                    false,
                                )),
                                replayed: false,
                            },
                        };
                    }
                    return match self.complete(&request_id, &request_hash, &response) {
                        Ok(()) => OutcomeExecution {
                            response,
                            replayed: false,
                        },
                        Err(error) => OutcomeExecution {
                            response: ledger_error_response(
                                request_id,
                                effect,
                                "daemon.request_outcome_persist_failed",
                                error,
                                true,
                            ),
                            replayed: false,
                        },
                    };
                }
                Err(error) => {
                    return OutcomeExecution {
                        response: ledger_error_response(
                            request_id,
                            effect,
                            "daemon.request_outcome_reservation_failed",
                            error,
                            false,
                        ),
                        replayed: false,
                    };
                }
            }
        }
    }

    /// Execute `workbench.launch` without persisting its bearer-bearing response.
    ///
    /// The durable row records only a typed completion marker. Exact retry
    /// successful responses live in daemon memory and are returned through
    /// `replay_launch` only while the original enrollment authority remains
    /// current. Terminal launch errors also become markers: their first
    /// diagnostic is returned once, and a retry must use a new request ID.
    #[allow(clippy::too_many_arguments)]
    pub fn execute_workbench_launch<F, Retain, Replay, Discard>(
        &self,
        request: RequestEnvelope,
        effect: Effect,
        instance_id: &str,
        in_flight_wait: Duration,
        execute: F,
        retain_launch: Retain,
        replay_launch: Replay,
        discard_launch: Discard,
    ) -> OutcomeExecution
    where
        F: FnOnce(RequestEnvelope) -> ResponseEnvelope,
        Retain: Fn(&str, &ResponseEnvelope) -> Result<()>,
        Replay: Fn(&str) -> Option<ResponseEnvelope>,
        Discard: Fn(&str),
    {
        let request_id = request.id.clone();
        let request_hashes = match request_hashes(&request) {
            Ok(hashes) => hashes,
            Err(error) => {
                return OutcomeExecution {
                    response: ledger_error_response(
                        request_id,
                        effect,
                        "daemon.request_outcome_fingerprint_failed",
                        error,
                        false,
                    ),
                    replayed: false,
                };
            }
        };
        let request_hash = request_hashes.current.clone();

        let mut request = Some(request);
        let mut execute = Some(execute);
        loop {
            match self.reserve_compatible(
                &request_id,
                &request_hashes,
                effect,
                RecoveryClass::ExternalAtMostOnce,
                instance_id,
            ) {
                Ok(Reservation::Replay(completion)) => {
                    return OutcomeExecution {
                        response: replay_workbench_launch_completion(
                            &request_id,
                            effect,
                            completion,
                            Some(&replay_launch),
                        ),
                        replayed: true,
                    };
                }
                Ok(Reservation::Conflict) => {
                    return OutcomeExecution {
                        response: request_id_conflict_response(request_id, effect),
                        replayed: false,
                    };
                }
                Ok(Reservation::InFlight {
                    instance_id: owner, ..
                }) if owner == instance_id => {
                    match self.wait_for_response(&request_id, &request_hash, in_flight_wait) {
                        Ok(WaitForResponse::Completed(completion)) => {
                            return OutcomeExecution {
                                response: replay_workbench_launch_completion(
                                    &request_id,
                                    effect,
                                    completion,
                                    Some(&replay_launch),
                                ),
                                replayed: true,
                            };
                        }
                        Ok(WaitForResponse::TimedOut) => {
                            return OutcomeExecution {
                                response: in_flight_response(request_id, effect, &owner, false),
                                replayed: false,
                            };
                        }
                        Ok(WaitForResponse::ReservationReleased) => {}
                        Err(error) => {
                            return OutcomeExecution {
                                response: ledger_error_response(
                                    request_id,
                                    effect,
                                    "daemon.request_outcome_lookup_failed",
                                    error,
                                    false,
                                ),
                                replayed: false,
                            };
                        }
                    }
                }
                Ok(Reservation::InFlight { .. }) => {
                    return OutcomeExecution {
                        response: workbench_launch_replay_unavailable_response(request_id, effect),
                        replayed: false,
                    };
                }
                Ok(Reservation::Execute) => {
                    let (Some(execute), Some(request)) = (execute.take(), request.take()) else {
                        return OutcomeExecution {
                            response: ledger_error_response(
                                request_id,
                                effect,
                                "daemon.request_outcome_execution_state_invalid",
                                anyhow!("workbench launch execution state was already consumed"),
                                false,
                            ),
                            replayed: false,
                        };
                    };
                    let response = execute(request);
                    if is_retryable_daemon_busy_response(&response) {
                        return match self.abandon(&request_id, &request_hash, instance_id) {
                            Ok(()) => OutcomeExecution {
                                response: normalize_retryable_daemon_busy_response(response),
                                replayed: false,
                            },
                            Err(error) => OutcomeExecution {
                                response: without_committed_effect(ledger_error_response(
                                    request_id,
                                    effect,
                                    "daemon.request_outcome_abandon_failed",
                                    error,
                                    false,
                                )),
                                replayed: false,
                            },
                        };
                    }
                    if response.status == Status::ConfirmRequired
                        || is_rejected_execution_confirmation(&response)
                    {
                        return match self.abandon(&request_id, &request_hash, instance_id) {
                            Ok(()) => OutcomeExecution {
                                response,
                                replayed: false,
                            },
                            Err(error) => OutcomeExecution {
                                response: without_committed_effect(ledger_error_response(
                                    request_id,
                                    effect,
                                    "daemon.request_outcome_abandon_failed",
                                    error,
                                    false,
                                )),
                                replayed: false,
                            },
                        };
                    }

                    let retained = retain_launch(&request_id, &response);
                    let completed =
                        self.complete_workbench_launch_marker(&request_id, &request_hash);
                    if let Err(error) = completed {
                        discard_launch(&request_id);
                        return OutcomeExecution {
                            response: ledger_error_response(
                                request_id,
                                effect,
                                "daemon.request_outcome_persist_failed",
                                error,
                                true,
                            ),
                            replayed: false,
                        };
                    }
                    if let Err(error) = retained {
                        return OutcomeExecution {
                            response: ledger_error_response(
                                request_id,
                                effect,
                                "workbench.launch_replay_cache_failed",
                                error,
                                true,
                            ),
                            replayed: false,
                        };
                    }
                    return OutcomeExecution {
                        response,
                        replayed: false,
                    };
                }
                Err(error) => {
                    return OutcomeExecution {
                        response: ledger_error_response(
                            request_id,
                            effect,
                            "daemon.request_outcome_reservation_failed",
                            error,
                            false,
                        ),
                        replayed: false,
                    };
                }
            }
        }
    }

    /// Execute a canonical project-state request with state and core response
    /// committed in one SQLite transaction.
    ///
    /// A reservation owned by a previous daemon instance is recoverable for
    /// this class: V021 either contains the committed response or proves that
    /// the interrupted transaction did not commit.
    #[allow(clippy::too_many_arguments)]
    pub fn execute_atomic_project_state<F, G>(
        &self,
        request: RequestEnvelope,
        effect: Effect,
        instance_id: &str,
        in_flight_wait: Duration,
        project_db_path: &Path,
        execute: F,
        finalize: G,
    ) -> OutcomeExecution
    where
        F: FnOnce(RequestEnvelope) -> ResponseEnvelope,
        G: FnOnce(ResponseEnvelope) -> Result<ResponseEnvelope, ResponseEnvelope>,
    {
        self.execute_atomic_project_state_after_compatibility_preflight(
            request,
            effect,
            instance_id,
            in_flight_wait,
            project_db_path,
            || Ok(()),
            execute,
            finalize,
        )
    }

    /// Execute an atomic request while fencing the canonical lookup used when
    /// a same-instance reservation times out. The preflight runs immediately
    /// before that lookup so a changed projection cannot bypass the writer
    /// compatibility contract during recovery.
    #[allow(clippy::too_many_arguments)]
    pub fn execute_atomic_project_state_after_compatibility_preflight<F, G, P>(
        &self,
        request: RequestEnvelope,
        effect: Effect,
        instance_id: &str,
        in_flight_wait: Duration,
        project_db_path: &Path,
        timeout_preflight: P,
        execute: F,
        finalize: G,
    ) -> OutcomeExecution
    where
        F: FnOnce(RequestEnvelope) -> ResponseEnvelope,
        G: FnOnce(ResponseEnvelope) -> Result<ResponseEnvelope, ResponseEnvelope>,
        P: FnOnce() -> Result<()>,
    {
        let request_id = request.id.clone();
        let request_hashes = match request_hashes(&request) {
            Ok(hashes) => hashes,
            Err(error) => {
                return OutcomeExecution {
                    response: without_committed_effect(ledger_error_response(
                        request_id,
                        effect,
                        "daemon.request_outcome_fingerprint_failed",
                        error,
                        false,
                    )),
                    replayed: false,
                };
            }
        };
        let request_hash = request_hashes.current.clone();

        let owns_runtime_reservation = match self.reserve_compatible(
            &request_id,
            &request_hashes,
            effect,
            RecoveryClass::AtomicProjectState,
            instance_id,
        ) {
            Ok(Reservation::Replay(completion)) => {
                return OutcomeExecution {
                    response: generic_replay_response(&request_id, effect, completion),
                    replayed: true,
                };
            }
            Ok(Reservation::Conflict) => {
                return OutcomeExecution {
                    response: without_committed_effect(request_id_conflict_response(
                        request_id, effect,
                    )),
                    replayed: false,
                };
            }
            Ok(Reservation::InFlight {
                instance_id: owner, ..
            }) if owner == instance_id => {
                match self.wait_for_response(&request_id, &request_hash, in_flight_wait) {
                    Ok(WaitForResponse::Completed(completion)) => {
                        return OutcomeExecution {
                            response: generic_replay_response(&request_id, effect, completion),
                            replayed: true,
                        };
                    }
                    Ok(WaitForResponse::TimedOut) => {
                        if let Err(error) = timeout_preflight() {
                            return OutcomeExecution {
                                response: without_committed_effect(ledger_error_response(
                                    request_id,
                                    effect,
                                    "daemon.request_outcome_lookup_failed",
                                    error,
                                    false,
                                )),
                                replayed: false,
                            };
                        }
                        match canonical_atomic_outcome_exists(project_db_path, &request_id) {
                            Ok(true) => false,
                            Ok(false) => {
                                return OutcomeExecution {
                                    response: in_flight_response(request_id, effect, &owner, false),
                                    replayed: false,
                                };
                            }
                            Err(error) => {
                                return OutcomeExecution {
                                    response: without_committed_effect(ledger_error_response(
                                        request_id,
                                        effect,
                                        "daemon.request_outcome_lookup_failed",
                                        error,
                                        false,
                                    )),
                                    replayed: false,
                                };
                            }
                        }
                    }
                    Ok(WaitForResponse::ReservationReleased) => {
                        return OutcomeExecution {
                            response: without_committed_effect(ledger_error_response(
                                request_id,
                                effect,
                                "daemon.request_outcome_lookup_failed",
                                anyhow!("daemon outcome reservation disappeared"),
                                false,
                            )),
                            replayed: false,
                        };
                    }
                    Err(error) => {
                        return OutcomeExecution {
                            response: without_committed_effect(ledger_error_response(
                                request_id,
                                effect,
                                "daemon.request_outcome_lookup_failed",
                                error,
                                false,
                            )),
                            replayed: false,
                        };
                    }
                }
            }
            Ok(Reservation::Execute) => true,
            Ok(Reservation::InFlight {
                recovery_class: Some(RecoveryClass::AtomicProjectState),
                ..
            }) => false,
            Ok(Reservation::InFlight {
                instance_id: owner, ..
            }) => {
                return OutcomeExecution {
                    response: in_flight_response(request_id, effect, &owner, true),
                    replayed: false,
                };
            }
            // V021 remains sufficient when the runtime-only ledger is
            // temporarily unavailable. Completion below is best-effort.
            Err(_) => false,
        };

        let atomic = match execute_atomic_core(
            project_db_path,
            &request_hash,
            effect,
            request,
            execute,
            || Ok(()),
        ) {
            Ok(execution) => execution,
            Err(error) => {
                if owns_runtime_reservation {
                    let _ = self.abandon(&request_id, &request_hash, instance_id);
                }
                return OutcomeExecution {
                    response: without_committed_effect(ledger_error_response(
                        request_id,
                        effect,
                        "daemon.atomic_request_commit_failed",
                        error,
                        false,
                    )),
                    replayed: false,
                };
            }
        };

        if atomic.request_id_conflict {
            if owns_runtime_reservation {
                let _ = self.abandon(&request_id, &request_hash, instance_id);
            }
            return OutcomeExecution {
                response: atomic.response,
                replayed: false,
            };
        }

        let response = if atomic.committed {
            match finalize(atomic.response) {
                Ok(response) => response,
                Err(response) => {
                    // Finalization is idempotent. Removing the runtime-only
                    // reservation lets the same request replay the canonical
                    // core response and retry projection/checkpoint work.
                    if owns_runtime_reservation {
                        let _ = self.abandon(&request_id, &request_hash, instance_id);
                    }
                    return OutcomeExecution {
                        response,
                        replayed: atomic.replayed,
                    };
                }
            }
        } else {
            atomic.response
        };

        if self
            .complete(&request_id, &request_hash, &response)
            .is_err()
            && owns_runtime_reservation
        {
            let _ = self.abandon(&request_id, &request_hash, instance_id);
        }
        let _ = self.prune_canonical_outcomes(project_db_path);
        OutcomeExecution {
            response,
            replayed: atomic.replayed,
        }
    }

    fn connection(&self) -> Result<Connection> {
        create_owner_only_file_if_missing(&self.path)?;
        let connection = Connection::open_with_flags(&self.path, OpenFlags::SQLITE_OPEN_READ_WRITE)
            .with_context(|| format!("open daemon outcome ledger {}", self.path.display()))?;
        connection.pragma_update(None, "journal_mode", "wal")?;
        connection.pragma_update(None, "synchronous", "full")?;
        connection.pragma_update(None, "busy_timeout", 5_000)?;
        connection.pragma_update(None, "secure_delete", "on")?;
        self.harden_owner_only_files()?;
        Ok(connection)
    }

    fn harden_owner_only_files(&self) -> Result<()> {
        for path in outcome_file_paths(&self.path) {
            if path.exists() {
                set_owner_only_file_permissions(&path)?;
            }
        }
        Ok(())
    }

    fn notify_waiters(&self) {
        let Ok(mut generation) = self.notifications.generation.lock() else {
            return;
        };
        *generation = generation.wrapping_add(1);
        self.notifications.changed.notify_all();
    }

    fn sanitize_legacy_workbench_launch_responses(&self, connection: &Connection) -> Result<()> {
        connection.pragma_update(None, "secure_delete", "on")?;
        let rows = {
            let mut statement = connection.prepare(
                "SELECT request_id, response_json
                 FROM daemon_request_outcomes
                 WHERE response_json IS NOT NULL",
            )?;
            statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        let legacy_request_ids = rows
            .into_iter()
            .filter_map(|(request_id, response_json)| {
                raw_response_is_workbench_launch(&response_json).then_some(request_id)
            })
            .collect::<Vec<_>>();
        if legacy_request_ids.is_empty() {
            return Ok(());
        }
        let marker = workbench_launch_completion_marker_json();
        for request_id in legacy_request_ids {
            connection.execute(
                "UPDATE daemon_request_outcomes SET response_json = ?2 WHERE request_id = ?1",
                params![request_id, marker],
            )?;
        }
        connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;")?;
        self.harden_owner_only_files()?;
        Ok(())
    }

    fn replace_legacy_workbench_launch_response(
        &self,
        request_id: &str,
        request_hash: &str,
        response_json: &str,
    ) -> Result<()> {
        let marker = workbench_launch_completion_marker_json();
        let connection = self.connection()?;
        connection.execute(
            "UPDATE daemon_request_outcomes
             SET response_json = ?4
             WHERE request_id = ?1 AND request_hash = ?2 AND response_json = ?3",
            params![request_id, request_hash, response_json, marker],
        )?;
        connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
        self.harden_owner_only_files()?;
        Ok(())
    }

    fn runtime_outcome_state(
        &self,
        request_id: &str,
        request_hashes: &RequestHashes,
    ) -> Result<RuntimeOutcomeState> {
        let connection = self.connection()?;
        let existing = connection
            .query_row(
                "SELECT request_hash, instance_id, recovery_class, response_json
                 FROM daemon_request_outcomes
                 WHERE request_id = ?1",
                [request_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()?;
        Ok(match existing {
            None => RuntimeOutcomeState::Missing,
            Some((stored_hash, _, _, _)) if !request_hashes.matches(&stored_hash) => {
                RuntimeOutcomeState::Terminal
            }
            Some((_, _, _, Some(_))) => RuntimeOutcomeState::Terminal,
            Some((_, instance_id, recovery_class, None)) => RuntimeOutcomeState::InFlight {
                instance_id,
                recovery_class: recovery_class.as_deref().and_then(recovery_class_from_name),
            },
        })
    }

    #[cfg(test)]
    fn reserve(
        &self,
        request_id: &str,
        request_hash: &str,
        effect: Effect,
        recovery_class: RecoveryClass,
        instance_id: &str,
    ) -> Result<Reservation> {
        self.reserve_compatible(
            request_id,
            &RequestHashes {
                current: request_hash.to_string(),
                legacy: request_hash.to_string(),
            },
            effect,
            recovery_class,
            instance_id,
        )
    }

    #[cfg(test)]
    pub(crate) fn reserve_atomic_request_for_test(
        &self,
        request: &RequestEnvelope,
        effect: Effect,
        instance_id: &str,
    ) -> Result<()> {
        let request_hashes = request_hashes(request)?;
        match self.reserve_compatible(
            &request.id,
            &request_hashes,
            effect,
            RecoveryClass::AtomicProjectState,
            instance_id,
        )? {
            Reservation::Execute => Ok(()),
            reservation => Err(anyhow!(
                "expected a new atomic reservation, got {reservation:?}"
            )),
        }
    }

    fn reserve_compatible(
        &self,
        request_id: &str,
        request_hashes: &RequestHashes,
        effect: Effect,
        recovery_class: RecoveryClass,
        instance_id: &str,
    ) -> Result<Reservation> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing = transaction
            .query_row(
                "SELECT request_hash, instance_id, recovery_class, response_json
                 FROM daemon_request_outcomes
                 WHERE request_id = ?1",
                [request_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()?;

        if let Some((stored_hash, _, _, _)) = &existing
            && request_hashes.matches(stored_hash)
            && stored_hash != &request_hashes.current
        {
            transaction.execute(
                "UPDATE daemon_request_outcomes
                 SET request_hash = ?2
                 WHERE request_id = ?1 AND request_hash = ?3",
                params![request_id, request_hashes.current, stored_hash],
            )?;
        }

        let reservation = match existing {
            Some((stored_hash, _, _, _)) if !request_hashes.matches(&stored_hash) => {
                Reservation::Conflict
            }
            Some((_, _, _, Some(response_json))) => {
                Reservation::Replay(persisted_completion_from_json(&response_json)?)
            }
            Some((_, owner_instance_id, stored_recovery_class, None)) => Reservation::InFlight {
                instance_id: owner_instance_id,
                recovery_class: stored_recovery_class
                    .as_deref()
                    .and_then(recovery_class_from_name),
            },
            None => {
                transaction.execute(
                    "INSERT INTO daemon_request_outcomes (
                         request_id, request_hash, effect, instance_id, recovery_class, started_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        request_id,
                        request_hashes.current,
                        effect_name(effect),
                        instance_id,
                        recovery_class_name(recovery_class),
                        now_timestamp(),
                    ],
                )?;
                Reservation::Execute
            }
        };
        transaction.commit()?;
        Ok(reservation)
    }

    fn complete(
        &self,
        request_id: &str,
        request_hash: &str,
        response: &ResponseEnvelope,
    ) -> Result<()> {
        let response_json = serde_json::to_string(response)?;
        let connection = self.connection()?;
        let updated = connection.execute(
            "UPDATE daemon_request_outcomes
             SET response_json = ?3, completed_at = ?4
             WHERE request_id = ?1 AND request_hash = ?2 AND response_json IS NULL",
            params![request_id, request_hash, response_json, now_timestamp()],
        )?;
        if updated != 1 {
            return Err(anyhow!(
                "daemon outcome reservation disappeared before completion"
            ));
        }
        self.notify_waiters();
        // The completed response is already durable. Retention cleanup is
        // best-effort maintenance and must not change the request outcome.
        let _ = self.prune_completed(&connection);
        Ok(())
    }

    fn complete_workbench_launch_marker(&self, request_id: &str, request_hash: &str) -> Result<()> {
        let marker = workbench_launch_completion_marker_json();
        let connection = self.connection()?;
        let updated = connection.execute(
            "UPDATE daemon_request_outcomes
             SET response_json = ?3, completed_at = ?4
             WHERE request_id = ?1 AND request_hash = ?2 AND response_json IS NULL",
            params![request_id, request_hash, marker, now_timestamp()],
        )?;
        if updated != 1 {
            return Err(anyhow!(
                "daemon outcome reservation disappeared before workbench launch completion"
            ));
        }
        self.notify_waiters();
        // Marker durability is the safety boundary. Retention cleanup is
        // maintenance and must not make the owner discard the matching live
        // response after waiters can already observe the committed marker.
        let _ = self.prune_completed(&connection);
        Ok(())
    }

    fn wait_for_response(
        &self,
        request_id: &str,
        request_hash: &str,
        timeout: Duration,
    ) -> Result<WaitForResponse> {
        #[cfg(test)]
        self.notifications.waiters.fetch_add(1, Ordering::AcqRel);
        #[cfg(test)]
        let _waiter = OutcomeWaiterGuard(&self.notifications.waiters);
        let deadline = Instant::now() + timeout;
        loop {
            let observed_generation = *self
                .notifications
                .generation
                .lock()
                .map_err(|_| anyhow!("daemon outcome waiter notification is unavailable"))?;
            let connection = self.connection()?;
            let row = connection
                .query_row(
                    "SELECT request_hash, response_json
                     FROM daemon_request_outcomes
                     WHERE request_id = ?1",
                    [request_id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
                )
                .optional()?;
            drop(connection);
            match row {
                Some((stored_hash, _)) if stored_hash != request_hash => {
                    return Err(anyhow!("request id was reused with a different payload"));
                }
                Some((_, Some(response_json))) => {
                    return Ok(WaitForResponse::Completed(persisted_completion_from_json(
                        &response_json,
                    )?));
                }
                None => return Ok(WaitForResponse::ReservationReleased),
                Some((_, None)) if Instant::now() < deadline => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    let generation = self.notifications.generation.lock().map_err(|_| {
                        anyhow!("daemon outcome waiter notification is unavailable")
                    })?;
                    if *generation == observed_generation {
                        drop(
                            self.notifications
                                .changed
                                .wait_timeout(generation, remaining)
                                .map_err(|_| {
                                    anyhow!("daemon outcome waiter notification is unavailable")
                                })?,
                        );
                    } else {
                        drop(generation);
                    }
                }
                Some((_, None)) => return Ok(WaitForResponse::TimedOut),
            }
        }
    }

    fn abandon(&self, request_id: &str, request_hash: &str, instance_id: &str) -> Result<()> {
        let connection = self.connection()?;
        let deleted = connection.execute(
            "DELETE FROM daemon_request_outcomes
             WHERE request_id = ?1 AND request_hash = ?2 AND instance_id = ?3
               AND response_json IS NULL",
            params![request_id, request_hash, instance_id],
        )?;
        if deleted != 1 {
            return Err(anyhow!(
                "daemon outcome reservation disappeared before abandonment"
            ));
        }
        self.notify_waiters();
        Ok(())
    }

    fn prune_completed(&self, connection: &Connection) -> Result<()> {
        let cutoff = now_timestamp() - COMPLETED_OUTCOME_RETENTION_SECS;
        let launch_marker = workbench_launch_completion_marker_json();
        connection.execute(
            "DELETE FROM daemon_request_outcomes
             WHERE completed_at IS NOT NULL AND completed_at < ?1
               AND response_json != ?2",
            params![cutoff, launch_marker],
        )?;
        Ok(())
    }

    fn prune_canonical_outcomes(&self, project_db_path: &Path) -> Result<()> {
        if !project_db_path.exists() {
            return Ok(());
        }

        // Hold the runtime write lock while choosing canonical rows. A retry
        // cannot create a new unresolved reservation between this snapshot and
        // the canonical deletion. Maintenance is best-effort, so contention
        // skips this pass instead of delaying the request response.
        let mut runtime_connection = self.connection()?;
        runtime_connection.pragma_update(None, "busy_timeout", 0)?;
        let runtime_transaction =
            runtime_connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let unresolved_request_ids: HashSet<String> = {
            let mut statement = runtime_transaction.prepare(
                "SELECT request_id FROM daemon_request_outcomes
                 WHERE response_json IS NULL",
            )?;
            statement
                .query_map([], |row| row.get(0))?
                .collect::<std::result::Result<HashSet<_>, _>>()?
        };

        let mut project_connection = exosuit_storage::open_fenced_connection(project_db_path)
            .map_err(crate::storage_compatibility::map_database_error)
            .with_context(|| format!("open project database {}", project_db_path.display()))?;
        project_connection.pragma_update(None, "busy_timeout", 0)?;
        let project_transaction =
            project_connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let table_exists: bool = project_transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'atomic_request_outcomes'
             )",
            [],
            |row| row.get(0),
        )?;
        if table_exists {
            let cutoff = now_timestamp() - COMPLETED_OUTCOME_RETENTION_SECS;
            let expired_request_ids: Vec<String> = {
                let mut statement = project_transaction.prepare(
                    "SELECT request_id FROM atomic_request_outcomes
                     WHERE committed_at < ?1",
                )?;
                statement
                    .query_map([cutoff], |row| row.get(0))?
                    .collect::<std::result::Result<Vec<_>, _>>()?
            };
            for request_id in expired_request_ids {
                if !unresolved_request_ids.contains(&request_id) {
                    project_transaction.execute(
                        "DELETE FROM atomic_request_outcomes WHERE request_id = ?1",
                        [&request_id],
                    )?;
                }
            }
        }
        project_transaction.commit()?;
        runtime_transaction.commit()?;
        Ok(())
    }
}

fn canonical_atomic_outcome_exists(project_db_path: &Path, request_id: &str) -> Result<bool> {
    if !project_db_path.exists() {
        return Ok(false);
    }
    let connection = exosuit_storage::open_fenced_connection(project_db_path)
        .map_err(crate::storage_compatibility::map_database_error)
        .with_context(|| {
            format!(
                "open canonical atomic outcome database {}",
                project_db_path.display()
            )
        })?;
    let table_exists: bool = connection.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM sqlite_master
             WHERE type = 'table' AND name = 'atomic_request_outcomes'
         )",
        [],
        |row| row.get(0),
    )?;
    if !table_exists {
        return Ok(false);
    }
    Ok(connection
        .query_row(
            "SELECT 1 FROM atomic_request_outcomes WHERE request_id = ?1",
            [request_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

pub fn resolved_request_recovery(
    workspace_root: &Path,
    request: &RequestEnvelope,
) -> Option<ResolvedRequestRecovery> {
    let Op::Call(params) = &request.op else {
        return None;
    };
    let (namespace, operation) = request_command_path(request)?;
    static COMMAND_SPEC: OnceLock<CommandSpec> = OnceLock::new();
    let spec = COMMAND_SPEC.get_or_init(|| CommandSpec::from_registry(&default_registry()));
    let invocation = Invocation::from_json(&params.input, &namespace, &operation, spec).ok()?;
    build_command_from_invocation(&invocation, workspace_root)
        .ok()?
        .map(|command| ResolvedRequestRecovery {
            effect: command.effect(),
            recovery_class: command.recovery_class(),
        })
}

pub fn request_declared_recovery(request: &RequestEnvelope) -> Option<ResolvedRequestRecovery> {
    let Some((namespace, operation)) = request_command_path(request) else {
        return None;
    };
    static COMMAND_SPEC: OnceLock<CommandSpec> = OnceLock::new();
    COMMAND_SPEC
        .get_or_init(|| CommandSpec::from_registry(&default_registry()))
        .operation(&namespace, &operation)
        .map(|operation| ResolvedRequestRecovery {
            effect: operation.effect,
            recovery_class: operation.recovery_class,
        })
}

pub fn request_command_path(request: &RequestEnvelope) -> Option<(String, String)> {
    let Op::Call(params) = &request.op else {
        return None;
    };
    let Address::Operation { path } = &params.address else {
        return None;
    };
    match path.as_slice() {
        [operation] => Some((String::new(), operation.clone())),
        [namespace, operation] => Some((namespace.clone(), operation.clone())),
        [namespace, first, second] => Some((namespace.clone(), format!("{first}.{second}"))),
        _ => None,
    }
}

pub(crate) fn is_workbench_launch_request(request: &RequestEnvelope) -> bool {
    matches!(
        request_command_path(request),
        Some((namespace, operation))
            if namespace == "workbench" && operation == "launch"
    )
}

fn workbench_launch_completion_marker_json() -> String {
    serde_json::json!({
        "kind": WORKBENCH_LAUNCH_COMPLETION_KIND,
        "schema_version": WORKBENCH_LAUNCH_COMPLETION_SCHEMA_VERSION,
    })
    .to_string()
}

fn persisted_completion_from_json(json: &str) -> Result<PersistedCompletion> {
    let value: serde_json::Value =
        serde_json::from_str(json).context("deserialize recorded daemon completion")?;
    if value.get("kind").and_then(serde_json::Value::as_str)
        == Some(WORKBENCH_LAUNCH_COMPLETION_KIND)
        && value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            == Some(u64::from(WORKBENCH_LAUNCH_COMPLETION_SCHEMA_VERSION))
    {
        return Ok(PersistedCompletion::WorkbenchLaunchMarker);
    }
    serde_json::from_value(value)
        .map(Box::new)
        .map(PersistedCompletion::Response)
        .context("deserialize recorded daemon response")
}

fn raw_response_is_workbench_launch(json: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(json).map_or_else(
        |_| json.contains("workbench.launch") && json.contains("#ticket="),
        |value| {
            value
                .get("result")
                .and_then(|result| result.get("kind"))
                .and_then(serde_json::Value::as_str)
                == Some("workbench.launch")
        },
    )
}

fn generic_replay_response(
    request_id: &str,
    effect: Effect,
    completion: PersistedCompletion,
) -> ResponseEnvelope {
    match completion {
        PersistedCompletion::Response(response) => *response,
        PersistedCompletion::WorkbenchLaunchMarker => {
            workbench_launch_replay_unavailable_response(request_id.to_string(), effect)
        }
    }
}

fn replay_workbench_launch_completion(
    request_id: &str,
    effect: Effect,
    completion: PersistedCompletion,
    replay_launch: Option<&LaunchReplayCallback<'_>>,
) -> ResponseEnvelope {
    if !matches!(completion, PersistedCompletion::WorkbenchLaunchMarker) {
        // Old Exo versions durably recorded the full launch response. It may
        // contain a live bearer ticket, so never deserialize it into a replay.
        return workbench_launch_replay_unavailable_response(request_id.to_string(), effect);
    }
    let Some(mut response) = replay_launch.and_then(|replay| replay(request_id)) else {
        return workbench_launch_replay_unavailable_response(request_id.to_string(), effect);
    };
    response.id = request_id.to_string();
    response
}

pub fn resolved_request_effect(workspace_root: &Path, request: &RequestEnvelope) -> Option<Effect> {
    resolved_request_recovery(workspace_root, request).map(|recovery| recovery.effect)
}

fn execute_atomic_core<F, H>(
    project_db_path: &Path,
    request_hash: &str,
    effect: Effect,
    request: RequestEnvelope,
    execute: F,
    before_commit: H,
) -> Result<AtomicCoreExecution>
where
    F: FnOnce(RequestEnvelope) -> ResponseEnvelope,
    H: FnOnce() -> Result<()>,
{
    let request_id = request.id.clone();
    let transaction = RequestTransaction::begin(project_db_path)
        .map_err(crate::storage_compatibility::map_database_error)
        .context("begin atomic request transaction")?;
    let existing = transaction
        .database()
        .connection()
        .query_row(
            "SELECT request_hash, response_json
             FROM atomic_request_outcomes
             WHERE request_id = ?1",
            [&request_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;

    if let Some((stored_hash, _)) = &existing
        && stored_hash != request_hash
    {
        transaction.rollback()?;
        return Ok(AtomicCoreExecution {
            response: without_committed_effect(request_id_conflict_response(request_id, effect)),
            committed: false,
            replayed: false,
            request_id_conflict: true,
        });
    }

    if let Some((_, response_json)) = existing {
        let response = serde_json::from_str(&response_json)
            .context("deserialize canonical atomic request outcome")?;
        transaction.rollback()?;
        return Ok(AtomicCoreExecution {
            response,
            committed: true,
            replayed: true,
            request_id_conflict: false,
        });
    }

    let mut response = execute(request);
    if !atomic_response_commits(&response) {
        response.effect = None;
        transaction.rollback()?;
        return Ok(AtomicCoreExecution {
            response,
            committed: false,
            replayed: false,
            request_id_conflict: false,
        });
    }
    response.effect.get_or_insert(effect);

    let response_json =
        serde_json::to_string(&response).context("serialize canonical atomic request outcome")?;
    transaction.database().connection().execute(
        "INSERT INTO atomic_request_outcomes (
             request_id, request_hash, effect, response_json, committed_at
         ) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            request_id,
            request_hash,
            effect_name(effect),
            response_json,
            now_timestamp(),
        ],
    )?;
    before_commit()?;
    transaction
        .commit()
        .map_err(crate::storage_compatibility::map_database_error)?;

    Ok(AtomicCoreExecution {
        response,
        committed: true,
        replayed: false,
        request_id_conflict: false,
    })
}

fn atomic_response_commits(response: &ResponseEnvelope) -> bool {
    response.status == Status::Ok
        || response.status == Status::Error
            && response
                .error
                .as_ref()
                .is_some_and(|error| error.code == ErrorCode::PreconditionFailed)
            && response.error.as_ref().is_some_and(|error| {
                error
                    .details
                    .as_ref()
                    .is_some_and(contains_recorded_workflow_confirmation)
            })
}

fn is_retryable_daemon_busy_response(response: &ResponseEnvelope) -> bool {
    response.status == Status::Error
        && response.error.as_ref().is_some_and(|error| {
            error.code == ErrorCode::PreconditionFailed
                && error
                    .details
                    .as_ref()
                    .is_some_and(has_retryable_daemon_busy_details)
        })
}

fn has_retryable_daemon_busy_details(details: &serde_json::Value) -> bool {
    retryable_daemon_busy_details(details).is_some()
}

fn retryable_daemon_busy_details(details: &serde_json::Value) -> Option<&serde_json::Value> {
    let is_busy = details.get("kind").and_then(serde_json::Value::as_str) == Some("daemon.busy")
        && details
            .get("retry_with_same_request_id")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        && details
            .get("request_outcome_checked")
            .and_then(serde_json::Value::as_bool)
            == Some(false)
        && details
            .get("retryable")
            .and_then(serde_json::Value::as_bool)
            == Some(true);
    if is_busy {
        Some(details)
    } else {
        details
            .get("details")
            .and_then(retryable_daemon_busy_details)
    }
}

fn normalize_retryable_daemon_busy_response(mut response: ResponseEnvelope) -> ResponseEnvelope {
    if let Some(error) = response.error.as_mut()
        && let Some(details) = error
            .details
            .as_ref()
            .and_then(retryable_daemon_busy_details)
            .cloned()
    {
        error.details = Some(details);
    }
    without_committed_effect(response)
}

pub(crate) fn normalize_retryable_daemon_busy_response_if_needed(
    response: ResponseEnvelope,
) -> ResponseEnvelope {
    if is_retryable_daemon_busy_response(&response) {
        normalize_retryable_daemon_busy_response(response)
    } else {
        response
    }
}

fn without_committed_effect(mut response: ResponseEnvelope) -> ResponseEnvelope {
    response.effect = None;
    response
}

fn contains_recorded_workflow_confirmation(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(object) => {
            object
                .get("workflow_confirmation")
                .and_then(serde_json::Value::as_object)
                .and_then(|workflow| workflow.get("evidence_recorded"))
                .and_then(serde_json::Value::as_bool)
                == Some(true)
                || object.values().any(contains_recorded_workflow_confirmation)
        }
        serde_json::Value::Array(values) => {
            values.iter().any(contains_recorded_workflow_confirmation)
        }
        _ => false,
    }
}

fn is_rejected_execution_confirmation(response: &ResponseEnvelope) -> bool {
    response.status == Status::Error
        && response
            .error
            .as_ref()
            .is_some_and(|error| error.code == ErrorCode::ConfirmRequired)
}

fn is_transient_execution_confirmation(response: &ResponseEnvelope) -> bool {
    response.status == Status::ConfirmRequired || is_rejected_execution_confirmation(response)
}

fn request_hashes(request: &RequestEnvelope) -> Result<RequestHashes> {
    let legacy = legacy_request_hash(request)?;
    let current = request_hash(request)?;
    Ok(RequestHashes { current, legacy })
}

fn request_hash(request: &RequestEnvelope) -> Result<String> {
    let mut replay_identity = request.clone();
    // Auth advances from absent to approved without changing the operation.
    replay_identity.auth = None;
    let bytes = serde_json::to_vec(&replay_identity)?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

fn legacy_request_hash(request: &RequestEnvelope) -> Result<String> {
    let bytes = serde_json::to_vec(request)?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

fn now_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs() as i64
}

fn create_owner_only_file_if_missing(path: &Path) -> Result<()> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    options.mode(0o600);
    drop(
        options
            .open(path)
            .with_context(|| format!("create daemon outcome ledger {}", path.display()))?,
    );
    set_owner_only_file_permissions(path)
}

fn outcome_file_paths(path: &Path) -> [PathBuf; 3] {
    let with_suffix = |suffix: &str| {
        let mut candidate = path.as_os_str().to_os_string();
        candidate.push(suffix);
        PathBuf::from(candidate)
    };
    [path.to_path_buf(), with_suffix("-wal"), with_suffix("-shm")]
}

#[cfg(unix)]
fn set_owner_only_file_permissions(path: &Path) -> Result<()> {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("secure daemon outcome ledger file {}", path.display()))
}

#[cfg(not(unix))]
fn set_owner_only_file_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

const fn effect_name(effect: Effect) -> &'static str {
    match effect {
        Effect::Pure => "pure",
        Effect::Write => "write",
        Effect::Exec => "exec",
    }
}

fn effect_from_name(name: &str) -> Result<Effect> {
    match name {
        "pure" => Ok(Effect::Pure),
        "write" => Ok(Effect::Write),
        "exec" => Ok(Effect::Exec),
        _ => Err(anyhow!("unknown recorded daemon effect {name}")),
    }
}

const fn recovery_class_name(recovery_class: RecoveryClass) -> &'static str {
    match recovery_class {
        RecoveryClass::ReplayableRead => "replayable_read",
        RecoveryClass::AtomicProjectState => "atomic_project_state",
        RecoveryClass::ExternalAtMostOnce => "external_at_most_once",
    }
}

fn recovery_class_from_name(name: &str) -> Option<RecoveryClass> {
    match name {
        "replayable_read" => Some(RecoveryClass::ReplayableRead),
        "atomic_project_state" => Some(RecoveryClass::AtomicProjectState),
        "external_at_most_once" => Some(RecoveryClass::ExternalAtMostOnce),
        _ => None,
    }
}

const fn response_error(
    id: String,
    effect: Effect,
    code: ErrorCode,
    message: String,
    details: serde_json::Value,
) -> ResponseEnvelope {
    ResponseEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id,
        status: Status::Error,
        result: None,
        error: Some(ErrorBody {
            code,
            message,
            details: Some(details),
        }),
        ticket: None,
        steering: None,
        reminders: None,
        display: None,
        preview: None,
        effect: Some(effect),
        trace: None,
    }
}

fn request_id_conflict_response(id: String, effect: Effect) -> ResponseEnvelope {
    response_error(
        id.clone(),
        effect,
        ErrorCode::InvalidInput,
        format!("daemon request id {id} was reused with a different request payload"),
        serde_json::json!({
            "kind": "daemon.request_id_conflict",
            "request_id": id,
            "mutation_performed": false,
        }),
    )
}

fn in_flight_response(
    id: String,
    effect: Effect,
    owner_instance_id: &str,
    previous_instance: bool,
) -> ResponseEnvelope {
    let kind = if previous_instance {
        "daemon.request_outcome_indeterminate"
    } else {
        "daemon.request_outcome_pending"
    };
    let message = if previous_instance {
        "The daemon was replaced while this request was in progress. Exo preserved the request identity and will not execute it twice, but no completed outcome was recorded."
    } else {
        "The original daemon request is still in progress. Exo preserved the request identity and did not execute it twice."
    };
    response_error(
        id.clone(),
        effect,
        ErrorCode::PreconditionFailed,
        message.to_string(),
        serde_json::json!({
            "kind": kind,
            "request_id": id,
            "effect": effect_name(effect),
            "owner_instance_id": owner_instance_id,
            "previous_instance": previous_instance,
            "mutation_replayed": false,
        }),
    )
}

fn workbench_launch_replay_unavailable_response(id: String, effect: Effect) -> ResponseEnvelope {
    response_error(
        id.clone(),
        effect,
        ErrorCode::PreconditionFailed,
        "The original workbench launch cannot be replayed safely. Use a new request ID to issue a fresh one-time enrollment link."
            .to_string(),
        serde_json::json!({
            "kind": "workbench.launch_replay_unavailable",
            "request_id": id,
            "retry_with_new_request_id": true,
            "mutation_replayed": false,
        }),
    )
}

fn ledger_error_response(
    id: String,
    effect: Effect,
    kind: &str,
    error: anyhow::Error,
    mutation_may_have_completed: bool,
) -> ResponseEnvelope {
    if let Some(failure) =
        crate::storage_compatibility::writer_compatibility_failure_from_error(&error)
    {
        return response_error(
            id,
            effect,
            failure.error.code,
            failure.error.message,
            failure
                .error
                .details
                .unwrap_or_else(|| serde_json::json!({})),
        );
    }
    response_error(
        id.clone(),
        effect,
        ErrorCode::Internal,
        format!("daemon request outcome persistence failed: {error}"),
        serde_json::json!({
            "kind": kind,
            "request_id": id,
            "effect": effect_name(effect),
            "mutation_may_have_completed": mutation_may_have_completed,
            "mutation_replayed": false,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::protocol::{CallParams, Op};
    use crate::context::SqliteWriter;
    use exosuit_storage::open_database;
    use std::cell::Cell;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier, Mutex, mpsc};

    #[test]
    fn wrapped_writer_compatibility_error_survives_atomic_daemon_response() {
        let error = crate::storage_compatibility::map_writer_compatibility_error(
            exosuit_storage::WriterCompatibilityError::Busy {
                lock_path: std::path::PathBuf::from("/tmp/exo.writer-compat.lock"),
            },
        )
        .context("begin atomic request transaction");

        let response = ledger_error_response(
            "request-compat".to_string(),
            Effect::Write,
            "daemon.atomic_request_commit_failed",
            error,
            false,
        );

        let error = response.error.expect("structured error");
        assert_eq!(error.code, ErrorCode::PreconditionFailed);
        let details = error.details.expect("compatibility details");
        assert_eq!(details["kind"], "storage.compatibility_busy");
        assert_eq!(details["request_outcome_checked"], false);
        assert_eq!(details["retry_with_same_request_id"], true);
        assert_eq!(details["retryable"], true);
    }

    fn request(id: &str, task_id: &str) -> RequestEnvelope {
        RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            id: id.to_string(),
            op: Op::Call(CallParams {
                address: Address::Operation {
                    path: vec!["task".to_string(), "complete".to_string()],
                },
                input: serde_json::json!({ "id": task_id, "log": "Done" }),
            }),
            workspace_root: None,
            auth: None,
            workflow_confirmation: None,
            agent_id: None,
        }
    }

    fn response(id: &str) -> ResponseEnvelope {
        ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            id: id.to_string(),
            status: Status::Ok,
            result: Some(serde_json::json!({ "completed": true })),
            error: None,
            ticket: None,
            steering: None,
            reminders: None,
            display: None,
            preview: None,
            effect: Some(Effect::Write),
            trace: None,
        }
    }

    fn launch_request(id: &str, workspace_root: &str) -> RequestEnvelope {
        RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            id: id.to_string(),
            op: Op::Call(CallParams {
                address: Address::Operation {
                    path: vec!["workbench".to_string(), "launch".to_string()],
                },
                input: serde_json::json!({}),
            }),
            workspace_root: Some(PathBuf::from(workspace_root)),
            auth: None,
            workflow_confirmation: None,
            agent_id: None,
        }
    }

    fn launch_response(id: &str, bearer: &str) -> ResponseEnvelope {
        ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            id: id.to_string(),
            status: Status::Ok,
            result: Some(serde_json::json!({
                "kind": "workbench.launch",
                "ok": true,
                "schema_version": 2,
                "url": format!("https://workbench.example/#ticket={bearer}"),
            })),
            error: None,
            ticket: None,
            steering: None,
            reminders: None,
            display: Some(crate::api::protocol::Display {
                invocation_message: "Launching workbench".to_string(),
                summary: "Open the Exo workbench".to_string(),
                body: Some(format!("Open https://workbench.example/#ticket={bearer}")),
            }),
            preview: None,
            effect: Some(Effect::Write),
            trace: None,
        }
    }

    fn response_json(response: &ResponseEnvelope) -> serde_json::Value {
        serde_json::to_value(response).expect("serialize response")
    }

    fn error_kind(response: &ResponseEnvelope) -> Option<&str> {
        response
            .error
            .as_ref()?
            .details
            .as_ref()?
            .get("kind")?
            .as_str()
    }

    fn reconcile_busy_response(id: &str) -> ResponseEnvelope {
        ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            id: id.to_string(),
            status: Status::Error,
            result: None,
            error: Some(ErrorBody {
                code: ErrorCode::PreconditionFailed,
                message: "RFC reconciliation is busy; retry later with the same request ID"
                    .to_string(),
                details: Some(serde_json::json!({
                    "kind": "daemon.busy",
                    "reason": "rfc_reconcile_lock",
                    "retryable": true,
                    "retry_with_same_request_id": true,
                    "request_outcome_checked": false,
                })),
            }),
            ticket: None,
            steering: None,
            reminders: None,
            display: None,
            preview: None,
            effect: Some(Effect::Write),
            trace: None,
        }
    }

    fn confirm_required_response(id: &str) -> ResponseEnvelope {
        ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            id: id.to_string(),
            status: Status::ConfirmRequired,
            result: None,
            error: None,
            ticket: Some("ticket".to_string()),
            steering: None,
            reminders: None,
            display: None,
            preview: None,
            effect: Some(Effect::Exec),
            trace: None,
        }
    }

    fn rejected_confirmation_response(id: &str) -> ResponseEnvelope {
        ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            id: id.to_string(),
            status: Status::Error,
            result: None,
            error: Some(ErrorBody {
                code: ErrorCode::ConfirmRequired,
                message: "Invalid confirmation ticket".to_string(),
                details: None,
            }),
            ticket: None,
            steering: None,
            reminders: None,
            display: None,
            preview: None,
            effect: Some(Effect::Exec),
            trace: None,
        }
    }

    #[test]
    fn request_hash_ignores_authorization_but_preserves_workspace_identity() {
        let mut original = request("approved-retry", "task-a");
        original.workspace_root = Some(PathBuf::from("/workspace-a"));
        let mut approved = original.clone();
        approved.auth = Some(crate::api::protocol::Auth {
            ticket: "ticket".to_string(),
            confirm: true,
            request_id: Some(original.id.clone()),
            workspace_root: original.workspace_root.clone(),
        });
        let mut other_workspace = approved.clone();
        other_workspace.workspace_root = Some(PathBuf::from("/workspace-b"));

        assert_eq!(
            request_hash(&original).unwrap(),
            request_hash(&approved).unwrap()
        );
        assert_ne!(
            request_hash(&approved).unwrap(),
            request_hash(&other_workspace).unwrap()
        );
    }

    #[test]
    fn workbench_launch_replays_exact_memory_response_without_persisting_bearer() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ledger = RequestOutcomeLedger::open(temp.path().join(DAEMON_OUTCOME_DB_NAME))
            .expect("open ledger");
        let request = launch_request("launch-once", "/workspace-a");
        let expected = launch_response(&request.id, "secret-launch-bearer");
        let cached = Arc::new(Mutex::new(None::<ResponseEnvelope>));
        let invocations = AtomicUsize::new(0);

        let first_cache = Arc::clone(&cached);
        let first = ledger.execute_workbench_launch(
            request.clone(),
            Effect::Write,
            "instance-a",
            Duration::ZERO,
            |_| {
                invocations.fetch_add(1, Ordering::Relaxed);
                expected.clone()
            },
            |_, response| {
                *first_cache.lock().expect("cache launch") = Some(response.clone());
                Ok(())
            },
            |_| None,
            |_| {},
        );
        assert_eq!(response_json(&first.response), response_json(&expected));

        let replay_cache = Arc::clone(&cached);
        let second = ledger.execute_workbench_launch(
            request.clone(),
            Effect::Write,
            "instance-a",
            Duration::ZERO,
            |_| panic!("same request must not launch again"),
            |_, _| panic!("replay must not replace the cache"),
            move |_| replay_cache.lock().expect("read cache").clone(),
            |_| {},
        );
        assert!(second.replayed);
        assert_eq!(response_json(&second.response), response_json(&expected));
        assert_eq!(invocations.load(Ordering::Relaxed), 1);

        let recorded: String = ledger
            .connection()
            .expect("open ledger")
            .query_row(
                "SELECT response_json FROM daemon_request_outcomes WHERE request_id = ?1",
                [&request.id],
                |row| row.get(0),
            )
            .expect("recorded marker");
        assert_eq!(recorded, workbench_launch_completion_marker_json());
        assert!(!recorded.contains("secret-launch-bearer"));
        assert!(!recorded.contains("url"));
        assert!(!recorded.contains("display"));
        let path = ledger.path().to_path_buf();
        drop(ledger);
        for candidate in outcome_file_paths(&path) {
            if candidate.exists() {
                let bytes = std::fs::read(&candidate).expect("read outcome file");
                assert!(
                    !bytes
                        .windows("secret-launch-bearer".len())
                        .any(|window| window == b"secret-launch-bearer"),
                    "launch bearer reached {}",
                    candidate.display()
                );
            }
        }
    }

    #[test]
    fn post_commit_prune_failure_preserves_exact_launch_for_owner_and_waiter() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ledger = Arc::new(
            RequestOutcomeLedger::open(temp.path().join(DAEMON_OUTCOME_DB_NAME))
                .expect("open ledger"),
        );
        let connection = ledger.connection().expect("open prune fixture connection");
        connection
            .execute(
                "INSERT INTO daemon_request_outcomes (
                     request_id, request_hash, effect, instance_id, recovery_class,
                     response_json, started_at, completed_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 0)",
                params![
                    "stale-prune-probe",
                    "stale-hash",
                    "write",
                    "stale-instance",
                    "external_at_most_once",
                    serde_json::to_string(&response("stale-prune-probe"))
                        .expect("serialize stale response"),
                ],
            )
            .expect("insert stale completed response");
        connection
            .execute_batch(
                "CREATE TRIGGER fail_stale_prune
                 BEFORE DELETE ON daemon_request_outcomes
                 WHEN OLD.request_id = 'stale-prune-probe'
                 BEGIN
                     SELECT RAISE(ABORT, 'forced prune failure');
                 END;",
            )
            .expect("install deterministic prune failure");
        drop(connection);
        let request = launch_request("launch-concurrent", "/workspace-a");
        let expected = launch_response(&request.id, "concurrent-launch-bearer");
        let cached = Arc::new(Mutex::new(None::<ResponseEnvelope>));
        let invocations = Arc::new(AtomicUsize::new(0));
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();

        let owner_ledger = Arc::clone(&ledger);
        let owner_request = request.clone();
        let owner_expected = expected.clone();
        let owner_cache = Arc::clone(&cached);
        let owner_invocations = Arc::clone(&invocations);
        let owner = std::thread::spawn(move || {
            owner_ledger.execute_workbench_launch(
                owner_request,
                Effect::Write,
                "instance-a",
                Duration::from_secs(2),
                |_| {
                    owner_invocations.fetch_add(1, Ordering::Relaxed);
                    started_tx.send(()).expect("announce launch");
                    release_rx.recv().expect("release launch");
                    owner_expected
                },
                |_, response| {
                    *owner_cache.lock().expect("cache launch") = Some(response.clone());
                    Ok(())
                },
                |_| None,
                |_| {},
            )
        });
        started_rx.recv().expect("launch started");

        let waiter_ledger = Arc::clone(&ledger);
        let waiter_request = request;
        let waiter_cache = Arc::clone(&cached);
        let waiter = std::thread::spawn(move || {
            waiter_ledger.execute_workbench_launch(
                waiter_request,
                Effect::Write,
                "instance-a",
                Duration::from_secs(2),
                |_| panic!("waiter must not launch again"),
                |_, _| panic!("waiter must not replace cache"),
                move |_| waiter_cache.lock().expect("read cache").clone(),
                |_| {},
            )
        });
        let waiter_deadline = Instant::now() + Duration::from_secs(1);
        while ledger.notifications.waiters.load(Ordering::Acquire) == 0
            && Instant::now() < waiter_deadline
        {
            std::thread::yield_now();
        }
        assert_eq!(
            ledger.notifications.waiters.load(Ordering::Acquire),
            1,
            "the concurrent retry must be waiting on the owner's in-flight reservation"
        );
        release_tx.send(()).expect("release owner");

        let owner = owner.join().expect("join owner");
        let waiter = waiter.join().expect("join waiter");
        assert_eq!(response_json(&owner.response), response_json(&expected));
        assert_eq!(response_json(&waiter.response), response_json(&expected));
        assert!(waiter.replayed);
        assert_eq!(invocations.load(Ordering::Relaxed), 1);

        let connection = ledger.connection().expect("inspect completed launch");
        let marker: String = connection
            .query_row(
                "SELECT response_json FROM daemon_request_outcomes WHERE request_id = ?1",
                [&expected.id],
                |row| row.get(0),
            )
            .expect("launch marker was committed");
        assert_eq!(marker, workbench_launch_completion_marker_json());
        let stale_row_exists: bool = connection
            .query_row(
                "SELECT EXISTS (
                     SELECT 1 FROM daemon_request_outcomes WHERE request_id = 'stale-prune-probe'
                 )",
                [],
                |row| row.get(0),
            )
            .expect("inspect stale prune probe");
        assert!(stale_row_exists, "the forced retention prune must fail");
        assert!(
            ledger.prune_completed(&connection).is_err(),
            "the fixture must deterministically reject retention pruning"
        );
    }

    #[test]
    fn completed_outcome_pruning_preserves_launch_tombstones() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ledger = RequestOutcomeLedger::open(temp.path().join(DAEMON_OUTCOME_DB_NAME))
            .expect("open ledger");
        let launch = launch_request("launch-retained-tombstone", "/workspace-a");
        let first = ledger.execute_workbench_launch(
            launch.clone(),
            Effect::Write,
            "instance-a",
            Duration::ZERO,
            |_| launch_response(&launch.id, "retention-bearer"),
            |_, _| Ok(()),
            |_| None,
            |_| {},
        );
        assert_eq!(first.response.status, Status::Ok);
        let ordinary = request("ordinary-expired-outcome", "task-a");
        let ordinary_result = ledger.execute(
            ordinary.clone(),
            Effect::Write,
            "instance-a",
            Duration::ZERO,
            |request| response(&request.id),
        );
        assert_eq!(ordinary_result.response.status, Status::Ok);

        let connection = ledger.connection().expect("open retention fixture");
        connection
            .execute(
                "UPDATE daemon_request_outcomes SET completed_at = ?1
                 WHERE request_id IN (?2, ?3)",
                params![
                    now_timestamp() - COMPLETED_OUTCOME_RETENTION_SECS - 1,
                    launch.id,
                    ordinary.id,
                ],
            )
            .expect("expire completed outcomes");
        ledger
            .prune_completed(&connection)
            .expect("prune ordinary outcomes");

        let retained: bool = connection
            .query_row(
                "SELECT EXISTS (
                     SELECT 1 FROM daemon_request_outcomes WHERE request_id = ?1
                 )",
                [&launch.id],
                |row| row.get(0),
            )
            .expect("inspect launch tombstone");
        let ordinary_retained: bool = connection
            .query_row(
                "SELECT EXISTS (
                     SELECT 1 FROM daemon_request_outcomes WHERE request_id = ?1
                 )",
                [&ordinary.id],
                |row| row.get(0),
            )
            .expect("inspect ordinary outcome");
        assert!(retained, "launch request IDs remain terminal");
        assert!(
            !ordinary_retained,
            "ordinary completed outcomes still prune"
        );
        drop(connection);

        let retry = ledger.execute_workbench_launch(
            launch,
            Effect::Write,
            "instance-b",
            Duration::ZERO,
            |_| panic!("retained launch request ID must not execute again"),
            |_, _| panic!("retained launch request ID must not replace its marker"),
            |_| None,
            |_| {},
        );
        assert!(retry.replayed);
        assert_eq!(
            error_kind(&retry.response),
            Some("workbench.launch_replay_unavailable")
        );
    }

    #[test]
    fn workbench_launch_marker_fails_closed_without_live_same_daemon_cache() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join(DAEMON_OUTCOME_DB_NAME);
        let request = launch_request("launch-cross-daemon", "/workspace-a");
        let first_ledger = RequestOutcomeLedger::open(&path).expect("open first ledger");
        let first = first_ledger.execute_workbench_launch(
            request.clone(),
            Effect::Write,
            "instance-a",
            Duration::ZERO,
            |_| launch_response(&request.id, "cross-daemon-bearer"),
            |_, _| Ok(()),
            |_| None,
            |_| {},
        );
        assert_eq!(first.response.status, Status::Ok);
        drop(first_ledger);

        let replacement = RequestOutcomeLedger::open(path).expect("open replacement ledger");
        let replay = replacement.execute_workbench_launch(
            request,
            Effect::Write,
            "instance-b",
            Duration::ZERO,
            |_| panic!("replacement daemon must not launch again"),
            |_, _| panic!("replacement daemon must not retain a response"),
            |_| None,
            |_| {},
        );
        assert!(replay.replayed);
        assert_eq!(replay.response.status, Status::Error);
        assert_eq!(
            error_kind(&replay.response),
            Some("workbench.launch_replay_unavailable")
        );
        assert_eq!(
            replay
                .response
                .error
                .as_ref()
                .unwrap()
                .details
                .as_ref()
                .unwrap()["retry_with_new_request_id"],
            true
        );
    }

    #[test]
    fn terminal_workbench_launch_error_requires_a_new_request_id_on_retry() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ledger = RequestOutcomeLedger::open(temp.path().join(DAEMON_OUTCOME_DB_NAME))
            .expect("open ledger");
        let request = launch_request("launch-terminal-error", "/workspace-a");
        let original = error_response(
            &request.id,
            ErrorCode::PreconditionFailed,
            Some(serde_json::json!({ "kind": "workbench.publisher_not_ready" })),
        );
        let first = ledger.execute_workbench_launch(
            request.clone(),
            Effect::Write,
            "instance-a",
            Duration::ZERO,
            |_| original.clone(),
            |_, _| Ok(()),
            |_| None,
            |_| {},
        );
        assert_eq!(response_json(&first.response), response_json(&original));

        let retry = ledger.execute_workbench_launch(
            request,
            Effect::Write,
            "instance-a",
            Duration::ZERO,
            |_| panic!("terminal launch error must not execute twice"),
            |_, _| panic!("terminal launch error has no replay cache"),
            |_| None,
            |_| {},
        );
        assert_eq!(
            error_kind(&retry.response),
            Some("workbench.launch_replay_unavailable")
        );
    }

    #[test]
    fn workbench_launch_request_conflict_never_consults_live_cache() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ledger = RequestOutcomeLedger::open(temp.path().join(DAEMON_OUTCOME_DB_NAME))
            .expect("open ledger");
        let original = launch_request("launch-conflict", "/workspace-a");
        ledger.execute_workbench_launch(
            original.clone(),
            Effect::Write,
            "instance-a",
            Duration::ZERO,
            |_| launch_response(&original.id, "conflict-bearer"),
            |_, _| Ok(()),
            |_| None,
            |_| {},
        );

        let conflicting = launch_request("launch-conflict", "/workspace-b");
        let replay_consulted = Cell::new(false);
        let conflict = ledger.execute_workbench_launch(
            conflicting,
            Effect::Write,
            "instance-a",
            Duration::ZERO,
            |_| panic!("conflicting request must not execute"),
            |_, _| panic!("conflicting request must not retain"),
            |_| {
                replay_consulted.set(true);
                None
            },
            |_| {},
        );
        assert_eq!(
            error_kind(&conflict.response),
            Some("daemon.request_id_conflict")
        );
        assert!(!replay_consulted.get());
    }

    #[test]
    fn completed_legacy_authenticated_hash_replays_after_upgrade() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ledger = RequestOutcomeLedger::open(temp.path().join(DAEMON_OUTCOME_DB_NAME))
            .expect("open ledger");
        let mut approved = request("legacy-approved-retry", "task-a");
        approved.auth = Some(crate::api::protocol::Auth {
            ticket: "legacy-ticket".to_string(),
            confirm: true,
            request_id: Some(approved.id.clone()),
            workspace_root: None,
        });
        let current_hash = request_hash(&approved).expect("current request hash");
        let legacy_hash = legacy_request_hash(&approved).expect("legacy request hash");
        assert_ne!(current_hash, legacy_hash);
        let recorded_response = response(&approved.id);
        let response_json = serde_json::to_string(&recorded_response).expect("serialize response");
        ledger
            .connection()
            .expect("open ledger connection")
            .execute(
                "INSERT INTO daemon_request_outcomes (
                     request_id, request_hash, effect, instance_id, recovery_class,
                     response_json, started_at, completed_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    approved.id,
                    legacy_hash,
                    effect_name(Effect::Exec),
                    "instance-before-upgrade",
                    recovery_class_name(RecoveryClass::ExternalAtMostOnce),
                    response_json,
                    now_timestamp(),
                    now_timestamp(),
                ],
            )
            .expect("insert legacy outcome");

        let replay = ledger
            .terminal_outcome_before_preparation(&approved)
            .expect("read legacy outcome")
            .expect("legacy outcome should match");

        assert!(replay.replayed);
        assert_eq!(replay.response.id, recorded_response.id);
        assert_eq!(replay.response.status, recorded_response.status);
        assert_eq!(replay.response.result, recorded_response.result);
    }

    #[test]
    fn legacy_raw_workbench_launch_is_scrubbed_and_fails_closed() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join(DAEMON_OUTCOME_DB_NAME);
        let request = launch_request("legacy-launch", "/workspace-a");
        let ledger = RequestOutcomeLedger::open(&path).expect("open ledger");
        let response = launch_response(&request.id, "legacy-secret-bearer");
        ledger
            .connection()
            .expect("open ledger connection")
            .execute(
                "INSERT INTO daemon_request_outcomes (
                     request_id, request_hash, effect, instance_id, recovery_class,
                     response_json, started_at, completed_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    request.id,
                    request_hash(&request).unwrap(),
                    effect_name(Effect::Write),
                    "legacy-instance",
                    recovery_class_name(RecoveryClass::ExternalAtMostOnce),
                    serde_json::to_string(&response).unwrap(),
                    now_timestamp(),
                    now_timestamp(),
                ],
            )
            .expect("insert legacy launch response");
        drop(ledger);

        let ledger = RequestOutcomeLedger::open(&path).expect("reopen and scrub ledger");
        let replay = ledger
            .terminal_outcome_before_preparation_with_launch_replay(&request, Some(&|_| None))
            .expect("read scrubbed outcome")
            .expect("terminal launch marker");
        assert_eq!(
            error_kind(&replay.response),
            Some("workbench.launch_replay_unavailable")
        );
        let recorded: String = ledger
            .connection()
            .unwrap()
            .query_row(
                "SELECT response_json FROM daemon_request_outcomes WHERE request_id = ?1",
                [&request.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(recorded, workbench_launch_completion_marker_json());

        drop(ledger);
        for candidate in outcome_file_paths(&path) {
            if candidate.exists() {
                let bytes = std::fs::read(&candidate).expect("read outcome file");
                assert!(
                    !bytes
                        .windows("legacy-secret-bearer".len())
                        .any(|window| window == b"legacy-secret-bearer"),
                    "legacy bearer remained in {}",
                    candidate.display()
                );
            }
        }
    }

    #[test]
    fn generic_external_response_containing_ticket_fragment_replays_unchanged() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join(DAEMON_OUTCOME_DB_NAME);
        let ledger = RequestOutcomeLedger::open(&path).expect("open ledger");
        let request = request("generic-ticket-fragment", "task-a");
        let mut response = response(&request.id);
        response.result = Some(serde_json::json!({
            "note": "documentation example https://example.test/#ticket=not-a-workbench-ticket"
        }));
        let first = ledger.execute(
            request.clone(),
            Effect::Write,
            "instance-a",
            Duration::ZERO,
            |_| response.clone(),
        );
        assert_eq!(response_json(&first.response), response_json(&response));
        drop(ledger);

        let ledger = RequestOutcomeLedger::open(path).expect("reopen ledger");
        let second = ledger.execute(request, Effect::Write, "instance-b", Duration::ZERO, |_| {
            panic!("generic response must replay")
        });
        assert_eq!(response_json(&second.response), response_json(&response));
    }

    #[cfg(unix)]
    #[test]
    fn daemon_outcome_database_and_live_wal_files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt as _;

        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join(DAEMON_OUTCOME_DB_NAME);
        let ledger = RequestOutcomeLedger::open(&path).expect("open ledger");
        let connection = ledger.connection().expect("open live connection");
        connection
            .pragma_update(None, "wal_autocheckpoint", 0)
            .expect("disable automatic WAL checkpoints");
        connection
            .execute_batch(
                "BEGIN IMMEDIATE;
                 CREATE TABLE IF NOT EXISTS outcome_permission_probe (value INTEGER);
                 INSERT INTO outcome_permission_probe VALUES (1);
                 COMMIT;",
            )
            .expect("materialize WAL files");

        let outcome_files = outcome_file_paths(&path);
        for candidate in &outcome_files {
            assert!(
                candidate.exists(),
                "normal ledger writes must materialize {}",
                candidate.display()
            );
            let mode = std::fs::metadata(candidate)
                .expect("outcome metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "unexpected mode for {}", candidate.display());
        }

        for candidate in &outcome_files {
            std::fs::set_permissions(candidate, std::fs::Permissions::from_mode(0o644))
                .expect("weaken fixture permissions");
        }
        let repair = ledger.connection().expect("reopen and repair permissions");
        for candidate in &outcome_files {
            let mode = std::fs::metadata(candidate)
                .expect("repaired outcome metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(
                mode,
                0o600,
                "mode was not repaired for {}",
                candidate.display()
            );
        }
        drop(repair);
        drop(connection);
    }

    #[test]
    fn approved_retry_discards_legacy_terminal_confirmation() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ledger = RequestOutcomeLedger::open(temp.path().join(DAEMON_OUTCOME_DB_NAME))
            .expect("open ledger");
        let mut approved = request("legacy-confirmation-retry", "task-a");
        let stored_hash = request_hash(&approved).expect("request hash");
        let stored_response =
            serde_json::to_string(&confirm_required_response(&approved.id)).expect("serialize");
        ledger
            .connection()
            .expect("open ledger connection")
            .execute(
                "INSERT INTO daemon_request_outcomes (
                     request_id, request_hash, effect, instance_id, recovery_class,
                     response_json, started_at, completed_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    approved.id,
                    stored_hash,
                    effect_name(Effect::Exec),
                    "instance-before-upgrade",
                    recovery_class_name(RecoveryClass::ExternalAtMostOnce),
                    stored_response,
                    now_timestamp(),
                    now_timestamp(),
                ],
            )
            .expect("insert legacy confirmation");
        approved.auth = Some(crate::api::protocol::Auth {
            ticket: "ticket".to_string(),
            confirm: true,
            request_id: Some(approved.id.clone()),
            workspace_root: None,
        });

        assert!(
            ledger
                .terminal_outcome_before_preparation(&approved)
                .expect("inspect terminal outcome")
                .is_none(),
            "approved retry must continue instead of replaying the old prompt"
        );
        let retained: i64 = ledger
            .connection()
            .expect("open ledger connection")
            .query_row(
                "SELECT COUNT(*) FROM daemon_request_outcomes WHERE request_id = ?1",
                [&approved.id],
                |row| row.get(0),
            )
            .expect("count retained outcomes");
        assert_eq!(retained, 0);
    }

    #[test]
    fn confirm_required_response_releases_reservation_for_approved_retry() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ledger = RequestOutcomeLedger::open(temp.path().join(DAEMON_OUTCOME_DB_NAME)).unwrap();
        let request = request("approved-retry", "task-a");
        let invocations = Cell::new(0);

        let first = ledger.execute(
            request.clone(),
            Effect::Exec,
            "instance-a",
            Duration::ZERO,
            |request| {
                invocations.set(invocations.get() + 1);
                confirm_required_response(&request.id)
            },
        );
        assert_eq!(first.response.status, Status::ConfirmRequired);

        let mut approved = request;
        approved.auth = Some(crate::api::protocol::Auth {
            ticket: "ticket".to_string(),
            confirm: true,
            request_id: Some(approved.id.clone()),
            workspace_root: None,
        });
        let second = ledger.execute(
            approved,
            Effect::Exec,
            "instance-a",
            Duration::ZERO,
            |request| {
                invocations.set(invocations.get() + 1);
                response(&request.id)
            },
        );

        assert_eq!(second.response.status, Status::Ok);
        assert!(!second.replayed);
        assert_eq!(invocations.get(), 2);
    }

    #[test]
    fn rejected_confirmation_releases_reservation_for_corrected_retry() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ledger = RequestOutcomeLedger::open(temp.path().join(DAEMON_OUTCOME_DB_NAME)).unwrap();
        let mut request = request("corrected-approval", "task-a");
        request.auth = Some(crate::api::protocol::Auth {
            ticket: "wrong-ticket".to_string(),
            confirm: true,
            request_id: Some(request.id.clone()),
            workspace_root: None,
        });
        let invocations = Cell::new(0);

        let first = ledger.execute(
            request.clone(),
            Effect::Exec,
            "instance-a",
            Duration::ZERO,
            |request| {
                invocations.set(invocations.get() + 1);
                rejected_confirmation_response(&request.id)
            },
        );
        assert_eq!(first.response.status, Status::Error);
        assert_eq!(
            first.response.error.as_ref().map(|error| error.code),
            Some(ErrorCode::ConfirmRequired)
        );

        let mut corrected = request;
        corrected.auth.as_mut().expect("approved auth").ticket = "correct-ticket".to_string();
        let second = ledger.execute(
            corrected,
            Effect::Exec,
            "instance-a",
            Duration::ZERO,
            |request| {
                invocations.set(invocations.get() + 1);
                response(&request.id)
            },
        );

        assert_eq!(second.response.status, Status::Ok);
        assert!(!second.replayed);
        assert_eq!(invocations.get(), 2);
    }

    #[test]
    fn reconcile_busy_response_abandons_external_runtime_reservation() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ledger = RequestOutcomeLedger::open(temp.path().join(DAEMON_OUTCOME_DB_NAME)).unwrap();
        let request = request("retry-after-reconcile-busy", "task-a");
        let invocations = Cell::new(0);

        let first = ledger.execute(
            request.clone(),
            Effect::Write,
            "instance-a",
            Duration::ZERO,
            |request| {
                invocations.set(invocations.get() + 1);
                reconcile_busy_response(&request.id)
            },
        );
        assert_eq!(first.response.status, Status::Error);
        assert_eq!(first.response.effect, None);
        let details = first
            .response
            .error
            .as_ref()
            .and_then(|error| error.details.as_ref())
            .expect("normalized busy details");
        assert_eq!(details["kind"], "daemon.busy");
        assert_eq!(details["request_outcome_checked"], false);
        assert_eq!(details["retry_with_same_request_id"], true);
        assert_eq!(details["retryable"], true);
        assert!(details.get("details").is_none());

        let second = ledger.execute(
            request,
            Effect::Write,
            "instance-a",
            Duration::ZERO,
            |request| {
                invocations.set(invocations.get() + 1);
                response(&request.id)
            },
        );
        assert_eq!(second.response.status, Status::Ok);
        assert_eq!(invocations.get(), 2, "the busy result must not be replayed");
    }

    #[test]
    fn wrapped_reconcile_busy_response_abandons_external_runtime_reservation() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ledger = RequestOutcomeLedger::open(temp.path().join(DAEMON_OUTCOME_DB_NAME)).unwrap();
        let request = request("retry-after-wrapped-reconcile-busy", "task-a");
        let mut busy = reconcile_busy_response(&request.id);
        let details = busy
            .error
            .as_mut()
            .and_then(|error| error.details.take())
            .unwrap();
        busy.error.as_mut().unwrap().details = Some(serde_json::json!({
            "details": details,
            "steering": [],
        }));

        let first = ledger.execute(
            request.clone(),
            Effect::Write,
            "instance-a",
            Duration::ZERO,
            |_| busy,
        );
        assert_eq!(first.response.status, Status::Error);
        assert_eq!(first.response.effect, None);
        let details = first
            .response
            .error
            .as_ref()
            .and_then(|error| error.details.as_ref())
            .expect("normalized busy details");
        assert_eq!(details["kind"], "daemon.busy");
        assert_eq!(details["request_outcome_checked"], false);
        assert_eq!(details["retry_with_same_request_id"], true);
        assert_eq!(details["retryable"], true);
        assert!(details.get("details").is_none());

        let second = ledger.execute(
            request,
            Effect::Write,
            "instance-a",
            Duration::ZERO,
            |request| response(&request.id),
        );
        assert_eq!(second.response.status, Status::Ok);
    }

    #[test]
    fn same_instance_waiter_re_reserves_after_busy_owner_abandons() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ledger = Arc::new(
            RequestOutcomeLedger::open(temp.path().join(DAEMON_OUTCOME_DB_NAME))
                .expect("open ledger"),
        );
        let request = request("waiter-after-reconcile-busy", "task-a");
        let request_hash = request_hash(&request).unwrap();
        assert!(matches!(
            ledger
                .reserve(
                    &request.id,
                    &request_hash,
                    Effect::Write,
                    RecoveryClass::ExternalAtMostOnce,
                    "instance-a",
                )
                .unwrap(),
            Reservation::Execute
        ));

        let waiter_ledger = Arc::clone(&ledger);
        let waiter_request = request.clone();
        let (started_tx, started_rx) = mpsc::channel();
        let waiter = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            waiter_ledger.execute(
                waiter_request,
                Effect::Write,
                "instance-a",
                Duration::from_secs(2),
                |request| response(&request.id),
            )
        });
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        std::thread::sleep(Duration::from_millis(75));
        ledger
            .abandon(&request.id, &request_hash, "instance-a")
            .unwrap();

        let execution = waiter.join().unwrap();
        assert_eq!(execution.response.status, Status::Ok);
        assert_ne!(
            execution
                .response
                .error
                .as_ref()
                .and_then(|error| error.details.as_ref())
                .and_then(|details| details.get("kind"))
                .and_then(serde_json::Value::as_str),
            Some("daemon.request_outcome_lookup_failed")
        );
    }

    #[test]
    fn abandoning_missing_reservation_reports_ledger_error() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ledger = RequestOutcomeLedger::open(temp.path().join(DAEMON_OUTCOME_DB_NAME)).unwrap();

        let error = ledger
            .abandon("missing-request", "missing-hash", "instance-a")
            .expect_err("missing reservation must not be reported as abandoned");
        assert!(error.to_string().contains("disappeared before abandonment"));
    }

    #[test]
    fn concurrent_preparation_probes_do_not_stall_in_sqlite_connection_churn() {
        const WORKERS: usize = 32;
        const PROBES_PER_WORKER: usize = 100;

        let temp = tempfile::tempdir().expect("tempdir");
        let ledger = Arc::new(
            RequestOutcomeLedger::open(temp.path().join(DAEMON_OUTCOME_DB_NAME))
                .expect("open ledger"),
        );
        let start = Arc::new(Barrier::new(WORKERS + 1));
        let (completed_tx, completed_rx) = mpsc::channel();
        let mut workers = Vec::with_capacity(WORKERS);

        for worker in 0..WORKERS {
            let ledger = Arc::clone(&ledger);
            let start = Arc::clone(&start);
            let completed_tx = completed_tx.clone();
            workers.push(std::thread::spawn(move || {
                start.wait();
                for probe in 0..PROBES_PER_WORKER {
                    let request_id = format!("probe-{worker}-{probe}");
                    let request = request(&request_id, "task-a");
                    assert!(
                        ledger
                            .terminal_outcome_before_preparation(&request)
                            .expect("terminal outcome probe")
                            .is_none()
                    );
                    assert!(
                        ledger
                            .reserved_request_recovery_before_preparation(&request)
                            .expect("reserved recovery probe")
                            .is_none()
                    );
                }
                completed_tx.send(worker).expect("report completion");
            }));
        }
        drop(completed_tx);
        start.wait();

        let completion_deadline = std::time::Instant::now() + Duration::from_secs(30);
        for _ in 0..WORKERS {
            completed_rx
                .recv_timeout(
                    completion_deadline.saturating_duration_since(std::time::Instant::now()),
                )
                .expect("all concurrent preparation probes must remain bounded");
        }
        for worker in workers {
            worker.join().expect("probe worker");
        }
    }

    fn error_response(
        id: &str,
        code: ErrorCode,
        details: Option<serde_json::Value>,
    ) -> ResponseEnvelope {
        ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            id: id.to_string(),
            status: Status::Error,
            result: None,
            error: Some(ErrorBody {
                code,
                message: "request failed".to_string(),
                details,
            }),
            ticket: None,
            steering: None,
            reminders: None,
            display: None,
            preview: None,
            effect: Some(Effect::Write),
            trace: None,
        }
    }

    fn insert_epoch(db_path: &Path, text_id: &str) {
        SqliteWriter::open(db_path)
            .expect("open request writer")
            .add_epoch(text_id, None, &[])
            .expect("insert epoch");
    }

    fn epoch_count(db_path: &Path) -> i64 {
        open_database(db_path)
            .expect("open project database")
            .connection()
            .query_row("SELECT COUNT(*) FROM epochs_data", [], |row| row.get(0))
            .expect("count epochs")
    }

    fn atomic_outcome_count(db_path: &Path) -> i64 {
        open_database(db_path)
            .expect("open project database")
            .connection()
            .query_row("SELECT COUNT(*) FROM atomic_request_outcomes", [], |row| {
                row.get(0)
            })
            .expect("count atomic outcomes")
    }

    fn atomic_outcome_exists(db_path: &Path, request_id: &str) -> bool {
        open_database(db_path)
            .expect("open project database")
            .connection()
            .query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM atomic_request_outcomes WHERE request_id = ?1
                 )",
                [request_id],
                |row| row.get(0),
            )
            .expect("check atomic outcome")
    }

    fn runtime_reservation(
        ledger: &RequestOutcomeLedger,
        request_id: &str,
    ) -> Option<(String, bool)> {
        Connection::open(ledger.path())
            .expect("open runtime ledger")
            .query_row(
                "SELECT instance_id, response_json IS NOT NULL
                 FROM daemon_request_outcomes WHERE request_id = ?1",
                [request_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .expect("read runtime reservation")
    }

    #[test]
    fn completed_outcome_replays_without_executing_twice() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join(DAEMON_OUTCOME_DB_NAME);
        let ledger = RequestOutcomeLedger::open(&path).expect("open ledger");
        let executions = Cell::new(0);

        let first = ledger.execute(
            request("request-1", "task-a"),
            Effect::Write,
            "instance-a",
            Duration::ZERO,
            |request| {
                executions.set(executions.get() + 1);
                response(&request.id)
            },
        );
        assert!(!first.replayed);

        let reopened = RequestOutcomeLedger::open(&path).expect("reopen ledger");
        let replay = reopened.execute(
            request("request-1", "task-a"),
            Effect::Write,
            "instance-b",
            Duration::ZERO,
            |_| {
                executions.set(executions.get() + 1);
                response("request-1")
            },
        );

        assert!(replay.replayed);
        assert_eq!(replay.response.status, Status::Ok);
        assert_eq!(replay.response.result, first.response.result);
        assert_eq!(executions.get(), 1);
    }

    #[test]
    fn terminal_runtime_outcome_replays_after_issuing_workspace_is_removed() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ledger = RequestOutcomeLedger::open(temp.path().join(DAEMON_OUTCOME_DB_NAME))
            .expect("open ledger");
        let workspace = temp.path().join("linked-worktree");
        std::fs::create_dir(&workspace).expect("create issuing workspace");
        let mut request = request("request-removed-runtime-workspace", "task-a");
        request.workspace_root = Some(workspace.clone());
        let first = ledger.execute(
            request.clone(),
            Effect::Write,
            "instance-a",
            Duration::ZERO,
            |request| response(&request.id),
        );
        std::fs::remove_dir(&workspace).expect("remove issuing workspace");

        let replay = ledger
            .terminal_outcome_before_preparation(&request)
            .expect("probe terminal runtime outcome")
            .expect("completed runtime outcome");

        assert!(replay.replayed);
        assert_eq!(replay.response.result, first.response.result);
    }

    #[test]
    fn dynamic_effect_runtime_outcome_replays_after_issuing_workspace_is_removed() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ledger = RequestOutcomeLedger::open(temp.path().join(DAEMON_OUTCOME_DB_NAME))
            .expect("open ledger");
        let workspace = temp.path().join("linked-worktree");
        std::fs::create_dir(&workspace).expect("create issuing workspace");
        let mut request = request("request-removed-dynamic-workspace", "task-a");
        let Op::Call(params) = &mut request.op else {
            unreachable!("test request is a call");
        };
        params.address = Address::Operation {
            path: vec!["dogfood".to_string(), "repair".to_string()],
        };
        params.input = serde_json::json!({ "apply": true });
        request.workspace_root = Some(workspace.clone());
        let first = ledger.execute(
            request.clone(),
            Effect::Exec,
            "instance-a",
            Duration::ZERO,
            |request| response(&request.id),
        );
        std::fs::remove_dir(&workspace).expect("remove issuing workspace");

        let replay = ledger
            .terminal_outcome_before_preparation(&request)
            .expect("probe dynamic terminal runtime outcome")
            .expect("completed dynamic runtime outcome");

        assert!(replay.replayed);
        assert_eq!(replay.response.result, first.response.result);
    }

    #[test]
    fn terminal_runtime_outcome_replay_does_not_require_registered_command() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ledger = RequestOutcomeLedger::open(temp.path().join(DAEMON_OUTCOME_DB_NAME))
            .expect("open ledger");
        let mut request = request("request-retired-command", "task-a");
        let Op::Call(params) = &mut request.op else {
            unreachable!("test request is a call");
        };
        params.address = Address::Operation {
            path: vec!["retired".to_string(), "command".to_string()],
        };
        request.workspace_root = Some(temp.path().join("removed-worktree"));
        let first = ledger.execute(
            request.clone(),
            Effect::Exec,
            "instance-a",
            Duration::ZERO,
            |request| response(&request.id),
        );

        assert!(resolved_request_recovery(temp.path(), &request).is_none());
        let replay = ledger
            .terminal_outcome_before_preparation(&request)
            .expect("probe retired command outcome")
            .expect("completed retired command outcome");

        assert!(replay.replayed);
        assert_eq!(replay.response.result, first.response.result);
    }

    #[test]
    fn terminal_atomic_request_id_conflict_has_no_committed_effect() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("exo.db");
        drop(open_database(&db_path).expect("initialize project database"));
        let ledger = RequestOutcomeLedger::open(temp.path().join(DAEMON_OUTCOME_DB_NAME))
            .expect("open ledger");
        let original = request("request-terminal-atomic-conflict", "task-a");
        let first = ledger.execute_atomic_project_state(
            original,
            Effect::Write,
            "instance-a",
            Duration::ZERO,
            &db_path,
            |request| response(&request.id),
            Ok,
        );
        assert_eq!(first.response.status, Status::Ok);

        let conflict = ledger
            .terminal_outcome_before_preparation(&request(
                "request-terminal-atomic-conflict",
                "task-b",
            ))
            .expect("probe terminal atomic conflict")
            .expect("terminal conflict response");

        assert!(!conflict.replayed);
        assert_eq!(conflict.response.status, Status::Error);
        assert_eq!(conflict.response.effect, None);
        assert_eq!(
            conflict
                .response
                .error
                .as_ref()
                .and_then(|error| error.details.as_ref())
                .and_then(|details| details.get("mutation_performed")),
            Some(&serde_json::json!(false))
        );
    }

    #[test]
    fn terminal_legacy_atomic_request_id_conflict_has_no_committed_effect() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("exo.db");
        drop(open_database(&db_path).expect("initialize project database"));
        let ledger = RequestOutcomeLedger::open(temp.path().join(DAEMON_OUTCOME_DB_NAME))
            .expect("open ledger");
        let original = request("request-terminal-legacy-atomic-conflict", "task-a");
        let first = ledger.execute_atomic_project_state(
            original,
            Effect::Write,
            "instance-a",
            Duration::ZERO,
            &db_path,
            |request| response(&request.id),
            Ok,
        );
        assert_eq!(first.response.status, Status::Ok);
        Connection::open(ledger.path())
            .expect("open runtime outcome ledger")
            .execute(
                "UPDATE daemon_request_outcomes
                 SET recovery_class = NULL
                 WHERE request_id = 'request-terminal-legacy-atomic-conflict'",
                [],
            )
            .expect("simulate a migrated completed atomic outcome");

        let conflict = ledger
            .terminal_outcome_before_preparation(&request(
                "request-terminal-legacy-atomic-conflict",
                "task-b",
            ))
            .expect("probe legacy terminal atomic conflict")
            .expect("legacy terminal conflict response");

        assert!(!conflict.replayed);
        assert_eq!(conflict.response.status, Status::Error);
        assert_eq!(conflict.response.effect, None);
        assert_eq!(
            conflict
                .response
                .error
                .as_ref()
                .and_then(|error| error.details.as_ref())
                .and_then(|details| details.get("mutation_performed")),
            Some(&serde_json::json!(false))
        );
    }

    #[test]
    fn request_id_reuse_with_different_payload_is_rejected() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ledger = RequestOutcomeLedger::open(temp.path().join(DAEMON_OUTCOME_DB_NAME))
            .expect("open ledger");
        let executions = Cell::new(0);
        let _ = ledger.execute(
            request("request-1", "task-a"),
            Effect::Write,
            "instance-a",
            Duration::ZERO,
            |request| {
                executions.set(executions.get() + 1);
                response(&request.id)
            },
        );

        let conflict = ledger.execute(
            request("request-1", "task-b"),
            Effect::Write,
            "instance-a",
            Duration::ZERO,
            |_| {
                executions.set(executions.get() + 1);
                response("request-1")
            },
        );

        assert_eq!(conflict.response.status, Status::Error);
        assert_eq!(
            conflict.response.error.as_ref().map(|error| error.code),
            Some(ErrorCode::InvalidInput)
        );
        assert_eq!(executions.get(), 1);
    }

    #[test]
    fn canonical_request_id_conflict_does_not_mask_the_original_outcome() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("exo.db");
        drop(open_database(&db_path).expect("initialize project database"));
        let ledger = RequestOutcomeLedger::open(temp.path().join(DAEMON_OUTCOME_DB_NAME))
            .expect("open ledger");
        let original = request("request-canonical-conflict", "task-a");
        let original_hash = request_hash(&original).expect("original request hash");
        execute_atomic_core(
            &db_path,
            &original_hash,
            Effect::Write,
            original.clone(),
            |request| response(&request.id),
            || Ok(()),
        )
        .expect("commit original canonical outcome");

        let conflict = ledger.execute_atomic_project_state(
            request("request-canonical-conflict", "task-b"),
            Effect::Write,
            "instance-b",
            Duration::ZERO,
            &db_path,
            |request| response(&request.id),
            Ok,
        );
        assert_eq!(conflict.response.status, Status::Error);
        assert_eq!(
            conflict.response.error.as_ref().map(|error| error.code),
            Some(ErrorCode::InvalidInput)
        );
        assert_eq!(
            runtime_reservation(&ledger, "request-canonical-conflict"),
            None,
            "canonical conflict must abandon the newly inserted runtime reservation"
        );

        let executions = Cell::new(0);
        let replay = ledger.execute_atomic_project_state(
            original,
            Effect::Write,
            "instance-c",
            Duration::ZERO,
            &db_path,
            |request| {
                executions.set(executions.get() + 1);
                response(&request.id)
            },
            Ok,
        );
        assert!(replay.replayed);
        assert_eq!(replay.response.status, Status::Ok);
        assert_eq!(executions.get(), 0);
    }

    #[test]
    fn unfinished_previous_instance_is_not_reexecuted() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ledger = RequestOutcomeLedger::open(temp.path().join(DAEMON_OUTCOME_DB_NAME))
            .expect("open ledger");
        let request = request("request-1", "task-a");
        let hash = request_hash(&request).expect("request hash");
        assert!(matches!(
            ledger
                .reserve(
                    &request.id,
                    &hash,
                    Effect::Exec,
                    RecoveryClass::ExternalAtMostOnce,
                    "instance-a",
                )
                .expect("reserve"),
            Reservation::Execute
        ));

        let executions = Cell::new(0);
        let result = ledger.execute(request, Effect::Exec, "instance-b", Duration::ZERO, |_| {
            executions.set(executions.get() + 1);
            response("request-1")
        });

        assert_eq!(executions.get(), 0);
        assert_eq!(result.response.status, Status::Error);
        assert_eq!(
            result.response.error.as_ref().and_then(|error| {
                error
                    .details
                    .as_ref()
                    .and_then(|details| details["kind"].as_str())
            }),
            Some("daemon.request_outcome_indeterminate")
        );
    }

    #[test]
    fn legacy_in_flight_atomic_request_remains_indeterminate() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("exo.db");
        drop(open_database(&db_path).expect("initialize project database"));
        let ledger_path = temp.path().join(DAEMON_OUTCOME_DB_NAME);
        let request = request("request-legacy-in-flight", "task-a");
        let hash = request_hash(&request).expect("request hash");
        let legacy = Connection::open(&ledger_path).expect("open legacy runtime ledger");
        legacy
            .execute_batch(
                "CREATE TABLE daemon_request_outcomes (
                     request_id TEXT PRIMARY KEY,
                     request_hash TEXT NOT NULL,
                     effect TEXT NOT NULL,
                     instance_id TEXT NOT NULL,
                     response_json TEXT,
                     started_at INTEGER NOT NULL,
                     completed_at INTEGER
                 );",
            )
            .expect("create legacy runtime schema");
        legacy
            .execute(
                "INSERT INTO daemon_request_outcomes (
                     request_id, request_hash, effect, instance_id, started_at
                 ) VALUES (?1, ?2, 'write', 'instance-old', ?3)",
                params![request.id, hash, now_timestamp()],
            )
            .expect("insert legacy in-flight reservation");
        drop(legacy);

        let ledger = RequestOutcomeLedger::open(&ledger_path).expect("upgrade runtime ledger");
        assert!(
            !ledger
                .atomic_request_needs_preparation(&request, &db_path, "instance-new")
                .expect("probe legacy reservation"),
            "legacy in-flight reservations must return indeterminate before preparation"
        );
        let executions = Cell::new(0);
        let result = ledger.execute_atomic_project_state(
            request,
            Effect::Write,
            "instance-new",
            Duration::ZERO,
            &db_path,
            |request| {
                executions.set(executions.get() + 1);
                response(&request.id)
            },
            Ok,
        );

        assert_eq!(executions.get(), 0);
        assert_eq!(result.response.status, Status::Error);
        assert_eq!(
            result.response.error.as_ref().and_then(|error| {
                error
                    .details
                    .as_ref()
                    .and_then(|details| details["kind"].as_str())
            }),
            Some("daemon.request_outcome_indeterminate")
        );
        assert_eq!(atomic_outcome_count(&db_path), 0);
    }

    #[test]
    fn same_instance_atomic_retry_remains_pending_without_db_contention() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("exo.db");
        drop(open_database(&db_path).expect("initialize project database"));
        let ledger = RequestOutcomeLedger::open(temp.path().join(DAEMON_OUTCOME_DB_NAME))
            .expect("open ledger");
        let request = request("request-atomic-in-flight", "task-a");
        let hash = request_hash(&request).expect("request hash");
        assert!(matches!(
            ledger
                .reserve(
                    &request.id,
                    &hash,
                    Effect::Write,
                    RecoveryClass::AtomicProjectState,
                    "instance-a",
                )
                .expect("reserve active request"),
            Reservation::Execute
        ));

        let executions = Cell::new(0);
        let result = ledger.execute_atomic_project_state(
            request,
            Effect::Write,
            "instance-a",
            Duration::ZERO,
            &db_path,
            |request| {
                executions.set(executions.get() + 1);
                response(&request.id)
            },
            Ok,
        );

        assert_eq!(executions.get(), 0);
        assert_eq!(result.response.status, Status::Error);
        assert_eq!(
            result.response.error.as_ref().and_then(|error| {
                error
                    .details
                    .as_ref()
                    .and_then(|details| details["kind"].as_str())
            }),
            Some("daemon.request_outcome_pending")
        );
        assert_eq!(
            runtime_reservation(&ledger, "request-atomic-in-flight"),
            Some(("instance-a".to_string(), false))
        );
    }

    #[test]
    fn same_instance_atomic_retry_replays_visible_canonical_outcome() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("exo.db");
        drop(open_database(&db_path).expect("initialize project database"));
        let ledger = RequestOutcomeLedger::open(temp.path().join(DAEMON_OUTCOME_DB_NAME))
            .expect("open ledger");
        let request = request("request-atomic-visible", "task-a");
        let hash = request_hash(&request).expect("request hash");
        assert!(matches!(
            ledger
                .reserve(
                    &request.id,
                    &hash,
                    Effect::Write,
                    RecoveryClass::AtomicProjectState,
                    "instance-a",
                )
                .expect("reserve active request"),
            Reservation::Execute
        ));
        execute_atomic_core(
            &db_path,
            &hash,
            Effect::Write,
            request.clone(),
            |request| response(&request.id),
            || Ok(()),
        )
        .expect("commit canonical outcome");

        let executions = Cell::new(0);
        let result = ledger.execute_atomic_project_state(
            request,
            Effect::Write,
            "instance-a",
            Duration::ZERO,
            &db_path,
            |request| {
                executions.set(executions.get() + 1);
                response(&request.id)
            },
            Ok,
        );

        assert!(result.replayed);
        assert_eq!(result.response.status, Status::Ok);
        assert_eq!(executions.get(), 0);
    }

    #[test]
    fn failed_atomic_recovery_preserves_another_instances_reservation() {
        let temp = tempfile::tempdir().expect("tempdir");
        let invalid_db_path = temp.path().join("database-directory");
        std::fs::create_dir(&invalid_db_path).expect("create invalid database path");
        let ledger = RequestOutcomeLedger::open(temp.path().join(DAEMON_OUTCOME_DB_NAME))
            .expect("open ledger");
        let request = request("request-atomic-recovery", "task-a");
        let hash = request_hash(&request).expect("request hash");
        assert!(matches!(
            ledger
                .reserve(
                    &request.id,
                    &hash,
                    Effect::Write,
                    RecoveryClass::AtomicProjectState,
                    "instance-a",
                )
                .expect("reserve previous request"),
            Reservation::Execute
        ));

        let result = ledger.execute_atomic_project_state(
            request,
            Effect::Write,
            "instance-b",
            Duration::ZERO,
            &invalid_db_path,
            |request| response(&request.id),
            Ok,
        );

        assert_eq!(result.response.status, Status::Error);
        assert_eq!(
            result.response.error.as_ref().and_then(|error| {
                error
                    .details
                    .as_ref()
                    .and_then(|details| details["kind"].as_str())
            }),
            Some("daemon.atomic_request_commit_failed")
        );
        assert_eq!(
            runtime_reservation(&ledger, "request-atomic-recovery"),
            Some(("instance-a".to_string(), false)),
            "recovery failure must not delete another instance's reservation"
        );
    }

    #[test]
    fn atomic_request_rolls_back_state_and_outcome_before_commit() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("exo.db");
        drop(open_database(&db_path).expect("initialize project database"));
        let request = request("request-atomic-rollback", "task-a");
        let hash = request_hash(&request).expect("request hash");

        let result = execute_atomic_core(
            &db_path,
            &hash,
            Effect::Write,
            request,
            |request| {
                insert_epoch(&db_path, "epoch-before-crash");
                response(&request.id)
            },
            || Err(anyhow!("failpoint before commit")),
        );

        assert!(result.is_err());
        assert_eq!(epoch_count(&db_path), 0);
        assert_eq!(atomic_outcome_count(&db_path), 0);
    }

    #[test]
    fn atomic_preparation_probe_distinguishes_replay_pending_and_execution() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("exo.db");
        drop(open_database(&db_path).expect("initialize project database"));
        let ledger = RequestOutcomeLedger::open(temp.path().join(DAEMON_OUTCOME_DB_NAME))
            .expect("open runtime ledger");

        let runtime_request = request("request-runtime-terminal", "task-a");
        let runtime_outcome = ledger.execute(
            runtime_request.clone(),
            Effect::Write,
            "instance-a",
            Duration::ZERO,
            |request| response(&request.id),
        );
        assert_eq!(runtime_outcome.response.status, Status::Ok);
        assert!(
            !ledger
                .atomic_request_needs_preparation(&runtime_request, &db_path, "instance-a")
                .expect("probe runtime outcome"),
            "completed runtime outcome should replay before preparation"
        );

        let incomplete_request = request("request-runtime-incomplete", "task-a");
        let incomplete_hash = request_hash(&incomplete_request).expect("request hash");
        assert!(matches!(
            ledger
                .reserve(
                    &incomplete_request.id,
                    &incomplete_hash,
                    Effect::Write,
                    RecoveryClass::AtomicProjectState,
                    "instance-a",
                )
                .expect("reserve incomplete request"),
            Reservation::Execute
        ));
        assert!(
            !ledger
                .atomic_request_needs_preparation(&incomplete_request, &db_path, "instance-a")
                .expect("probe same-instance outcome"),
            "same-instance in-flight request should return pending before preparation"
        );
        assert!(
            ledger
                .atomic_request_needs_preparation(&incomplete_request, &db_path, "instance-b")
                .expect("probe previous-instance outcome"),
            "previous-instance request without a canonical outcome may need recovery execution"
        );

        let canonical_request = request("request-canonical-terminal", "task-a");
        let canonical_hash = request_hash(&canonical_request).expect("request hash");
        execute_atomic_core(
            &db_path,
            &canonical_hash,
            Effect::Write,
            canonical_request.clone(),
            |request| response(&request.id),
            || Ok(()),
        )
        .expect("commit canonical outcome");
        assert!(
            !ledger
                .atomic_request_needs_preparation(&canonical_request, &db_path, "instance-b")
                .expect("probe canonical outcome"),
            "canonical outcome should replay before preparation"
        );

        let missing_request = request("request-missing", "task-a");
        assert!(
            ledger
                .atomic_request_needs_preparation(&missing_request, &db_path, "instance-a")
                .expect("probe missing outcome"),
            "new requests require current project preparation"
        );
    }

    #[test]
    fn canonical_pruning_preserves_outcomes_with_unresolved_runtime_references() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("exo.db");
        drop(open_database(&db_path).expect("initialize project database"));
        let ledger = RequestOutcomeLedger::open(temp.path().join(DAEMON_OUTCOME_DB_NAME))
            .expect("open runtime ledger");

        let protected = request("request-expired-protected", "task-a");
        let protected_hash = request_hash(&protected).expect("protected request hash");
        execute_atomic_core(
            &db_path,
            &protected_hash,
            Effect::Write,
            protected.clone(),
            |request| response(&request.id),
            || Ok(()),
        )
        .expect("commit protected canonical outcome");
        assert!(matches!(
            ledger
                .reserve(
                    &protected.id,
                    &protected_hash,
                    Effect::Write,
                    RecoveryClass::AtomicProjectState,
                    "instance-old",
                )
                .expect("reserve unresolved runtime reference"),
            Reservation::Execute
        ));

        let unreferenced = request("request-expired-unreferenced", "task-a");
        let unreferenced_hash = request_hash(&unreferenced).expect("unreferenced request hash");
        execute_atomic_core(
            &db_path,
            &unreferenced_hash,
            Effect::Write,
            unreferenced,
            |request| response(&request.id),
            || Ok(()),
        )
        .expect("commit unreferenced canonical outcome");
        open_database(&db_path)
            .expect("open project database")
            .connection()
            .execute(
                "UPDATE atomic_request_outcomes SET committed_at = ?1
                 WHERE request_id IN ('request-expired-protected', 'request-expired-unreferenced')",
                [now_timestamp() - COMPLETED_OUTCOME_RETENTION_SECS - 1],
            )
            .expect("expire canonical outcomes");

        let trigger = ledger.execute_atomic_project_state(
            request("request-prune-trigger", "task-a"),
            Effect::Write,
            "instance-current",
            Duration::ZERO,
            &db_path,
            |request| response(&request.id),
            Ok,
        );
        assert_eq!(trigger.response.status, Status::Ok);
        assert!(atomic_outcome_exists(&db_path, "request-expired-protected"));
        assert!(
            !atomic_outcome_exists(&db_path, "request-expired-unreferenced"),
            "expired canonical outcomes without unresolved references should still prune"
        );
    }

    #[test]
    fn previous_daemon_recovers_canonical_outcome_after_atomic_commit() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("exo.db");
        drop(open_database(&db_path).expect("initialize project database"));
        let ledger = RequestOutcomeLedger::open(temp.path().join(DAEMON_OUTCOME_DB_NAME))
            .expect("open runtime ledger");
        let request = request("request-atomic-recovery", "task-a");
        let hash = request_hash(&request).expect("request hash");
        assert!(matches!(
            ledger
                .reserve(
                    &request.id,
                    &hash,
                    Effect::Write,
                    RecoveryClass::AtomicProjectState,
                    "instance-a",
                )
                .expect("reserve runtime request"),
            Reservation::Execute
        ));

        let committed = execute_atomic_core(
            &db_path,
            &hash,
            Effect::Write,
            request.clone(),
            |request| {
                insert_epoch(&db_path, "epoch-committed");
                response(&request.id)
            },
            || Ok(()),
        )
        .expect("commit canonical state and response");
        assert!(committed.committed);
        assert_eq!(epoch_count(&db_path), 1);

        let executions = Cell::new(0);
        let recovered = ledger.execute_atomic_project_state(
            request,
            Effect::Write,
            "instance-b",
            Duration::ZERO,
            &db_path,
            |_| {
                executions.set(executions.get() + 1);
                response("request-atomic-recovery")
            },
            Ok,
        );

        assert!(recovered.replayed);
        assert_eq!(recovered.response.status, Status::Ok);
        assert_eq!(executions.get(), 0);
        assert_eq!(epoch_count(&db_path), 1);
    }

    #[test]
    fn finalization_failure_keeps_atomic_request_recoverable() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("exo.db");
        drop(open_database(&db_path).expect("initialize project database"));
        let ledger = RequestOutcomeLedger::open(temp.path().join(DAEMON_OUTCOME_DB_NAME))
            .expect("open runtime ledger");
        let request = request("request-finalization-retry", "task-a");
        let executions = Cell::new(0);
        let finalizations = Cell::new(0);

        let first = ledger.execute_atomic_project_state(
            request.clone(),
            Effect::Write,
            "instance-a",
            Duration::ZERO,
            &db_path,
            |request| {
                executions.set(executions.get() + 1);
                insert_epoch(&db_path, "epoch-finalize");
                response(&request.id)
            },
            |response| {
                finalizations.set(finalizations.get() + 1);
                Err(error_response(
                    &response.id,
                    ErrorCode::PreconditionFailed,
                    Some(serde_json::json!({ "kind": "test.finalization" })),
                ))
            },
        );
        assert_eq!(first.response.status, Status::Error);
        assert_eq!(first.response.effect, Some(Effect::Write));

        let second = ledger.execute_atomic_project_state(
            request,
            Effect::Write,
            "instance-a",
            Duration::ZERO,
            &db_path,
            |_| {
                executions.set(executions.get() + 1);
                response("request-finalization-retry")
            },
            |response| {
                finalizations.set(finalizations.get() + 1);
                Ok(response)
            },
        );

        assert!(second.replayed);
        assert_eq!(second.response.status, Status::Ok);
        assert_eq!(executions.get(), 1);
        assert_eq!(finalizations.get(), 2);
        assert_eq!(epoch_count(&db_path), 1);
    }

    #[test]
    fn atomic_request_uses_canonical_outcome_when_runtime_ledger_is_unavailable() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("exo.db");
        drop(open_database(&db_path).expect("initialize project database"));
        let unusable_ledger_path = temp.path().join("runtime-ledger-directory");
        std::fs::create_dir(&unusable_ledger_path).expect("create unusable ledger path");
        let ledger = RequestOutcomeLedger {
            path: unusable_ledger_path,
            notifications: Arc::new(OutcomeNotifications::default()),
        };
        let request = request("request-without-runtime-ledger", "task-a");
        assert!(
            ledger
                .atomic_request_needs_preparation(&request, &db_path, "instance-a")
                .expect("canonical database should authorize preparation"),
            "runtime lookup failure must not block canonical atomic execution"
        );

        let execution = ledger.execute_atomic_project_state(
            request,
            Effect::Write,
            "instance-a",
            Duration::ZERO,
            &db_path,
            |request| {
                insert_epoch(&db_path, "epoch-without-runtime-ledger");
                response(&request.id)
            },
            Ok,
        );

        assert_eq!(execution.response.status, Status::Ok);
        assert_eq!(epoch_count(&db_path), 1);
        assert_eq!(atomic_outcome_count(&db_path), 1);
    }

    #[test]
    fn completion_review_precondition_commits_stateful_response() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("exo.db");
        drop(open_database(&db_path).expect("initialize project database"));
        let request = request("request-review", "task-a");
        let hash = request_hash(&request).expect("request hash");

        let execution = execute_atomic_core(
            &db_path,
            &hash,
            Effect::Write,
            request,
            |request| {
                insert_epoch(&db_path, "epoch-review");
                error_response(
                    &request.id,
                    ErrorCode::PreconditionFailed,
                    Some(serde_json::json!({
                        "details": {
                            "workflow_confirmation": {
                                "kind": "workflow_completion_confirmation",
                                "evidence_recorded": true
                            }
                        }
                    })),
                )
            },
            || Ok(()),
        )
        .expect("commit stateful review response");

        assert!(execution.committed);
        assert_eq!(epoch_count(&db_path), 1);
        assert_eq!(atomic_outcome_count(&db_path), 1);
    }

    #[test]
    fn completion_review_prompt_without_evidence_rolls_back() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("exo.db");
        drop(open_database(&db_path).expect("initialize project database"));
        let request = request("request-review-prompt", "task-a");
        let hash = request_hash(&request).expect("request hash");

        let execution = execute_atomic_core(
            &db_path,
            &hash,
            Effect::Write,
            request,
            |request| {
                insert_epoch(&db_path, "epoch-review-prompt");
                error_response(
                    &request.id,
                    ErrorCode::PreconditionFailed,
                    Some(serde_json::json!({
                        "details": {
                            "workflow_confirmation": {
                                "kind": "workflow_completion_confirmation",
                                "evidence_recorded": false
                            }
                        }
                    })),
                )
            },
            || Ok(()),
        )
        .expect("return approval prompt without committing state");

        assert!(!execution.committed);
        assert_eq!(execution.response.effect, None);
        assert_eq!(epoch_count(&db_path), 0);
        assert_eq!(atomic_outcome_count(&db_path), 0);
    }

    #[test]
    fn ordinary_error_rolls_back_atomic_state() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("exo.db");
        drop(open_database(&db_path).expect("initialize project database"));
        let request = request("request-error", "task-a");
        let hash = request_hash(&request).expect("request hash");

        let execution = execute_atomic_core(
            &db_path,
            &hash,
            Effect::Write,
            request,
            |request| {
                insert_epoch(&db_path, "epoch-error");
                error_response(&request.id, ErrorCode::InvalidInput, None)
            },
            || Ok(()),
        )
        .expect("return ordinary command error");

        assert!(!execution.committed);
        assert_eq!(execution.response.effect, None);
        assert_eq!(epoch_count(&db_path), 0);
        assert_eq!(atomic_outcome_count(&db_path), 0);
    }

    #[test]
    fn resolved_effect_comes_from_built_command() {
        let mut status = request("request-status", "task-a");
        let Op::Call(params) = &mut status.op else {
            unreachable!("test request is a call");
        };
        params.address = Address::Operation {
            path: vec!["status".to_string()],
        };
        params.input = serde_json::json!({});

        assert_eq!(
            resolved_request_effect(Path::new("."), &request("request-1", "task-a")),
            Some(Effect::Write)
        );
    }

    #[test]
    fn resolved_effect_honors_argument_dependent_exec_commands() {
        let mut apply = request("request-1", "task-a");
        {
            let Op::Call(params) = &mut apply.op else {
                unreachable!("test request is a call");
            };
            params.address = Address::Operation {
                path: vec!["dogfood".to_string(), "repair".to_string()],
            };
            params.input = serde_json::json!({ "apply": true });
        }

        assert_eq!(
            resolved_request_effect(Path::new("."), &apply),
            Some(Effect::Exec)
        );

        let Op::Call(params) = &mut apply.op else {
            unreachable!("test request is a call");
        };
        params.input = serde_json::json!({ "apply": false });
        assert_eq!(
            resolved_request_effect(Path::new("."), &apply),
            Some(Effect::Pure)
        );
    }

    #[test]
    fn resolved_recovery_class_separates_atomic_and_external_commands() {
        let mut epoch_add = request("request-epoch", "task-a");
        let Op::Call(params) = &mut epoch_add.op else {
            unreachable!("test request is a call");
        };
        params.address = Address::Operation {
            path: vec!["epoch".to_string(), "add".to_string()],
        };
        params.input = serde_json::json!({ "title": "Atomic Epoch" });
        assert_eq!(
            resolved_request_recovery(Path::new("."), &epoch_add),
            Some(ResolvedRequestRecovery {
                effect: Effect::Write,
                recovery_class: RecoveryClass::AtomicProjectState,
            })
        );

        let mut phase_finish = request("request-phase", "task-a");
        let Op::Call(params) = &mut phase_finish.op else {
            unreachable!("test request is a call");
        };
        params.address = Address::Operation {
            path: vec!["phase".to_string(), "finish".to_string()],
        };
        params.input = serde_json::json!({ "message": "Finish phase" });
        assert_eq!(
            resolved_request_recovery(Path::new("."), &phase_finish),
            Some(ResolvedRequestRecovery {
                effect: Effect::Write,
                recovery_class: RecoveryClass::ExternalAtMostOnce,
            })
        );
    }
}
