#![cfg_attr(
    not(test),
    deny(clippy::expect_used, clippy::panic, clippy::unwrap_used)
)]

use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};

use exo::api::protocol::Effect;
use exo::dogfood_activation::{DogfoodActivation, status_value as dogfood_activation_status_value};
use exo::mcp::{
    ExecutableIdentity, MCP_WORKER_PROTOCOL_VERSION, executable_identity_for_path,
    executable_identity_matches_path,
};
use exosuit_process_host::{HostError, JsonLineWorker, LineResponse, WorkerSpec, serve_json_lines};
use serde::Deserialize;
use serde_json::{Value as JsonValue, json};
use uuid::Uuid;

fn main() {
    exo_reexec::maybe_reexec();

    if let Err(error) = run() {
        eprintln!("exo-mcp: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let worker_spec = worker_spec()?.current_dir(std::env::current_dir()?);

    if std::env::args().nth(1).as_deref() == Some("--proxy-health") {
        let mut proxy_worker = ProxyWorker::new(worker_spec)?;
        let health_error = proxy_worker
            .ensure_worker()
            .err()
            .map(|error| error.to_string());
        let mut stdout = std::io::stdout().lock();
        serde_json::to_writer(
            &mut stdout,
            &json!({
                "kind": "exo-mcp.proxy-health",
                "worker_protocol_version": MCP_WORKER_PROTOCOL_VERSION,
                "ok": health_error.is_none(),
                "issue": health_error,
                "status": proxy_worker.status_value(),
            }),
        )?;
        writeln!(stdout)?;
        return Ok(());
    }

    let mut proxy_worker = ProxyWorker::new(worker_spec)?;

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let stdout_lock = stdout.lock();

    {
        let reader = BufReader::new(stdin.lock());
        serve_json_lines(reader, stdout_lock, |line| {
            if let Some(value) = proxy_worker.status_response_for_line(line) {
                return LineResponse::reply(value);
            }

            match proxy_worker.call_line(line) {
                Ok(Some(value)) => LineResponse::reply(value),
                Ok(None) => LineResponse::none(),
                Err(error) => LineResponse::reply(worker_error_for_line(line, &error)),
            }
        })?;
    }

    Ok(())
}

#[derive(Debug)]
struct ProxyWorker {
    spec: WorkerSpec,
    proxy_identity: ProxyExecutableIdentity,
    dogfood_activation: DogfoodActivation,
    running: Option<RunningWorker>,
    restart_count: u64,
    last_restart_reason: Option<String>,
    last_error: Option<String>,
    pending_restart_reason: Option<String>,
    worker_request_seq: u64,
}

impl ProxyWorker {
    fn new(spec: WorkerSpec) -> std::io::Result<Self> {
        Ok(Self {
            spec,
            proxy_identity: ProxyExecutableIdentity::capture()?,
            dogfood_activation: DogfoodActivation::from_environment(),
            running: None,
            restart_count: 0,
            last_restart_reason: None,
            last_error: None,
            pending_restart_reason: None,
            worker_request_seq: 0,
        })
    }

    fn call_line(&mut self, line: &str) -> Result<Option<JsonValue>, HostError> {
        if let Some(value) = self.client_response_for_line(line)? {
            return Ok(Some(value));
        }
        self.forward_line_to_worker(line)
    }

    fn client_response_for_line(&mut self, line: &str) -> Result<Option<JsonValue>, HostError> {
        let Ok(value) = serde_json::from_str::<JsonValue>(line) else {
            return Ok(None);
        };
        let Some(object) = value.as_object() else {
            return Ok(None);
        };
        let Some(id) = object.get("id").cloned() else {
            return Ok(None);
        };
        let Some(method) = object.get("method").and_then(JsonValue::as_str) else {
            return Ok(None);
        };
        let params = object.get("params").cloned();

        match method {
            "tools/list" => self.handle_tools_list(id, params).map(Some),
            "tools/call" => self.handle_tools_call(id, params).map(Some),
            method if method.starts_with("worker/") => {
                Ok(Some(worker_method_rejected_response(id, method)))
            }
            _ => Ok(None),
        }
    }

    fn handle_tools_list(
        &mut self,
        client_id: JsonValue,
        params: Option<JsonValue>,
    ) -> Result<JsonValue, HostError> {
        let response = match self.call_worker_method("worker/listTools", params.clone()) {
            Ok(output) => output.response,
            Err(failure) if failure.request_started => {
                self.call_worker_method("worker/listTools", params)
                    .map_err(WorkerMethodFailure::into_error)?
                    .response
            }
            Err(failure) => return Err(failure.into_error()),
        };
        Ok(worker_response_for_client_id(client_id, response))
    }

    fn handle_tools_call(
        &mut self,
        client_id: JsonValue,
        params: Option<JsonValue>,
    ) -> Result<JsonValue, HostError> {
        let params = bind_outcome_request_id(params);
        let mut classification_response = self.classify_worker_call(params.clone())?;
        let mut pre_call_restart_reclassifications = 0;

        loop {
            if classification_response.get("error").is_some() {
                return Ok(worker_response_for_client_id(
                    client_id,
                    classification_response,
                ));
            }
            let classification = worker_call_classification(&classification_response)?;
            if let Some(tool_result) = classification.tool_result.clone() {
                return Ok(json!({
                    "jsonrpc": "2.0",
                    "id": client_id,
                    "result": tool_result,
                }));
            }

            match self.call_worker_method_bound_to_classification("worker/call", params.clone()) {
                Ok(output) => return Ok(worker_response_for_client_id(client_id, output.response)),
                Err(failure) if failure.restarted_before_request() => {
                    if pre_call_restart_reclassifications >= 1 {
                        return Ok(retry_required_response(
                            client_id,
                            &classification,
                            &failure,
                        ));
                    }
                    pre_call_restart_reclassifications += 1;
                    classification_response = self.classify_worker_call(params.clone())?;
                }
                Err(failure) => {
                    let retry_classification_response =
                        match self.call_worker_method("worker/classify", params.clone()) {
                            Ok(output) => output.response,
                            Err(_) => {
                                return Ok(retry_required_response(
                                    client_id,
                                    &classification,
                                    &failure,
                                ));
                            }
                        };
                    if retry_classification_response.get("error").is_some() {
                        return Ok(worker_response_for_client_id(
                            client_id,
                            retry_classification_response,
                        ));
                    }
                    let retry_classification =
                        worker_call_classification(&retry_classification_response)?;
                    if let Some(tool_result) = retry_classification.tool_result {
                        return Ok(json!({
                            "jsonrpc": "2.0",
                            "id": client_id,
                            "result": tool_result,
                        }));
                    }
                    if retry_classification.effect != classification.effect {
                        return Ok(retry_required_response(
                            client_id,
                            &retry_classification,
                            &failure,
                        ));
                    }

                    return match self
                        .call_worker_method_bound_to_classification("worker/call", params)
                    {
                        Ok(output) => Ok(worker_response_for_client_id(client_id, output.response)),
                        Err(failure) => Ok(retry_required_response(
                            client_id,
                            &retry_classification,
                            &failure,
                        )),
                    };
                }
            }
        }
    }

    fn classify_worker_call(&mut self, params: Option<JsonValue>) -> Result<JsonValue, HostError> {
        match self.call_worker_method("worker/classify", params.clone()) {
            Ok(output) => Ok(output.response),
            Err(_) => self
                .call_worker_method("worker/classify", params)
                .map(|output| output.response)
                .map_err(WorkerMethodFailure::into_error),
        }
    }

    fn forward_line_to_worker(&mut self, line: &str) -> Result<Option<JsonValue>, HostError> {
        self.ensure_worker()?;
        self.ensure_worker_identity_is_current()?;

        let result = self
            .running
            .as_mut()
            .ok_or_else(|| HostError::Protocol("worker was not running".to_string()))?
            .worker
            .call_line(line);

        match result {
            Ok(value) => Ok(value),
            Err(error) => {
                self.record_worker_call_error(&error);
                Err(error)
            }
        }
    }

    fn call_worker_method(
        &mut self,
        method: &str,
        params: Option<JsonValue>,
    ) -> Result<WorkerMethodOutput, WorkerMethodFailure> {
        self.call_worker_method_inner(method, params, true)
    }

    fn call_worker_method_bound_to_classification(
        &mut self,
        method: &str,
        params: Option<JsonValue>,
    ) -> Result<WorkerMethodOutput, WorkerMethodFailure> {
        self.call_worker_method_inner(method, params, false)
    }

    fn call_worker_method_inner(
        &mut self,
        method: &str,
        params: Option<JsonValue>,
        allow_restart_before_request: bool,
    ) -> Result<WorkerMethodOutput, WorkerMethodFailure> {
        let mut restart_reason = match self.ensure_worker_with_restart_reason() {
            Ok(reason) => reason,
            Err(error) => {
                return Err(WorkerMethodFailure::before_request(
                    error,
                    self.pending_restart_reason.clone(),
                ));
            }
        };
        match self.ensure_worker_identity_is_current_with_restart_reason() {
            Ok(reason) => {
                if reason.is_some() {
                    restart_reason = reason;
                }
            }
            Err(error) => {
                return Err(WorkerMethodFailure::before_request(
                    error,
                    self.pending_restart_reason.clone(),
                ));
            }
        }

        if !allow_restart_before_request && let Some(reason) = restart_reason.clone() {
            return Err(WorkerMethodFailure::before_request(
                HostError::Protocol(format!(
                    "{method} requires reclassification after worker restart"
                )),
                Some(reason),
            ));
        }

        let id = self.next_worker_request_id(method);
        let mut request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
        });
        if let Some(params) = params {
            request["params"] = params;
        }
        let line = serde_json::to_string(&request).map_err(|error| {
            WorkerMethodFailure::before_request(HostError::Json(error), restart_reason.clone())
        })?;
        let result = self
            .running
            .as_mut()
            .ok_or_else(|| {
                WorkerMethodFailure::before_request(
                    HostError::Protocol("worker was not running".to_string()),
                    restart_reason.clone(),
                )
            })?
            .worker
            .call_line(&line);

        match result {
            Ok(Some(value)) => {
                if let Err(error) = verify_worker_response_id(method, &id, &value) {
                    self.record_worker_call_error(&error);
                    return Err(WorkerMethodFailure::during_request(
                        error,
                        self.pending_restart_reason.clone(),
                    ));
                }
                Ok(WorkerMethodOutput { response: value })
            }
            Ok(None) => {
                let error = HostError::Protocol(format!("{method} did not return a response"));
                self.record_worker_call_error(&error);
                Err(WorkerMethodFailure::during_request(
                    error,
                    self.pending_restart_reason.clone(),
                ))
            }
            Err(error) => {
                self.record_worker_call_error(&error);
                Err(WorkerMethodFailure::during_request(
                    error,
                    self.pending_restart_reason.clone(),
                ))
            }
        }
    }

    fn next_worker_request_id(&mut self, method: &str) -> String {
        self.worker_request_seq += 1;
        format!(
            "exo-proxy-worker-{}-{}",
            self.worker_request_seq,
            method.replace('/', ".")
        )
    }

    fn status_response_for_line(&mut self, line: &str) -> Option<JsonValue> {
        let value = serde_json::from_str::<JsonValue>(line).ok()?;
        let object = value.as_object()?;
        if object.get("method").and_then(JsonValue::as_str) != Some("exo/proxy/status") {
            return None;
        }
        if !object.contains_key("id") {
            return None;
        }
        let _ = self.ensure_worker_identity_is_current();
        let id = object.get("id").cloned().unwrap_or(JsonValue::Null);
        Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": self.status_value(),
        }))
    }

    fn status_value(&self) -> JsonValue {
        let worker = self.running.as_ref().map(|running| {
            json!({
                "pid": running.process_id,
                "identity": running.identity,
            })
        });

        json!({
            "proxy": self.proxy_identity.status_value(),
            "activation": dogfood_activation_status_value(self.dogfood_activation.status(
                &self.proxy_identity.executable_path,
                self.running.as_ref().map(|running| &running.identity),
            )),
            "restart_count": self.restart_count,
            "last_restart_reason": self.last_restart_reason,
            "last_error": self.last_error,
            "pending_restart_reason": self.pending_restart_reason,
            "worker": worker,
        })
    }

    fn ensure_worker(&mut self) -> Result<(), HostError> {
        self.ensure_worker_with_restart_reason().map(|_| ())
    }

    fn ensure_worker_with_restart_reason(&mut self) -> Result<Option<String>, HostError> {
        self.dogfood_activation
            .ensure_before_worker(&self.proxy_identity.executable_path)
            .map_err(HostError::Protocol)?;
        if self.running.is_some() {
            return Ok(None);
        }

        let reason = self
            .pending_restart_reason
            .clone()
            .unwrap_or_else(|| "initial_start".to_string());
        let record_restart = reason != "initial_start";
        self.spawn_worker_recording(&reason, record_restart)?;
        let Some(running) = self.running.as_ref() else {
            return Err(HostError::Protocol("worker did not start".to_string()));
        };
        self.dogfood_activation
            .ensure_worker(&self.proxy_identity.executable_path, &running.identity)
            .map_err(HostError::Protocol)?;
        Ok(Some(reason))
    }

    fn ensure_worker_identity_is_current(&mut self) -> Result<(), HostError> {
        self.ensure_worker_identity_is_current_with_restart_reason()
            .map(|_| ())
    }

    fn ensure_worker_identity_is_current_with_restart_reason(
        &mut self,
    ) -> Result<Option<String>, HostError> {
        let Some(running) = self.running.as_ref() else {
            return Ok(None);
        };
        let is_stale = executable_identity_matches_path(
            &running.executable_identity,
            &running.identity_check_path,
        )
        .map_or(true, |matches| !matches);
        if !is_stale {
            return Ok(None);
        }

        self.running = None;
        self.pending_restart_reason = Some("worker_binary_changed".to_string());
        self.spawn_worker_recording("worker_binary_changed", true)?;
        Ok(Some("worker_binary_changed".to_string()))
    }

    fn spawn_worker_recording(
        &mut self,
        reason: &str,
        record_restart: bool,
    ) -> Result<(), HostError> {
        match self.spawn_worker(reason, record_restart) {
            Ok(()) => {
                self.pending_restart_reason = None;
                Ok(())
            }
            Err(error) => {
                self.record_restart_error(reason, &error);
                self.pending_restart_reason = Some(reason.to_string());
                Err(error)
            }
        }
    }

    fn spawn_worker(&mut self, reason: &str, record_restart: bool) -> Result<(), HostError> {
        let mut worker = JsonLineWorker::spawn(&self.spec)?;
        let process_id = worker.process_id();
        let identity = request_worker_identity(&mut worker)?;
        let probe = worker_identity_probe(&identity)?;
        let identity_check_path = worker_identity_check_path(&self.spec, &probe.executable_path);

        self.last_restart_reason = Some(reason.to_string());
        if record_restart {
            self.restart_count += 1;
        }
        self.last_error = None;
        self.running = Some(RunningWorker {
            worker,
            process_id,
            identity_check_path,
            executable_identity: probe.executable_identity,
            identity,
        });
        Ok(())
    }

    fn record_restart_error(&mut self, reason: &str, error: &HostError) {
        self.pending_restart_reason = Some(reason.to_string());
        self.last_error = Some(error.to_string());
    }

    fn record_worker_call_error(&mut self, error: &HostError) {
        let reason = worker_failure_reason(error);
        self.last_error = Some(error.to_string());
        self.running = None;
        self.pending_restart_reason = Some(reason.to_string());
    }
}

