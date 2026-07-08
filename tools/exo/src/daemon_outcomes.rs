//! Durable daemon request outcomes for transparent mutation recovery.
//!
//! Mutating requests are reserved before command dispatch and their complete
//! response is persisted before the daemon writes to the client socket. A
//! reconnecting client can therefore resend the same request envelope without
//! executing the command a second time.

use crate::api::protocol::{
    Address, Effect, ErrorBody, ErrorCode, Op, PROTOCOL_VERSION, RequestEnvelope, ResponseEnvelope,
    Status,
};
use crate::command::command_spec::CommandSpec;
use crate::command::registry::{build_command_from_invocation, default_registry};
use crate::command::router::Invocation;
use anyhow::{Context, Result, anyhow};
use exosuit_storage::rusqlite::TransactionBehavior;
use exosuit_storage::{Connection, OptionalExtension, params};
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
    InFlight { instance_id: String },
    Conflict,
}

#[derive(Debug)]
pub struct OutcomeExecution {
    pub response: ResponseEnvelope,
    pub replayed: bool,
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
                 response_json TEXT,
                 started_at INTEGER NOT NULL,
                 completed_at INTEGER
             );
             CREATE INDEX IF NOT EXISTS daemon_request_outcomes_completed_at
                 ON daemon_request_outcomes(completed_at);",
        )?;
        ledger.prune_completed(&connection)?;
        Ok(ledger)
    }

    pub fn path(&self) -> &Path {
        &self.path
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

        match self.reserve(&request_id, &request_hash, effect, instance_id) {
            Ok(Reservation::Replay(response)) => OutcomeExecution {
                response: *response,
                replayed: true,
            },
            Ok(Reservation::Conflict) => OutcomeExecution {
                response: request_id_conflict_response(request_id, effect),
                replayed: false,
            },
            Ok(Reservation::InFlight { instance_id: owner }) if owner == instance_id => {
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
            Ok(Reservation::InFlight { instance_id: owner }) => OutcomeExecution {
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

    fn connection(&self) -> Result<Connection> {
        let connection = Connection::open(&self.path)
            .with_context(|| format!("open daemon outcome ledger {}", self.path.display()))?;
        connection.pragma_update(None, "journal_mode", "wal")?;
        connection.pragma_update(None, "synchronous", "full")?;
        connection.pragma_update(None, "busy_timeout", 5_000)?;
        Ok(connection)
    }

    fn reserve(
        &self,
        request_id: &str,
        request_hash: &str,
        effect: Effect,
        instance_id: &str,
    ) -> Result<Reservation> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing = transaction
            .query_row(
                "SELECT request_hash, instance_id, response_json
                 FROM daemon_request_outcomes
                 WHERE request_id = ?1",
                [request_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .optional()?;

        let reservation = match existing {
            Some((stored_hash, _, _)) if stored_hash != request_hash => Reservation::Conflict,
            Some((_, _, Some(response_json))) => Reservation::Replay(Box::new(
                serde_json::from_str(&response_json)
                    .context("deserialize recorded daemon response")?,
            )),
            Some((_, owner_instance_id, None)) => Reservation::InFlight {
                instance_id: owner_instance_id,
            },
            None => {
                transaction.execute(
                    "INSERT INTO daemon_request_outcomes (
                         request_id, request_hash, effect, instance_id, started_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        request_id,
                        request_hash,
                        effect_name(effect),
                        instance_id,
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

    fn prune_completed(&self, connection: &Connection) -> Result<()> {
        let cutoff = now_timestamp() - COMPLETED_OUTCOME_RETENTION_SECS;
        connection.execute(
            "DELETE FROM daemon_request_outcomes
             WHERE completed_at IS NOT NULL AND completed_at < ?1",
            [cutoff],
        )?;
        Ok(())
    }
}

pub fn resolved_request_effect(workspace_root: &Path, request: &RequestEnvelope) -> Option<Effect> {
    let Op::Call(params) = &request.op else {
        return None;
    };
    let Address::Operation { path } = &params.address else {
        return None;
    };
    let (namespace, operation) = match path.as_slice() {
        [operation] => ("", operation.clone()),
        [namespace, operation] => (namespace.as_str(), operation.clone()),
        [namespace, first, second] => (namespace.as_str(), format!("{first}.{second}")),
        _ => return None,
    };
    static COMMAND_SPEC: OnceLock<CommandSpec> = OnceLock::new();
    let spec = COMMAND_SPEC.get_or_init(|| CommandSpec::from_registry(&default_registry()));
    let invocation = Invocation::from_json(&params.input, namespace, &operation, spec).ok()?;
    build_command_from_invocation(&invocation, workspace_root)
        .ok()?
        .map(|command| command.effect())
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
    fn unfinished_previous_instance_is_not_reexecuted() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ledger = RequestOutcomeLedger::open(temp.path().join(DAEMON_OUTCOME_DB_NAME))
            .expect("open ledger");
        let request = request("request-1", "task-a");
        let hash = request_hash(&request).expect("request hash");
        assert!(matches!(
            ledger
                .reserve(&request.id, &hash, Effect::Exec, "instance-a")
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
    fn resolved_effect_comes_from_built_command() {
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
}
