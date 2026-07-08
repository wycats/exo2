use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};
use uuid::Uuid;

use crate::api::handler;
use crate::api::protocol::{
    Address, Auth, CallParams, Display, Effect, ErrorBody, ErrorCode, HelpParams, Op,
    PROTOCOL_VERSION, RequestEnvelope, ResponseEnvelope, Status, WorkflowConfirmationDecision,
    WorkflowConfirmationInput,
};
use crate::command::command_spec::CommandSpec;
use crate::command::registry::{build_command_from_invocation, default_registry};
use crate::command::router::Invocation;
use crate::command_text::{CommandTextIntent, parse_command_text, tokens_request_json_output};
use crate::context::AgentContext;
use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::project::Project;
use crate::router::{Compilation, compile_argv};

pub const MCP_PROTOCOL_VERSION: &str = "2025-06-18";
pub const MCP_WORKER_PROTOCOL_VERSION: u32 = 2;
pub const MCP_OUTCOME_REQUEST_ID_PARAM: &str = "_exo_outcome_request_id";
const EXO_RUN_TOOL_NAME: &str = "exo-run";

#[derive(Debug, Clone, Deserialize)]
pub struct ExoRunInput {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default, rename = "workflowConfirmation")]
    pub workflow_confirmation: Option<McpWorkflowConfirmationInput>,
    #[serde(default)]
    pub auth: Option<Auth>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct McpWorkflowConfirmationInput {
    pub kind: String,
    #[serde(rename = "entityType")]
    pub entity_type: String,
    #[serde(rename = "entityId")]
    pub entity_id: String,
    pub decision: WorkflowConfirmationDecision,
    pub outcome: String,
}

impl From<McpWorkflowConfirmationInput> for WorkflowConfirmationInput {
    fn from(value: McpWorkflowConfirmationInput) -> Self {
        Self {
            kind: value.kind,
            entity_type: value.entity_type,
            entity_id: value.entity_id,
            decision: value.decision,
            outcome: value.outcome,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct McpToolResult {
    pub content: Vec<McpContent>,
    #[serde(rename = "structuredContent", skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<JsonValue>,
    #[serde(rename = "isError")]
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpContent {
    Text { text: String },
}

#[derive(Debug, Clone, Deserialize)]
struct JsonRpcMessage {
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    params: Option<JsonValue>,
}

#[derive(Debug, Clone, Deserialize)]
struct McpToolsCallParams {
    name: String,
    #[serde(default)]
    arguments: Option<JsonValue>,
}

#[derive(Debug, Clone)]
struct PreparedExoRunCall {
    request: RequestEnvelope,
    original_command: String,
    include_structured: bool,
    effect: Effect,
    preload_required: bool,
}

#[derive(Debug, Clone)]
struct PreparedWorkerCall {
    cache_key: String,
    prepared: PreparedExoRunCall,
}

#[derive(Debug, Default)]
struct WorkerCallCache {
    prepared: Option<PreparedWorkerCall>,
}

impl WorkerCallCache {
    fn store(
        &mut self,
        params: &Option<JsonValue>,
        prepared: PreparedExoRunCall,
    ) -> anyhow::Result<()> {
        self.prepared = Some(PreparedWorkerCall {
            cache_key: params_cache_key(params)?,
            prepared,
        });
        Ok(())
    }

    fn take(&mut self, params: &Option<JsonValue>) -> Option<PreparedExoRunCall> {
        let key = params_cache_key(params).ok()?;
        let prepared = self.prepared.take()?;
        if prepared.cache_key == key {
            Some(prepared.prepared)
        } else {
            self.prepared = Some(prepared);
            None
        }
    }
}

pub fn serve_stdio(workspace_root: &Path, project: Option<&Project>) -> anyhow::Result<()> {
    serve_stdio_inner(workspace_root, project, false)
}

pub fn serve_worker_stdio(workspace_root: &Path, project: Option<&Project>) -> anyhow::Result<()> {
    serve_stdio_inner(workspace_root, project, true)
}

fn serve_stdio_inner(
    workspace_root: &Path,
    project: Option<&Project>,
    worker_mode: bool,
) -> anyhow::Result<()> {
    let stale_guard = if worker_mode {
        None
    } else {
        McpExecutableGuard::capture().ok().flatten()
    };
    let stdin = std::io::stdin();
    let reader = std::io::BufReader::new(stdin.lock());
    let stdout = std::io::stdout();
    let mut stdout_lock = stdout.lock();
    let mut worker_call_cache = WorkerCallCache::default();

    if let Ok(line) = std::env::var("EXO_MCP_REPLAY_LINE") {
        if !line.trim().is_empty() {
            if let Some(response) = response_for_line(
                workspace_root,
                project,
                &line,
                worker_mode,
                &mut worker_call_cache,
            ) {
                serde_json::to_writer(&mut stdout_lock, &response)?;
                writeln!(stdout_lock)?;
                stdout_lock.flush()?;
            }
        }
    }

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let mut exit_after_response = false;
        let response = if stale_guard
            .as_ref()
            .is_some_and(McpExecutableGuard::is_stale)
        {
            match reexec_mcp_with_replay(&line) {
                Ok(never) => match never {},
                Err(error) => {
                    exit_after_response = true;
                    let id = serde_json::from_str::<JsonValue>(&line)
                        .ok()
                        .and_then(|value| value.get("id").cloned())
                        .unwrap_or(JsonValue::Null);
                    Some(mcp_reexec_error(id, &error, stale_guard.as_ref()))
                }
            }
        } else {
            response_for_line(
                workspace_root,
                project,
                &line,
                worker_mode,
                &mut worker_call_cache,
            )
        };

        if let Some(response) = response {
            serde_json::to_writer(&mut stdout_lock, &response)?;
            writeln!(stdout_lock)?;
            stdout_lock.flush()?;
        }
        if exit_after_response {
            break;
        }
    }

    Ok(())
}

fn response_for_line(
    workspace_root: &Path,
    project: Option<&Project>,
    line: &str,
    worker_mode: bool,
    worker_call_cache: &mut WorkerCallCache,
) -> Option<JsonValue> {
    match serde_json::from_str::<JsonValue>(line) {
        Ok(value) => handle_json_rpc_value(
            workspace_root,
            project,
            value,
            worker_mode,
            worker_call_cache,
        ),
        Err(error) => Some(json_rpc_error(
            JsonValue::Null,
            -32700,
            "Parse error",
            Some(json!({ "message": error.to_string() })),
        )),
    }
}

#[cfg(unix)]
fn reexec_mcp_with_replay(line: &str) -> Result<std::convert::Infallible, std::io::Error> {
    use std::os::unix::process::CommandExt;

    let exe = std::env::current_exe()?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let error = std::process::Command::new(exe)
        .args(args)
        .env("EXO_MCP_REPLAY_LINE", line)
        .exec();
    Err(error)
}

#[cfg(not(unix))]
fn reexec_mcp_with_replay(_line: &str) -> Result<std::convert::Infallible, std::io::Error> {
    Err(std::io::Error::other(
        "MCP self re-exec is not supported on this platform",
    ))
}

#[derive(Debug, Clone)]
struct McpExecutableGuard {
    path: PathBuf,
    identity: ExecutableIdentity,
}

impl McpExecutableGuard {
    fn capture() -> std::io::Result<Option<Self>> {
        let path = std::env::current_exe()?;
        match std::fs::metadata(&path) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        }

        Ok(Some(Self {
            identity: ExecutableIdentity::for_path(&path)?,
            path,
        }))
    }

    fn is_stale(&self) -> bool {
        self.identity
            .metadata_matches_path(&self.path)
            .map_or(true, |matches| !matches)
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
pub struct ExecutableIdentity {
    len: u64,
    modified_unix_ms: Option<u64>,
    stable_hash: String,
    #[cfg(unix)]
    dev: u64,
    #[cfg(unix)]
    ino: u64,
    #[cfg(unix)]
    changed_unix_s: i64,
    #[cfg(unix)]
    changed_unix_ns: i64,
}

impl ExecutableIdentity {
    fn for_path(path: &Path) -> std::io::Result<Self> {
        let metadata = std::fs::metadata(path)?;
        Ok(Self {
            len: metadata.len(),
            modified_unix_ms: metadata_modified_unix_ms(&metadata),
            stable_hash: stable_file_hash(path)?,
            #[cfg(unix)]
            dev: metadata_dev(&metadata),
            #[cfg(unix)]
            ino: metadata_ino(&metadata),
            #[cfg(unix)]
            changed_unix_s: metadata_changed_unix_s(&metadata),
            #[cfg(unix)]
            changed_unix_ns: metadata_changed_unix_ns(&metadata),
        })
    }

    /// Returns whether cheap filesystem metadata still matches this identity.
    ///
    /// This intentionally avoids hashing the file on hot request paths. Call
    /// `executable_identity_matches_path` when weaker platform metadata should
    /// fall back to validating `stable_hash`.
    pub fn metadata_matches_path(&self, path: &Path) -> std::io::Result<bool> {
        Ok(!matches!(
            self.metadata_match_strength(path)?,
            MetadataMatchStrength::Mismatch
        ))
    }

    fn metadata_match_strength(&self, path: &Path) -> std::io::Result<MetadataMatchStrength> {
        let metadata = std::fs::metadata(path)?;
        if self.len != metadata.len()
            || self.modified_unix_ms != metadata_modified_unix_ms(&metadata)
        {
            return Ok(MetadataMatchStrength::Mismatch);
        }

        #[cfg(unix)]
        {
            if self.dev == metadata_dev(&metadata)
                && self.ino == metadata_ino(&metadata)
                && self.changed_unix_s == metadata_changed_unix_s(&metadata)
                && self.changed_unix_ns == metadata_changed_unix_ns(&metadata)
            {
                Ok(MetadataMatchStrength::Strong)
            } else {
                Ok(MetadataMatchStrength::Mismatch)
            }
        }
        #[cfg(not(unix))]
        {
            Ok(MetadataMatchStrength::Weak)
        }
    }
}

enum MetadataMatchStrength {
    Mismatch,
    #[cfg(unix)]
    Strong,
    #[cfg(not(unix))]
    Weak,
}

pub fn executable_identity_for_path(path: &Path) -> std::io::Result<ExecutableIdentity> {
    ExecutableIdentity::for_path(path)
}

pub fn executable_identity_matches_path(
    identity: &ExecutableIdentity,
    path: &Path,
) -> std::io::Result<bool> {
    match identity.metadata_match_strength(path)? {
        MetadataMatchStrength::Mismatch => Ok(false),
        #[cfg(unix)]
        MetadataMatchStrength::Strong => Ok(true),
        #[cfg(not(unix))]
        MetadataMatchStrength::Weak => Ok(identity.stable_hash == stable_file_hash(path)?),
    }
}

fn stable_file_hash(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
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

fn metadata_modified_unix_ms(metadata: &std::fs::Metadata) -> Option<u64> {
    metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|duration| duration.as_millis().try_into().ok())
}

#[cfg(unix)]
fn metadata_dev(metadata: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    metadata.dev()
}

#[cfg(unix)]
fn metadata_ino(metadata: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    metadata.ino()
}

#[cfg(unix)]
fn metadata_changed_unix_s(metadata: &std::fs::Metadata) -> i64 {
    use std::os::unix::fs::MetadataExt;
    metadata.ctime()
}

#[cfg(unix)]
fn metadata_changed_unix_ns(metadata: &std::fs::Metadata) -> i64 {
    use std::os::unix::fs::MetadataExt;
    metadata.ctime_nsec()
}

fn worker_identity(workspace_root: &Path, project: Option<&Project>) -> anyhow::Result<JsonValue> {
    let executable_path = std::env::current_exe()?;
    let command_spec = CommandSpec::from_registry(&default_registry());
    let command_spec_value = serde_json::to_value(&command_spec)?;
    let exo_run_tool = exo_run_tool_definition();

    Ok(json!({
        "executable_path": executable_path,
        "executable_identity": executable_identity_for_path(&executable_path)?,
        "workspace_root": workspace_root,
        "project_id": project.map(|project| project.id.as_str()),
        "git_common_dir": project.map(|project| project.git_common_dir.as_path()),
        "state_policy": project.map(|project| project.policy.as_str()),
        "state_root": project.map(|project| project.state_root.as_path()),
        "database_path": project.map(Project::db_path),
        "sidecar_key": project.and_then(|project| project.sidecar_key.as_deref()),
        "sidecar_root": project.and_then(|project| project.sidecar_root.as_deref()),
        "worker_protocol_version": MCP_WORKER_PROTOCOL_VERSION,
        "tool_schema_identity": stable_json_hash(&exo_run_tool)?,
        "command_spec_identity": stable_json_hash(&command_spec_value)?,
    }))
}

fn stable_json_hash(value: &JsonValue) -> anyhow::Result<String> {
    Ok(blake3::hash(&serde_json::to_vec(value)?)
        .to_hex()
        .to_string())
}

fn mcp_reexec_error(
    id: JsonValue,
    error: &std::io::Error,
    guard: Option<&McpExecutableGuard>,
) -> JsonValue {
    let details = guard.map(|guard| {
        json!({
            "path": guard.path,
            "reason": "binary_changed",
            "reexec_error": error.to_string()
        })
    });
    json_rpc_error(
        id,
        -32000,
        "Exo MCP server binary changed on disk and automatic restart failed.",
        details,
    )
}

fn handle_json_rpc_value(
    workspace_root: &Path,
    project: Option<&Project>,
    value: JsonValue,
    worker_mode: bool,
    worker_call_cache: &mut WorkerCallCache,
) -> Option<JsonValue> {
    if value.is_array() {
        return Some(json_rpc_error(
            JsonValue::Null,
            -32600,
            "Invalid request",
            Some(json!({ "message": "JSON-RPC batches are not supported" })),
        ));
    }

    let has_id = value.get("id").is_some();
    let id = value.get("id").cloned().unwrap_or(JsonValue::Null);
    let message = match serde_json::from_value::<JsonRpcMessage>(value) {
        Ok(message) => message,
        Err(error) => {
            return Some(json_rpc_error(
                id,
                -32600,
                "Invalid request",
                Some(json!({ "message": error.to_string() })),
            ));
        }
    };

    let Some(method) = message.method.as_deref() else {
        return Some(json_rpc_error(id, -32600, "Invalid request", None));
    };

    let is_notification = !has_id;
    if is_notification {
        return None;
    }

    match method {
        "worker/hello" if worker_mode => Some(match worker_identity(workspace_root, project) {
            Ok(identity) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "identity": identity },
            }),
            Err(error) => json_rpc_error(
                id,
                -32000,
                "Exo MCP worker identity is unavailable.",
                Some(json!({ "message": error.to_string() })),
            ),
        }),
        "worker/listTools" if worker_mode => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "tools": [exo_run_tool_definition()] },
        })),
        "worker/classify" if worker_mode => Some(worker_classify_tools_call(
            workspace_root,
            id,
            message.params,
            worker_call_cache,
        )),
        "worker/call" if worker_mode => handle_tools_call(
            workspace_root,
            project,
            id,
            message.params,
            Some(worker_call_cache),
        ),
        "worker/status" if worker_mode => Some(match worker_identity(workspace_root, project) {
            Ok(identity) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "identity": identity,
                    "worker_protocol_version": MCP_WORKER_PROTOCOL_VERSION,
                },
            }),
            Err(error) => json_rpc_error(
                id,
                -32000,
                "Exo MCP worker status is unavailable.",
                Some(json!({ "message": error.to_string() })),
            ),
        }),
        "initialize" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": initialize_result(message.params.as_ref()),
        })),
        "ping" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {},
        })),
        "tools/list" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "tools": [exo_run_tool_definition()] },
        })),
        "tools/call" => handle_tools_call(workspace_root, project, id, message.params, None),
        _ => Some(json_rpc_error(id, -32601, "Method not found", None)),
    }
}

