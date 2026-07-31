use super::{
    MAX_REQUEST_BODY_BYTES, SESSION_COOKIE_PREFIX, TicketExchangeError, WorkbenchHostInner,
    WorkbenchSession, assets,
    planning::{self, BrowserPlanningOperation, BrowserPlanningRequest},
};
use crate::api::protocol::{
    Address, CallParams, ErrorBody, ErrorCode, Op, PROTOCOL_VERSION, RequestEnvelope,
    ResponseEnvelope, Status, WorkflowConfirmationDecision, WorkflowConfirmationInput,
};
use async_stream::stream;
use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::HeaderName;
use axum::http::header::{CACHE_CONTROL, COOKIE, HOST, ORIGIN, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue, Response, StatusCode, Uri};
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response as AxumResponse};
use axum::routing::{get, post};
use futures_core::Stream;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::convert::Infallible;
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::watch;

#[derive(Clone)]
struct HttpState {
    inner: Weak<WorkbenchHostInner>,
}

#[derive(Debug, Deserialize)]
struct SessionRequest {
    ticket: String,
}

#[derive(Debug, Deserialize)]
struct SessionRenewRequest {
    session_key: String,
}

#[derive(Debug, Deserialize)]
struct BrowserCommandRequestV1 {
    protocol_version: u32,
    id: String,
    session_key: String,
    operation: BrowserOperation,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum BrowserCommandRequest {
    V1(BrowserCommandRequestV1),
    Planning(BrowserPlanningRequest),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum BrowserOperation {
    Snapshot,
    LaneFocus { lane_id: String },
}

#[derive(Debug, Serialize)]
struct HttpErrorBody {
    kind: &'static str,
    ok: bool,
    message: &'static str,
}

pub(super) async fn serve(
    listener: TcpListener,
    inner: Weak<WorkbenchHostInner>,
    mut shutdown: watch::Receiver<bool>,
) -> std::io::Result<()> {
    let app = Router::new()
        .route("/api/session", post(create_session))
        .route("/api/session/renew", post(renew_session))
        .route("/api/command", post(run_command))
        .route("/api/events", get(events))
        .fallback(get(static_asset))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES))
        .with_state(HttpState { inner });
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            while !*shutdown.borrow() {
                if shutdown.changed().await.is_err() {
                    break;
                }
            }
        })
        .await
}

