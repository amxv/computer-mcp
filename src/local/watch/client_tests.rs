use std::collections::HashMap;
use std::fs;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, Method, Request, Response, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::super::{LocalObservabilityDiscovery, LocalPaths, LocalRuntimeDiscovery};
use super::client::{NETWORK_EVENT_CAPACITY, ObserverClient, WatchNetworkEvent};
use super::model::{ConnectionState, WatchApp, WatchEffect, WatchOptions, WatchScope};
use super::test_support::{RUNTIME_ID, agent, bootstrap, command_detail};
use super::{RestoreGuard, Subscription, apply_live_update, handle_network_event, valid_agent_id};
use crate::local::history::HistoryLiveEvent;
use crate::local::observability::{
    ApiAgentList, ApiInvocationDetail, ApiInvocationList, ApiStatusDocument,
};
use crate::local::{PRESENTATION_SCHEMA_VERSION, PresentationDocument};

const BEARER: &str = "watch-test-bearer-0123456789abcdef0123456789abcdef";
type RecordedRequest = (Method, String, Option<String>);

#[test]
fn terminal_restore_guard_runs_during_unwind_and_can_be_disarmed() {
    use std::cell::Cell;

    let restored = Cell::new(0_u32);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = RestoreGuard::new(|| restored.set(restored.get() + 1));
        panic!("exercise terminal restoration guard");
    }));
    assert!(result.is_err());
    assert_eq!(restored.get(), 1);

    {
        let mut guard = RestoreGuard::new(|| restored.set(restored.get() + 1));
        guard.disarm();
    }
    assert_eq!(restored.get(), 1);
}

#[test]
fn dedicated_watch_agent_ids_use_the_public_four_character_shape() {
    assert!(valid_agent_id("k7m2"));
    assert!(!valid_agent_id("K7M2"));
    assert!(!valid_agent_id("abc"));
    assert!(!valid_agent_id("ab\u{1b}c"));
}

#[derive(Clone)]
struct FakeState {
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
}

struct FakeObserver {
    _dir: TempDir,
    paths: LocalPaths,
    state: FakeState,
    task: tokio::task::JoinHandle<()>,
}

impl FakeObserver {
    async fn start() -> Self {
        crate::install_rustls_crypto_provider();
        let dir = tempfile::tempdir().unwrap();
        let paths = LocalPaths::from_roots(
            dir.path().join("config"),
            dir.path().join("data"),
            dir.path().join("state"),
        )
        .unwrap();
        paths.ensure_persistent_dirs().unwrap();
        fs::write(paths.observability_bearer_file(), format!("{BEARER}\n")).unwrap();
        fs::create_dir_all(paths.runtime_dir()).unwrap();

        let state = FakeState {
            requests: Arc::new(Mutex::new(Vec::new())),
        };
        let app = Router::new()
            .route("/v1/status", get(fake_status))
            .route("/v1/agents", get(fake_agents))
            .route("/v1/invocations", get(fake_invocations))
            .route("/v1/invocations/{id}", get(fake_invocation))
            .route("/v1/events", get(fake_events))
            .with_state(state.clone())
            .layer(middleware::from_fn_with_state(
                state.clone(),
                record_request,
            ));
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let discovery = LocalRuntimeDiscovery {
            schema_version: crate::local::LOCAL_DISCOVERY_SCHEMA_VERSION,
            runtime_id: RUNTIME_ID.to_owned(),
            pid: std::process::id(),
            start_directory: dir.path().join("repo"),
            started_at: "2026-08-16T00:00:00Z".to_owned(),
            expires_at: Some("2026-08-17T00:00:00Z".to_owned()),
            observability: LocalObservabilityDiscovery::active(
                format!("http://{address}"),
                paths.observability_bearer_file(),
            ),
        };
        fs::write(
            paths.discovery_file(),
            serde_json::to_vec_pretty(&discovery).unwrap(),
        )
        .unwrap();

        Self {
            _dir: dir,
            paths,
            state,
            task,
        }
    }