fn initialize_result(params: Option<&JsonValue>) -> JsonValue {
    let requested = params
        .and_then(|params| params.get("protocolVersion"))
        .and_then(JsonValue::as_str)
        .unwrap_or(MCP_PROTOCOL_VERSION);
    let protocol_version = if requested == MCP_PROTOCOL_VERSION {
        requested
    } else {
        MCP_PROTOCOL_VERSION
    };

    json!({
        "protocolVersion": protocol_version,
        "capabilities": {
            "tools": {
                "listChanged": false
            }
        },
        "serverInfo": {
            "name": "exo",
            "title": "Exo",
            "version": env!("CARGO_PKG_VERSION")
        },
        "instructions": "Use exo-run with Exo CLI syntax, without the leading exo. This is not a shell runner."
    })
}

fn handle_tools_call(
    workspace_root: &Path,
    project: Option<&Project>,
    id: JsonValue,
    params: Option<JsonValue>,
    worker_call_cache: Option<&mut WorkerCallCache>,
) -> Option<JsonValue> {
    if let Some(cache) = worker_call_cache
        && let Some(prepared) = cache.take(&params)
    {
        let result = call_prepared_exo_run_tool(workspace_root, project, prepared);
        return Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        }));
    }

    let input = match exo_run_input_from_tools_call_params(&id, params.clone()) {
        Ok(input) => input,
        Err(error) => return Some(error),
    };

    let request_id = tool_call_request_id(&id, params.as_ref());
    let result = call_exo_run_tool_with_request_id(workspace_root, project, input, request_id);
    Some(json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    }))
}

fn worker_classify_tools_call(
    workspace_root: &Path,
    id: JsonValue,
    params: Option<JsonValue>,
    worker_call_cache: &mut WorkerCallCache,
) -> JsonValue {
    let input = match exo_run_input_from_tools_call_params(&id, params.clone()) {
        Ok(input) => input,
        Err(error) => return error,
    };
    let command = input.command.clone();
    let has_auth = input.auth.is_some();
    let has_workflow_confirmation = input.workflow_confirmation.is_some();

    let request_id = tool_call_request_id(&id, params.as_ref());
    let prepared = match prepare_exo_run_tool_call(workspace_root, input, request_id) {
        Ok(prepared) => prepared,
        Err(response) => {
            return worker_tool_error_classification(
                id,
                command,
                has_auth,
                has_workflow_confirmation,
                &response,
            );
        }
    };
    let effect = prepared.effect;
    if let Err(error) = worker_call_cache.store(&params, prepared) {
        return json_rpc_error(
            id,
            -32000,
            "Unable to cache classified tool call",
            Some(json!({ "message": error.to_string() })),
        );
    }

    let identities = match worker_protocol_identities() {
        Ok(identities) => identities,
        Err(error) => {
            return json_rpc_error(
                id,
                -32000,
                "Exo MCP worker classification metadata is unavailable.",
                Some(json!({ "message": error.to_string() })),
            );
        }
    };

    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "tool_name": EXO_RUN_TOOL_NAME,
            "effect": effect,
            "retry_policy": retry_policy_for_effect(effect),
            "requires_confirmation": effect == Effect::Exec,
            "request_summary": {
                "tool_name": EXO_RUN_TOOL_NAME,
                "command": command,
            },
            "has_auth": has_auth,
            "has_workflow_confirmation": has_workflow_confirmation,
            "tool_schema_identity": identities.tool_schema_identity,
            "command_spec_identity": identities.command_spec_identity,
        },
    })
}