fn bind_outcome_request_id(params: Option<JsonValue>) -> Option<JsonValue> {
    let mut params = params.unwrap_or_else(|| json!({}));
    if let Some(object) = params.as_object_mut() {
        object
            .entry(exo::mcp::MCP_OUTCOME_REQUEST_ID_PARAM)
            .or_insert_with(|| JsonValue::String(Uuid::new_v4().to_string()));
    }
    Some(params)
}

#[derive(Debug, Clone)]
struct ProxyExecutableIdentity {
    executable_path: PathBuf,
    executable_identity: ExecutableIdentity,
}

impl ProxyExecutableIdentity {
    fn capture() -> std::io::Result<Self> {
        let executable_path = std::env::current_exe()?;
        let executable_identity = executable_identity_for_path(&executable_path)?;
        Ok(Self {
            executable_path,
            executable_identity,
        })
    }

    fn status_value(&self) -> JsonValue {
        let on_disk = match executable_identity_for_path(&self.executable_path) {
            Ok(identity) => {
                let matches_startup = executable_identity_matches_path(
                    &self.executable_identity,
                    &self.executable_path,
                )
                .unwrap_or(false);
                json!({
                    "matches_startup": matches_startup,
                    "executable_identity": identity,
                })
            }
            Err(error) => json!({
                "matches_startup": false,
                "error": error.to_string(),
            }),
        };

        json!({
            "executable_path": self.executable_path,
            "executable_identity": self.executable_identity,
            "on_disk": on_disk,
            "worker_protocol_version": MCP_WORKER_PROTOCOL_VERSION,
        })
    }
}