    fn requests(&self) -> Vec<(Method, String, Option<String>)> {
        self.state.requests.lock().unwrap().clone()
    }

    fn clear_requests(&self) {
        self.state.requests.lock().unwrap().clear();
    }
}

impl Drop for FakeObserver {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn record_request(
    State(state): State<FakeState>,
    request: Request<Body>,
    next: Next,
) -> Response<Body> {
    let method = request.method().clone();
    let uri = request.uri().to_string();
    let auth = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    state.requests.lock().unwrap().push((method, uri, auth));
    next.run(request).await
}

async fn fake_status(State(_): State<FakeState>) -> Json<ApiStatusDocument> {
    Json(bootstrap(vec![agent("k7m2", &["/one"]), agent("m4n8", &["/two"])]).status)
}

async fn fake_agents(State(_): State<FakeState>) -> Json<ApiAgentList> {
    Json(ApiAgentList {
        schema_version: crate::local::LOCAL_OBSERVABILITY_API_VERSION,
        runtime_id: RUNTIME_ID.to_owned(),
        agents: vec![agent("k7m2", &["/one"]), agent("m4n8", &["/two"])],
    })
}

async fn fake_invocation(Path(id): Path<i64>) -> Json<ApiInvocationDetail> {
    let agent_id = if id == 1 { "k7m2" } else { "m4n8" };
    Json(command_detail(
        id,
        Some(agent_id),
        &format!("echo invocation-{id}"),
        "running",
        None,
    ))
}

async fn fake_invocations(Query(query): Query<HashMap<String, String>>) -> Json<ApiInvocationList> {
    let agent_id = query.get("agent_id").map(String::as_str);
    let ids: &[i64] = match agent_id {
        Some("m4n8") => &[2, 3],
        Some("k7m2") => &[1],
        _ => &[1, 2, 3],
    };
    Json(ApiInvocationList {
        schema_version: crate::local::LOCAL_OBSERVABILITY_API_VERSION,
        presentation_version: PRESENTATION_SCHEMA_VERSION,
        runtime_id: RUNTIME_ID.to_owned(),
        invocations: ids
            .iter()
            .map(|id| {
                let agent_id = if *id == 1 { "k7m2" } else { "m4n8" };
                command_detail(
                    *id,
                    Some(agent_id),
                    &format!("echo invocation-{id}"),
                    "running",
                    None,
                )
                .invocation
            })
            .collect(),
        presentation: PresentationDocument {
            schema_version: PRESENTATION_SCHEMA_VERSION,
            agents: Vec::new(),
            records: Vec::new(),
        },
    })
}

async fn fake_events(Query(query): Query<HashMap<String, String>>) -> impl IntoResponse {
    let agent_id = query
        .get("agent_id")
        .cloned()
        .unwrap_or_else(|| "k7m2".to_owned());
    let invocation_id = if agent_id == "m4n8" { 2 } else { 1 };
    let event = HistoryLiveEvent {
        schema_version: crate::local::history::HISTORY_LIVE_EVENT_SCHEMA_VERSION,
        runtime_id: RUNTIME_ID.to_owned(),
        sequence: invocation_id as u64,
        emitted_at_ms: 1_000,
        event_type: "invocation_started".to_owned(),
        agent_id: Some(agent_id),
        invocation_id: Some(invocation_id),
        presentation_revision: Some(1),
        payload: json!({}),
    };
    let body = format!(
        "id: {}\nevent: invocation_started\ndata: {}\n\n",
        event.sequence,
        serde_json::to_string(&event).unwrap()
    );
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    (StatusCode::OK, headers, body)
}

#[tokio::test]
async fn discovery_bootstrap_is_get_only_and_does_not_preload_history() {
    let fake = FakeObserver::start().await;
    let (_client, bootstrap) = ObserverClient::discover(&fake.paths).await.unwrap();
    assert_eq!(bootstrap.agents.len(), 2);

    let requests = fake.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].0, Method::GET);
    assert_eq!(requests[0].1, "/v1/status");
    assert_eq!(requests[1].0, Method::GET);
    assert!(requests[1].1.starts_with("/v1/agents"));
    assert!(
        requests
            .iter()
            .all(|(_, path, _)| !path.contains("invocations"))
    );
    assert!(requests.iter().all(|(method, _, _)| *method == Method::GET));
    let expected_auth = format!("Bearer {BEARER}");
    assert!(
        requests
            .iter()
            .all(|(_, _, auth)| auth.as_deref() == Some(expected_auth.as_str()))
    );
}