fn worker_tool_error_classification(
    id: JsonValue,
    command: String,
    has_auth: bool,
    has_workflow_confirmation: bool,
    response: &ResponseEnvelope,
) -> JsonValue {
    let identities = match worker_protocol_identities() {
        Ok(identities) => identities,
        Err(error) => {
            return json_rpc_error(
                id,
                -32000,
                "Exo MCP worker classification metadata is unavailable.",
                Some(json!({ "message": error.to_string() })),
            );
        }
    };

    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "tool_name": EXO_RUN_TOOL_NAME,
            "classification": "tool_error",
            "effect": Effect::Pure,
            "retry_policy": "no_retry",
            "requires_confirmation": false,
            "request_summary": {
                "tool_name": EXO_RUN_TOOL_NAME,
                "command": command,
            },
            "has_auth": has_auth,
            "has_workflow_confirmation": has_workflow_confirmation,
            "tool_result": machine_response_to_tool_result(response),
            "tool_schema_identity": identities.tool_schema_identity,
            "command_spec_identity": identities.command_spec_identity,
        },
    })
}

fn tool_call_request_id(_id: &JsonValue, params: Option<&JsonValue>) -> String {
    if let Some(request_id) = params
        .and_then(|params| params.get(MCP_OUTCOME_REQUEST_ID_PARAM))
        .and_then(JsonValue::as_str)
    {
        return format!("mcp.exo-run.{request_id}");
    }
    format!("mcp.exo-run.{}", Uuid::new_v4())
}

fn exo_run_input_from_tools_call_params(
    id: &JsonValue,
    params: Option<JsonValue>,
) -> Result<ExoRunInput, JsonValue> {
    let Some(params) = params else {
        return Err(json_rpc_error(id.clone(), -32602, "Invalid params", None));
    };
    let call = match serde_json::from_value::<McpToolsCallParams>(params) {
        Ok(call) => call,
        Err(error) => {
            return Err(json_rpc_error(
                id.clone(),
                -32602,
                "Invalid params",
                Some(json!({ "message": error.to_string() })),
            ));
        }
    };
    if call.name != EXO_RUN_TOOL_NAME {
        return Err(json_rpc_error(
            id.clone(),
            -32602,
            "Unknown tool",
            Some(json!({ "name": call.name })),
        ));
    }

    let arguments = call.arguments.unwrap_or_else(|| json!({}));
    serde_json::from_value::<ExoRunInput>(arguments).map_err(|error| {
        json_rpc_error(
            id.clone(),
            -32602,
            "Invalid tool arguments",
            Some(json!({ "message": error.to_string() })),
        )
    })
}

fn prepare_exo_run_tool_call(
    workspace_root: &Path,
    input: ExoRunInput,
    request_id: String,
) -> Result<PreparedExoRunCall, ResponseEnvelope> {
    let include_structured = explicit_json_output_requested(&input);
    let compiled = compile_exo_run_input(input, request_id)?;
    let preload_required = compiled.requires_workspace_preload();
    if preload_required {
        ensure_workspace_context_loaded(
            workspace_root,
            compiled.request.id.clone(),
            &compiled.original_command,
        )?;
    }
    let effect = effect_for_compiled_request(workspace_root, &compiled).map_err(|message| {
        error_response(
            compiled.request.id.clone(),
            ErrorCode::Internal,
            message,
            Some(json!({ "classification": "unavailable" })),
        )
    })?;

    Ok(PreparedExoRunCall {
        request: compiled.request,
        original_command: compiled.original_command,
        include_structured,
        effect,
        preload_required,
    })
}

fn ensure_workspace_context_loaded(
    workspace_root: &Path,
    request_id: String,
    original_command: &str,
) -> Result<(), ResponseEnvelope> {
    AgentContext::load(workspace_root.to_path_buf())
        .map(|_| ())
        .map_err(|error| {
            context_load_error_response(workspace_root, request_id, original_command, error)
        })
}

fn context_load_error_response(
    workspace_root: &Path,
    request_id: String,
    original_command: &str,
    error: anyhow::Error,
) -> ResponseEnvelope {
    if let Some(guidance) =
        crate::preload_guidance::classify_context_load_error(&error, original_command)
    {
        return ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            id: request_id,
            status: Status::Error,
            result: None,
            error: Some(ErrorBody {
                code: guidance.error_code,
                message: guidance.message(),
                details: Some(guidance.details()),
            }),
            ticket: None,
            steering: Some(guidance.to_steering()),
            reminders: None,
            display: None,
            preview: None,
            effect: None,
            trace: None,
        };
    }

    error_response(
        request_id,
        ErrorCode::PreconditionFailed,
        format!("Failed to load agent context: {error}"),
        Some(json!({
            "classification": "context_load_failed",
            "workspace_root": workspace_root,
        })),
    )
}

fn effect_for_compiled_request(
    workspace_root: &Path,
    compiled: &CompiledExoRunRequest,
) -> Result<Effect, String> {
    match (&compiled.request.op, compiled.invocation.as_ref()) {
        (Op::Help(_) | Op::List(_), _) => Ok(Effect::Pure),
        (Op::Call(params) | Op::Preview(params), Some(invocation)) => {
            match build_command_from_invocation(invocation, workspace_root) {
                Ok(Some(command)) => Ok(command.effect()),
                Ok(None) => Err(format!(
                    "operation is not available via machine channel: {:?}",
                    params.address
                )),
                Err(error) => Err(error.to_string()),
            }
        }
        (Op::Call(params) | Op::Preview(params), None) => effect_for_address(&params.address),
    }
}

fn effect_for_address(address: &Address) -> Result<Effect, String> {
    let spec = CommandSpec::from_registry(&default_registry());
    let (namespace, operation) = match address {
        Address::Operation { path } if path.len() == 1 => ("", path[0].as_str()),
        Address::Operation { path } if path.len() == 2 => (path[0].as_str(), path[1].as_str()),
        other => {
            return Err(format!(
                "cannot classify non-operation command address: {other:?}"
            ));
        }
    };
    spec.operation(namespace, operation)
        .map(|operation| operation.effect)
        .ok_or_else(|| format!("operation not found for classification: {address:?}"))
}

const fn retry_policy_for_effect(effect: Effect) -> &'static str {
    match effect {
        Effect::Pure => "auto_retry_read",
        Effect::Write | Effect::Exec => "auto_recover_outcome",
    }
}

#[derive(Debug, Clone)]
struct WorkerProtocolIdentities {
    tool_schema_identity: String,
    command_spec_identity: String,
}

fn worker_protocol_identities() -> anyhow::Result<WorkerProtocolIdentities> {
    static WORKER_PROTOCOL_IDENTITIES: OnceLock<WorkerProtocolIdentities> = OnceLock::new();
    if let Some(cached) = WORKER_PROTOCOL_IDENTITIES.get() {
        return Ok(cached.clone());
    }

    let command_spec = CommandSpec::from_registry(&default_registry());
    let identities = WorkerProtocolIdentities {
        tool_schema_identity: stable_json_hash(&exo_run_tool_definition())?,
        command_spec_identity: stable_json_hash(&serde_json::to_value(command_spec)?)?,
    };
    let _ = WORKER_PROTOCOL_IDENTITIES.set(identities.clone());
    Ok(identities)
}

pub fn call_exo_run_tool(
    workspace_root: &Path,
    project: Option<&Project>,
    input: ExoRunInput,
) -> McpToolResult {
    let request_id = format!("mcp.exo-run.{}", Uuid::new_v4());
    call_exo_run_tool_with_request_id(workspace_root, project, input, request_id)
}

fn call_exo_run_tool_with_request_id(
    workspace_root: &Path,
    project: Option<&Project>,
    input: ExoRunInput,
    request_id: String,
) -> McpToolResult {
    let include_structured = explicit_json_output_requested(&input);
    let compiled = match compile_exo_run_input(input, request_id) {
        Ok(compiled) => compiled,
        Err(response) => return machine_response_to_tool_result(&response),
    };
    if compiled.requires_workspace_preload()
        && let Err(response) = ensure_workspace_context_loaded(
            workspace_root,
            compiled.request.id.clone(),
            &compiled.original_command,
        )
    {
        return machine_response_to_tool_result(&response);
    }

    let mut response =
        handler::handle_request_with_project(workspace_root, project, compiled.request);
    let reminders = crate::verifiers::run_global_verifiers(workspace_root);
    if !reminders.is_empty() {
        response.reminders = Some(reminders);
    }

    machine_response_to_tool_result_with_profile(&response, include_structured)
}

fn call_prepared_exo_run_tool(
    workspace_root: &Path,
    project: Option<&Project>,
    prepared: PreparedExoRunCall,
) -> McpToolResult {
    if prepared.preload_required
        && let Err(response) = ensure_workspace_context_loaded(
            workspace_root,
            prepared.request.id.clone(),
            &prepared.original_command,
        )
    {
        return machine_response_to_tool_result(&response);
    }

    let mut response =
        handler::handle_request_with_project(workspace_root, project, prepared.request);
    let reminders = crate::verifiers::run_global_verifiers(workspace_root);
    if !reminders.is_empty() {
        response.reminders = Some(reminders);
    }

    machine_response_to_tool_result_with_profile(&response, prepared.include_structured)
}

pub fn build_exo_run_request(
    input: ExoRunInput,
    request_id: String,
) -> Result<RequestEnvelope, ResponseEnvelope> {
    Ok(compile_exo_run_input(input, request_id)?.request)
}

#[derive(Debug, Clone)]
struct CompiledExoRunRequest {
    request: RequestEnvelope,
    invocation: Option<Invocation>,
    original_command: String,
}

impl CompiledExoRunRequest {
    fn requires_workspace_preload(&self) -> bool {
        self.invocation
            .as_ref()
            .is_some_and(|invocation| !invocation_uses_lightweight_context(invocation))
    }
}

fn invocation_uses_lightweight_context(invocation: &Invocation) -> bool {
    match (invocation.namespace(), invocation.operation()) {
        ("", "update") => true,
        ("project", "resolve" | "list" | "snapshot" | "repair" | "repair-apply" | "move-root") => {
            true
        }
        ("sidecar", "bootstrap" | "discover" | "init" | "link" | "setup" | "status" | "unlink") => {
            true
        }
        _ => false,
    }
}

fn normalize_project_repair_apply_shorthand(tokens: &mut Vec<String>) {
    if tokens.first().map(String::as_str) != Some("project")
        || tokens.get(1).map(String::as_str) != Some("repair")
        || !tokens.iter().any(|token| token == "--apply")
    {
        return;
    }

    tokens[1] = "repair-apply".to_string();
    tokens.retain(|token| token != "--apply");
}