fn worker_failure_reason(error: &HostError) -> &'static str {
    match error {
        HostError::WorkerClosed | HostError::MissingPipe(_) => "worker_exited",
        HostError::Io(_) => "worker_io_error",
        HostError::Json(_) | HostError::Protocol(_) => "worker_protocol_error",
    }
}

#[derive(Debug)]
struct WorkerMethodOutput {
    response: JsonValue,
}

#[derive(Debug)]
struct WorkerMethodFailure {
    error: HostError,
    restart_reason: Option<String>,
    request_started: bool,
}

impl WorkerMethodFailure {
    fn before_request(error: HostError, restart_reason: Option<String>) -> Self {
        Self {
            error,
            restart_reason,
            request_started: false,
        }
    }

    fn during_request(error: HostError, restart_reason: Option<String>) -> Self {
        Self {
            error,
            restart_reason,
            request_started: true,
        }
    }

    fn restarted_before_request(&self) -> bool {
        !self.request_started && self.restart_reason.is_some()
    }

    fn into_error(self) -> HostError {
        self.error
    }

    fn reason(&self) -> &'static str {
        if let Some(reason) = self.restart_reason.as_deref() {
            return match reason {
                "initial_start" => "initial_start",
                "worker_binary_changed" => "worker_binary_changed",
                "worker_exited" => "worker_exited",
                "worker_protocol_error" => "worker_protocol_error",
                "worker_io_error" => "worker_io_error",
                _ => "worker_restart",
            };
        }

