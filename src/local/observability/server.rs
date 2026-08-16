use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use axum::extract::{Path as AxumPath, Query, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use tokio::task::JoinHandle;
use tokio_stream::StreamExt as _;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tokio_util::sync::CancellationToken;

use crate::local::history::{
    HISTORY_LIVE_EVENT_SCHEMA_VERSION, HistoryLiveEvent, LocalHistoryReader,
    normalize_declared_workdir,
};
use crate::local::presentation::{PRESENTATION_SCHEMA_VERSION, build_presentation};
use crate::local::{HistoryQuery, LocalHistoryRuntime};

use super::model::{
    ApiAgent, ApiAgentDetail, ApiAgentList, ApiError, ApiInvocationDetail, ApiInvocationList,
    ApiLogicalInvocation, ApiOutputPage, ApiStatusDocument, LOCAL_OBSERVABILITY_API_VERSION,
    presentation_version, schema_version,
};

const DEFAULT_INVOCATION_LIMIT: usize = 50;
const MAX_INVOCATION_LIMIT: usize = 100;
const DEFAULT_OUTPUT_CHUNK_LIMIT: usize = 16;
const MAX_OUTPUT_CHUNK_LIMIT: usize = 64;

#[derive(Clone)]
struct ApiState {
    history: Arc<LocalHistoryRuntime>,
}

#[derive(Clone)]
struct AuthState {
    token: HeaderValue,
}

pub struct LocalObservabilityServer {
    addr: SocketAddr,
    cancellation: CancellationToken,
    task: JoinHandle<Result<()>>,
}

impl LocalObservabilityServer {
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn request_shutdown(&self) {
        self.cancellation.cancel();
    }

    pub async fn shutdown(self) -> Result<()> {
        self.request_shutdown();
        self.task
            .await
            .context("Local observability server task failed to join")??;
        Ok(())
    }
}

pub async fn start_local_observability_server(
    history: Arc<LocalHistoryRuntime>,
    bearer: impl Into<Arc<str>>,
) -> Result<LocalObservabilityServer> {
    let token = validate_bearer(bearer.into())?;
    let app = build_router(history, token);
    let listener =
        tokio::net::TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
            .await
            .context("failed to bind Zodex Local observability loopback listener")?;
    let addr = listener
        .local_addr()
        .context("failed to inspect Zodex Local observability listener address")?;
    if !addr.ip().is_loopback() {
        bail!("Zodex Local observability listener bound outside loopback: {addr}");
    }
    let cancellation = CancellationToken::new();
    let shutdown = cancellation.clone();
    let task = tokio::spawn(async move {
        tokio::select! {
            result = axum::serve(listener, app.into_make_service()) => {
                result.context("Zodex Local observability server terminated unexpectedly")
            }
            _ = shutdown.cancelled_owned() => Ok(()),
        }
    });
    Ok(LocalObservabilityServer {
        addr,
        cancellation,
        task,
    })
}

pub(super) fn build_router(history: Arc<LocalHistoryRuntime>, token: HeaderValue) -> Router {
    Router::new()
        .route("/v1/status", get(status))
        .route("/v1/agents", get(agents))
        .route("/v1/agents/{id}", get(agent_detail))
        .route("/v1/invocations", get(invocations))
        .route("/v1/invocations/{id}", get(invocation_detail))
        .route("/v1/invocations/{id}/output", get(invocation_output))
        .route("/v1/events", get(events))
        .with_state(Arc::new(ApiState { history }))
        .layer(middleware::from_fn_with_state(
            AuthState { token },
            bearer_auth_no_store,
        ))
}

async fn status(State(state): State<Arc<ApiState>>) -> Result<Json<ApiStatusDocument>, ApiFailure> {
    let path = state.history.database_path().to_path_buf();
    let runtime_id = state.history.runtime_id().to_string();
    let current_runtime = runtime_id.clone();
    let (history, agent_count) = run_blocking(move || {
        let history = LocalHistoryReader::status(&path)?;
        let agent_count = LocalHistoryReader::agent_count(&path, Some(&current_runtime))?;
        Ok((history, agent_count))
    })
    .await?;
    Ok(Json(ApiStatusDocument {
        schema_version: schema_version(),
        api_version: LOCAL_OBSERVABILITY_API_VERSION,
        presentation_version: presentation_version(),
        runtime_id,
        history,
        current_runtime_agent_count: agent_count,
        active_process_count: state.history.active_process_count(),
    }))
}

#[derive(Debug, Deserialize)]
struct AgentListQuery {
    runtime: Option<String>,
}

async fn agents(
    State(state): State<Arc<ApiState>>,
    Query(query): Query<AgentListQuery>,
) -> Result<Json<ApiAgentList>, ApiFailure> {
    let runtime_id = state.history.runtime_id().to_string();
    let runtime_filter = match query.runtime.as_deref() {
        None => None,
        Some("current") => Some(runtime_id.clone()),
        Some(other) => return Err(bad_request(format!("unsupported runtime filter `{other}`"))),
    };
    let path = state.history.database_path().to_path_buf();
    let records =
        run_blocking(move || LocalHistoryReader::agent_records(&path, runtime_filter.as_deref()))
            .await?;
    let active = state.history.active_process_counts();
    let agents = records
        .into_iter()
        .map(|record| {
            let count = active.get(&record.summary.id).copied().unwrap_or(0);
            ApiAgent::from_record(record, &runtime_id, count)
        })
        .collect();
    Ok(Json(ApiAgentList {
        schema_version: schema_version(),
        runtime_id,
        agents,
    }))
}

async fn agent_detail(
    State(state): State<Arc<ApiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<ApiAgentDetail>, ApiFailure> {
    validate_agent_id(&id)?;
    let path = state.history.database_path().to_path_buf();
    let requested = id.clone();
    let record = run_blocking(move || LocalHistoryReader::agent_record(&path, &requested))
        .await?
        .ok_or_else(|| not_found(format!("Local Agent `{id}` was not found")))?;
    let runtime_id = state.history.runtime_id().to_string();
    let active = state
        .history
        .active_process_counts()
        .get(&id)
        .copied()
        .unwrap_or(0);
    Ok(Json(ApiAgentDetail {
        schema_version: schema_version(),
        runtime_id: runtime_id.clone(),
        agent: ApiAgent::from_record(record, &runtime_id, active),
    }))
}

#[derive(Debug, Deserialize)]
struct InvocationListQuery {
    last: Option<usize>,
    since_ms: Option<i64>,
    recovery_since_ms: Option<i64>,
    agent_id: Option<String>,
    workdir: Option<String>,
}

async fn invocations(
    State(state): State<Arc<ApiState>>,
    Query(query): Query<InvocationListQuery>,
) -> Result<Json<ApiInvocationList>, ApiFailure> {
    let last = query.last.unwrap_or(DEFAULT_INVOCATION_LIMIT);
    if last == 0 || last > MAX_INVOCATION_LIMIT {
        return Err(bad_request(format!(
            "last must be between 1 and {MAX_INVOCATION_LIMIT}"
        )));
    }
    if let Some(agent_id) = query.agent_id.as_deref() {
        validate_agent_id(agent_id)?;
    }
    let normalized_workdir = query
        .workdir
        .as_deref()
        .map(|workdir| {
            if !Path::new(workdir).is_absolute() {
                return Err(bad_request("workdir must be an absolute path"));
            }
            normalize_declared_workdir(workdir)
                .ok_or_else(|| bad_request("workdir could not be normalized"))
        })
        .transpose()?;
    let path = state.history.database_path().to_path_buf();
    let active_process_invocation_ids = if query.recovery_since_ms.is_some() {
        state.history.active_process_invocation_ids()
    } else {
        Vec::new()
    };
    let history_query = HistoryQuery {
        last,
        since_ms: query.since_ms,
        active_or_changed_since_ms: query.recovery_since_ms,
        active_process_invocation_ids,
        agent_id: query.agent_id,
        normalized_workdir,
        invocation_id: None,
        include_raw: false,
    };
    let (records, presentation) = run_blocking(move || {
        let records = LocalHistoryReader::query(&path, &history_query)?;
        let agents = LocalHistoryReader::agent_summaries(&path, &records)?;
        let presentation = build_presentation(&records, &agents);
        Ok((records, presentation))
    })
    .await?;
    Ok(Json(ApiInvocationList {
        schema_version: schema_version(),
        presentation_version: PRESENTATION_SCHEMA_VERSION,
        runtime_id: state.history.runtime_id().to_string(),
        invocations: records
            .into_iter()
            .map(ApiLogicalInvocation::from)
            .collect(),
        presentation,
    }))
}

async fn invocation_detail(
    State(state): State<Arc<ApiState>>,
    AxumPath(id): AxumPath<i64>,
) -> Result<Json<ApiInvocationDetail>, ApiFailure> {
    if id <= 0 {
        return Err(not_found("invocation was not found"));
    }
    let path = state.history.database_path().to_path_buf();
    let (record, presentation, output) = run_blocking(move || {
        let mut records = LocalHistoryReader::query(
            &path,
            &HistoryQuery {
                last: 1,
                invocation_id: Some(id),
                ..HistoryQuery::default()
            },
        )?;
        let Some(record) = records.pop() else {
            return Ok(None);
        };
        let agents = LocalHistoryReader::agent_summaries(&path, std::slice::from_ref(&record))?;
        let presentation = build_presentation(std::slice::from_ref(&record), &agents);
        let output = LocalHistoryReader::output_metadata(&path, id)?
            .context("Local invocation disappeared while reading output metadata")?;
        Ok(Some((record, presentation, output)))
    })
    .await?
    .ok_or_else(|| not_found(format!("invocation {id} was not found")))?;
    Ok(Json(ApiInvocationDetail {
        schema_version: schema_version(),
        presentation_version: PRESENTATION_SCHEMA_VERSION,
        runtime_id: state.history.runtime_id().to_string(),
        invocation: ApiLogicalInvocation::from(record),
        presentation,
        output,
    }))
}

#[derive(Debug, Deserialize)]
struct OutputQuery {
    cursor: Option<u64>,
    limit: Option<usize>,
}

async fn invocation_output(
    State(state): State<Arc<ApiState>>,
    AxumPath(id): AxumPath<i64>,
    Query(query): Query<OutputQuery>,
) -> Result<Json<ApiOutputPage>, ApiFailure> {
    let limit = query.limit.unwrap_or(DEFAULT_OUTPUT_CHUNK_LIMIT);
    if limit == 0 || limit > MAX_OUTPUT_CHUNK_LIMIT {
        return Err(bad_request(format!(
            "limit must be between 1 and {MAX_OUTPUT_CHUNK_LIMIT}"
        )));
    }
    let cursor = query.cursor.unwrap_or(0);
    let path = state.history.database_path().to_path_buf();
    let page = run_blocking(move || LocalHistoryReader::output_page(&path, id, cursor, limit))
        .await?
        .ok_or_else(|| not_found(format!("invocation {id} was not found")))?;
    Ok(Json(ApiOutputPage {
        schema_version: schema_version(),
        runtime_id: state.history.runtime_id().to_string(),
        invocation_id: id,
        chunks: page.chunks,
        next_cursor: page.next_cursor,
    }))
}

#[derive(Debug, Deserialize)]
struct EventQuery {
    agent_id: Option<String>,
}

async fn events(
    State(state): State<Arc<ApiState>>,
    Query(query): Query<EventQuery>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, ApiFailure> {
    if let Some(agent_id) = query.agent_id.as_deref() {
        validate_agent_id(agent_id)?;
    }
    let runtime_id = state.history.runtime_id().to_string();
    let (sequence, receiver) = state.history.subscribe_live_events();
    let mut last_global_sequence = sequence;
    let agent_filter = query.agent_id;
    let stream = BroadcastStream::new(receiver).filter_map(move |result| match result {
        Ok(event) => {
            last_global_sequence = event.sequence;
            if agent_filter
                .as_deref()
                .is_some_and(|id| event.agent_id.as_deref() != Some(id))
            {
                return None;
            }
            Some(Ok(sse_event(&event)))
        }
        Err(BroadcastStreamRecvError::Lagged(skipped)) => {
            last_global_sequence = last_global_sequence.saturating_add(skipped);
            let through_sequence = last_global_sequence;
            let gap = json!({
                "schema_version": HISTORY_LIVE_EVENT_SCHEMA_VERSION,
                "runtime_id": runtime_id,
                "sequence": through_sequence,
                "emitted_at_ms": current_time_ms(),
                "event_type": "gap",
                "agent_id": agent_filter,
                "invocation_id": null,
                "presentation_revision": null,
                "payload": {
                    "skipped_events": skipped,
                    "recovery": "durable_history_or_invocation_detail"
                }
            });
            Some(Ok(Event::default()
                .id(through_sequence.to_string())
                .event("gap")
                .data(gap.to_string())))
        }
    });
    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keepalive"),
    ))
}

fn sse_event(event: &HistoryLiveEvent) -> Event {
    Event::default()
        .id(event.sequence.to_string())
        .event(event.event_type.clone())
        .data(serde_json::to_string(event).expect("Local live event must serialize"))
}

async fn bearer_auth_no_store(
    State(auth): State<AuthState>,
    request: Request,
    next: Next,
) -> Response {
    let authorized = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .and_then(|value| HeaderValue::from_str(value).ok())
        .is_some_and(|value| value == auth.token);
    let mut response = if authorized {
        next.run(request).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(ApiError {
                schema_version: schema_version(),
                error: "unauthorized".to_string(),
            }),
        )
            .into_response()
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn validate_bearer(token: Arc<str>) -> Result<HeaderValue> {
    if token.trim().len() < 32 {
        bail!("Zodex Local observability bearer must contain at least 32 characters");
    }
    HeaderValue::from_str(token.trim())
        .context("Zodex Local observability bearer is not a valid HTTP header value")
}

fn validate_agent_id(value: &str) -> Result<(), ApiFailure> {
    if value.len() == 4
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    {
        return Ok(());
    }
    Err(bad_request("agent_id must match [a-z0-9]{4}"))
}

type ApiFailure = (StatusCode, Json<ApiError>);

fn bad_request(message: impl Into<String>) -> ApiFailure {
    failure(StatusCode::BAD_REQUEST, message)
}

fn not_found(message: impl Into<String>) -> ApiFailure {
    failure(StatusCode::NOT_FOUND, message)
}

fn failure(status: StatusCode, message: impl Into<String>) -> ApiFailure {
    (
        status,
        Json(ApiError {
            schema_version: schema_version(),
            error: message.into(),
        }),
    )
}

async fn run_blocking<T: Send + 'static>(
    operation: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> Result<T, ApiFailure> {
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| {
            failure(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("read task failed: {error}"),
            )
        })?
        .map_err(|error| failure(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}

fn current_time_ms() -> i64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    i64::try_from(millis).unwrap_or(i64::MAX)
}