fn compile_exo_run_input(
    input: ExoRunInput,
    request_id: String,
) -> Result<CompiledExoRunRequest, ResponseEnvelope> {
    let original_command = input.command.clone();
    let parsed = match parse_command_text(&input.command, &input.args) {
        Ok(parsed) => parsed,
        Err(error) => {
            return Err(error_response(
                request_id,
                ErrorCode::InvalidInput,
                error,
                None,
            ));
        }
    };
    let mut tokens = parsed.tokens;
    normalize_project_repair_apply_shorthand(&mut tokens);

    if tokens.is_empty() {
        return Err(error_response(
            request_id,
            ErrorCode::InvalidInput,
            "Empty command".to_string(),
            None,
        ));
    }

    let op = match parsed.intent {
        CommandTextIntent::Help { target } => Op::Help(HelpParams {
            address: help_address_for_tokens(&target),
        }),
        CommandTextIntent::Call if namespace_only_help_target(&tokens) => Op::Help(HelpParams {
            address: help_address_for_tokens(&tokens),
        }),
        CommandTextIntent::Call => {
            let spec = CommandSpec::from_registry(&default_registry());
            let compilation = compile_argv(&spec, &tokens);
            let Some(invocation) = compilation.invocation else {
                return Err(compilation_error_response(
                    request_id,
                    &tokens,
                    &compilation,
                ));
            };

            let address = invocation_to_address(&invocation);
            let call_input = invocation.to_json_input();
            return Ok(CompiledExoRunRequest {
                request: RequestEnvelope {
                    protocol_version: PROTOCOL_VERSION,
                    id: request_id,
                    op: Op::Call(CallParams {
                        address,
                        input: call_input,
                    }),
                    auth: input.auth,
                    workflow_confirmation: input.workflow_confirmation.map(Into::into),
                    agent_id: None,
                },
                invocation: Some(invocation),
                original_command,
            });
        }
    };

    Ok(CompiledExoRunRequest {
        request: RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            id: request_id,
            op,
            auth: input.auth,
            workflow_confirmation: input.workflow_confirmation.map(Into::into),
            agent_id: None,
        },
        invocation: None,
        original_command,
    })
}

fn params_cache_key(params: &Option<JsonValue>) -> anyhow::Result<String> {
    let bytes = serde_json::to_vec(params)?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

fn namespace_only_help_target(tokens: &[String]) -> bool {
    if tokens.len() == 1 {
        let spec = CommandSpec::from_registry(&default_registry());
        if spec.namespaces.contains_key(&tokens[0]) {
            return true;
        }
    }
    false
}

fn explicit_json_output_requested(input: &ExoRunInput) -> bool {
    parse_command_text(&input.command, &input.args)
        .is_ok_and(|parsed| tokens_request_json_output(&parsed.tokens))
}

fn help_address_for_tokens(tokens: &[String]) -> Address {
    if tokens.is_empty() {
        return Address::Root;
    }

    let spec = CommandSpec::from_registry(&default_registry());
    if tokens.len() == 1 {
        if spec.namespaces.contains_key(&tokens[0]) {
            Address::Namespace {
                path: vec![tokens[0].clone()],
            }
        } else {
            Address::Operation {
                path: vec![tokens[0].clone()],
            }
        }
    } else {
        Address::Operation {
            path: vec![tokens[0].clone(), tokens[1..].join(".")],
        }
    }
}

fn invocation_to_address(invocation: &crate::command::router::Invocation) -> Address {
    let path = &invocation.path;
    if path.namespace.is_empty() {
        Address::Operation {
            path: vec![path.operation.clone()],
        }
    } else {
        Address::Operation {
            path: vec![path.namespace.clone(), path.operation.clone()],
        }
    }
}

fn compilation_error_response(
    id: String,
    argv: &[String],
    compilation: &Compilation,
) -> ResponseEnvelope {
    let code = compilation
        .diagnostics
        .first()
        .map_or(ErrorCode::InvalidInput, diagnostic_error_code);

    let message = compilation.diagnostics.first().map_or_else(
        || "Invalid command".to_string(),
        |diag| diag.message.clone(),
    );

    ResponseEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id,
        status: Status::Error,
        result: None,
        error: Some(ErrorBody {
            code,
            message,
            details: Some(json!({
                "argv": argv,
                "diagnostics": compilation.diagnostics,
            })),
        }),
        ticket: None,
        steering: compilation.steering.clone(),
        reminders: None,
        display: None,
        preview: None,
        effect: None,
        trace: None,
    }
}

const fn diagnostic_error_code(diagnostic: &Diagnostic) -> ErrorCode {
    match diagnostic.code {
        DiagnosticCode::MissingRequired | DiagnosticCode::MissingValue => ErrorCode::MissingArg,
        DiagnosticCode::InvalidValue => ErrorCode::TypeMismatch,
        DiagnosticCode::AmbiguousSubcommand => ErrorCode::UnknownCommand,
        DiagnosticCode::ShellOperator
        | DiagnosticCode::UnknownFlag
        | DiagnosticCode::InvalidFlag
        | DiagnosticCode::TooManyPositionals
        | DiagnosticCode::NonRepeatable => ErrorCode::InvalidInput,
    }
}

const fn error_response(
    id: String,
    code: ErrorCode,
    message: String,
    details: Option<JsonValue>,
) -> ResponseEnvelope {
    ResponseEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id,
        status: Status::Error,
        result: None,
        error: Some(ErrorBody {
            code,
            message,
            details,
        }),
        ticket: None,
        steering: None,
        reminders: None,
        display: None,
        preview: None,
        effect: None,
        trace: None,
    }
}

fn machine_response_to_tool_result(response: &ResponseEnvelope) -> McpToolResult {
    machine_response_to_tool_result_with_profile(response, false)
}

fn machine_response_to_tool_result_with_profile(
    response: &ResponseEnvelope,
    include_structured: bool,
) -> McpToolResult {
    let text = format_machine_response_text(response);
    McpToolResult {
        content: vec![McpContent::Text { text }],
        structured_content: structured_content_for_tool_response(response, include_structured),
        is_error: response.status != Status::Ok,
    }
}

fn structured_content_for_tool_response(
    response: &ResponseEnvelope,
    include_structured: bool,
) -> Option<JsonValue> {
    (include_structured || response.status != Status::Ok || response.ticket.is_some())
        .then(|| structured_content_for_response(response))
}

fn structured_content_for_response(response: &ResponseEnvelope) -> JsonValue {
    let mut value = json!({
        "protocol_version": response.protocol_version,
        "id": response.id,
        "status": response.status,
    });

    if let Some(effect) = response.effect {
        value["effect"] = json!(effect);
    }

    if let Some(display) = &response.display {
        value["display"] = compact_display(display);
    }

    if let Some(preview) = &response.preview {
        value["preview"] = serde_json::to_value(preview).unwrap_or_else(|_| json!({}));
    }

    if let Some(result) = &response.result {
        value["result"] = if is_help_result(result) {
            compact_help_result(result)
        } else {
            compact_json_value(result, 0)
        };
    }

    if let Some(error) = &response.error {
        value["error"] = compact_error_body(error);
    }

    if let Some(ticket) = &response.ticket {
        value["ticket"] = json!(ticket);
    }

    if let Some(steering) = &response.steering {
        value["steering"] = compact_json_value(
            &serde_json::to_value(steering).unwrap_or_else(|_| json!({})),
            0,
        );
    }

    if let Some(reminders) = &response.reminders
        && !reminders.is_empty()
    {
        value["reminders"] = compact_json_value(
            &serde_json::to_value(reminders).unwrap_or_else(|_| json!([])),
            0,
        );
    }

    normalize_workflow_confirmation_details(&mut value);
    value
}

fn compact_display(display: &Display) -> JsonValue {
    json!({
        "invocation_message": display.invocation_message,
        "summary": display.summary,
    })
}

fn compact_error_body(error: &ErrorBody) -> JsonValue {
    let mut value = json!({
        "code": error.code,
        "message": error.message,
    });

    let Some(details) = &error.details else {
        return value;
    };

    let mut compact_details = json!({});
    if let Some(workflow) = workflow_confirmation_from_details(Some(details)) {
        compact_details["workflow_confirmation"] = workflow.clone();
    }
    if let Some(diagnostics) = details.get("diagnostics") {
        compact_details["diagnostics"] = compact_diagnostics(diagnostics);
    }
    if let Some(argv) = details.get("argv") {
        compact_details["argv"] = compact_json_value(argv, 0);
    }
    if compact_details
        .as_object()
        .is_some_and(|object| !object.is_empty())
    {
        value["details"] = compact_details;
    }

    value
}

fn compact_diagnostics(value: &JsonValue) -> JsonValue {
    let Some(diagnostics) = value.as_array() else {
        return compact_json_value(value, 0);
    };

    JsonValue::Array(
        diagnostics
            .iter()
            .take(8)
            .map(|diagnostic| {
                let mut compact = json!({});
                if let Some(code) = diagnostic.get("code") {
                    compact["code"] = code.clone();
                }
                if let Some(message) = diagnostic.get("message") {
                    compact["message"] = message.clone();
                }
                if let Some(suggestions) =
                    diagnostic.get("suggestions").and_then(JsonValue::as_array)
                {
                    compact["suggestions"] = JsonValue::Array(
                        suggestions
                            .iter()
                            .take(5)
                            .map(compact_suggestion)
                            .collect::<Vec<_>>(),
                    );
                }
                compact
            })
            .collect(),
    )
}

fn compact_suggestion(suggestion: &JsonValue) -> JsonValue {
    let mut compact = json!({});
    if let Some(label) = suggestion.get("label") {
        compact["label"] = label.clone();
    }
    if let Some(replacement) = suggestion.get("replacement").and_then(JsonValue::as_str) {
        compact["replacement"] = json!(agent_facing_command(replacement));
    }
    compact
}