        worker_failure_reason(&self.error)
    }
}

#[derive(Debug, Clone, Deserialize)]
struct WorkerCallClassification {
    #[serde(default)]
    tool_result: Option<JsonValue>,
    effect: Effect,
    #[serde(default)]
    request_summary: Option<JsonValue>,
    #[serde(default)]
    has_auth: bool,
    #[serde(default)]
    has_workflow_confirmation: bool,
}

fn worker_call_classification(response: &JsonValue) -> Result<WorkerCallClassification, HostError> {
    let result = response
        .get("result")
        .cloned()
        .ok_or_else(|| HostError::Protocol("worker/classify omitted result".to_string()))?;
    serde_json::from_value(result).map_err(HostError::Json)
}

fn worker_method_rejected_response(id: JsonValue, method: &str) -> JsonValue {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": -32601,
            "message": "Method not found",
            "data": {
                "method": method,
                "reason": "worker methods are internal to the Exo MCP proxy"
            }
        }
    })
}

fn verify_worker_response_id(
    method: &str,
    expected_id: &str,
    response: &JsonValue,
) -> Result<(), HostError> {
    match response.get("id").and_then(JsonValue::as_str) {
        Some(actual_id) if actual_id == expected_id => Ok(()),
        Some(actual_id) => Err(HostError::Protocol(format!(
            "{method} response id mismatch: expected {expected_id}, got {actual_id}"
        ))),
        None => Err(HostError::Protocol(format!(
            "{method} response omitted id {expected_id}"
        ))),
    }
}