async fn create_session(
    State(state): State<HttpState>,
    headers: HeaderMap,
    payload: Result<Json<SessionRequest>, JsonRejection>,
) -> AxumResponse {
    let Json(request) = match payload {
        Ok(request) => request,
        Err(error) => return json_rejection_response(error),
    };
    let Some(inner) = state.inner.upgrade() else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "workbench.host_unavailable",
            "The workbench host is no longer available",
        );
    };
    if !origin_matches(&inner, &headers) {
        return error_response(
            StatusCode::FORBIDDEN,
            "workbench.origin_mismatch",
            "The workbench request origin is not accepted",
        );
    }
    let (session_id, result) = match inner.redeem_ticket(&request.ticket) {
        Ok(result) => result,
        Err(TicketExchangeError::Invalid) => {
            return error_response(
                StatusCode::UNAUTHORIZED,
                "workbench.ticket_invalid",
                "The workbench launch ticket is invalid",
            );
        }
        Err(TicketExchangeError::Busy) => {
            return error_response(
                StatusCode::TOO_MANY_REQUESTS,
                "workbench.busy",
                "The workbench session limit is reached",
            );
        }
        Err(TicketExchangeError::Unavailable) => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "workbench.host_unavailable",
                "The workbench session could not be persisted",
            );
        }
    };
    inner.touch_daemon_activity();
    let cookie_name = session_cookie_name(&result.session_key);
    let mut response = Json(result).into_response();
    if let Ok(value) = session_cookie(&cookie_name, &session_id).parse() {
        response.headers_mut().insert(SET_COOKIE, value);
    }
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn renew_session(
    State(state): State<HttpState>,
    headers: HeaderMap,
    payload: Result<Json<SessionRenewRequest>, JsonRejection>,
) -> AxumResponse {
    let Json(request) = match payload {
        Ok(request) => request,
        Err(error) => return json_rejection_response(error),
    };
    let Some(inner) = state.inner.upgrade() else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "workbench.host_unavailable",
            "The workbench host is no longer available",
        );
    };
    if !origin_matches(&inner, &headers) {
        return error_response(
            StatusCode::FORBIDDEN,
            "workbench.origin_mismatch",
            "The workbench request origin is not accepted",
        );
    }
    if !valid_session_key(&request.session_key) {
        return error_response(
            StatusCode::BAD_REQUEST,
            "workbench.invalid_request",
            "The workbench session request is invalid",
        );
    }
    let cookie_name = session_cookie_name(&request.session_key);
    let Some(session_secret) = cookie_value(&headers, &cookie_name) else {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "workbench.session_invalid",
            "The workbench session is invalid",
        );
    };
    let result = match inner.renew_session(&request.session_key, session_secret) {
        Ok(Some(result)) => result,
        Ok(None) => {
            return error_response(
                StatusCode::UNAUTHORIZED,
                "workbench.session_invalid",
                "The workbench session is invalid",
            );
        }
        Err(_) => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "workbench.host_unavailable",
                "The workbench session could not be renewed",
            );
        }
    };
    inner.touch_daemon_activity();
    let mut response = Json(result).into_response();
    if let Ok(value) = session_cookie(&cookie_name, session_secret).parse() {
        response.headers_mut().insert(SET_COOKIE, value);
    }
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn run_command(
    State(state): State<HttpState>,
    headers: HeaderMap,
    payload: Result<Json<BrowserCommandRequest>, JsonRejection>,
) -> AxumResponse {
    let Json(request) = match payload {
        Ok(request) => request,
        Err(error) => return json_rejection_response(error),
    };
    let Some(inner) = state.inner.upgrade() else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "workbench.host_unavailable",
            "The workbench host is no longer available",
        );
    };
    if !origin_matches(&inner, &headers) {
        return error_response(
            StatusCode::FORBIDDEN,
            "workbench.origin_mismatch",
            "The workbench request origin is not accepted",
        );
    }
    match request {
        BrowserCommandRequest::V1(request) => run_v1_command(&inner, &headers, request).await,
        BrowserCommandRequest::Planning(request) => {
            run_planning_command(inner, &headers, request).await
        }
    }
}