fn compact_help_result(result: &JsonValue) -> JsonValue {
    let mut compact = json!({});
    if let Some(title) = result.get("title") {
        compact["title"] = title.clone();
    }
    if let Some(summary) = result.get("summary") {
        compact["summary"] = summary.clone();
    }

    if let Some(namespaces) = result.get("namespaces").and_then(JsonValue::as_array) {
        compact["namespaces"] = JsonValue::Array(
            namespaces
                .iter()
                .take(20)
                .map(|namespace| {
                    json!({
                        "path": namespace.get("path").cloned().unwrap_or_else(|| json!([])),
                        "summary": namespace.get("summary").cloned().unwrap_or(JsonValue::Null),
                    })
                })
                .collect(),
        );
    }

    if let Some(operations) = result.get("operations").and_then(JsonValue::as_array) {
        compact["operations"] = JsonValue::Array(
            operations
                .iter()
                .take(20)
                .map(compact_help_operation)
                .collect(),
        );
    }

    if let Some(next_calls) = result.get("next_calls").and_then(JsonValue::as_array)
        && !next_calls.is_empty()
    {
        compact["next_calls"] = JsonValue::Array(
            next_calls
                .iter()
                .take(5)
                .map(compact_help_next_call)
                .collect(),
        );
    }

    compact
}

fn compact_help_operation(operation: &JsonValue) -> JsonValue {
    let path = operation
        .get("path")
        .and_then(JsonValue::as_str)
        .unwrap_or("");
    let args = operation
        .get("args")
        .and_then(JsonValue::as_array)
        .cloned()
        .unwrap_or_default();

    let mut compact = json!({
        "path": path,
        "effect": operation.get("effect").cloned().unwrap_or(JsonValue::Null),
        "summary": operation.get("summary").cloned().unwrap_or(JsonValue::Null),
    });

    if !args.is_empty() {
        compact["usage"] = json!(help_usage(path, &args));
        compact["args"] = JsonValue::Array(args.iter().map(compact_help_arg).collect());
    }

    compact
}

fn compact_help_arg(arg: &JsonValue) -> JsonValue {
    let mut compact = json!({});
    for key in [
        "id",
        "name",
        "kind",
        "description",
        "optional",
        "default",
        "short",
        "repeatable",
    ] {
        if let Some(value) = arg.get(key)
            && !value.is_null()
        {
            compact[key] = value.clone();
        }
    }
    if let Some(value_type) = arg.get("value_type") {
        compact["value_type"] = json!(value_type_label_from_json(value_type));
    }
    compact
}

fn compact_help_next_call(next_call: &JsonValue) -> JsonValue {
    let mut compact = json!({});
    if let Some(kind) = next_call.get("kind") {
        compact["kind"] = kind.clone();
    }
    if let Some(address) = next_call
        .get("params")
        .and_then(|params| params.get("address"))
    {
        compact["address"] = compact_json_value(address, 0);
    }
    compact
}

fn compact_json_value(value: &JsonValue, depth: usize) -> JsonValue {
    const MAX_DEPTH: usize = 4;
    const MAX_ARRAY_ITEMS: usize = 20;
    const MAX_STRING_CHARS: usize = 4000;

    if depth >= MAX_DEPTH {
        return match value {
            JsonValue::Array(items) => json!({
                "kind": "array",
                "count": items.len(),
                "truncated": true,
            }),
            JsonValue::Object(object) => json!({
                "kind": "object",
                "keys": object.len(),
                "truncated": true,
            }),
            JsonValue::String(text) => compact_string(text, MAX_STRING_CHARS),
            _ => value.clone(),
        };
    }

    match value {
        JsonValue::Array(items) => {
            let mut compact_items = items
                .iter()
                .take(MAX_ARRAY_ITEMS)
                .map(|item| compact_json_value(item, depth + 1))
                .collect::<Vec<_>>();
            if items.len() > MAX_ARRAY_ITEMS {
                compact_items.push(json!({
                    "truncated": true,
                    "remaining": items.len() - MAX_ARRAY_ITEMS,
                }));
            }
            JsonValue::Array(compact_items)
        }
        JsonValue::Object(object) => {
            let mut compact = serde_json::Map::new();
            for (key, nested) in object {
                if is_bulky_machine_key(key) {
                    continue;
                }
                compact.insert(key.clone(), compact_json_value(nested, depth + 1));
            }
            JsonValue::Object(compact)
        }
        JsonValue::String(text) => compact_string(text, MAX_STRING_CHARS),
        _ => value.clone(),
    }
}

fn compact_string(text: &str, max_chars: usize) -> JsonValue {
    if text.chars().count() <= max_chars {
        return json!(text);
    }

    let prefix = text.chars().take(max_chars).collect::<String>();
    json!({
        "text": prefix,
        "truncated": true,
        "original_chars": text.chars().count(),
    })
}

fn is_bulky_machine_key(key: &str) -> bool {
    matches!(
        key,
        "trace"
            | "steering"
            | "dependencies"
            | "resources"
            | "cell_id"
            | "revision"
            | "revision_before"
            | "revision_after"
    )
}

fn normalize_workflow_confirmation_details(value: &mut JsonValue) {
    let Some(details) = value
        .get_mut("error")
        .and_then(|error| error.get_mut("details"))
    else {
        return;
    };

    if details.get("workflow_confirmation").is_some() {
        return;
    }

    let Some(workflow) = details
        .get("details")
        .and_then(|nested| nested.get("workflow_confirmation"))
        .cloned()
    else {
        return;
    };

    let is_completion_confirmation = workflow
        .get("kind")
        .and_then(JsonValue::as_str)
        .is_some_and(|kind| kind == "workflow_completion_confirmation");
    if !is_completion_confirmation {
        return;
    }

    if let Some(details) = details.as_object_mut() {
        details.insert("workflow_confirmation".to_string(), workflow);
    }
}

fn format_machine_response_text(response: &ResponseEnvelope) -> String {
    let mut text = match response.status {
        Status::Ok => format_ok_response_text(response),
        Status::ConfirmRequired => {
            "Execution confirmation required.\n\nAsk the human whether to approve this action. If they approve, continue with the confirmed action.".to_string()
        }
        Status::NeedsInput => "Command needs additional input.".to_string(),
        Status::Error => format_error_response_text(response),
    };

    if let Some(reminders) = &response.reminders
        && !reminders.is_empty()
    {
        text.push_str("\n\n---\nReminders:\n");
        for reminder in reminders {
            text.push_str(&format!(
                "- [{:?}] {}\n",
                reminder.severity, reminder.message
            ));
        }
    }

    text
}

fn format_ok_response_text(response: &ResponseEnvelope) -> String {
    if let Some(Display {
        body: Some(body), ..
    }) = &response.display
    {
        return body.clone();
    }

    if let Some(display) = &response.display {
        return display.summary.clone();
    }

    if let Some(result) = &response.result {
        if is_help_result(result) {
            return format_help_result(result);
        }
        return serde_json::to_string_pretty(result).unwrap_or_else(|_| "OK".to_string());
    }

    "OK".to_string()
}

fn format_error_response_text(response: &ResponseEnvelope) -> String {
    let Some(error) = &response.error else {
        return "Command failed.".to_string();
    };

    if let Some(workflow) = workflow_confirmation_from_details(error.details.as_ref()) {
        return format_workflow_confirmation(workflow);
    }

    let mut lines = vec![format!("Error: {}", error.message)];
    if let Some(details) = &error.details
        && let Some(diagnostics) = details.get("diagnostics").and_then(JsonValue::as_array)
    {
        for diagnostic in diagnostics {
            if let Some(suggestions) = diagnostic.get("suggestions").and_then(JsonValue::as_array) {
                for suggestion in suggestions {
                    let label = suggestion.get("label").and_then(JsonValue::as_str);
                    let replacement = suggestion.get("replacement").and_then(JsonValue::as_str);
                    if let (Some(label), Some(replacement)) = (label, replacement) {
                        lines.push(format!(
                            "Suggestion: {label} -> {}",
                            agent_facing_command(replacement)
                        ));
                    }
                }
            }
        }
    }

    lines.join("\n")
}

fn is_help_result(value: &JsonValue) -> bool {
    value.get("title").and_then(JsonValue::as_str).is_some()
        && value
            .get("operations")
            .and_then(JsonValue::as_array)
            .is_some()
        && value
            .get("namespaces")
            .and_then(JsonValue::as_array)
            .is_some()
}

fn format_help_result(value: &JsonValue) -> String {
    let mut lines = Vec::new();
    if let Some(title) = value.get("title").and_then(JsonValue::as_str) {
        lines.push(format!("# {title}"));
    }
    if let Some(summary) = value.get("summary").and_then(JsonValue::as_str) {
        lines.push(summary.to_string());
    }

    if let Some(namespaces) = value.get("namespaces").and_then(JsonValue::as_array)
        && !namespaces.is_empty()
    {
        lines.push(String::new());
        lines.push("## Namespaces".to_string());
        for namespace in namespaces {
            let path = namespace
                .get("path")
                .and_then(JsonValue::as_array)
                .map(|segments| {
                    segments
                        .iter()
                        .filter_map(JsonValue::as_str)
                        .collect::<Vec<_>>()
                        .join(".")
                })
                .unwrap_or_default();
            let summary = namespace
                .get("summary")
                .and_then(JsonValue::as_str)
                .unwrap_or("");
            lines.push(format!("- {path}: {summary}"));
        }
    }

    if let Some(operations) = value.get("operations").and_then(JsonValue::as_array)
        && !operations.is_empty()
    {
        lines.push(String::new());
        lines.push("## Operations".to_string());
        for operation in operations {
            let path = operation
                .get("path")
                .and_then(JsonValue::as_str)
                .unwrap_or("");
            let effect = operation
                .get("effect")
                .and_then(JsonValue::as_str)
                .unwrap_or("unknown");
            let summary = operation
                .get("summary")
                .and_then(JsonValue::as_str)
                .unwrap_or("");
            lines.push(format!("- {path} [{effect}]: {summary}"));

            if let Some(args) = operation.get("args").and_then(JsonValue::as_array) {
                if !args.is_empty() {
                    lines.push(format!("  Usage: {}", help_usage(path, args)));
                }
                for arg in args {
                    let name = arg
                        .get("name")
                        .or_else(|| arg.get("id"))
                        .and_then(JsonValue::as_str)
                        .unwrap_or("");
                    let kind = arg.get("kind").and_then(JsonValue::as_str).unwrap_or("");
                    let optional = arg
                        .get("optional")
                        .and_then(JsonValue::as_bool)
                        .unwrap_or(false);
                    let req = if optional { "optional" } else { "required" };
                    let description = arg
                        .get("description")
                        .and_then(JsonValue::as_str)
                        .unwrap_or("");
                    lines.push(format!("  - {name} ({kind}, {req}): {description}"));
                }
            }
        }
    }

    if lines.is_empty() {
        serde_json::to_string_pretty(value).unwrap_or_default()
    } else {
        lines.join("\n")
    }
}