#[tokio::test]
async fn initial_sse_connection_refreshes_stale_agent_snapshot_without_history_preload() {
    let fake = FakeObserver::start().await;
    let (client, _) = ObserverClient::discover(&fake.paths).await.unwrap();
    let mut app = WatchApp::new(&bootstrap(Vec::new()), WatchOptions::automatic());
    let (network_tx, _network_rx) = mpsc::channel(NETWORK_EVENT_CAPACITY);
    let mut generation = 1_u64;
    let mut subscription =
        Subscription::start(&client, generation, app.stream_filter(), network_tx.clone());
    fake.clear_requests();

    handle_network_event(
        WatchNetworkEvent::Connected(generation),
        generation,
        &client,
        &mut app,
        &network_tx,
        &mut subscription,
        &mut generation,
    )
    .await
    .unwrap();

    assert_eq!(app.scope, WatchScope::Picker);
    assert_eq!(generation, 1, "global Waiting->picker needs no resubscribe");
    let requests = fake.requests();
    assert!(
        requests
            .iter()
            .any(|(_, path, _)| path.starts_with("/v1/agents"))
    );
    assert!(requests.iter().any(|(_, path, _)| path == "/v1/status"));
    assert!(
        requests
            .iter()
            .all(|(_, path, _)| !path.starts_with("/v1/invocations")),
        "the first connection refreshes runtime facts but must not preload invocation history"
    );
}

#[tokio::test]
async fn subscription_handover_accepts_draining_generation_until_new_stream_connects() {
    let fake = FakeObserver::start().await;
    let (client, _) = ObserverClient::discover(&fake.paths).await.unwrap();
    let (network_tx, _network_rx) = mpsc::channel(NETWORK_EVENT_CAPACITY);
    let mut subscription = Subscription::start(&client, 1, None, network_tx.clone());

    subscription.replace(&client, 2, Some("k7m2".to_owned()), network_tx);
    assert!(subscription.accepts_generation(1));
    assert!(subscription.accepts_generation(2));

    subscription.mark_connected(2);
    assert!(
        subscription.accepts_generation(1),
        "already queued events from the drained old stream remain admissible"
    );
    assert!(subscription.accepts_generation(2));
}

#[tokio::test]
async fn two_agent_filtered_viewers_receive_independent_streams_without_control_requests() {
    let fake = FakeObserver::start().await;
    let (client, _) = ObserverClient::discover(&fake.paths).await.unwrap();
    fake.clear_requests();

    let (first_tx, mut first_rx) = mpsc::channel(NETWORK_EVENT_CAPACITY);
    let first_cancel = CancellationToken::new();
    let first_task =
        client.spawn_event_stream(11, Some("k7m2".to_owned()), first_tx, first_cancel.clone());
    let (second_tx, mut second_rx) = mpsc::channel(NETWORK_EVENT_CAPACITY);
    let second_cancel = CancellationToken::new();
    let second_task = client.spawn_event_stream(
        22,
        Some("m4n8".to_owned()),
        second_tx,
        second_cancel.clone(),
    );

    let first = receive_live(&mut first_rx, 11).await;
    let second = receive_live(&mut second_rx, 22).await;
    first_cancel.cancel();
    second_cancel.cancel();
    first_task.await.unwrap();
    second_task.await.unwrap();

    assert_eq!(first.agent_id.as_deref(), Some("k7m2"));
    assert_eq!(second.agent_id.as_deref(), Some("m4n8"));
    assert_eq!(first.invocation_id, Some(1));
    assert_eq!(second.invocation_id, Some(2));

    let requests = fake.requests();
    assert!(
        requests
            .iter()
            .any(|(_, path, _)| path == "/v1/events?agent_id=k7m2")
    );
    assert!(
        requests
            .iter()
            .any(|(_, path, _)| path == "/v1/events?agent_id=m4n8")
    );
    assert!(requests.iter().all(|(method, _, _)| *method == Method::GET));
    assert!(
        requests
            .iter()
            .all(|(_, path, _)| !path.contains("exec-command"))
    );
}