async fn run_v1_command(
    inner: &Arc<WorkbenchHostInner>,
    headers: &HeaderMap,
    request: BrowserCommandRequestV1,
) -> AxumResponse {
    if request.protocol_version != PROTOCOL_VERSION
        || request.id.trim().is_empty()
        || !valid_session_key(&request.session_key)
    {
        return error_response(
            StatusCode::BAD_REQUEST,
            "workbench.invalid_request",
            "The workbench command request is invalid",
        );
    }
    let Some(session) = authenticated_session(inner, headers, &request.session_key) else {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "workbench.session_invalid",
            "The workbench session is invalid",
        );
    };
    let (capability, address, input) = match request.operation {
        BrowserOperation::Snapshot => (
            "workbench.snapshot",
            Address::Operation {
                path: vec!["workbench".to_string(), "snapshot".to_string()],
            },
            json!({}),
        ),
        BrowserOperation::LaneFocus { lane_id } => (
            "lane.focus",
            Address::Operation {
                path: vec!["lane".to_string(), "focus".to_string()],
            },
            json!({ "id": lane_id }),
        ),
    };
    if !session.allows(capability) {
        return error_response(
            StatusCode::FORBIDDEN,
            "workbench.capability_denied",
            "The workbench session does not allow this operation",
        );
    }
    let workspace_root = match inner.validate_session_workspace(&session.workspace_root) {
        Ok(root) => root,
        Err(_) => {
            return error_response(
                StatusCode::GONE,
                "workbench.workspace_unavailable",
                "The workbench workspace is no longer available",
            );
        }
    };
    let Some(dispatcher) = inner.dispatcher().cloned() else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "workbench.host_unavailable",
            "The workbench command dispatcher is unavailable",
        );
    };
    inner.touch_daemon_activity();
    let response = dispatcher
        .dispatch(RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            id: request.id,
            op: Op::Call(CallParams { address, input }),
            workspace_root: Some(workspace_root),
            auth: None,
            workflow_confirmation: None,
            agent_id: None,
        })
        .await;
    let response = browser_safe_response(response, capability);
    let mut response = Json(response).into_response();
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn run_planning_command(
    inner: Arc<WorkbenchHostInner>,
    headers: &HeaderMap,
    request: BrowserPlanningRequest,
) -> AxumResponse {
    if request.protocol_version != planning::PLANNING_PROTOCOL_VERSION
        || request.id.trim().is_empty()
        || request.expected_daemon_instance_id.trim().is_empty()
        || request.expected_phase_id.trim().is_empty()
        || !valid_session_key(&request.session_key)
    {
        return error_response(
            StatusCode::BAD_REQUEST,
            "workbench.invalid_request",
            "The workbench planning request is invalid",
        );
    }
    let Some(session) = authenticated_session(&inner, headers, &request.session_key) else {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "workbench.session_invalid",
            "The workbench session is invalid",
        );
    };
    let capability = request.operation.capability();
    if !session.allows(capability) {
        return error_response(
            StatusCode::FORBIDDEN,
            "workbench.capability_denied",
            "The workbench session does not allow this operation",
        );
    }
    let workspace_root = match inner.validate_session_workspace(&session.workspace_root) {
        Ok(root) => root,
        Err(_) => {
            return error_response(
                StatusCode::GONE,
                "workbench.workspace_unavailable",
                "The workbench workspace is no longer available",
            );
        }
    };
    inner.touch_daemon_activity();

    if let BrowserPlanningOperation::TaskCompleteReview { task_id, outcome } = &request.operation {
        let review_inner = Arc::clone(&inner);
        let review_session = session.clone();
        let request_id = request.id.clone();
        let expected_daemon_instance_id = request.expected_daemon_instance_id.clone();
        let expected_revision = request.expected_revision;
        let expected_phase_id = request.expected_phase_id.clone();
        let task_id = task_id.clone();
        let outcome = outcome.clone();
        let review = tokio::task::spawn_blocking(move || {
            review_inner.completion_review(
                &review_session,
                &request_id,
                &expected_daemon_instance_id,
                expected_revision,
                &expected_phase_id,
                &task_id,
                &outcome,
            )
        })
        .await;
        let response = match review {
            Ok(Ok(review)) => planning::review_response(request.id, &review),
            Ok(Err(error)) => error.response(request.id),
            Err(_) => planning::WorkbenchPlanningError::internal().response(request.id),
        };
        return no_store_json(response);
    }

    let Some(dispatcher) = inner.dispatcher().cloned() else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "workbench.host_unavailable",
            "The workbench command dispatcher is unavailable",
        );
    };
    let operation_name = request.operation.operation_name();
    let review_binding = match &request.operation {
        BrowserPlanningOperation::TaskCompleteApprove { review_id } => {
            let approval = match inner.prepare_completion_approval(
                &session,
                &request.id,
                &request.expected_daemon_instance_id,
                request.expected_revision,
                &request.expected_phase_id,
                review_id,
            ) {
                Ok(approval) => approval,
                Err(error) => return no_store_json(error.response(request.id)),
            };
            let task_id = approval.task_id;
            let proposed_outcome = approval.proposed_outcome;
            let mut envelope = RequestEnvelope {
                protocol_version: PROTOCOL_VERSION,
                id: request.id.clone(),
                op: Op::Call(CallParams {
                    address: Address::Operation {
                        path: vec!["task".to_string(), "complete".to_string()],
                    },
                    input: json!({
                        "id": task_id,
                        "log": proposed_outcome,
                    }),
                }),
                workspace_root: Some(workspace_root),
                auth: None,
                workflow_confirmation: Some(WorkflowConfirmationInput {
                    kind: "workflow_completion_confirmation".to_string(),
                    entity_type: "task".to_string(),
                    entity_id: task_id,
                    decision: WorkflowConfirmationDecision::YesComplete,
                    outcome: proposed_outcome,
                }),
                agent_id: None,
            };
            if let Err(error) = planning::attach_context(&mut envelope, &approval.context) {
                return no_store_json(error.response(request.id));
            }
            Some((envelope, review_id.clone()))
        }
        _ => {
            let envelope = match planning::mutation_request(
                request.id.clone(),
                workspace_root,
                &session,
                request.expected_daemon_instance_id.clone(),
                request.expected_revision,
                request.expected_phase_id.clone(),
                &request.operation,
            ) {
                Ok(envelope) => envelope,
                Err(error) => return no_store_json(error.response(request.id)),
            };
            Some((envelope, String::new()))
        }
    };
    let Some((envelope, review_id)) = review_binding else {
        return no_store_json(
            planning::WorkbenchPlanningError::invalid_request().response(request.id),
        );
    };
    let response = dispatcher.dispatch(envelope).await;
    if !review_id.is_empty() && response.effect == Some(crate::api::protocol::Effect::Write) {
        inner.mark_completion_review_consumed(&session.id, &review_id, &request.id);
    }
    let response = browser_safe_planning_response(response, operation_name);
    no_store_json(response)
}