fn help_usage(path: &str, args: &[JsonValue]) -> String {
    let mut usage = path.to_string();
    for arg in args {
        let Some(kind) = arg.get("kind").and_then(JsonValue::as_str) else {
            continue;
        };
        match kind {
            "positional" => {
                let name = arg
                    .get("name")
                    .or_else(|| arg.get("id"))
                    .and_then(JsonValue::as_str)
                    .unwrap_or("value");
                usage.push_str(&format!(" <{name}>"));
            }
            "flag" => {
                let name = arg
                    .get("name")
                    .or_else(|| arg.get("id"))
                    .and_then(JsonValue::as_str)
                    .unwrap_or("");
                let optional = arg
                    .get("optional")
                    .and_then(JsonValue::as_bool)
                    .unwrap_or(false);
                if optional {
                    usage.push_str(&format!(" [--{name}]"));
                } else {
                    usage.push_str(&format!(" --{name}"));
                }
            }
            "option" => {
                let name = arg
                    .get("name")
                    .or_else(|| arg.get("id"))
                    .and_then(JsonValue::as_str)
                    .unwrap_or("value");
                let type_label = arg
                    .get("value_type")
                    .map(value_type_label_from_json)
                    .unwrap_or_else(|| "value".to_string());
                let optional = arg
                    .get("optional")
                    .and_then(JsonValue::as_bool)
                    .unwrap_or(false);
                if optional {
                    usage.push_str(&format!(" [--{name} <{type_label}>]"));
                } else {
                    usage.push_str(&format!(" --{name} <{type_label}>"));
                }
            }
            _ => {}
        }
    }
    usage
}

fn value_type_label_from_json(value: &JsonValue) -> String {
    if let Some(text) = value.as_str() {
        return text.to_string();
    }
    if let Some(object) = value.as_object()
        && let Some(variants) = object.get("enum").and_then(JsonValue::as_array)
    {
        return format!(
            "enum({})",
            variants
                .iter()
                .filter_map(JsonValue::as_str)
                .collect::<Vec<_>>()
                .join("|")
        );
    }
    "value".to_string()
}

fn agent_facing_command(command: &str) -> String {
    let command = command.trim();
    command
        .strip_prefix("exo ")
        .map_or_else(|| command.to_string(), ToString::to_string)
}

fn workflow_confirmation_from_details(details: Option<&JsonValue>) -> Option<&JsonValue> {
    let details = details?;
    let direct = details.get("workflow_confirmation");
    let nested = details
        .get("details")
        .and_then(|nested| nested.get("workflow_confirmation"));
    let candidate = direct.or(nested)?;
    (candidate
        .get("kind")
        .and_then(JsonValue::as_str)
        .is_some_and(|kind| kind == "workflow_completion_confirmation"))
    .then_some(candidate)
}

fn format_workflow_confirmation(workflow: &JsonValue) -> String {
    let entity_type = workflow
        .get("completion_input")
        .and_then(|input| input.get("entity_type"))
        .or_else(|| workflow.get("entity_type"))
        .and_then(JsonValue::as_str)
        .unwrap_or("entity");
    let proposed = workflow
        .get("proposed_outcome")
        .and_then(JsonValue::as_str)
        .unwrap_or("");
    let header = workflow
        .get("header")
        .and_then(JsonValue::as_str)
        .unwrap_or("Outcome ready for review");
    let question = workflow
        .get("question")
        .and_then(JsonValue::as_str)
        .unwrap_or("Approve this outcome?");
    let message = workflow
        .get("message")
        .and_then(JsonValue::as_str)
        .unwrap_or("");

    let mut lines = vec![
        "Outcome ready for review.".to_string(),
        String::new(),
        header.to_string(),
    ];

    if !message.trim().is_empty() {
        lines.push(String::new());
        lines.push(message.to_string());
    } else if !proposed.trim().is_empty() {
        lines.push(String::new());
        lines.push("Outcome:".to_string());
        lines.push(proposed.to_string());
    }

    lines.push(String::new());
    lines.push(question.to_string());

    if let Some(options) = workflow.get("options").and_then(JsonValue::as_array)
        && !options.is_empty()
    {
        lines.push(String::new());
        lines.push("Choices:".to_string());
        for option in options {
            let Some(label) = option.get("label").and_then(JsonValue::as_str) else {
                continue;
            };
            let description = option
                .get("description")
                .and_then(JsonValue::as_str)
                .unwrap_or("");
            if description.is_empty() {
                lines.push(format!("- {label}"));
            } else {
                lines.push(format!("- {label}: {description}"));
            }
        }
    }

    lines.push(String::new());
    lines.push(format!(
        "After approval, record the {entity_type} outcome with the approved summary."
    ));

    lines.join("\n")
}

fn json_rpc_error(
    id: JsonValue,
    code: i64,
    message: impl Into<String>,
    data: Option<JsonValue>,
) -> JsonValue {
    let mut error = json!({
        "code": code,
        "message": message.into(),
    });
    if let Some(data) = data {
        error["data"] = data;
    }
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": error,
    })
}