fn worker_response_for_client_id(client_id: JsonValue, worker_response: JsonValue) -> JsonValue {
    if let Some(error) = worker_response.get("error") {
        return json!({
            "jsonrpc": "2.0",
            "id": client_id,
            "error": error,
        });
    }

    json!({
        "jsonrpc": "2.0",
        "id": client_id,
        "result": worker_response.get("result").cloned().unwrap_or(JsonValue::Null),
    })
}

fn retry_required_response(
    client_id: JsonValue,
    classification: &WorkerCallClassification,
    failure: &WorkerMethodFailure,
) -> JsonValue {
    let effect = effect_name(classification.effect);
    let request_state = if failure.request_started {
        "may_have_started"
    } else {
        "not_started"
    };
    let message = if failure.request_started {
        format!(
            "Exo could not retrieve the recorded {effect} outcome after automatic worker recovery."
        )
    } else {
        format!(
            "Exo MCP worker could not start a {effect} request. Retry after the worker is healthy."
        )
    };
    json!({
        "jsonrpc": "2.0",
        "id": client_id,
        "error": {
            "code": -32000,
            "message": message,
            "data": {
                "code": "exo.retry_required",
                "effect": effect,
                "worker_restart_reason": failure.reason(),
                "request_state": request_state,
                "worker_error": failure.error.to_string(),
                "request_summary": classification.request_summary.clone().unwrap_or_else(|| json!({})),
                "has_auth": classification.has_auth,
                "has_workflow_confirmation": classification.has_workflow_confirmation,
            },
        },
    })
}