fn no_store_json(response: ResponseEnvelope) -> AxumResponse {
    let mut response = Json(response).into_response();
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn browser_safe_response(mut response: ResponseEnvelope, capability: &str) -> ResponseEnvelope {
    response.ticket = None;
    response.steering = None;
    response.reminders = None;
    response.display = None;
    response.preview = None;
    response.trace = None;

    if response.status == Status::Ok {
        if let Some(result) = response
            .result
            .as_mut()
            .and_then(serde_json::Value::as_object_mut)
        {
            result.remove("post_write");
        }
        return response;
    }

    let code = response
        .error
        .as_ref()
        .map_or(ErrorCode::Internal, |error| error.code);
    let message = match capability {
        "workbench.snapshot" => "The workbench snapshot is temporarily unavailable",
        "lane.focus" => "The lane focus request could not be completed",
        _ => "The workbench command could not be completed",
    };
    response.result = None;
    response.error = Some(ErrorBody {
        code,
        message: message.to_string(),
        details: None,
    });
    response
}

fn browser_safe_planning_response(
    mut response: ResponseEnvelope,
    operation: &str,
) -> ResponseEnvelope {
    response.ticket = None;
    response.steering = None;
    response.reminders = None;
    response.display = None;
    response.preview = None;
    response.trace = None;

    if response.status == Status::Ok {
        match planning::safe_mutation_result(&response, operation) {
            Ok(result) => response.result = Some(result),
            Err(error) => return error.response(response.id),
        }
        return response;
    }

    let effect = response.effect;
    let mut safe = planning::safe_planning_error(&response).response(response.id);
    safe.effect = effect;
    safe
}

async fn events(
    State(state): State<HttpState>,
    headers: HeaderMap,
    uri: Uri,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, AxumResponse> {
    let Some(inner) = state.inner.upgrade() else {
        return Err(error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "workbench.host_unavailable",
            "The workbench host is no longer available",
        ));
    };
    if !event_origin_matches(&inner, &headers) {
        return Err(error_response(
            StatusCode::FORBIDDEN,
            "workbench.origin_mismatch",
            "The workbench request origin is not accepted",
        ));
    }
    let Some(session_key) = session_key_from_uri(&uri) else {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "workbench.invalid_request",
            "The workbench event request is invalid",
        ));
    };
    let Some(session) = authenticated_session(&inner, &headers, session_key) else {
        return Err(error_response(
            StatusCode::UNAUTHORIZED,
            "workbench.session_invalid",
            "The workbench session is invalid",
        ));
    };
    let permit = inner.event_admission().try_acquire_owned().map_err(|_| {
        error_response(
            StatusCode::TOO_MANY_REQUESTS,
            "workbench.busy",
            "The workbench event stream limit is reached",
        )
    })?;
    let last_event_id = headers
        .get(HeaderName::from_static("last-event-id"))
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    let mut write_events = inner.subscribe();
    let initial_revision = inner.current_revision();
    let session_id = session.id;
    let session_key = session.selector;
    let stream_inner = Arc::clone(&inner);
    let event_stream = stream! {
        let _permit = permit;
        yield Ok(Event::default()
            .event("ready")
            .id(initial_revision.to_string())
            .json_data(json!({
                "kind": "workbench.ready",
                "revision": initial_revision,
            }))
            .unwrap_or_else(|_| Event::default().event("ready")));
        if last_event_id.is_some_and(|revision| revision != initial_revision) {
            yield Ok(invalidation_event(initial_revision));
        }
        let mut keepalive = tokio::time::interval(Duration::from_secs(15));
        keepalive.tick().await;
        loop {
            tokio::select! {
                _ = keepalive.tick() => {
                    if stream_inner
                        .session_by_digest(&session_key, &session_id)
                        .is_none()
                    {
                        break;
                    }
                    stream_inner.touch_daemon_activity();
                    yield Ok(Event::default().comment("keepalive"));
                }
                event = write_events.recv() => {
                    let revision = match event {
                        Ok(revision) => revision,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            stream_inner.current_revision()
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    };
                    yield Ok(invalidation_event(revision));
                }
            }
        }
    };
    Ok(Sse::new(event_stream))
}

