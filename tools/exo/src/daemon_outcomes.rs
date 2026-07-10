//! Durable daemon request outcomes for transparent mutation recovery.
//!
//! Mutating requests are reserved before command dispatch and their complete
//! response is persisted before the daemon writes to the client socket. A
//! reconnecting client can therefore resend the same request envelope without
//! executing the command a second time.

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
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const DAEMON_OUTCOME_DB_NAME: &str = "daemon-outcomes.sqlite3";
const COMPLETED_OUTCOME_RETENTION_SECS: i64 = 7 * 24 * 60 * 60;

#[derive(Debug, Clone)]
pub struct RequestOutcomeLedger {
    path: PathBuf,
}

#[derive(Debug)]
enum Reservation {
    Execute,
    Replay(Box<ResponseEnvelope>),
    InFlight {
        instance_id: String,
        recovery_class: Option<RecoveryClass>,
    },
    Conflict,
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

impl RequestOutcomeLedger {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let ledger = Self { path: path.into() };
        if let Some(parent) = ledger.path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!(
                    "create daemon outcome ledger directory {}",
                    parent.display()
                )
            })?;
        }
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
        ledger.prune_completed(&connection)?;
        Ok(ledger)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Return a completed response or request-ID conflict before request
    /// preparation. Replays remain available when the workspace that issued
    /// the original request has since been removed.
    pub(crate) fn terminal_outcome_before_preparation(
        &self,
        request: &RequestEnvelope,
        project_db_path: &Path,
    ) -> Result<Option<OutcomeExecution>> {
        let request_hash = request_hash(request)?;
        let runtime_outcome = self.connection().and_then(|connection| {
            connection
                .query_row(
                    "SELECT request_hash, effect, response_json
             FROM daemon_request_outcomes
             WHERE request_id = ?1",
                    [&request.id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<String>>(2)?,
                        ))
                    },
                )
                .optional()
                .map_err(Into::into)
        });

        if let Ok(Some((stored_hash, effect, response_json))) = &runtime_outcome {
            let effect = effect_from_name(&effect)?;
            if stored_hash != &request_hash {
                return Ok(Some(OutcomeExecution {
                    response: request_id_conflict_response(request.id.clone(), effect),
                    replayed: false,
                }));
            }
            if let Some(response_json) = response_json {
                return Ok(Some(OutcomeExecution {
                    response: serde_json::from_str(response_json)
                        .context("deserialize recorded daemon response")?,
                    replayed: true,
                }));
            }
        }

        let canonical_outcome =
            canonical_atomic_terminal_outcome(project_db_path, request, &request_hash)?;
        if canonical_outcome.is_some() {
            return Ok(canonical_outcome);
        }
        runtime_outcome?;
        Ok(None)
    }

    /// Return whether an atomic request may execute and therefore needs current
    /// project preparation. Completed, conflicting, and same-instance in-flight
    /// requests are resolved by the outcome ledger before mutable preparation.
    pub(crate) fn atomic_request_needs_preparation(
        &self,
        request: &RequestEnvelope,
        project_db_path: &Path,
        instance_id: &str,
    ) -> Result<bool> {
        let request_hash = request_hash(request)?;
        let runtime_outcome = self.runtime_outcome_state(&request.id, &request_hash);
        if matches!(runtime_outcome, Ok(RuntimeOutcomeState::Terminal))
            || matches!(
                &runtime_outcome,
                Ok(RuntimeOutcomeState::InFlight {
                    instance_id: owner,
                    ..
                }) if owner == instance_id
            )
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
        let request_hash = match request_hash(&request) {
            Ok(hash) => hash,
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

        match self.reserve(
            &request_id,
            &request_hash,
            effect,
            RecoveryClass::ExternalAtMostOnce,
            instance_id,
        ) {
            Ok(Reservation::Replay(response)) => OutcomeExecution {
                response: *response,
                replayed: true,
            },
            Ok(Reservation::Conflict) => OutcomeExecution {
                response: request_id_conflict_response(request_id, effect),
                replayed: false,
            },
            Ok(Reservation::InFlight {
                instance_id: owner, ..
            }) if owner == instance_id => {
                match self.wait_for_response(&request_id, &request_hash, in_flight_wait) {
                    Ok(Some(response)) => OutcomeExecution {
                        response,
                        replayed: true,
                    },
                    Ok(None) => OutcomeExecution {
                        response: in_flight_response(request_id, effect, &owner, false),
                        replayed: false,
                    },
                    Err(error) => OutcomeExecution {
                        response: ledger_error_response(
                            request_id,
                            effect,
                            "daemon.request_outcome_lookup_failed",
                            error,
                            false,
                        ),
                        replayed: false,
                    },
                }
            }
            Ok(Reservation::InFlight {
                instance_id: owner, ..
            }) => OutcomeExecution {
                response: in_flight_response(request_id, effect, &owner, true),
                replayed: false,
            },
            Ok(Reservation::Execute) => {
                let response = execute(request);
                match self.complete(&request_id, &request_hash, &response) {
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
                }
            }
            Err(error) => OutcomeExecution {
                response: ledger_error_response(
                    request_id,
                    effect,
                    "daemon.request_outcome_reservation_failed",
                    error,
                    false,
                ),
                replayed: false,
            },
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
        let request_id = request.id.clone();
        let request_hash = match request_hash(&request) {
            Ok(hash) => hash,
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

        let owns_runtime_reservation = match self.reserve(
            &request_id,
            &request_hash,
            effect,
            RecoveryClass::AtomicProjectState,
            instance_id,
        ) {
            Ok(Reservation::Replay(response)) => {
                return OutcomeExecution {
                    response: *response,
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
                    Ok(Some(response)) => {
                        return OutcomeExecution {
                            response,
                            replayed: true,
                        };
                    }
                    Ok(None) => {
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
        let connection = Connection::open(&self.path)
            .with_context(|| format!("open daemon outcome ledger {}", self.path.display()))?;
        connection.pragma_update(None, "journal_mode", "wal")?;
        connection.pragma_update(None, "synchronous", "full")?;
        connection.pragma_update(None, "busy_timeout", 5_000)?;
        Ok(connection)
    }

    fn runtime_outcome_state(
        &self,
        request_id: &str,
        request_hash: &str,
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
            Some((stored_hash, _, _, _)) if stored_hash != request_hash => {
                RuntimeOutcomeState::Terminal
            }
            Some((_, _, _, Some(_))) => RuntimeOutcomeState::Terminal,
            Some((_, instance_id, recovery_class, None)) => RuntimeOutcomeState::InFlight {
                instance_id,
                recovery_class: recovery_class.as_deref().and_then(recovery_class_from_name),
            },
        })
    }

    fn reserve(
        &self,
        request_id: &str,
        request_hash: &str,
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

        let reservation = match existing {
            Some((stored_hash, _, _, _)) if stored_hash != request_hash => Reservation::Conflict,
            Some((_, _, _, Some(response_json))) => Reservation::Replay(Box::new(
                serde_json::from_str(&response_json)
                    .context("deserialize recorded daemon response")?,
            )),
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
                        request_hash,
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
        self.prune_completed(&connection)?;
        Ok(())
    }

    fn wait_for_response(
        &self,
        request_id: &str,
        request_hash: &str,
        timeout: Duration,
    ) -> Result<Option<ResponseEnvelope>> {
        let deadline = Instant::now() + timeout;
        loop {
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
            match row {
                Some((stored_hash, _)) if stored_hash != request_hash => {
                    return Err(anyhow!("request id was reused with a different payload"));
                }
                Some((_, Some(response_json))) => {
                    return Ok(Some(
                        serde_json::from_str(&response_json)
                            .context("deserialize recorded daemon response")?,
                    ));
                }
                None => return Err(anyhow!("daemon outcome reservation disappeared")),
                Some((_, None)) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(25));
                }
                Some((_, None)) => return Ok(None),
            }
        }
    }

    fn abandon(&self, request_id: &str, request_hash: &str, instance_id: &str) -> Result<()> {
        let connection = self.connection()?;
        connection.execute(
            "DELETE FROM daemon_request_outcomes
             WHERE request_id = ?1 AND request_hash = ?2 AND instance_id = ?3
               AND response_json IS NULL",
            params![request_id, request_hash, instance_id],
        )?;
        Ok(())
    }

    fn prune_completed(&self, connection: &Connection) -> Result<()> {
        let cutoff = now_timestamp() - COMPLETED_OUTCOME_RETENTION_SECS;
        connection.execute(
            "DELETE FROM daemon_request_outcomes
             WHERE completed_at IS NOT NULL AND completed_at < ?1",
            [cutoff],
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

        let mut project_connection = Connection::open(project_db_path)
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
    let connection = Connection::open_with_flags(project_db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
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

fn canonical_atomic_terminal_outcome(
    project_db_path: &Path,
    request: &RequestEnvelope,
    request_hash: &str,
) -> Result<Option<OutcomeExecution>> {
    if !project_db_path.exists() {
        return Ok(None);
    }
    let connection = Connection::open_with_flags(project_db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
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
        return Ok(None);
    }
    let existing = connection
        .query_row(
            "SELECT request_hash, effect, response_json
             FROM atomic_request_outcomes
             WHERE request_id = ?1",
            [&request.id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?;
    let Some((stored_hash, effect, response_json)) = existing else {
        return Ok(None);
    };
    let effect = effect_from_name(&effect)?;
    if stored_hash != request_hash {
        return Ok(Some(OutcomeExecution {
            response: without_committed_effect(request_id_conflict_response(
                request.id.clone(),
                effect,
            )),
            replayed: false,
        }));
    }
    Ok(Some(OutcomeExecution {
        response: serde_json::from_str(&response_json)
            .context("deserialize canonical atomic request outcome")?,
        replayed: true,
    }))
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

pub fn request_may_have_recorded_outcome(request: &RequestEnvelope) -> bool {
    let Some((namespace, operation)) = request_command_path(request) else {
        return false;
    };
    static COMMAND_SPEC: OnceLock<CommandSpec> = OnceLock::new();
    COMMAND_SPEC
        .get_or_init(|| CommandSpec::from_registry(&default_registry()))
        .operation(&namespace, &operation)
        .is_some_and(|operation| operation.effect != Effect::Pure)
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
    let transaction =
        RequestTransaction::begin(project_db_path).context("begin atomic request transaction")?;
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
    transaction.commit()?;

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

fn request_hash(request: &RequestEnvelope) -> Result<String> {
    let bytes = serde_json::to_vec(request)?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

fn now_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs() as i64
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

fn ledger_error_response(
    id: String,
    effect: Effect,
    kind: &str,
    error: anyhow::Error,
    mutation_may_have_completed: bool,
) -> ResponseEnvelope {
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
            .terminal_outcome_before_preparation(&request, &temp.path().join("missing.db"))
            .expect("probe terminal runtime outcome")
            .expect("completed runtime outcome");

        assert!(replay.replayed);
        assert_eq!(replay.response.result, first.response.result);
    }

    #[test]
    fn terminal_canonical_outcome_replays_after_issuing_workspace_is_removed() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("exo.db");
        drop(open_database(&db_path).expect("initialize project database"));
        let ledger = RequestOutcomeLedger::open(temp.path().join(DAEMON_OUTCOME_DB_NAME))
            .expect("open ledger");
        let workspace = temp.path().join("linked-worktree");
        std::fs::create_dir(&workspace).expect("create issuing workspace");
        let mut request = request("request-removed-canonical-workspace", "task-a");
        request.workspace_root = Some(workspace.clone());
        let hash = request_hash(&request).expect("request hash");
        let first = execute_atomic_core(
            &db_path,
            &hash,
            Effect::Write,
            request.clone(),
            |request| response(&request.id),
            || Ok(()),
        )
        .expect("commit canonical outcome");
        std::fs::remove_dir(&workspace).expect("remove issuing workspace");

        let replay = ledger
            .terminal_outcome_before_preparation(&request, &db_path)
            .expect("probe terminal canonical outcome")
            .expect("completed canonical outcome");

        assert!(replay.replayed);
        assert_eq!(replay.response.result, first.response.result);
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
        };
        let request = request("request-without-runtime-ledger", "task-a");
        assert!(
            ledger
                .atomic_request_needs_preparation(&request, &db_path, "instance-a")
                .expect("canonical database should authorize preparation"),
            "runtime lookup failure must not block canonical atomic execution"
        );

        let execution = ledger.execute_atomic_project_state(
            request.clone(),
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
        let replay = ledger
            .terminal_outcome_before_preparation(&request, &db_path)
            .expect("canonical replay should tolerate unavailable runtime ledger")
            .expect("canonical replay outcome");
        assert!(replay.replayed);
        assert_eq!(replay.response.result, execution.response.result);
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

        assert!(!request_may_have_recorded_outcome(&status));
        assert!(request_may_have_recorded_outcome(&request(
            "request-1",
            "task-a"
        )));
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