const fn effect_name(effect: Effect) -> &'static str {
    match effect {
        Effect::Pure => "pure",
        Effect::Write => "write",
        Effect::Exec => "exec",
    }
}

#[derive(Debug)]
struct RunningWorker {
    worker: JsonLineWorker,
    process_id: u32,
    identity_check_path: PathBuf,
    executable_identity: ExecutableIdentity,
    identity: JsonValue,
}

#[derive(Debug, Deserialize)]
struct WorkerIdentityProbe {
    executable_path: PathBuf,
    executable_identity: ExecutableIdentity,
    worker_protocol_version: u32,
}

fn request_worker_identity(worker: &mut JsonLineWorker) -> Result<JsonValue, HostError> {
    let line = serde_json::to_string(&json!({
        "jsonrpc": "2.0",
        "id": "exo-proxy-worker-hello",
        "method": "worker/hello",
    }))?;
    let response = worker
        .call_line(&line)?
        .ok_or_else(|| HostError::Protocol("worker/hello did not return a response".to_string()))?;
    if let Some(error) = response.get("error") {
        return Err(HostError::Protocol(format!("worker/hello failed: {error}")));
    }
    response
        .pointer("/result/identity")
        .cloned()
        .ok_or_else(|| HostError::Protocol("worker/hello omitted result.identity".to_string()))
}