async fn static_asset(State(_state): State<HttpState>, uri: Uri) -> Response<Body> {
    if uri.path().starts_with("/api/") {
        return error_response(
            StatusCode::NOT_FOUND,
            "workbench.invalid_request",
            "The workbench API route does not exist",
        );
    }
    assets::response(uri.path())
}

fn authenticated_session(
    inner: &WorkbenchHostInner,
    headers: &HeaderMap,
    session_key: &str,
) -> Option<WorkbenchSession> {
    let cookie_name = session_cookie_name(session_key);
    let session_id = cookie_value(headers, &cookie_name)?;
    inner.session(session_key, session_id)
}

fn session_cookie_name(session_key: &str) -> String {
    format!("{SESSION_COOKIE_PREFIX}{session_key}")
}

fn session_cookie(name: &str, value: &str) -> String {
    format!("{name}={value}; HttpOnly; SameSite=Strict; Path=/; Max-Age=43200")
}

fn session_key_from_uri(uri: &Uri) -> Option<&str> {
    let mut values = uri
        .query()?
        .split('&')
        .filter_map(|part| part.split_once('='))
        .filter_map(|(name, value)| {
            (name == "session_key" && valid_session_key(value)).then_some(value)
        });
    let value = values.next()?;
    values.next().is_none().then_some(value)
}

fn valid_session_key(value: &str) -> bool {
    value.len() == 43
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|part| part.trim().split_once('='))
        .find_map(|(candidate, value)| (candidate == name).then_some(value))
}

fn origin_matches(inner: &WorkbenchHostInner, headers: &HeaderMap) -> bool {
    host_matches(inner, headers)
        && headers
            .get(ORIGIN)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| inner.origin().as_deref() == Some(value))
}

fn event_origin_matches(inner: &WorkbenchHostInner, headers: &HeaderMap) -> bool {
    host_matches(inner, headers)
        && headers
            .get(ORIGIN)
            .map(|value| {
                value
                    .to_str()
                    .ok()
                    .is_some_and(|value| inner.origin().as_deref() == Some(value))
            })
            .unwrap_or(true)
}

fn host_matches(inner: &WorkbenchHostInner, headers: &HeaderMap) -> bool {
    let Some(expected_host) = inner.expected_host() else {
        return false;
    };
    headers
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == expected_host)
}

fn invalidation_event(revision: u64) -> Event {
    Event::default()
        .event("invalidate")
        .id(revision.to_string())
        .json_data(json!({
            "kind": "workbench.invalidate",
            "revision": revision,
        }))
        .unwrap_or_else(|_| Event::default().event("invalidate"))
}