fn exo_run_tool_definition() -> JsonValue {
    json!({
        "name": EXO_RUN_TOOL_NAME,
        "title": "Run Exo command",
        "description": "Run an Exo project-management command using Exo CLI syntax. This is not a shell runner.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Exo command to run, without the leading exo."
                },
                "args": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Values for $1, $2, $3 placeholders."
                },
                "workflowConfirmation": {
                    "type": "object",
                    "description": "Machine-only completion confirmation returned by a previous goal or task review prompt. Do not display this object or its fields to the user.",
                    "properties": {
                        "kind": {
                            "type": "string",
                            "const": "workflow_completion_confirmation",
                            "description": "Canonical workflow confirmation kind."
                        },
                        "entityType": {
                            "type": "string",
                            "description": "Entity type being confirmed, such as goal or task."
                        },
                        "entityId": {
                            "type": "string",
                            "description": "Entity ID being confirmed."
                        },
                        "decision": {
                            "type": "string",
                            "enum": [
                                "yes_complete",
                                "revise_outcome",
                                "not_complete_yet",
                                "discuss"
                            ],
                            "description": "User-selected workflow confirmation decision."
                        },
                        "outcome": {
                            "type": "string",
                            "description": "Confirmed outcome summary."
                        }
                    },
                    "required": [
                        "kind",
                        "entityType",
                        "entityId",
                        "decision",
                        "outcome"
                    ],
                    "additionalProperties": false
                },
                "auth": {
                    "type": "object",
                    "description": "Hidden execution approval input for confirm-required commands. Do not display this object or its fields to the user.",
                    "properties": {
                        "ticket": {
                            "type": "string",
                            "description": "Opaque hidden approval token returned by the previous confirm_required response."
                        },
                        "confirm": {
                            "type": "boolean",
                            "const": true,
                            "description": "Must be true to replay a confirmed command."
                        }
                    },
                    "required": ["ticket", "confirm"],
                    "additionalProperties": false
                }
            },
            "required": ["command"],
            "additionalProperties": false
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::protocol::{NextCallKind, Status};

    fn input(command: &str) -> ExoRunInput {
        ExoRunInput {
            command: command.to_string(),
            args: Vec::new(),
            workflow_confirmation: None,
            auth: None,
        }
    }

    fn structured(result: &McpToolResult) -> &JsonValue {
        result
            .structured_content
            .as_ref()
            .expect("structuredContent")
    }

    #[test]
    fn context_load_error_response_preserves_preload_guidance() {
        let error =
            anyhow::anyhow!("RFC file missing anchor comment: docs/rfcs/stage-1/0001-legacy.md")
                .context("Failed to reconcile RFC metadata from disk into SQLite");
        let response = context_load_error_response(
            Path::new("/workspace/demo"),
            "mcp.exo-run.test".to_string(),
            "status",
            error,
        );

        assert_eq!(response.status, Status::Error);
        let error = response.error.as_ref().expect("error body");
        assert_eq!(error.code, ErrorCode::PreconditionFailed);
        assert!(
            error
                .message
                .contains("Workspace context is blocked by a legacy RFC metadata migration."),
            "{}",
            error.message
        );
        let details = error.details.as_ref().expect("error details");
        assert_eq!(
            details["classification"],
            "migration_blocked:rfc_metadata_anchor"
        );
        assert_eq!(details["next_command"], "exo update");

        let steering = response.steering.as_ref().expect("steering");
        assert_eq!(steering.next_call.kind, NextCallKind::Call);
        assert_eq!(steering.next_call.params["address"]["path"][0], "update");
    }

    #[test]
    fn bootstrap_and_update_commands_skip_workspace_preload() {
        for command in [
            "update",
            "update --help",
            "project resolve",
            "project repair --apply",
            "sidecar bootstrap --key demo",
            "sidecar status",
            "task --help",
        ] {
            let compiled = compile_exo_run_input(input(command), "t1".to_string())
                .unwrap_or_else(|response| panic!("{command} failed to compile: {response:?}"));
            assert!(
                !compiled.requires_workspace_preload(),
                "{command} should use lightweight context"
            );
        }

        let status =
            compile_exo_run_input(input("status"), "t1".to_string()).expect("compile status");
        assert!(status.requires_workspace_preload());
    }

    #[test]
    fn confirm_required_text_hides_replay_protocol() {
        let response = ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            id: "t1".to_string(),
            status: Status::ConfirmRequired,
            result: None,
            error: None,
            ticket: Some("secret-ticket".to_string()),
            steering: None,
            reminders: None,
            display: None,
            preview: None,
            effect: None,
            trace: None,
        };

        let text = format_machine_response_text(&response);
        assert!(text.contains("Execution confirmation required."));
        assert!(text.contains("Ask the human whether to approve this action."));
        assert!(!text.contains("secret-ticket"));
        assert!(!text.contains("auth"));
        assert!(!text.contains("ticket"));
        assert!(!text.contains("{ \""));
    }

    #[test]
    fn workflow_confirmation_text_uses_human_fields_only() {
        let response = ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            id: "t1".to_string(),
            status: Status::Error,
            result: None,
            error: Some(ErrorBody {
                code: ErrorCode::PreconditionFailed,
                message: "Needs review".to_string(),
                details: Some(json!({
                    "workflow_confirmation": {
                        "kind": "workflow_completion_confirmation",
                        "entity_type": "goal",
                        "entity_id": "demo",
                        "completion_input": {
                            "kind": "workflow_completion_confirmation",
                            "entity_type": "goal",
                            "entity_id": "demo",
                            "decision": "yes_complete",
                            "outcome": "Done"
                        },
                        "header": "Outcome ready for review",
                        "question": "Approve this outcome?",
                        "message": "All child tasks are complete.\n\nProposed outcome: Done",
                        "readiness_rationale": "All child tasks are complete.",
                        "proposed_outcome": "Done",
                        "options": [
                            {
                                "label": "Approve outcome",
                                "value": "yes_complete",
                                "description": "Record this outcome and close the goal."
                            },
                            {
                                "label": "Keep working",
                                "value": "not_complete_yet",
                                "description": "Leave the goal pending and continue work."
                            }
                        ]
                    }
                })),
            }),
            ticket: None,
            steering: None,
            reminders: None,
            display: None,
            preview: None,
            effect: None,
            trace: None,
        };

        let text = format_machine_response_text(&response);
        assert!(text.contains("Outcome ready for review."));
        assert!(!text.contains("Entity: goal demo"));
        assert!(text.contains("Outcome ready for review"));
        assert!(text.contains("Approve this outcome?"));
        assert!(text.contains("Approve outcome: Record this outcome and close the goal."));
        assert!(text.contains("Keep working: Leave the goal pending and continue work."));
        assert!(
            text.contains("After approval, record the goal outcome with the approved summary.")
        );
        assert!(!text.contains("workflowConfirmation"));
        assert!(!text.contains("workflow_confirmation"));
        assert!(!text.contains("completion_input"));
        assert!(!text.contains("structuredContent"));
        assert!(!text.contains("yes_complete"));
        assert!(!text.contains("entity_type"));
        assert!(!text.contains("entity_id"));
    }

    #[test]
    fn executable_identity_changes_when_file_is_replaced() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("exo");
        std::fs::write(&path, "old").expect("write old binary");
        let old = ExecutableIdentity::for_path(&path).expect("identity for old binary");
        assert!(
            old.metadata_matches_path(&path)
                .expect("metadata matches original binary")
        );

        std::fs::remove_file(&path).expect("remove old binary");
        std::fs::write(&path, "new binary contents").expect("write new binary");
        let new = ExecutableIdentity::for_path(&path).expect("identity for new binary");

        assert_ne!(old, new);
        assert!(
            !old.metadata_matches_path(&path)
                .expect("metadata differs after replacement")
        );
    }

    #[test]
    fn builds_status_as_root_call() {
        let request = build_exo_run_request(input("status"), "t1".to_string()).expect("request");
        match request.op {
            Op::Call(params) => {
                assert_eq!(
                    params.address,
                    Address::Operation {
                        path: vec!["status".to_string()]
                    }
                );
                assert_eq!(params.input, json!({}));
            }
            other => panic!("expected call, got {other:?}"),
        }
    }

    #[test]
    fn builds_help_namespace_request() {
        let request = build_exo_run_request(input("help task"), "t1".to_string()).expect("request");
        match request.op {
            Op::Help(params) => {
                assert_eq!(
                    params.address,
                    Address::Namespace {
                        path: vec!["task".to_string()]
                    }
                );
            }
            other => panic!("expected help, got {other:?}"),
        }
    }

    #[test]
    fn cli_shaped_help_aliases_build_help_requests() {
        for (command, expected_address) in [
            (
                "task",
                Address::Namespace {
                    path: vec!["task".to_string()],
                },
            ),
            (
                "task --help",
                Address::Namespace {
                    path: vec!["task".to_string()],
                },
            ),
            (
                "task help",
                Address::Namespace {
                    path: vec!["task".to_string()],
                },
            ),
            (
                "rfc",
                Address::Namespace {
                    path: vec!["rfc".to_string()],
                },
            ),
            (
                "rfc --help",
                Address::Namespace {
                    path: vec!["rfc".to_string()],
                },
            ),
            (
                "rfc help",
                Address::Namespace {
                    path: vec!["rfc".to_string()],
                },
            ),
            (
                "rfc promote --help",
                Address::Operation {
                    path: vec!["rfc".to_string(), "promote".to_string()],
                },
            ),
            (
                "help rfc promote",
                Address::Operation {
                    path: vec!["rfc".to_string(), "promote".to_string()],
                },
            ),
        ] {
            let request = build_exo_run_request(input(command), "t1".to_string())
                .unwrap_or_else(|err| panic!("{command} should build help request: {err:?}"));
            match request.op {
                Op::Help(params) => assert_eq!(params.address, expected_address, "{command}"),
                other => panic!("expected help for {command}, got {other:?}"),
            }
        }
    }

    #[test]
    fn substitutes_placeholders_before_compiling_args() {
        let exo_input = ExoRunInput {
            command: "task complete my-task --log $1".to_string(),
            args: vec!["Implemented\nwith details".to_string()],
            workflow_confirmation: None,
            auth: None,
        };
        let request = build_exo_run_request(exo_input, "t1".to_string()).expect("request");
        match request.op {
            Op::Call(params) => {
                assert_eq!(
                    params.address,
                    Address::Operation {
                        path: vec!["task".to_string(), "complete".to_string()]
                    }
                );
                assert_eq!(params.input["id"], "my-task");
                assert_eq!(params.input["log"], "Implemented\nwith details");
            }
            other => panic!("expected call, got {other:?}"),
        }
    }

    #[test]
    fn maps_workflow_confirmation_to_machine_channel_shape() {
        let exo_input = ExoRunInput {
            command: "task complete my-task --log Done".to_string(),
            args: Vec::new(),
            workflow_confirmation: Some(McpWorkflowConfirmationInput {
                kind: "workflow_completion_confirmation".to_string(),
                entity_type: "task".to_string(),
                entity_id: "my-task".to_string(),
                decision: WorkflowConfirmationDecision::YesComplete,
                outcome: "Done".to_string(),
            }),
            auth: None,
        };
        let request = build_exo_run_request(exo_input, "t1".to_string()).expect("request");
        let confirmation = request.workflow_confirmation.expect("confirmation");
        assert_eq!(confirmation.entity_type, "task");
        assert_eq!(confirmation.entity_id, "my-task");
    }

    #[test]
    fn rejects_shell_syntax_before_execution() {
        for command in [
            "status | cat",
            "status && task list",
            "status > out.txt",
            "FOO=bar status",
            "status $(pwd)",
            "task list *",
        ] {
            let response = build_exo_run_request(input(command), "t1".to_string())
                .expect_err("expected rejection");
            assert_eq!(response.status, Status::Error);
            assert_eq!(
                response.error.as_ref().map(|error| error.code),
                Some(ErrorCode::InvalidInput)
            );
        }
    }

    #[test]
    fn compile_errors_keep_structured_steering_when_available() {
        let response =
            build_exo_run_request(input("status --definitely-not-real"), "t1".to_string())
                .expect_err("expected compile error");
        assert_eq!(response.status, Status::Error);
        assert_eq!(
            response.error.as_ref().map(|error| error.code),
            Some(ErrorCode::InvalidInput)
        );
        assert!(
            response
                .error
                .as_ref()
                .and_then(|error| error.details.as_ref())
                .is_some()
        );
    }

    #[test]
    fn compile_error_suggestions_are_exo_run_shaped() {
        let response = error_response(
            "t1".to_string(),
            ErrorCode::InvalidInput,
            "No operation specified for 'task'".to_string(),
            Some(json!({
                "diagnostics": [
                    {
                        "code": "ambiguous_subcommand",
                        "message": "No operation specified for 'task'",
                        "suggestions": [
                            {
                                "label": "Show namespace help",
                                "replacement": "exo task --help"
                            }
                        ]
                    }
                ]
            })),
        );
        let result = machine_response_to_tool_result(&response);
        let text = match &result.content[0] {
            McpContent::Text { text } => text,
        };

        assert!(text.contains("Suggestion: Show namespace help -> task --help"));
        assert!(!text.contains("-> exo task --help"));
        let structured = structured(&result);
        assert_eq!(
            structured["error"]["details"]["diagnostics"][0]["suggestions"][0]["replacement"],
            "task --help"
        );
    }

    #[test]
    fn tool_result_marks_machine_errors_as_tool_errors() {
        let response = error_response(
            "t1".to_string(),
            ErrorCode::InvalidInput,
            "Nope".to_string(),
            None,
        );
        let result = machine_response_to_tool_result(&response);
        assert!(result.is_error);
        assert!(matches!(result.content[0], McpContent::Text { .. }));
    }

    #[test]
    fn tool_result_lifts_nested_workflow_confirmation_details() {
        let mut response = error_response(
            "t1".to_string(),
            ErrorCode::PreconditionFailed,
            "Needs review".to_string(),
            Some(json!({
                "details": {
                    "workflow_confirmation": {
                        "kind": "workflow_completion_confirmation",
                        "completion_input": {
                            "entity_type": "task",
                            "entity_id": "my-task"
                        }
                    }
                },
                "steering": {}
            })),
        );
        response.status = Status::Error;

        let result = machine_response_to_tool_result(&response);
        let structured = structured(&result);
        assert_eq!(
            structured["error"]["details"]["workflow_confirmation"]["kind"],
            "workflow_completion_confirmation"
        );
        assert!(
            structured["error"]["details"]["workflow_confirmation"]["completion_input"].is_object()
        );
        assert!(structured["error"]["details"]["details"]["workflow_confirmation"].is_null());
    }

    #[test]
    fn ordinary_success_omits_structured_content_by_default() {
        let response = ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            id: "t1".to_string(),
            status: Status::Ok,
            result: Some(json!({
                "kind": "task.list",
                "ok": true,
                "items": [
                    { "id": "task-1", "label": "demo" }
                ]
            })),
            error: None,
            ticket: None,
            steering: None,
            reminders: None,
            display: Some(Display {
                invocation_message: "Listing tasks".to_string(),
                summary: "1 task".to_string(),
                body: Some("task-1 demo".to_string()),
            }),
            preview: None,
            effect: Some(crate::api::protocol::Effect::Pure),
            trace: Some(json!({ "dependencies": [ { "cell_id": 1, "revision": "large" } ] })),
        };

        let result = machine_response_to_tool_result(&response);
        assert!(result.structured_content.is_none());
        let serialized = serde_json::to_value(&result).expect("serialize result");
        assert!(serialized.get("structuredContent").is_none());
    }

    #[test]
    fn tool_result_structured_content_is_compact_but_keeps_replay_data() {
        let response = ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            id: "t1".to_string(),
            status: Status::Ok,
            result: Some(json!({
                "kind": "task.list",
                "ok": true,
                "items": (0..40)
                    .map(|i| json!({ "id": format!("task-{i}"), "label": "demo" }))
                    .collect::<Vec<_>>(),
                "steering": {
                    "large": "not useful in the default MCP payload"
                }
            })),
            error: None,
            ticket: Some("secret-ticket".to_string()),
            steering: None,
            reminders: None,
            display: Some(Display {
                invocation_message: "Listing tasks".to_string(),
                summary: "40 tasks".to_string(),
                body: Some("task list body".to_string()),
            }),
            preview: None,
            effect: Some(crate::api::protocol::Effect::Pure),
            trace: Some(json!({
                "dependencies": (0..100)
                    .map(|i| json!({ "cell_id": i, "revision": "large" }))
                    .collect::<Vec<_>>()
            })),
        };

        let result = machine_response_to_tool_result(&response);
        let structured = structured(&result);
        assert_eq!(structured["status"], "ok");
        assert_eq!(structured["ticket"], "secret-ticket");
        assert!(structured["display"]["body"].is_null());
        assert_eq!(structured["result"]["kind"], "task.list");
        assert!(structured["trace"].is_null());
        assert!(structured["result"]["steering"].is_null());
        assert_eq!(
            structured["result"]["items"]
                .as_array()
                .expect("compact items include array")
                .len(),
            21
        );
    }

    #[test]
    fn help_success_defaults_to_plaintext_only() {
        let response = ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            id: "t1".to_string(),
            status: Status::Ok,
            result: Some(json!({
                "title": "task",
                "summary": "Task commands",
                "namespaces": [],
                "operations": [
                    {
                        "path": "task complete",
                        "effect": "write",
                        "summary": "Complete a task",
                        "args": [
                            {
                                "id": "id",
                                "name": "id",
                                "description": "Task ID",
                                "kind": "positional",
                                "value_type": "string",
                                "optional": false,
                                "repeatable": false
                            },
                            {
                                "id": "log",
                                "name": "log",
                                "description": "Completion log",
                                "kind": "option",
                                "value_type": "string",
                                "optional": true,
                                "repeatable": false
                            }
                        ]
                    }
                ],
                "next_calls": []
            })),
            error: None,
            ticket: None,
            steering: None,
            reminders: None,
            display: None,
            preview: None,
            effect: None,
            trace: Some(json!({ "dependencies": [ { "cell_id": 1, "revision": "large" } ] })),
        };

        let result = machine_response_to_tool_result(&response);
        assert!(result.structured_content.is_none());
        let text = match &result.content[0] {
            McpContent::Text { text } => text,
        };
        assert!(text.contains("Usage: task complete <id> [--log <string>]"));
    }

    #[test]
    fn explicit_json_help_structured_content_keeps_usage_and_arg_details() {
        let response = ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            id: "t1".to_string(),
            status: Status::Ok,
            result: Some(json!({
                "title": "rfc promote",
                "summary": "Promote RFC to the specified next stage",
                "namespaces": [],
                "operations": [
                    {
                        "path": "rfc promote",
                        "effect": "write",
                        "summary": "Promote RFC to the specified next stage",
                        "args": [
                            {
                                "id": "id",
                                "name": "id",
                                "description": "RFC ID to promote",
                                "kind": "positional",
                                "value_type": "string",
                                "optional": false,
                                "repeatable": false
                            },
                            {
                                "id": "stage",
                                "name": "stage",
                                "description": "Target stage",
                                "kind": "option",
                                "value_type": "int",
                                "optional": false,
                                "repeatable": false
                            }
                        ]
                    }
                ],
                "next_calls": []
            })),
            error: None,
            ticket: None,
            steering: None,
            reminders: None,
            display: None,
            preview: None,
            effect: None,
            trace: Some(json!({ "dependencies": [ { "cell_id": 1, "revision": "large" } ] })),
        };

        let result = machine_response_to_tool_result_with_profile(&response, true);
        let structured = structured(&result);
        let operation = &structured["result"]["operations"][0];
        assert_eq!(operation["usage"], "rfc promote <id> --stage <int>");
        assert_eq!(operation["args"][0]["name"], "id");
        assert_eq!(operation["args"][0]["kind"], "positional");
        assert_eq!(operation["args"][1]["name"], "stage");
        assert_eq!(operation["args"][1]["value_type"], "int");
        assert!(operation["args"][1]["keys"].is_null());
        assert!(structured["trace"].is_null());
    }

    #[test]
    fn explicit_json_output_detection_accepts_global_format_spellings() {
        assert!(explicit_json_output_requested(&input(
            "--format json status"
        )));
        assert!(explicit_json_output_requested(&input(
            "status --format=json"
        )));
        assert!(explicit_json_output_requested(&input(
            "status --format JSON"
        )));
        assert!(!explicit_json_output_requested(&input("status")));
        assert!(!explicit_json_output_requested(&input(
            "status --format compact"
        )));
    }

    #[test]
    fn tool_call_request_ids_prefer_the_proxy_outcome_identity() {
        let params = json!({ MCP_OUTCOME_REQUEST_ID_PARAM: "stable-call" });
        let first =
            tool_call_request_id(&json!("exo-proxy-worker-1-worker.classify"), Some(&params));
        let second =
            tool_call_request_id(&json!("exo-proxy-worker-2-worker.classify"), Some(&params));

        assert_eq!(first, "mcp.exo-run.stable-call");
        assert_eq!(second, first);
    }

    #[test]
    fn direct_tool_calls_receive_fresh_outcome_identities() {
        let first = tool_call_request_id(&json!(1), None);
        let second = tool_call_request_id(&json!(1), None);

        assert_ne!(first, second);
        assert!(first.starts_with("mcp.exo-run."));
        assert!(second.starts_with("mcp.exo-run."));
    }

    fn handle_json_rpc_value_for_test(value: JsonValue) -> Option<JsonValue> {
        let mut worker_call_cache = WorkerCallCache::default();
        handle_json_rpc_value(Path::new("."), None, value, false, &mut worker_call_cache)
    }

    #[test]
    fn json_rpc_initialize_and_tools_list_work() {
        let init = handle_json_rpc_value_for_test(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "0" }
            }
        }))
        .expect("init response");
        assert_eq!(init["result"]["protocolVersion"], MCP_PROTOCOL_VERSION);
        assert!(init["result"]["capabilities"]["tools"].is_object());

        let list = handle_json_rpc_value_for_test(json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list"
        }))
        .expect("list response");
        assert_eq!(list["result"]["tools"][0]["name"], EXO_RUN_TOOL_NAME);
    }

    #[test]
    fn unknown_tool_is_protocol_error() {
        let response = handle_json_rpc_value_for_test(json!({
            "jsonrpc": "2.0",
            "id": "call-1",
            "method": "tools/call",
            "params": { "name": "nope", "arguments": {} }
        }))
        .expect("response");
        assert_eq!(response["error"]["code"], -32602);
    }

    #[test]
    fn ping_returns_empty_result() {
        let response =
            handle_json_rpc_value_for_test(json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" }))
                .expect("response");
        assert!(response["result"].is_object());
    }

    #[test]
    fn explicit_null_id_is_request_not_notification() {
        let response = handle_json_rpc_value_for_test(
            json!({ "jsonrpc": "2.0", "id": null, "method": "ping" }),
        )
        .expect("response");
        assert_eq!(response["id"], JsonValue::Null);
        assert!(response["result"].is_object());
    }

    #[test]
    fn initialized_notification_has_no_response() {
        let response = handle_json_rpc_value_for_test(
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        );
        assert!(response.is_none());
    }

    #[test]
    fn compilation_error_uses_help_steering_when_compile_detects_shell_operator() {
        let spec = CommandSpec::from_registry(&default_registry());
        let compilation = compile_argv(&spec, &["status".to_string(), "|".to_string()]);
        let response =
            compilation_error_response("t1".to_string(), &["status".to_string()], &compilation);
        assert_eq!(
            response.steering.as_ref().map(|s| s.next_call.kind),
            Some(NextCallKind::Help)
        );
    }
}