fn worker_identity_probe(identity: &JsonValue) -> Result<WorkerIdentityProbe, HostError> {
    let probe: WorkerIdentityProbe =
        serde_json::from_value(identity.clone()).map_err(HostError::Json)?;
    if probe.worker_protocol_version != MCP_WORKER_PROTOCOL_VERSION {
        return Err(HostError::Protocol(format!(
            "worker protocol version mismatch: proxy requires {}, worker advertised {}",
            MCP_WORKER_PROTOCOL_VERSION, probe.worker_protocol_version
        )));
    }
    Ok(probe)
}

fn worker_identity_check_path(spec: &WorkerSpec, worker_executable_path: &Path) -> PathBuf {
    if is_direct_exo_worker_spec(spec) {
        spec.program().to_path_buf()
    } else {
        worker_executable_path.to_path_buf()
    }
}

fn is_direct_exo_worker_spec(spec: &WorkerSpec) -> bool {
    spec.args() == ["mcp", "worker"]
}

fn worker_spec() -> Result<WorkerSpec, std::io::Error> {
    if let Some(path) = std::env::var_os("EXO_MCP_WORKER") {
        return Ok(exo_worker_spec(PathBuf::from(path)));
    }

    let current = std::env::current_exe()?;
    Ok(worker_spec_for_activation(
        DogfoodActivation::source_worker_path_from_environment(),
        &current,
        current.with_file_name(worker_binary_name()).is_file(),
        std::env::var_os("CARGO").map(PathBuf::from),
        &exo_manifest_path(),
    ))
}

fn worker_spec_for_activation(
    source_worker: Option<PathBuf>,
    current_exe: &Path,
    sibling_exists: bool,
    cargo: Option<PathBuf>,
    manifest_path: &Path,
) -> WorkerSpec {
    if let Some(path) = source_worker {
        return exo_worker_spec(path);
    }
    worker_spec_for(current_exe, sibling_exists, cargo, manifest_path)
}

fn worker_spec_for(
    current_exe: &Path,
    sibling_exists: bool,
    cargo: Option<PathBuf>,
    manifest_path: &Path,
) -> WorkerSpec {
    let sibling = current_exe.with_file_name(worker_binary_name());
    if sibling_exists {
        return exo_worker_spec(sibling);
    }

    WorkerSpec::new(cargo.unwrap_or_else(|| PathBuf::from("cargo")))
        .arg("run")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(manifest_path.to_string_lossy())
        .arg("--bin")
        .arg("exo")
        .arg("--")
        .arg("mcp")
        .arg("worker")
}

fn exo_worker_spec(program: PathBuf) -> WorkerSpec {
    WorkerSpec::new(program).arg("mcp").arg("worker")
}

fn exo_manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")
}

const fn worker_binary_name() -> &'static str {
    if cfg!(windows) { "exo.exe" } else { "exo" }
}

fn worker_error_for_line(line: &str, error: &dyn std::error::Error) -> JsonValue {
    json!({
        "jsonrpc": "2.0",
        "id": json_rpc_id(line),
        "error": {
            "code": -32000,
            "message": "Exo MCP worker failed.",
            "data": {
                "message": error.to_string(),
            },
        },
    })
}