fn error_response(status: StatusCode, kind: &'static str, message: &'static str) -> AxumResponse {
    let mut response = (
        status,
        Json(HttpErrorBody {
            kind,
            ok: false,
            message,
        }),
    )
        .into_response();
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn json_rejection_response(error: JsonRejection) -> AxumResponse {
    if error.status() == StatusCode::PAYLOAD_TOO_LARGE {
        error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            "workbench.busy",
            "The workbench request body limit is exceeded",
        )
    } else {
        error_response(
            StatusCode::BAD_REQUEST,
            "workbench.invalid_request",
            "The workbench request body is invalid",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::protocol::Effect;

    fn response(
        status: Status,
        result: Option<serde_json::Value>,
        error: Option<ErrorBody>,
    ) -> ResponseEnvelope {
        ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            id: "browser-redaction".to_string(),
            status,
            result,
            error,
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
    fn browser_safe_response_redacts_dispatch_failures_and_post_write_reports() {
        let local_path = "/private/project/cache/exo.db";
        let failed = browser_safe_response(
            response(
                Status::Error,
                None,
                Some(ErrorBody {
                    code: ErrorCode::Internal,
                    message: format!("Failed to open database at {local_path}"),
                    details: Some(json!({ "database_path": local_path })),
                }),
            ),
            "lane.focus",
        );
        let failed_json = serde_json::to_string(&failed).expect("serialize browser error");
        assert!(!failed_json.contains(local_path));
        assert_eq!(failed.effect, Some(Effect::Write));
        assert_eq!(
            failed.error.as_ref().map(|error| error.message.as_str()),
            Some("The lane focus request could not be completed")
        );
        assert!(
            failed
                .error
                .as_ref()
                .is_some_and(|error| error.details.is_none())
        );

        let succeeded = browser_safe_response(
            response(
                Status::Ok,
                Some(json!({
                    "kind": "lane.focus",
                    "ok": true,
                    "lane": { "id": "lane-one" },
                    "post_write": { "issue": format!("checkpoint failed at {local_path}") },
                })),
                None,
            ),
            "lane.focus",
        );
        let succeeded_json = serde_json::to_string(&succeeded).expect("serialize browser success");
        assert!(!succeeded_json.contains(local_path));
        assert!(
            succeeded
                .result
                .as_ref()
                .and_then(serde_json::Value::as_object)
                .is_some_and(|result| !result.contains_key("post_write"))
        );
        assert_eq!(
            succeeded.result.as_ref().expect("result")["lane"]["id"],
            "lane-one"
        );
    }

    #[test]
    fn browser_safe_planning_response_preserves_only_stable_retry_contracts() {
        let local_path = "/private/project/cache/exo.db";
        let stale = browser_safe_planning_response(
            response(
                Status::Error,
                None,
                Some(ErrorBody {
                    code: ErrorCode::PreconditionFailed,
                    message: format!("stale snapshot while reading {local_path}"),
                    details: Some(json!({
                        "kind": "workbench.stale_snapshot",
                        "database_path": local_path,
                    })),
                }),
            ),
            "task_update",
        );
        let stale_json = serde_json::to_string(&stale).expect("serialize planning error");
        assert!(!stale_json.contains(local_path));
        assert_eq!(
            stale.error.as_ref().expect("stale error").details,
            Some(json!({
                "kind": "workbench.stale_snapshot",
                "retry_with_same_request_id": false,
            }))
        );

        let busy = browser_safe_planning_response(
            response(
                Status::Error,
                None,
                Some(ErrorBody {
                    code: ErrorCode::PreconditionFailed,
                    message: "daemon admission is saturated".to_string(),
                    details: Some(json!({ "kind": "daemon.busy" })),
                }),
            ),
            "task_log",
        );
        assert_eq!(
            busy.error.as_ref().expect("busy error").details,
            Some(json!({
                "kind": "workbench.busy",
                "retry_with_same_request_id": true,
            }))
        );
    }
}