#[tokio::test]
async fn first_live_reference_fetches_only_that_invocation_and_gap_recovers_missed_history() {
    let fake = FakeObserver::start().await;
    let (client, bootstrap) = ObserverClient::discover(&fake.paths).await.unwrap();
    let mut app = WatchApp::new(
        &bootstrap,
        WatchOptions {
            agent: Some("m4n8".to_owned()),
            all: false,
        },
    );
    fake.clear_requests();

    let live = HistoryLiveEvent {
        schema_version: crate::local::history::HISTORY_LIVE_EVENT_SCHEMA_VERSION,
        runtime_id: RUNTIME_ID.to_owned(),
        sequence: 2,
        emitted_at_ms: 1_000,
        event_type: "invocation_started".to_owned(),
        agent_id: Some("m4n8".to_owned()),
        invocation_id: Some(2),
        presentation_revision: Some(1),
        payload: json!({}),
    };
    let effects = apply_live_update(&client, &mut app, &live).await;
    assert!(effects.is_empty());
    assert!(app.knows_invocation(2));
    assert_eq!(app.visible_cards().len(), 1);
    let requests = fake.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].0, Method::GET);
    assert_eq!(requests[0].1, "/v1/invocations/2");

    fake.clear_requests();
    let gap = HistoryLiveEvent {
        event_type: "gap".to_owned(),
        invocation_id: None,
        sequence: 8,
        payload: json!({"skipped_events": 5}),
        ..live
    };
    let effects = apply_live_update(&client, &mut app, &gap).await;
    assert_eq!(effects, vec![WatchEffect::RefreshAgents]);
    let requests = fake.requests();
    assert!(requests.iter().all(|(method, _, _)| *method == Method::GET));
    assert!(requests.iter().any(|(_, path, _)| {
        path.starts_with("/v1/invocations?")
            && path.contains("last=100")
            && path.contains("recovery_since_ms=")
            && path.contains("agent_id=m4n8")
    }));
    assert!(
        requests
            .iter()
            .any(|(_, path, _)| path == "/v1/invocations/2")
    );
    assert!(
        requests
            .iter()
            .any(|(_, path, _)| path == "/v1/invocations/3")
    );
    assert!(app.knows_invocation(3));
    assert_eq!(app.visible_cards().len(), 2);
    assert_eq!(app.connection, ConnectionState::Connected);
    assert_eq!(
        app.recovery_notice(),
        Some("live event gap recovered from durable history")
    );
}

async fn receive_live(
    receiver: &mut mpsc::Receiver<WatchNetworkEvent>,
    generation: u64,
) -> HistoryLiveEvent {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            match receiver.recv().await.unwrap() {
                WatchNetworkEvent::Connected(value) => assert_eq!(value, generation),
                WatchNetworkEvent::Live(value, event) => {
                    assert_eq!(value, generation);
                    return event;
                }
                WatchNetworkEvent::Disconnected(_, message) => {
                    panic!("stream disconnected before live event: {message}")
                }
            }
        }
    })
    .await
    .unwrap()
}