fn json_rpc_id(line: &str) -> JsonValue {
    serde_json::from_str::<JsonValue>(line)
        .ok()
        .and_then(|value| value.get("id").cloned())
        .unwrap_or(JsonValue::Null)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use serde_json::json;

    use super::{
        bind_outcome_request_id, exo_worker_spec, worker_binary_name, worker_identity_check_path,
        worker_spec_for, worker_spec_for_activation,
    };

    #[test]
    fn outcome_request_id_is_added_when_params_are_absent() {
        let params = bind_outcome_request_id(None).expect("params object");
        assert!(
            params
                .get(exo::mcp::MCP_OUTCOME_REQUEST_ID_PARAM)
                .and_then(|value| value.as_str())
                .is_some_and(|value| !value.is_empty())
        );
    }

    #[test]
    fn existing_outcome_request_id_is_preserved() {
        let params = bind_outcome_request_id(Some(json!({
            exo::mcp::MCP_OUTCOME_REQUEST_ID_PARAM: "caller-owned-id",
            "command": "status",
        })))
        .expect("params object");

        assert_eq!(
            params[exo::mcp::MCP_OUTCOME_REQUEST_ID_PARAM],
            "caller-owned-id"
        );
    }

    #[test]
    fn outcome_request_id_is_added_to_existing_params_object() {
        let params =
            bind_outcome_request_id(Some(json!({ "command": "status" }))).expect("params object");
        assert!(
            params
                .get(exo::mcp::MCP_OUTCOME_REQUEST_ID_PARAM)
                .and_then(|value| value.as_str())
                .is_some_and(|value| !value.is_empty())
        );
        assert_eq!(params["command"], "status");
    }

    #[test]
    fn worker_spec_prefers_sibling_exo_binary() {
        let manifest_path = PathBuf::from("/repo/tools/exo/Cargo.toml");
        let spec = worker_spec_for(
            Path::new("/tmp/target/debug/exo-mcp"),
            true,
            Some(PathBuf::from("/usr/bin/cargo")),
            &manifest_path,
        );

        let expected = Path::new("/tmp/target/debug").join(worker_binary_name());
        assert_eq!(spec.program(), expected.as_path());
        assert_eq!(spec.args(), ["mcp", "worker"]);
    }

    #[test]
    fn worker_spec_falls_back_to_cargo_run_when_sibling_is_missing() {
        let manifest_path = PathBuf::from("/repo/tools/exo/Cargo.toml");
        let spec = worker_spec_for(
            Path::new("/tmp/target/debug/exo-mcp"),
            false,
            Some(PathBuf::from("/usr/bin/cargo")),
            &manifest_path,
        );

        assert_eq!(spec.program(), Path::new("/usr/bin/cargo"));
        assert_eq!(
            spec.args(),
            [
                "run",
                "--quiet",
                "--manifest-path",
                "/repo/tools/exo/Cargo.toml",
                "--bin",
                "exo",
                "--",
                "mcp",
                "worker"
            ]
        );
    }

    #[test]
    fn direct_worker_identity_check_uses_configured_program_path() {
        let spec = exo_worker_spec(PathBuf::from("/releases/current/exo"));
        let path = worker_identity_check_path(&spec, Path::new("/releases/one/exo"));

        assert_eq!(path, Path::new("/releases/current/exo"));
    }

    #[test]
    fn cargo_fallback_identity_check_uses_worker_reported_executable_path() {
        let manifest_path = PathBuf::from("/repo/tools/exo/Cargo.toml");
        let spec = worker_spec_for(
            Path::new("/tmp/target/debug/exo-mcp"),
            false,
            Some(PathBuf::from("/usr/bin/cargo")),
            &manifest_path,
        );
        let path = worker_identity_check_path(&spec, Path::new("/tmp/target/debug/exo"));

        assert_eq!(path, Path::new("/tmp/target/debug/exo"));
    }

    #[test]
    fn dogfood_activation_prefers_the_source_worker_over_the_installed_sibling() {
        let source_worker = PathBuf::from("/workspace/target/debug/exo");
        let spec = worker_spec_for_activation(
            Some(source_worker.clone()),
            Path::new("/install/bin/exo-mcp"),
            true,
            None,
            Path::new("/workspace/tools/exo/Cargo.toml"),
        );

        assert_eq!(spec.program(), source_worker);
        assert_eq!(spec.args(), ["mcp", "worker"]);
    }
}
