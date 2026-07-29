use super::{
    MAX_REQUEST_BODY_BYTES, SESSION_COOKIE_PREFIX, TicketExchangeError, WorkbenchHostInner,
    WorkbenchSession, assets,
};
use crate::api::protocol::{
    Address, CallParams, ErrorBody, ErrorCode, Op, PROTOCOL_VERSION, RequestEnvelope,
    ResponseEnvelope, Status,
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
struct BrowserCommandRequest {
    protocol_version: u32,
    id: String,
    session_key: String,
    operation: BrowserOperation,
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
    };
    inner.touch_daemon_activity();
    let cookie_name = session_cookie_name(&result.session_key);
    let cookie =
        format!("{cookie_name}={session_id}; HttpOnly; SameSite=Strict; Path=/; Max-Age=43200");
    let mut response = Json(result).into_response();
    if let Ok(value) = cookie.parse() {
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
    let Some(session) = authenticated_session(&inner, &headers, &request.session_key) else {
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
                    if stream_inner.session(&session_key, &session_id).is_none() {
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
}
